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
pub struct PocketSynthParams {
    pub waveform: Waveform,
    pub pulse_width: f32,
    pub sub: f32,
    pub vibrato: f32,
    pub drive: f32,
    pub chorus: f32,
    pub delay: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub filter_amount: f32,
    pub key_track: f32,
    pub envelope: EnvelopeParams,
    pub lfo_rate_hz: f32,
    pub glide_ms: f32,
    pub polyphony: u8,
    pub gain: f32,
}
impl Default for PocketSynthParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            pulse_width: 0.5,
            sub: 0.1,
            vibrato: 0.0,
            drive: 0.0,
            chorus: 0.0,
            delay: 0.0,
            cutoff_hz: 4000.0,
            resonance: 0.2,
            filter_amount: 2.0,
            key_track: 0.5,
            envelope: EnvelopeParams::default(),
            lfo_rate_hz: 0.8,
            glide_ms: 0.0,
            polyphony: 8,
            gain: 0.5,
        }
    }
}
param_set!(PocketSynthParams, "Pocket synth", {
choice [waveform] "waveform" "Oscillator shape" { Waveform, Saw, [Saw "saw" "Saw", Pulse "pulse" "Pulse", Triangle "triangle" "Triangle", Sine "sine" "Sine", Square "square" "Square", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
float [pulse_width] "pulseWidth" "Pulse width" { Fraction, Linear, 0.05, 0.95, 0.5 }
float [sub] "sub" "Sub octave" { Fraction, Linear, 0.0, 1.0, 0.1 }
float [vibrato] "vibrato" "Vibrato" { Semitones, Linear, 0.0, 1.0, 0.0 }
float [drive] "drive" "Warmth" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [chorus] "chorus" "Chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 20000.0, 4000.0 }
float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [filter_amount] "filterAmount" "Filter envelope depth" { Octaves, Linear, -6.0, 6.0, 2.0 }
float [key_track] "keyTrack" "Filter key tracking" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [envelope.attack_ms] "envelope.attackMs" "Amp attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [envelope.decay_ms] "envelope.decayMs" "Amp decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [envelope.sustain] "envelope.sustain" "Amp sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [envelope.release_ms] "envelope.releaseMs" "Amp release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [lfo_rate_hz] "lfoRateHz" "LFO rate" { Hertz, Logarithmic, 0.01, 20.0, 0.8 }
float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
int [polyphony] "polyphony" "Polyphony" { None, 1.0, 16.0, 8.0 }
float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});
impl Patch for PocketSynthParams {
    fn frame(&self) -> Frame {
        Frame {
            amp: self.envelope,
            filter_env: self.envelope,
            cutoff: self.cutoff_hz,
            env_amount: self.filter_amount,
            key_track: self.key_track,
            lfo_rate: self.lfo_rate_hz,
            gain: self.gain,
            glide: self.glide_ms,
            polyphony: self.polyphony as usize,
            chorus: self.chorus,
            delay: self.delay,
            ..Frame::default()
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let pitch = c.pitch + c.lfo * self.vibrato;
        let main = oscillator(t, 0, 0, pitch, c.rate, self.waveform, self.pulse_width);
        let sub = oscillator(t, 0, 1, pitch - 12.0, c.rate, Waveform::Triangle, 0.5);
        let raw = core::drive(main + sub * self.sub, self.drive);
        let low = t.svf[0]
            .tick(
                raw,
                core::swept_gain(c.cutoff, 0.0, c.rate),
                core::svf_damping(self.resonance),
            )
            .0;
        (low, low)
    }
}
instrument!(PocketSynth, PocketSynthParams);
