//! Portable rack-node assets. Payloads belong to nodes, never runtime handles
//! or project SampleIds. A missing payload retains its source for relinking.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use crate::{EffectKind, InstrumentKind, RackProcessor};

pub const MAX_RACK_ASSET_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackAudioAsset {
    pub name: String,
    pub sample_rate: u32,
    /// None is a missing source, displayed and relinked by the module editor.
    #[serde(default)]
    pub frames: Option<Vec<[f32; 2]>>,
}
impl RackAudioAsset {
    pub fn check(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > 256 || !(8000..=384000).contains(&self.sample_rate) {
            return Err("Invalid rack audio source name or sample rate".into());
        }
        if let Some(frames) = &self.frames {
            if frames.is_empty() || frames.len() > MAX_RACK_ASSET_BYTES / 8
                || frames.iter().flatten().any(|v| !v.is_finite() || v.abs() > 8.0) {
                return Err("Rack audio source must contain finite PCM within the 64 MiB limit".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackSampleZone {
    pub mapping: windfall_dsp::sample_bank::SampleZone,
    pub audio: RackAudioAsset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum RackNodeAsset {
    Instrument { asset: windfall_dsp::InstrumentAsset },
    Impulse { impulse: crate::ConvolverImpulse },
    Audio { audio: RackAudioAsset },
    Zones { zones: Vec<RackSampleZone> },
    SoundFont {
        name: String,
        bank: u16,
        program: u16,
        #[serde(default)]
        bytes: Option<Vec<u8>>,
    },
}
impl RackNodeAsset {
    pub fn check(&self, processor: &RackProcessor) -> Result<(), String> {
        match (self, processor) {
            (Self::Instrument { asset }, RackProcessor::Instrument { params }) => asset.validate(params.kind()).map_err(str::to_owned),
            (Self::Impulse { impulse }, RackProcessor::Effect { params, .. }) if params.kind() == EffectKind::Convolver => impulse.check().map_err(str::to_owned),
            (Self::Audio { audio }, RackProcessor::Effect { params, .. }) if params.kind() == EffectKind::Scratch => audio.check(),
            (Self::Zones { zones }, RackProcessor::Instrument { params }) if matches!(params.kind(), InstrumentKind::MultiSampler | InstrumentKind::SamplePlayer | InstrumentKind::PadBank | InstrumentKind::SoundFont | InstrumentKind::PianoKeys | InstrumentKind::TextVoice) => {
                if zones.is_empty() || zones.len() > windfall_dsp::sample_bank::MAX_BANK_ZONES {
                    return Err("Rack sample bank must contain 1 to 256 zones".into());
                }
                for zone in zones {
                    zone.mapping.validate().map_err(str::to_owned)?;
                    // No reference to another project's sample namespace survives.
                    if zone.mapping.sample != 0 { return Err("Rack zones use owned audio, not project sample IDs".into()); }
                    zone.audio.check()?;
                    if let Some(frames) = &zone.audio.frames {
                        if zone.mapping.loop_end as usize > frames.len() { return Err("Rack zone loop exceeds its owned sample".into()); }
                    }
                }
                Ok(())
            }
            (Self::SoundFont { name, bank, program, bytes }, RackProcessor::Instrument { params }) if params.kind() == InstrumentKind::SoundFont => {
                if name.is_empty() || name.len() > 256 || *bank > 16383 || *program > 127
                    || bytes.as_ref().is_some_and(|bytes| bytes.is_empty() || bytes.len() > MAX_RACK_ASSET_BYTES) {
                    return Err("Invalid rack SoundFont reference or payload".into());
                }
                Ok(())
            }
            _ => Err("The rack processor cannot own this asset type".into()),
        }
    }
    pub fn payload_bytes(&self) -> usize {
        match self {
            Self::Audio { audio } => audio.frames.as_ref().map_or(0, |f| f.len() * 8),
            Self::Zones { zones } => zones.iter().map(|z| z.audio.frames.as_ref().map_or(0, |f| f.len() * 8)).sum(),
            Self::SoundFont { bytes, .. } => bytes.as_ref().map_or(0, Vec::len),
            Self::Impulse { impulse } => impulse.frames.len() * 8,
            // Serialization size is also bounded by the instrument's own checks.
            Self::Instrument { asset } => serde_json::to_vec(asset).map_or(MAX_RACK_ASSET_BYTES + 1, |v| v.len()),
        }
    }
    pub fn missing_sources(&self) -> Vec<String> {
        match self {
            Self::Audio { audio } if audio.frames.is_none() => vec![audio.name.clone()],
            Self::Zones { zones } => zones.iter().filter(|z| z.audio.frames.is_none()).map(|z| z.audio.name.clone()).collect(),
            Self::SoundFont { name, bytes: None, .. } => vec![name.clone()],
            _ => Vec::new(),
        }
    }
}
