//! Collect only files created in this session, never arbitrary folder contents.
//! Saved/backup/imported assets are pinned for the lifetime of the session.
use super::Session;
use crate::sync::lock;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use windfall_project::SamplePath;

#[derive(Default)]
pub(super) struct Assets {
    owned: BTreeSet<PathBuf>,
    pinned: BTreeSet<PathBuf>,
}
impl Session {
    pub(super) fn own_derived(&self, path: &Path) {
        lock(&self.inner.derived_assets)
            .owned
            .insert(crate::paths::clean(path));
    }
    pub(super) fn pin_derived(&self, path: &Path) {
        // Import callers pin before decoding, so collection cannot invalidate
        // a decoded-but-not-yet-attached source. Unknown files are never owned.
        lock(&self.inner.derived_assets)
            .pinned
            .insert(crate::paths::clean(path));
    }
    pub(super) fn pin_derived_project(&self, project: &windfall_project::Project) {
        let mut assets = lock(&self.inner.derived_assets);
        for sample in &project.samples {
            if let SamplePath::External(path) = &sample.path {
                assets.pinned.insert(crate::paths::clean(Path::new(path)));
            }
        }
    }
    pub fn collect_derived_assets(&self) -> Result<u32, String> {
        // Exclude saves, takes, document commits and history changes while
        // checking roots/deleting. IO runs with the document lock released.
        let _saving = lock(&self.inner.save);
        let _recording = self.recording_idle()?;
        let retained: BTreeSet<_> = self
            .state()
            .document
            .sample_sources()
            .filter_map(|(_, path)| match path {
                SamplePath::External(path) => Some(crate::paths::clean(Path::new(path))),
                _ => None,
            })
            .collect();
        let mut assets = lock(&self.inner.derived_assets);
        let dead: Vec<_> = assets
            .owned
            .iter()
            .filter(|path| !retained.contains(*path) && !assets.pinned.contains(*path))
            .cloned()
            .collect();
        let mut removed = 0;
        for path in dead {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    assets.owned.remove(&path);
                    removed += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    assets.owned.remove(&path);
                }
                Err(error) => {
                    return Err(windfall_ipc::ui_message(
                        "derivedAudio.collectFailed",
                        serde_json::json!({"path": path.display().to_string(), "error": error.to_string()}),
                    ));
                }
            }
        }
        Ok(removed)
    }
}
