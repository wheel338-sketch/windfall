//! Offline classical dereverberation. No weights, runtime or audio-thread work.
//! WPE uses delayed multichannel normal equations with desired-power weights;
//! the music mode uses a shared, floor-limited decaying late-power estimate.
//! Canonical two-second windows / one-second hops are independent of caller
//! block sizes. Only that window and one overlap are retained for long files.
use crate::{AnalysisError, Result, Work};
use rustfft::{
    FftPlanner,
    num_complex::{Complex32, Complex64},
};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DereverbMode {
    Vocal,
    General,
}
#[derive(Debug, Clone, Copy)]
pub struct DereverbSettings {
    pub mode: DereverbMode,
    pub amount: f32,
}

pub struct DereverbStream {
    rate: u32,
    channels: usize,
    settings: DereverbSettings,
    pending: Vec<f32>,
    overlap: Vec<f32>,
    finished: bool,
}
impl DereverbStream {
    pub fn new(rate: u32, channels: u16, settings: DereverbSettings) -> Result<Self> {
        if !(8_000..=192_000).contains(&rate)
            || !(1..=2).contains(&channels)
            || !settings.amount.is_finite()
            || !(0.0..=1.0).contains(&settings.amount)
        {
            return Err(AnalysisError::Invalid("dereverb rate/channels/amount"));
        }
        Ok(Self {
            rate,
            channels: channels as usize,
            settings,
            pending: Vec::with_capacity(rate as usize * channels as usize * 2),
            overlap: Vec::new(),
            finished: false,
        })
    }
    pub fn push(
        &mut self,
        samples: &[f32],
        work: &mut Work,
        emit: &mut impl FnMut(&[f32], &mut Work) -> Result<()>,
    ) -> Result<()> {
        work.check()?;
        if self.finished || !samples.len().is_multiple_of(self.channels) {
            return Err(AnalysisError::Invalid("dereverb block lifecycle/alignment"));
        }
        if self.settings.amount == 0.0 {
            for block in samples.chunks(4096) {
                work.checkpoint(block.len() as u64)?;
                if block.iter().any(|x| !x.is_finite()) {
                    return Err(AnalysisError::Invalid("dereverb nonfinite input"));
                }
                emit(block, work)?;
            }
            return Ok(());
        }
        let size = self.rate as usize * self.channels * 2;
        // A huge caller block is never copied wholesale.
        let mut cursor = 0;
        while cursor < samples.len() {
            let count = (size - self.pending.len())
                .min(samples.len() - cursor)
                .min(4096);
            work.checkpoint(count as u64)?;
            if samples[cursor..cursor + count]
                .iter()
                .any(|x| !x.is_finite())
            {
                return Err(AnalysisError::Invalid("dereverb nonfinite input"));
            }
            self.pending
                .extend_from_slice(&samples[cursor..cursor + count]);
            cursor += count;
            if self.pending.len() == size {
                self.segment(false, work, emit)?;
            }
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        work: &mut Work,
        emit: &mut impl FnMut(&[f32], &mut Work) -> Result<()>,
    ) -> Result<()> {
        work.check()?;
        if self.finished {
            return Err(AnalysisError::Invalid("dereverb already finished"));
        }
        self.finished = true;
        if !self.pending.is_empty() {
            self.segment(true, work, emit)?;
        }
        work.check()
    }
    /// Audio retained by streaming, excluding the bounded segment DSP scratch.
    pub fn retained_frames(&self) -> usize {
        (self.pending.len() + self.overlap.len()) / self.channels
    }
    fn segment(
        &mut self,
        last: bool,
        work: &mut Work,
        emit: &mut impl FnMut(&[f32], &mut Work) -> Result<()>,
    ) -> Result<()> {
        work.check()?;
        let step = self.rate as usize * self.channels;
        let mut output =
            render_segment(&self.pending, self.rate, self.channels, self.settings, work)?;
        let first = step.min(output.len());
        if !self.overlap.is_empty() {
            for (i, sample) in output[..first].iter_mut().enumerate() {
                let weight = (i / self.channels) as f32 / self.rate as f32;
                *sample = self.overlap[i] * (1.0 - weight) + *sample * weight;
            }
        }
        for block in output[..first].chunks(4096) {
            work.check()?;
            emit(block, work)?;
        }
        if last {
            for block in output[first..].chunks(4096) {
                work.check()?;
                emit(block, work)?;
            }
            self.pending.clear();
            self.overlap.clear();
        } else {
            self.overlap.clear();
            self.overlap.extend_from_slice(&output[step..]);
            self.pending.copy_within(step.., 0);
            self.pending.truncate(step);
        }
        Ok(())
    }
}

// Fit log-energy release runs, rejecting short/flat/noisy fits. RT60 is inferred
// from the energy slope, not from a user-supplied room response. Natural long
// musical releases are ambiguous; low-confidence material remains unchanged.
fn decay(energy: &[f64], seconds: f64) -> Option<f64> {
    let maximum = energy.iter().copied().fold(0.0, f64::max);
    if maximum < 1e-12 {
        return None;
    }
    let mut estimates = Vec::new();
    for start in 1..energy.len().saturating_sub(8) {
        if energy[start] < maximum * 0.01 || energy[start] < energy[start - 1] {
            continue;
        }
        let mut end = start + 1;
        while end < energy.len()
            && energy[end] <= energy[end - 1] * 1.6
            && energy[end] > maximum * 1e-7
        {
            end += 1;
        }
        if end - start < 8 || energy[end - 1] > energy[start] * 0.25 {
            continue;
        }
        let n = (end - start) as f64;
        let mean_x = (n - 1.0) * seconds / 2.0;
        let mean_y = energy[start..end]
            .iter()
            .map(|v| v.max(1e-20).ln())
            .sum::<f64>()
            / n;
        let mut xx = 0.0;
        let mut xy = 0.0;
        let mut yy = 0.0;
        for (i, e) in energy[start..end].iter().enumerate() {
            let x = i as f64 * seconds - mean_x;
            let y = e.max(1e-20).ln() - mean_y;
            xx += x * x;
            xy += x * y;
            yy += y * y;
        }
        let slope = xy / xx.max(1e-20);
        let fit = xy * xy / (xx * yy).max(1e-20);
        let rt = -13.815510557964274 / slope;
        if fit >= 0.8 && slope < 0.0 && (0.15..=3.5).contains(&rt) {
            estimates.push(rt.clamp(0.15, 2.5));
        }
    }
    estimates.sort_by(f64::total_cmp);
    estimates.get(estimates.len() / 2).copied()
}

fn render_segment(
    samples: &[f32],
    rate: u32,
    channels: usize,
    settings: DereverbSettings,
    work: &mut Work,
) -> Result<Vec<f32>> {
    let frames = samples.len() / channels;
    let cell = (rate as usize / 50).max(1);
    let energy: Vec<f64> = samples
        .chunks(cell * channels)
        .map(|b| b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / b.len() as f64)
        .collect();
    let Some(rt) = decay(&energy, cell as f64 / rate as f64) else {
        return Ok(samples.to_vec());
    };
    if settings.amount == 0.0 {
        return Ok(samples.to_vec());
    }
    let n = ((rate as f64 * 0.032).ceil() as usize)
        .next_power_of_two()
        .clamp(256, 8192);
    let hop = n / 4;
    let bins = n / 2 + 1;
    let count = (frames + n / 2).div_ceil(hop) + 1;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let window: Vec<f32> = (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * (i as f64 + 0.5) / n as f64).cos()).sqrt() as f32)
        .collect();
    let mut buffer = vec![Complex32::default(); n];
    let scratch_size = fft
        .get_inplace_scratch_len()
        .max(inverse.get_inplace_scratch_len());
    let mut scratch = vec![Complex32::default(); scratch_size];
    let mut spectrum = vec![Complex32::default(); count * channels * bins];
    for t in 0..count {
        work.checkpoint((n * channels) as u64)?;
        for c in 0..channels {
            for i in 0..n {
                let frame = (t * hop + i) as isize - (n / 2) as isize;
                buffer[i] = Complex32::new(
                    if frame >= 0 && (frame as usize) < frames {
                        samples[frame as usize * channels + c] * window[i]
                    } else {
                        0.0
                    },
                    0.0,
                );
            }
            fft.process_with_scratch(&mut buffer, &mut scratch);
            spectrum[(t * channels + c) * bins..(t * channels + c + 1) * bins]
                .copy_from_slice(&buffer[..bins]);
        }
    }
    let mut powers = vec![0.0; count];
    for t in 0..count {
        work.checkpoint((channels * bins) as u64)?;
        powers[t] = (0..channels)
            .flat_map(|c| spectrum[(t * channels + c) * bins..(t * channels + c + 1) * bins].iter())
            .map(|x| x.norm_sqr() as f64)
            .sum::<f64>();
    }
    // Shared broadband release confidence and transient hold protect attacks.
    let protect = (0.045 * rate as f64 / hop as f64).ceil() as usize;
    let mut strength = vec![0.0; count];
    let mut peak: f64 = 0.0;
    let mut hold = protect;
    for t in 0..count {
        if t == 0 || powers[t] > powers[t - 1] * 1.8 {
            hold = protect;
            peak = powers[t];
        }
        peak = peak.max(powers[t]);
        if hold > 0 {
            hold -= 1;
        } else {
            strength[t] = (1.0 - powers[t] / peak.max(1e-20)).clamp(0.0, 1.0);
        }
    }
    let delay = (0.035 * rate as f64 / hop as f64).ceil() as usize;
    let alpha = (-13.815510557964274 * hop as f64 / rate as f64 / rt).exp();
    for k in 0..bins {
        work.checkpoint(count as u64)?;
        let mut original = vec![Complex64::default(); count * channels];
        for t in 0..count {
            for c in 0..channels {
                let x = spectrum[(t * channels + c) * bins + k];
                original[t * channels + c] = Complex64::new(x.re as f64, x.im as f64);
            }
        }
        let mut desired = original.clone();
        if settings.mode == DereverbMode::Vocal {
            wpe(&original, &mut desired, count, channels, delay, work)?;
        }
        let mut late: f64 = 0.0;
        for t in 0..count {
            let power = original[t * channels..(t + 1) * channels]
                .iter()
                .map(|x| x.norm_sqr())
                .sum::<f64>()
                / channels as f64;
            if t >= delay {
                let past = original[(t - delay) * channels..(t - delay + 1) * channels]
                    .iter()
                    .map(|x| x.norm_sqr())
                    .sum::<f64>()
                    / channels as f64;
                late = (late * alpha).max(past * alpha.powi(delay as i32));
            }
            let floor: f64 = if settings.mode == DereverbMode::General {
                0.38
            } else {
                0.22
            };
            let removal = if settings.mode == DereverbMode::General {
                0.75
            } else {
                0.45
            };
            let gain = (1.0 - removal * strength[t] * late / power.max(1e-20))
                .max(floor * floor)
                .sqrt();
            let dry_power = original[t * channels..(t + 1) * channels]
                .iter()
                .map(|x| x.norm_sqr())
                .sum::<f64>();
            let wet_power = (0..channels)
                .map(|c| {
                    let dry = original[t * channels + c];
                    ((dry + (desired[t * channels + c] - dry) * strength[t]) * gain).norm_sqr()
                })
                .sum::<f64>();
            let safety_gain = if wet_power > dry_power && wet_power > 1e-20 {
                (dry_power / wet_power).sqrt()
            } else {
                1.0
            };
            // Identical spectral attenuation across channels; joint WPE uses
            // the same regression matrix / shared desired-power weights.
            for c in 0..channels {
                let dry = original[t * channels + c];
                let predicted = desired[t * channels + c] - dry;
                let correction = predicted * strength[t];
                // Never let an ill-conditioned prediction amplify a bin.
                let wet = (dry + correction) * gain * safety_gain;
                let result = dry + (wet - dry) * settings.amount as f64;
                spectrum[(t * channels + c) * bins + k] =
                    Complex32::new(result.re as f32, result.im as f32);
            }
        }
    }
    let mut output = vec![0.0f64; samples.len()];
    let mut norm = vec![0.0f64; frames];
    for t in 0..count {
        work.checkpoint((n * channels) as u64)?;
        for c in 0..channels {
            buffer[..bins].copy_from_slice(
                &spectrum[(t * channels + c) * bins..(t * channels + c + 1) * bins],
            );
            // DC/Nyquist must be real for Hermitian reconstruction.
            buffer[0].im = 0.0;
            buffer[n / 2].im = 0.0;
            for k in 1..n / 2 {
                buffer[n - k] = buffer[k].conj();
            }
            inverse.process_with_scratch(&mut buffer, &mut scratch);
            for i in 0..n {
                let frame = (t * hop + i) as isize - (n / 2) as isize;
                if frame >= 0 && (frame as usize) < frames {
                    let f = frame as usize;
                    output[f * channels + c] += buffer[i].re as f64 * window[i] as f64 / n as f64;
                    if c == 0 {
                        norm[f] += (window[i] as f64).powi(2);
                    }
                }
            }
        }
    }
    let mut since_attack: usize = 0;
    let protect_samples = (rate as f64 * 0.045) as usize;
    let release_samples = (rate as f64 * 0.01).max(1.0);
    for (f, &normalization) in norm.iter().enumerate() {
        if f.is_multiple_of(2048) {
            work.checkpoint((2048 * channels) as u64)?;
        }
        if f.is_multiple_of(cell) {
            let i = f / cell;
            if i == 0 || energy[i] > energy[i - 1] * 1.8 {
                since_attack = 0;
            }
        }
        for c in 0..channels {
            let i = f * channels + c;
            let blend =
                (since_attack.saturating_sub(protect_samples) as f64 / release_samples).min(1.0);
            output[i] = samples[i] as f64
                + (output[i] / normalization.max(1e-20) - samples[i] as f64) * blend;
        }
        since_attack = since_attack.saturating_add(1);
    }
    for block in output.chunks(4096) {
        work.checkpoint(block.len() as u64)?;
        if block
            .iter()
            .any(|x| !x.is_finite() || x.abs() > f32::MAX as f64)
        {
            return Err(AnalysisError::Invalid("dereverb nonfinite output"));
        }
    }
    Ok(output.into_iter().map(|x| x as f32).collect())
}

fn wpe(
    original: &[Complex64],
    desired: &mut [Complex64],
    frames: usize,
    channels: usize,
    delay: usize,
    work: &mut Work,
) -> Result<()> {
    const TAPS: usize = 8;
    const ITERATIONS: usize = 3;
    let dimensions = TAPS * channels;
    let start = delay + TAPS - 1;
    if frames <= start + dimensions {
        return Ok(());
    }
    let maximum = original.iter().map(|x| x.norm_sqr()).fold(0.0, f64::max);
    if maximum < 1e-20 {
        return Ok(());
    }
    let mut features = vec![Complex64::default(); dimensions];
    for _ in 0..ITERATIONS {
        work.checkpoint((frames * dimensions * dimensions) as u64)?;
        let mut matrix = vec![Complex64::default(); dimensions * dimensions];
        let mut rhs = vec![Complex64::default(); dimensions * channels];
        for t in start..frames {
            if t.is_multiple_of(32) {
                work.check()?;
            }
            let power = desired[t * channels..(t + 1) * channels]
                .iter()
                .map(|x| x.norm_sqr())
                .sum::<f64>()
                / channels as f64;
            let weight = 1.0 / power.max(maximum * 1e-4);
            for tap in 0..TAPS {
                for c in 0..channels {
                    features[tap * channels + c] = original[(t - delay - tap) * channels + c];
                }
            }
            for i in 0..dimensions {
                for j in 0..dimensions {
                    matrix[i * dimensions + j] += features[i] * features[j].conj() * weight;
                }
                for c in 0..channels {
                    rhs[i * channels + c] +=
                        features[i] * original[t * channels + c].conj() * weight;
                }
            }
        }
        let ridge = (0..dimensions)
            .map(|i| matrix[i * dimensions + i].re)
            .sum::<f64>()
            / dimensions as f64
            * 0.002
            + 1e-12;
        for i in 0..dimensions {
            matrix[i * dimensions + i].re += ridge;
        }
        // Hermitian positive definite system, bounded Cholesky dimensions <=16.
        let Some(filters) = solve(matrix, rhs, dimensions, channels) else {
            return Ok(());
        };
        for t in start..frames {
            for tap in 0..TAPS {
                for c in 0..channels {
                    features[tap * channels + c] = original[(t - delay - tap) * channels + c];
                }
            }
            for c in 0..channels {
                let prediction = (0..dimensions)
                    .map(|i| filters[i * channels + c].conj() * features[i])
                    .sum::<Complex64>();
                desired[t * channels + c] = original[t * channels + c] - prediction;
            }
        }
    }
    Ok(())
}
fn solve(
    mut matrix: Vec<Complex64>,
    mut rhs: Vec<Complex64>,
    n: usize,
    channels: usize,
) -> Option<Vec<Complex64>> {
    for i in 0..n {
        for j in 0..=i {
            let sum = (0..j)
                .map(|k| matrix[i * n + k] * matrix[j * n + k].conj())
                .sum::<Complex64>();
            let value = matrix[i * n + j] - sum;
            matrix[i * n + j] = if i == j {
                if value.re <= 0.0 || !value.re.is_finite() {
                    return None;
                }
                Complex64::new(value.re.sqrt(), 0.0)
            } else {
                value / matrix[j * n + j]
            };
        }
    }
    for c in 0..channels {
        for i in 0..n {
            let sum = (0..i)
                .map(|j| matrix[i * n + j] * rhs[j * channels + c])
                .sum::<Complex64>();
            rhs[i * channels + c] = (rhs[i * channels + c] - sum) / matrix[i * n + i];
        }
        for i in (0..n).rev() {
            let sum = (i + 1..n)
                .map(|j| matrix[j * n + i].conj() * rhs[j * channels + c])
                .sum::<Complex64>();
            rhs[i * channels + c] = (rhs[i * channels + c] - sum) / matrix[i * n + i].conj();
        }
    }
    Some(rhs)
}
