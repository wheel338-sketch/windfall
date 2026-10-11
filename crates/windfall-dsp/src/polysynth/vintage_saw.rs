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
pub struct VintageSawParams {
    pub pulse: f32,
    pub sub: f32,
    pub pulse_width: f32,
    pub pwm: f32,
    pub drift: f32,
    pub asymmetry: f32,
    pub drive: f32,
    pub pitch_drop: f32,
    pub pitch_decay_ms: f32,
    pub chorus: f32,
    pub delay: f32,
    pub phaser: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub filter_amount: f32,
    pub key_track: f32,
    pub envelope: EnvelopeParams,
    pub filter_envelope: EnvelopeParams,
    pub lfo_rate_hz: f32,
    pub lfo_filter: f32,
    pub unison: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub glide_ms: f32,
    pub polyphony: u8,
    pub gain: f32,
}
impl Default for VintageSawParams {
    fn default() -> Self {
        Self {
            pulse: 0.15,
            sub: 0.1,
            pulse_width: 0.5,
            pwm: 0.0,
            drift: 0.3,
            asymmetry: 0.2,
            drive: 0.3,
            pitch_drop: 0.0,
            pitch_decay_ms: 120.0,
            chorus: 0.0,
            delay: 0.0,
            phaser: 0.0,
            cutoff_hz: 4000.0,
            resonance: 0.2,
            filter_amount: 2.0,
            key_track: 0.5,
            envelope: EnvelopeParams::default(),
            filter_envelope: EnvelopeParams::default(),
            lfo_rate_hz: 0.8,
            lfo_filter: 0.0,
            unison: 1,
            detune_cents: 12.0,
            spread: 0.6,
            glide_ms: 0.0,
            polyphony: 8,
            gain: 0.5,
        }
    }
}
param_set!(VintageSawParams, "Vintage saw", {
float [pulse] "pulse" "Pulse blend" { Fraction, Linear, 0.0, 1.0, 0.15 }
float [sub] "sub" "Sub level" { Fraction, Linear, 0.0, 1.0, 0.1 }
float [pulse_width] "pulseWidth" "Pulse width" { Fraction, Linear, 0.05, 0.95, 0.5 }
float [pwm] "pwm" "Pulse modulation" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [drift] "drift" "Component drift" { Fraction, Linear, 0.0, 1.0, 0.3 }
float [asymmetry] "asymmetry" "Oscillator asymmetry" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [drive] "drive" "Filter drive" { Fraction, Linear, 0.0, 1.0, 0.3 }
float [pitch_drop] "pitchDrop" "Pitch envelope" { Semitones, Linear, -24.0, 24.0, 0.0 }
float [pitch_decay_ms] "pitchDecayMs" "Pitch decay" { Milliseconds, Linear, 1.0, 2000.0, 120.0 }
float [chorus] "chorus" "Stereo chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [phaser] "phaser" "Phaser mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 20000.0, 4000.0 }
float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [filter_amount] "filterAmount" "Filter envelope depth" { Octaves, Linear, -6.0, 6.0, 2.0 }
float [key_track] "keyTrack" "Filter key tracking" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [envelope.attack_ms] "envelope.attackMs" "Amp attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [envelope.decay_ms] "envelope.decayMs" "Amp decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [envelope.sustain] "envelope.sustain" "Amp sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [envelope.release_ms] "envelope.releaseMs" "Amp release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [filter_envelope.attack_ms] "filterEnvelope.attackMs" "Filter attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [filter_envelope.decay_ms] "filterEnvelope.decayMs" "Filter decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [filter_envelope.sustain] "filterEnvelope.sustain" "Filter sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [filter_envelope.release_ms] "filterEnvelope.releaseMs" "Filter release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [lfo_rate_hz] "lfoRateHz" "LFO rate" { Hertz, Logarithmic, 0.01, 20.0, 0.8 }
float [lfo_filter] "lfoFilter" "LFO filter depth" { Octaves, Linear, 0.0, 4.0, 0.0 }
int [unison] "unison" "Unison voices" { None, 1.0, 7.0, 1.0 }
float [detune_cents] "detuneCents" "Unison detune" { Cents, Linear, 0.0, 60.0, 12.0 }
float [spread] "spread" "Stereo spread" { Fraction, Linear, 0.0, 1.0, 0.6 }
float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
int [polyphony] "polyphony" "Polyphony" { None, 1.0, 16.0, 8.0 }
float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});
impl Patch for VintageSawParams {
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
            delay: self.delay,
            phaser: self.phaser,
            ..Frame::default()
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let drift = t.drift.tick(c.slot, c.rate) * self.drift;
        let pitch_env =
            self.pitch_drop * (-(t.elapsed as f32) * 2000.0 / (c.rate * self.pitch_decay_ms)).exp();
        let mut sum = (0.0, 0.0);
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + pitch_env + (pos * self.detune_cents + drift) / 100.0;
            let saw = oscillator(t, u, 0, pitch, c.rate, Waveform::Saw, 0.5);
            // Unequal charge/discharge response: quadratic even harmonics with
            // their DC term removed, evaluated at the oversampled rate.
            let shaped = saw + self.asymmetry * (saw * saw - 1.0 / 3.0) * 0.5;
            let pulse = oscillator(
                t,
                u,
                1,
                pitch,
                c.rate,
                Waveform::Pulse,
                self.pulse_width + c.lfo * self.pwm * 0.4,
            );
            let sub = oscillator(t, u, 2, pitch - 12.0, c.rate, Waveform::Triangle, 0.5);
            let raw = shaped * (1.0 - self.pulse) + pulse * self.pulse + sub * self.sub;
            let pan = core::pan_gains(pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let g = core::swept_gain(c.cutoff, 0.0, c.rate);
        let k = core::svf_damping(self.resonance);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            t.svf[side]
                .tick(core::drive(input / self.unison as f32, self.drive), g, k)
                .0
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(VintageSaw, VintageSawParams);
