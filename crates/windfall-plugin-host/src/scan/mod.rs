//! Finding out what a plugin file holds without trusting it.
//!
//! Loading a plugin runs its code, and plugin code crashes, hangs and shows
//! dialogs. So the app never loads a file to look at it. It starts the
//! `windfall-plugin-scan` program on one file, reads what that prints, and
//! gives up on it when it dies or goes quiet. A bad plugin costs the scanner
//! process and nothing else.
//!
//! One file can hold many plugins, and one of them hanging must not hide the
//! others. The scanner says which plugin it is about to create before it
//! does. When it dies there, [`scan_file`] blames that plugin and runs the
//! scanner again with the plugin left out.

mod cache;
pub mod probe;
mod protocol;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub use cache::{BlockedPlugin, PluginCatalog, RefreshSummary};
pub use protocol::ScanLine;

use crate::descriptor::{PluginDescriptor, PluginFormat, PluginLayout};

/// Why a file or a plugin is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The scanner process died while loading it.
    Crashed,
    /// The scanner process stopped answering while loading it.
    TimedOut,
    /// It loaded and said no, or described itself in a way the host cannot
    /// use.
    Rejected,
}

/// A failure, with words for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFailure {
    pub kind: FailureKind,
    pub message: String,
}

impl ScanFailure {
    /// True for the failures that could take the app down with them. These
    /// put a plugin on the blocklist.
    pub fn is_dangerous(&self) -> bool {
        matches!(self.kind, FailureKind::Crashed | FailureKind::TimedOut)
    }
}

/// One plugin of a scanned file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedPlugin {
    pub descriptor: PluginDescriptor,
    /// Its ports and what else it said about itself, if it could be created.
    pub layout: Option<PluginLayout>,
    /// Why it cannot be used, if it cannot.
    pub failure: Option<ScanFailure>,
}

impl ScannedPlugin {
    pub fn is_usable(&self) -> bool {
        self.failure.is_none() && self.layout.is_some()
    }
}

/// Everything a scan found out about one file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileScan {
    pub plugins: Vec<ScannedPlugin>,
    /// Why the file as a whole could not be read, if it could not.
    pub failure: Option<ScanFailure>,
}

/// What to scan.
#[derive(Debug, Clone, Copy)]
pub struct ScanRequest<'a> {
    pub path: &'a Path,
    /// Plugin ids to list but not create.
    pub skip: &'a [String],
}

/// How one run of the scanner ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// The process ended, with this exit code if the system gave one.
    Exited(Option<i32>),
    /// The process printed nothing for too long and was killed.
    TimedOut,
}

/// What one run of the scanner printed, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub lines: Vec<ScanLine>,
    pub end: RunEnd,
}

/// The scanner could not be run at all. This says nothing about the plugin,
/// so it is never cached.
#[derive(Debug, thiserror::Error)]
#[error("{}", windfall_core::notice!("pluginHost.scannerUnavailable", detail = windfall_core::notice::term(_0)))]
pub struct ScannerUnavailable(pub String);

/// Something that can run the scanner on a file. The real one starts a
/// process. Tests pass one that returns prepared output.
pub trait ScanRunner {
    fn run(&self, request: ScanRequest<'_>) -> Result<RunOutput, ScannerUnavailable>;
}

/// Runs the `windfall-plugin-scan` program.
#[derive(Debug, Clone)]
pub struct ProcessRunner {
    /// The scanner program.
    pub program: PathBuf,
    /// How long the scanner may go without printing a line. Each step of a
    /// scan prints one, so this is the time one plugin gets to load.
    pub timeout: Duration,
}

/// How long a scanner that has finished its work gets to exit. Some plugins
/// hang while unloading.
const EXIT_GRACE: Duration = Duration::from_secs(2);

impl ProcessRunner {
    /// A runner with the default patience of 30 seconds per step. Plugins
    /// that check a license or build a sample index take a while.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: Duration::from_secs(30),
        }
    }

    /// The scanner that sits next to the running program, which is where
    /// the app's installer puts it.
    pub fn beside_current_exe() -> Option<Self> {
        let name = format!("windfall-plugin-scan{}", std::env::consts::EXE_SUFFIX);
        let program = std::env::current_exe().ok()?.with_file_name(name);
        program.is_file().then(|| Self::new(program))
    }
}

impl ScanRunner for ProcessRunner {
    fn run(&self, request: ScanRequest<'_>) -> Result<RunOutput, ScannerUnavailable> {
        let mut command = Command::new(&self.program);
        command.arg(request.path);
        for id in request.skip {
            command.arg("--skip").arg(id);
        }
        let mut lines = Vec::new();
        let end = run_reporting(command, self.timeout, &mut |line| {
            let line = ScanLine::parse(line)?;
            let done = line == ScanLine::Done;
            lines.push(line);
            Some(done)
        })?;
        Ok(RunOutput { lines, end })
    }
}

/// Runs a helper program that reports its progress one line at a time.
///
/// `on_line` returns `Some(false)` for progress, `Some(true)` for completion,
/// and `None` for plugin chatter. Only progress extends the deadline.
pub(crate) fn run_reporting(
    mut command: Command,
    timeout: Duration,
    on_line: &mut dyn FnMut(&str) -> Option<bool>,
) -> Result<RunEnd, ScannerUnavailable> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|error| {
        let program = command.get_program().to_string_lossy().into_owned();
        ScannerUnavailable(format!("{program}: {error}"))
    })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let (sender, receiver) = mpsc::sync_channel(32);
    // The reader is never joined. A plugin may have started a process of
    // its own that keeps the pipe open after the helper is gone, and the
    // thread ends by itself once the pipe closes or the receiver is
    // dropped.
    std::thread::spawn(move || {
        // Bytes, not strings: a plugin may print anything.
        // Bound both retained lines and a plugin's unterminated output.
        for line in BufReader::new(stdout)
            .take(16 * 1024 * 1024)
            .split(b'\n')
            .map_while(Result::ok)
        {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    let mut deadline = Instant::now() + timeout;
    loop {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => match on_line(&String::from_utf8_lossy(&line)) {
                Some(true) => break,
                Some(false) => deadline = Instant::now() + timeout,
                None => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(RunEnd::TimedOut);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(RunEnd::Exited(wait_briefly(&mut child)))
}

/// Waits for a process that should be about to exit, and kills it if it
/// does not.
fn wait_briefly(child: &mut std::process::Child) -> Option<i32> {
    let deadline = Instant::now() + EXIT_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: the app is a window program, and a console would
    // flash up for every file.
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

pub(crate) fn exit_words(code: Option<i32>) -> String {
    match code {
        // Windows reports crashes as large codes such as 0xC0000005.
        Some(code) => windfall_core::notice!(
            "pluginHost.helperExit",
            code = format!("{:#x}", code as u32)
        )
        .to_string(),
        None => "the helper process was ended by a signal".to_owned(),
    }
}

/// Scans one plugin file with `runner`, running it again as often as it
/// takes to get past plugins that crash or hang.
///
/// The result always lists every plugin the file named, in the file's
/// order, each with its layout or the reason it has none.
pub fn scan_file(runner: &dyn ScanRunner, path: &Path) -> Result<FileScan, ScannerUnavailable> {
    let mut descriptors: Option<Vec<PluginDescriptor>> = None;
    let mut layouts: Vec<(String, PluginLayout)> = Vec::new();
    let mut failures: Vec<(String, ScanFailure)> = Vec::new();
    let mut skip: Vec<String> = Vec::new();

    loop {
        let output = runner.run(ScanRequest { path, skip: &skip })?;
        let mut probing: Option<String> = None;
        let mut done = false;
        for line in output.lines {
            match line {
                ScanLine::File { plugins, .. } => {
                    descriptors.get_or_insert(plugins);
                }
                ScanLine::Failed { message } => {
                    return Ok(FileScan {
                        plugins: Vec::new(),
                        failure: Some(ScanFailure {
                            kind: FailureKind::Rejected,
                            message,
                        }),
                    });
                }
                ScanLine::Probing { id } => probing = Some(id),
                ScanLine::Probed { id, layout } => {
                    probing = None;
                    layouts.push((id, layout));
                }
                ScanLine::ProbeFailed { id, message } => {
                    probing = None;
                    let failure = ScanFailure {
                        kind: FailureKind::Rejected,
                        message,
                    };
                    failures.push((id, failure));
                }
                ScanLine::Done => done = true,
            }
        }
        if done {
            break;
        }

        let (kind, message) = match output.end {
            RunEnd::TimedOut => (
                FailureKind::TimedOut,
                "the plugin did not answer in time".to_owned(),
            ),
            RunEnd::Exited(code) => (FailureKind::Crashed, exit_words(code)),
        };
        let failure = ScanFailure { kind, message };
        if descriptors.is_none() {
            return Ok(FileScan {
                plugins: Vec::new(),
                failure: Some(failure),
            });
        }
        // Without a plugin to blame, or with one that is being skipped
        // already, another run would end the same way.
        match probing {
            Some(id) if !skip.contains(&id) => {
                failures.push((id.clone(), failure));
                skip.push(id);
            }
            _ => break,
        }
    }

    let plugins = descriptors
        .unwrap_or_default()
        .into_iter()
        .map(|descriptor| {
            let layout = layouts
                .iter()
                .find(|(id, _)| *id == descriptor.id)
                .map(|(_, layout)| layout.clone());
            let failure = failures
                .iter()
                .find(|(id, _)| *id == descriptor.id)
                .map(|(_, failure)| failure.clone())
                .or_else(|| {
                    layout.is_none().then(|| ScanFailure {
                        kind: FailureKind::Crashed,
                        message: "the scanner ended before it reached this plugin".to_owned(),
                    })
                });
            ScannedPlugin {
                descriptor,
                layout,
                failure,
            }
        })
        .collect();
    Ok(FileScan {
        plugins,
        failure: None,
    })
}

/// Checks that a file's format is one the host scans.
pub(crate) fn format_of(path: &Path) -> Result<PluginFormat, String> {
    PluginFormat::of(path).ok_or_else(|| {
        windfall_core::notice!("pluginHost.notAPluginFile", path = path.display()).to_string()
    })
}
