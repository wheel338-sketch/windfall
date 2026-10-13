//! New, open, save and backups.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::file::{self, DEFAULT_BACKUP_COUNT, FILE_EXTENSION};
use windfall_project::{
    CommandError, Document, DocumentSnapshot, Project, ProjectSession, SampleId, SamplePath,
    Touched,
};

use super::samples::{Decoded, decode_all};
use super::{Session, State};
use crate::events::Event;
use crate::paths;
use crate::sync::lock;
use crate::template::default_project;

/// What saving says when the project has never been given a file.
pub const NO_FILE_YET: &str = "This project has no file yet. Use Save as.";

/// What "Save as" says when it had to rename a sample in the new folder,
/// the project was edited before it could finish, and the new name is one
/// that a sample in the undo history already has. The edits cannot be kept
/// and the sample renamed both, so the open project stays where it was.
/// The error also reports the copy's already-published filename.
pub const EDITED_WHILE_MOVING: &str = "The open project could not move without losing sample sources in its undo history after a concurrent edit. The open project and history are unchanged.";

/// Numbered names tried for a sample whose own name is taken in the folder
/// a project is saved into.
const MAX_SAMPLE_NAMES: u32 = 1000;

/// Bytes of two files compared at a time.
const COMPARE_CHUNK: usize = 1 << 16;

/// What a request to replace the document notes down before its slow work,
/// to tell afterwards whether it may still go ahead.
pub(super) struct Replacement {
    /// Its place among all such requests.
    pub(super) request: u64,
    /// The document it set out to replace.
    generation: u64,
    /// How many edits that document had seen.
    edits: u64,
    /// Archive cancellation is checked at the final installation boundary,
    /// after slow sample/plugin preparation. Ordinary replacements omit it.
    pub(super) cancelled: Option<Arc<AtomicBool>>,
}

/// Why a project that was ready was not swapped in after all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A later request to replace the document was made, or one got there
    /// first.
    Superseded,
    Recording,
    /// The document was edited after the request was made.
    Edited,
    Cancelled,
    SamplerPreparation,
    NativePreparation,
    ProjectPreparation,
    EngineChanged,
}

impl Refusal {
    /// The message for the UI. `what` says what did not happen, as in
    /// `"song.windfall" was not opened`.
    pub(super) fn message(self, what: &str) -> String {
        match self {
            Refusal::Cancelled => "Project archive cancelled.".into(),
            Refusal::SamplerPreparation => {
                format!("{what}: sampler preparation failed; see the project warning.")
            }
            Refusal::NativePreparation => {
                format!("{what}: native plugin preparation failed; see the project warning.")
            }
            Refusal::ProjectPreparation => {
                format!("{what}: project preparation failed; see the project warning.")
            }
            Refusal::EngineChanged => format!(
                "{what}: engine readiness changed before publication. Try again; see the project warning."
            ),
            Refusal::Recording => "Stop or cancel recording before replacing the project.".into(),
            Refusal::Superseded => {
                format!("{what} because another project was opened or started after it.")
            }
            Refusal::Edited => format!(
                "{what} because the project was edited in the meantime. Save or discard those changes, then try again."
            ),
        }
    }
}

impl Session {
    /// Replaces the open project with the default one. Refused in the cases
    /// [`project_open`](Self::project_open) is.
    pub fn project_new(&self) -> Result<DocumentSnapshot, String> {
        drop(self.recording_idle()?);
        let ticket = self.begin_replacement();
        let project = default_project();
        let decoded = decode_all(&self.inner.cache, &project, None, &self.inner.factory_dir);
        #[cfg(test)]
        self.pause("new:install");
        self.install(&ticket, Document::new(project), None, None, None, decoded)
            .map_err(|refusal| refusal.message("The new project was not started"))
    }

    /// Opens the project file at `path`. Samples that are missing or cannot
    /// be read leave their channels silent and are reported as warnings.
    ///
    /// Slow: it reads the file and decodes every sample before the project
    /// is swapped in, so playback of the old project carries on until then.
    ///
    /// The UI asks what to do with unsaved changes before it calls this,
    /// and the answer holds for the project as it was at that moment. So
    /// the open is refused, with the open project left alone, when by the
    /// time the file is ready another new or open was asked for, another
    /// project was swapped in, or the open project was edited.
    ///
    /// The transport is set the way the file says the project was being
    /// played when it was saved: its mode, song looping and the pattern
    /// selected. A file that does not say opens in pattern mode on the
    /// first pattern, with looping as it is.
    ///
    /// A backup opens as a copy with no file of its own: its samples are
    /// found in the folder of the project it was made of, and saving it
    /// asks where to, as for a new project. Saving therefore never writes
    /// into the `Backup` folder and never replaces the project file unless
    /// the user picks it. A warning says so when the backup opens.
    pub fn project_open(&self, path: &str) -> Result<DocumentSnapshot, String> {
        if Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            return self.project_archive_open(path);
        }
        drop(self.recording_idle()?);
        let file = paths::absolute(path)?;
        let ticket = self.begin_replacement();
        let (project, played) = file::load_with_session(&file).map_err(sentence)?;
        let sample_dir = file::sample_dir(&file);
        let mut decoded = decode_all(
            &self.inner.cache,
            &project,
            sample_dir.as_deref(),
            &self.inner.factory_dir,
        );
        let save_to = match file::backup_origin(&file) {
            None => Some(file.clone()),
            Some(origin) => {
                decoded.warnings.insert(
                    0,
                    format!(
                        "\"{}\" is a backup of \"{}\". It was opened as a copy: use Save as to keep it.",
                        paths::name(&file),
                        paths::name(&origin)
                    ),
                );
                None
            }
        };
        // A backup is opened as a copy with no file of its own, so it is
        // not a project to offer again under Open recent.
        let has_own_file = save_to.is_some();
        #[cfg(test)]
        self.pause("open:install");
        let snapshot = self
            .install(
                &ticket,
                Document::new(project),
                save_to,
                played,
                sample_dir,
                decoded,
            )
            .map_err(|refusal| {
                refusal.message(&format!("\"{}\" was not opened", paths::name(&file)))
            })?;
        if has_own_file {
            self.store().remember_project(&file);
        }
        Ok(snapshot)
    }

    /// Saves the project to `path`, or to the file it already has. Returns
    /// the path saved to, which gains the `.windfall` extension if it lacks
    /// it.
    ///
    /// The project is written as it was when this was called. If it was
    /// edited while the file was being written, it stays marked as having
    /// unsaved changes. Saves and backups are written one at a time, in
    /// the order they were asked for.
    ///
    /// How the project is being played goes into the file beside it: the
    /// transport's mode, song looping and selected pattern, as they are
    /// now. They are not part of the project, so changing one neither
    /// makes the project unsaved nor can be undone.
    ///
    /// Saving into another folder takes the project's own samples along;
    /// [`carry_samples`] has the rules. If a sample had to be renamed
    /// there, the open project's sample is pointed at the new name too,
    /// with [`Document::relink_sample_source`]. That is not an undo step, since
    /// undoing it would point the project at the other file of that name,
    /// and it reaches into the undo history, so undoing and redoing past
    /// the point where the sample was added brings it back under the new
    /// name. The UI hears of it as an ordinary patch.
    ///
    /// When source paths cannot be relinked with the history intact, the
    /// copy is left on disk and the open document stays where it was,
    /// including its history. A save never discards undo/redo to adopt a copy.
    ///
    /// A project that was edited while it was being written into another
    /// folder is finished all the same. It moves to the new file, its
    /// samples are pointed at the names they were given there, the edits
    /// made meanwhile are kept, and it stays marked as having unsaved
    /// changes, because those edits are not in the file. A sample those
    /// edits brought in from the old project folder was not taken along,
    /// so it is pointed at the file where it is. The same applies to sources
    /// held only by undo/redo history, even when no edits happened during
    /// the save. Each exact original source path is retained; missing files
    /// are not replaced by unrelated names in the new folder. When a
    /// concurrent save cannot relink a renamed sample with the history
    /// intact, it is refused with [`EDITED_WHILE_MOVING`], leaving the open
    /// project at its old root.
    pub fn project_save(&self, path: Option<&str>) -> Result<String, String> {
        self.project_save_mode(path, false)
    }

    pub(super) fn project_save_mode(
        &self,
        path: Option<&str>,
        numbered: bool,
    ) -> Result<String, String> {
        self.project_save_owned(path, numbered, false)
    }
    pub(super) fn project_save_owned(&self, path: Option<&str>, numbered: bool, installation: bool) -> Result<String, String> {
        if !installation { drop(self.recording_idle()?); }
        let chosen = path
            .map(|path| paths::absolute(path).map(with_project_extension))
            .transpose()?;

        // Held until the document is marked: of two saves, the one that
        // copied the project first also writes first, so an older copy can
        // never land on top of a newer one.
        let saving = lock(&self.inner.save);
        if !installation { drop(self.recording_idle()?); }
        let (
            mut project,
            _asset_pins,
            played,
            target,
            previous_dir,
            edits,
            generation,
            plugin_revision,
        ) = {
            let state = self.state();
            let target = match chosen {
                Some(target) => target,
                None => state.path.clone().ok_or(NO_FILE_YET)?,
            };
            (
                state.document.project().clone(),
                state.document.effect_asset_pins(),
                self.played(),
                target,
                state.sample_dir.clone(),
                state.edits,
                state.generation,
                self.plugin_revision(),
            )
        };
        #[cfg(test)]
        self.pause("save:write");
        project = if installation { self.capture_plugins_idle(project, plugin_revision)? } else { self.capture_plugins(project, plugin_revision)? };
        self.pin_derived_project(&project);

        let prepare = |project: &mut Project, target: &Path| -> Result<_, String> {
            // Where this exact filename will look for its samples on reopen.
            // Adding a version suffix can change backup recognition.
            let target_dir = file::sample_dir(target);
            let moved = match (&previous_dir, &target_dir) {
                (Some(from), Some(to)) => !paths::same(from, to),
                (None, None) => false,
                _ => true,
            };
            let mut renamed = match (&previous_dir, &target_dir) {
                (Some(from), Some(to)) if moved => carry_samples(project, from, to, || {
                    #[cfg(test)]
                    self.pause("save:sample-missing");
                })?,
                _ => Vec::new(),
            };
            if let Some(to) = &target_dir {
                renamed.extend(carry_owned_presets(
                    project,
                    to,
                    &self.store().recordings_dir().join("Imported presets"),
                )?);
            }
            self.record_extraction_references(project, target)?;
            Ok((target_dir, moved, renamed))
        };
        let (target, project, (target_dir, moved, renamed)) = if numbered {
            super::versions::write(&project, &played, &target, prepare)?
        } else {
            let prepared = prepare(&mut project, &target)?;
            file::save_with(&project, Some(&played), &target).map_err(sentence)?;
            (target, project, prepared)
        };

        let retry_samples = {
            let mut state = self.state();
            // A project opened while this one was being written is not the
            // one that was saved.
            let current = state.generation == generation;
            let unchanged = state.edits == edits;
            if current {
                // Preserve sources absent from the saved snapshot, including
                // undo/redo-only samples and edits made during the write.
                // Do this first so their old relative names do not block a
                // carried sample's new name in the destination root.
                let mut relinks = match &previous_dir {
                    Some(from) if moved => {
                        left_behind(state.document.sample_sources(), &project, &renamed, from)
                    }
                    _ => Vec::new(),
                };
                relinks.extend(renamed);
                match relink(&state.document, &relinks) {
                    Some((document, touched)) => {
                        state.document = document;
                        state.path = Some(target.clone());
                        state.sample_dir = target_dir;
                        if unchanged {
                            state.document.mark_saved();
                        }
                        // It carries the renamed samples, if any, and the
                        // new dirty flag to every window.
                        self.publish(&mut state, &touched);
                    }
                    None if unchanged => {
                        return Err(format!(
                            "A copy was saved at {}, but the open project could not move without losing sample sources in its undo history. The open project and history are unchanged.",
                            paths::display(&target)
                        ));
                    }
                    None => {
                        return Err(format!(
                            "A copy was saved at {}. {EDITED_WHILE_MOVING}",
                            paths::display(&target)
                        ));
                    }
                }
            }
            // A sample that was missing may be there in the new folder.
            current && moved && !state.failed.is_empty()
        };
        drop(saving);
        self.prune_extractions();
        if retry_samples {
            self.samples_reload();
        }
        self.store().remember_project(&target);
        Ok(paths::display(&target))
    }

    /// The recent projects that still exist, most recent first.
    pub fn recent_projects(&self) -> Vec<String> {
        self.store().recent_projects()
    }

    /// Writes a backup beside the project file if the project has one and
    /// has unsaved changes. Returns the backup's path, or `None` when there
    /// was nothing to back up. `timestamp` goes into the file name; see
    /// [`file::write_backup`] for what it may hold.
    ///
    /// Backups queue up with saves, so two backups with one timestamp end
    /// with the newer project in the file.
    pub fn write_backup(&self, timestamp: &str) -> Result<Option<PathBuf>, String> {
        let _saving = lock(&self.inner.save);
        let (project, _asset_pins, played, path, plugin_revision) = {
            let state = self.state();
            match &state.path {
                Some(path) if state.document.is_dirty() => (
                    state.document.project().clone(),
                    state.document.effect_asset_pins(),
                    self.played(),
                    path.clone(),
                    self.plugin_revision(),
                ),
                _ => return Ok(None),
            }
        };
        #[cfg(test)]
        self.pause("backup:write");
        let project = self.capture_plugins(project, plugin_revision)?;
        self.pin_derived_project(&project);
        let backup = path.with_file_name(file::BACKUP_FOLDER).join(format!(
            "{} {timestamp}.{FILE_EXTENSION}",
            paths::stem(&path)
        ));
        self.record_extraction_references(&project, &backup)?;
        file::write_backup_with(
            &project,
            Some(&played),
            &path,
            timestamp,
            DEFAULT_BACKUP_COUNT,
        )
        .map(Some)
        .map_err(sentence)
    }

    /// How the project is being played right now, for its file to keep.
    pub(super) fn played(&self) -> ProjectSession {
        let transport = self.controller().transport();
        ProjectSession {
            mode: transport.mode,
            loop_song: transport.loop_song,
            pattern: Some(transport.pattern),
        }
    }

    /// Registers a request to replace the document. Call it before the
    /// slow work, and hand the result to [`install`](Self::install).
    pub(super) fn begin_replacement(&self) -> Replacement {
        let mut state = self.state();
        self.inner
            .sampler_ticket
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        state.replacements += 1;
        Replacement {
            request: state.replacements,
            generation: state.generation,
            edits: state.edits,
            cancelled: None,
        }
    }

    /// Swaps in a project that is ready to play: stops playback, replaces
    /// the document and the pool, resets the transport and tells the UI.
    ///
    /// Does nothing and says why when the request is no longer the last
    /// one made, or the document it was to replace is gone or was edited
    /// since. `save_to` is the file saving will write to, and `sample_dir`
    /// the folder the project's own samples are in. `played` is how the
    /// project's file says it was being played, which the transport is set
    /// to; with none it is set to pattern mode on the first pattern.
    pub(super) fn install(
        &self,
        ticket: &Replacement,
        document: Document,
        save_to: Option<PathBuf>,
        played: Option<ProjectSession>,
        sample_dir: Option<PathBuf>,
        mut decoded: Decoded,
    ) -> Result<DocumentSnapshot, Refusal> {
        drop(self.recording_idle().map_err(|_| Refusal::Recording)?);
        let runtime = lock(&self.inner.plugins)
            .as_ref()
            .map(|manager| manager.runtime.clone());
        let staged = runtime.as_ref().map(|runtime| runtime.prepare_document());
        if let Some(staged) = &staged {
            decoded.pool.set_plugin_factory(staged.clone());
        }
        let (baseline_pool, preparation) = {
            let state = self.state();
            if state.replacements != ticket.request || state.generation != ticket.generation {
                return Err(Refusal::Superseded);
            }
            if state.edits != ticket.edits {
                return Err(Refusal::Edited);
            }
            (state.pool.clone(), self.project_preparation(&state))
        };
        if staged.is_none()
            && let Some(factory) = baseline_pool.plugin_factory()
        {
            decoded.pool.set_plugin_factory(factory);
        }
        decoded.pool.share_sampler_budget(&baseline_pool);
        let project = document.project();
        let first_pattern = project.patterns.first().map(|pattern| pattern.id);
        let transport = match played {
            Some(played) => TransportPatch {
                mode: Some(played.mode),
                pattern: played
                    .pattern
                    .filter(|id| project.pattern(*id).is_some())
                    .or(first_pattern),
                loop_song: Some(played.loop_song),
                ..TransportPatch::default()
            },
            None => TransportPatch {
                mode: Some(PlayMode::Pattern),
                pattern: first_pattern,
                loop_song: None,
                ..TransportPatch::default()
            },
        };
        let mut prepared = preparation
            .with_sample_dir(sample_dir.clone())
            .prepare(
                project,
                &decoded.pool,
                windfall_engine::ProjectPublicationIntent::Replace { transport },
            )
            .map_err(|error| {
                let refusal = match &error {
                    windfall_engine::ProjectPreparationError::Sampler(_) => {
                        Refusal::SamplerPreparation
                    }
                    windfall_engine::ProjectPreparationError::Native { .. } => {
                        Refusal::NativePreparation
                    }
                    _ => Refusal::ProjectPreparation,
                };
                self.emit(Event::ProjectWarnings(vec![error.to_string()]));
                refusal
            })?;
        let preparation_pool = prepared.pool().clone();
        decoded.pool.install_sampler_preparation(prepared.pool());
        if let Some(runtime) = runtime {
            decoded.pool.set_plugin_factory(runtime);
        }
        let mut candidate = Some(State {
            midi_target: None,
            document,
            path: save_to,
            sample_dir,
            pool: decoded.pool,
            loaded: decoded.loaded,
            loading: HashSet::new(),
            failed: decoded.failed,
            generation: 0,
            edits: 0,
            replacements: 0,
            midi_import: None,
            midi_ticket: 0,
            slice_review: None,
            slice_ticket: 0,
        });
        let mut old_state = None;
        let mut retirement = None;
        #[cfg(test)]
        self.pause("sampler:install-prepared");
        for attempt in 0..8 {
            let mut next = None;
            let admitted = {
                let _recording = self.recording_idle().map_err(|_| Refusal::Recording)?;
                let mut state = self.state();
                if ticket
                    .cancelled
                    .as_ref()
                    .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
                {
                    return Err(Refusal::Cancelled);
                }
                if state.replacements != ticket.request || state.generation != ticket.generation {
                    return Err(Refusal::Superseded);
                }
                if state.edits != ticket.edits {
                    return Err(Refusal::Edited);
                }
                let generation = state.generation.checked_add(1).ok_or(Refusal::Superseded)?;
                let publication = match prepared.publication(self, &state) {
                    Ok(lease) => {
                        let mut candidate = candidate.take().expect("uncommitted replacement");
                        candidate.generation = generation;
                        candidate.replacements = state.replacements;
                        candidate.midi_ticket = state.midi_ticket;
                        candidate.slice_ticket = state.slice_ticket;
                        old_state = Some(std::mem::replace(&mut *state, candidate));
                        self.inner
                            .window_document_generation
                            .store(generation, std::sync::atomic::Ordering::Release);
                        self.analyzer_project_replaced(generation);
                        retirement = Some(lease.install());
                        self.refresh_input_monitor_signature(&state);
                        if let Some(staged) = &staged {
                            staged.install_document();
                        }
                        let snapshot = state.document.snapshot(state.path_text());
                        self.emit(Event::ProjectLoaded(snapshot.clone()));
                        self.announce_transport();
                        if !decoded.warnings.is_empty() {
                            self.emit(Event::ProjectWarnings(std::mem::take(
                                &mut decoded.warnings,
                            )));
                        }
                        Ok(snapshot)
                    }
                    Err(error) => Err(error),
                };
                match publication {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => {
                        // A source reload/background ready publication does not
                        // edit this document. Recapture only the same immutable
                        // stream/provider environment, never install a stale token.
                        if attempt < 7 {
                            next = prepared.refresh(self, &state, &error);
                        }
                        self.emit(Event::ProjectWarnings(vec![error.to_string()]));
                        if next.is_none() {
                            if attempt == 7 {
                                self.emit(Event::ProjectWarnings(vec![
                                    "Replacement audio readiness retry limit (8 attempts) reached; project unchanged.".into(),
                                ]));
                            }
                            return Err(Refusal::EngineChanged);
                        }
                        None
                    }
                }
            };
            if let Some(snapshot) = admitted {
                // Outer owners survive all guards, also during panic unwinding.
                drop(old_state);
                if let Some(retirement) = &mut retirement {
                    self.retire_project(retirement);
                }
                self.audio_logger_project_replaced();
                self.prune_extractions();
                return Ok(snapshot);
            }
            // A refused candidate cannot be selected/captured. Drop it before
            // starting any fresh native constructor, outside every guard.
            drop(prepared);
            self.emit(Event::ProjectWarnings(vec![
                "The source snapshot refreshed; retrying replacement audio preparation.".into(),
            ]));
            prepared = next
                .expect("retry admitted")
                .with_sample_dir(
                    candidate
                        .as_ref()
                        .expect("uncommitted replacement")
                        .sample_dir
                        .clone(),
                )
                .prepare(
                    candidate
                        .as_ref()
                        .expect("uncommitted replacement")
                        .document
                        .project(),
                    &preparation_pool,
                    windfall_engine::ProjectPublicationIntent::Replace { transport },
                )
                .map_err(|error| {
                    let refusal = match &error {
                        windfall_engine::ProjectPreparationError::Sampler(_) => {
                            Refusal::SamplerPreparation
                        }
                        windfall_engine::ProjectPreparationError::Native { .. } => {
                            Refusal::NativePreparation
                        }
                        _ => Refusal::ProjectPreparation,
                    };
                    self.emit(Event::ProjectWarnings(vec![error.to_string()]));
                    refusal
                })?;
            candidate
                .as_mut()
                .expect("uncommitted replacement")
                .pool
                .install_sampler_preparation(prepared.pool());
        }
        unreachable!("bounded replacement retry returns on its eighth attempt")
    }
}

/// An exact original source identity and the path it must use after a move.
type SampleRelink = (SampleId, SamplePath, SamplePath);

/// Points samples of the open document at new paths without touching the
/// undo history: all of them or none. Returns the document with the paths
/// changed and what that changed in its project, or `None` when a sample
/// could not be given its path, because another sample in the project or
/// its history has it.
///
/// A sample that is neither in the project nor in its history any more, as
/// after an edit that left the step which added it behind for good, has
/// nothing to point anywhere.
fn relink(document: &Document, renamed: &[SampleRelink]) -> Option<(Document, Touched)> {
    let mut document = document.clone();
    let mut touched = Touched::default();
    for (sample, source, path) in renamed {
        match document.relink_sample_source(*sample, source, path.clone()) {
            Ok(changed) => touched.merge(&changed),
            Err(CommandError::NotFound { .. }) => {}
            Err(_) => return None,
        }
    }
    Some((document, touched))
}

/// Sources in the live document or its retained history that were not
/// carried in `written`. Preserve their exact old files as absolute paths,
/// including missing files: a same-name file in the new root is unrelated.
fn left_behind<'a>(
    sources: impl IntoIterator<Item = (SampleId, &'a SamplePath)>,
    written: &Project,
    carried: &[SampleRelink],
    from: &Path,
) -> Vec<SampleRelink> {
    let mut seen = HashSet::new();
    sources
        .into_iter()
        .filter(|(_, source)| matches!(source, SamplePath::Project(_)))
        .filter(|(id, source)| {
            !written.sample(*id).is_some_and(|sample| {
                // A written output name can equal another historical source
                // path. Recover the captured original identity first.
                let original = carried
                    .iter()
                    .find(|(carried_id, _, new)| carried_id == id && new == &sample.path)
                    .map_or(&sample.path, |(_, old, _)| old);
                original == *source
            })
        })
        .filter_map(|(id, source)| {
            if !seen.insert((id, source.clone())) {
                return None;
            }
            let file = file::resolve_sample_path(source, Some(from), from)?;
            Some((
                id,
                source.clone(),
                SamplePath::External(paths::display(&file)),
            ))
        })
        .collect()
}

/// Starts a message from one of the project crate's errors, which begin in
/// lower case, with a capital.
fn sentence(error: impl std::fmt::Display) -> String {
    let text = error.to_string();
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

/// Adds `.windfall` to a file name that does not end in it.
fn with_project_extension(path: PathBuf) -> PathBuf {
    let has_it = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(FILE_EXTENSION));
    if has_it {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(FILE_EXTENSION);
    PathBuf::from(name)
}

/// Takes the samples a project keeps in its own folder, `from`, along to
/// the folder it is being saved into, `to`, so "Save as" to another folder
/// does not leave the new file pointing at nothing. `project` is the copy
/// about to be written; it is changed wherever a sample had to be renamed,
/// and the result lists those samples with their new paths.
///
/// A sample goes to the place inside `to` that it has inside `from`. A
/// file that is already there is used if it holds the same bytes. If it
/// holds anything else it is some other sound that happens to share the
/// name: the sample is copied beside it under a numbered name, such as
/// `kick (2).wav`, and the project is pointed at the copy.
/// Every copy is staged and published without replacement; a concurrent
/// arrival is compared or given another name before the project is written.
///
/// A sample that cannot be copied fails the save, because a project file
/// that points at nothing must not be written as if all were well. A
/// sample whose file is already missing has nothing to take along and is
/// left as it is.
fn carry_samples(
    project: &mut Project,
    from: &Path,
    to: &Path,
    mut missing: impl FnMut(),
) -> Result<Vec<SampleRelink>, String> {
    // Windows does not tell names apart by letter case, so neither does
    // this.
    let mut taken: HashSet<String> = project
        .samples
        .iter()
        .filter_map(|sample| match &sample.path {
            SamplePath::Project(relative) => Some(relative.to_lowercase()),
            _ => None,
        })
        .collect();
    let mut renamed = Vec::new();
    for sample in &mut project.samples {
        let SamplePath::Project(relative) = &sample.path else {
            continue;
        };
        let Some(source) = file::resolve_sample_path(&sample.path, Some(from), from) else {
            continue;
        };
        if !source.is_file() {
            continue;
        }
        let placed =
            place_sample(&source, to, relative, &taken, &mut missing).map_err(|error| {
                format!(
                    "Could not copy the sample \"{}\" to {}: {error}. The project was not saved.",
                    sample.name,
                    paths::display(to)
                )
            })?;
        if placed != *relative {
            taken.insert(placed.to_lowercase());
            let source = sample.path.clone();
            sample.path = SamplePath::Project(placed);
            renamed.push((sample.id, source, sample.path.clone()));
        }
    }
    Ok(renamed)
}

/// The import worker owns these original texts before the first save. Adopt
/// them into the portable project folder through the ordinary history relink.
/// Arbitrary external files and unrelated app storage are never collected.
fn carry_owned_presets(
    project: &mut Project,
    to: &Path,
    owned_folder: &Path,
) -> Result<Vec<SampleRelink>, String> {
    let mut taken: HashSet<_> = project
        .samples
        .iter()
        .filter_map(|s| match &s.path {
            SamplePath::Project(path) => Some(path.to_lowercase()),
            _ => None,
        })
        .collect();
    let mut relinks = Vec::new();
    for sample in &mut project.samples {
        let SamplePath::External(source) = &sample.path else {
            continue;
        };
        let source = Path::new(source);
        if source.parent() != Some(owned_folder)
            || !source
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ds"))
        {
            continue;
        }
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid owned preset filename.")?;
        let placed = place_sample(source, to, &format!("Presets/{name}"), &taken, &mut || {})
            .map_err(|e| format!("Could not carry owned preset {}: {e}", source.display()))?;
        taken.insert(placed.to_lowercase());
        let original = sample.path.clone();
        sample.path = SamplePath::Project(placed);
        relinks.push((sample.id, original, sample.path.clone()));
    }
    Ok(relinks)
}

/// Makes sure `folder` holds a copy of the file `source` at `relative`, or
/// failing that under the first numbered name that is free or already
/// holds the same bytes. Returns the relative path the copy is at. Names
/// in `taken` belong to the project's other samples and are not used.
fn place_sample(
    source: &Path,
    folder: &Path,
    relative: &str,
    taken: &HashSet<String>,
    missing: &mut impl FnMut(),
) -> io::Result<String> {
    for number in 1..=MAX_SAMPLE_NAMES {
        let candidate = if number == 1 {
            relative.to_owned()
        } else {
            numbered(relative, number)
        };
        if number > 1 && taken.contains(&candidate.to_lowercase()) {
            continue;
        }
        let stored = SamplePath::Project(candidate.clone());
        let target = file::resolve_sample_path(&stored, Some(folder), folder).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "its stored path cannot be used",
            )
        })?;
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing();
                if copy_into_place(source, &target)? {
                    return Ok(candidate);
                }
                // Another owner published after the missing-file check.
                // Reuse only identical bytes; otherwise try a new name.
                if fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_file())
                    && same_contents(source, &target)?
                {
                    return Ok(candidate);
                }
            }
            Err(error) => return Err(error),
            Ok(metadata) if metadata.is_file() && same_contents(source, &target)? => {
                return Ok(candidate);
            }
            // Something else has the name.
            Ok(_) => {}
        }
    }
    Err(io::Error::other(format!(
        "{MAX_SAMPLE_NAMES} other files there already have its name"
    )))
}

/// `sounds/kick.wav` numbered 2 is `sounds/kick (2).wav`.
fn numbered(relative: &str, number: u32) -> String {
    let name_at = relative.rfind('/').map_or(0, |slash| slash + 1);
    let (folder, name) = relative.split_at(name_at);
    // A dot that starts the name is part of the name, not an extension.
    let dot = name.rfind('.').filter(|dot| *dot > 0).unwrap_or(name.len());
    let (stem, extension) = name.split_at(dot);
    format!("{folder}{stem} ({number}){extension}")
}

/// Stage a complete sample in an owned temporary file, then publish it
/// without replacement. False means a competitor has the final name. All
/// error/collision cleanup belongs to the temporary file, never `target`.
fn copy_into_place(source: &Path, target: &Path) -> io::Result<bool> {
    copy_into_place_checked(source, target, &mut |_| Ok(()))
}
/// Production sample copier with a byte frontier for deterministic local
/// interruption tests. The no-op production hook is monomorphized away.
pub(super) fn copy_into_place_checked(
    source: &Path,
    target: &Path,
    check: &mut impl FnMut(u64) -> io::Result<()>,
) -> io::Result<bool> {
    use std::io::Write;
    let folder = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the sample has no folder"))?;
    fs::create_dir_all(folder)?;
    let mut staged = tempfile::NamedTempFile::new_in(folder)?;
    let mut source = File::open(source)?;
    let mut bytes = [0_u8; 64 * 1024];
    let mut copied = 0_u64;
    check(copied)?;
    loop {
        let count = source.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        staged.as_file_mut().write_all(&bytes[..count])?;
        copied += count as u64;
        check(copied)?;
    }
    staged.as_file().sync_all()?;
    match staged.persist_noclobber(target) {
        Ok(_) => Ok(true),
        Err(error)
            if error.error.kind() == io::ErrorKind::AlreadyExists
                || fs::symlink_metadata(target).is_ok() =>
        {
            Ok(false)
        }
        Err(error) => Err(error.error),
    }
}

/// Whether two files hold the same bytes. Failing to read `source` is an
/// error; a file at `other` that cannot be read is simply not the same.
fn same_contents(source: &Path, other: &Path) -> io::Result<bool> {
    let mut source = File::open(source)?;
    let Ok(mut other) = File::open(other) else {
        return Ok(false);
    };
    let length = source.metadata()?.len();
    if other.metadata().map(|metadata| metadata.len()).ok() != Some(length) {
        return Ok(false);
    }
    let mut ours = vec![0; COMPARE_CHUNK];
    let mut theirs = vec![0; COMPARE_CHUNK];
    loop {
        let read = read_full(&mut source, &mut ours)?;
        let Ok(other_read) = read_full(&mut other, &mut theirs) else {
            return Ok(false);
        };
        if ours[..read] != theirs[..other_read] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

/// Reads until the buffer is full or the file ends.
fn read_full(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_carry_output_name_never_classifies_a_different_original_source_as_carried() {
        use windfall_project::Command;
        fn version(path: &str) -> Document {
            let mut document = Document::new(Project::new("Source version"));
            let sample = SampleId(
                document
                    .dispatch(
                        Command::AddSample {
                            name: "Version".into(),
                            path: SamplePath::Project(path.into()),
                        },
                        None,
                    )
                    .unwrap()
                    .created[0],
            );
            let channel = document
                .dispatch(
                    Command::AddChannel {
                        name: None,
                        sample: Some(sample),
                        instrument: None,
                        index: None,
                        mixer_track: None,
                    },
                    None,
                )
                .unwrap();
            document
                .dispatch(
                    Command::ToggleStep {
                        pattern: windfall_project::PatternId(1),
                        channel: windfall_project::ChannelId(channel.created[0]),
                        step: 0,
                    },
                    None,
                )
                .unwrap();
            document
        }
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("old");
        let to = root.path().join("new");
        let tone = |path: &Path, value, frames| {
            let audio =
                windfall_core::AudioBuffer::from_interleaved(48_000, 2, vec![value; frames]);
            windfall_codec::write_wav(path, &audio, windfall_codec::WavSampleFormat::Int24)
                .unwrap();
        };
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&to).unwrap();
        tone(&from.join("a.wav"), 0.25, 9600);
        tone(&from.join("a (2).wav"), -0.75, 4800);
        tone(&to.join("a.wav"), -0.75, 4800);
        let original_current = fs::read(from.join("a.wav")).unwrap();
        let original_history = fs::read(from.join("a (2).wav")).unwrap();
        let competitor = fs::read(to.join("a.wav")).unwrap();
        let current = version("a.wav");
        let history = version("a (2).wav");
        let sample = current.project().samples[0].id;
        assert_eq!(sample, history.project().samples[0].id);
        // Two legal document versions provide the source occurrences to the
        // same iterator seam used by a live document's retained history. No
        // production command currently creates this differing-path shape.
        let mut written = current.project().clone();
        let carried = carry_samples(&mut written, &from, &to, || {}).unwrap();
        assert_eq!(
            written.sample(sample).unwrap().path,
            SamplePath::Project("a (2).wav".into())
        );
        let sources = current.sample_sources().chain(history.sample_sources());
        let mut relinks = left_behind(sources, &written, &carried, &from);
        relinks.extend(carried);
        for (document, source, name) in [
            (current, "a.wav", "current.windfall"),
            (history, "a (2).wav", "historical.windfall"),
        ] {
            let audio = windfall_codec::decode_file(from.join(source)).unwrap();
            let (mut document, _) = relink(&document, &relinks).unwrap();
            let before = document.project().clone();
            let steps = document.history().entries.len();
            for _ in 0..steps {
                document.undo().unwrap();
                document.project().check().unwrap();
            }
            for _ in 0..steps {
                document.redo().unwrap();
                document.project().check().unwrap();
            }
            assert_eq!(document.project(), &before);
            let saved = to.join(name);
            file::save(document.project(), &saved).unwrap();
            let reopened = file::load(&saved).unwrap();
            let cold = decode_all(
                &crate::samples::SampleCache::new(),
                &reopened,
                Some(&to),
                root.path(),
            );
            assert!(cold.warnings.is_empty(), "{:?}", cold.warnings);
            assert!(cold.loaded.contains(&sample));
            assert!(
                cold.pool.get(sample).unwrap().samples() == audio.samples(),
                "cold loading {source} aliased another source's carried output"
            );
        }
        assert_eq!(fs::read(from.join("a.wav")).unwrap(), original_current);
        assert_eq!(fs::read(from.join("a (2).wav")).unwrap(), original_history);
        assert_eq!(fs::read(to.join("a.wav")).unwrap(), competitor);
        assert_eq!(fs::read(to.join("a (2).wav")).unwrap(), original_current);
    }

    #[test]
    fn the_project_extension_is_added_only_when_missing() {
        let named = |name: &str| with_project_extension(PathBuf::from(name));
        assert_eq!(named("beat"), PathBuf::from("beat.windfall"));
        assert_eq!(named("beat.windfall"), PathBuf::from("beat.windfall"));
        assert_eq!(named("Beat.WINDFALL"), PathBuf::from("Beat.WINDFALL"));
        // A dot in the name is part of the name, not an extension to replace.
        assert_eq!(named("take.2"), PathBuf::from("take.2.windfall"));
    }

    #[test]
    fn messages_start_with_a_capital() {
        assert_eq!(sentence("could not read x"), "Could not read x");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn a_numbered_name_keeps_its_folder_and_extension() {
        assert_eq!(numbered("kick.wav", 2), "kick (2).wav");
        assert_eq!(
            numbered("sounds/808/kick.wav", 3),
            "sounds/808/kick (3).wav"
        );
        assert_eq!(numbered("takes/take.2.flac", 2), "takes/take.2 (2).flac");
        assert_eq!(numbered("v1.2/noise", 2), "v1.2/noise (2)");
        assert_eq!(numbered(".hidden", 2), ".hidden (2)");
    }

    #[test]
    fn portable_sample_publication_reuses_only_identical_bytes_and_cleans_owned_staging() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("source.wav");
        let destination = folder.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let target = destination.join("sound.wav");
        fs::write(&source, b"ours").unwrap();
        fs::write(&target, b"theirs").unwrap();
        assert!(!copy_into_place(&source, &target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), b"theirs");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        let taken = HashSet::new();
        assert_eq!(
            place_sample(&source, &destination, "sound.wav", &taken, &mut || {}).unwrap(),
            "sound (2).wav"
        );
        assert_eq!(
            fs::read(destination.join("sound (2).wav")).unwrap(),
            b"ours"
        );
        assert_eq!(
            place_sample(&source, &destination, "sound.wav", &taken, &mut || {}).unwrap(),
            "sound (2).wav"
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 2);
        assert_eq!(fs::read(&target).unwrap(), b"theirs");
    }

    #[test]
    fn portable_failed_sample_staging_leaves_competitors_untouched() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("competitor.wav");
        fs::write(&target, b"competitor's complete audio").unwrap();
        assert!(copy_into_place(&folder.path().join("missing.wav"), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"competitor's complete audio");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
    }

    #[test]
    fn files_are_the_same_only_byte_for_byte() {
        let folder = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = folder.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        };
        // Longer than one chunk, and differing only in the last byte.
        let mut long = vec![7_u8; COMPARE_CHUNK + 100];
        let a = file("a", &long);
        let same = file("same", &long);
        *long.last_mut().unwrap() = 8;
        let last_byte = file("last byte", &long);
        let shorter = file("shorter", &long[..COMPARE_CHUNK]);
        let empty = file("empty", &[]);

        assert!(same_contents(&a, &same).unwrap());
        assert!(!same_contents(&a, &last_byte).unwrap());
        assert!(!same_contents(&a, &shorter).unwrap());
        assert!(!same_contents(&a, &empty).unwrap());
        assert!(same_contents(&empty, &empty).unwrap());
        assert!(!same_contents(&a, &folder.path().join("missing")).unwrap());
        assert!(!same_contents(&a, folder.path()).unwrap());
        assert!(same_contents(&folder.path().join("missing"), &a).is_err());
    }
}
