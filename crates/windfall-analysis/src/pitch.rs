//! Prepared, bounded classical monophonic estimates. Worker/control side only.
//! No file IO, model, project mutation, registration or realtime promise.
use crate::{AnalysisError, AudioShape, FrameRange, Work};
use rustfft::{Fft, FftDirection, algorithm::Radix4, num_complex::Complex};
use std::mem::size_of;
use thiserror::Error;

pub const ALGORITHM_VERSION: &str = "windfall-fixed-window-yin-fft-v2";
const VALIDATION_CHUNK: usize = 4096;

pub type PitchResult<T> = std::result::Result<T, PitchError>;

#[derive(Debug, Error)]
pub enum PitchError {
    #[error("{}", windfall_core::notice!("analysis.pitch.invalid", detail = windfall_core::notice::term(_0.to_string())))]
    Invalid(&'static str),
    #[error("{}", windfall_core::notice!("analysis.pitch.overflow"))]
    Overflow,
    #[error("{}", windfall_core::notice!("analysis.pitch.limit", detail = windfall_core::notice::term(_0.to_string())))]
    Limit(&'static str),
    #[error("{}", windfall_core::notice!("analysis.pitch.allocation"))]
    Allocation,
    #[error("{}", windfall_core::notice!("analysis.pitch.nonfinitePcm", sample = sample))]
    NonfinitePcm { sample: usize },
    #[error("{}", windfall_core::notice!("analysis.pitch.tooShort", required = required, actual = actual))]
    TooShort { required: u64, actual: u64 },
    #[error("{}", windfall_core::notice!("analysis.pitch.numerical"))]
    Numerical,
    #[error(transparent)]
    Work(#[from] AnalysisError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CancelToken;
    use std::time::Duration;

    #[test]
    fn fft_difference_agrees_with_independent_direct_squared_differences() {
        let mut work = Work::new(
            CancelToken::default(),
            1_000_000_000_000,
            Duration::from_secs(60),
        )
        .unwrap();
        let mut analyzer =
            PitchAnalyzer::prepare(PitchConfig::default(), PitchLimits::default(), &mut work)
                .unwrap();
        let t = analyzer.resources.support_frames;
        // Deliberately nonstationary, nonzero DC, unequal endpoint values: an
        // autocorrelation with shrinking overlap or circular wrap cannot pass.
        let pcm: Vec<f32> = (0..t)
            .map(|j| {
                let q = j as f64;
                (0.7 + 0.0002 * q + (q * 0.173).sin() * 0.4 + (q * q * 0.00003).cos() * 0.3) as f32
            })
            .collect();
        let source = PitchSource {
            pcm: &pcm,
            shape: AudioShape {
                frames: t as u64,
                channels: 1,
                sample_rate: 48_000,
            },
            range: FrameRange {
                start: 0,
                end: t as u64,
            },
            frame_origin: 0,
        };
        analyzer.fill_window(source, 0, &work).unwrap();
        analyzer.differences(&work).unwrap();
        for lag in 1..=analyzer.resources.max_lag + 1 {
            // No FFT/prefix/CMND algebra in this oracle. A constant mean cancels.
            let direct = (0..analyzer.resources.comparison_frames)
                .map(|j| {
                    let d = f64::from(pcm[j]) - f64::from(pcm[j + lag]);
                    d * d
                })
                .sum::<f64>();
            assert!(
                (direct - analyzer.difference[lag]).abs() <= 1e-9 * direct.max(1.0),
                "lag {lag}: {direct} vs {}",
                analyzer.difference[lag]
            );
        }
    }
}

/// No downmix. Strongest is per-window centered energy, lowest-index tie break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPolicy {
    Channel(u16),
    Strongest,
}

#[derive(Debug, Clone, Copy)]
pub struct PitchConfig {
    pub sample_rate: u32,
    pub min_hz: f64,
    pub max_hz: f64,
    pub hop_frames: u32,
    pub channel_policy: ChannelPolicy,
    /// First interpolated normalized-difference trough strictly below this threshold.
    pub yin_threshold: f64,
    pub rms_floor: f64,
    /// Adjacent voiced cells farther apart than this start a new region.
    pub segment_jump_cents: f64,
}

impl Default for PitchConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            min_hz: 50.0,
            max_hz: 1500.0,
            hop_frames: 480,
            channel_policy: ChannelPolicy::Strongest,
            yin_threshold: 0.15,
            rms_floor: 0.003,
            segment_jump_cents: 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PitchLimits {
    /// Requested Rust allocation bound, excluding allocator metadata/OS RSS.
    pub analyzer_bytes: u64,
    pub output_bytes: u64,
    /// Entire borrowed source, checked but never copied.
    pub source_bytes: u64,
    pub selected_frames: u64,
    pub estimates: usize,
    pub segments: usize,
}

impl Default for PitchLimits {
    fn default() -> Self {
        Self {
            analyzer_bytes: 16 * 1024 * 1024,
            output_bytes: 64 * 1024 * 1024,
            source_bytes: 1024 * 1024 * 1024,
            selected_frames: 48_000 * 60 * 30,
            estimates: 200_000,
            segments: 200_000,
        }
    }
}

/// Immutable raw interleaved PCM. `range` is relative to this borrowed source;
/// result coordinates are `frame_origin + range`, with checked integer addition.
#[derive(Debug, Clone, Copy)]
pub struct PitchSource<'a> {
    pub pcm: &'a [f32],
    pub shape: AudioShape,
    pub range: FrameRange,
    pub frame_origin: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEstimate {
    pub range: FrameRange,
    pub center_frame: u64,
    pub support: FrameRange,
    pub channel: u16,
    pub f0_hz: Option<f64>,
    /// One minus interpolated CMND trough depth, not a probability of correct F0.
    /// Zero on rejected cells.
    pub confidence: f64,
    pub rms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchSegment {
    pub range: FrameRange,
    pub channel: u16,
    pub f0_hz: Option<f64>,
    pub confidence: f64,
    pub estimate_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchResources {
    pub comparison_frames: usize,
    pub support_frames: usize,
    pub max_lag: usize,
    pub fft_len: usize,
    pub scratch_bytes: u64,
    /// Conservative peak includes both plan construction transients.
    pub analyzer_peak_bytes: u64,
    pub preparation_work: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchRequirements {
    pub estimates: usize,
    pub segment_capacity: usize,
    pub output_peak_bytes: u64,
    /// Validation + worst-case hop/segmentation units, excluding preparation.
    pub work_units: u64,
}

/// Constructed only on complete success. No user override or persistence schema.
#[derive(Debug)]
pub struct PitchAnalysis {
    range: FrameRange,
    estimates: Vec<PitchEstimate>,
    segments: Vec<PitchSegment>,
    requirements: PitchRequirements,
}

impl PitchAnalysis {
    pub fn range(&self) -> FrameRange {
        self.range
    }
    pub fn estimates(&self) -> &[PitchEstimate] {
        &self.estimates
    }
    pub fn segments(&self) -> &[PitchSegment] {
        &self.segments
    }
    pub fn requirements(&self) -> PitchRequirements {
        self.requirements
    }
}

/// Exclusive reusable scratch; prepare, use and drop on a worker, outside guards.
pub struct PitchAnalyzer {
    config: PitchConfig,
    limits: PitchLimits,
    resources: PitchResources,
    forward: Radix4<f64>,
    inverse: Radix4<f64>,
    a: Vec<Complex<f64>>,
    b: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    energy: Vec<f64>,
    difference: Vec<f64>,
}

fn add(a: u64, b: u64) -> PitchResult<u64> {
    a.checked_add(b).ok_or(PitchError::Overflow)
}
fn mul(a: u64, b: u64) -> PitchResult<u64> {
    a.checked_mul(b).ok_or(PitchError::Overflow)
}
fn usize_of(n: u64) -> PitchResult<usize> {
    usize::try_from(n).map_err(|_| PitchError::Overflow)
}
fn reserve<T>(n: usize) -> PitchResult<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(n)
        .map_err(|_| PitchError::Allocation)?;
    Ok(values)
}
fn zeros<T: Clone + Default>(n: usize) -> PitchResult<Vec<T>> {
    let mut values = reserve(n)?;
    values.resize(n, T::default());
    Ok(values)
}

impl PitchAnalyzer {
    pub fn prepare(config: PitchConfig, limits: PitchLimits, work: &mut Work) -> PitchResult<Self> {
        work.check()?;
        if !(8000..=192_000).contains(&config.sample_rate)
            || !config.min_hz.is_finite()
            || !config.max_hz.is_finite()
            || config.min_hz < 40.0
            || config.min_hz >= config.max_hz
            || config.max_hz > 2000.0
            || config.max_hz > f64::from(config.sample_rate) / 8.0
            || config.hop_frames < config.sample_rate.div_ceil(1000)
            || config.hop_frames > config.sample_rate / 10
            || !config.yin_threshold.is_finite()
            || !(0.01..=0.3).contains(&config.yin_threshold)
            || !config.rms_floor.is_finite()
            || !(0.000001..=1.0).contains(&config.rms_floor)
            || !config.segment_jump_cents.is_finite()
            || !(25.0..=1200.0).contains(&config.segment_jump_cents)
            || matches!(config.channel_policy, ChannelPolicy::Channel(c) if c >= 8)
        {
            return Err(PitchError::Invalid(
                "frequency/rate/hop/threshold/channel bounds",
            ));
        }
        if limits.analyzer_bytes == 0
            || limits.analyzer_bytes > 256 * 1024 * 1024
            || limits.output_bytes == 0
            || limits.output_bytes > 512 * 1024 * 1024
            || limits.source_bytes == 0
            || limits.source_bytes > 8 * 1024 * 1024 * 1024
            || limits.selected_frames == 0
            || limits.selected_frames > u64::from(config.sample_rate) * 86400
            || limits.estimates == 0
            || limits.estimates > 2_000_000
            || limits.segments == 0
            || limits.segments > 2_000_000
        {
            return Err(PitchError::Invalid("resource bounds"));
        }
        // Float-to-integer conversions are bounded above by checked rate/F0.
        let max_lag = (f64::from(config.sample_rate) / config.min_hz).ceil() as usize;
        let comparison = max_lag.checked_mul(2).ok_or(PitchError::Overflow)?;
        let support = comparison
            .checked_add(max_lag + 1)
            .ok_or(PitchError::Overflow)?;
        let fft_len = support
            .checked_mul(2)
            .and_then(usize::checked_next_power_of_two)
            .ok_or(PitchError::Overflow)?;
        let n = fft_len as u64;
        let scratch_bytes = add(mul(48, n)?, mul(8, (support + max_lag + 3) as u64)?)?;
        // Radix4 6.4.1: each twiddle Vec capacity 2N, boxed length <N.
        // Bound both constructors (including shrink coexistence), base Arcs and
        // structs by 4N complex values + 4096; deliberately conservative.
        let analyzer_peak_bytes = add(scratch_bytes, add(mul(64, n)?, 4096)?)?;
        if analyzer_peak_bytes > limits.analyzer_bytes {
            return Err(PitchError::Limit("analyzer bytes"));
        }
        let preparation_work = add(mul(n, 16 * u64::from(fft_len.ilog2()) + 16)?, scratch_bytes)?;
        work.checkpoint(preparation_work)?;
        let forward = Radix4::new(fft_len, FftDirection::Forward);
        work.check()?;
        let inverse = Radix4::new(fft_len, FftDirection::Inverse);
        work.check()?;
        // The explicitly selected exact-version power-of-two plan needs N.
        if forward.get_inplace_scratch_len() > fft_len
            || inverse.get_inplace_scratch_len() > fft_len
        {
            return Err(PitchError::Limit("unexpected FFT scratch"));
        }
        let analyzer = Self {
            config,
            limits,
            forward,
            inverse,
            resources: PitchResources {
                comparison_frames: comparison,
                support_frames: support,
                max_lag,
                fft_len,
                scratch_bytes,
                analyzer_peak_bytes,
                preparation_work,
            },
            a: zeros(fft_len)?,
            b: zeros(fft_len)?,
            scratch: zeros(fft_len)?,
            energy: zeros(support + 1)?,
            difference: zeros(max_lag + 2)?,
        };
        work.check()?;
        Ok(analyzer)
    }

    pub fn config(&self) -> PitchConfig {
        self.config
    }
    pub fn resources(&self) -> PitchResources {
        self.resources
    }

    fn frame_work(&self, channels: u16) -> PitchResult<u64> {
        let r = self.resources;
        let fft = mul(r.fft_len as u64, 96 * u64::from(r.fft_len.ilog2()) + 32)?;
        let scan = mul(r.support_frames as u64, 16 * (u64::from(channels) + 1))?;
        add(add(fft, scan)?, 32 * (r.max_lag as u64 + 2) + 128)
    }

    /// Checks metadata/capacity only; `analyze` still validates every PCM sample.
    /// This pure preflight does not allocate or consume Work.
    pub fn requirements(&self, source: PitchSource<'_>) -> PitchResult<PitchRequirements> {
        let s = source.shape;
        if s.sample_rate != self.config.sample_rate || !(1..=8).contains(&s.channels) {
            return Err(PitchError::Invalid("source rate/channels"));
        }
        if matches!(self.config.channel_policy, ChannelPolicy::Channel(c) if c >= s.channels) {
            return Err(PitchError::Invalid("selected channel absent"));
        }
        let samples = mul(s.frames, u64::from(s.channels))?;
        let bytes = mul(samples, 4)?;
        if bytes > self.limits.source_bytes {
            return Err(PitchError::Limit("source bytes"));
        }
        if usize_of(samples)? != source.pcm.len() {
            return Err(PitchError::Invalid("PCM shape"));
        }
        add(source.frame_origin, s.frames)?;
        let selected = source
            .range
            .end
            .checked_sub(source.range.start)
            .ok_or(PitchError::Invalid("reversed range"))?;
        if source.range.end > s.frames {
            return Err(PitchError::Invalid("range outside source"));
        }
        if selected > self.limits.selected_frames {
            return Err(PitchError::Limit("selected frames"));
        }
        if selected != 0 && selected < self.resources.support_frames as u64 {
            return Err(PitchError::TooShort {
                required: self.resources.support_frames as u64,
                actual: selected,
            });
        }
        let count = usize_of(selected.div_ceil(u64::from(self.config.hop_frames)))?;
        if count > self.limits.estimates {
            return Err(PitchError::Limit("estimate count"));
        }
        let segment_capacity = count.min(self.limits.segments);
        let output_peak_bytes = add(
            mul(count as u64, size_of::<PitchEstimate>() as u64)?,
            mul(segment_capacity as u64, size_of::<PitchSegment>() as u64)?,
        )?;
        if output_peak_bytes > self.limits.output_bytes {
            return Err(PitchError::Limit("output bytes"));
        }
        Ok(PitchRequirements {
            estimates: count,
            segment_capacity,
            output_peak_bytes,
            work_units: add(samples, mul(count as u64, self.frame_work(s.channels)?)?)?,
        })
    }

    pub fn analyze(
        &mut self,
        source: PitchSource<'_>,
        work: &mut Work,
    ) -> PitchResult<PitchAnalysis> {
        work.check()?;
        let requirements = self.requirements(source)?;
        // Full borrowed source, all channels, before normalization or output.
        for (chunk_index, chunk) in source.pcm.chunks(VALIDATION_CHUNK).enumerate() {
            work.checkpoint(chunk.len() as u64)?;
            for (offset, sample) in chunk.iter().enumerate() {
                if !sample.is_finite() {
                    return Err(PitchError::NonfinitePcm {
                        sample: chunk_index * VALIDATION_CHUNK + offset,
                    });
                }
            }
        }
        let absolute_range = FrameRange {
            start: add(source.frame_origin, source.range.start)?,
            end: add(source.frame_origin, source.range.end)?,
        };
        let mut estimates = reserve(requirements.estimates)?;
        let mut segments: Vec<PitchSegment> = reserve(requirements.segment_capacity)?;
        let mut region_log_sum = 0.0;
        let mut region_confidence_sum = 0.0;
        let mut start = source.range.start;
        let mut previous: Option<PitchEstimate> = None;
        while start < source.range.end {
            work.checkpoint(self.frame_work(source.shape.channels)?)?;
            let end = add(start, u64::from(self.config.hop_frames))?.min(source.range.end);
            let center = start + (end - start) / 2;
            // Clamp the WINDOW placement at selection edges, not source time.
            let half = (self.resources.support_frames / 2) as u64;
            let support_start = center
                .checked_sub(half)
                .unwrap_or(source.range.start)
                .max(source.range.start)
                .min(source.range.end - self.resources.support_frames as u64);
            let (channel, rms) = self.fill_window(source, usize_of(support_start)?, work)?;
            let (f0_hz, confidence) = if rms < self.config.rms_floor {
                (None, 0.0)
            } else {
                self.detect(work)?
            };
            let estimate = PitchEstimate {
                range: FrameRange {
                    start: add(source.frame_origin, start)?,
                    end: add(source.frame_origin, end)?,
                },
                center_frame: add(source.frame_origin, center)?,
                support: FrameRange {
                    start: add(source.frame_origin, support_start)?,
                    end: add(
                        source.frame_origin,
                        support_start + self.resources.support_frames as u64,
                    )?,
                },
                channel,
                f0_hz,
                confidence,
                rms,
            };
            let split = previous.is_none_or(|p| {
                p.channel != channel
                    || p.f0_hz.is_some() != f0_hz.is_some()
                    || p.f0_hz.zip(f0_hz).is_some_and(|(a, b)| {
                        (1200.0 * (b / a).log2()).abs() > self.config.segment_jump_cents
                    })
            });
            if split {
                if segments.len() == self.limits.segments {
                    return Err(PitchError::Limit("segment count"));
                }
                region_log_sum = f0_hz.map_or(0.0, f64::ln);
                region_confidence_sum = confidence;
                segments.push(PitchSegment {
                    range: estimate.range,
                    channel,
                    f0_hz,
                    confidence,
                    estimate_count: 1,
                });
            } else {
                region_log_sum += f0_hz.map_or(0.0, f64::ln);
                region_confidence_sum += confidence;
                let segment = segments.last_mut().ok_or(PitchError::Numerical)?;
                segment.range.end = estimate.range.end;
                segment.estimate_count += 1;
                segment.f0_hz =
                    f0_hz.map(|_| (region_log_sum / segment.estimate_count as f64).exp());
                segment.confidence = region_confidence_sum / segment.estimate_count as f64;
            }
            estimates.push(estimate);
            previous = Some(estimate);
            start = end;
        }
        work.check()?;
        Ok(PitchAnalysis {
            range: absolute_range,
            estimates,
            segments,
            requirements,
        })
    }

    fn fill_window(
        &mut self,
        source: PitchSource<'_>,
        start: usize,
        work: &Work,
    ) -> PitchResult<(u16, f64)> {
        let channels = usize::from(source.shape.channels);
        let t = self.resources.support_frames;
        let (first, last) = match self.config.channel_policy {
            ChannelPolicy::Channel(c) => (c, c + 1),
            ChannelPolicy::Strongest => (0, source.shape.channels),
        };
        let mut chosen = first;
        let mut best_energy = -1.0;
        let mut chosen_mean = 0.0;
        for channel in first..last {
            work.check()?;
            // Two passes avoid cancellation of DC variance in sum(x*x)-sum(x)^2.
            let mean = (0..t)
                .map(|j| f64::from(source.pcm[(start + j) * channels + usize::from(channel)]))
                .sum::<f64>()
                / t as f64;
            let energy = (0..t)
                .map(|j| {
                    let x =
                        f64::from(source.pcm[(start + j) * channels + usize::from(channel)]) - mean;
                    x * x
                })
                .sum::<f64>();
            if energy > best_energy {
                chosen = channel;
                chosen_mean = mean;
                best_energy = energy;
            }
        }
        self.a.fill(Complex::default());
        self.b.fill(Complex::default());
        self.energy[0] = 0.0;
        for j in 0..t {
            let x =
                f64::from(source.pcm[(start + j) * channels + usize::from(chosen)]) - chosen_mean;
            self.b[j].re = x;
            if j < self.resources.comparison_frames {
                self.a[j].re = x;
            }
            self.energy[j + 1] = self.energy[j] + x * x;
        }
        work.check()?;
        Ok((chosen, (best_energy / t as f64).sqrt()))
    }

    fn differences(&mut self, work: &Work) -> PitchResult<()> {
        work.check()?;
        self.forward
            .process_with_scratch(&mut self.a, &mut self.scratch);
        work.check()?;
        self.forward
            .process_with_scratch(&mut self.b, &mut self.scratch);
        work.check()?;
        for (a, b) in self.a.iter_mut().zip(&self.b) {
            *a = a.conj() * b;
        }
        self.inverse
            .process_with_scratch(&mut self.a, &mut self.scratch);
        work.check()?;
        self.difference[0] = 0.0;
        for lag in 1..self.difference.len() {
            self.difference[lag] = self.raw_difference(lag)?;
        }
        Ok(())
    }

    // Use identical checked raw math for CMND and final period interpolation.
    fn raw_difference(&self, lag: usize) -> PitchResult<f64> {
        let w = self.resources.comparison_frames;
        let energy = self.energy[w] + self.energy[w + lag] - self.energy[lag];
        let d = energy - 2.0 * self.a[lag].re / self.resources.fft_len as f64;
        // Only roundoff in this derived sum may be clamped; PCM is untouched.
        if !d.is_finite()
            || d < -1e-10 * self.energy[self.resources.support_frames].max(f64::MIN_POSITIVE)
        {
            return Err(PitchError::Numerical);
        }
        Ok(d.max(0.0))
    }

    fn detect(&mut self, work: &Work) -> PitchResult<(Option<f64>, f64)> {
        self.differences(work)?;
        let mut sum = 0.0;
        self.difference[0] = 1.0;
        for lag in 1..self.difference.len() {
            let d = self.difference[lag];
            sum += d;
            self.difference[lag] = if sum > 0.0 { d * lag as f64 / sum } else { 1.0 };
        }
        // Inspect shorter lags too: skipping an above-range fundamental can
        // otherwise accept its in-range subharmonic. The first trough's
        // interpolated Hz itself must be inside the requested bounds.
        if let Some((lag, depth)) = windfall_core::yin::first_trough(
            &self.difference,
            self.resources.max_lag,
            self.config.yin_threshold,
        ) {
            let offset = parabolic_minimum(
                self.raw_difference(lag - 1)?,
                self.raw_difference(lag)?,
                self.raw_difference(lag + 1)?,
            )
            .0;
            let hz = f64::from(self.config.sample_rate) / (lag as f64 + offset);
            if (self.config.min_hz..=self.config.max_hz).contains(&hz) {
                return Ok((Some(hz), (1.0 - depth).clamp(0.0, 1.0)));
            }
        }
        Ok((None, 0.0))
    }
}

use windfall_core::yin::parabolic_minimum;
