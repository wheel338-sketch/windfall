//! Portable impulse assets, independent of Copy/automatable scalar settings.
use crate::{EffectId, EffectParams, EffectSlot};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConvolverImpulse {
    pub name: String,
    pub sample_rate: u32,
    /// Owned user/generated PCM. Mono import duplicates channels explicitly.
    pub frames: Vec<[f32; 2]>,
}
impl ConvolverImpulse {
    pub fn check(&self) -> Result<(), &'static str> {
        if self.name.is_empty() || self.name.len() > 256 {
            return Err("IR name must contain 1 to 256 bytes");
        }
        if !(8000..=384000).contains(&self.sample_rate) {
            return Err("IR sample rate must be 8000 to 384000 Hz");
        }
        if self.frames.is_empty() || self.frames.len() > windfall_dsp::spectral::MAX_IMPULSE_SAMPLES
        {
            return Err("IR must contain 1 to 65536 frames; trim longer files explicitly");
        }
        if self
            .frames
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 8.0)
        {
            return Err("IR samples must be finite within -8 to 8");
        }
        Ok(())
    }
    /// Rate conversion occurs before processor publication, never in process.
    pub fn at_rate(&self, rate: u32) -> Result<Vec<[f32; 2]>, &'static str> {
        self.check()?;
        let length =
            (self.frames.len() as u64 * u64::from(rate)).div_ceil(u64::from(self.sample_rate));
        if length == 0 || length > windfall_dsp::spectral::MAX_IMPULSE_SAMPLES as u64 {
            return Err("resampled IR exceeds the 65536-frame runtime limit");
        }
        Ok((0..length as usize)
            .map(|n| {
                let at = n as f64 * f64::from(self.sample_rate) / f64::from(rate);
                let a = (at as usize).min(self.frames.len() - 1);
                let b = (a + 1).min(self.frames.len() - 1);
                std::array::from_fn(|ch| {
                    self.frames[a][ch]
                        + (self.frames[b][ch] - self.frames[a][ch]) * at.fract() as f32
                })
            })
            .collect())
    }
}

/// Migrate the former inline 2048-sample mono payload when opening old files.
/// The new serializer writes it only in the dedicated asset, never in params.
impl<'de> Deserialize<'de> for EffectSlot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: EffectId,
            enabled: bool,
            mix: f32,
            params: serde_json::Value,
            #[serde(default)]
            impulse: Option<ConvolverImpulse>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let params: EffectParams =
            serde_json::from_value(wire.params.clone()).map_err(serde::de::Error::custom)?;
        let mut impulse = wire.impulse;
        if impulse.is_none() && params.kind() == crate::EffectKind::Convolver {
            let length = wire
                .params
                .get("impulseLength")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            if length > 2048 {
                return Err(serde::de::Error::custom("legacy IR exceeds 2048 frames"));
            }
            if length > 0 {
                let samples: Vec<f32> =
                    serde_json::from_value(wire.params.get("impulse").cloned().unwrap_or_default())
                        .map_err(serde::de::Error::custom)?;
                let frames = (0..length)
                    .map(|i| [samples.get(i).copied().unwrap_or(0.0); 2])
                    .collect();
                impulse = Some(ConvolverImpulse {
                    name: "Imported legacy impulse".into(),
                    sample_rate: 48000,
                    frames,
                });
            }
        }
        if let Some(asset) = &impulse {
            asset.check().map_err(serde::de::Error::custom)?;
        }
        Ok(Self {
            id: wire.id,
            enabled: wire.enabled,
            mix: wire.mix,
            params,
            impulse,
        })
    }
}
