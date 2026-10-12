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
pub enum GrooveMode {
    Analog,
    Phase,
    Ring,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct GrooveSynthParams {
    pub mode: GrooveMode,
    pub mix: f32,
    pub coarse: f32,
    pub ratio: f32,
    pub shape: f32,
    pub index: f32,
    pub motion: f32,
    pub drive: f32,
    pub sync: bool,
    pub division: f32,
    pub gate: f32,
    pub chorus: f32,
    pub delay: f32,
    pub phaser: f32,
    pub reverb: f32,
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
impl Default for GrooveSynthParams {
    fn default() -> Self {
        Self {
            mode: GrooveMode::Analog,
            mix: 0.5,
            coarse: 0.0,
            ratio: 2.0,
            shape: 0.3,
            index: 1.0,
            motion: 0.5,
            drive: 0.2,
            sync: true,
            division: 0.5,
            gate: 0.0,
            chorus: 0.0,
            delay: 0.0,
            phaser: 0.0,
            reverb: 0.0,
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
param_set!(GrooveSynthParams, "Groove synth", {
choice [mode] "mode" "Synthesis mode" { GrooveMode, Analog, [Analog "analog" "Analog", Phase "phase" "Phase modulation", Ring "ring" "Ring modulation", Table "table" "Harmonic scan"] }
float [mix] "mix" "Oscillator mix" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [coarse] "coarse" "Oscillator B pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
float [ratio] "ratio" "Modulator ratio" { Ratio, Linear, 0.25, 12.0, 2.0 }
float [shape] "shape" "Oscillator shape" { Fraction, Linear, 0.0, 1.0, 0.3 }
float [index] "index" "Modulation index" { Ratio, Linear, 0.0, 6.0, 1.0 }
float [motion] "motion" "Motion depth" { Octaves, Linear, 0.0, 4.0, 0.5 }
float [drive] "drive" "Drive" { Fraction, Linear, 0.0, 1.0, 0.2 }
toggle [sync] "sync" "Tempo motion" { true }
float [division] "division" "Motion beats" { Ratio, Linear, 0.125, 4.0, 0.5 }
float [gate] "gate" "Rhythmic gate" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [chorus] "chorus" "Chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [phaser] "phaser" "Phaser" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [reverb] "reverb" "Room" { Fraction, Linear, 0.0, 1.0, 0.0 }
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
impl Patch for GrooveSynthParams {
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
            reverb: self.reverb,
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let seconds = t.elapsed as f32 * 2.0 / c.rate;
        let motion = if self.sync {
            (std::f32::consts::TAU * seconds / core::division_seconds(self.division, c.bpm)).sin()
        } else {
            c.lfo
        };
        let mut sum = (0.0, 0.0);
        let ratio_pitch = 12.0 * self.ratio.log2();
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + pos * self.detune_cents / 100.0;
            let inc = t.phase_increment(u, 0, pitch, c.rate);
            let a = if matches!(self.mode, GrooveMode::Analog | GrooveMode::Ring) {
                t.oscillators[u][0].tick(Waveform::Saw, inc, 0.5)
            } else {
                t.oscillators[u][0].advance(inc);
                0.0
            };
            let b = if self.mode == GrooveMode::Analog {
                oscillator(
                    t,
                    u,
                    1,
                    pitch + self.coarse,
                    c.rate,
                    Waveform::Pulse,
                    0.1 + self.shape * 0.8,
                )
            } else {
                let inc = t.phase_increment(u, 1, pitch + self.coarse, c.rate);
                t.oscillators[u][1].advance(inc);
                0.0
            };
            let modulator = if matches!(self.mode, GrooveMode::Phase | GrooveMode::Ring) {
                oscillator(t, u, 2, pitch + ratio_pitch, c.rate, Waveform::Sine, 0.5)
            } else {
                let inc = t.phase_increment(u, 2, pitch + ratio_pitch, c.rate);
                t.oscillators[u][2].advance(inc);
                0.0
            };
            let raw = match self.mode {
                GrooveMode::Analog => a * (1.0 - self.mix) + b * self.mix,
                GrooveMode::Phase => {
                    let carrier = t.oscillators[u][3].phase();
                    oscillator(t, u, 3, pitch, c.rate, Waveform::Sine, 0.5);
                    let index = core::phase_index(
                        core::increment(pitch, c.rate),
                        core::increment(pitch + 12.0 * self.ratio.log2(), c.rate),
                        self.index * (0.25 + self.shape),
                    );
                    (std::f32::consts::TAU * carrier + modulator * index).sin()
                }
                GrooveMode::Ring => a * (1.0 - self.mix) + a * modulator * self.mix,
                GrooveMode::Table => {
                    // Scan two original harmonic spectra, dropping every partial
                    // above the *host* Nyquist rather than folding it back.
                    let inc = core::increment(pitch, c.rate);
                    let mut total = 0.0;
                    let carrier = t.oscillators[u][4].phase();
                    oscillator(t, u, 4, pitch, c.rate, Waveform::Sine, 0.5);
                    // sin((h+1)t) = 2 cos(t) sin(ht) - sin((h-1)t).
                    // Double precision bounds recurrence error over all 16
                    // unchanged partials, with one trigonometric pair.
                    let (mut harmonic, cosine) =
                        (std::f64::consts::TAU * f64::from(carrier)).sin_cos();
                    let mut previous = 0.0;
                    for h in 1..=16 {
                        if inc * h as f32 >= 0.24 {
                            break;
                        }
                        let odd = if h % 2 == 1 {
                            1.0 / (h * h) as f32
                        } else {
                            0.0
                        };
                        let bright = 1.0 / h as f32;
                        total += harmonic as f32 * (odd + (bright - odd) * self.shape);
                        let next = 2.0 * cosine * harmonic - previous;
                        previous = harmonic;
                        harmonic = next;
                    }
                    total * 0.6
                }
            };
            let gate = 1.0 - self.gate * (0.5 - 0.5 * motion);
            let pan = t.pan(u, pos * self.spread);
            sum.0 += raw * gate * pan.0;
            sum.1 += raw * gate * pan.1;
        }
        let cutoff = core::swept(c.cutoff, motion * self.motion, c.rate);
        let (gain, damping) = t.filter_coefficients(cutoff, self.resonance, c.rate);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            let driven = t.drive(input / self.unison as f32, self.drive);
            t.svf[side].tick(driven, gain, damping).0
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(GrooveSynth, GrooveSynthParams);
