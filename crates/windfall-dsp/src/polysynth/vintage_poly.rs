//! Digitally clocked saw/pulse/sub oscillator, resonant ladder and stereo chorus.
//! This is Windfall's own vintage polysynth model, with deterministic component drift.
use super::{
    core,
    runtime::{Context, Frame, Patch, Tone, instrument, oscillator},
};
use crate::param::param_set;
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct VintagePolyParams {
    pub saw: f32,
    pub pulse: f32,
    pub sub: f32,
    pub noise: f32,
    pub pulse_width: f32,
    pub pwm: f32,
    pub drift: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub highpass_hz: f32,
    pub filter_amount: f32,
    pub key_track: f32,
    pub lfo_rate_hz: f32,
    pub lfo_filter: f32,
    pub envelope: EnvelopeParams,
    pub filter_envelope: EnvelopeParams,
    pub unison: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub glide_ms: f32,
    pub polyphony: u8,
    pub chorus: f32,
    pub gain: f32,
}
impl Default for VintagePolyParams {
    fn default() -> Self {
        Self {
            saw: 0.8,
            pulse: 0.2,
            sub: 0.15,
            noise: 0.0,
            pulse_width: 0.5,
            pwm: 0.0,
            drift: 0.2,
            cutoff_hz: 4000.0,
            resonance: 0.2,
            highpass_hz: 20.0,
            filter_amount: 2.0,
            key_track: 0.5,
            lfo_rate_hz: 0.8,
            lfo_filter: 0.0,
            envelope: EnvelopeParams::default(),
            filter_envelope: EnvelopeParams::default(),
            unison: 1,
            detune_cents: 12.0,
            spread: 0.6,
            glide_ms: 0.0,
            polyphony: 8,
            chorus: 0.0,
            gain: 0.5,
        }
    }
}
param_set!(VintagePolyParams, "Vintage poly", {
    float [saw] "saw" "Saw level" { Gain, Linear, 0.0, 1.0, 0.8 }
    float [pulse] "pulse" "Pulse level" { Gain, Linear, 0.0, 1.0, 0.2 }
    float [sub] "sub" "Sub level" { Gain, Linear, 0.0, 1.0, 0.15 }
    float [noise] "noise" "Noise level" { Gain, Linear, 0.0, 1.0, 0.0 }
    float [pulse_width] "pulseWidth" "Pulse width" { Fraction, Linear, 0.05, 0.95, 0.5 }
    float [pwm] "pwm" "Pulse modulation" { Fraction, Linear, 0.0, 0.4, 0.0 }
    float [drift] "drift" "Component drift" { Fraction, Linear, 0.0, 1.0, 0.2 }
    float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 20000.0, 4000.0 }
    float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.2 }
    float [highpass_hz] "highpassHz" "Bass cut" { Hertz, Logarithmic, 20.0, 4000.0, 20.0 }
    float [filter_amount] "filterAmount" "Filter envelope depth" { Octaves, Linear, -6.0, 6.0, 2.0 }
    float [key_track] "keyTrack" "Filter key tracking" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [lfo_rate_hz] "lfoRateHz" "LFO rate" { Hertz, Logarithmic, 0.01, 20.0, 0.8 }
    float [lfo_filter] "lfoFilter" "LFO filter depth" { Octaves, Linear, 0.0, 4.0, 0.0 }
    float [envelope.attack_ms] "envelope.attackMs" "Amp attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
    float [envelope.decay_ms] "envelope.decayMs" "Amp decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
    float [envelope.sustain] "envelope.sustain" "Amp sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
    float [envelope.release_ms] "envelope.releaseMs" "Amp release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
    float [filter_envelope.attack_ms] "filterEnvelope.attackMs" "Filter attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
    float [filter_envelope.decay_ms] "filterEnvelope.decayMs" "Filter decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
    float [filter_envelope.sustain] "filterEnvelope.sustain" "Filter sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
    float [filter_envelope.release_ms] "filterEnvelope.releaseMs" "Filter release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
    int [unison] "unison" "Unison voices" { None, 1, 7, 1 }
    float [detune_cents] "detuneCents" "Unison detune" { Cents, Linear, 0.0, 60.0, 12.0 }
    float [spread] "spread" "Stereo spread" { Fraction, Linear, 0.0, 1.0, 0.6 }
    float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
    int [polyphony] "polyphony" "Polyphony" { None, 1, 16, 8 }
    float [chorus] "chorus" "Stereo chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});
impl Patch for VintagePolyParams {
    fn frame(&self) -> Frame {
        Frame {
            amp: self.envelope,
            filter_env: self.filter_envelope,
            cutoff: self.cutoff_hz,
            env_amount: self.filter_amount,
            key_track: self.key_track,
            lfo_rate: self.lfo_rate_hz,
            lfo_filter: self.lfo_filter,
            gain: self.gain,
            glide: self.glide_ms,
            polyphony: self.polyphony as usize,
            chorus: self.chorus,
            ..Frame::default()
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let drift = t.drift.tick(c.slot, c.rate) * self.drift;
        let mut sum = (0.0, 0.0);
        let noise = t.noise.bipolar() * self.noise;
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + (drift + pos * self.detune_cents) / 100.0;
            // Saw and pulse share a DCO clock, the divided sub has its own octave clock.
            let phase = t.oscillators[u][0].phase();
            let inc = core::increment(pitch, c.rate);
            let saw = oscillator(t, u, 0, pitch, c.rate, Waveform::Saw, 0.5);
            let pulse = crate::blocks::oscillator::pulse(
                phase,
                inc,
                (self.pulse_width + c.lfo * self.pwm).clamp(0.05, 0.95),
            );
            let sub = oscillator(t, u, 1, pitch - 12.0, c.rate, Waveform::Square, 0.5);
            let raw = self.saw * saw + self.pulse * pulse + self.sub * sub + noise;
            let pan = core::pan_gains(pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let k = core::ladder_feedback(self.resonance);
        let g = core::swept_gain(c.cutoff, 0.0, c.rate);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            let hp = t.highpass[side]
                .tick(
                    input / self.unison as f32,
                    core::swept_gain(self.highpass_hz, 0.0, c.rate),
                    1.414,
                )
                .2;
            core::ladder_low4(&t.ladder[side].tick(hp, g, k)) * core::ladder_makeup(k)
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(VintagePoly, VintagePolyParams);
