//! The document: the one copy of a project, and its undo history.

use crate::command::Command;
use crate::edit::{self, Direction, Edit};
use crate::error::CommandError;
use crate::lower;
use crate::model::{Project, SampleId, SamplePath};
use crate::patch::{DocumentSnapshot, HistoryEntry, HistoryView, ProjectPatch, Touched};

/// What a successful [`Document::dispatch`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// Ids the command created, in the order its documentation gives. A
    /// batch lists the ids of its commands one after another.
    pub created: Vec<u32>,
    /// The sections of the project that changed. Empty when the command
    /// changed nothing.
    pub touched: Touched,
    /// The history label of the command, such as "Add channel".
    pub label: String,
}

/// Owns a [`Project`] and the history of edits made to it.
///
/// # Undo and ids
///
/// Undo restores every part of the project exactly, with one exception:
/// `next_id` never goes back. Ids an undone edit handed out stay retired, so
/// an edit made after an undo can never receive an id that an earlier,
/// undone edit used. Anything that keys on an id for the life of the
/// document, such as decoded audio cached by sample id or a selection in the
/// UI, therefore never sees that id come to mean something else.
///
/// Redo does not allocate. It replays the stored edits, which carry the ids
/// they first created, so undo followed by redo brings back the same ids.
///
/// A command that fails does not use up ids: `next_id` is put back along
/// with everything else. A command that succeeds but nets out to no change
/// keeps any ids it allocated retired, because it has already reported them.
///
/// Because of this, `next_id` is not part of what [`is_dirty`](Self::is_dirty)
/// tracks: undoing back to the saved state makes the document clean even
/// though `next_id` is higher than in the file.
#[derive(Debug, Clone)]
pub struct Document {
    _asset_pins: windfall_dsp::asset::AssetPins,
    project: Project,
    revision: u64,
    entries: Vec<Entry>,
    /// How many entries are applied. Entries from here on have been undone.
    cursor: usize,
    /// The cursor at the last save, or `None` once that point of the history
    /// has been discarded and cannot be reached again.
    saved: Option<usize>,
    /// The gesture that made the last applied entry, while later dispatches
    /// of that gesture may still merge into it.
    open_gesture: Option<u64>,
}

#[derive(Debug, Clone)]
struct Entry {
    _asset_pins: windfall_dsp::asset::AssetPins,
    label: String,
    edits: Vec<Edit>,
}

fn project_asset_pins(project: &Project) -> windfall_dsp::asset::AssetPins {
    windfall_dsp::asset::AssetPins::for_effects(
        project
            .mixer
            .tracks
            .iter()
            .flat_map(|track| track.effects.iter().map(|effect| &effect.params)),
    )
}

fn history_asset_pins(edits: &[Edit]) -> windfall_dsp::asset::AssetPins {
    let mut pins = windfall_dsp::asset::AssetPins::default();
    for edit in edits {
        pins.extend(edit.effect_asset_pins());
    }
    pins
}

impl Document {
    /// Wraps a project with an empty history. The project is taken to be
    /// valid and saved; run [`Project::check`] first on one that comes from
    /// outside.
    pub fn new(project: Project) -> Self {
        Self {
            _asset_pins: project_asset_pins(&project),
            project,
            revision: 0,
            entries: Vec::new(),
            cursor: 0,
            saved: Some(0),
            open_gesture: None,
        }
    }

    /// Wraps a checked imported project that has not yet been saved.
    pub fn new_unsaved(project: Project) -> Self {
        let mut document = Self::new(project);
        document.saved = None;
        document
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Retain effect assets beside a cloned project during slow control-side
    /// work. Acquire while borrowing this document, before releasing its lock.
    pub fn effect_asset_pins(&self) -> windfall_dsp::asset::AssetPins {
        self._asset_pins.clone()
    }

    /// Source identities in the current project and every retained undo or
    /// redo edit. An identity may occur more than once; different paths for
    /// the same id remain distinct. Use this when moving a document's sample
    /// root, since samples absent from the project can still return later.
    pub fn sample_sources(&self) -> impl Iterator<Item = (SampleId, &SamplePath)> + Clone {
        self.project
            .samples
            .iter()
            .chain(
                self.entries
                    .iter()
                    .flat_map(|entry| &entry.edits)
                    .filter_map(Edit::sample),
            )
            .map(|sample| (sample.id, &sample.path))
    }

    /// The revision of the last patch built by [`patch`](Self::patch). 0
    /// until the first one.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Applies a command and records it for undo.
    ///
    /// Consecutive dispatches that carry the same non-`None` gesture id
    /// collapse into one undo step, as for a fader drag. A different gesture
    /// id, `None`, an undo, a redo, a jump or [`mark_saved`](Self::mark_saved)
    /// in between starts a new step. A gesture that ends up back where it
    /// started leaves no step at all.
    ///
    /// A command that changes nothing succeeds with an empty `touched` and
    /// leaves the history, including anything that can be redone, alone.
    /// Any other successful command discards the edits that had been undone.
    ///
    /// On failure the project, the history and the dirty state are exactly
    /// as they were.
    pub fn dispatch(
        &mut self,
        command: Command,
        gesture: Option<u64>,
    ) -> Result<Applied, CommandError> {
        let outcome = lower::execute(&mut self.project, command)?;
        // Acquire new roots before releasing the old project or merged edits.
        let next_pins = project_asset_pins(&self.project);
        let mut touched = Touched::default();
        for edit in &outcome.edits {
            edit.mark(Direction::Forward, &mut touched);
        }
        self.drop_removed_patterns(&mut touched);

        if outcome.edits.is_empty() {
            if self.open_gesture != gesture {
                self.open_gesture = None;
            }
        } else if let Some(entry) = self.gesture_entry(gesture) {
            for edit in outcome.edits {
                edit::push(&mut entry.edits, edit);
            }
            entry._asset_pins = history_asset_pins(&entry.edits);
            if entry.edits.is_empty() {
                self.entries.pop();
                self.cursor -= 1;
                self.open_gesture = None;
            }
        } else {
            // The saved state lies in the part of the history being discarded.
            if self.saved.is_some_and(|saved| saved > self.cursor) {
                self.saved = None;
            }
            self.entries.truncate(self.cursor);
            self.entries.push(Entry {
                _asset_pins: history_asset_pins(&outcome.edits),
                label: outcome.label.clone(),
                edits: outcome.edits,
            });
            self.cursor += 1;
            self.open_gesture = gesture;
        }

        self._asset_pins = next_pins;

        Ok(Applied {
            created: outcome.created,
            touched,
            label: outcome.label,
        })
    }

    /// Undoes the last applied edit. Returns `None` when there is none.
    pub fn undo(&mut self) -> Option<Touched> {
        self.open_gesture = None;
        let mut touched = Touched::default();
        if !self.step_back(&mut touched) {
            return None;
        }
        self.drop_removed_patterns(&mut touched);
        self._asset_pins = project_asset_pins(&self.project);
        Some(touched)
    }

    /// Applies the last undone edit again. Returns `None` when there is none.
    pub fn redo(&mut self) -> Option<Touched> {
        self.open_gesture = None;
        let mut touched = Touched::default();
        if !self.step_forward(&mut touched) {
            return None;
        }
        self.drop_removed_patterns(&mut touched);
        self._asset_pins = project_asset_pins(&self.project);
        Some(touched)
    }

    /// Undoes or redoes until `cursor` entries are applied. A cursor past the
    /// end of the history means all of it.
    pub fn jump(&mut self, cursor: u32) -> Touched {
        self.open_gesture = None;
        let target = (cursor as usize).min(self.entries.len());
        let mut touched = Touched::default();
        while self.cursor > target && self.step_back(&mut touched) {}
        while self.cursor < target && self.step_forward(&mut touched) {}
        self.drop_removed_patterns(&mut touched);
        self._asset_pins = project_asset_pins(&self.project);
        touched
    }

    pub fn history(&self) -> HistoryView {
        HistoryView {
            entries: self
                .entries
                .iter()
                .map(|entry| HistoryEntry {
                    label: entry.label.clone(),
                })
                .collect(),
            cursor: self.cursor as u32,
        }
    }

    /// True when the project differs from what was last saved. Undoing back
    /// to the saved state makes the document clean again.
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.cursor)
    }

    /// Records that the project as it is now has been saved.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.cursor);
        // A gesture that carried on into the saved entry would change what
        // "saved" means without the document turning dirty.
        self.open_gesture = None;
    }

    /// Changes where a sample's audio file is stored, as when saving the
    /// project to another folder had to give the file a new name there.
    ///
    /// This is not an edit: it adds no step to the history and leaves the
    /// dirty state alone, because the project means the same sound as
    /// before. The path changes in the project and in every stored edit
    /// that holds the sample, so undoing or redoing across the point where
    /// the sample was added or removed brings it back under the new path
    /// too.
    ///
    /// Fails, with nothing changed, when no sample has this id, neither in
    /// the project nor in the history; when the path is not well formed;
    /// and when another sample, in the project or anywhere in the history,
    /// has the same path, since two samples must never share one. Returns
    /// what changed in the project: its samples, or nothing when the sample
    /// had that path already or exists only in the history.
    pub fn relink_sample(
        &mut self,
        id: SampleId,
        path: SamplePath,
    ) -> Result<Touched, CommandError> {
        self.relink_sample_matching(id, None, path)
    }

    /// Like [`Self::relink_sample`], but changes only occurrences with this
    /// exact original source path. Other historical source versions with
    /// the same id keep their own paths. Refuses an unknown source identity
    /// with nothing changed, including history and dirty state.
    pub fn relink_sample_source(
        &mut self,
        id: SampleId,
        source: &SamplePath,
        path: SamplePath,
    ) -> Result<Touched, CommandError> {
        self.relink_sample_matching(id, Some(source), path)
    }

    fn relink_sample_matching(
        &mut self,
        id: SampleId,
        source: Option<&SamplePath>,
        path: SamplePath,
    ) -> Result<Touched, CommandError> {
        if let Some(problem) = path.problem() {
            return Err(CommandError::invalid(format!(
                "the sample path is not valid: {problem}"
            )));
        }
        let matches = |known_id, known_path: &SamplePath| {
            known_id == id && source.is_none_or(|source| source == known_path)
        };
        {
            let mut known = self.sample_sources();
            if !known.clone().any(|(id, path)| matches(id, path)) {
                return Err(CommandError::not_found("sample", id));
            }
            if known.any(|(known_id, known_path)| known_id != id && *known_path == path) {
                return Err(CommandError::invalid(
                    "another sample of the project already has that path",
                ));
            }
        }

        let mut touched = Touched::default();
        if let Some(sample) = self
            .project
            .samples
            .iter_mut()
            .find(|s| matches(s.id, &s.path))
            && sample.path != path
        {
            sample.path = path.clone();
            touched.samples = true;
        }
        let edits = self.entries.iter_mut().flat_map(|entry| &mut entry.edits);
        for sample in edits.filter_map(|edit| edit.sample_mut(id)) {
            if matches(sample.id, &sample.path) {
                sample.path = path.clone();
            }
        }
        Ok(touched)
    }

    /// Builds the patch for the UI and bumps the revision. Call it once for
    /// every `Touched` that `dispatch`, `undo`, `redo`, `jump` or
    /// `relink_sample` returns.
    pub fn patch(&mut self, touched: &Touched) -> ProjectPatch {
        self.revision += 1;
        self.patch_contents(touched)
    }

    /// A read-only response for a refused operation, at the current revision.
    pub fn unchanged_patch(&self) -> ProjectPatch {
        self.patch_contents(&Touched::default())
    }

    fn patch_contents(&self, touched: &Touched) -> ProjectPatch {
        let project = &self.project;
        ProjectPatch {
            revision: self.revision,
            settings: touched.settings.then(|| project.settings.clone()),
            plugins: touched.plugins.then(|| project.plugins.clone()),
            notebook: touched.notebook.then(|| project.notebook.clone()),
            samples: touched.samples.then(|| project.samples.clone()),
            channels: touched.channels.then(|| project.channels.clone()),
            mixer: touched.mixer.then(|| project.mixer.clone()),
            playlist: touched.playlist.then(|| project.playlist.clone()),
            automations: touched.automations.then(|| project.automations.clone()),
            pattern_order: touched
                .pattern_list
                .then(|| project.patterns.iter().map(|pattern| pattern.id).collect()),
            patterns: project
                .patterns
                .iter()
                .filter(|pattern| touched.patterns.contains(&pattern.id))
                .cloned()
                .collect(),
            history: self.history(),
            dirty: self.is_dirty(),
        }
    }

    /// The whole document, for a UI that is starting or has missed a patch.
    pub fn snapshot(&self, path: Option<String>) -> DocumentSnapshot {
        DocumentSnapshot {
            revision: self.revision,
            project: self.project.clone(),
            history: self.history(),
            dirty: self.is_dirty(),
            path,
        }
    }

    /// The entry a dispatch with this gesture id merges into, if any.
    fn gesture_entry(&mut self, gesture: Option<u64>) -> Option<&mut Entry> {
        if gesture.is_none() || self.open_gesture != gesture {
            return None;
        }
        // An open gesture always belongs to the last entry: anything that
        // moves the cursor closes it.
        self.entries.last_mut()
    }

    fn step_back(&mut self, touched: &mut Touched) -> bool {
        let Some(entry) = self.cursor.checked_sub(1).and_then(|i| self.entries.get(i)) else {
            return false;
        };
        for edit in entry.edits.iter().rev() {
            edit.apply(&mut self.project, Direction::Backward);
            edit.mark(Direction::Backward, touched);
        }
        self.cursor -= 1;
        true
    }

    fn step_forward(&mut self, touched: &mut Touched) -> bool {
        let Some(entry) = self.entries.get(self.cursor) else {
            return false;
        };
        for edit in &entry.edits {
            edit.apply(&mut self.project, Direction::Forward);
            edit.mark(Direction::Forward, touched);
        }
        self.cursor += 1;
        true
    }

    /// `Touched::patterns` lists patterns whose data changed; one that no
    /// longer exists has no data to send.
    fn drop_removed_patterns(&self, touched: &mut Touched) {
        touched
            .patterns
            .retain(|id| self.project.pattern(*id).is_some());
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn exact_relink_keeps_an_older_source_when_current_output_uses_its_old_name() {
        let root = tempfile::tempdir().unwrap();
        let old = SamplePath::Project("a (2).wav".into());
        let current = SamplePath::Project("a.wav".into());
        let absolute_old =
            SamplePath::External(root.path().join("a (2).wav").to_string_lossy().into_owned());
        let mut document = Document::new(Project::new("Source versions"));
        let id = SampleId(
            document
                .dispatch(
                    Command::AddSample {
                        name: "Sample".into(),
                        path: old.clone(),
                    },
                    None,
                )
                .unwrap()
                .created[0],
        );
        // A legal model fixture for the public exact-source contract; path
        // mutation with one retained ID is not exposed by production commands.
        document.project.samples.last_mut().unwrap().path = current.clone();
        document.project().check().unwrap();
        let history = document.history();
        document
            .relink_sample_source(id, &old, absolute_old.clone())
            .unwrap();
        document
            .relink_sample_source(id, &current, old.clone())
            .unwrap();
        assert_eq!(document.history(), history);
        assert_eq!(document.project().sample(id).unwrap().path, old);
        document.project().check().unwrap();
        let current_json = crate::file::to_json(document.project()).unwrap();
        assert_eq!(
            crate::file::from_json(&current_json)
                .unwrap()
                .sample(id)
                .unwrap()
                .path,
            old
        );
        document.undo().unwrap();
        document.project().check().unwrap();
        assert!(document.project().sample(id).is_none());
        document.redo().unwrap();
        document.project().check().unwrap();
        assert_eq!(document.project().sample(id).unwrap().path, absolute_old);
        let old_json = crate::file::to_json(document.project()).unwrap();
        assert_eq!(
            crate::file::from_json(&old_json)
                .unwrap()
                .sample(id)
                .unwrap()
                .path,
            absolute_old
        );
    }

    #[test]
    fn sample_sources_and_exact_relink_keep_different_paths_for_one_id() {
        let mut document = Document::new(Project::new("Sources"));
        let old = SamplePath::Project("old.wav".into());
        let current = SamplePath::Project("current.wav".into());
        let moved = SamplePath::Project("moved.wav".into());
        let id = SampleId(
            document
                .dispatch(
                    Command::AddSample {
                        name: "Source".into(),
                        path: old.clone(),
                    },
                    None,
                )
                .unwrap()
                .created[0],
        );
        // Model a newer source version without changing the stored older
        // occurrence; the public helper must expose and match both paths.
        document.project.samples.last_mut().unwrap().path = current.clone();
        let identities: Vec<_> = document
            .sample_sources()
            .map(|(id, path)| (id, path.clone()))
            .collect();
        assert!(identities.contains(&(id, old.clone())));
        assert!(identities.contains(&(id, current.clone())));
        let history = document.history();
        let dirty = document.is_dirty();
        assert!(
            document
                .relink_sample_source(id, &old, moved.clone())
                .unwrap()
                .is_empty()
        );
        assert_eq!(document.project().sample(id).unwrap().path, current);
        assert!(
            document
                .sample_sources()
                .any(|source| source == (id, &moved))
        );
        assert_eq!(document.history(), history);
        assert_eq!(document.is_dirty(), dirty);
        // The existing whole-ID API continues to relink every version.
        document.relink_sample(id, old.clone()).unwrap();
        assert!(document.sample_sources().all(|source| source == (id, &old)));
    }
}
