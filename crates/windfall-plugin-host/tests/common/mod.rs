//! Builds the test plugins and hands out copies of them.
//!
//! The plugins are a crate of their own in `test-plugins`, outside the
//! workspace. The first test that needs them runs `cargo build` on it, into
//! the directory cargo gives integration tests for scratch files. Nothing
//! is written to the source tree and nothing installed on the machine is
//! used.

#![allow(dead_code)]

#[cfg(target_os = "macos")]
pub mod macos_bundle;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use windfall_plugin_host::{
    PluginHost, PluginInstance, PluginModule, PluginNotification, ProcessRunner,
};

pub const GAIN: &str = "org.windfall.test.gain";
pub const SINE: &str = "org.windfall.test.sine";
pub const MIDI_SINE: &str = "org.windfall.test.midi-sine";
pub const SWAP: &str = "org.windfall.test.swap";
pub const SIDECHAIN: &str = "org.windfall.test.sidechain";
pub const MONO: &str = "org.windfall.test.mono";
pub const NAN: &str = "org.windfall.test.nan";
pub const PROCESS_ERROR: &str = "org.windfall.test.process-error";
pub const PROCESS_PANIC: &str = "org.windfall.test.process-panic";
pub const ABSURD_PORTS: &str = "org.windfall.test.absurd-ports";
pub const SLOW: &str = "org.windfall.test.slow";
pub const INIT_FAIL: &str = "org.windfall.test.init-fail";
pub const INIT_HANG: &str = "org.windfall.test.init-hang";
pub const INIT_CRASH: &str = "org.windfall.test.init-crash";

/// The ids of the gain plugin's parameters and readings.
pub mod gain {
    pub const GAIN: u32 = 7;
    pub const MODE: u32 = 9;
    pub const TIMER_TICKS: u32 = 20;
    pub const MAIN_THREAD_CALLS: u32 = 21;
    pub const TEMPO: u32 = 22;
    pub const PLAYING: u32 = 23;
    pub const BEATS: u32 = 24;
    pub const DENORMALS_FLUSHED: u32 = 25;
    pub const IN_PLACE: u32 = 26;
}

/// The id of the sine instrument's level, and its latency in samples.
pub const SINE_LEVEL: u32 = 1;
pub const SINE_LATENCY: usize = 32;

/// Builds the test plugin library once per test program and returns it.
fn built_library() -> &'static Path {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        if let Some(path) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "explicit prebuilt fixture must exist");
            return path;
        }
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-plugins/Cargo.toml");
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("windfall-test-plugins");
        let output = Command::new(env!("CARGO"))
            .arg("build")
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target)
            .output()
            .expect("cargo can be run");
        assert!(
            output.status.success(),
            "the test plugins did not build:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let name = format!(
            "{}windfall_test_plugins{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let library = target.join("debug").join(name);
        assert!(library.is_file(), "{} was not built", library.display());
        library
    })
}

/// A folder of this test program's own, emptied when the program starts.
/// Each program needs its own because Windows will not overwrite a library
/// another process has loaded.
pub fn scratch() -> &'static Path {
    static FOLDER: OnceLock<PathBuf> = OnceLock::new();
    FOLDER.get_or_init(|| {
        let program = std::env::current_exe().expect("a test program has a path");
        let name = program
            .file_stem()
            .expect("a test program has a name")
            .to_string_lossy()
            .into_owned();
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("scratch-{name}"));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("the scratch folder can be made");
        folder
    })
}

/// A copy of the test plugin library under `name` in a folder of its own
/// inside [`scratch`]. The name decides how the library behaves when it is
/// loaded: see the test plugins' crate documentation.
pub fn plugin_file(folder: &str, name: &str) -> PathBuf {
    // Parallel tests share names. Publish the copy before any can load it;
    // Windows refuses to overwrite a DLL once another test has loaded it.
    static COPY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _copy = COPY.lock().expect("the fixture copy lock is intact");
    let folder = scratch().join(folder);
    // Mac tickets keep their live lifecycle owner. Parallel test owners use
    // independent complete bundles; callers still share a path on one owner.
    #[cfg(target_os = "macos")]
    let folder = if name.ends_with(".vst3") {
        folder.join(format!("mac-owner-{:?}", std::thread::current().id()))
    } else {
        folder
    };
    std::fs::create_dir_all(&folder).expect("the plugin folder can be made");
    let file = folder.join(name);
    if !file.exists() {
        #[cfg(target_os = "macos")]
        if name.ends_with(".vst3") {
            macos_bundle::publish(&file, built_library(), "windfall-fixture");
            return file;
        }
        std::fs::copy(built_library(), &file).expect("the plugin can be copied");
    }
    file
}

/// The well-behaved copy that most tests load.
pub fn test_plugins() -> PathBuf {
    plugin_file("good", "windfall-test.clap")
}

/// A host with the test plugins loaded.
pub fn load() -> (PluginHost, PluginModule) {
    let host = PluginHost::windfall();
    let module = host.load(&test_plugins()).expect("the test plugins load");
    (host, module)
}

/// Creates one of the test plugins. The module is returned too, because
/// the plugin file must stay loaded as long as the plugin exists.
pub fn create(id: &str) -> (PluginModule, PluginInstance) {
    let (_, module) = load();
    let instance = module.create(id).expect("the test plugin can be created");
    (module, instance)
}

/// Everything `idle` reports, as a list.
pub fn idle(instance: &mut PluginInstance) -> Vec<PluginNotification> {
    let mut notifications = Vec::new();
    instance.idle(&mut |notification| notifications.push(notification));
    notifications
}

/// The real scanner program, with a patience short enough for tests.
pub fn scanner(timeout: Duration) -> ProcessRunner {
    let mut runner = ProcessRunner::new(env!("CARGO_BIN_EXE_windfall-plugin-scan"));
    runner.timeout = timeout;
    runner
}

/// The real check program.
pub fn check_program() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_windfall-plugin-check"))
}

/// A block of a 1 kHz-ish test signal that is different on the two sides.
pub fn signal(frames: usize) -> (Vec<f32>, Vec<f32>) {
    let left = (0..frames)
        .map(|frame| (frame as f32 * 0.1).sin() * 0.5)
        .collect();
    let right = (0..frames)
        .map(|frame| (frame as f32 * 0.07).cos() * 0.25)
        .collect();
    (left, right)
}
