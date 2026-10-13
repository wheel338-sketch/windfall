//! Durable archive/template ownership. Exclusive OS leases survive overlapping
//! sessions and are released by process death. Captures own immutable copies,
//! rather than pinning a project's extraction forever.
use super::Session;
use crate::{paths, sync::lock};
use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
};
use windfall_archive::{ExtractionRecord, OWNER_LOCK, OWNERSHIP};
use windfall_project::{Project, file};
#[derive(Default)]
pub(super) struct Extractions {
    roots: Vec<PathBuf>,
    owners: HashMap<PathBuf, File>,
    leased: HashMap<PathBuf, usize>,
    pub(super) recovered: Vec<PathBuf>,
}

fn managed_child(root: &Path, managed: &Path) -> bool {
    root.parent() == Some(managed)
        && root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("project-"))
        && fs::symlink_metadata(root).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn read_record(root: &Path) -> Result<ExtractionRecord, String> {
    let marker = root.join(OWNERSHIP);
    let meta = fs::symlink_metadata(&marker).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024 {
        return Err("Invalid extraction ownership record".into());
    }
    let record: ExtractionRecord =
        serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if record.version != 1
        || record.references.len() > 4096
        || record.references.iter().any(|path| !path.is_absolute())
    {
        return Err("Unsupported extraction ownership record".into());
    }
    Ok(record)
}
fn write_record(root: &Path, record: &ExtractionRecord) -> Result<(), String> {
    file::write_atomic(
        &root.join(OWNERSHIP),
        &serde_json::to_vec(record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Could not record extracted audio ownership: {e}"))
}
// Windows will not remove or move a byte-range-locked file. Serialize collectors
// using a lock outside the child being reclaimed, then release its owner lease.
// Another session cannot claim the child during that short filesystem operation.
fn collector_lease(managed: &Path) -> std::io::Result<File> {
    use sha2::{Digest, Sha256};

    // Keep synchronization metadata out of project and export directories.
    // The stable canonical identity lets independent sessions/processes share
    // the same OS lock without deleting a lock inode while a waiter owns it.
    let identity = fs::canonicalize(managed).or_else(|_| {
        let parent = managed.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing extraction parent")
        })?;
        Ok::<_, std::io::Error>(fs::canonicalize(parent)?.join(managed.file_name().unwrap()))
    })?;
    let mut identity = paths::clean(&identity).as_os_str().as_encoded_bytes().to_vec();
    if cfg!(windows) {
        identity.make_ascii_lowercase();
    }
    let hash = Sha256::digest(&identity);
    let lease = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(std::env::temp_dir().join(format!("windfall-project-collector-{hash:x}.lock")))?;
    lease.lock()?;
    Ok(lease)
}
fn under(root: &Path, source: &Path) -> bool {
    let root = paths::clean(root);
    let source = paths::clean(source);
    source
        .ancestors()
        .any(|ancestor| paths::same(ancestor, &root))
}
fn referenced(root: &Path, path: &Path) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    }
    let loaded = file::load(path).map_err(|e| e.to_string())?;
    let directory = file::sample_dir(path);
    Ok(loaded.samples.iter().any(|sample| {
        file::resolve_sample_path(&sample.path, directory.as_deref(), Path::new(""))
            .is_some_and(|source| under(root, &source))
    }))
}
impl Extractions {
    fn checkpoint(&self, project: &Project, directory: Option<&Path>) -> Result<(), String> {
        let Some(directory) = directory else {
            return Ok(());
        };
        for root in &self.roots {
            if under(root, directory) {
                file::save(project, root.join("project.windfall")).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    /// Claim only abandoned, marked direct roots. A malformed saved project or
    /// journal is a preservation decision, never permission to remove audio.
    pub(super) fn recover(managed: &Path) -> (Self, Vec<String>) {
        let mut registry = Self::default();
        let mut warnings = Vec::new();
        let Ok(_collector) = collector_lease(managed) else {
            return (registry, warnings);
        };
        let Ok(listing) = fs::read_dir(managed) else {
            return (registry, warnings);
        };
        for entry in listing.flatten() {
            let root = entry.path();
            if !managed_child(&root, managed) {
                continue;
            }
            let Ok(mut record) = read_record(&root) else {
                continue;
            };
            let lease = root.join(OWNER_LOCK);
            if !fs::symlink_metadata(&lease)
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                continue;
            }
            let Ok(owner) = File::options().read(true).write(true).open(lease) else {
                continue;
            };
            if owner.try_lock().is_err() {
                continue;
            }
            let refs: Result<Vec<_>, _> = record
                .references
                .iter()
                .map(|path| referenced(&root, path).map(|used| (path.clone(), used)))
                .collect();
            match refs {
                Err(error) => {
                    warnings.push(format!("Kept extracted audio {} because a saved reference could not be checked: {error}", paths::display(&root)));
                    continue;
                }
                Ok(refs) => {
                    record.references = refs
                        .into_iter()
                        .filter_map(|(path, used)| used.then_some(path))
                        .collect()
                }
            }
            if !record.references.is_empty() {
                if let Err(error) = write_record(&root, &record) {
                    warnings.push(error);
                    continue;
                }
                registry.roots.push(root.clone());
                registry.owners.insert(root, owner);
                continue;
            }
            // A complete unsaved project is recoverable data, not an abandoned
            // capture pin. Preserve all bytes and offer its native project path.
            if record.installed && !record.capture {
                let checkpoint = root.join("project.windfall");
                if file::load(&checkpoint).is_err() {
                    continue;
                }
                let recovery = managed
                    .parent()
                    .unwrap_or(managed)
                    .join("recovered-projects");
                if fs::create_dir_all(&recovery).is_err() {
                    continue;
                }
                let target = recovery.join(root.file_name().unwrap());
                drop(owner);
                match fs::rename(&root, &target) {
                    Ok(()) => {
                        registry.recovered.push(target.join("project.windfall"));
                        warnings.push(format!("Recovered an unsaved template/archive project at {}. Open it from Recent projects to recover its audio.", paths::display(&target.join("project.windfall"))));
                    }
                    Err(error) => warnings.push(format!(
                        "Could not recover extracted project {}: {error}",
                        paths::display(&root)
                    )),
                }
            } else {
                drop(owner);
                if let Err(error) = fs::remove_dir_all(&root) {
                    warnings.push(format!(
                        "Could not reclaim abandoned extraction {}: {error}",
                        paths::display(&root)
                    ));
                }
            }
        }
        (registry, warnings)
    }
}
impl Drop for super::Inner {
    fn drop(&mut self) {
        // Last session owner only: exclusive &mut access needs no State/save
        // guards. This control-side checkpoint preserves graceful-session-end
        // edits while the extraction leases are still held. Interrupted
        // extraction/install uses the pre-install durable checkpoint instead.
        let state = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        let extractions = self
            .extractions
            .get_mut()
            .unwrap_or_else(|e| e.into_inner());
        if let Err(error) =
            extractions.checkpoint(state.document.project(), state.sample_dir.as_deref())
        {
            self.events.emit(crate::events::Event::ProjectWarnings(vec![format!("Could not checkpoint the unsaved extracted project: {error}. Its initial project and audio are retained.")]));
        }
    }
}
impl Drop for Extractions {
    fn drop(&mut self) {
        // A reusable capture expires with its owning session. Graceful close
        // collects it immediately; process death leaves the same marked root
        // for startup recovery. Saved references retain the exact bytes.
        for (root, owner) in self.owners.drain() {
            if let Ok(record) = read_record(&root)
                && record.capture
                && !record
                    .references
                    .iter()
                    .any(|path| referenced(&root, path).unwrap_or(true))
                && let Some(managed) = root.parent()
                && managed_child(&root, managed)
                && let Ok(_collector) = collector_lease(managed)
            {
                drop(owner);
                if let Err(error) = fs::remove_dir_all(&root) {
                    log::warn!(
                        "Could not reclaim ended-session preset capture {}: {error}",
                        paths::display(&root)
                    );
                }
            } else {
                drop(owner);
            }
        }
    }
}
impl Session {
    pub(super) fn prepare_extraction(&self, root: &Path, project: &Project) -> Result<(), String> {
        file::save(project, root.join("project.windfall")).map_err(|e| e.to_string())?;
        let mut record = read_record(root)?;
        record.installed = true;
        write_record(root, &record)
    }
    pub(super) fn retain_extraction(&self, root: PathBuf, owner: File) {
        let mut roots = lock(&self.inner.extractions);
        if !roots.roots.contains(&root) {
            roots.roots.push(root.clone());
        }
        roots.owners.insert(root, owner);
        drop(roots);
        self.prune_extractions();
    }
    /// Make an immutable, reusable IPC capture independent of project lifetime.
    /// The capture lease is reclaimed after its owner ends, including crashes.
    pub(super) fn capture_audio(&self, source: &Path) -> Result<PathBuf, String> {
        let bytes = fs::metadata(source).map_err(|e| e.to_string())?.len();
        if bytes > windfall_project::preset::MAX_PRESET_ASSET_BYTES {
            return Err("The preset capture audio exceeds 1 GiB".into());
        }
        let managed = self
            .store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join("projects");
        fs::create_dir_all(&managed).map_err(|e| e.to_string())?;
        let root = tempfile::Builder::new()
            .prefix("project-")
            .tempdir_in(&managed)
            .map_err(|e| e.to_string())?;
        let owner = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.path().join(OWNER_LOCK))
            .map_err(|e| e.to_string())?;
        owner.lock().map_err(|e| e.to_string())?;
        write_record(
            root.path(),
            &ExtractionRecord {
                version: 1,
                capture: true,
                ..Default::default()
            },
        )?;
        let name = source.file_name().ok_or("Capture audio has no filename")?;
        let target = root.path().join(name);
        fs::copy(source, &target)
            .map_err(|e| format!("Could not preserve preset capture audio: {e}"))?;
        File::options()
            .write(true)
            .open(&target)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        let root = root.keep();
        // Capture copies stay usable for the owning session, but never pin the
        // extraction that supplied them and do not enter project-root GC.
        lock(&self.inner.extractions).owners.insert(root, owner);
        Ok(target)
    }
    /// Journal outgoing saved references before project publication. An aborted
    /// save is harmless: recovery checks the actual saved project, not the intent.
    pub(super) fn record_extraction_references(
        &self,
        project: &Project,
        target: &Path,
    ) -> Result<(), String> {
        let directory = file::sample_dir(target);
        let roots = lock(&self.inner.extractions);
        for root in roots.owners.keys() {
            if project.samples.iter().any(|sample| {
                file::resolve_sample_path(
                    &sample.path,
                    directory.as_deref(),
                    &self.inner.factory_dir,
                )
                .is_some_and(|source| under(root, &source))
            }) {
                let mut record = read_record(root)?;
                if !record.references.contains(&target.to_path_buf()) {
                    if record.references.len() >= 4096 {
                        return Err("Extracted audio has too many saved references; save a portable project copy".into());
                    }
                    record.references.push(target.to_path_buf());
                    write_record(root, &record)?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn extraction_lease(&self, directory: Option<&Path>) -> ExtractionLease {
        let root = directory.map(Path::to_path_buf);
        if let Some(root) = &root {
            *lock(&self.inner.extractions)
                .leased
                .entry(root.clone())
                .or_default() += 1;
        }
        ExtractionLease {
            session: self.clone(),
            root,
        }
    }
    pub(super) fn prune_extractions(&self) {
        // Save/backup own source files from snapshot through copying. Archive
        // jobs are single-owner, and a new archive keeps its TempDir until install.
        // A replacement must not block an in-flight save/dialog worker. Its
        // completion or a later replacement retries this conservative sweep.
        let Ok(_saving) = self.inner.save.try_lock() else {
            return;
        };
        let managed = self
            .store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join("projects");
        let Ok(_collector) = collector_lease(&managed) else {
            return;
        };
        let (sources, sample_dir, project_path) = {
            let state = self.state();
            let sources: Vec<_> = state
                .document
                .sample_sources()
                .filter_map(|(_, path)| {
                    windfall_project::file::resolve_sample_path(
                        path,
                        state.sample_dir.as_deref(),
                        &self.inner.factory_dir,
                    )
                })
                .collect();
            (sources, state.sample_dir.clone(), state.path.clone())
        };
        // IO starts after releasing State. Save exclusion keeps journals stable.
        let retired = {
            let mut roots = lock(&self.inner.extractions);
            let leased = roots.leased.clone();
            let mut retired = Vec::new();
            roots.roots.retain(|root| {
                let saved_reference = read_record(root)
                    .map(|record| {
                        record
                            .references
                            .iter()
                            .any(|path| referenced(root, path).unwrap_or(true))
                    })
                    .unwrap_or(true);
                let keep = saved_reference
                    || leased.keys().any(|path| under(root, path))
                    || sample_dir.as_ref().is_some_and(|dir| under(root, dir))
                    || project_path.as_ref().is_some_and(|path| under(root, path))
                    || sources.iter().any(|path| under(root, path));
                if !keep {
                    retired.push(root.clone());
                }
                keep
            });
            retired
        };
        for root in retired {
            // Only direct, marked and exclusively owned children.
            // also refuse one here so a renamed/replaced root never becomes a traversal.
            if root.parent() != Some(managed.as_path())
                || !root
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("project-"))
            {
                continue;
            }
            // The collector lease prevents recovery while Windows requires us
            // to release the child's byte-range owner lock before removal.
            let owner = lock(&self.inner.extractions).owners.remove(&root);
            if owner.is_none() {
                continue;
            }
            drop(owner);
            if std::fs::symlink_metadata(&root)
                .is_ok_and(|meta| meta.file_type().is_dir() && !meta.file_type().is_symlink())
                && let Err(error) = std::fs::remove_dir_all(&root)
            {
                self.emit(crate::events::Event::ProjectWarnings(vec![format!(
                    "Could not remove unused extracted audio {}: {error}",
                    paths::display(&root)
                )]));
                let mut registry = lock(&self.inner.extractions);
                registry.roots.push(root.clone());
                if let Ok(owner) = File::options()
                    .read(true)
                    .write(true)
                    .open(root.join(OWNER_LOCK))
                    && owner.try_lock().is_ok()
                {
                    registry.owners.insert(root, owner);
                }
            }
        }
    }
}

pub(super) struct ExtractionLease {
    session: Session,
    root: Option<PathBuf>,
}
impl Drop for ExtractionLease {
    fn drop(&mut self) {
        if let Some(root) = &self.root {
            let mut roots = lock(&self.session.inner.extractions);
            if let Some(count) = roots.leased.get_mut(root) {
                *count -= 1;
                if *count == 0 {
                    roots.leased.remove(root);
                }
            }
        }
        self.session.prune_extractions();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use windfall_project::{SampleAsset, SampleId, SamplePath};

    fn marked(root: &Path, record: ExtractionRecord) -> File {
        fs::create_dir_all(root).unwrap();
        let owner = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join(OWNER_LOCK))
            .unwrap();
        owner.lock().unwrap();
        write_record(root, &record).unwrap();
        owner
    }

    #[test]
    fn extraction_owner_crash_child() {
        let Ok(folder) = std::env::var("WINDFALL_R4_EXTRACTION_CHILD") else {
            return;
        };
        let mode = std::env::var("WINDFALL_R4_EXTRACTION_MODE").unwrap();
        let folder = Path::new(&folder);
        let root = folder.join("projects/project-child");
        let target = folder.join("saved.windfall");
        let _owner = marked(
            &root,
            ExtractionRecord {
                version: 1,
                installed: mode == "installed" || mode == "saved",
                capture: mode == "capture",
                references: if mode == "saved" || mode == "unpublished" {
                    vec![target.clone()]
                } else {
                    vec![]
                },
            },
        );
        fs::create_dir(root.join("samples")).unwrap();
        let audio = root.join("samples/exact.bin");
        fs::write(&audio, [19, 27, 0, 255, 45]).unwrap();
        File::options()
            .write(true)
            .open(&audio)
            .unwrap()
            .sync_all()
            .unwrap();
        let mut project = Project::new("Recover");
        let id = SampleId(project.next_id);
        project.next_id += 1;
        project.samples.push(SampleAsset {
            id,
            name: "Exact".into(),
            path: SamplePath::Project("samples/exact.bin".into()),
            processing: None,
            regions: vec![],
        });
        if mode == "installed" || mode == "saved" {
            file::save(&project, root.join("project.windfall")).unwrap();
        }
        if mode == "saved" {
            project.samples[0].path = SamplePath::External(paths::display(&audio));
            file::save(&project, &target).unwrap();
        }
        // No destructors: the operating system, not graceful Rust cleanup,
        // releases the lease and leaves the journal and extraction on disk.
        std::process::exit(73);
    }

    #[test]
    fn process_death_reclaims_partial_and_capture_roots_preserves_saved_refs_and_recovers_complete_audio()
     {
        for mode in ["partial", "capture", "installed", "saved", "unpublished"] {
            let folder = tempfile::tempdir().unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "session::extractions::tests::extraction_owner_crash_child",
                    "--nocapture",
                ])
                .env("WINDFALL_R4_EXTRACTION_CHILD", folder.path())
                .env("WINDFALL_R4_EXTRACTION_MODE", mode)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{mode}");
            let managed = folder.path().join("projects");
            let root = managed.join("project-child");
            let (registry, warnings) = Extractions::recover(&managed);
            match mode {
                "installed" => {
                    assert!(!root.exists());
                    assert_eq!(registry.recovered.len(), 1);
                    let path = &registry.recovered[0];
                    assert_eq!(file::load(path).unwrap().settings.name, "Recover");
                    assert_eq!(
                        fs::read(path.parent().unwrap().join("samples/exact.bin")).unwrap(),
                        [19, 27, 0, 255, 45]
                    );
                    assert!(warnings.iter().any(|w| w.contains("Recent projects")));
                }
                "saved" => {
                    assert!(root.exists());
                    assert_eq!(registry.roots.as_slice(), std::slice::from_ref(&root));
                    assert!(referenced(&root, &folder.path().join("saved.windfall")).unwrap());
                    assert_eq!(
                        fs::read(root.join("samples/exact.bin")).unwrap(),
                        [19, 27, 0, 255, 45]
                    );
                }
                _ => {
                    assert!(!root.exists(), "{mode}");
                    assert!(registry.roots.is_empty());
                }
            }
        }
    }

    #[test]
    fn a_live_owner_and_unmarked_or_damaged_roots_are_never_swept() {
        let folder = tempfile::tempdir().unwrap();
        let managed = folder.path().join("projects");
        let root = managed.join("project-active");
        let owner = marked(
            &root,
            ExtractionRecord {
                version: 1,
                capture: true,
                ..Default::default()
            },
        );
        fs::write(root.join("audio"), b"active").unwrap();
        let unmarked = managed.join("project-user");
        fs::create_dir(&unmarked).unwrap();
        fs::write(unmarked.join("keep"), b"user").unwrap();
        let damaged = managed.join("project-damaged");
        let bad_owner = marked(
            &damaged,
            ExtractionRecord {
                version: 1,
                ..Default::default()
            },
        );
        fs::write(damaged.join(OWNERSHIP), b"damaged").unwrap();
        drop(bad_owner);
        let _ = Extractions::recover(&managed);
        assert_eq!(fs::read(root.join("audio")).unwrap(), b"active");
        assert_eq!(fs::read(unmarked.join("keep")).unwrap(), b"user");
        assert!(damaged.exists());
        drop(owner);
        let _ = Extractions::recover(&managed);
        assert!(!root.exists());
        assert!(unmarked.exists());
        assert!(damaged.exists());
    }
}
