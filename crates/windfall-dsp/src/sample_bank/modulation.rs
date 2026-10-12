//! Persisted per-zone routes; nonzero groups share the first member's routes.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ZoneLfo {
    pub rate_hz: f32,
    pub delay_ms: f32,
    pub phase: f32,
    pub pitch_cents: f32,
    pub filter_cents: f32,
    pub amplitude: f32,
}
impl Default for ZoneLfo {
    fn default() -> Self {
        Self {
            rate_hz: 5.0,
            delay_ms: 0.0,
            phase: 0.0,
            pitch_cents: 0.0,
            filter_cents: 0.0,
            amplitude: 0.0,
        }
    }
}
impl ZoneLfo {
    fn validate(self) -> bool {
        finite(self.rate_hz, 0.0, 100.0)
            && finite(self.delay_ms, 0.0, 100000.0)
            && finite(self.phase, 0.0, 1.0)
            && depth(self.pitch_cents)
            && depth(self.filter_cents)
            && finite(self.amplitude, -1.0, 1.0)
    }
    pub(super) fn enabled(self) -> bool {
        self.pitch_cents != 0.0 || self.filter_cents != 0.0 || self.amplitude != 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ZoneModulation {
    pub cutoff_hz: f32,
    pub delay_ms: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub envelope_pitch_cents: f32,
    pub envelope_filter_cents: f32,
    pub envelope_amplitude: f32,
    pub lfos: [ZoneLfo; 2],
}
impl Default for ZoneModulation {
    fn default() -> Self {
        Self {
            cutoff_hz: 20000.0,
            delay_ms: 0.0,
            attack_ms: 0.0,
            hold_ms: 0.0,
            decay_ms: 200.0,
            sustain: 0.0,
            release_ms: 100.0,
            envelope_pitch_cents: 0.0,
            envelope_filter_cents: 0.0,
            envelope_amplitude: 0.0,
            lfos: [ZoneLfo::default(); 2],
        }
    }
}
fn finite(value: f32, low: f32, high: f32) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}
fn depth(value: f32) -> bool {
    finite(value, -12000.0, 12000.0)
}
impl ZoneModulation {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite(self.cutoff_hz, 20.0, 20000.0)
            || !finite(self.sustain, 0.0, 1.0)
            || !depth(self.envelope_pitch_cents)
            || !depth(self.envelope_filter_cents)
            || !finite(self.envelope_amplitude, -1.0, 1.0)
            || [
                self.delay_ms,
                self.attack_ms,
                self.hold_ms,
                self.decay_ms,
                self.release_ms,
            ]
            .iter()
            .any(|v| !finite(*v, 0.0, 100000.0))
            || self.lfos.iter().any(|lfo| !lfo.validate())
        {
            return Err("zone modulation contains a nonfinite or out-of-range control");
        }
        Ok(())
    }
    pub(super) fn envelope(self, time_ms: f32) -> f32 {
        let time = time_ms - self.delay_ms;
        if time < 0.0 {
            0.0
        } else if time < self.attack_ms {
            time / self.attack_ms
        } else if time < self.attack_ms + self.hold_ms {
            1.0
        } else if time < self.attack_ms + self.hold_ms + self.decay_ms {
            1.0 + (self.sustain - 1.0) * (time - self.attack_ms - self.hold_ms) / self.decay_ms
        } else {
            self.sustain
        }
    }
}
