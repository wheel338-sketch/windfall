use super::crossover::Crossover;
use super::params::*;
use crate::balance::{Controls, audio, rate};
use crate::blocks::math::{db_to_gain_exp, flush, level_to_db, smoothing_coefficient};
use crate::effect::{Effect, GainReductionMeter};
use crate::limiter::{Limiter, LimiterParams};
use crate::param::ParamSet;

pub struct BandSplit {
    crossover: Crossover,
    gains: Controls<3>,
    mode: Controls<1>,
}
impl Default for BandSplit {
    fn default() -> Self {
        Self {
            crossover: Crossover::default(),
            gains: Controls::new([1.0; 3]),
            mode: Controls::new([1.0]),
        }
    }
}
impl BandSplit {
    /// Allocation-free separate-output seam. Canonical order is low/mid/high.
    /// In two-band mode mid is zero and high contains the entire upper band.
    /// The returned bands include their smoothed gains.
    pub fn split_frame(&mut self, input: [f32; 2]) -> [[f32; 2]; 3] {
        let bands = self.crossover.tick(input);
        let [low, mid, high] = self.gains.tick();
        // Keep the crossover running in both modes; routing transitions glide.
        let mode = self.mode.tick()[0];
        [
            bands[0].map(|x| audio(x * low)),
            bands[1].map(|x| audio(x * mid * mode)),
            std::array::from_fn(|c| audio(bands[2][c] * high + bands[1][c] * mid * (1.0 - mode))),
        ]
    }
}

impl Effect for BandSplit {
    type Params = BandSplitParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.crossover.prepare(sample_rate);
        self.gains.prepare(sample_rate);
        self.mode.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.crossover.reset();
        self.gains.reset();
        self.mode.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.crossover.set(p.low_crossover_hz, p.high_crossover_hz);
        self.gains.set([
            db_to_gain_exp(p.low_gain_db),
            db_to_gain_exp(if p.bands == BandCount::Two {
                p.high_gain_db
            } else {
                p.mid_gain_db
            }),
            db_to_gain_exp(p.high_gain_db),
        ]);
        self.mode.set([if p.bands == BandCount::Three {
            1.0
        } else {
            0.0
        }]);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let bands = self.split_frame([*l, *r]);
            *l = audio(bands[0][0] + bands[1][0] + bands[2][0]);
            *r = audio(bands[0][1] + bands[1][1] + bands[2][1]);
        }
    }
    fn tail_samples(&self) -> usize {
        self.crossover.tail_samples()
    }
}

struct BandDynamics {
    // threshold, reduction slope, makeup gain, attack keep, release keep.
    controls: Controls<5>,
    reduction_db: f32,
}
impl Default for BandDynamics {
    fn default() -> Self {
        let mut band = Self {
            controls: Controls::new([0.0; 5]),
            reduction_db: 0.0,
        };
        band.set(&BandDynamicsParams::default(), 48_000.0);
        band
    }
}
impl BandDynamics {
    fn set(&mut self, p: &BandDynamicsParams, sample_rate: f32) {
        self.controls.set([
            p.threshold_db,
            if p.ratio >= 100.0 {
                1.0
            } else {
                1.0 - 1.0 / p.ratio
            },
            db_to_gain_exp(p.makeup_db),
            1.0 - smoothing_coefficient(p.attack_ms, sample_rate),
            1.0 - smoothing_coefficient(p.release_ms, sample_rate),
        ]);
    }
    fn reset(&mut self) {
        self.controls.reset();
        self.reduction_db = 0.0;
    }
    fn tick(&mut self, band: [f32; 2], detector: [f32; 2]) -> [f32; 2] {
        let [threshold, slope, makeup, attack, release] = self.controls.tick();
        let level = level_to_db(detector[0].abs().max(detector[1].abs()));
        let wanted = (level - threshold).max(0.0) * slope;
        let keep = if wanted > self.reduction_db {
            attack
        } else {
            release
        };
        self.reduction_db = flush(wanted + (self.reduction_db - wanted) * keep);
        let gain = makeup * db_to_gain_exp(-self.reduction_db);
        band.map(|x| audio(x * gain))
    }
}

struct DynamicsCore {
    audio_split: Crossover,
    key_split: Crossover,
    bands: [BandDynamics; 3],
    params: MultibandCompressorParams,
    sample_rate: f32,
    /// Deepest band reduction in dB since it was last taken.
    deepest: f32,
}
impl Default for DynamicsCore {
    fn default() -> Self {
        Self {
            audio_split: Crossover::default(),
            key_split: Crossover::default(),
            bands: std::array::from_fn(|_| BandDynamics::default()),
            params: MultibandCompressorParams::default(),
            sample_rate: 48_000.0,
            deepest: 0.0,
        }
    }
}
impl DynamicsCore {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = rate(sample_rate).max(8.0);
        self.audio_split.prepare(self.sample_rate);
        self.key_split.prepare(self.sample_rate);
        for band in &mut self.bands {
            band.controls.prepare(self.sample_rate);
        }
        self.reset();
    }
    fn reset(&mut self) {
        self.audio_split.reset();
        self.key_split.reset();
        for band in &mut self.bands {
            band.reset();
        }
        let params = self.params;
        self.set(&params);
    }
    fn set(&mut self, params: &MultibandCompressorParams) {
        self.params = params.sanitized();
        self.audio_split
            .set(self.params.low_crossover_hz, self.params.high_crossover_hz);
        self.key_split
            .set(self.params.low_crossover_hz, self.params.high_crossover_hz);
        for (band, p) in
            self.bands
                .iter_mut()
                .zip([self.params.low, self.params.mid, self.params.high])
        {
            band.set(&p, self.sample_rate);
        }
    }
    fn tick(&mut self, input: [f32; 2], key: [f32; 2]) -> [[f32; 2]; 3] {
        let bands = self.audio_split.tick(input);
        let key = self.key_split.tick(key);
        let out = std::array::from_fn(|i| {
            self.bands[i].tick(
                bands[i],
                if self.params.sidechain {
                    key[i]
                } else {
                    bands[i]
                },
            )
        });
        for band in &self.bands {
            self.deepest = self.deepest.max(band.reduction_db);
        }
        out
    }
    /// The deepest reduction any band applied since the last call, in dB.
    fn take_deepest(&mut self) -> f32 {
        std::mem::take(&mut self.deepest)
    }
    fn tail_samples(&self) -> usize {
        self.audio_split.tail_samples()
    }
}

#[derive(Default)]
pub struct MultibandCompressor {
    core: DynamicsCore,
    meter: GainReductionMeter,
}
impl MultibandCompressor {
    /// Reads the deepest reduction of any band. Take it before the effect
    /// goes to the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }
    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        for (i, (l, r)) in left.iter_mut().zip(right).enumerate() {
            let bands = self.core.tick(
                [*l, *r],
                key.and_then(|k| k.get(i)).copied().unwrap_or([0.0; 2]),
            );
            *l = audio(bands[0][0] + bands[1][0] + bands[2][0]);
            *r = audio(bands[0][1] + bands[1][1] + bands[2][1]);
        }
        self.meter.raise(self.core.take_deepest());
    }
}
impl Effect for MultibandCompressor {
    type Params = MultibandCompressorParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.core.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.core.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.core.set(params);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        self.run(left, right, key);
    }
    fn tail_samples(&self) -> usize {
        self.core.tail_samples()
    }
}

/// Independent compression and look-ahead limiting per band, followed by a
/// second look-ahead limiter on the sum. Both stages use a fixed 1 ms window.
pub struct MultibandMaximizer {
    core: DynamicsCore,
    input_gain: Controls<1>,
    band_limiters: [Limiter; 3],
    sum_limiter: Limiter,
    /// The limiters' own meters, read on the audio thread after each block.
    band_meters: [GainReductionMeter; 3],
    sum_meter: GainReductionMeter,
    meter: GainReductionMeter,
}
impl Default for MultibandMaximizer {
    fn default() -> Self {
        let band_limiters: [Limiter; 3] = std::array::from_fn(|_| Limiter::default());
        let sum_limiter = Limiter::default();
        let mut effect = Self {
            core: DynamicsCore::default(),
            input_gain: Controls::new([1.0]),
            band_meters: std::array::from_fn(|i| band_limiters[i].meter()),
            sum_meter: sum_limiter.meter(),
            band_limiters,
            sum_limiter,
            meter: GainReductionMeter::default(),
        };
        effect.set_params(&MultibandMaximizerParams::default());
        effect
    }
}
impl MultibandMaximizer {
    /// Reads how far the signal was turned down in all: the deepest band
    /// compression, the deepest band limiting and the limiting of the sum,
    /// added in dB. Take it before the effect goes to the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }
    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        for (i, (l, r)) in left.iter_mut().zip(right).enumerate() {
            let gain = self.input_gain.tick()[0];
            let input = [audio(*l) * gain, audio(*r) * gain];
            let mut bands = self.core.tick(
                input,
                key.and_then(|k| k.get(i)).copied().unwrap_or([0.0; 2]),
            );
            for (band, limiter) in bands.iter_mut().zip(&mut self.band_limiters) {
                let mut bl = [band[0]];
                let mut br = [band[1]];
                limiter.process(&mut bl, &mut br);
                *band = [bl[0], br[0]];
            }
            let mut sum_l = [audio(bands[0][0] + bands[1][0] + bands[2][0])];
            let mut sum_r = [audio(bands[0][1] + bands[1][1] + bands[2][1])];
            self.sum_limiter.process(&mut sum_l, &mut sum_r);
            *l = audio(sum_l[0]);
            *r = audio(sum_r[0]);
        }
        let mut band_limiting = 0.0_f32;
        for meter in &self.band_meters {
            band_limiting = band_limiting.max(meter.take_db());
        }
        self.meter
            .raise(self.core.take_deepest() + band_limiting + self.sum_meter.take_db());
    }
}
impl Effect for MultibandMaximizer {
    type Params = MultibandMaximizerParams;
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let sample_rate = rate(sample_rate).max(8.0);
        self.core.prepare(sample_rate);
        self.input_gain.prepare(sample_rate);
        for limiter in &mut self.band_limiters {
            limiter.prepare(sample_rate, max_block);
        }
        self.sum_limiter.prepare(sample_rate, max_block);
    }
    fn reset(&mut self) {
        self.core.reset();
        self.input_gain.reset();
        for limiter in &mut self.band_limiters {
            limiter.reset();
        }
        self.sum_limiter.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.core.set(&MultibandCompressorParams {
            low_crossover_hz: p.low_crossover_hz,
            high_crossover_hz: p.high_crossover_hz,
            low: p.low,
            mid: p.mid,
            high: p.high,
            sidechain: p.sidechain,
        });
        self.input_gain.set([db_to_gain_exp(p.input_gain_db)]);
        for (limiter, (ceiling, release)) in self.band_limiters.iter_mut().zip([
            (p.low_ceiling_db, p.low.release_ms),
            (p.mid_ceiling_db, p.mid.release_ms),
            (p.high_ceiling_db, p.high.release_ms),
        ]) {
            limiter.set_params(&LimiterParams {
                ceiling_db: ceiling,
                input_gain_db: 0.0,
                release_ms: release,
                lookahead_ms: 1.0,
                ..LimiterParams::default()
            });
        }
        self.sum_limiter.set_params(&LimiterParams {
            ceiling_db: p.ceiling_db,
            input_gain_db: 0.0,
            release_ms: p.limiter_release_ms,
            lookahead_ms: 1.0,
            ..LimiterParams::default()
        });
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        self.run(left, right, key);
    }
    fn latency_samples(&self) -> usize {
        self.band_limiters[0].latency_samples() + self.sum_limiter.latency_samples()
    }
    fn warm_up_samples(&self) -> usize {
        self.band_limiters[0].warm_up_samples() + self.sum_limiter.warm_up_samples()
    }
    fn tail_samples(&self) -> usize {
        self.core.tail_samples() + self.latency_samples()
    }
}

/// A constrained tonal/loudness recipe; amount is the entire public interface.
pub struct OneKnob {
    maximizer: MultibandMaximizer,
}
impl Default for OneKnob {
    fn default() -> Self {
        let mut effect = Self {
            maximizer: MultibandMaximizer::default(),
        };
        effect.set_params(&OneKnobParams::default());
        effect
    }
}
impl OneKnob {
    /// The meter of the maximizer inside. Take it before the effect goes to
    /// the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.maximizer.meter()
    }
    fn recipe(amount: f32) -> MultibandMaximizerParams {
        let band = |threshold: f32, ratio: f32, attack: f32, release: f32, makeup: f32| {
            BandDynamicsParams {
                threshold_db: threshold * amount,
                ratio: 1.0 + (ratio - 1.0) * amount,
                attack_ms: attack,
                release_ms: release,
                makeup_db: makeup * amount,
            }
        };
        MultibandMaximizerParams {
            low: band(-24.0, 3.0, 15.0, 180.0, 3.0),
            mid: band(-18.0, 2.5, 8.0, 100.0, 2.0),
            high: band(-22.0, 2.0, 2.0, 70.0, 4.0),
            input_gain_db: 8.0 * amount,
            ceiling_db: -0.3 * amount,
            ..MultibandMaximizerParams::default()
        }
    }
}
impl Effect for OneKnob {
    type Params = OneKnobParams;
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.maximizer.prepare(sample_rate, max_block);
    }
    fn reset(&mut self) {
        self.maximizer.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.maximizer
            .set_params(&Self::recipe(params.sanitized().amount));
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.maximizer.process(left, right);
    }
    fn latency_samples(&self) -> usize {
        self.maximizer.latency_samples()
    }
    fn warm_up_samples(&self) -> usize {
        self.maximizer.warm_up_samples()
    }
    fn tail_samples(&self) -> usize {
        self.maximizer.tail_samples()
    }
}
