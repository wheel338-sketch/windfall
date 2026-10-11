//! Portable processing state; project routing and hardware remain destination-owned.
use crate::{
    EffectSlot, MAX_EFFECT_SLOTS, MAX_GAIN, MixerTrack, PluginBinding, PluginTarget, Project,
    TrackId,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_dsp::ParamSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MixerTrackPreset {
    pub version: u32,
    pub name: String,
    pub color: u32,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub processing: windfall_dsp::TrackParams,
    pub latency_offset_ms: f64,
    pub effects: Vec<EffectSlot>,
    pub plugins: Vec<PluginBinding>,
}

impl MixerTrackPreset {
    pub fn capture(project: &Project, id: TrackId) -> Result<Self, String> {
        let track = project
            .mixer
            .track(id)
            .ok_or("The preset source track no longer exists")?;
        let preset = Self {
            version: 1, name: track.name.clone(), color: track.color, volume: track.volume,
            pan: track.pan, muted: track.muted, processing: track.processing,
            latency_offset_ms: track.latency_offset_ms, effects: track.effects.clone(),
            plugins: project.plugins.iter().filter(|plugin| matches!(plugin.target, PluginTarget::Effect { effect } if track.effects.iter().any(|slot| slot.id == effect))).cloned().collect(),
        };
        preset.validate()?;
        Ok(preset)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported mixer preset version".into());
        }
        if self.name.len() > 4096
            || self.name.chars().any(char::is_control)
            || self.color > 0xffffff
            || !self.volume.is_finite()
            || !(0.0..=MAX_GAIN).contains(&self.volume)
            || !self.pan.is_finite()
            || !(-1.0..=1.0).contains(&self.pan)
            || !self.latency_offset_ms.is_finite()
            || self.latency_offset_ms.abs() > 1000.0
            || self.processing.sanitized() != self.processing
        {
            return Err("Mixer preset contains invalid track settings".into());
        }
        if self.effects.len() > MAX_EFFECT_SLOTS || self.plugins.len() > self.effects.len() {
            return Err("Mixer preset exceeds the effect slot limit".into());
        }
        let mut ids = std::collections::HashSet::new();
        for effect in &self.effects {
            if let Some(asset) = &effect.impulse {
                if effect.kind() != crate::EffectKind::Convolver { return Err("Only convolver slots can hold an IR".into()); }
                asset.check().map_err(str::to_owned)?;
            }
            if effect.id.0 == 0
                || !ids.insert(effect.id)
                || !effect.mix.is_finite()
                || !(0.0..=1.0).contains(&effect.mix)
                || effect.params.sanitized() != effect.params
            {
                return Err("Mixer preset contains invalid effect settings or IDs".into());
            }
        }
        let mut targets = std::collections::HashSet::new();
        let mut bytes = 0usize;
        for plugin in &self.plugins {
            plugin.validate().map_err(str::to_owned)?;
            if !matches!(plugin.target, PluginTarget::Effect { effect } if ids.contains(&effect))
                || !targets.insert(plugin.target)
            {
                return Err("Preset plugin does not belong to a unique effect slot".into());
            }
            bytes = bytes.saturating_add(plugin.state.len());
        }
        if bytes > 256 * 1024 * 1024 {
            return Err("Preset plugin states exceed 256 MiB".into());
        }
        Ok(())
    }

    pub(crate) fn settings_into(&self, track: &mut MixerTrack, name_color: bool) {
        if name_color {
            track.name = self.name.clone();
            track.color = self.color;
        }
        track.volume = self.volume;
        track.pan = self.pan;
        track.muted = self.muted;
        track.processing = self.processing;
        track.latency_offset_ms = self.latency_offset_ms;
    }
}
