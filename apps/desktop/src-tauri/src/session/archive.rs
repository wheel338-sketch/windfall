//! Native archive jobs consume the existing plugin capture and replacement
//! APIs; no plugin runtime ownership or musical edits are introduced here.
use super::{Session, samples::Decoded};
use crate::{paths, sync::lock};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(test)]
use windfall_archive::Stage;
use windfall_archive::{Error, Limits};
use windfall_project::{Document, DocumentSnapshot, Project};

pub(super) struct AudioLimits {
    pub(super) entry: u64,
    pub(super) total: u64,
}
impl Default for AudioLimits {
    fn default() -> Self {
        Self {
            entry: 256 * 1024 * 1024,
            total: 512 * 1024 * 1024,
        }
    }
}

pub(crate) struct ArchiveJob {
    session: Session,
    cancelled: Arc<AtomicBool>,
}
impl ArchiveJob {
    fn check(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl Drop for ArchiveJob {
    fn drop(&mut self) {
        *lock(&self.session.inner.archive_job) = None;
    }
}
impl Session {
    /// Reserve cancellation ownership before a shell command queues blocking
    /// work, so Cancel can also stop a job waiting to enter its worker.
    pub(crate) fn archive_job(&self) -> Result<ArchiveJob, String> {
        let mut active = lock(&self.inner.archive_job);
        if active.is_some() {
            return Err("Another project archive operation is running.".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some(cancelled.clone());
        Ok(ArchiveJob {
            session: self.clone(),
            cancelled,
        })
    }
    pub fn project_archive_cancel(&self) {
        if let Some(cancelled) = &*lock(&self.inner.archive_job) {
            cancelled.store(true, Ordering::Release);
        }
    }
    /// Export a captured snapshot without changing the live paths, history,
    /// current file or dirty flag. Existing archives are never overwritten.
    pub fn project_archive_save(&self, path: &str) -> Result<String, String> {
        self.project_archive_save_with(path, self.archive_job()?)
    }
    pub(crate) fn project_archive_save_with(
        &self,
        path: &str,
        job: ArchiveJob,
    ) -> Result<String, String> {
        job.check().map_err(|e| e.to_string())?;
        drop(self.recording_idle()?);
        let target = paths::absolute(path)?;
        let target = if target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            target
        } else {
            PathBuf::from(format!("{}.zip", paths::display(&target)))
        };
        self.archive_export_to(target, job)
    }
    /// The export itself, to exactly `target`, whatever its extension: a
    /// project template is the same archive under another name.
    pub(super) fn archive_export_to(
        &self,
        target: PathBuf,
        job: ArchiveJob,
    ) -> Result<String, String> {
        job.check().map_err(|e| e.to_string())?;
        // Sample directory and document snapshot must be captured together,
        // and serialized with Save as, which may carry/relink these sources.
        let saving = lock(&self.inner.save);
        drop(self.recording_idle()?);
        let (project, _asset_pins, played, directory, revision) = {
            let state = self.state();
            (
                state.document.project().clone(),
                state.document.effect_asset_pins(),
                self.played(),
                state.sample_dir.clone(),
                self.plugin_revision(),
            )
        };
        #[cfg(test)]
        self.pause("archive:capture");
        job.check().map_err(|e| e.to_string())?;
        let project = self.capture_plugins(project, revision)?;
        drop(saving);
        // Refuse an existing but unreadable/non-audio source as well. Bound
        // files before invoking the existing decoder; this validation uses
        // a private pool and never changes the live document or audio pool.
        let limits = Limits::default();
        if project.samples.len().saturating_add(2) > limits.entries {
            return Err("Too many archive audio sources.".into());
        }
        let mut bytes = 0u64;
        for sample in &project.samples {
            job.check().map_err(|e| e.to_string())?;
            if let Some(path) = windfall_project::file::resolve_sample_path(
                &sample.path,
                directory.as_deref(),
                &self.inner.factory_dir,
            ) && let Ok(metadata) = std::fs::metadata(path)
            {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or("Archive source size overflow.")?;
                if metadata.len() > limits.entry_bytes || bytes > limits.expanded_bytes {
                    return Err("Project archive exceeds its size limit.".into());
                }
            }
        }
        let checked =
            self.archive_audio(&project, directory.as_deref(), &job, AudioLimits::default())?;
        if !checked.failed.is_empty() {
            let missing = project
                .samples
                .iter()
                .filter(|sample| checked.failed.contains(&sample.id))
                .map(|sample| format!("{} ({:?})", sample.name, sample.path))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!(
                "These audio sources are missing or unreadable; no portable archive was created:\n{missing}\n{}",
                checked.warnings.join("\n")
            ));
        }
        drop(checked);
        windfall_archive::create(
            &project,
            Some(&played),
            directory.as_deref(),
            &self.inner.factory_dir,
            &target,
            limits,
            &mut |stage| {
                #[cfg(test)]
                if stage == Stage::Publish {
                    self.pause("archive:publish");
                }
                let _ = stage;
                job.check()
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(paths::display(&target))
    }
    /// Archives open as copies: Save asks for a normal project destination.
    /// The managed extraction is retained only after guarded installation,
    /// because pool/history references may continue using its files afterwards.
    pub fn project_archive_open(&self, path: &str) -> Result<DocumentSnapshot, String> {
        self.project_archive_open_with(path, self.archive_job()?)
    }
    pub(crate) fn project_archive_open_with(
        &self,
        path: &str,
        job: ArchiveJob,
    ) -> Result<DocumentSnapshot, String> {
        self.archive_open_adjusted(path, job, "The project archive", |_| {})
    }
    /// Opens the archive at `path` as a copy, after `adjust` has had the
    /// project that is about to be installed. `what` names the file in the
    /// messages: a project template is opened through here too.
    pub(super) fn archive_open_adjusted(
        &self,
        path: &str,
        job: ArchiveJob,
        what: &str,
        adjust: impl FnOnce(&mut Project),
    ) -> Result<DocumentSnapshot, String> {
        job.check().map_err(|e| e.to_string())?;
        drop(self.recording_idle()?);
        let source = paths::absolute(path)?;
        let mut ticket = self.begin_replacement();
        ticket.cancelled = Some(job.cancelled.clone());
        let managed = self
            .store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join("projects");
        let extracted =
            windfall_archive::extract(&source, &managed, Limits::default(), &mut |stage| {
                #[cfg(test)]
                if stage == Stage::Ready {
                    self.pause("archive:ready");
                }
                let _ = stage;
                job.check()
            })
            .map_err(|e| e.to_string())?;
        let root = extracted.root().to_path_buf();
        let decoded = self.archive_audio(
            &extracted.project,
            Some(&root),
            &job,
            AudioLimits::default(),
        )?;
        job.check().map_err(|e| e.to_string())?;
        if !decoded.failed.is_empty() {
            return Err(format!(
                "{what} contains unreadable audio and was not opened:\n{}",
                decoded.warnings.join("\n")
            ));
        }
        let mut project = extracted.project.clone();
        adjust(&mut project);
        project.check()?;
        #[cfg(test)]
        self.pause("archive:install");
        let snapshot = self
            .install(
                &ticket,
                Document::new(project),
                None,
                extracted.session,
                Some(root),
                decoded,
            )
            .map_err(|refusal| refusal.message(&format!("{what} was not opened")))?;
        extracted.retain();
        Ok(snapshot)
    }

    /// Compressed audio has its own expansion bound, separate from ZIP bytes.
    /// Keep this policy local to archive validation; ordinary project loading
    /// and the live sample cache retain their existing behavior.
    pub(super) fn archive_audio(
        &self,
        project: &Project,
        directory: Option<&Path>,
        job: &ArchiveJob,
        limits: AudioLimits,
    ) -> Result<Decoded, String> {
        let mut decoded = Decoded {
            pool: windfall_engine::SamplePool::new(),
            loaded: HashSet::new(),
            failed: HashSet::new(),
            warnings: Vec::new(),
        };
        if !project.retained_plugins.is_empty() {
            decoded.warnings.push(format!("{} imported plugin states are retained; unsupported instruments stay silent and effects bypassed. Use the source FL project with a compatible host to recover those sounds.", project.retained_plugins.len()));
        }
        let mut bytes = 0u64;
        for sample in &project.samples {
            job.check().map_err(|e| e.to_string())?;
            let options = windfall_codec::DecodeOptions {
                max_decoded_bytes: limits.entry.min(limits.total.saturating_sub(bytes)),
            };
            let result = windfall_project::file::resolve_sample_path(
                &sample.path,
                directory,
                &self.inner.factory_dir,
            )
            .ok_or_else(|| "The sample path cannot be resolved.".to_owned())
            .and_then(|path| {
                windfall_codec::decode_file_with(path, &options).map_err(|e| e.to_string())
            });
            job.check().map_err(|e| e.to_string())?;
            match result {
                Ok(buffer) => {
                    bytes += std::mem::size_of_val(buffer.samples()) as u64;
                    decoded.pool.insert(sample.id, buffer);
                    decoded.loaded.insert(sample.id);
                }
                Err(error) => {
                    decoded.failed.insert(sample.id);
                    decoded.warnings.push(format!("{} ({:?}): {error}. Archive audio limits are {} bytes per source and {} bytes total decoded audio.", sample.name, sample.path, limits.entry, limits.total));
                }
            }
        }
        Ok(decoded)
    }
}
