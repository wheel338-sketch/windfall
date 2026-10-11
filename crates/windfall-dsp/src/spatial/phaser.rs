//! Four to twelve lossless lattice allpasses, with a colored feedback path.
//! With four identical stages it is the circuit of the classic four-stage
//! pedal: two notches that sweep together, sharpened by feedback.
use super::common::{Controls, Lowpass, Tail, advance, audio, bounded, pole, rate, sine};
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most allpass stages on each side.
pub const PHASER_MAX_STAGES: usize = 12;

/// How many allpass stages each side runs. Two stages make one notch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PhaserStages {
    /// Six notches. This is what the effect had before the count could be
    /// chosen, so it stays the default.
    #[default]
    Twelve,
    /// Two notches, as in the classic four-stage pedal.
    Four,
    Six,
    Eight,
}

impl PhaserStages {
    pub fn count(self) -> usize {
        match self {
            PhaserStages::Twelve => 12,
            PhaserStages::Four => 4,
            PhaserStages::Six => 6,
            PhaserStages::Eight => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct VintagePhaserParams {
    pub min_hz: f32,
    pub max_hz: f32,
    pub rate_hz: f32,
    pub feedback: f32,
    pub color_hz: f32,
    pub stereo_phase: f32,
    pub mix: f32,
    /// Allpass stages on each side. Default twelve.
    pub stages: PhaserStages,
    /// How far apart the stages are tuned, 0 to 1. At 0 every stage has
    /// the same corner, as in the pedal, and the notches are as deep as
    /// they get. At 1, the default, each stage sits a little higher than
    /// the one before, which spreads the notches out.
    pub stagger: f32,
    pub sync: bool,
    pub cycle_beats: f32,
}
impl Default for VintagePhaserParams {
    fn default() -> Self {
        Self {
            min_hz: 150.0,
            max_hz: 2400.0,
            rate_hz: 0.3,
            feedback: 0.45,
            color_hz: 2500.0,
            stereo_phase: 0.15,
            mix: 0.5,
            stages: PhaserStages::Twelve,
            stagger: 1.0,
            sync: false,
            cycle_beats: 4.0,
        }
    }
}
param_set!(VintagePhaserParams, "Stage Phaser", {
    float [min_hz] "minHz" "Sweep minimum" { Hertz, Logarithmic, 20.0, 10_000.0, 150.0 }
    float [max_hz] "maxHz" "Sweep maximum" { Hertz, Logarithmic, 20.0, 10_000.0, 2400.0 }
    float [rate_hz] "rateHz" "Rate" { Hertz, Linear, 0.0, 5.0, 0.3 }
    float [feedback] "feedback" "Feedback" { Fraction, Linear, -0.8, 0.8, 0.45 }
    float [color_hz] "colorHz" "Feedback color" { Hertz, Logarithmic, 200.0, 12_000.0, 2500.0 }
    float [stereo_phase] "stereoPhase" "Stereo phase (cycles)" { Fraction, Linear, 0.0, 1.0, 0.15 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.5 }
    choice [stages] "stages" "Stages" {
        PhaserStages, Twelve,
        [Twelve "twelve" "12", Four "four" "4", Six "six" "6", Eight "eight" "8"]
    }
    float [stagger] "stagger" "Stagger" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [sync] "sync" "Tempo sync" { false }
    float [cycle_beats] "cycleBeats" "Cycle (beats)" { None, Logarithmic, 0.125, 16.0, 4.0 }
});

/// The corner of stage `index` as a share of the sweep's center, at a
/// stagger of 1.
pub fn stage_ratio(index: usize) -> f32 {
    0.72 + 0.055 * index as f32
}

pub struct VintagePhaser {
    params: VintagePhaserParams,
    tempo: f32,
    controls: Controls<8>,
    states: [[f32; PHASER_MAX_STAGES]; 2],
    /// How much each stage is in the chain, 0 or 1 at rest. A stage that
    /// is switched in or out glides.
    weights: [LinearRamp; PHASER_MAX_STAGES],
    back: [f32; 2],
    color: [Lowpass; 2],
    phase: f64,
    rate: f32,
    tail: Tail,
    prepared: bool,
}
impl Default for VintagePhaser {
    fn default() -> Self {
        Self {
            params: VintagePhaserParams::default(),
            tempo: 120.0,
            controls: Controls::default(),
            states: [[0.0; PHASER_MAX_STAGES]; 2],
            weights: [LinearRamp::new(1.0); PHASER_MAX_STAGES],
            back: [0.0; 2],
            color: [Lowpass::default(); 2],
            phase: 0.0,
            rate: 48_000.0,
            tail: Tail::default(),
            prepared: false,
        }
    }
}
impl VintagePhaser {
    fn apply(&mut self) {
        let p = self.params;
        let glide = self.controls.glide();
        let count = p.stages.count();
        for (index, weight) in self.weights.iter_mut().enumerate() {
            weight.set_target(if index < count { 1.0 } else { 0.0 }, glide);
        }
        self.controls.set([
            p.min_hz.ln(),
            p.max_hz.ln(),
            super::common::modulation_hz(p.rate_hz, p.sync, p.cycle_beats, self.tempo),
            p.feedback,
            p.color_hz,
            p.stereo_phase,
            p.mix,
            p.stagger,
        ]);
    }
    fn clear(&mut self) {
        self.states = [[0.0; PHASER_MAX_STAGES]; 2];
        self.back = [0.0; 2];
        self.color = [Lowpass::default(); 2];
    }
}
impl Effect for VintagePhaser {
    type Params = VintagePhaserParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.controls.prepare(self.rate);
        self.tail.prepare(3.0, self.rate);
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.clear();
        self.phase = 0.0;
        self.controls.reset();
        self.tail.reset();
        self.apply();
    }
    fn set_params(&mut self, p: &Self::Params) {
        self.params = p.sanitized();
        self.apply();
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.tempo = bpm;
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let input = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = input;
                continue;
            }
            let [lo, hi, hz, feedback, color, stereo, mix, stagger] = self.controls.tick();
            let (gain, expired) = self.tail.tick(input);
            let mut weights = [0.0_f32; PHASER_MAX_STAGES];
            for (weight, ramp) in weights.iter_mut().zip(&mut self.weights) {
                *weight = ramp.tick();
            }
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let motion = 0.5 + 0.5 * sine(self.phase + stereo as f64 * ch as f64);
                let center = (lo.min(hi) + (hi - lo).abs() * motion).exp();
                let mut wet = bounded(input[ch] + feedback * self.back[ch]);
                for (i, state) in self.states[ch].iter_mut().enumerate() {
                    let weight = weights[i];
                    if weight == 0.0 {
                        // A stage that is out of the chain forgets, so it
                        // comes back in without an old sound in it.
                        *state = 0.0;
                        continue;
                    }
                    let ratio = if stagger == 1.0 {
                        stage_ratio(i)
                    } else {
                        1.0 + stagger * (stage_ratio(i) - 1.0)
                    };
                    let frequency = (center * ratio).min(0.45 * self.rate);
                    let g = (std::f32::consts::PI * frequency / self.rate).tan();
                    let a = ((g - 1.0) / (g + 1.0)).clamp(-0.9995, 0.9995);
                    let b = (1.0 - a * a).sqrt();
                    // Orthogonal scattering remains passive while sweeping.
                    let next = a * wet + b * *state;
                    *state = bounded(b * wet - a * *state);
                    wet = if weight == 1.0 {
                        bounded(next)
                    } else {
                        // Between a stage and no stage, which are both
                        // allpass, so the blend cannot exceed its input.
                        bounded(wet + weight * (next - wet))
                    };
                }
                self.back[ch] = self.color[ch].tick(wet, pole(color, self.rate));
                out[ch] = bounded(input[ch] + mix * (wet * gain - input[ch]));
            }
            if expired {
                self.clear();
            }
            advance(&mut self.phase, hz, self.rate);
            [*l, *r] = out;
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
}
