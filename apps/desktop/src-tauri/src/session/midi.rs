//! MIDI review, append and snapshot export; slow file work never holds state.

use windfall_ipc::{MidiExportOptions, MidiImportOptions, MidiImportPreview};
use windfall_project::{DispatchResult, file::write_atomic};

use super::{PROJECT_REPLACED, Session};
use crate::paths;

pub(super) struct Prepared {
    token: u32,
    generation: u64,
    plan: windfall_midi::ImportPlan,
}

impl Session {
    /// Reads once and retains the exact plan the UI reviews. A later preview
    /// replaces the previous one; opening another project invalidates it.
    pub fn midi_preview(
        &self,
        path: &str,
        options: MidiImportOptions,
    ) -> Result<MidiImportPreview, String> {
        let (generation, ticket) = {
            let mut state = self.state();
            state.midi_ticket = state.midi_ticket.checked_add(1).ok_or_else(|| {
                windfall_ipc::ui_message("midi.preview.tokensExhausted", serde_json::json!({}))
            })?;
            state.midi_import = None;
            (state.generation, state.midi_ticket)
        };
        let path = paths::absolute(path)?;
        let song = windfall_midi::read_file(path).map_err(windfall_ipc::midi_error_message)?;
        let plan = windfall_midi::import(&song, &options.core());
        #[cfg(test)]
        self.pause("midi:read");
        let mut state = self.state();
        if state.generation != generation {
            return Err(PROJECT_REPLACED.to_owned());
        }
        if state.midi_ticket != ticket {
            return Err(windfall_ipc::ui_message(
                "midi.preview.replaced",
                serde_json::json!({}),
            ));
        }
        let mut trial = state.document.clone();
        trial
            .dispatch(plan.command(trial.project()), None)
            .map_err(|error| error.to_string())?;
        trial.project().check().map_err(|error| error.to_string())?;
        let preview = MidiImportPreview::from_plan(ticket, &plan);
        state.midi_import = Some(Prepared {
            token: ticket,
            generation,
            plan,
        });
        Ok(preview)
    }

    /// Appends the reviewed plan as one checked undo step. Sample changes go
    /// through the ordinary pool loader, including factory drum mappings.
    pub fn import_midi(&self, token: u32) -> Result<DispatchResult, String> {
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let reviewed = state
                .midi_import
                .as_ref()
                .filter(|p| p.token == token)
                .ok_or_else(|| {
                    windfall_ipc::ui_message("midi.preview.expired", serde_json::json!({}))
                })?;
            if reviewed.generation != state.generation {
                return Err(PROJECT_REPLACED.to_owned());
            }
            let command = reviewed.plan.command(state.document.project());
            let mut trial = state.document.clone();
            trial
                .dispatch(command.clone(), None)
                .map_err(|error| error.to_string())?;
            trial.project().check().map_err(|error| error.to_string())?;
            self.sample_edit_ticket(&state, command, None, Vec::new())?
        };
        let mut prepared = ticket.prepare()?;
        let mut retirement = None;
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if state.midi_import.as_ref().is_none_or(|p| p.token != token) {
                return Err(windfall_ipc::ui_message(
                    "midi.preview.expired",
                    serde_json::json!({}),
                ));
            }
            let result = prepared.commit(&mut state, &mut retirement)?;
            state.midi_import = None;
            result
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        Ok(result)
    }

    /// Releases a cancelled review without changing the project.
    pub fn midi_discard(&self, token: u32) {
        let mut state = self.state();
        if state.midi_import.as_ref().is_some_and(|p| p.token == token) {
            state.midi_import = None;
        }
    }

    /// Writes a snapshot atomically. A failed conversion leaves an existing
    /// destination untouched, and export never changes document history.
    pub fn export_midi(&self, path: &str, options: MidiExportOptions) -> Result<String, String> {
        let mut path = paths::absolute(path)?;
        match path.extension().and_then(|ext| ext.to_str()) {
            None => {
                path.set_extension("mid");
            }
            Some(ext) if ext.eq_ignore_ascii_case("mid") || ext.eq_ignore_ascii_case("midi") => {}
            _ => {
                return Err(windfall_ipc::ui_message(
                    "midi.export.extension",
                    serde_json::json!({}),
                ));
            }
        }
        let project = self.state().document.project().clone();
        let bytes = options.bytes(&project)?;
        write_atomic(&path, &bytes).map_err(|error| error.to_string())?;
        Ok(paths::display(&path))
    }
}
