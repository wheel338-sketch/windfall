//! What the shell tells every window, and the one trait it tells them
//! through.

use windfall_ipc::{EngineStatus, ExportProgress, TransportState};
use windfall_project::{DocumentSnapshot, ProjectPatch};

/// An event for the UI. docs/ARCHITECTURE.md lists when each one is sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ProjectPatch(ProjectPatch),
    ProjectLoaded(DocumentSnapshot),
    /// Problems with the project's files, each a sentence that can be shown
    /// as it is. Never empty.
    ProjectWarnings(Vec<String>),
    TransportState(TransportState),
    EngineStatus(EngineStatus),
    ExportProgress(ExportProgress),
    /// A note from a MIDI input that the audition destination took.
    MidiHardwareNote(windfall_ipc::MidiHardwareNote),
    /// Transient caller correlation, outside generated musical/project types.
    ExportRequestProgress { request: String, progress: ExportProgress },
}

impl Event {
    /// The name the UI listens for.
    pub fn name(&self) -> &'static str {
        match self {
            Event::ProjectPatch(_) => "project:patch",
            Event::ProjectLoaded(_) => "project:loaded",
            Event::ProjectWarnings(_) => "project:warnings",
            Event::TransportState(_) => "transport:state",
            Event::EngineStatus(_) => "engine:status",
            Event::ExportProgress(_) => "export:progress",
            Event::ExportRequestProgress { .. } => "export:progress",
            Event::MidiHardwareNote(_) => "midi:note",
        }
    }

    /// The payload as the JSON text the UI receives.
    pub fn payload(&self) -> serde_json::Result<String> {
        match self {
            Event::ProjectPatch(patch) => serde_json::to_string(patch),
            Event::ProjectLoaded(snapshot) => serde_json::to_string(snapshot),
            Event::ProjectWarnings(warnings) => serde_json::to_string(warnings),
            Event::TransportState(state) => serde_json::to_string(state),
            Event::EngineStatus(status) => serde_json::to_string(status),
            Event::ExportProgress(progress) => serde_json::to_string(progress),
            Event::ExportRequestProgress { request, progress } => {
                #[derive(serde::Serialize)]
                struct Payload<'a> {
                    request: &'a str,
                    #[serde(flatten)]
                    progress: &'a ExportProgress,
                }
                serde_json::to_string(&Payload { request, progress })
            }
            Event::MidiHardwareNote(note) => serde_json::to_string(note),
        }
    }
}

/// Where the session sends its events. The app gives it one that emits to
/// every window; tests give it one that records.
///
/// `emit` is called while the session holds its locks, which is what keeps
/// events in the order the changes happened. It must hand the event off and
/// return, and must not call back into the session.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: Event);
}
