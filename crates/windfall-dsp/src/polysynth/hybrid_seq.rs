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
#[serde(rename_all = "camelCase")]
pub enum HybridAlgorithm {
    Serial,
    Pairs,
    Stack,
    Parallel,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct HybridStep {
    pub pitch: i8,
    pub velocity: f32,
    pub enabled: bool,
}
impl Default for HybridStep {
    fn default() -> Self {
        Self {
            pitch: 0,
            velocity: 1.0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct HybridSeqParams {
    pub operators: [MatrixOperator; 6],
    pub algorithm: HybridAlgorithm,
    pub index: f32,
    pub saw_mix: f32,
    pub drive: f32,
    pub chorus: f32,
    pub delay: f32,
    pub reverb: f32,
    pub sequencer: bool,
    pub length: u8,
    pub division: f32,
    pub gate: f32,
    pub steps: [HybridStep; 16],
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
impl Default for HybridSeqParams {
    fn default() -> Self {
        Self {
            operators: [MatrixOperator::default(); 6],
            algorithm: HybridAlgorithm::Pairs,
            index: 1.0,
            saw_mix: 0.2,
            drive: 0.0,
            chorus: 0.0,
            delay: 0.0,
            reverb: 0.0,
            sequencer: false,
            length: 16,
            division: 0.25,
            gate: 0.8,
            steps: [HybridStep::default(); 16],
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
param_set!(HybridSeqParams, "Hybrid sequencer", {
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
choice [algorithm] "algorithm" "Operator algorithm" { HybridAlgorithm, Pairs, [Serial "serial" "Serial chain", Pairs "pairs" "Three pairs", Stack "stack" "Two stacks", Parallel "parallel" "Six carriers"] }
float [index] "index" "FM index" { Ratio, Linear, 0.0, 6.0, 1.0 }
float [saw_mix] "sawMix" "Subtractive saw mix" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [drive] "drive" "Drive" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [chorus] "chorus" "Chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [reverb] "reverb" "Room" { Fraction, Linear, 0.0, 1.0, 0.0 }
toggle [sequencer] "sequencer" "Sequence enabled" { false }
int [length] "length" "Sequence length" { None, 1.0, 16.0, 16.0 }
float [division] "division" "Step beats" { Ratio, Linear, 0.125, 2.0, 0.25 }
float [gate] "gate" "Step gate" { Fraction, Linear, 0.05, 1.0, 0.8 }
int [steps[0].pitch] "steps.0.pitch" "Step 1 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[0].velocity] "steps.0.velocity" "Step 1 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[0].enabled] "steps.0.enabled" "Step 1 enabled" { true }
int [steps[1].pitch] "steps.1.pitch" "Step 2 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[1].velocity] "steps.1.velocity" "Step 2 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[1].enabled] "steps.1.enabled" "Step 2 enabled" { true }
int [steps[2].pitch] "steps.2.pitch" "Step 3 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[2].velocity] "steps.2.velocity" "Step 3 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[2].enabled] "steps.2.enabled" "Step 3 enabled" { true }
int [steps[3].pitch] "steps.3.pitch" "Step 4 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[3].velocity] "steps.3.velocity" "Step 4 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[3].enabled] "steps.3.enabled" "Step 4 enabled" { true }
int [steps[4].pitch] "steps.4.pitch" "Step 5 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[4].velocity] "steps.4.velocity" "Step 5 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[4].enabled] "steps.4.enabled" "Step 5 enabled" { true }
int [steps[5].pitch] "steps.5.pitch" "Step 6 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[5].velocity] "steps.5.velocity" "Step 6 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[5].enabled] "steps.5.enabled" "Step 6 enabled" { true }
int [steps[6].pitch] "steps.6.pitch" "Step 7 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[6].velocity] "steps.6.velocity" "Step 7 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[6].enabled] "steps.6.enabled" "Step 7 enabled" { true }
int [steps[7].pitch] "steps.7.pitch" "Step 8 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[7].velocity] "steps.7.velocity" "Step 8 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[7].enabled] "steps.7.enabled" "Step 8 enabled" { true }
int [steps[8].pitch] "steps.8.pitch" "Step 9 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[8].velocity] "steps.8.velocity" "Step 9 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[8].enabled] "steps.8.enabled" "Step 9 enabled" { true }
int [steps[9].pitch] "steps.9.pitch" "Step 10 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[9].velocity] "steps.9.velocity" "Step 10 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[9].enabled] "steps.9.enabled" "Step 10 enabled" { true }
int [steps[10].pitch] "steps.10.pitch" "Step 11 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[10].velocity] "steps.10.velocity" "Step 11 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[10].enabled] "steps.10.enabled" "Step 11 enabled" { true }
int [steps[11].pitch] "steps.11.pitch" "Step 12 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[11].velocity] "steps.11.velocity" "Step 12 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[11].enabled] "steps.11.enabled" "Step 12 enabled" { true }
int [steps[12].pitch] "steps.12.pitch" "Step 13 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[12].velocity] "steps.12.velocity" "Step 13 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[12].enabled] "steps.12.enabled" "Step 13 enabled" { true }
int [steps[13].pitch] "steps.13.pitch" "Step 14 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[13].velocity] "steps.13.velocity" "Step 14 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[13].enabled] "steps.13.enabled" "Step 14 enabled" { true }
int [steps[14].pitch] "steps.14.pitch" "Step 15 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[14].velocity] "steps.14.velocity" "Step 15 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[14].enabled] "steps.14.enabled" "Step 15 enabled" { true }
int [steps[15].pitch] "steps.15.pitch" "Step 16 pitch" { Semitones, -24.0, 24.0, 0.0 }
float [steps[15].velocity] "steps.15.velocity" "Step 16 velocity" { Fraction, Linear, 0.0, 1.0, 1.0 }
toggle [steps[15].enabled] "steps.15.enabled" "Step 16 enabled" { true }
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
impl Patch for HybridSeqParams {
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
        super::runtime::envelope(&mut t.sequence_env, self.envelope, rate * 2.0);
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let mut pitch = c.pitch;
        let mut velocity = 1.0;
        if self.sequencer {
            let samples = core::division_seconds(self.division, c.bpm) * c.rate * 0.5;
            let position = t.elapsed as f64 / samples as f64;
            let step = position.floor() as u64;
            if c.held && step != t.sequence_step {
                t.sequence_step = step;
                let s = &self.steps[step as usize % self.length as usize];
                t.sequence_open = s.enabled && s.velocity > 0.0;
                if t.sequence_open {
                    t.sequence_env.gate_on();
                    for op in &mut t.operators {
                        op.gate_on();
                    }
                } else {
                    t.sequence_env.gate_off();
                    for op in &mut t.operators {
                        op.gate_off();
                    }
                }
            }
            if t.sequence_open && (!c.held || position.fract() >= self.gate as f64) {
                t.sequence_open = false;
                t.sequence_env.gate_off();
                for op in &mut t.operators {
                    op.gate_off();
                }
            }
            let s = &self.steps[t.sequence_step.wrapping_rem(self.length as u64) as usize];
            pitch += s.pitch as f32;
            velocity = s.velocity * t.sequence_env.tick();
        }
        let mut fm = [[0.0; 6]; 6];
        let ring = [[0.0; 6]; 6];
        let mut outputs = [0.0; 6];
        match self.algorithm {
            HybridAlgorithm::Serial => {
                outputs[0] = 1.0;
                for (to, row) in fm.iter_mut().enumerate().take(5) {
                    row[to + 1] = self.index;
                }
            }
            HybridAlgorithm::Pairs => {
                for to in [0, 2, 4] {
                    outputs[to] = 1.0 / 3.0;
                    fm[to][to + 1] = self.index;
                }
            }
            HybridAlgorithm::Stack => {
                outputs[0] = 0.5;
                outputs[3] = 0.5;
                fm[0][1] = self.index;
                fm[1][2] = self.index;
                fm[3][4] = self.index;
                fm[4][5] = self.index;
            }
            HybridAlgorithm::Parallel => outputs = [1.0 / 6.0; 6],
        }
        let mut sum = (0.0, 0.0);
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let p = pitch + pos * self.detune_cents / 100.0;
            let ops = operators::matrix(t, c, &self.operators, &fm, &ring, u, p, 0.0);
            let fm = ops.iter().zip(outputs).map(|(v, g)| v * g).sum::<f32>();
            // A separate BLEP saw lives beside the operator bank, never masquerading
            // as an FM preset. Its phase is not one of the operator phases.
            let phase = t.sample_phase[u];
            let inc = core::increment(p, c.rate);
            let saw = crate::blocks::oscillator::saw(phase, inc);
            core::advance(&mut t.sample_phase[u], inc);
            let raw = (fm * (1.0 - self.saw_mix) + saw * self.saw_mix) * velocity;
            let pan = core::pan_gains(pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let filter = |t: &mut Tone, side: usize, input: f32| {
            t.svf[side]
                .tick(
                    core::drive(input / self.unison as f32, self.drive),
                    core::swept_gain(c.cutoff, 0.0, c.rate),
                    core::svf_damping(self.resonance),
                )
                .0
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(HybridSeq, HybridSeqParams);
