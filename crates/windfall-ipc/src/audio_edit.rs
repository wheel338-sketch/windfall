//! An editor token identifies one immutable rendered clip view, never a file to overwrite.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{ClipId, ClipStretchQuality};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditPreview {
    pub token: u32,
    pub clip: ClipId,
    pub name: String,
    pub frames: u32,
    pub sample_rate: u32,
    /// Stereo min/max envelope, retaining opposite-phase channels.
    pub peaks: Vec<f32>,
    /// 128 time columns × 64 linear-frequency bins, normalized -96..0 dBFS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<f32>>", optional)]
    pub spectral: Vec<f32>,
    pub regions: Vec<windfall_project::AudioRegion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioAnalysisPreview {
    pub token: u32,
    pub recipe: windfall_project::AudioEditRecipe,
    /// Envelope of the preserved analysis input, not the derived clip view.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<f32>>", optional)]
    pub source_peaks: Vec<f32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioAnalysisEdit {
    pub token: u32,
    pub recipe: windfall_project::AudioEditRecipe,
}

/// A second frame range of the same clip view: [start, end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditRange {
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct AudioScriptRequest {
    #[ts(type = "number")]
    pub request: u64,
    pub token: u32,
    pub range: AudioEditRange,
    pub source: String,
    #[serde(default)]
    pub parameters: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioScriptPreview {
    #[ts(type = "number")]
    pub request: u64,
    pub token: u32,
    pub range: AudioEditRange,
    pub frames: u32,
    pub sample_rate: u32,
    pub peaks: Vec<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioNormalizeMode {
    #[default]
    Peak,
    /// Root mean square of the selection across its channels.
    Rms,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioFadeDirection {
    #[default]
    In,
    Out,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioFadeShape {
    /// Playback's sine fade law, as used by `FadeIn`/`FadeOut`.
    #[default]
    EqualPower,
    Linear,
    /// Raised cosine.
    SCurve,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioEditChannels {
    #[default]
    Both,
    Left,
    Right,
}

/// Unit variants keep their original wire names. Every field of a parameter
/// variant has a default, so `{"declick": {}}` still parses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum AudioEditOperation {
    Trim,
    Extract,
    /// Peak, -1 dBFS. `NormalizeTo` carries a target and mode.
    Normalize,
    Reverse,
    /// Equal-power. `Fade` carries a shape.
    FadeIn,
    FadeOut,
    Silence,
    Cut,
    /// Subtracts each channel's mean over the selection.
    RemoveDc,
    Declick {
        /// 0 (only gross impulses) to 1 (most sensitive).
        #[serde(default = "default_strength")]
        strength: f32,
    },
    ReduceNoise {
        /// Largest attenuation of a noise-only band, in dB.
        #[serde(default = "default_reduction_db")]
        reduction_db: f32,
        /// Noise-only range to learn from. Absent: estimate from the selection.
        #[serde(default)]
        profile: Option<AudioEditRange>,
    },
    /// Removes the selection's leading and trailing frames below the threshold.
    TrimSilence {
        #[serde(default = "default_threshold_db")]
        threshold_db: f32,
    },
    /// Refused, rather than clipped, when a boost would exceed full scale.
    Gain {
        #[serde(default)]
        db: f32,
    },
    NormalizeTo {
        #[serde(default = "default_target_db")]
        target_db: f32,
        #[serde(default)]
        mode: AudioNormalizeMode,
    },
    Fade {
        #[serde(default)]
        direction: AudioFadeDirection,
        #[serde(default)]
        shape: AudioFadeShape,
    },
    SwapChannels,
    /// Writes the average of left and right to both.
    MonoSum,
    InvertPhase {
        #[serde(default)]
        channels: AudioEditChannels,
    },
    /// Spectral time-stretch and/or pitch-shift of the selection.
    Stretch {
        /// Output duration divided by selected duration, 0.25 to 4.
        #[serde(default = "default_ratio")]
        ratio: f64,
        /// -24 to 24.
        #[serde(default)]
        semitones: f32,
        #[serde(default)]
        quality: ClipStretchQuality,
        #[serde(default)]
        formants: bool,
    },
    /// Inserts silence before the selection's first frame.
    InsertSilence {
        #[serde(default)]
        frames: u32,
    },
    Equalize {
        low_db: f32,
        mid_db: f32,
        high_db: f32,
    },
    /// Spectral magnitude blur, 0..1, with original phases retained.
    Blur {
        amount: f32,
    },
    /// Captured impulse range of this immutable clip view (maximum 32768 frames).
    Convolve {
        impulse: AudioEditRange,
        wet: f32,
    },
    RepairClipping {
        threshold: f32,
    },
    LoopTune {
        crossfade_frames: u32,
    },
    /// Split at silent runs, retaining their timeline positions.
    StripSilence {
        threshold_db: f32,
        min_silence_frames: u32,
    },
    /// Only the sandbox worker's selection entry point may render this operation.
    Script,
}

fn default_strength() -> f32 {
    0.5
}
fn default_reduction_db() -> f32 {
    12.0
}
fn default_threshold_db() -> f32 {
    -60.0
}
fn default_target_db() -> f32 {
    -1.0
}
fn default_ratio() -> f64 {
    1.0
}

impl AudioEditOperation {
    /// Names the undo step and the derived sample.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Trim => "Trim",
            Self::Extract => "Extract",
            Self::Normalize | Self::NormalizeTo { .. } => "Normalize",
            Self::Reverse => "Reverse",
            Self::FadeIn => "FadeIn",
            Self::FadeOut => "FadeOut",
            Self::Fade {
                direction: AudioFadeDirection::In,
                ..
            } => "FadeIn",
            Self::Fade { .. } => "FadeOut",
            Self::Silence => "Silence",
            Self::Cut => "Cut",
            Self::RemoveDc => "Remove DC",
            Self::Declick { .. } => "De-click",
            Self::ReduceNoise { .. } => "Reduce noise",
            Self::TrimSilence { .. } => "Trim silence",
            Self::Gain { .. } => "Gain",
            Self::SwapChannels => "Swap channels",
            Self::MonoSum => "Mono sum",
            Self::InvertPhase { .. } => "Invert phase",
            Self::Stretch { .. } => "Stretch",
            Self::InsertSilence { .. } => "Insert silence",
            Self::Equalize { .. } => "Equalize",
            Self::Blur { .. } => "Spectral blur",
            Self::Convolve { .. } => "Convolve",
            Self::RepairClipping { .. } => "Repair clipping",
            Self::LoopTune { .. } => "Tune loop",
            Self::StripSilence { .. } => "Strip silence",
            Self::Script => "Script selection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditRequest {
    pub token: u32,
    pub operation: AudioEditOperation,
    /// Inclusive first frame and exclusive end frame of the clip view.
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AudioEditDestination {
    Sampler {
        channel: windfall_project::ChannelId,
    },
    Playlist {
        track: Option<windfall_project::PlaylistTrackId>,
        start: u32,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditTransferRequest {
    pub token: u32,
    pub start_frame: u32,
    pub end_frame: u32,
    pub destination: AudioEditDestination,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(operation: &str) -> AudioEditOperation {
        serde_json::from_str::<AudioEditRequest>(&format!(
            r#"{{"token":7,"operation":{operation},"startFrame":1,"endFrame":3}}"#
        ))
        .unwrap()
        .operation
    }

    #[test]
    fn audio_edit_requests_from_before_parameters_still_parse() {
        for (wire, operation) in [
            ("trim", AudioEditOperation::Trim),
            ("extract", AudioEditOperation::Extract),
            ("normalize", AudioEditOperation::Normalize),
            ("reverse", AudioEditOperation::Reverse),
            ("fadeIn", AudioEditOperation::FadeIn),
            ("fadeOut", AudioEditOperation::FadeOut),
            ("silence", AudioEditOperation::Silence),
            ("cut", AudioEditOperation::Cut),
        ] {
            assert_eq!(parse(&format!("\"{wire}\"")), operation);
            assert_eq!(
                serde_json::to_string(&operation).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn audio_edit_parameters_default_when_absent_and_use_camel_case() {
        assert_eq!(
            parse(r#"{"declick":{}}"#),
            AudioEditOperation::Declick { strength: 0.5 }
        );
        assert_eq!(
            parse(r#"{"reduceNoise":{}}"#),
            AudioEditOperation::ReduceNoise {
                reduction_db: 12.0,
                profile: None
            }
        );
        assert_eq!(
            parse(r#"{"reduceNoise":{"reductionDb":20,"profile":{"startFrame":4,"endFrame":9}}}"#),
            AudioEditOperation::ReduceNoise {
                reduction_db: 20.0,
                profile: Some(AudioEditRange {
                    start_frame: 4,
                    end_frame: 9
                })
            }
        );
        assert_eq!(
            parse(r#"{"trimSilence":{}}"#),
            AudioEditOperation::TrimSilence {
                threshold_db: -60.0
            }
        );
        assert_eq!(
            parse(r#"{"gain":{}}"#),
            AudioEditOperation::Gain { db: 0.0 }
        );
        assert_eq!(
            parse(r#"{"normalizeTo":{}}"#),
            AudioEditOperation::NormalizeTo {
                target_db: -1.0,
                mode: AudioNormalizeMode::Peak
            }
        );
        assert_eq!(
            parse(r#"{"normalizeTo":{"targetDb":-18,"mode":"rms"}}"#),
            AudioEditOperation::NormalizeTo {
                target_db: -18.0,
                mode: AudioNormalizeMode::Rms
            }
        );
        assert_eq!(
            parse(r#"{"fade":{}}"#),
            AudioEditOperation::Fade {
                direction: AudioFadeDirection::In,
                shape: AudioFadeShape::EqualPower
            }
        );
        assert_eq!(
            parse(r#"{"fade":{"direction":"out","shape":"sCurve"}}"#),
            AudioEditOperation::Fade {
                direction: AudioFadeDirection::Out,
                shape: AudioFadeShape::SCurve
            }
        );
        assert_eq!(
            parse(r#"{"invertPhase":{}}"#),
            AudioEditOperation::InvertPhase {
                channels: AudioEditChannels::Both
            }
        );
        assert_eq!(
            parse(r#"{"stretch":{}}"#),
            AudioEditOperation::Stretch {
                ratio: 1.0,
                semitones: 0.0,
                quality: ClipStretchQuality::Standard,
                formants: false
            }
        );
        assert_eq!(
            parse(r#"{"insertSilence":{"frames":48}}"#),
            AudioEditOperation::InsertSilence { frames: 48 }
        );
        for (wire, operation) in [
            ("removeDc", AudioEditOperation::RemoveDc),
            ("swapChannels", AudioEditOperation::SwapChannels),
            ("monoSum", AudioEditOperation::MonoSum),
        ] {
            assert_eq!(parse(&format!("\"{wire}\"")), operation);
        }
        assert_eq!(
            serde_json::to_string(&AudioEditOperation::ReduceNoise {
                reduction_db: 6.0,
                profile: None
            })
            .unwrap(),
            r#"{"reduceNoise":{"reductionDb":6.0,"profile":null}}"#
        );
    }
}
