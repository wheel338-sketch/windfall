//! Admission for file/picker edits after asynchronous dialogs and native discovery.
use super::Session;
use windfall_project::{Command, DispatchResult};
impl Session {
    pub(crate) fn dispatch_file_command(
        &self,
        command: Command,
        generation: u64,
        revision: u64,
    ) -> Result<DispatchResult, String> {
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err(
                    "The project changed while the file or plugin was chosen; try again".into(),
                );
            }
            self.sample_edit_ticket(&state, command, None, Vec::new())?
                .ordinary()
        };
        self.finish_sample_edit(ticket)
    }
    pub fn preset_apply_plugin(
        &self,
        path: &str,
        target: windfall_project::PluginTarget,
        generation: u64,
        revision: u64,
    ) -> Result<DispatchResult, String> {
        let review = self.preset_read(path)?;
        let preset = review
            .preset
            .plugin
            .ok_or("Choose a plugin preset for this destination")?;
        let expected =
            windfall_project::PluginPreset::capture(&self.document_snapshot().project, target)?
                .state;
        self.dispatch_file_command(
            Command::ApplyPluginPreset {
                target,
                expected,
                preset,
            },
            generation,
            revision,
        )
    }
}
