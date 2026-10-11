use super::Session;
use crate::backup::{BackupProvider, FolderProvider};
use crate::sync::lock;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use windfall_ipc::BackupVersion;

pub(crate) struct BackupJob { session: Session, pub(crate) cancel: Arc<AtomicBool> }
impl Drop for BackupJob {
    fn drop(&mut self) { *lock(&self.session.inner.backup_job) = None; }
}
impl Session {
    pub(crate) fn backup_job(&self) -> Result<BackupJob, String> {
        let mut active = lock(&self.inner.backup_job);
        if active.is_some() { return Err("Another backup job is running.".into()); }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(cancel.clone());
        Ok(BackupJob { session: self.clone(), cancel })
    }
    pub fn backup_cancel(&self) {
        if let Some(cancel) = &*lock(&self.inner.backup_job) { cancel.store(true, Ordering::Release); }
    }
    pub fn backup_versions(&self, folder: &str) -> Result<Vec<BackupVersion>, String> {
        FolderProvider::new(Path::new(folder))?.versions()
    }
    pub(crate) fn backup_save_with(&self, folder: &str, expected: Option<&str>, keep: usize, job: BackupJob) -> Result<BackupVersion, String> {
        let provider = FolderProvider::new(Path::new(folder))?;
        if job.cancel.load(Ordering::Acquire) { return Err("Backup cancelled.".into()); }
        // Request-owned staging directory. Export retains plugin capture and Save
        // serialization, includes audio, and never marks the live project clean.
        let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
        let archive = staging.path().join("snapshot.zip");
        let archive_job = self.archive_job_with_cancel(job.cancel.clone())?;
        self.project_archive_save_with(&crate::paths::display(&archive), archive_job)?;
        provider.publish(&archive, expected, keep, &job.cancel)
    }
    pub(crate) fn backup_restore_with(&self, folder: &str, id: &str, target: &str, job: BackupJob) -> Result<String, String> {
        let path = crate::paths::absolute(target)?;
        FolderProvider::new(Path::new(folder))?.restore(id, &path, &job.cancel)?;
        // Restore only publishes a verified archive. Opening it uses the normal
        // unsaved-change/replacement flow in the UI, rather than replacing here.
        Ok(crate::paths::display(&path))
    }
}
