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
    #[serde(default, skip_serializing_if = "is_zero_delay")]
    #[ts(as = "Option<u32>", optional)]
    pub group_delay: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub eq_gains: Option<[f32; 3]>,
    /// Owned user/generated PCM. Mono import duplicates channels explicitly.
    pub frames: Vec<[f32; 2]>,
}
fn is_zero_delay(delay: &u32) -> bool {
    *delay == 0
}
impl ConvolverImpulse {
    pub fn group_delay_at(&self, rate: u32) -> usize {
        (f64::from(self.group_delay) * f64::from(rate) / f64::from(self.sample_rate.max(1))).round()
            as usize
    }
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
        if self
            .eq_gains
            .is_some_and(|gains| gains.iter().any(|v| !v.is_finite() || v.abs() > 24.0))
        {
            return Err("EQ gains must be finite within -24 to 24 dB");
        }
        if self.group_delay != 0 {
            let delay = self.group_delay as usize;
            if delay.checked_mul(2).and_then(|n| n.checked_add(1)) != Some(self.frames.len())
                || self
                    .frames
                    .iter()
                    .zip(self.frames.iter().rev())
                    .any(|(a, b)| a.iter().zip(b).any(|(a, b)| (a - b).abs() > 1e-6))
            {
                return Err("linear-phase IR delay requires an odd symmetric kernel");
            }
        }
        Ok(())
    }
    /// Rate conversion occurs before processor publication, never in process.
    pub fn at_rate(&self, rate: u32) -> Result<Vec<[f32; 2]>, &'static str> {
        self.check()?;
        if !(8000..=384000).contains(&rate) {
            return Err("destination IR rate must be 8000 to 384000 Hz");
        }
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
        if impulse.is_none() {
            impulse = legacy_impulse(&wire.params).map_err(serde::de::Error::custom)?;
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

/// Extract legacy PCM before preset scalar migration removes obsolete fields.
fn legacy_impulse(params: &serde_json::Value) -> Result<Option<ConvolverImpulse>, String> {
    if params.get("type").and_then(|v| v.as_str()) != Some("convolver") {
        return Ok(None);
    }
    let length = params
        .get("impulseLength")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if length > 2048 {
        return Err("legacy IR exceeds 2048 frames".into());
    }
    if length == 0 {
        return Ok(None);
    }
    let samples: Vec<f32> =
        serde_json::from_value(params.get("impulse").cloned().unwrap_or_default())
            .map_err(|error| error.to_string())?;
    let asset = ConvolverImpulse {
        name: "Imported legacy impulse".into(),
        sample_rate: 48000,
        group_delay: 0,
        eq_gains: None,
        frames: (0..length as usize)
            .map(|i| [samples.get(i).copied().unwrap_or(0.0); 2])
            .collect(),
    };
    asset.check().map_err(str::to_owned)?;
    Ok(Some(asset))
}
pub(crate) fn migrate_impulse_holder(holder: &mut serde_json::Value) -> Result<(), String> {
    if holder.get("impulse").is_none_or(serde_json::Value::is_null)
        && let Some(asset) = holder
            .get("params")
            .map(legacy_impulse)
            .transpose()?
            .flatten()
    {
        holder
            .as_object_mut()
            .ok_or("IR holder must be an object")?
            .insert(
                "impulse".into(),
                serde_json::to_value(asset).map_err(|error| error.to_string())?,
            );
    }
    Ok(())
}
