use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::lfo::LfoShape;
use crate::blocks::math::clean;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};

use super::{Controls, balance, rate};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PanLfoParams {
    pub rate_hz: f32,
    pub depth: f32,
    pub pan: f32,
    pub volume_depth: f32,
    pub gain: f32,
    /// The shape of one cycle; default sine.
    pub shape: LfoShape,
    /// Follows the tempo: one cycle lasts `beats` quarter notes and
    /// `rate_hz` is ignored. Default off.
    pub sync: bool,
    /// Quarter notes per cycle while synced, 1/16 to 16; default 1.
    pub beats: f32,
}

impl Default for PanLfoParams {
    fn default() -> Self {
        Self {
            rate_hz: 1.0,
            depth: 1.0,
            pan: 0.0,
            volume_depth: 0.0,
            gain: 1.0,
            shape: LfoShape::Sine,
            sync: false,
            beats: 1.0,
        }
    }
}

param_set!(PanLfoParams, "Pan Motion", {
    float [rate_hz] "rateHz" "Rate" { Hertz, Logarithmic, 0.01, 20.0, 1.0 }
    float [depth] "depth" "Pan Depth" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [pan] "pan" "Pan Center" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [volume_depth] "volumeDepth" "Volume Depth" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [gain] "gain" "Gain" { Gain, Linear, 0.0, 2.0, 1.0 }
    choice [shape] "shape" "Shape" { LfoShape, Sine, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square", Random "random" "Random"] }
    toggle [sync] "sync" "Sync to tempo" { false }
    float [beats] "beats" "Cycle" { None, Logarithmic, 0.0625, 16.0, 1.0 }
});

/// Where the random shape's sequence starts after a reset.
const RANDOM_SEED: u32 = 0x9e37_79b9;

/// Time the shapes that jump take to cover 63% of a jump, so a square or
/// random pan does not click.
const JUMP_SMOOTHING_MS: f32 = 2.0;

pub struct PanLfo {
    controls: Controls<5>,
    sample_rate: f32,
    phase: f64,
    params: PanLfoParams,
    bpm: f32,
    /// State of the random shape's generator, and the level it holds for
    /// the cycle under way.
    random: u32,
    held: f32,
    /// The wave as last put out, which a shape that jumps glides from.
    wave: f32,
    jump_step: f32,
}

impl Default for PanLfo {
    fn default() -> Self {
        let mut motion = Self {
            controls: Controls::new([1.0, 1.0, 0.0, 0.0, 1.0]),
            sample_rate: 48_000.0,
            phase: 0.0,
            params: PanLfoParams::default(),
            bpm: 120.0,
            random: RANDOM_SEED,
            held: 0.0,
            wave: 0.0,
            jump_step: 1.0,
        };
        motion.held = motion.draw();
        motion
    }
}

impl PanLfo {
    /// The next level of the random shape, from -1 to 1.
    fn draw(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        (f64::from(self.random) / f64::from(u32::MAX) * 2.0 - 1.0) as f32
    }

    /// Hands the controls their targets. The rate is the tempo's while
    /// synced.
    fn apply(&mut self) {
        let p = self.params;
        let hz = if p.sync {
            self.bpm / 60.0 / p.beats
        } else {
            p.rate_hz
        };
        self.controls
            .set([hz, p.depth, p.pan, p.volume_depth, p.gain]);
    }

    fn tick(&mut self) -> [f32; 2] {
        let [hz, depth, center, volume, gain] = self.controls.tick();
        let phase = self.phase;
        let (raw, jumps) = match self.params.shape {
            LfoShape::Sine => ((std::f64::consts::TAU * phase).sin() as f32, false),
            LfoShape::Triangle => {
                let at = phase as f32;
                let value = if at < 0.25 {
                    4.0 * at
                } else if at < 0.75 {
                    2.0 - 4.0 * at
                } else {
                    4.0 * at - 4.0
                };
                (value, false)
            }
            LfoShape::Saw => ((2.0 * phase - 1.0) as f32, true),
            LfoShape::Square => (if phase < 0.5 { 1.0 } else { -1.0 }, true),
            LfoShape::Random => (self.held, true),
        };
        let next = self.phase + f64::from(hz / self.sample_rate);
        if next >= 1.0 {
            self.held = self.draw();
        }
        self.phase = next.fract();
        let wave = if jumps {
            self.wave + (raw - self.wave) * self.jump_step
        } else {
            raw
        };
        self.wave = wave;
        [
            (center + depth * wave).clamp(-1.0, 1.0),
            gain * (1.0 - volume * (0.5 + 0.5 * wave)),
        ]
    }

    /// Outputs [bipolar pan, linear gain], one pair per sample.
    pub fn process_control<const N: usize>(&mut self, output: &mut [[f32; 2]; N]) {
        for frame in output {
            *frame = self.tick();
        }
    }

    pub fn process_with_control<const N: usize>(
        &mut self,
        left: &mut [f32; N],
        right: &mut [f32; N],
        output: &mut [[f32; 2]; N],
    ) {
        for ((l, r), frame) in left.iter_mut().zip(right).zip(output) {
            *frame = self.tick();
            [*l, *r] = balance(*l, *r, frame[0], frame[1]);
        }
    }
}

impl Effect for PanLfo {
    type Params = PanLfoParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.jump_step =
            crate::blocks::math::smoothing_coefficient(JUMP_SMOOTHING_MS, self.sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.random = RANDOM_SEED;
        self.held = self.draw();
        self.wave = 0.0;
        self.controls.reset();
    }

    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        self.apply();
    }

    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = clean(bpm, 1.0, 999.0, 120.0);
        self.apply();
    }


    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let [pan, gain] = self.tick();
            [*l, *r] = balance(*l, *r, pan, gain);
        }
    }
}
