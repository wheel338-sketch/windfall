//! Reviewable bounded command batches. No provider gets mutable document access.
use crate::{Command, Document, SettingsPatch};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_PROPOSAL_COMMANDS: usize = 128;
pub const MAX_PROPOSAL_BYTES: usize = 524_288;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ProposalStamp {
    #[ts(type = "number")]
    pub generation: u64,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct CommandProposal {
    pub stamp: ProposalStamp,
    pub label: String,
    pub explanation: String,
    pub commands: Vec<Command>,
}

impl CommandProposal {
    /// Deliberately narrower than Command: no filesystem, project replacement,
    /// opaque plugin state or recursively nested batches. Add capabilities here
    /// only when review and resource bounds for that command are defined.
    pub fn batch(&self) -> Result<Command, String> {
        if self.label.trim().is_empty() || self.label.len() > 128 || self.explanation.len() > 4096 {
            return Err("proposal label/explanation limit exceeded".into());
        }
        if self.commands.is_empty() || self.commands.len() > MAX_PROPOSAL_COMMANDS {
            return Err("proposal must contain 1..128 commands".into());
        }
        for command in &self.commands {
            match command {
                Command::UpdateSettings { patch }
                    if patch
                        == &(SettingsPatch {
                            tempo_bpm: patch.tempo_bpm,
                            ..SettingsPatch::default()
                        }) => {}
                Command::AddPattern { .. }
                | Command::UpdatePattern { .. }
                | Command::AddChannel { .. }
                | Command::UpdateChannel { .. }
                | Command::AddNotes { .. }
                | Command::RemoveNotes { .. }
                | Command::UpdateCapturedNotes { .. }
                | Command::UpdateMixerTrack { .. }
                | Command::SetInstrumentParam { .. }
                | Command::SetEffectParam { .. }
                | Command::SetPluginParam { .. }
                | Command::SetTrackParam { .. }
                | Command::SetTrackOutput { .. }
                | Command::SetSend { .. }
                | Command::SetSidechain { .. }
                | Command::UpdateEffect { .. }
                | Command::SetScriptLibrary { .. } => (),
                _ => return Err("command is outside the proposal capability registry".into()),
            }
        }
        // Serialized size bounds large note arrays and script/library payloads
        // as well as the command count. The JSON decoder still owns its IPC limit.
        struct Bound(usize);
        impl std::io::Write for Bound {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.saturating_add(bytes.len());
                if self.0 > MAX_PROPOSAL_BYTES {
                    return Err(std::io::Error::other("proposal byte limit exceeded"));
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        serde_json::to_writer(Bound(0), self).map_err(|e| e.to_string())?;
        Ok(Command::Batch {
            label: Some(self.label.clone()),
            commands: self.commands.clone(),
        })
    }

    pub fn validate(
        &self,
        document: &Document,
        current: ProposalStamp,
        recording: bool,
    ) -> Result<Command, String> {
        if recording {
            return Err("proposals cannot apply while recording".into());
        }
        if self.stamp != current || current.revision != document.revision() {
            return Err("proposal is stale; generate a new preview".into());
        }
        let batch = self.batch()?;
        let mut candidate = document.clone();
        let applied = candidate
            .dispatch(batch.clone(), None)
            .map_err(|e| e.to_string())?;
        if applied.touched.is_empty() {
            return Err(
                "proposal produces no changes; the requested values are already set".into(),
            );
        }
        candidate.project().check()?;
        Ok(batch)
    }

    pub fn apply(
        &self,
        document: &mut Document,
        current: ProposalStamp,
        recording: bool,
    ) -> Result<crate::Applied, String> {
        let batch = self.validate(document, current, recording)?;
        document.dispatch(batch, None).map_err(|e| e.to_string())
    }
}
