//! The `.windfall` file format: one pretty-printed JSON file holding a
//! [`Project`], next to the samples it owns and a `Backup` folder.
//!
//! # Beside the project
//!
//! A file may hold one thing that is not the project: `session`, a
//! [`ProjectSession`], which says how the project was being played when it
//! was saved. It is a key of its own in the file's JSON, after the
//! project's, and [`Project`] has no field for it. So it is not an edit:
//! it has no place in the undo history, a project is not changed by it,
//! and two projects that differ in nothing else are equal. A file without
//! it, such as every file from before it existed, loads the same project.
//!
//! # Backups and samples
//!
//! A backup is written exactly as a save would be, so the two can be
//! compared byte for byte. Its [`SamplePath::Project`] paths therefore stay
//! relative to the project folder, which is the folder that holds `Backup`,
//! and not to `Backup` itself. [`backup_origin`] tells a backup from a
//! project file, and [`sample_dir`] gives the folder to resolve either
//! one's project samples against.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use ts_rs::TS;

use crate::model::{
    FORMAT_VERSION, PatternId, PlayMode, Project, SamplePath, relative_path_problem,
};

/// File extension of a project, without the dot.
pub const FILE_EXTENSION: &str = "windfall";

/// Name of the folder, beside the project file, that holds its backups.
pub const BACKUP_FOLDER: &str = "Backup";

/// How many backups of a project [`write_backup`] usually keeps.
pub const DEFAULT_BACKUP_COUNT: usize = 20;

/// Why a project could not be loaded.
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("could not read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("this is not a Windfall project: {0}")]
    Parse(#[from] serde_json::Error),
    #[error(
        "this project was saved by a newer version of Windfall (file format {found}; this version reads up to format {supported})"
    )]
    TooNew { found: u32, supported: u32 },
    #[error("the project file is damaged: {0}")]
    Invalid(String),
}

/// Why a project could not be saved.
#[derive(Debug, Error)]
pub enum SaveError {
    #[error("could not write {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not encode the project: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("{0}")]
    BadName(String),
    #[error("the project breaks a rule of the file format and was not saved: {0}")]
    Invalid(String),
}

/// How a project was being played when it was saved, kept in its file so
/// that it opens the way it was left: a song opens in song mode.
///
/// These are the transport's settings and not the project's. Changing one
/// is not an edit, cannot be undone and does not make the project unsaved;
/// whoever saves reads them off the transport, and whoever opens hands
/// them back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSession {
    pub mode: PlayMode,
    /// In song mode, whether the song starts over at its end.
    #[serde(default)]
    pub loop_song: bool,
    /// The pattern that plays in pattern mode. An id the project does not
    /// have is for the reader to ignore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pattern: Option<PatternId>,
}

/// Encodes a project as the text of a `.windfall` file.
pub fn to_json(project: &Project) -> Result<String, serde_json::Error> {
    to_json_with(project, None)
}

/// Encodes a project as the text of a `.windfall` file that also holds how
/// it was being played. With no session the text is what [`to_json`]
/// gives; with one, it is the same text with a `session` key at its end.
pub fn to_json_with(
    project: &Project,
    session: Option<&ProjectSession>,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct WithSession<'a> {
        #[serde(flatten)]
        project: &'a Project,
        session: &'a ProjectSession,
    }

    let mut json = match session {
        Some(session) => serde_json::to_string_pretty(&WithSession { project, session })?,
        None => serde_json::to_string_pretty(project)?,
    };
    json.push('\n');
    Ok(json)
}

/// The session the text of a `.windfall` file holds beside its project.
/// `None` when it holds none, and also when what it holds cannot be read,
/// as a session written by a later version might not be: a project must
/// not fail to open over how it was last played.
pub fn session_from_json(json: &str) -> Option<ProjectSession> {
    #[derive(Deserialize)]
    struct Beside {
        session: Option<ProjectSession>,
    }

    serde_json::from_str::<Beside>(json).ok()?.session
}

/// Decodes the text of a `.windfall` file. Files in an older format are
/// upgraded, files in a newer format are refused, and the result is checked
/// with [`Project::check`].
pub fn from_json(json: &str) -> Result<Project, LoadError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        format_version: u32,
    }

    let found = serde_json::from_str::<Header>(json)?.format_version;
    if found > FORMAT_VERSION {
        return Err(LoadError::TooNew {
            found,
            supported: FORMAT_VERSION,
        });
    }
    let mut project: Project = if found == FORMAT_VERSION {
        serde_json::from_str(json)?
    } else {
        let mut value: Value = serde_json::from_str(json)?;
        migrate(&mut value, found, MIGRATIONS)?;
        serde_json::from_value(value)?
    };
    if project.settings.scripts.parameter_addresses.is_empty() {
        project
            .settings
            .scripts
            .register_parameters(&crate::scripting::ScriptLibrary::default())
            .map_err(LoadError::Invalid)?;
    }
    project.check().map_err(LoadError::Invalid)?;
    Ok(project)
}

/// Writes a project to `path`, replacing any file there. The text goes to a
/// temporary file in the same folder first and is then renamed into place,
/// so a crash cannot leave half a file. Missing folders are created.
///
/// The project is checked with [`Project::check`] first, and one that
/// breaks a rule is refused with nothing written: a file that would not
/// load again is worse than no file.
///
/// The file is written with [`write_atomic`], which says what that promises
/// when several writes or another program meet at one path. Afterwards the
/// folder is cleared of temporary files that writes cut short by a crash
/// left behind more than a day ago.
pub fn save(project: &Project, path: impl AsRef<Path>) -> Result<(), SaveError> {
    save_with(project, None, path)
}

/// [`save`], with how the project is being played written beside it.
pub fn save_with(
    project: &Project,
    session: Option<&ProjectSession>,
    path: impl AsRef<Path>,
) -> Result<(), SaveError> {
    let path = path.as_ref();
    project.check().map_err(SaveError::Invalid)?;
    let json = to_json_with(project, session)?;
    write_atomic(path, json.as_bytes()).map_err(|source| SaveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(not(target_family = "wasm"))]
    if let Some(folder) = path.parent() {
        sweep_stale_temps(folder, SystemTime::now());
    }
    Ok(())
}

/// Reads the project at `path`. See [`from_json`] for what is accepted.
pub fn load(path: impl AsRef<Path>) -> Result<Project, LoadError> {
    load_with_session(path).map(|(project, _)| project)
}

/// Reads the project at `path` and the session its file holds beside it,
/// if it holds one. See [`from_json`] and [`session_from_json`].
pub fn load_with_session(
    path: impl AsRef<Path>,
) -> Result<(Project, Option<ProjectSession>), LoadError> {
    let path = path.as_ref();
    let json = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((from_json(&json)?, session_from_json(&json)))
}

/// Writes a backup of the project to `Backup/<name> <timestamp>.windfall`
/// beside the project file at `project_path`, and returns the backup's path.
///
/// Afterwards only the newest `keep` backups of this project remain;
/// [`DEFAULT_BACKUP_COUNT`] is the usual number, and the backup just written
/// is always kept. "Newest" goes by file name, so every timestamp passed for
/// one project must have the same length and sort in time order, as
/// `2026-10-06 18-13-05` does. The timestamp becomes part of a file name and
/// so cannot contain characters such as `:` or `/`. [`backup_origin`] knows a
/// backup again only when its timestamp has exactly that shape.
///
/// The backup holds the project as [`save`] writes it, sample paths
/// included; the module documentation says how those resolve.
pub fn write_backup(
    project: &Project,
    project_path: impl AsRef<Path>,
    timestamp: &str,
    keep: usize,
) -> Result<PathBuf, SaveError> {
    write_backup_with(project, None, project_path, timestamp, keep)
}

/// [`write_backup`], with how the project is being played written beside
/// it as [`save_with`] writes it.
pub fn write_backup_with(
    project: &Project,
    session: Option<&ProjectSession>,
    project_path: impl AsRef<Path>,
    timestamp: &str,
    keep: usize,
) -> Result<PathBuf, SaveError> {
    let project_path = project_path.as_ref();
    let name = project_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            SaveError::BadName(format!(
                "{} does not name a project file",
                project_path.display()
            ))
        })?;
    let forbidden = |c: char| c.is_control() || r#"/\:*?"<>|"#.contains(c);
    if timestamp.is_empty() || timestamp.contains(forbidden) {
        return Err(SaveError::BadName(format!(
            "\"{timestamp}\" cannot be used as the timestamp in a backup's file name"
        )));
    }

    let folder = project_path.with_file_name(BACKUP_FOLDER);
    let prefix = format!("{name} ");
    let suffix = format!(".{FILE_EXTENSION}");
    let backup = folder.join(format!("{prefix}{timestamp}{suffix}"));
    save_with(project, session, &backup)?;
    prune_backups(&folder, &prefix, &suffix, timestamp.len(), keep.max(1));
    Ok(backup)
}

/// Deletes all but the `keep` newest backups whose names are `prefix`, a
/// timestamp of `timestamp_len` bytes, then `suffix`.
fn prune_backups(folder: &Path, prefix: &str, suffix: &str, timestamp_len: usize, keep: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    // The length test tells the backups of "Song" from those of "Song 2",
    // which share the folder and the prefix.
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| {
            name.len() == prefix.len() + timestamp_len + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        })
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    for name in names.iter().skip(keep) {
        // A backup that cannot be deleted costs disk space only. It must not
        // turn a backup that was written into a failure.
        let _ = fs::remove_file(folder.join(name));
    }
}

/// The project file a backup was made of, if `path` is a backup: a file
/// named `<name> <timestamp>.windfall` right inside a folder named `Backup`,
/// with a timestamp shaped like `2026-10-06 18-13-05`. The answer is
/// `<name>.windfall` in the folder that holds `Backup`.
///
/// Only the path is looked at. The disk is not touched, and the project
/// file itself may be gone.
pub fn backup_origin(path: &Path) -> Option<PathBuf> {
    let is_project = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(FILE_EXTENSION));
    let folder = path.parent()?;
    let in_backups = folder
        .components()
        .next_back()
        .is_some_and(|last| same_part(last, Component::Normal(BACKUP_FOLDER.as_ref())));
    if !is_project || !in_backups {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let split = stem.len().checked_sub(BACKUP_TIMESTAMP_SHAPE.len())?;
    let (name, timestamp) = stem.split_at_checked(split)?;
    let name = name.strip_suffix(' ').filter(|name| !name.is_empty())?;
    if !is_backup_timestamp(timestamp) {
        return None;
    }
    Some(folder.parent()?.join(format!("{name}.{FILE_EXTENSION}")))
}

/// The folder that the [`SamplePath::Project`] paths of the project file at
/// `path` are relative to: the folder the file is in or, for a backup, the
/// folder of the project it was made of. Pass it to [`resolve_sample_path`]
/// and [`sample_path_for`] as the project folder.
pub fn sample_dir(path: &Path) -> Option<PathBuf> {
    let project_file = backup_origin(path);
    project_file
        .as_deref()
        .unwrap_or(path)
        .parent()
        .map(Path::to_path_buf)
}

/// A backup timestamp with every digit written as `0`.
const BACKUP_TIMESTAMP_SHAPE: &str = "0000-00-00 00-00-00";

fn is_backup_timestamp(text: &str) -> bool {
    text.len() == BACKUP_TIMESTAMP_SHAPE.len()
        && text
            .bytes()
            .zip(BACKUP_TIMESTAMP_SHAPE.bytes())
            .all(|(byte, shape)| match shape {
                b'0' => byte.is_ascii_digit(),
                _ => byte == shape,
            })
}

/// Numbers the temporary files of this process, so no two writes share one.
static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Names tried for a temporary file before giving up. Each one that is
/// taken is a file some crashed run left behind.
const TEMP_ATTEMPTS: u32 = 1024;

/// Times the rename that puts a finished file in its place is tried.
const RENAME_ATTEMPTS: u32 = 6;

/// How long the first failed rename waits before the next try. Every later
/// wait is twice the one before: 10, 20, 40, 80 and 160 ms, a third of a
/// second in all.
const RENAME_BACKOFF: Duration = Duration::from_millis(10);

/// How old a temporary file has to be before it is taken to belong to a
/// write that died, and is deleted.
const STALE_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Writes `contents` to the file at `path`, replacing any file there.
/// Project files, backups and the app's settings are all written this way.
///
/// The bytes go to a temporary file in the same folder first and are then
/// renamed into place, so a crash cannot leave half a file, and the file at
/// `path` is never damaged: until the rename it is whatever it was. Missing
/// folders are created.
///
/// Every write has a temporary file of its own, so writes to one path from
/// several threads or processes cannot damage each other, and the file at
/// `path` is always the whole of one of them. Which one is up to the
/// caller: whoever must not be overtaken has to keep its writes in order.
///
/// Windows refuses to replace a file that another program holds open, as a
/// virus scanner or a search indexer does for a moment after a file has
/// changed. A rename that is refused that way is tried again a few times
/// over a third of a second before the write fails, with the file at `path`
/// still as it was.
///
/// A write that is cut short by a crash can leave its temporary file
/// behind. Its name starts with a dot and ends in `.tmp`, so it is never
/// taken for a project, and it never gets in the way of a later write.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(folder)?;
    }
    let mut temp = TempFile::create(path, &TEMP_SERIAL)?;
    temp.write(contents)?;
    temp.publish(path)
}

/// Renames `from` onto `to`, trying again after a short wait while the
/// failure looks like one that passes. `rename` does the renaming and
/// `wait` the waiting.
fn rename_patiently(
    from: &Path,
    to: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut wait: impl FnMut(Duration),
) -> io::Result<()> {
    let mut backoff = RENAME_BACKOFF;
    let mut tries = 1;
    loop {
        let error = match rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if !passes(&error, to) {
            return Err(error);
        }
        if tries == RENAME_ATTEMPTS {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "the file could not be replaced in {tries} tries; another program may be holding it open ({error})"
                ),
            ));
        }
        wait(backoff);
        backoff *= 2;
        tries += 1;
    }
}

/// Whether a failed rename onto `target` may succeed a moment later: the
/// file there was in use, or the call was interrupted. A folder in the way
/// stays in the way.
fn passes(error: &io::Error, target: &Path) -> bool {
    // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION.
    let in_use = cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33));
    let kind = matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::ResourceBusy
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
    );
    (in_use || kind) && !target.is_dir()
}

fn pause(duration: Duration) {
    // A browser has no thread to put to sleep, and no file to wait for.
    #[cfg(not(target_family = "wasm"))]
    std::thread::sleep(duration);
    #[cfg(target_family = "wasm")]
    let _ = duration;
}

/// Whether `name` is shaped like the name [`temp_path`] gives a temporary
/// file: a dot, the name of the file being written, a dot, a process id, a
/// dash, a serial number and `.tmp`.
fn is_temp_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((target, tag)) = rest.rsplit_once('.') else {
        return false;
    };
    let number = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    !target.is_empty()
        && tag
            .split_once('-')
            .is_some_and(|(process, serial)| number(process) && number(serial))
}

/// Deletes the temporary files in `folder` that were last written more than
/// [`STALE_TEMP_AGE`] before `now`. Whatever wrote them is long gone: a
/// write takes moments. Younger ones may belong to a write under way, here
/// or in another copy of Windfall, and are left alone.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
fn sweep_stale_temps(folder: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let named_like_one = entry.file_name().to_str().is_some_and(is_temp_name);
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > STALE_TEMP_AGE);
        if named_like_one && metadata.is_file() && stale {
            // Left where it is if it cannot be deleted: it costs disk
            // space only.
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The name of the temporary file numbered `serial` for a write to `target`.
/// The process id keeps two running copies of Windfall apart.
fn temp_path(target: &Path, serial: u64) -> io::Result<PathBuf> {
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".{}-{serial}.tmp", std::process::id()));
    Ok(target.with_file_name(name))
}

/// The temporary file of one write. Nothing else ever opens, renames or
/// deletes it, and it is deleted when dropped unless it was published.
struct TempFile {
    path: PathBuf,
    /// `None` once the contents are on disk and the file is closed.
    file: Option<File>,
    published: bool,
}

impl TempFile {
    /// Makes a new, empty file beside `target`, numbered from `serials`.
    fn create(target: &Path, serials: &AtomicU64) -> io::Result<Self> {
        let mut taken = None;
        for _ in 0..TEMP_ATTEMPTS {
            let path = temp_path(target, serials.fetch_add(1, Ordering::Relaxed))?;
            // `create_new` never opens a file that is already there, so a
            // file left behind by a run that crashed, under a process id
            // this run was given again, is stepped over and not reused.
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        published: false,
                    });
                }
                // Windows reports a folder of that name as access denied,
                // so the name is looked up instead of trusting the kind.
                Err(error)
                    if error.kind() == io::ErrorKind::AlreadyExists
                        || fs::symlink_metadata(&path).is_ok() =>
                {
                    taken = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(taken.unwrap_or_else(|| io::Error::other("no temporary file name was tried")))
    }

    /// Writes the contents and waits until they are on disk.
    fn write(&mut self, contents: &[u8]) -> io::Result<()> {
        let Some(mut file) = self.file.take() else {
            return Err(io::Error::other("the temporary file is already written"));
        };
        file.write_all(contents)?;
        // Without this the rename can reach the disk before the contents do.
        file.sync_all()
    }

    /// Renames the file into place, replacing what is at `target`.
    fn publish(mut self, target: &Path) -> io::Result<()> {
        // Closed first: Windows can refuse to rename a file that is open.
        self.file = None;
        rename_patiently(&self.path, target, |from, to| fs::rename(from, to), pause)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.published {
            self.file = None;
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Upgrades the JSON of a project by one format version.
type Migration = fn(&mut Value) -> Result<(), String>;

/// `MIGRATIONS[n]` upgrades a file of format version `n + 1` to version
/// `n + 2`. A migration changes the JSON in place and leaves `formatVersion`
/// alone; [`migrate`] sets it.
const MIGRATIONS: &[Migration] = &[];

// Every bump of FORMAT_VERSION must come with the migration that reaches it.
const _: () = assert!(MIGRATIONS.len() + 1 == FORMAT_VERSION as usize);

/// Runs the migrations that bring a file from format version `from` up to
/// the version after the last migration.
fn migrate(value: &mut Value, from: u32, migrations: &[Migration]) -> Result<(), LoadError> {
    if from == 0 {
        return Err(LoadError::Invalid(
            "format version 0 does not exist".to_owned(),
        ));
    }
    for (index, migration) in migrations.iter().enumerate().skip(from as usize - 1) {
        let version = index + 1;
        migration(value).map_err(|problem| {
            LoadError::Invalid(format!(
                "could not upgrade from format {version} to {}: {problem}",
                version + 1
            ))
        })?;
        if let Some(object) = value.as_object_mut() {
            object.insert("formatVersion".to_owned(), Value::from(version + 1));
        }
    }
    Ok(())
}

/// Where on disk a sample's audio file is. `project_dir` is the folder that
/// holds the project file, or `None` for a project that has never been
/// saved. Returns `None` for a project sample of an unsaved project, and for
/// a stored relative path that is malformed or would leave its folder.
pub fn resolve_sample_path(
    sample: &SamplePath,
    project_dir: Option<&Path>,
    factory_dir: &Path,
) -> Option<PathBuf> {
    match sample {
        SamplePath::Factory(relative) => join_relative(factory_dir, relative),
        SamplePath::Project(relative) => join_relative(project_dir?, relative),
        SamplePath::External(absolute) => Some(PathBuf::from(absolute)),
    }
}

fn join_relative(folder: &Path, relative: &str) -> Option<PathBuf> {
    if relative_path_problem(relative).is_some() {
        return None;
    }
    let mut path = folder.to_path_buf();
    for part in relative.split('/') {
        // On Windows a part such as "C:" would replace the path built so far.
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) => path.push(part),
            _ => return None,
        }
    }
    Some(path)
}

/// The way to store the audio file at `file` in a project: relative to the
/// project folder when it is inside it, else relative to the factory content
/// folder when it is inside that, else as it is.
///
/// Paths are compared part by part without touching the disk, so `file` and
/// the folders must be written the same way: all absolute, and all with or
/// all without Windows' `\\?\` prefix. Letter case is ignored on Windows.
pub fn sample_path_for(file: &Path, project_dir: Option<&Path>, factory_dir: &Path) -> SamplePath {
    if let Some(relative) = project_dir.and_then(|folder| relative_to(file, folder)) {
        return SamplePath::Project(relative);
    }
    if let Some(relative) = relative_to(file, factory_dir) {
        return SamplePath::Factory(relative);
    }
    SamplePath::External(file.to_string_lossy().into_owned())
}

/// The path of `file` inside `folder`, with forward slashes, or `None` when
/// it is not inside or cannot be stored as a relative path.
fn relative_to(file: &Path, folder: &Path) -> Option<String> {
    let mut parts = file.components();
    for folder_part in folder.components() {
        if !same_part(parts.next()?, folder_part) {
            return None;
        }
    }
    let rest: Vec<&str> = parts
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let relative = rest.join("/");
    relative_path_problem(&relative)
        .is_none()
        .then_some(relative)
}

fn same_part(a: Component, b: Component) -> bool {
    if cfg!(windows) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rename_title(value: &mut Value) -> Result<(), String> {
        let object = value.as_object_mut().ok_or("not an object")?;
        let title = object.remove("title").ok_or("no title")?;
        object.insert("name".to_owned(), title);
        Ok(())
    }

    fn add_tempo(value: &mut Value) -> Result<(), String> {
        let object = value.as_object_mut().ok_or("not an object")?;
        object.insert("tempo".to_owned(), json!(120));
        Ok(())
    }

    #[test]
    fn migrations_run_in_order_from_the_file_version() {
        let mut value = json!({ "formatVersion": 1, "title": "Song" });
        migrate(&mut value, 1, &[rename_title, add_tempo]).unwrap();
        assert_eq!(
            value,
            json!({ "formatVersion": 3, "name": "Song", "tempo": 120 })
        );
    }

    #[test]
    fn migrations_before_the_file_version_are_skipped() {
        let mut value = json!({ "formatVersion": 2, "name": "Song" });
        migrate(&mut value, 2, &[rename_title, add_tempo]).unwrap();
        assert_eq!(
            value,
            json!({ "formatVersion": 3, "name": "Song", "tempo": 120 })
        );
    }

    #[test]
    fn a_file_at_the_newest_version_is_left_alone() {
        let mut value = json!({ "formatVersion": 3, "name": "Song" });
        migrate(&mut value, 3, &[rename_title, add_tempo]).unwrap();
        assert_eq!(value, json!({ "formatVersion": 3, "name": "Song" }));
    }

    #[test]
    fn a_failed_migration_names_the_versions() {
        let mut value = json!({ "formatVersion": 1 });
        let error = migrate(&mut value, 1, &[rename_title]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the project file is damaged: could not upgrade from format 1 to 2: no title"
        );
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn two_writes_to_one_path_never_share_a_temporary_file() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        let serials = AtomicU64::new(0);

        // A has its temporary file open while B does all of its work.
        let mut a = TempFile::create(&target, &serials).unwrap();
        let mut b = TempFile::create(&target, &serials).unwrap();
        assert_ne!(a.path, b.path);
        b.write(b"written by B").unwrap();
        b.publish(&target).unwrap();

        // What A writes now must not land in the file B published.
        a.write(b"written by A, at greater length").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"written by B");
        a.publish(&target).unwrap();
        assert_eq!(
            fs::read(&target).unwrap(),
            b"written by A, at greater length"
        );
        assert_eq!(names(folder.path()), ["Song.windfall"]);
    }

    #[test]
    fn a_write_that_fails_removes_only_its_own_temporary_file() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        let serials = AtomicU64::new(0);

        let mut a = TempFile::create(&target, &serials).unwrap();
        let mut b = TempFile::create(&target, &serials).unwrap();
        a.write(b"half of A").unwrap();
        let (a_path, b_path) = (a.path.clone(), b.path.clone());
        // A gives up, as it does after an error.
        drop(a);
        assert!(!a_path.exists());
        assert!(b_path.exists());
        b.write(b"all of B").unwrap();
        b.publish(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"all of B");

        // A folder is in the way of the rename.
        let taken = folder.path().join("Taken.windfall");
        fs::create_dir(&taken).unwrap();
        let mut c = TempFile::create(&taken, &serials).unwrap();
        c.write(b"all of C").unwrap();
        assert!(c.publish(&taken).is_err());
        assert_eq!(names(folder.path()), ["Song.windfall", "Taken.windfall"]);
    }

    #[test]
    fn a_temporary_file_left_by_a_crashed_run_is_stepped_over() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        // The names this run would use first are taken, one by a file and
        // one by a folder.
        let (left_file, left_folder) = (
            temp_path(&target, 0).unwrap(),
            temp_path(&target, 1).unwrap(),
        );
        fs::write(&left_file, b"half a proj").unwrap();
        fs::create_dir(&left_folder).unwrap();

        let serials = AtomicU64::new(0);
        let mut temp = TempFile::create(&target, &serials).unwrap();
        assert_eq!(temp.path, temp_path(&target, 2).unwrap());
        temp.write(b"a whole project").unwrap();
        temp.publish(&target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"a whole project");
        assert_eq!(fs::read(&left_file).unwrap(), b"half a proj");
        assert!(left_folder.is_dir());
        assert_eq!(names(folder.path()).len(), 3);
    }

    #[test]
    fn a_temporary_file_is_never_named_like_a_project() {
        let target = Path::new("songs").join("Song.windfall");
        let temp = temp_path(&target, 7).unwrap();
        assert_eq!(temp.parent(), target.parent());
        let name = temp.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(".Song.windfall."), "{name}");
        assert_eq!(temp.extension().unwrap(), "tmp");
        assert_eq!(backup_origin(&temp), None);
    }

    #[test]
    fn a_rename_that_is_refused_for_a_moment_is_tried_again() {
        let busy = || io::Error::from(io::ErrorKind::PermissionDenied);
        let (from, to) = (Path::new("a.tmp"), Path::new("a"));

        // Refused twice, then let through.
        let mut calls = 0;
        let mut waits = Vec::new();
        let renamed = rename_patiently(
            from,
            to,
            |_, _| {
                calls += 1;
                if calls < 3 { Err(busy()) } else { Ok(()) }
            },
            |wait| waits.push(wait),
        );
        assert!(renamed.is_ok());
        assert_eq!(calls, 3);
        assert_eq!(waits, [RENAME_BACKOFF, RENAME_BACKOFF * 2]);

        // Refused every time: it gives up after the last try and says so.
        let mut calls = 0;
        let mut waited = Duration::ZERO;
        let error = rename_patiently(
            from,
            to,
            |_, _| {
                calls += 1;
                Err(busy())
            },
            |wait| waited += wait,
        )
        .unwrap_err();
        assert_eq!(calls, RENAME_ATTEMPTS);
        assert_eq!(waited, Duration::from_millis(310));
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let message = error.to_string();
        assert!(
            message.starts_with(
                "the file could not be replaced in 6 tries; another program may be holding it open"
            ),
            "{message}"
        );

        // A failure that will not pass is not tried again.
        let mut calls = 0;
        let error = rename_patiently(
            from,
            to,
            |_, _| {
                calls += 1;
                Err(io::Error::from(io::ErrorKind::NotFound))
            },
            |_| panic!("waited for a failure that cannot pass"),
        )
        .unwrap_err();
        assert_eq!((calls, error.kind()), (1, io::ErrorKind::NotFound));
    }

    /// Opens a file the way a program that does not share it does: while
    /// the handle is open, Windows refuses to rename another file onto it.
    #[cfg(windows)]
    fn open_unshared(path: &Path) -> File {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn a_file_another_program_holds_open_for_a_moment_is_still_replaced() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        fs::write(&target, b"the old project").unwrap();

        let held = open_unshared(&target);
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            drop(held);
        });
        write_atomic(&target, b"the new project").unwrap();
        holder.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the new project");
        assert_eq!(names(folder.path()), ["Song.windfall"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_that_stays_held_fails_the_write_and_is_left_as_it_was() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        fs::write(&target, b"the old project").unwrap();

        let held = open_unshared(&target);
        let error = write_atomic(&target, b"the new project").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("another program may be holding it open"),
            "{error}"
        );
        drop(held);
        assert_eq!(fs::read(&target).unwrap(), b"the old project");
        // The write took its temporary file with it.
        assert_eq!(names(folder.path()), ["Song.windfall"]);

        // Once the file is let go, the next write goes through.
        write_atomic(&target, b"the new project").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the new project");
    }

    #[test]
    fn only_names_shaped_like_a_temporary_file_are_taken_for_one() {
        for name in [
            ".Song.windfall.4312-0.tmp",
            ".settings.json.1-18446744073709551615.tmp",
            ".a.7-7.tmp",
        ] {
            assert!(is_temp_name(name), "{name}");
        }
        for name in [
            "Song.windfall",
            ".Song.windfall.tmp",
            ".Song.windfall.4312.tmp",
            ".Song.windfall.4312-.tmp",
            ".Song.windfall.x-1.tmp",
            "Song.windfall.4312-0.tmp",
            ".4312-0.tmp",
            ".Song.windfall.4312-0.tmp.bak",
            "notes.tmp",
        ] {
            assert!(!is_temp_name(name), "{name}");
        }
        let target = Path::new("Song.windfall");
        let temp = temp_path(target, 3).unwrap();
        assert!(is_temp_name(temp.file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn saving_clears_out_temporary_files_a_dead_write_left_long_ago() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Song.windfall");
        let aged = |name: &str, age: Duration| {
            let path = folder.path().join(name);
            fs::write(&path, b"half a proj").unwrap();
            let file = OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::now() - age).unwrap();
        };
        let two_days = Duration::from_secs(48 * 60 * 60);
        let an_hour = Duration::from_secs(60 * 60);
        aged(".Song.windfall.999999-3.tmp", two_days);
        aged(".Other.windfall.12-0.tmp", two_days);
        // Too young to be sure of, and not ours at all.
        aged(".Song.windfall.999999-4.tmp", an_hour);
        aged("old notes.tmp", two_days);
        aged(".hidden", two_days);
        fs::create_dir(folder.path().join(".Folder.windfall.1-1.tmp")).unwrap();

        save(&Project::new("Song"), &target).unwrap();
        assert_eq!(
            names(folder.path()),
            [
                ".Folder.windfall.1-1.tmp",
                ".Song.windfall.999999-4.tmp",
                ".hidden",
                "Song.windfall",
                "old notes.tmp",
            ]
        );
    }

    #[test]
    fn version_zero_is_refused() {
        let mut value = json!({ "formatVersion": 0 });
        assert!(matches!(
            migrate(&mut value, 0, &[]),
            Err(LoadError::Invalid(_))
        ));
    }
}
