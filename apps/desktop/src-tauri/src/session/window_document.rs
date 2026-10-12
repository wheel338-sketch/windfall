//! Transient native-window document identity; never part of the project format.
use super::Session;
use serde::Serialize;
use windfall_project::{Command, DispatchResult, DocumentSnapshot};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowDocumentSnapshot {
    pub generation: u64,
    pub snapshot: DocumentSnapshot,
}

impl Session {
    pub fn window_document_snapshot(&self) -> WindowDocumentSnapshot {
        let state = self.state();
        WindowDocumentSnapshot {
            generation: state.generation,
            snapshot: state.document.snapshot(state.path_text()),
        }
    }

    /// Event emission already holds State. Read only its committed atomic
    /// mirror here, so the native event sink never recursively locks State.
    pub fn window_document_generation(&self) -> u64 {
        self.inner
            .window_document_generation
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn dispatch_from_window(
        &self,
        generation: u64,
        command: Command,
        gesture: Option<u64>,
    ) -> Result<DispatchResult, String> {
        let tweaks = self.controller_tweak_probe(&command);
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation {
                return Err(super::PROJECT_REPLACED.into());
            }
            self.sample_edit_ticket(&state, command, gesture, Vec::new())?
                .ordinary()
        };
        // The existing prepared commit rechecks this ticket's generation AND
        // replacement watermark under State, including during slow DSP work.
        let result = self.finish_sample_edit(ticket);
        if result.is_ok() {
            self.controller_tweaked_guarded(&tweaks, Some(generation));
        }
        result
    }
}
