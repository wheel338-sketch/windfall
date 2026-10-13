//! Keeping the engine's sample pool in line with the project.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use windfall_core::AudioBuffer;
use windfall_engine::SamplePool;
use windfall_project::file::resolve_sample_path;
use windfall_project::{Project, SampleAsset, SampleId, SamplePath};

use super::{Session, State};
use crate::events::Event;
use crate::paths;
use crate::samples::SampleCache;

/// The decoded samples of a project, and what could not be decoded.
pub(super) struct Decoded {
    pub pool: SamplePool,
    pub loaded: HashSet<SampleId>,
    pub failed: HashSet<SampleId>,
    /// One sentence per sample that is missing or unreadable.
    pub warnings: Vec<String>,
    pub bank_workspaces: Vec<windfall_dsp::sample_bank::BankReservation>,
}

/// Decodes every sample of a project. Slow: call it with no lock held.
pub(super) fn decode_all(
    cache: &SampleCache,
    project: &Project,
    project_dir: Option<&Path>,
    factory_dir: &Path,
) -> Decoded {
    let mut decoded = Decoded {
        pool: SamplePool::new(),
        loaded: HashSet::new(),
        failed: HashSet::new(),
        warnings: Vec::new(),
        bank_workspaces: Vec::new(),
    };
    if !project.retained_plugins.is_empty() {
        decoded.warnings.push(format!("{} imported plugin states are retained; unsupported instruments stay silent and effects bypassed. Use the source FL project with a compatible host to recover those sounds.", project.retained_plugins.len()));
    }
    for sample in &project.samples {
        if project.is_soundfont_source(sample.id) {
            continue;
        }
        let bank_source = project
            .channels
            .iter()
            .filter_map(|c| c.sample_instrument.as_ref())
            .any(|asset| asset.samples().any(|id| id == sample.id));
        let buffer = locate(sample, project_dir, factory_dir).and_then(|file| {
            if bank_source {
                let (audio, workspace) =
                    cache.decode_sample_bank(&file, &decoded.pool.sample_bank_budget_handle())?;
                decoded.bank_workspaces.push(workspace);
                Ok(audio)
            } else {
                decode(cache, &file)
            }
        });
        match buffer {
            Ok(buffer) => {
                decoded.pool.insert(sample.id, buffer);
                decoded.loaded.insert(sample.id);
            }
            Err(warning) => {
                decoded.failed.insert(sample.id);
                decoded.warnings.push(warning);
            }
        }
    }
    decoded
}

/// The file a sample's audio is in, or the warning to show when its stored
/// path leads nowhere.
pub(super) fn locate(
    sample: &SampleAsset,
    project_dir: Option<&Path>,
    factory_dir: &Path,
) -> Result<PathBuf, String> {
    resolve_sample_path(&sample.path, project_dir, factory_dir)
        .map(|file| paths::clean(&file))
        .ok_or_else(|| {
            let (SamplePath::Factory(stored)
            | SamplePath::Project(stored)
            | SamplePath::External(stored)) = &sample.path;
            format!("Missing sample: {stored}")
        })
}

/// Decodes a sample's file, with the warning to show when that fails.
fn decode(cache: &SampleCache, file: &Path) -> Result<AudioBuffer, String> {
    cache.decode(file).map_err(|error| {
        if file.exists() {
            error
        } else {
            format!("Missing sample: {}", paths::display(file))
        }
    })
}

impl Session {
    /// A prepared publication must attach the exact handles it compiled, never
    /// fetch a newer cache entry between validation and plan installation.
    pub(super) fn sync_prepared_samples(&self, state: &mut State, prepared: &SamplePool) {
        let removed: Vec<_> = state
            .pool
            .iter()
            .filter(|(id, _)| !prepared.contains(*id) && !state.loading.contains(id))
            .map(|(id, _)| id)
            .collect();
        for id in removed {
            state.pool.remove(id);
            state.loaded.remove(&id);
            state.failed.remove(&id);
        }
        self.sync_sample_sources(state, Some(prepared));
    }

    fn sync_sample_sources(&self, state: &mut State, prepared: Option<&SamplePool>) {
        let State {
            document,
            sample_dir,
            pool,
            loaded,
            loading,
            failed,
            generation,
            ..
        } = state;
        let project = document.project();
        let project_dir = sample_dir.as_deref();

        let wanted: HashSet<SampleId> = project
            .samples
            .iter()
            .filter(|s| !s.soundfont)
            .map(|sample| sample.id)
            .collect();
        loaded.retain(|id| {
            let keep = wanted.contains(id);
            if !keep {
                pool.remove(*id);
            }
            keep
        });
        failed.retain(|id| wanted.contains(id));
        loading.retain(|id| wanted.contains(id));

        if let Some(prepared) = prepared {
            for (id, audio) in prepared.iter().filter(|(id, _)| wanted.contains(id)) {
                // This publication does not own a pending decoder's lifetime.
                // Its existing held source (if any) stays in place until that
                // job finishes and schedules its own guarded preparation.
                if loading.contains(&id) {
                    continue;
                }
                pool.insert(id, audio.clone());
                loaded.insert(id);
                failed.remove(&id);
            }
        }

        let mut warnings = Vec::new();
        let mut pending = Vec::new();
        for sample in &project.samples {
            if project.is_soundfont_source(sample.id) {
                continue;
            }
            let id = sample.id;
            if loaded.contains(&id) || loading.contains(&id) || failed.contains(&id) {
                continue;
            }
            match locate(sample, project_dir, &self.inner.factory_dir) {
                Err(warning) => {
                    failed.insert(id);
                    warnings.push(warning);
                }
                // Unresolved prepared sources load later, then publish their own
                // guarded banks. No unprepared cache attachment enters this plan.
                Ok(file) => match prepared
                    .is_none()
                    .then(|| self.inner.cache.peek(&file))
                    .flatten()
                {
                    Some(buffer) => {
                        pool.insert(id, buffer);
                        loaded.insert(id);
                    }
                    None => {
                        loading.insert(id);
                        pending.push((id, file));
                    }
                },
            }
        }

        if !warnings.is_empty() {
            self.emit(Event::ProjectWarnings(warnings));
        }
        if pending.is_empty() {
            return;
        }
        let session = self.clone();
        let generation = *generation;
        let ids: Vec<SampleId> = pending.iter().map(|(id, _)| *id).collect();
        let spawned = std::thread::Builder::new()
            .name("windfall-sample-loader".to_owned())
            .spawn(move || {
                session.load_samples(generation, pending, Vec::new(), false);
            });
        if let Err(error) = spawned {
            log::error!("could not start a thread to load samples: {error}");
            for id in ids {
                loading.remove(&id);
            }
        }
    }

    /// Tries again to load every sample that has no audio because its file
    /// was missing or could not be read: looks each file up afresh, decodes
    /// what is there now and hands the project to the engine again. What
    /// is still missing is reported as warnings, in one event. Returns how
    /// many samples of the open project are still without audio.
    ///
    /// Slow when files have turned up: they are decoded on the caller's
    /// thread, with no lock held.
    pub fn samples_reload(&self) -> u32 {
        let (generation, pending, warnings) = {
            let mut state = self.state();
            let State {
                document,
                sample_dir,
                loading,
                failed,
                generation,
                ..
            } = &mut *state;
            let mut pending = Vec::new();
            let mut warnings = Vec::new();
            for sample in &document.project().samples {
                if document.project().is_soundfont_source(sample.id) {
                    continue;
                }
                if !failed.contains(&sample.id) {
                    continue;
                }
                match locate(sample, sample_dir.as_deref(), &self.inner.factory_dir) {
                    Ok(file) => {
                        // Marked as loading, so an edit made meanwhile
                        // does not start a second attempt.
                        failed.remove(&sample.id);
                        loading.insert(sample.id);
                        pending.push((sample.id, file));
                    }
                    Err(warning) => warnings.push(warning),
                }
            }
            (*generation, pending, warnings)
        };
        self.load_samples(generation, pending, warnings, true)
    }

    /// Decodes files for the document numbered `generation` and hands the
    /// audio to the engine, unless that document has been replaced.
    ///
    /// `warnings` are sent along with those of the files that fail here.
    /// The engine is handed the project when a sample got its audio, or
    /// whatever happened if `push` is set. Returns how many samples of the
    /// open project are without audio afterwards.
    pub(super) fn load_samples(
        &self,
        generation: u64,
        pending: Vec<(SampleId, PathBuf)>,
        mut warnings: Vec<String>,
        push: bool,
    ) -> u32 {
        let (bank_sources, budget) = {
            let state = self.state();
            let ids: HashSet<_> = state
                .document
                .project()
                .channels
                .iter()
                .filter_map(|c| c.sample_instrument.as_ref())
                .flat_map(|asset| asset.samples())
                .collect();
            (ids, state.pool.sample_bank_budget_handle())
        };
        let mut workspaces = Vec::new();
        let results: Vec<_> = pending
            .into_iter()
            .map(|(id, file)| {
                let audio = if bank_sources.contains(&id) {
                    self.inner
                        .cache
                        .decode_sample_bank(&file, &budget)
                        .map(|(audio, workspace)| {
                            workspaces.push(workspace);
                            audio
                        })
                } else {
                    decode(&self.inner.cache, &file)
                };
                (id, file, audio)
            })
            .collect();
        #[cfg(test)]
        self.pause("samples:decoded");

        let missing = |state: &State| u32::try_from(state.failed.len()).unwrap_or(u32::MAX);
        for _ in 0..8 {
            let (project, mut pool, preparation, directory) = {
                let state = self.state();
                if state.generation != generation {
                    return missing(&state);
                }
                (
                    state.document.project().clone(),
                    state.pool.clone(),
                    self.project_preparation(&state),
                    state.sample_dir.clone(),
                )
            };
            let mut changed = false;
            for (id, file, result) in &results {
                if project.sample(*id).is_some_and(|sample| {
                    !sample.soundfont
                        && locate(sample, directory.as_deref(), &self.inner.factory_dir)
                            .ok()
                            .is_some_and(|now| paths::same(&now, file))
                }) && let Ok(buffer) = result
                {
                    pool.insert(*id, buffer.clone());
                    changed = true;
                }
            }
            let prepared = if changed || push {
                match preparation.prepare(
                    &project,
                    &pool,
                    windfall_engine::ProjectPublicationIntent::Edit,
                ) {
                    Ok(ready) => Some(ready),
                    Err(error) => {
                        warnings.push(error.to_string());
                        break;
                    }
                }
            } else {
                None
            };
            let mut prepared = prepared;
            let mut retirement = None;
            let count = {
                let _recording = match self.recording_idle() {
                    Ok(guard) => guard,
                    Err(error) => {
                        warnings.push(error.to_string());
                        break;
                    }
                };
                let mut state = self.state();
                if state.generation != generation {
                    return missing(&state);
                }
                if let Some(prepared) = &mut prepared {
                    let pool = prepared.pool().clone();
                    let lease = match prepared.publication(self, &state) {
                        Ok(lease) => lease,
                        Err(error) => {
                            warnings.push(error.to_string());
                            continue;
                        }
                    };
                    retirement = Some(lease.install());
                    state.pool = pool;
                }
                for (id, file, result) in &results {
                    if state.document.project().sample(*id).is_none_or(|sample| {
                        sample.soundfont
                            || locate(sample, state.sample_dir.as_deref(), &self.inner.factory_dir)
                                .ok()
                                .is_none_or(|now| !paths::same(&now, file))
                    }) {
                        state.loading.remove(id);
                        continue;
                    }
                    state.loading.remove(id);
                    if state.document.project().sample(*id).is_none() {
                        continue;
                    }
                    match result {
                        Ok(_) => {
                            state.loaded.insert(*id);
                            state.failed.remove(id);
                        }
                        Err(warning) => {
                            state.failed.insert(*id);
                            warnings.push(warning.clone());
                        }
                    }
                }
                self.sync_transport();
                missing(&state)
            };
            if let Some(retirement) = &mut retirement {
                self.retire_project(retirement);
            }
            if !warnings.is_empty() {
                self.emit(Event::ProjectWarnings(warnings));
            }
            return count;
        }
        // Refusal leaves the old pool/plan untouched. Release only this load's
        // pending flags, making a visible failed load eligible for manual retry.
        let count = {
            let mut state = self.state();
            if state.generation != generation {
                return missing(&state);
            }
            for (id, _, _) in &results {
                state.loading.remove(id);
                if state
                    .document
                    .project()
                    .sample(*id)
                    .is_some_and(|sample| !sample.soundfont)
                {
                    state.failed.insert(*id);
                }
            }
            missing(&state)
        };
        warnings
            .push("Sample publication was refused or superseded. Reload samples to retry.".into());
        self.emit(Event::ProjectWarnings(warnings));
        count
    }
}
