//! Conservative reclamation of archive/template roots created by this session.
//! Untracked directories and roots pinned by exported captures are never touched.
use super::Session;
use crate::{paths, sync::lock};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
#[derive(Default)]
pub(super) struct Extractions {
    roots: Vec<PathBuf>,
    pinned: HashSet<PathBuf>,
}
impl Session {
    pub(super) fn retain_extraction(&self, root: PathBuf) {
        let mut roots = lock(&self.inner.extractions);
        if !roots.roots.contains(&root) {
            roots.roots.push(root);
        }
        drop(roots);
        self.prune_extractions();
    }
    /// Called while State is held, before a capture can escape the document.
    /// IPC captures are reusable immutable snapshots with no release callback,
    /// so their audio root is conservatively pinned for the session lifetime.
    pub(super) fn pin_extraction(&self, directory: Option<&Path>) {
        if let Some(directory) = directory {
            let mut roots = lock(&self.inner.extractions);
            let pinned: Vec<_> = roots
                .roots
                .iter()
                .filter(|root| directory.starts_with(root))
                .cloned()
                .collect();
            roots.pinned.extend(pinned);
            roots.pinned.insert(directory.to_path_buf());
        }
    }
    pub(super) fn prune_extractions(&self) {
        // Save/backup own source files from snapshot through copying. Archive
        // jobs are single-owner, and a new archive keeps its TempDir until install.
        let _saving = lock(&self.inner.save);
        let retired = {
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
            let mut roots = lock(&self.inner.extractions);
            let pinned = roots.pinned.clone();
            let mut retired = Vec::new();
            roots.roots.retain(|root| {
                let referenced = pinned.contains(root)
                    || state
                        .sample_dir
                        .as_ref()
                        .is_some_and(|dir| dir.starts_with(root))
                    || state
                        .save_to
                        .as_ref()
                        .is_some_and(|path| path.starts_with(root))
                    || sources.iter().any(|path| path.starts_with(root));
                if !referenced {
                    retired.push(root.clone());
                }
                referenced
            });
            retired
        };
        let managed = self
            .store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join("projects");
        for root in retired {
            // Only direct, own-session children. Rust removes symlinks themselves;
            // also refuse one here so a renamed/replaced root never becomes a traversal.
            if root.parent() != Some(managed.as_path())
                || !root
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("project-"))
            {
                continue;
            }
            if std::fs::symlink_metadata(&root)
                .is_ok_and(|meta| meta.file_type().is_dir() && !meta.file_type().is_symlink())
            {
                if let Err(error) = std::fs::remove_dir_all(&root) {
                    self.emit(crate::events::Event::ProjectWarnings(vec![format!(
                        "Could not remove unused extracted audio {}: {error}",
                        paths::display(&root)
                    )]));
                    lock(&self.inner.extractions).roots.push(root);
                }
            }
        }
    }
}
