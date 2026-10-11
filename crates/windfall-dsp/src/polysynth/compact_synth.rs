use super::{
    core,
    runtime::{Context, Frame, Patch, Tone, instrument, oscillator},
};
use crate::param::param_set;
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CompactResponse {
    Low,
    Band,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct CompactSynthParams {
    pub wave_a: Waveform,
    pub wave_b: Waveform,
    pub mix: f32,
    pub coarse: f32,
    pub fine: f32,
    pub pulse_width: f32,
    pub pwm: f32,
    pub ring: f32,
    pub response: CompactResponse,
    pub drive: f32,
    pub chorus: f32,
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
impl Default for CompactSynthParams {
    fn default() -> Self {
        Self {
            wave_a: Waveform::Saw,
            wave_b: Waveform::Pulse,
            mix: 0.5,
            coarse: 0.0,
            fine: 7.0,
            pulse_width: 0.5,
            pwm: 0.0,
            ring: 0.0,
            response: CompactResponse::Low,
            drive: 0.0,
            chorus: 0.0,
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
param_set!(CompactSynthParams, "Compact synth", {
choice [wave_a] "waveA" "Oscillator A" { Waveform, Saw, [Saw "saw" "Saw", Pulse "pulse" "Pulse", Triangle "triangle" "Triangle", Sine "sine" "Sine", Square "square" "Square", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
choice [wave_b] "waveB" "Oscillator B" { Waveform, Pulse, [Saw "saw" "Saw", Pulse "pulse" "Pulse", Triangle "triangle" "Triangle", Sine "sine" "Sine", Square "square" "Square", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
float [mix] "mix" "Oscillator mix" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [coarse] "coarse" "Oscillator B pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
float [fine] "fine" "Oscillator B fine" { Cents, Linear, -100.0, 100.0, 7.0 }
float [pulse_width] "pulseWidth" "Pulse width" { Fraction, Linear, 0.05, 0.95, 0.5 }
float [pwm] "pwm" "Pulse modulation" { Fraction, Linear, 0.0, 0.4, 0.0 }
float [ring] "ring" "Ring mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
choice [response] "response" "Filter response" { CompactResponse, Low, [Low "low" "Lowpass", Band "band" "Bandpass", High "high" "Highpass"] }
float [drive] "drive" "Saturation" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [chorus] "chorus" "Stereo chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
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
int [unison] "unison" "Unison voices" { None, 1, 7, 1 }
float [detune_cents] "detuneCents" "Unison detune" { Cents, Linear, 0.0, 60.0, 12.0 }
float [spread] "spread" "Stereo spread" { Fraction, Linear, 0.0, 1.0, 0.6 }
float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
int [polyphony] "polyphony" "Polyphony" { None, 1, 16, 8 }
float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});
impl Patch for CompactSynthParams {
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
        let mut sum = (0.0, 0.0);
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + pos * self.detune_cents / 100.0;
            let width = (self.pulse_width + c.lfo * self.pwm).clamp(0.05, 0.95);
            let a = oscillator(t, u, 0, pitch, c.rate, self.wave_a, width);
            let b = oscillator(
                t,
                u,
                1,
                pitch + self.coarse + self.fine / 100.0,
                c.rate,
                self.wave_b,
                width,
            );
            let raw = (a * (1.0 - self.mix) + b * self.mix) * (1.0 - self.ring) + a * b * self.ring;
            let pan = core::pan_gains(pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let g = core::swept_gain(c.cutoff, 0.0, c.rate);
        let k = core::svf_damping(self.resonance);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            let (low, band, high) =
                t.svf[side].tick(core::drive(input / self.unison as f32, self.drive), g, k);
            match self.response {
                CompactResponse::Low => low,
                CompactResponse::Band => band,
                CompactResponse::High => high,
            }
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(CompactSynth, CompactSynthParams);
