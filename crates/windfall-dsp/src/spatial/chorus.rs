//! Two voice choruses with wet-path tone. The bucket chorus has four mellow
//! voices, or one of three modelled bucket-brigade sweeps. The swarm chorus
//! has two to sixteen wide, detuned voices.
use super::bucket::{BUCKET_GUARD_HZ, Bucket, BucketMode, BucketSweep, ticks_for};
use super::common::{
    Controls, Lowpass, Tail, advance, audio, bounded, frames, pole, rate, samples, sine, triangle,
};
use crate::blocks::delay_line::DelayLine;
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most voices the swarm chorus runs.
pub const SWARM_MAX_VOICES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct VintageChorusParams {
    pub delay_ms: f32,
    pub depth_ms: f32,
    pub rate_hz: f32,
    pub spread: f32,
    pub tone_hz: f32,
    pub mix: f32,
    /// Free runs the four voices from the settings above. The other modes
    /// run the modelled two-line sweep, whose rate and delay range are
    /// fixed by the mode; Tone and Mix still apply. Default free, which is
    /// what a project saved before the modes were added sounds like.
    pub mode: BucketMode,
}
impl Default for VintageChorusParams {
    fn default() -> Self {
        Self {
            delay_ms: 8.0,
            depth_ms: 3.0,
            rate_hz: 0.55,
            spread: 0.6,
            tone_hz: 4500.0,
            mix: 0.5,
            mode: BucketMode::Free,
        }
    }
}
param_set!(VintageChorusParams, "Bucket Chorus", {
    float [delay_ms] "delayMs" "Delay" { Milliseconds, Linear, 1.0, 25.0, 8.0 }
    float [depth_ms] "depthMs" "Depth" { Milliseconds, Linear, 0.0, 12.0, 3.0 }
    float [rate_hz] "rateHz" "Rate" { Hertz, Linear, 0.0, 5.0, 0.55 }
    float [spread] "spread" "Spread" { Fraction, Linear, 0.0, 1.0, 0.6 }
    float [tone_hz] "toneHz" "Tone" { Hertz, Logarithmic, 500.0, 20_000.0, 4500.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.5 }
    choice [mode] "mode" "Mode" {
        BucketMode, Free, [Free "free" "Free", One "one" "I", Two "two" "II", Both "both" "I+II"]
    }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct HyperChorusParams {
    pub delay_ms: f32,
    pub depth_ms: f32,
    pub rate_hz: f32,
    pub spread: f32,
    pub tone_hz: f32,
    pub mix: f32,
    /// How many detuned voices each side has, 2 to 16. Default 8, which is
    /// what a project saved before the count could be set sounds like.
    pub voices: u8,
}
impl Default for HyperChorusParams {
    fn default() -> Self {
        Self {
            delay_ms: 16.0,
            depth_ms: 7.0,
            rate_hz: 0.8,
            spread: 1.0,
            tone_hz: 16_000.0,
            mix: 0.65,
            voices: 8,
        }
    }
}
param_set!(HyperChorusParams, "Swarm Chorus", {
    float [delay_ms] "delayMs" "Delay" { Milliseconds, Linear, 1.0, 25.0, 16.0 }
    float [depth_ms] "depthMs" "Depth" { Milliseconds, Linear, 0.0, 12.0, 7.0 }
    float [rate_hz] "rateHz" "Rate" { Hertz, Linear, 0.0, 5.0, 0.8 }
    float [spread] "spread" "Spread" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [tone_hz] "toneHz" "Tone" { Hertz, Logarithmic, 500.0, 20_000.0, 16_000.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.65 }
    int [voices] "voices" "Voices" { None, 2, 16, 8 }
});

/// The share of voice `index`'s own rate in the Rate setting. Every voice
/// moves at its own speed, so the voices never line up again.
pub fn voice_rate_ratio(index: usize) -> f32 {
    0.67 + 0.173 * index as f32
}

/// The share of voice `index`'s resting delay in the Delay setting. Each
/// voice sits a little later than the one before.
pub fn voice_delay_ratio(index: usize) -> f32 {
    1.0 + 0.04 * index as f32
}

/// `N` voice slots reading two shared delay lines. The first `base` voices
/// start evenly spaced round the cycle; any beyond start between them.
struct Voices<const N: usize> {
    lines: [DelayLine; 2],
    phases: [f64; N],
    /// How much each voice is heard, 0 or 1 at rest. A voice that is
    /// switched in or out glides, so changing the count does not click.
    weights: [LinearRamp; N],
    tone: [Lowpass; 2],
    base: usize,
    /// Whether odd voices read some of the other side's line.
    crossfeed: bool,
    rate: f32,
}
impl<const N: usize> Voices<N> {
    fn new(base: usize, crossfeed: bool) -> Self {
        let mut voices = Self {
            lines: std::array::from_fn(|_| DelayLine::default()),
            phases: [0.0; N],
            weights: std::array::from_fn(|i| LinearRamp::new(if i < base { 1.0 } else { 0.0 })),
            tone: [Lowpass::default(); 2],
            base,
            crossfeed,
            rate: 48_000.0,
        };
        voices.rewind();
        voices
    }
    fn rewind(&mut self) {
        let base = self.base as f64;
        self.phases = std::array::from_fn(|i| {
            if i < self.base {
                i as f64 / base
            } else {
                ((i - self.base) as f64 + 0.5) / base % 1.0
            }
        });
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = sample_rate;
        self.lines = std::array::from_fn(|_| DelayLine::new(frames(64.0, self.rate) + 4));
    }
    fn reset(&mut self) {
        self.clear();
        self.rewind();
    }
    fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for tone in &mut self.tone {
            tone.clear();
        }
    }
    /// Hears the first `count` voices, over `glide` frames.
    fn set_count(&mut self, count: usize, glide: u32) {
        for (index, weight) in self.weights.iter_mut().enumerate() {
            weight.set_target(if index < count { 1.0 } else { 0.0 }, glide);
        }
    }
    /// One frame of the voices through the tone filter, before any mix.
    fn tick(
        &mut self,
        input: [f32; 2],
        delay: f32,
        depth: f32,
        hz: f32,
        spread: f32,
        tone_pole: f32,
    ) -> [f32; 2] {
        let mut weights = [0.0_f32; N];
        let mut total = 0.0_f32;
        for (weight, ramp) in weights.iter_mut().zip(&mut self.weights) {
            *weight = ramp.tick();
            total += *weight;
        }
        // At least one voice is always fully heard; this is only a guard.
        let total = total.max(1.0e-3);
        let mut wet = [0.0; 2];
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let weight = weights[i];
            if weight > 0.0 {
                for (ch, out) in wet.iter_mut().enumerate() {
                    let offset = spread as f64 * (0.17 + 0.037 * i as f64) * ch as f64;
                    let time = delay * voice_delay_ratio(i)
                        + depth * (0.5 + 0.5 * sine(*phase + offset));
                    let samples = samples(time, self.rate)
                        .clamp(2.0, self.lines[ch].max_delay() as f32 - 2.0);
                    let own = self.lines[ch].tap_cubic(samples);
                    let other = self.lines[1 - ch].tap_cubic(samples);
                    // The swarm distributes alternating voices across both
                    // source channels; the four-voice circuit keeps them apart.
                    let cross = if self.crossfeed {
                        spread * (0.15 + 0.2 * (i % 2) as f32)
                    } else {
                        0.0
                    };
                    *out += weight * (own + cross * (other - own)) / total;
                }
            }
            advance(phase, hz * voice_rate_ratio(i), self.rate);
        }
        for ch in 0..2 {
            self.lines[ch].push(input[ch]);
            wet[ch] = self.tone[ch].tick(wet[ch], tone_pole);
        }
        wet
    }
}

/// The two bucket-brigade lines of the modelled sweep, with the two-pole
/// lowpass ahead of each and the tone filter after it.
#[derive(Default)]
struct Sweep {
    buckets: [Bucket; 2],
    guard: [[Lowpass; 2]; 2],
    tone: [Lowpass; 2],
    phase: f64,
}
impl Sweep {
    fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.guard = [[Lowpass::default(); 2]; 2];
        self.tone = [Lowpass::default(); 2];
    }
    /// One frame. `stereo` is 1 when the right line moves against the left
    /// and 0 when the two move together.
    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        input: [f32; 2],
        hz: f32,
        short_ms: f32,
        long_ms: f32,
        stereo: f32,
        tone_pole: f32,
        rate: f32,
    ) -> [f32; 2] {
        let rise = triangle(self.phase);
        let guard_pole = pole(BUCKET_GUARD_HZ, rate);
        let mut wet = [0.0; 2];
        for (ch, out) in wet.iter_mut().enumerate() {
            let position = if ch == 0 {
                rise
            } else {
                rise + stereo * (1.0 - 2.0 * rise)
            };
            let ms = short_ms + (long_ms - short_ms) * position;
            let [first, second] = &mut self.guard[ch];
            let guarded = second.tick(first.tick(input[ch], guard_pole), guard_pole);
            let delayed = self.buckets[ch].tick(guarded, ticks_for(samples(ms, rate)));
            *out = self.tone[ch].tick(delayed, tone_pole);
        }
        advance(&mut self.phase, hz, rate);
        wet
    }
}

/// Four mellow voices, or a modelled two-line bucket-brigade sweep.
pub struct VintageChorus {
    params: VintageChorusParams,
    voices: Voices<4>,
    sweep: Sweep,
    /// Delay, depth, rate, spread, tone, mix, then how much of the sweep is
    /// heard in place of the voices, and the sweep's rate, shortest and
    /// longest delay and stereo share.
    controls: Controls<11>,
    tail: Tail,
    rate: f32,
    prepared: bool,
}
impl Default for VintageChorus {
    fn default() -> Self {
        Self {
            params: VintageChorusParams::default(),
            voices: Voices::new(4, false),
            sweep: Sweep::default(),
            controls: Controls::default(),
            tail: Tail::default(),
            rate: 48_000.0,
            prepared: false,
        }
    }
}
impl VintageChorus {
    fn apply(&mut self) {
        let p = self.params;
        // While the voices are heard the sweep idles on the first mode, so
        // switching to a sweep never starts from a stale setting.
        let sweep = p.mode.sweep().unwrap_or(BucketSweep::ONE);
        self.controls.set([
            p.delay_ms,
            p.depth_ms,
            p.rate_hz,
            p.spread,
            p.tone_hz,
            p.mix,
            if p.mode == BucketMode::Free { 0.0 } else { 1.0 },
            sweep.rate_hz,
            sweep.min_ms,
            sweep.max_ms,
            if sweep.stereo { 1.0 } else { 0.0 },
        ]);
    }
    fn clear(&mut self) {
        self.voices.clear();
        self.sweep.clear();
    }
}
impl Effect for VintageChorus {
    type Params = VintageChorusParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.voices.prepare(self.rate);
        self.controls.prepare(self.rate);
        self.tail.prepare(0.3, self.rate);
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.voices.reset();
        self.sweep.clear();
        self.sweep.phase = 0.0;
        self.controls.reset();
        self.tail.reset();
        self.apply();
    }
    fn set_params(&mut self, p: &VintageChorusParams) {
        self.params = p.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let input = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = input;
                continue;
            }
            let [
                delay,
                depth,
                hz,
                spread,
                tone,
                mix,
                model,
                sweep_hz,
                short_ms,
                long_ms,
                stereo,
            ] = self.controls.tick();
            let (gain, expired) = self.tail.tick(input);
            let tone_pole = pole(tone, self.rate);
            // Both circuits always run, so either is ready when chosen.
            let voices = self
                .voices
                .tick(input, delay, depth, hz, spread, tone_pole);
            let swept = self.sweep.tick(
                input, sweep_hz, short_ms, long_ms, stereo, tone_pole, self.rate,
            );
            if expired {
                self.clear();
            }
            let mut out = [0.0; 2];
            for ch in 0..2 {
                // At rest on the voices this is the earlier circuit's
                // arithmetic, sample for sample.
                let chosen = if model == 0.0 {
                    voices[ch]
                } else if model == 1.0 {
                    swept[ch]
                } else {
                    voices[ch] + model * (swept[ch] - voices[ch])
                };
                let wet = chosen * gain;
                out[ch] = bounded(input[ch] + mix * (wet - input[ch]));
            }
            [*l, *r] = out;
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        frames(64.0, self.rate) + 2
    }
    fn warm_up_samples(&self) -> usize {
        self.gap_samples()
    }
    fn delay_readiness_samples(&self) -> usize {
        self.gap_samples()
    }
}

/// Two to sixteen wide, detuned voices on each side.
pub struct HyperChorus {
    params: HyperChorusParams,
    voices: Voices<SWARM_MAX_VOICES>,
    controls: Controls<6>,
    tail: Tail,
    rate: f32,
    prepared: bool,
}
impl Default for HyperChorus {
    fn default() -> Self {
        Self {
            params: HyperChorusParams::default(),
            voices: Voices::new(8, true),
            controls: Controls::default(),
            tail: Tail::default(),
            rate: 48_000.0,
            prepared: false,
        }
    }
}
impl HyperChorus {
    fn apply(&mut self) {
        let p = self.params;
        self.voices
            .set_count(usize::from(p.voices), self.controls.glide());
        self.controls.set([
            p.delay_ms, p.depth_ms, p.rate_hz, p.spread, p.tone_hz, p.mix,
        ]);
    }
}
impl Effect for HyperChorus {
    type Params = HyperChorusParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.voices.prepare(self.rate);
        self.controls.prepare(self.rate);
        self.tail.prepare(0.3, self.rate);
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.voices.reset();
        self.controls.reset();
        self.tail.reset();
        self.apply();
    }
    fn set_params(&mut self, p: &HyperChorusParams) {
        self.params = p.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let input = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = input;
                continue;
            }
            let [delay, depth, hz, spread, tone, mix] = self.controls.tick();
            let (gain, expired) = self.tail.tick(input);
            let wet = self
                .voices
                .tick(input, delay, depth, hz, spread, pole(tone, self.rate));
            if expired {
                self.voices.clear();
            }
            *l = bounded(input[0] + mix * (wet[0] * gain - input[0]));
            *r = bounded(input[1] + mix * (wet[1] * gain - input[1]));
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        frames(64.0, self.rate) + 2
    }
    fn warm_up_samples(&self) -> usize {
        self.gap_samples()
    }
    fn delay_readiness_samples(&self) -> usize {
        self.gap_samples()
    }
}
