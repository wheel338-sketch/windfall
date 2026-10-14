//! Numbered saves share the ordinary save/capture/carry workflow. The final
//! atomic no-clobber publish is the reservation, so even another process cannot
//! win a filename between an existence check and publication.
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use windfall_project::{Project, ProjectSession, file};

const ATTEMPTS: u32 = 10_000;
/// Prepare each candidate before writing it, so relative audio uses the root
/// of the filename actually published. A publication race retries from the
/// captured project, returning only the successful candidate's preparation.
pub(super) fn write<T>(
    project: &Project,
    session: &ProjectSession,
    base: &Path,
    prepare: impl FnMut(&mut Project, &Path) -> Result<T, String>,
) -> Result<(PathBuf, Project, T), String> {
    write_checked(project, session, base, prepare, &mut |_| Ok(()))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Publication {
    Reservation,
    Copy,
    Written,
    Rename,
    Published,
}
/// Uses the production publication path; hooks inject local faults or terminate
/// a child process at precise boundaries without replacing filesystem code.
pub(super) fn write_checked<T>(
    project: &Project,
    session: &ProjectSession,
    base: &Path,
    mut prepare: impl FnMut(&mut Project, &Path) -> Result<T, String>,
    check: &mut impl FnMut(Publication) -> Result<(), String>,
) -> Result<(PathBuf, Project, T), String> {
    project.check()?;
    let parent = base.parent().ok_or("The project has no folder.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("The project filename is not Unicode.")?;
    let (stem, first) = split(stem);
    for offset in 0..ATTEMPTS {
        let number = first
            .checked_add(offset)
            .filter(|n| *n <= 999_999)
            .ok_or("The project version number limit was reached.")?;
        let target = parent.join(format!("{stem} ({number:03}).windfall"));
        // This avoids repeating slow preparation for already occupied names.
        // persist_noclobber remains authoritative if a competitor arrives later.
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        check(Publication::Reservation)?;
        let mut written = project.clone();
        check(Publication::Copy)?;
        let prepared = prepare(&mut written, &target)?;
        written.check()?;
        let json = file::to_json_with(&written, Some(session)).map_err(|e| e.to_string())?;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        staged
            .write_all(json.as_bytes())
            .map_err(|e| e.to_string())?;
        staged.as_file().sync_all().map_err(|e| e.to_string())?;
        check(Publication::Written)?;
        check(Publication::Rename)?;
        match staged.persist_noclobber(&target) {
            Ok(_) => {
                check(Publication::Published)?;
                return Ok((target, written, prepared));
            }
            Err(error)
                if error.error.kind() == io::ErrorKind::AlreadyExists
                    || fs::symlink_metadata(&target).is_ok() =>
            {
                // The failed candidate's staging and preparation are dropped.
                // Carrying is idempotent when the next candidate shares its root.
            }
            Err(error) => {
                return Err(windfall_ipc::ui_message(
                    "versions.saveFailed",
                    serde_json::json!({"error": error.error.to_string()}),
                ));
            }
        }
    }
    Err("No unused project version was found within 10,000 filenames.".into())
}
fn split(stem: &str) -> (&str, u32) {
    if let Some((base, suffix)) = stem.rsplit_once(" (")
        && let Some(digits) = suffix.strip_suffix(')')
        && (3..=6).contains(&digits.len())
        && digits.bytes().all(|c| c.is_ascii_digit())
        && let Ok(number) = digits.parse::<u32>()
    {
        return (base, number.saturating_add(1));
    }
    (stem, 1)
}
impl super::Session {
    /// Unsaved projects may supply a base filename from the Save picker.
    pub fn project_save_new_version(&self, path: Option<&str>) -> Result<String, String> {
        self.project_save_mode(path, true)
    }
}
