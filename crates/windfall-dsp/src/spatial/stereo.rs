use super::common::{Controls, Lowpass, Tail, audio, bounded, frames, pole, rate, tap};
use crate::blocks::delay_line::DelayLine;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Haas decorrelation is injected as opposite side signals, so summing the
/// outputs retains the original mid exactly, including during automation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SpreaderParams {
    pub width: f32,
    pub haas_ms: f32,
    pub amount: f32,
    pub mix: f32,
}
impl Default for SpreaderParams {
    fn default() -> Self {
        Self {
            width: 1.2,
            haas_ms: 9.0,
            amount: 0.3,
            mix: 1.0,
        }
    }
}
param_set!(SpreaderParams, "Spreader", {
    float [width] "width" "Width" { None, Linear, 0.0, 2.0, 1.2 }
    float [haas_ms] "haasMs" "Haas delay" { Milliseconds, Linear, 0.0, 30.0, 9.0 }
    float [amount] "amount" "Haas amount" { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
});
pub struct Spreader {
    params: SpreaderParams,
    controls: Controls<4>,
    line: DelayLine,
    rate: f32,
    prepared: bool,
}
impl Default for Spreader {
    fn default() -> Self {
        Self {
            params: SpreaderParams::default(),
            controls: Controls::default(),
            line: DelayLine::default(),
            rate: 48_000.0,
            prepared: false,
        }
    }
}
impl Spreader {
    fn apply(&mut self) {
        let p = self.params;
        self.controls.set([p.width, p.haas_ms, p.amount, p.mix]);
    }
}
impl Effect for Spreader {
    type Params = SpreaderParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.line = DelayLine::new(frames(30.0, self.rate) + 2);
        self.controls.prepare(self.rate);
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.line.clear();
        self.controls.reset();
        self.apply();
    }
    fn set_params(&mut self, p: &Self::Params) {
        self.params = p.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let [dl, dr] = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = [dl, dr];
                continue;
            }
            let [width, ms, amount, mix] = self.controls.tick();
            let mid = 0.5 * (dl + dr);
            let d = super::common::samples(ms, self.rate);
            let delayed = if d < 1.0 {
                mid + d * (self.line.tap(1) - mid)
            } else {
                tap(&self.line, d)
            };
            self.line.push(mid);
            let side = width * (0.5 * (dl - dr) + amount * 0.5 * (mid - delayed));
            *l = bounded(dl + mix * (mid + side - dl));
            *r = bounded(dr + mix * (mid - side - dr));
        }
    }
    fn tail_samples(&self) -> usize {
        frames(30.0, self.rate) + 1
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
    fn warm_up_samples(&self) -> usize {
        self.tail_samples()
    }
    fn delay_readiness_samples(&self) -> usize {
        self.tail_samples()
    }
}

/// Frequency-selective side processing: a two-pole highpass removes bass from
/// S, while M is preserved. Matrix coefficients alone cannot do this.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StereoEnhancerParams {
    pub width: f32,
    pub bass_hz: f32,
    pub bass_lock: f32,
    pub balance: f32,
    pub mix: f32,
    /// Milliseconds one side is held back, -20 to 20; default 0. Above zero
    /// delays the right side, below zero the left. The other side is not
    /// delayed, so the effect adds no latency.
    pub phase_offset_ms: f32,
    /// Turns one side upside down; default off.
    pub invert: StereoInvert,
}
/// Which side [`StereoEnhancer`] turns upside down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StereoInvert {
    #[default]
    Off,
    Left,
    Right,
}
/// The longest phase offset, in milliseconds either way.
pub const MAX_PHASE_OFFSET_MS: f32 = 20.0;
impl Default for StereoEnhancerParams {
    fn default() -> Self {
        Self {
            width: 1.25,
            bass_hz: 180.0,
            bass_lock: 1.0,
            balance: 0.0,
            mix: 1.0,
            phase_offset_ms: 0.0,
            invert: StereoInvert::Off,
        }
    }
}
param_set!(StereoEnhancerParams, "Stereo Enhancer", {
    float [width] "width" "Width" { None, Linear, 0.0, 2.0, 1.25 }
    float [bass_hz] "bassHz" "Mono bass crossover" { Hertz, Logarithmic, 40.0, 1000.0, 180.0 }
    float [bass_lock] "bassLock" "Mono bass lock" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [balance] "balance" "Balance" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [phase_offset_ms] "phaseOffsetMs" "Phase offset" { Milliseconds, Linear, -MAX_PHASE_OFFSET_MS, MAX_PHASE_OFFSET_MS, 0.0 }
    choice [invert] "invert" "Invert" { StereoInvert, Off, [Off "off" "Off", Left "left" "Left", Right "right" "Right"] }
});
pub struct StereoEnhancer {
    params: StereoEnhancerParams,
    // Width, log crossover, lock, balance, mix, offset in ms, and the sign
    // of each side as a gain, so that an inversion glides through zero.
    controls: Controls<8>,
    bass: [Lowpass; 2],
    /// What each side put out, for the side the phase offset holds back.
    offset: [DelayLine; 2],
    tail: Tail,
    rate: f32,
    prepared: bool,
}
impl Default for StereoEnhancer {
    fn default() -> Self {
        Self {
            params: StereoEnhancerParams::default(),
            controls: Controls::default(),
            bass: [Lowpass::default(); 2],
            offset: [DelayLine::default(), DelayLine::default()],
            tail: Tail::default(),
            rate: 48_000.0,
            prepared: false,
        }
    }
}
impl StereoEnhancer {
    fn apply(&mut self) {
        let p = self.params;
        let sign = |side: StereoInvert| if p.invert == side { -1.0 } else { 1.0 };
        self.controls.set([
            p.width,
            p.bass_hz.ln(),
            p.bass_lock,
            p.balance,
            p.mix,
            p.phase_offset_ms,
            sign(StereoInvert::Left),
            sign(StereoInvert::Right),
        ]);
    }
}
impl Effect for StereoEnhancer {
    type Params = StereoEnhancerParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.controls.prepare(self.rate);
        self.tail.prepare(0.5, self.rate);
        let longest = frames(MAX_PHASE_OFFSET_MS, self.rate) + 2;
        self.offset = [DelayLine::new(longest), DelayLine::new(longest)];
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.bass = [Lowpass::default(); 2];
        for line in &mut self.offset {
            line.clear();
        }
        self.tail.reset();
        self.controls.reset();
        self.apply();
    }
    fn set_params(&mut self, p: &Self::Params) {
        self.params = p.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let dry = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = dry;
                continue;
            }
            let [width, hz, lock, balance, mix, offset, flip_left, flip_right] =
                self.controls.tick();
            let (gain, expired) = self.tail.tick(dry);
            let mid = 0.5 * (dry[0] + dry[1]);
            let side = 0.5 * (dry[0] - dry[1]);
            let mut high = side;
            let p = pole(hz.exp(), self.rate);
            for bass in &mut self.bass {
                high -= bass.tick(high, p);
            }
            let side = width * (side + lock * (high * gain - side));
            if expired {
                self.bass = [Lowpass::default(); 2];
                for line in &mut self.offset {
                    line.clear();
                }
            }
            let mut wet = [
                (mid + side) * (1.0 - balance).min(1.0) * flip_left,
                (mid - side) * (1.0 + balance).min(1.0) * flip_right,
            ];
            // The phase offset holds one side back. At zero neither line is
            // read, so the default is the signal itself.
            let late = [(-offset).max(0.0), offset.max(0.0)];
            for (channel, line) in self.offset.iter_mut().enumerate() {
                let now = wet[channel];
                let delay = super::common::samples(late[channel], self.rate);
                if delay >= 1.0 {
                    wet[channel] = tap(line, delay);
                } else if delay > 0.0 {
                    wet[channel] = now + delay * (line.tap(1) - now);
                }
                line.push(now);
            }
            *l = bounded(dry[0] + mix * (wet[0] - dry[0]));
            *r = bounded(dry[1] + mix * (wet[1] - dry[1]));
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
}
