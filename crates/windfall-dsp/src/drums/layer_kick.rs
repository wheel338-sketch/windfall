//! Swept kick plus an optional immutable PCM layer, distinct from core Kick.
use super::{Kick, KickParams};
use crate::blocks::smooth::LinearRamp;
use crate::instrument_asset::LayerPcm;
use crate::param::param_set;
use crate::{Instrument, ParamSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LayerKickParams {
    pub pitch_hz: f32,
    pub decay_ms: f32,
    pub pitch_drop_semitones: f32,
    pub pitch_decay_ms: f32,
    pub click: f32,
    pub level: f32,
    pub sample_mix: f32,
    pub sample_tune: f32,
    pub sample_start: f32,
}
impl Default for LayerKickParams {
    fn default() -> Self {
        let kick = KickParams::default();
        Self {
            pitch_hz: kick.pitch_hz,
            decay_ms: kick.decay_ms,
            pitch_drop_semitones: kick.pitch_drop_semitones,
            pitch_decay_ms: kick.pitch_decay_ms,
            click: kick.click,
            level: kick.level,
            sample_mix: 0.25,
            sample_tune: 0.0,
            sample_start: 0.0,
        }
    }
}
param_set!(LayerKickParams, "Layer Kick", {
    float [pitch_hz] "pitchHz" "Body Pitch" { Hertz, Logarithmic, 20.0, 250.0, 52.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Logarithmic, 10.0, 1500.0, 420.0 }
    float [pitch_drop_semitones] "pitchDropSemitones" "Pitch Drop" { Semitones, Linear, 0.0, 60.0, 36.0 }
    float [pitch_decay_ms] "pitchDecayMs" "Pitch Decay" { Milliseconds, Logarithmic, 1.0, 150.0, 18.0 }
    float [click] "click" "Click" { Fraction, Linear, 0.0, 1.0, 0.25 }
    float [level] "level" "Level" { Gain, Linear, 0.0, 1.0, 0.8 }
    float [sample_mix] "sampleMix" "Sample Mix" { Fraction, Linear, 0.0, 1.0, 0.25 }
    float [sample_tune] "sampleTune" "Sample Tune" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [sample_start] "sampleStart" "Sample Start" { Fraction, Linear, 0.0, 0.99, 0.0 }
});
#[derive(Clone, Copy)]
struct LayerVoice {
    active: bool,
    key: u8,
    position: f64,
    step: f64,
    velocity: f32,
    order: u64,
    fade: LinearRamp,
    correction: LinearRamp,
    last: f32,
}
impl Default for LayerVoice {
    fn default() -> Self {
        Self {
            active: false,
            key: 0,
            position: 0.0,
            step: 1.0,
            velocity: 0.0,
            order: 0,
            fade: LinearRamp::new(1.0),
            correction: LinearRamp::new(0.0),
            last: 0.0,
        }
    }
}
pub struct LayerKick {
    kick: Kick,
    params: LayerKickParams,
    pcm: Option<Box<LayerPcm>>,
    voices: [LayerVoice; 8],
    rate: f32,
    serial: u64,
    mix: LinearRamp,
    level: LinearRamp,
    fresh: bool,
}
impl Default for LayerKick {
    fn default() -> Self {
        Self {
            kick: Kick::default(),
            params: LayerKickParams::default(),
            pcm: None,
            voices: [LayerVoice::default(); 8],
            rate: 48_000.0,
            serial: 0,
            mix: LinearRamp::new(0.25),
            level: LinearRamp::new(0.8),
            fresh: true,
        }
    }
}
impl LayerKick {
    /// Control-side only. No PCM enters the Copy parameter struct.
    pub(crate) fn set_layer(&mut self, pcm: &LayerPcm) {
        self.pcm = Some(Box::new(pcm.clone()));
    }
}
impl Instrument for LayerKick {
    type Params = LayerKickParams;
    fn prepare(&mut self, rate: f32, max_block: usize) {
        self.rate = crate::blocks::math::clean(rate, 8000.0, 192000.0, 48000.0);
        self.kick.prepare(self.rate, max_block);
        let params = self.params;
        self.set_params(&params);
        self.reset();
    }
    fn reset(&mut self) {
        self.kick.reset();
        self.voices = [LayerVoice::default(); 8];
        self.serial = 0;
        self.mix.snap(self.params.sample_mix);
        self.level.snap(self.params.level);
        self.fresh = true;
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        self.kick.set_params(&KickParams {
            pitch_hz: self.params.pitch_hz,
            decay_ms: self.params.decay_ms,
            pitch_drop_semitones: self.params.pitch_drop_semitones,
            pitch_decay_ms: self.params.pitch_decay_ms,
            click: self.params.click,
            level: 1.0,
            ..Default::default()
        });
        if self.fresh {
            self.mix.snap(self.params.sample_mix);
            self.level.snap(self.params.level);
        } else {
            let span = (self.rate * 0.005) as u32;
            self.mix.set_target(self.params.sample_mix, span);
            self.level.set_target(self.params.level, span);
        }
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        if key > 127 {
            return;
        }
        if !velocity.is_finite() || velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        self.kick.note_on(key, velocity);
        let Some(pcm) = &self.pcm else {
            return;
        };
        let index = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.order)
                    .map_or(0, |(i, _)| i)
            });
        self.serial = self.serial.wrapping_add(1);
        let position = f64::from(self.params.sample_start) * pcm.samples.len() as f64;
        let mut correction = LinearRamp::new(0.0);
        if self.voices[index].active {
            correction.snap(self.voices[index].last - pcm.at(position) * velocity.min(1.0));
            correction.set_target(0.0, (self.rate * 0.004).max(1.0) as u32);
        }
        self.voices[index] = LayerVoice {
            active: true,
            key,
            position,
            step: f64::from(pcm.sample_rate) / f64::from(self.rate)
                * (f64::from(self.params.sample_tune) / 12.0).exp2(),
            velocity: velocity.min(1.0),
            order: self.serial,
            fade: LinearRamp::new(1.0),
            correction,
            last: 0.0,
        };
    }
    fn note_off(&mut self, key: u8) {
        self.kick.note_off(key);
        for v in &mut self.voices {
            if v.active && v.key == key && v.fade.target() != 0.0 {
                v.fade.set_target(0.0, (self.rate * 0.03) as u32);
            }
        }
    }
    fn all_notes_off(&mut self) {
        self.kick.all_notes_off();
        for v in &mut self.voices {
            if v.active {
                v.fade.set_target(0.0, (self.rate * 0.004) as u32);
            }
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.kick.process(left, right);
        for (left, right) in left.iter_mut().zip(right) {
            self.fresh = false;
            let mix = self.mix.tick();
            let level = self.level.tick();
            let mut layer = 0.0;
            if let Some(pcm) = &self.pcm {
                for v in &mut self.voices {
                    if v.active {
                        let fade = v.fade.tick();
                        v.last = (pcm.at(v.position) * v.velocity + v.correction.tick()) * fade;
                        layer += v.last;
                        v.position += v.step;
                        if (v.position >= pcm.samples.len() as f64 && v.correction.is_settled())
                            || v.fade.value() == 0.0
                        {
                            v.active = false;
                        }
                    }
                }
            }
            let mix = if self.pcm.is_some() { mix } else { 0.0 };
            *left = (*left * (1.0 - mix) + layer * mix) * level;
            *right = (*right * (1.0 - mix) + layer * mix) * level;
        }
    }
    fn active_voices(&self) -> usize {
        self.kick
            .active_voices()
            .max(self.voices.iter().filter(|v| v.active).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_only_layer_matches_independent_pcm_reference_and_partitions() {
        let pcm = LayerPcm::from_samples(48_000, &[0.0, 0.25, -0.5, 0.75, 0.0]);
        let sound = |partitions: &[usize]| {
            let mut unit = LayerKick::default();
            unit.set_layer(&pcm);
            unit.prepare(48_000.0, 64);
            unit.set_params(&LayerKickParams {
                sample_mix: 1.0,
                level: 1.0,
                ..Default::default()
            });
            unit.note_on(36, 1.0);
            let mut l = vec![0.0; 64];
            let mut r = vec![0.0; 64];
            let mut at = 0;
            let mut block = 0;
            while at < 64 {
                let end = (at + partitions[block % partitions.len()]).min(64);
                unit.process(&mut l[at..end], &mut r[at..end]);
                at = end;
                block += 1;
            }
            assert_eq!(l, r);
            l
        };
        let signal = sound(&[64]);
        assert_eq!(signal, sound(&[1, 7, 3]));
        for (i, &x) in signal.iter().enumerate() {
            assert!((x - pcm.frame(i)).abs() < 1e-6);
        }
    }
    #[test]
    fn missing_optional_layer_preserves_the_core_pitch_swept_kick() {
        let mut layer = LayerKick::default();
        let mut kick = Kick::default();
        layer.prepare(48_000.0, 64);
        kick.prepare(48_000.0, 64);
        layer.note_on(36, 1.0);
        kick.note_on(36, 1.0);
        let mut l = [0.0; 64];
        let mut r = [0.0; 64];
        let mut kl = [0.0; 64];
        let mut kr = [0.0; 64];
        layer.process(&mut l, &mut r);
        kick.process(&mut kl, &mut kr);
        assert_eq!(l, kl);
        assert_eq!(r, kr);
    }
    #[test]
    fn stolen_sample_layer_preserves_endpoint_and_stop_silences_corrections() {
        let pcm = LayerPcm::from_samples(48000, &vec![0.5; 4096]);
        let mut unit = LayerKick::default();
        unit.set_layer(&pcm);
        unit.prepare(48000.0, 257);
        unit.set_params(&LayerKickParams {
            sample_mix: 1.0,
            level: 1.0,
            ..Default::default()
        });
        for key in 36..44 {
            unit.note_on(key, 1.0);
        }
        let (mut l, mut r) = ([0.0; 257], [0.0; 257]);
        unit.process(&mut l, &mut r);
        let before = l[256];
        unit.note_on(44, 0.1);
        let (mut first, mut right) = ([0.0], [0.0]);
        unit.process(&mut first, &mut right);
        assert!((first[0] - before).abs() < 0.01);
        unit.all_notes_off();
        unit.process(&mut l, &mut r);
        unit.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| *x == 0.0));
    }
}
