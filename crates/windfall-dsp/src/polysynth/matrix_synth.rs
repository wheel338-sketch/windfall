use super::operators::{self, MatrixOperator, OperatorWave};
use super::{
    core,
    runtime::{Context, Frame, Patch, Tone, instrument},
};
use crate::EnvelopeParams;
use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MatrixSynthParams {
    pub operators: [MatrixOperator; 6],
    pub fm: [[f32; 6]; 6],
    pub ring: [[f32; 6]; 6],
    pub output: [f32; 6],
    pub filter_mix: f32,
    pub lfo_pitch: f32,
    pub lfo_index: f32,
    pub drive: f32,
    pub chorus: f32,
    pub delay: f32,
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
impl Default for MatrixSynthParams {
    fn default() -> Self {
        Self {
            operators: [MatrixOperator::default(); 6],
            fm: [[0.0; 6]; 6],
            ring: [[0.0; 6]; 6],
            output: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            filter_mix: 1.0,
            lfo_pitch: 0.0,
            lfo_index: 0.0,
            drive: 0.0,
            chorus: 0.0,
            delay: 0.0,
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
param_set!(MatrixSynthParams, "Matrix synth", {
float [operators[0].ratio] "operators.0.ratio" "Operator 1 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[0].detune_cents] "operators.0.detuneCents" "Operator 1 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[0].level] "operators.0.level" "Operator 1 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[0].feedback] "operators.0.feedback" "Operator 1 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[0].key_scale] "operators.0.keyScale" "Operator 1 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[0].waveform] "operators.0.waveform" "Operator 1 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[0].envelope.attack_ms] "operators.0.envelope.attackMs" "Operator 1 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[0].envelope.decay_ms] "operators.0.envelope.decayMs" "Operator 1 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[0].envelope.sustain] "operators.0.envelope.sustain" "Operator 1 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[0].envelope.release_ms] "operators.0.envelope.releaseMs" "Operator 1 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [operators[1].ratio] "operators.1.ratio" "Operator 2 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[1].detune_cents] "operators.1.detuneCents" "Operator 2 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[1].level] "operators.1.level" "Operator 2 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[1].feedback] "operators.1.feedback" "Operator 2 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[1].key_scale] "operators.1.keyScale" "Operator 2 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[1].waveform] "operators.1.waveform" "Operator 2 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[1].envelope.attack_ms] "operators.1.envelope.attackMs" "Operator 2 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[1].envelope.decay_ms] "operators.1.envelope.decayMs" "Operator 2 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[1].envelope.sustain] "operators.1.envelope.sustain" "Operator 2 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[1].envelope.release_ms] "operators.1.envelope.releaseMs" "Operator 2 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [operators[2].ratio] "operators.2.ratio" "Operator 3 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[2].detune_cents] "operators.2.detuneCents" "Operator 3 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[2].level] "operators.2.level" "Operator 3 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[2].feedback] "operators.2.feedback" "Operator 3 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[2].key_scale] "operators.2.keyScale" "Operator 3 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[2].waveform] "operators.2.waveform" "Operator 3 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[2].envelope.attack_ms] "operators.2.envelope.attackMs" "Operator 3 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[2].envelope.decay_ms] "operators.2.envelope.decayMs" "Operator 3 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[2].envelope.sustain] "operators.2.envelope.sustain" "Operator 3 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[2].envelope.release_ms] "operators.2.envelope.releaseMs" "Operator 3 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [operators[3].ratio] "operators.3.ratio" "Operator 4 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[3].detune_cents] "operators.3.detuneCents" "Operator 4 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[3].level] "operators.3.level" "Operator 4 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[3].feedback] "operators.3.feedback" "Operator 4 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[3].key_scale] "operators.3.keyScale" "Operator 4 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[3].waveform] "operators.3.waveform" "Operator 4 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[3].envelope.attack_ms] "operators.3.envelope.attackMs" "Operator 4 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[3].envelope.decay_ms] "operators.3.envelope.decayMs" "Operator 4 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[3].envelope.sustain] "operators.3.envelope.sustain" "Operator 4 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[3].envelope.release_ms] "operators.3.envelope.releaseMs" "Operator 4 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [operators[4].ratio] "operators.4.ratio" "Operator 5 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[4].detune_cents] "operators.4.detuneCents" "Operator 5 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[4].level] "operators.4.level" "Operator 5 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[4].feedback] "operators.4.feedback" "Operator 5 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[4].key_scale] "operators.4.keyScale" "Operator 5 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[4].waveform] "operators.4.waveform" "Operator 5 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[4].envelope.attack_ms] "operators.4.envelope.attackMs" "Operator 5 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[4].envelope.decay_ms] "operators.4.envelope.decayMs" "Operator 5 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[4].envelope.sustain] "operators.4.envelope.sustain" "Operator 5 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[4].envelope.release_ms] "operators.4.envelope.releaseMs" "Operator 5 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [operators[5].ratio] "operators.5.ratio" "Operator 6 ratio" { Ratio, Linear, 0.125, 32.0, 1.0 }
float [operators[5].detune_cents] "operators.5.detuneCents" "Operator 6 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
float [operators[5].level] "operators.5.level" "Operator 6 level" { Gain, Linear, 0.0, 1.0, 0.5 }
float [operators[5].feedback] "operators.5.feedback" "Operator 6 feedback" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [operators[5].key_scale] "operators.5.keyScale" "Operator 6 key scaling" { Octaves, Linear, -2.0, 2.0, 0.0 }
choice [operators[5].waveform] "operators.5.waveform" "Operator 6 shape" { OperatorWave, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square"] }
float [operators[5].envelope.attack_ms] "operators.5.envelope.attackMs" "Operator 6 attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [operators[5].envelope.decay_ms] "operators.5.envelope.decayMs" "Operator 6 decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [operators[5].envelope.sustain] "operators.5.envelope.sustain" "Operator 6 sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [operators[5].envelope.release_ms] "operators.5.envelope.releaseMs" "Operator 6 release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [fm[0][0]] "fm.0.0" "FM 1 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[0][1]] "fm.0.1" "FM 2 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[0][2]] "fm.0.2" "FM 3 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[0][3]] "fm.0.3" "FM 4 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[0][4]] "fm.0.4" "FM 5 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[0][5]] "fm.0.5" "FM 6 to 1" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][0]] "fm.1.0" "FM 1 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][1]] "fm.1.1" "FM 2 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][2]] "fm.1.2" "FM 3 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][3]] "fm.1.3" "FM 4 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][4]] "fm.1.4" "FM 5 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[1][5]] "fm.1.5" "FM 6 to 2" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][0]] "fm.2.0" "FM 1 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][1]] "fm.2.1" "FM 2 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][2]] "fm.2.2" "FM 3 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][3]] "fm.2.3" "FM 4 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][4]] "fm.2.4" "FM 5 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[2][5]] "fm.2.5" "FM 6 to 3" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][0]] "fm.3.0" "FM 1 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][1]] "fm.3.1" "FM 2 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][2]] "fm.3.2" "FM 3 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][3]] "fm.3.3" "FM 4 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][4]] "fm.3.4" "FM 5 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[3][5]] "fm.3.5" "FM 6 to 4" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][0]] "fm.4.0" "FM 1 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][1]] "fm.4.1" "FM 2 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][2]] "fm.4.2" "FM 3 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][3]] "fm.4.3" "FM 4 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][4]] "fm.4.4" "FM 5 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[4][5]] "fm.4.5" "FM 6 to 5" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][0]] "fm.5.0" "FM 1 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][1]] "fm.5.1" "FM 2 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][2]] "fm.5.2" "FM 3 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][3]] "fm.5.3" "FM 4 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][4]] "fm.5.4" "FM 5 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [fm[5][5]] "fm.5.5" "FM 6 to 6" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [ring[0][0]] "ring.0.0" "RING 1 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[0][1]] "ring.0.1" "RING 2 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[0][2]] "ring.0.2" "RING 3 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[0][3]] "ring.0.3" "RING 4 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[0][4]] "ring.0.4" "RING 5 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[0][5]] "ring.0.5" "RING 6 to 1" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][0]] "ring.1.0" "RING 1 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][1]] "ring.1.1" "RING 2 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][2]] "ring.1.2" "RING 3 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][3]] "ring.1.3" "RING 4 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][4]] "ring.1.4" "RING 5 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[1][5]] "ring.1.5" "RING 6 to 2" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][0]] "ring.2.0" "RING 1 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][1]] "ring.2.1" "RING 2 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][2]] "ring.2.2" "RING 3 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][3]] "ring.2.3" "RING 4 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][4]] "ring.2.4" "RING 5 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[2][5]] "ring.2.5" "RING 6 to 3" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][0]] "ring.3.0" "RING 1 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][1]] "ring.3.1" "RING 2 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][2]] "ring.3.2" "RING 3 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][3]] "ring.3.3" "RING 4 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][4]] "ring.3.4" "RING 5 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[3][5]] "ring.3.5" "RING 6 to 4" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][0]] "ring.4.0" "RING 1 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][1]] "ring.4.1" "RING 2 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][2]] "ring.4.2" "RING 3 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][3]] "ring.4.3" "RING 4 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][4]] "ring.4.4" "RING 5 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[4][5]] "ring.4.5" "RING 6 to 5" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][0]] "ring.5.0" "RING 1 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][1]] "ring.5.1" "RING 2 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][2]] "ring.5.2" "RING 3 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][3]] "ring.5.3" "RING 4 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][4]] "ring.5.4" "RING 5 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [ring[5][5]] "ring.5.5" "RING 6 to 6" { Ratio, Linear, 0.0, 1.0, 0.0 }
float [output[0]] "output.0" "Operator 1 output" { Gain, Linear, 0.0, 1.0, 1.0 }
float [output[1]] "output.1" "Operator 2 output" { Gain, Linear, 0.0, 1.0, 0.0 }
float [output[2]] "output.2" "Operator 3 output" { Gain, Linear, 0.0, 1.0, 0.0 }
float [output[3]] "output.3" "Operator 4 output" { Gain, Linear, 0.0, 1.0, 0.0 }
float [output[4]] "output.4" "Operator 5 output" { Gain, Linear, 0.0, 1.0, 0.0 }
float [output[5]] "output.5" "Operator 6 output" { Gain, Linear, 0.0, 1.0, 0.0 }
float [filter_mix] "filterMix" "Subtractive filter mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
float [lfo_pitch] "lfoPitch" "LFO pitch depth" { Semitones, Linear, 0.0, 12.0, 0.0 }
float [lfo_index] "lfoIndex" "LFO phase depth" { Ratio, Linear, 0.0, 6.0, 0.0 }
float [drive] "drive" "Drive" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [chorus] "chorus" "Chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo" { Fraction, Linear, 0.0, 1.0, 0.0 }
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
impl Patch for MatrixSynthParams {
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
            reverb: self.reverb,
            ..Frame::default()
        }
    }
    fn configure(&self, t: &mut Tone, rate: f32) {
        operators::configure(&self.operators, t, rate);
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let mut sum = (0.0, 0.0);
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + pos * self.detune_cents / 100.0 + c.lfo * self.lfo_pitch;
            let ops = operators::matrix(
                t,
                c,
                &self.operators,
                &self.fm,
                &self.ring,
                u,
                pitch,
                self.lfo_index,
            );
            let raw = ops.iter().zip(self.output).map(|(v, g)| v * g).sum::<f32>();
            let pan = t.pan(u, pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let (gain, damping) = t.filter_coefficients(c.cutoff, self.resonance, c.rate);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            let dry = t.drive(input / self.unison as f32, self.drive);
            let low = t.svf[side].tick(dry, gain, damping).0;
            dry + (low - dry) * self.filter_mix
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(MatrixSynth, MatrixSynthParams);
