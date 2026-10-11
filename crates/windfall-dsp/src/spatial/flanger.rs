//! One to four independently swept feedback combs in series, not parallel
//! voices. Each comb in the stack adds its own moving set of notches.
use super::common::{
    Controls, Lowpass, Tail, advance, audio, bounded, frames, pole, rate, sine, tap,
};
use crate::blocks::delay_line::DelayLine;
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StackedFlangerParams {
    pub delay_ms: f32,
    pub depth_ms: f32,
    pub rate1_hz: f32,
    pub rate2_hz: f32,
    pub feedback: f32,
    pub tone_hz: f32,
    pub spread: f32,
    pub mix: f32,
    /// How many combs are stacked, 1 to 4. Default 2, which is what a
    /// project saved before the count could be set sounds like.
    pub order: u8,
    pub sync: bool,
    pub cycle_beats: f32,
}

/// Most combs in the stack.
pub const FLANGER_MAX_ORDER: usize = 4;

/// Where each comb's sweep starts, in cycles.
const START_PHASES: [f64; FLANGER_MAX_ORDER] = [0.0, 0.31, 0.57, 0.83];

/// How much shorter comb `stage` is than the first: each is 13% of the
/// first comb's time shorter than the one before.
pub fn comb_time_ratio(stage: usize) -> f32 {
    1.0 - 0.13 * stage as f32
}

impl Default for StackedFlangerParams {
    fn default() -> Self {
        Self {
            delay_ms: 0.8,
            depth_ms: 3.0,
            rate1_hz: 0.23,
            rate2_hz: 0.41,
            feedback: 0.45,
            tone_hz: 8000.0,
            spread: 0.75,
            mix: 0.65,
            order: 2,
            sync: false,
            cycle_beats: 4.0,
        }
    }
}
param_set!(StackedFlangerParams, "Stacked Flanger", {
    float [delay_ms] "delayMs" "Delay" { Milliseconds, Linear, 0.1, 6.0, 0.8 }
    float [depth_ms] "depthMs" "Depth" { Milliseconds, Linear, 0.0, 8.0, 3.0 }
    float [rate1_hz] "rate1Hz" "First rate" { Hertz, Linear, 0.0, 5.0, 0.23 }
    float [rate2_hz] "rate2Hz" "Second rate" { Hertz, Linear, 0.0, 5.0, 0.41 }
    float [feedback] "feedback" "Feedback" { Fraction, Linear, -0.75, 0.75, 0.45 }
    float [tone_hz] "toneHz" "Feedback tone" { Hertz, Logarithmic, 500.0, 20_000.0, 8000.0 }
    float [spread] "spread" "Spread" { Fraction, Linear, 0.0, 1.0, 0.75 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.65 }
    int [order] "order" "Order" { None, 1, 4, 2 }
    toggle [sync] "sync" "Tempo sync" { false }
    float [cycle_beats] "cycleBeats" "Cycle (beats)" { None, Logarithmic, 0.125, 16.0, 4.0 }
});
pub struct StackedFlanger {
    params: StackedFlangerParams,
    tempo: f32,
    controls: Controls<8>,
    lines: [[DelayLine; 2]; FLANGER_MAX_ORDER],
    filters: [[Lowpass; 2]; FLANGER_MAX_ORDER],
    phases: [f64; FLANGER_MAX_ORDER],
    /// How much each comb is in the stack, 0 or 1 at rest. A comb that is
    /// switched in or out glides.
    weights: [LinearRamp; FLANGER_MAX_ORDER],
    tail: Tail,
    rate: f32,
    prepared: bool,
}
impl Default for StackedFlanger {
    fn default() -> Self {
        Self {
            params: StackedFlangerParams::default(),
            tempo: 120.0,
            controls: Controls::default(),
            lines: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            filters: [[Lowpass::default(); 2]; FLANGER_MAX_ORDER],
            phases: START_PHASES,
            weights: std::array::from_fn(|stage| {
                LinearRamp::new(if stage < 2 { 1.0 } else { 0.0 })
            }),
            tail: Tail::default(),
            rate: 48_000.0,
            prepared: false,
        }
    }
}
impl StackedFlanger {
    fn apply(&mut self) {
        let p = self.params;
        let glide = self.controls.glide();
        for (stage, weight) in self.weights.iter_mut().enumerate() {
            weight.set_target(
                if stage < usize::from(p.order) {
                    1.0
                } else {
                    0.0
                },
                glide,
            );
        }
        self.controls.set([
            p.delay_ms,
            p.depth_ms,
            super::common::modulation_hz(p.rate1_hz, p.sync, p.cycle_beats, self.tempo),
            if p.sync {
                super::common::modulation_hz(p.rate2_hz, true, p.cycle_beats, self.tempo) * 1.382
            } else {
                p.rate2_hz
            },
            p.feedback,
            p.tone_hz,
            p.spread,
            p.mix,
        ]);
    }
    fn clear(&mut self) {
        for stage in &mut self.lines {
            for line in stage {
                line.clear();
            }
        }
        self.filters = [[Lowpass::default(); 2]; FLANGER_MAX_ORDER];
    }
}
impl Effect for StackedFlanger {
    type Params = StackedFlangerParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.lines = std::array::from_fn(|_| {
            std::array::from_fn(|_| DelayLine::new(frames(16.0, self.rate) + 2))
        });
        self.controls.prepare(self.rate);
        self.tail.prepare(2.0, self.rate);
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.clear();
        self.phases = START_PHASES;
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
            let dry = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = dry;
                continue;
            }
            let [base, depth, r1, r2, feed, tone, spread, mix] = self.controls.tick();
            let (gain, expired) = self.tail.tick(dry);
            let mut wet = dry;
            let p = pole(tone, self.rate);
            for stage in 0..FLANGER_MAX_ORDER {
                let weight = self.weights[stage].tick();
                if weight == 0.0 {
                    // A comb that is out of the stack fills with silence,
                    // so it comes back in without an old sound in it.
                    for ch in 0..2 {
                        self.lines[stage][ch].push(0.0);
                        self.filters[stage][ch].clear();
                    }
                    continue;
                }
                for (ch, value) in wet.iter_mut().enumerate() {
                    let motion =
                        0.5 + 0.5 * sine(self.phases[stage] + ch as f64 * spread as f64 * 0.5);
                    let time = (base + depth * motion) * comb_time_ratio(stage);
                    let delayed = tap(
                        &self.lines[stage][ch],
                        super::common::samples(time, self.rate),
                    );
                    let back = self.filters[stage][ch].tick(delayed, p);
                    // The second comb receives the first comb's mixed output.
                    self.lines[stage][ch].push(bounded(*value + feed * back));
                    let combed = bounded(0.5 * (*value + delayed));
                    *value = if weight == 1.0 {
                        combed
                    } else {
                        bounded(*value + weight * (combed - *value))
                    };
                }
            }
            // The third and fourth combs move with the first and second
            // rates, at ratios that keep all four from lining up.
            advance(&mut self.phases[0], r1, self.rate);
            advance(&mut self.phases[1], r2, self.rate);
            advance(&mut self.phases[2], r1 * 0.618, self.rate);
            advance(&mut self.phases[3], r2 * 1.382, self.rate);
            if expired {
                self.clear();
            }
            *l = bounded(dry[0] + mix * (wet[0] * gain - dry[0]));
            *r = bounded(dry[1] + mix * (wet[1] * gain - dry[1]));
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
    fn warm_up_samples(&self) -> usize {
        // Four serial combs can hold more than the old two-comb warmup.
        frames(64.0, self.rate)
    }
    fn delay_readiness_samples(&self) -> usize {
        self.warm_up_samples()
    }
}
