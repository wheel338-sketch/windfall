//! Lush Space: a moving algorithmic hall built from two cross-coupled
//! allpass tanks. Host integration is described in the lush seam document.

use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::{clean, flush};
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const TANK_MS: [[f32; 4]; 2] = [[19.0, 151.0, 47.0, 211.0], [23.0, 173.0, 61.0, 233.0]];
const DIFFUSER_MS: [[f32; 2]; 2] = [[5.0, 11.0], [7.0, 13.0]];
const EARLY_MS: [[f32; 5]; 2] = [
    [17.0, 31.0, 53.0, 79.0, 107.0],
    [23.0, 41.0, 67.0, 89.0, 113.0],
];
const EARLY_GAIN: [f32; 5] = [0.55, -0.37, 0.29, 0.21, -0.15];
const ALLPASS_GAIN: f32 = 0.55;
const MIN_DECAY_S: f32 = 0.18;
const RAMPS: usize = 12;

mod pitch;

/// Settings of [`LushSpace`]. Decay is nominal low-frequency RT60.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct LushSpaceParams {
    /// Seconds, 0..8, default 3.5. Zero selects a short 0.18 second decay.
    pub decay_s: f32,
    /// Milliseconds before the hall input, 0..120, default 20.
    pub pre_delay_ms: f32,
    /// High-frequency loss in the feedback loop, 0..1, default 0.45.
    pub damping: f32,
    /// Independent early-reflection gain, 0..1, default 0.35.
    pub early_level: f32,
    /// Modulated allpass speed in Hz, 0..2, default 0.27.
    pub modulation_rate_hz: f32,
    /// Modulated allpass delay swing in milliseconds, 0..2, default 0.7.
    pub modulation_depth_ms: f32,
    /// Wet stereo width, 0..1, default 1. Zero retains the wet mid signal.
    pub width: f32,
    /// Dry/wet balance, 0..1, default 0.3.
    pub mix: f32,
    /// Wet feed-forward pitch voice, -12..12 semitones. No pitch feedback.
    pub pitch_semitones: f32,
    /// Share of pitched wet sound. Zero preserves the legacy hall exactly.
    pub pitch_amount: f32,
    /// Acoustic spacing, 0..1; 0.5 preserves the original tank dimensions.
    pub size: f32,
}

impl Default for LushSpaceParams {
    fn default() -> Self {
        Self {
            decay_s: 3.5,
            pre_delay_ms: 20.0,
            damping: 0.45,
            early_level: 0.35,
            modulation_rate_hz: 0.27,
            modulation_depth_ms: 0.7,
            width: 1.0,
            mix: 0.3,
            pitch_semitones: 12.0,
            pitch_amount: 0.0,
            size: 0.5,
        }
    }
}

param_set!(LushSpaceParams, "Lush Space", {
    float [decay_s] "decayS" "Decay" { Seconds, Linear, 0.0, 8.0, 3.5 }
    float [pre_delay_ms] "preDelayMs" "Pre-delay" { Milliseconds, Linear, 0.0, 120.0, 20.0 }
    float [damping] "damping" "High-frequency damping" { Fraction, Linear, 0.0, 1.0, 0.45 }
    float [early_level] "earlyLevel" "Early reflections" { Fraction, Linear, 0.0, 1.0, 0.35 }
    float [modulation_rate_hz] "modulationRateHz" "Modulation rate" { Hertz, Linear, 0.0, 2.0, 0.27 }
    float [modulation_depth_ms] "modulationDepthMs" "Modulation depth" { Milliseconds, Linear, 0.0, 2.0, 0.7 }
    float [width] "width" "Width" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [pitch_semitones] "pitchSemitones" "Pitch shift" { Semitones, Linear, -12.0, 12.0, 12.0 }
    float [pitch_amount] "pitchAmount" "Pitch amount" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [size] "size" "Size" { Fraction, Linear, 0.0, 1.0, 0.5 }
});

/// A stereo hall with serial diffusion and cross-coupled long delay tanks.
/// Construct and prepare away from the audio thread. All other Effect calls
/// use fixed storage; modulation and control ramps advance once per sample.
pub struct LushSpace {
    params: LushSpaceParams,
    rate: f32,
    prepared: bool,
    fresh: bool,
    ramps: [LinearRamp; RAMPS],
    pre: [DelayLine; 2],
    early: [DelayLine; 2],
    diffusers: [[DelayLine; 2]; 2],
    diffuser_lengths: [[usize; 2]; 2],
    tanks: [[DelayLine; 4]; 2],
    tank_lengths: [[usize; 4]; 2],
    damping: [f32; 2],
    returns: [f32; 2],
    phases: [f64; 2],
    idle: usize,
    lifetime: usize,
    tail_gain: LinearRamp,
    pitch: pitch::PitchVoice,
}

impl Default for LushSpace {
    fn default() -> Self {
        let mut hall = Self {
            params: LushSpaceParams::default(),
            rate: 48_000.0,
            prepared: false,
            fresh: true,
            ramps: [LinearRamp::new(0.0); RAMPS],
            pre: std::array::from_fn(|_| DelayLine::default()),
            early: std::array::from_fn(|_| DelayLine::default()),
            diffusers: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            diffuser_lengths: [[1; 2]; 2],
            tanks: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            tank_lengths: [[1; 4]; 2],
            damping: [0.0; 2],
            returns: [0.0; 2],
            phases: [0.0, 0.37],
            idle: 0,
            lifetime: 0,
            tail_gain: LinearRamp::new(1.0),
            pitch: pitch::PitchVoice::default(),
        };
        hall.apply();
        hall
    }
}

fn frames(ms: f32, rate: f32) -> usize {
    samples(ms, rate).round().max(1.0) as usize
}

fn samples(ms: f32, rate: f32) -> f32 {
    (f64::from(ms) * 0.001 * f64::from(rate)) as f32
}

fn audio(sample: f32) -> f32 {
    flush(clean(sample, -1_000.0, 1_000.0, 0.0))
}

fn state(sample: f32) -> f32 {
    flush(clean(sample, -1.0e6, 1.0e6, 0.0))
}

fn allpass(line: &mut DelayLine, delay: f32, input: f32) -> f32 {
    // Linear interpolation is a convex read, avoiding interpolation overshoot
    // in the recursive tank. Its mild treble loss is part of this hall's sound.
    let delayed = line.tap_linear(delay);
    let into = state(input + ALLPASS_GAIN * delayed);
    line.push(into);
    state(delayed - ALLPASS_GAIN * into)
}

impl LushSpace {
    fn apply(&mut self) {
        let p = self.params;
        let decay = p.decay_s.max(MIN_DECAY_S);
        let scale = 0.4 + 1.2 * p.size;
        let feedback = TANK_MS.map(|times| {
            // One sample of cross-coupling storage belongs to each loop.
            let length: usize = times.iter().map(|ms| frames(*ms * scale, self.rate)).sum();
            (-6.907_755 * (length + 1) as f32 / (decay * self.rate)).exp()
        });
        let cutoff = 1_100.0 + 16_900.0 * (1.0 - p.damping).powi(2);
        let pole = (-std::f32::consts::TAU * cutoff.min(self.rate * 0.45) / self.rate).exp();
        let values = [
            samples(p.pre_delay_ms, self.rate),
            p.early_level,
            pole,
            p.modulation_rate_hz,
            samples(p.modulation_depth_ms, self.rate),
            p.width,
            p.mix,
            feedback[0],
            feedback[1],
            (p.pitch_semitones / 12.0).exp2(),
            p.pitch_amount,
            scale,
        ];
        let glide = if self.fresh {
            0
        } else {
            frames(20.0, self.rate) as u32
        };
        for (ramp, value) in self.ramps.iter_mut().zip(values) {
            ramp.set_target(value, glide);
        }
        // Keep a conservative bound for audio already circulating when decay
        // is shortened. Reset starts a new lifetime at the current settings.
        let pitch_history = if p.pitch_amount > 0.0 {
            self.pitch.history()
        } else {
            0
        };
        let lifetime = ((1.5 * decay + 0.77 * scale) * self.rate).ceil() as usize + pitch_history;
        self.lifetime = if self.fresh {
            lifetime
        } else {
            self.lifetime.max(lifetime)
        };
    }

    fn clear_audio(&mut self) {
        for line in self
            .pre
            .iter_mut()
            .chain(&mut self.early)
            .chain(self.diffusers.iter_mut().flatten())
            .chain(self.tanks.iter_mut().flatten())
        {
            line.clear();
        }
        self.damping = [0.0; 2];
        self.returns = [0.0; 2];
        self.pitch.clear();
    }

    fn tick(&mut self, dry: [f32; 2], controls: [f32; RAMPS]) -> [f32; 2] {
        let [
            pre,
            early_level,
            pole,
            hz,
            depth,
            _,
            _,
            feedback_l,
            feedback_r,
            _,
            _,
            scale,
        ] = controls;
        let feedback = [feedback_l, feedback_r];
        let old_returns = self.returns;
        let mut wet = [0.0; 2];
        for ch in 0..2 {
            let input = if pre < 1.0 {
                dry[ch] + pre * (self.pre[ch].tap(1) - dry[ch])
            } else {
                self.pre[ch].tap_linear(pre)
            };
            self.pre[ch].push(dry[ch]);
            let mut early = 0.0;
            for (ms, gain) in EARLY_MS[ch].iter().zip(EARLY_GAIN) {
                early += gain
                    * self.early[ch].tap_linear(samples(*ms * scale, self.rate).round().max(1.0));
            }
            self.early[ch].push(input);
            let mut diffused = input;
            for i in 0..2 {
                diffused = allpass(
                    &mut self.diffusers[ch][i],
                    (self.diffuser_lengths[ch][i] as f32 * scale).max(1.0),
                    diffused,
                );
            }

            self.phases[ch] = (self.phases[ch] + f64::from(hz) / f64::from(self.rate)).fract();
            let swing = (std::f64::consts::TAU * self.phases[ch]).sin() as f32 * depth;
            let delay = (self.tank_lengths[ch][0] as f32 * scale + swing)
                .clamp(1.0, self.tanks[ch][0].max_delay().max(1) as f32);
            let fed = 0.4 * diffused + feedback[ch] * old_returns[1 - ch];
            let first = allpass(&mut self.tanks[ch][0], delay, fed);
            let echo =
                self.tanks[ch][1].tap_linear((self.tank_lengths[ch][1] as f32 * scale).max(1.0));
            self.tanks[ch][1].push(first);
            self.damping[ch] = state((1.0 - pole) * echo + pole * self.damping[ch]);
            let second = allpass(
                &mut self.tanks[ch][2],
                (self.tank_lengths[ch][2] as f32 * scale).max(1.0),
                self.damping[ch],
            );
            let returned =
                self.tanks[ch][3].tap_linear((self.tank_lengths[ch][3] as f32 * scale).max(1.0));
            self.tanks[ch][3].push(second);
            self.returns[ch] = returned;
            wet[ch] = state(0.6 * (echo + returned) + early_level * early);
        }
        wet
    }
}

impl Effect for LushSpace {
    type Params = LushSpaceParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = clean(sample_rate, 1.0, 384_000.0, 48_000.0);
        self.pre = std::array::from_fn(|_| DelayLine::new(frames(120.0, self.rate) + 2));
        self.early = std::array::from_fn(|_| DelayLine::new(frames(113.0 * 1.6, self.rate) + 2));
        self.diffuser_lengths = DIFFUSER_MS.map(|times| times.map(|ms| frames(ms, self.rate)));
        self.diffusers = std::array::from_fn(|ch| {
            std::array::from_fn(|i| DelayLine::new(frames(DIFFUSER_MS[ch][i] * 1.6, self.rate) + 2))
        });
        self.tank_lengths = TANK_MS.map(|times| times.map(|ms| frames(ms, self.rate)));
        self.tanks = std::array::from_fn(|ch| {
            std::array::from_fn(|i| {
                DelayLine::new(frames(TANK_MS[ch][i] * 1.6, self.rate) + frames(2.0, self.rate) + 2)
            })
        });
        self.pitch.prepare(self.rate);
        self.prepared = true;
        self.reset();
    }

    fn reset(&mut self) {
        self.clear_audio();
        self.fresh = true;
        self.phases = [0.0, 0.37];
        self.idle = 0;
        self.lifetime = 0;
        self.tail_gain.snap(1.0);
        self.apply();
    }

    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        self.apply();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let dry = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = dry;
                continue;
            }
            self.fresh = false;
            let controls = std::array::from_fn(|i| self.ramps[i].tick());
            let fade = frames(20.0, self.rate).min(self.lifetime).max(1);
            let expired = self.idle >= self.lifetime;
            if dry != [0.0; 2] {
                self.idle = 0;
                self.tail_gain.set_target(1.0, fade as u32);
            } else {
                self.idle = (self.idle + 1).min(self.lifetime);
                if self.idle > self.lifetime - fade {
                    self.tail_gain
                        .snap((self.lifetime - self.idle) as f32 / fade as f32);
                }
            }
            let wet = if self.idle < self.lifetime {
                let plain = self.tick(dry, controls);
                let pitched = self.pitch.tick(plain, controls[9]);
                if controls[10] == 0.0 {
                    plain
                } else {
                    std::array::from_fn(|ch| plain[ch] + controls[10] * (pitched[ch] - plain[ch]))
                }
            } else {
                if !expired {
                    self.clear_audio();
                }
                [0.0; 2]
            };
            let gain = self.tail_gain.tick();
            let mid = 0.5 * (wet[0] + wet[1]) * gain;
            let side = 0.5 * (wet[0] - wet[1]) * controls[5] * gain;
            *l = state(dry[0] + controls[6] * (mid + side - dry[0]));
            *r = state(dry[1] + controls[6] * (mid - side - dry[1]));
        }
    }

    fn tail_samples(&self) -> usize {
        self.lifetime
    }

    fn gap_samples(&self) -> usize {
        // Conservative under automation and the slow serial diffusion.
        self.tail_samples()
    }
}

#[cfg(test)]
mod tests;
