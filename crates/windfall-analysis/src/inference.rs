//! Operation-specific adapter contract. A model import is never an algorithm
//! installation; adapters opt into each operation and both dereverb modes.
use crate::{AnalysisError, ModelManifest, Result};
use serde::{Deserialize, Serialize};

/// Future neural score adapters share the real reviewed note shape. They must
/// return selected-range-local frame coordinates and validate preprocessing,
/// weights and postprocessing against a pinned upstream reference. No neural
/// note implementation is installed by declaring this boundary.
pub trait NoteInferenceAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn detect(
        &self,
        input: &crate::CapturedInput,
        model: &crate::VerifiedModel,
        work: &mut crate::Work,
    ) -> Result<windfall_project::AudioAnalysisData>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceTask {
    #[default]
    Audio,
    StemSeparation,
    DereverbGeneral,
    DereverbVocal,
}
impl InferenceTask {
    pub fn validate(self, model: &ModelManifest) -> Result<()> {
        let roles: Vec<_> = model.outputs.iter().map(|o| o.role.as_str()).collect();
        let valid = match self {
            Self::Audio => true,
            Self::StemSeparation => {
                roles.len() == 4
                    && ["drums", "bass", "vocals", "other"]
                        .iter()
                        .all(|r| roles.contains(r))
            }
            Self::DereverbGeneral => roles == ["dereverb-general"],
            Self::DereverbVocal => roles == ["dereverb-vocal"],
        };
        if !valid
            || (self != Self::Audio
                && model
                    .outputs
                    .iter()
                    .any(|o| o.channels != model.input_channels))
        {
            return Err(AnalysisError::Invalid(
                "operation output roles/channels do not match model pack",
            ));
        }
        Ok(())
    }
}
