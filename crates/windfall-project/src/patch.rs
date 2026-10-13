//! What the app shell sends the UI after an edit.
//!
//! Sending the whole project on every fader move would not scale to a
//! project with ten thousand notes, so the document reports which sections an
//! edit touched and the shell sends only those.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{
    Automation, Channel, Mixer, Pattern, PatternId, Playlist, Project, ProjectSettings, SampleAsset,
};

/// The sections of a project an edit changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touched {
    pub notebook: bool,
    pub settings: bool,
    pub plugins: bool,
    pub note_edit_groups: bool,
    pub samples: bool,
    pub channels: bool,
    pub mixer: bool,
    pub playlist: bool,
    pub automations: bool,
    /// A pattern was added, removed or reordered.
    pub pattern_list: bool,
    /// Patterns whose own data changed. Removed patterns are not listed.
    pub patterns: Vec<PatternId>,
}

impl Touched {
    /// Everything changed, as after loading a project.
    pub fn all(project: &Project) -> Self {
        Self {
            settings: true,
            notebook: true,
            plugins: true,
            note_edit_groups: true,
            samples: true,
            channels: true,
            mixer: true,
            playlist: true,
            automations: true,
            pattern_list: true,
            patterns: project.patterns.iter().map(|pattern| pattern.id).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Adds everything `other` touched to `self`.
    pub fn merge(&mut self, other: &Touched) {
        self.settings |= other.settings;
        self.notebook |= other.notebook;
        self.note_edit_groups |= other.note_edit_groups;
        self.plugins |= other.plugins;
        self.samples |= other.samples;
        self.channels |= other.channels;
        self.mixer |= other.mixer;
        self.playlist |= other.playlist;
        self.automations |= other.automations;
        self.pattern_list |= other.pattern_list;
        for id in &other.patterns {
            if !self.patterns.contains(id) {
                self.patterns.push(*id);
            }
        }
    }
}

/// The changed sections of a project. A section that is absent did not
/// change. The UI keeps a copy of the project and merges patches into it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note_edit_groups: Option<Vec<crate::NoteEditGroup>>,
    /// Counts up by one for every patch the document produces. A UI that
    /// sees a gap has missed a patch and must fetch the whole project again.
    #[ts(type = "number")]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settings: Option<ProjectSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notebook: Option<crate::Notebook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugins: Option<Vec<crate::PluginBinding>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub samples: Option<Vec<SampleAsset>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channels: Option<Vec<Channel>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mixer: Option<Mixer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub playlist: Option<Playlist>,
    /// Every automation of the project, in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automations: Option<Vec<Automation>>,
    /// Ids of every pattern, in order. Present when a pattern was added,
    /// removed or reordered. Patterns missing from it were removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pattern_order: Option<Vec<PatternId>>,
    /// Full data of each pattern that was added or changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<Pattern>,
    pub history: HistoryView,
    /// The project has edits that are not saved.
    pub dirty: bool,
}

/// The undo history as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryView {
    /// Oldest first.
    pub entries: Vec<HistoryEntry>,
    /// How many entries are currently applied. Entries at or past this index
    /// have been undone and can be redone.
    pub cursor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryEntry {
    /// What the edit did, in words a menu can show, such as "Add channel".
    pub label: String,
}

/// The result of dispatching a command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DispatchResult {
    /// Ids the command created, in the order its documentation gives.
    pub created: Vec<u32>,
    pub patch: ProjectPatch,
}

/// The whole document, sent when the UI starts and after a project loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentSnapshot {
    #[ts(type = "number")]
    pub revision: u64,
    pub project: Project,
    pub history: HistoryView,
    pub dirty: bool,
    /// Path of the `.windfall` file, or `None` for a project never saved.
    pub path: Option<String>,
}
