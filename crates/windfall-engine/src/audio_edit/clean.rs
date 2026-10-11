//! Cleanup DSP for the audio editor. Worker-only: every function allocates.
//!
//! Each function edits interleaved frames `[start, end)` in place and never
//! writes outside that range. Frames outside it may be read as context.
use rustfft::algorithm::Radix4;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftDirection};

/// Subtracts each channel's own mean over the selection. Offsets differ per
/// converter channel, so the channels are deliberately not linked.
pub fn remove_dc(data: &mut [f32], channels: usize, start: usize, end: usize) {
    for channel in 0..channels {
        let selected = || (start..end).map(|frame| frame * channels + channel);
        let mean = selected().map(|i| f64::from(data[i])).sum::<f64>() / (end - start) as f64;
        for i in selected() {
            data[i] = (f64::from(data[i]) - mean) as f32;
        }
    }
}

/// First frame and exclusive last frame of the selection holding a sample
/// above the threshold in any channel. None when all of it is below.
pub fn silence_bounds(
    data: &[f32],
    channels: usize,
    start: usize,
    end: usize,
    threshold_db: f32,
) -> Result<Option<(usize, usize)>, String> {
    if !threshold_db.is_finite() || !(-120.0..=-6.0).contains(&threshold_db) {
        return Err("The silence threshold must be between -120 and -6 dBFS.".into());
    }
    let threshold = 10.0_f64.powf(f64::from(threshold_db) / 20.0) as f32;
    let loud = |frame: usize| {
        data[frame * channels..(frame + 1) * channels]
            .iter()
            .any(|s| s.abs() > threshold)
    };
    let Some(first) = (start..end).find(|frame| loud(*frame)) else {
        return Ok(None);
    };
    let last = (first..end)
        .rev()
        .find(|frame| loud(*frame))
        .unwrap_or(first);
    Ok(Some((first, last + 1)))
}

// ---------------------------------------------------------------------------
// De-click: autoregressive outlier detection and least-squares interpolation.

const ORDER: usize = 32;
/// Samples sharing one autoregressive model.
const BLOCK: usize = 2048;
/// Prediction errors below -80 dBFS are never treated as clicks.
const CLICK_FLOOR: f64 = 1e-4;
/// Unknown samples solved together. Bounds the dense solve.
const MAX_CLUSTER: usize = 128;

type Model = [f64; ORDER];

/// Repairs short impulsive outliers of each channel independently and returns
/// how many samples were replaced. Samples that are not detected keep their
/// exact value, so clean audio is returned unchanged.
pub fn declick(
    data: &mut [f32],
    channels: usize,
    rate: u32,
    start: usize,
    end: usize,
    strength: f32,
) -> Result<usize, String> {
    if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
        return Err("De-click strength must be between 0 and 100 %.".into());
    }
    // Robust standard deviations a prediction error must exceed.
    let sigmas = 14.0 - 10.0 * f64::from(strength);
    // Longer runs are musical transients rather than clicks.
    let longest = (rate as usize / 500).clamp(16, MAX_CLUSTER);
    let frames = data.len() / channels;
    let low = start.saturating_sub(BLOCK);
    let high = (end + BLOCK).min(frames);
    let selection = start - low..end - low;
    let mut repaired = 0;
    for channel in 0..channels {
        let original: Vec<f32> = (low..high).map(|f| data[f * channels + channel]).collect();
        let (flags, models) = detect(&original, &original, &selection, sigmas, longest);
        if !flags.contains(&true) {
            continue;
        }
        // Clicks bias the first model. Fit again on the repaired signal, then
        // detect and interpolate the original with that cleaner model.
        let first = repair(&original, &flags, &models, selection.start);
        let (flags, models) = detect(&first, &original, &selection, sigmas, longest);
        let output = repair(&original, &flags, &models, selection.start);
        for (offset, flagged) in flags.iter().enumerate() {
            if *flagged {
                let sample = output[selection.start + offset];
                if !sample.is_finite() {
                    return Err("De-click produced non-finite audio.".into());
                }
                data[(start + offset) * channels + channel] = sample;
                repaired += 1;
            }
        }
    }
    Ok(repaired)
}

/// Autocorrelation-method linear prediction of a Hann-windowed frame.
fn model(frame: &[f32]) -> Model {
    let mut coefficients = [0.0; ORDER];
    let n = frame.len();
    if n <= ORDER * 4 {
        return coefficients;
    }
    let windowed: Vec<f64> = frame
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let phase = std::f64::consts::TAU * (i as f64 + 0.5) / n as f64;
            f64::from(*s) * (0.5 - 0.5 * phase.cos())
        })
        .collect();
    let mut r = [0.0; ORDER + 1];
    for (lag, value) in r.iter_mut().enumerate() {
        *value = windowed[..n - lag]
            .iter()
            .zip(&windowed[lag..])
            .map(|(a, b)| a * b)
            .sum();
    }
    if r[0] <= 1e-20 {
        return coefficients;
    }
    // -90 dB of white noise keeps pure tones well conditioned.
    r[0] *= 1.0 + 1e-9;
    let mut error = r[0];
    let mut previous = [0.0; ORDER];
    for i in 0..ORDER {
        let mut acc = r[i + 1];
        for j in 0..i {
            acc -= coefficients[j] * r[i - j];
        }
        let reflection = acc / error;
        if !reflection.is_finite() || reflection.abs() >= 1.0 {
            break;
        }
        previous[..i].copy_from_slice(&coefficients[..i]);
        coefficients[i] = reflection;
        for j in 0..i {
            coefficients[j] = previous[j] - reflection * previous[i - 1 - j];
        }
        error *= 1.0 - reflection * reflection;
        if error <= r[0] * 1e-12 {
            break;
        }
    }
    coefficients
}

/// Flags selection samples whose forward and backward prediction errors both
/// exceed the block's robust threshold. A click's forward error rings after
/// it and its backward error before it; only the click itself has both.
fn detect(
    estimate: &[f32],
    signal: &[f32],
    selection: &std::ops::Range<usize>,
    sigmas: f64,
    longest: usize,
) -> (Vec<bool>, Vec<Model>) {
    let n = signal.len();
    let mut flags = vec![false; selection.len()];
    let mut models = Vec::with_capacity(selection.len() / BLOCK + 1);
    let mut errors = Vec::with_capacity(BLOCK);
    let mut scratch = Vec::with_capacity(BLOCK);
    let mut first = selection.start;
    while first < selection.end {
        let last = (first + BLOCK).min(selection.end);
        let a = model(&estimate[first.saturating_sub(BLOCK / 2)..(last + BLOCK / 2).min(n)]);
        errors.clear();
        scratch.clear();
        for i in first..last {
            let (mut forward, mut backward) = (f64::from(signal[i]), f64::from(signal[i]));
            for (k, a) in a.iter().enumerate() {
                if i > k {
                    forward -= a * f64::from(signal[i - k - 1]);
                }
                if i + k + 1 < n {
                    backward -= a * f64::from(signal[i + k + 1]);
                }
            }
            scratch.push(forward.abs());
            errors.push(forward.abs().min(backward.abs()));
        }
        let middle = scratch.len() / 2;
        let (_, median, _) = scratch.select_nth_unstable_by(middle, f64::total_cmp);
        let threshold = (sigmas * 1.4826 * *median).max(CLICK_FLOOR);
        for (offset, error) in errors.iter().enumerate() {
            flags[first - selection.start + offset] = *error > threshold;
        }
        models.push(a);
        first = last;
    }
    // Clicks have skirts under the threshold: widen every detection by one.
    let detected = flags.clone();
    for (i, flagged) in detected.iter().enumerate() {
        if *flagged {
            flags[i.saturating_sub(1)] = true;
            if i + 1 < flags.len() {
                flags[i + 1] = true;
            }
        }
    }
    let mut i = 0;
    while i < flags.len() {
        if !flags[i] {
            i += 1;
            continue;
        }
        let run = flags[i..].iter().take_while(|f| **f).count();
        if run > longest {
            flags[i..i + run].fill(false);
        }
        i += run;
    }
    (flags, models)
}

/// Least-squares autoregressive interpolation of the flagged samples: the
/// values minimizing the model's prediction error across each gap.
fn repair(signal: &[f32], flags: &[bool], models: &[Model], offset: usize) -> Vec<f32> {
    let mut output = signal.to_vec();
    let n = signal.len();
    let mut unknown: Vec<usize> = Vec::with_capacity(MAX_CLUSTER);
    let mut matrix = vec![0.0_f64; MAX_CLUSTER * MAX_CLUSTER];
    let mut rhs = [0.0_f64; MAX_CLUSTER];
    let mut i = 0;
    while i < flags.len() {
        if !flags[i] {
            i += 1;
            continue;
        }
        // Join detections closer than the model order, so a neighbouring
        // click is solved rather than trusted as context.
        unknown.clear();
        let mut gap = 0;
        while i < flags.len() && gap < ORDER && unknown.len() < MAX_CLUSTER {
            if flags[i] {
                unknown.push(offset + i);
                gap = 0;
            } else {
                gap += 1;
            }
            i += 1;
        }
        let a = &models[(unknown[0] - offset) / BLOCK];
        // Autocorrelation of the prediction error filter [1, -a1, ..., -ap].
        let filter = |k: usize| if k == 0 { 1.0 } else { -a[k - 1] };
        let mut b = [0.0; ORDER + 1];
        for (lag, value) in b.iter_mut().enumerate() {
            *value = (0..=ORDER - lag).map(|k| filter(k) * filter(k + lag)).sum();
        }
        let m = unknown.len();
        for (row, &u) in unknown.iter().enumerate() {
            let mut known = 0.0;
            for at in u.saturating_sub(ORDER)..=(u + ORDER).min(n - 1) {
                if unknown.binary_search(&at).is_err() {
                    known -= b[u.abs_diff(at)] * f64::from(output[at]);
                }
            }
            rhs[row] = known;
            for (column, &v) in unknown.iter().enumerate() {
                let lag = u.abs_diff(v);
                matrix[row * m + column] = if lag <= ORDER { b[lag] } else { 0.0 };
            }
        }
        if solve(&mut matrix[..m * m], &mut rhs[..m], m) {
            for (row, &u) in unknown.iter().enumerate() {
                output[u] = rhs[row] as f32;
            }
        } else {
            straight_lines(&mut output, &unknown);
        }
    }
    output
}

/// In-place Cholesky solve of a symmetric positive definite system.
fn solve(matrix: &mut [f64], rhs: &mut [f64], m: usize) -> bool {
    for i in 0..m {
        for j in 0..=i {
            let mut sum = matrix[i * m + j];
            for k in 0..j {
                sum -= matrix[i * m + k] * matrix[j * m + k];
            }
            if i == j {
                if sum.is_nan() || sum <= 1e-12 {
                    return false;
                }
                matrix[i * m + i] = sum.sqrt();
            } else {
                matrix[i * m + j] = sum / matrix[j * m + j];
            }
        }
    }
    for i in 0..m {
        let mut sum = rhs[i];
        for k in 0..i {
            sum -= matrix[i * m + k] * rhs[k];
        }
        rhs[i] = sum / matrix[i * m + i];
    }
    for i in (0..m).rev() {
        let mut sum = rhs[i];
        for k in i + 1..m {
            sum -= matrix[k * m + i] * rhs[k];
        }
        rhs[i] = sum / matrix[i * m + i];
    }
    rhs.iter().all(|value| value.is_finite())
}

/// Fallback for an ill-conditioned model: join each gap's neighbours.
fn straight_lines(output: &mut [f32], unknown: &[usize]) {
    let mut i = 0;
    while i < unknown.len() {
        let mut j = i;
        while j + 1 < unknown.len() && unknown[j + 1] == unknown[j] + 1 {
            j += 1;
        }
        let (first, last) = (unknown[i], unknown[j]);
        let before = if first > 0 { output[first - 1] } else { 0.0 };
        let after = output.get(last + 1).copied().unwrap_or(0.0);
        let steps = (last - first + 2) as f32;
        for (offset, sample) in output[first..=last].iter_mut().enumerate() {
            let part = (offset + 1) as f32 / steps;
            *sample = before + (after - before) * part;
        }
        i = j + 1;
    }
}

// ---------------------------------------------------------------------------
// Noise reduction: overlap-add Wiener gain against a stationary noise spectrum.

/// Frames used by the automatic estimate, evenly spread over the selection.
const ESTIMATE_FRAMES: usize = 512;
/// Fewest analysis frames the automatic estimate accepts.
const ESTIMATE_MINIMUM: usize = 8;
/// Decision-directed smoothing of the signal-to-noise estimate.
const SMOOTHING: f32 = 0.98;
/// Quantile over time taken as the automatic noise floor, and the factor
/// restoring the mean of an exponentially distributed periodogram from it.
const QUANTILE: f64 = 0.25;
const QUANTILE_BIAS: f32 = 3.476; // 1 / -ln(1 - 0.25)
/// Bins either side of the frequency median that rejects stationary tones.
const TONE_REJECTION: usize = 12;

/// Transform length for a sample rate: 43 ms at 48 kHz.
pub fn noise_window(rate: u32) -> usize {
    match rate {
        0..=50_000 => 2048,
        50_001..=100_000 => 4096,
        _ => 8192,
    }
}

struct Stft {
    size: usize,
    window: Vec<f32>,
    forward: Radix4<f32>,
    inverse: Radix4<f32>,
}

impl Stft {
    fn new(size: usize) -> Self {
        Self {
            size,
            // Periodic Hann. Used twice, it overlap-adds to 1.5 at a quarter hop.
            window: (0..size)
                .map(|i| {
                    (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / size as f64).cos()) as f32
                })
                .collect(),
            forward: Radix4::new(size, FftDirection::Forward),
            inverse: Radix4::new(size, FftDirection::Inverse),
        }
    }

    /// Windowed spectrum of one channel at a signed frame position. Frames
    /// beyond the buffer read as silence.
    fn analyze(
        &self,
        data: &[f32],
        channels: usize,
        channel: usize,
        position: isize,
        out: &mut [Complex<f32>],
    ) {
        let frames = (data.len() / channels) as isize;
        for (i, (bin, window)) in out.iter_mut().zip(&self.window).enumerate() {
            let frame = position + i as isize;
            let sample = if (0..frames).contains(&frame) {
                data[frame as usize * channels + channel]
            } else {
                0.0
            };
            *bin = Complex::new(sample * window, 0.0);
        }
        self.forward.process(out);
    }
}

/// Mean noise power per bin, linked across channels, learned from a
/// noise-only range.
fn profile_noise(
    stft: &Stft,
    data: &[f32],
    channels: usize,
    start: usize,
    end: usize,
) -> Result<Vec<f32>, String> {
    let size = stft.size;
    if end - start < size {
        return Err(format!(
            "The noise profile needs at least {size} frames. Capture a longer noise-only range."
        ));
    }
    let bins = size / 2 + 1;
    let mut power = vec![0.0_f64; bins];
    let mut spectrum = vec![Complex::new(0.0, 0.0); size];
    let mut count = 0;
    let mut position = start;
    while position + size <= end {
        for channel in 0..channels {
            stft.analyze(data, channels, channel, position as isize, &mut spectrum);
            for (sum, bin) in power.iter_mut().zip(&spectrum) {
                *sum += f64::from(bin.norm_sqr());
            }
        }
        count += channels;
        position += size / 2;
    }
    Ok(power
        .iter()
        .map(|sum| (sum / count as f64) as f32)
        .collect())
}

/// Broadband noise floor of the processed range itself: per bin, a low
/// quantile over time (the floor between musical events), then a median
/// across neighbouring bins so that sustained tones are not learned as noise.
/// Narrow hum is therefore not captured; that needs a profile.
fn automatic_noise(
    stft: &Stft,
    data: &[f32],
    channels: usize,
    start: usize,
    end: usize,
) -> Result<Vec<f32>, String> {
    let size = stft.size;
    let hop = size / 2;
    let available = (end - start).saturating_sub(size) / hop + 1;
    if end - start < size || available < ESTIMATE_MINIMUM {
        return Err(format!(
            "Automatic noise reduction needs at least {} frames. Select a longer range or capture a noise profile.",
            size + (ESTIMATE_MINIMUM - 1) * hop
        ));
    }
    let stride = available.div_ceil(ESTIMATE_FRAMES);
    let used = available.div_ceil(stride);
    let bins = size / 2 + 1;
    let mut floor = vec![0.0_f32; bins];
    let mut spectrum = vec![Complex::new(0.0, 0.0); size];
    let mut history = vec![0.0_f32; used * bins];
    let mut column = vec![0.0_f32; used];
    for channel in 0..channels {
        for frame in 0..used {
            let position = start + frame * stride * hop;
            stft.analyze(data, channels, channel, position as isize, &mut spectrum);
            for (bin, value) in spectrum[..bins].iter().enumerate() {
                history[frame * bins + bin] = value.norm_sqr();
            }
        }
        let rank = (QUANTILE * (used - 1) as f64).round() as usize;
        for (bin, floor) in floor.iter_mut().enumerate() {
            for (frame, value) in column.iter_mut().enumerate() {
                *value = history[frame * bins + bin];
            }
            let (_, quantile, _) = column.select_nth_unstable_by(rank, f32::total_cmp);
            *floor += *quantile * QUANTILE_BIAS / channels as f32;
        }
    }
    let mut neighbours = Vec::with_capacity(TONE_REJECTION * 2 + 1);
    Ok((0..bins)
        .map(|bin| {
            neighbours.clear();
            neighbours.extend_from_slice(
                &floor[bin.saturating_sub(TONE_REJECTION)..(bin + TONE_REJECTION + 1).min(bins)],
            );
            let middle = neighbours.len() / 2;
            *neighbours.select_nth_unstable_by(middle, f32::total_cmp).1
        })
        .collect())
}

/// Attenuates stationary noise in the selection by up to `reduction_db`.
///
/// One gain per bin is shared by all channels, so the stereo image holds.
/// The gain is a decision-directed Wiener estimate limited to the reduction
/// floor: noise is turned down evenly rather than gated, which avoids
/// isolated "musical" tones. Where the selection ends inside the clip, the
/// result is crossfaded into the untouched audio over a quarter window.
pub fn reduce_noise(
    data: &mut [f32],
    channels: usize,
    rate: u32,
    start: usize,
    end: usize,
    reduction_db: f32,
    profile: Option<(usize, usize)>,
) -> Result<(), String> {
    if !reduction_db.is_finite() || !(0.1..=48.0).contains(&reduction_db) {
        return Err("Noise reduction must be between 0.1 and 48 dB.".into());
    }
    let frames = data.len() / channels;
    let stft = Stft::new(noise_window(rate));
    let noise = match profile {
        Some((first, last)) => {
            if first >= last || last > frames {
                return Err("The noise profile is outside the clip. Capture it again.".into());
            }
            profile_noise(&stft, data, channels, first, last)?
        }
        None => automatic_noise(&stft, data, channels, start, end)?,
    };
    if !noise.iter().any(|power| *power > 0.0) {
        return Err("The noise profile is silent, so there is nothing to reduce.".into());
    }
    let size = stft.size;
    let hop = size / 4;
    let bins = size / 2 + 1;
    let floor = 10.0_f32.powf(-reduction_db / 20.0);
    let mut spectra = vec![vec![Complex::new(0.0_f32, 0.0); size]; channels];
    let mut overlap = vec![vec![0.0_f32; size]; channels];
    // Previous frame's gain squared times its a-posteriori ratio, per bin.
    let mut carried = vec![0.0_f32; bins];
    let mut gains = vec![0.0_f32; bins];
    let blend = hop.min((end - start) / 2).max(1);
    let scale = 1.0 / (1.5 * size as f32);
    // Eight extra frames before the selection settle the estimate.
    let mut position = start as isize - (size + 8 * hop) as isize + hop as isize;
    while position < end as isize {
        for (channel, spectrum) in spectra.iter_mut().enumerate() {
            stft.analyze(data, channels, channel, position, spectrum);
        }
        for bin in 0..bins {
            let power = spectra.iter().map(|s| s[bin].norm_sqr()).sum::<f32>() / channels as f32;
            let gain = if noise[bin] > 0.0 {
                let ratio = power / noise[bin];
                let prior = SMOOTHING * carried[bin] + (1.0 - SMOOTHING) * (ratio - 1.0).max(0.0);
                let gain = (prior / (1.0 + prior)).max(floor);
                carried[bin] = gain * gain * ratio;
                gain
            } else {
                1.0
            };
            gains[bin] = gain;
        }
        for (spectrum, overlap) in spectra.iter_mut().zip(&mut overlap) {
            for bin in 0..bins {
                spectrum[bin] *= gains[bin];
                if bin > 0 && bin < size / 2 {
                    spectrum[size - bin] *= gains[bin];
                }
            }
            stft.inverse.process(spectrum);
            for ((sum, value), window) in overlap.iter_mut().zip(spectrum.iter()).zip(&stft.window)
            {
                *sum += value.re * window * scale;
            }
        }
        // The first hop of the overlap buffer is now complete. Later frames
        // only read frames at or after position + hop, so write in place.
        for i in 0..hop {
            let frame = position + i as isize;
            if frame < start as isize || frame >= end as isize {
                continue;
            }
            let frame = frame as usize;
            let mut wet = 1.0_f32;
            if start > 0 {
                wet = wet.min((frame - start + 1) as f32 / (blend + 1) as f32);
            }
            if end < frames {
                wet = wet.min((end - frame) as f32 / (blend + 1) as f32);
            }
            for (channel, overlap) in overlap.iter().enumerate() {
                let sample = &mut data[frame * channels + channel];
                *sample += (overlap[i] - *sample) * wet;
                if !sample.is_finite() {
                    return Err("Noise reduction produced non-finite audio.".into());
                }
            }
        }
        for overlap in &mut overlap {
            overlap.copy_within(hop.., 0);
            overlap[size - hop..].fill(0.0);
        }
        position += hop as isize;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Deterministic uniform noise in [-1, 1).
    struct Noise(u64);
    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 40) as f32 / (1u64 << 23) as f32) - 1.0
        }
        /// Approximately Gaussian with unit variance.
        fn gaussian(&mut self) -> f32 {
            (0..12).map(|_| self.next()).sum::<f32>() * 0.5
        }
    }

    fn sine(frames: usize, hz: f64, level: f64) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                ((std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() * level) as f32
            })
            .collect()
    }

    fn stereo(left: &[f32], right: &[f32]) -> Vec<f32> {
        left.iter().zip(right).flat_map(|(l, r)| [*l, *r]).collect()
    }

    fn db(ratio: f64) -> f64 {
        10.0 * ratio.log10()
    }

    #[test]
    fn audio_clean_dc_removal_zeroes_the_mean_and_keeps_the_signal() {
        let tone = sine(4800, 440.0, 0.4);
        let offset: Vec<f32> = tone.iter().map(|s| s + 0.25).collect();
        let other: Vec<f32> = tone.iter().map(|s| -s - 0.1).collect();
        let mut data = stereo(&offset, &other);
        let original = data.clone();
        remove_dc(&mut data, 2, 480, 4320);
        assert_eq!(&data[..960], &original[..960]);
        assert_eq!(&data[8640..], &original[8640..]);
        let selected = &data[960..8640];
        for channel in 0..2 {
            let mean = selected
                .iter()
                .skip(channel)
                .step_by(2)
                .map(|s| f64::from(*s))
                .sum::<f64>()
                / 3840.0;
            assert!(mean.abs() < 1e-7, "mean {mean}");
        }
        // 3840 frames hold exactly 35.2 cycles, so the tone's own mean over
        // the selection is not zero: compare against that exact expectation.
        let tone_mean = tone[480..4320].iter().map(|s| f64::from(*s)).sum::<f64>() / 3840.0;
        let mut worst = 0.0_f64;
        for (frame, pair) in selected.as_chunks::<2>().0.iter().enumerate() {
            let clean = f64::from(tone[480 + frame]) - tone_mean;
            worst = worst
                .max((f64::from(pair[0]) - clean).abs())
                .max((f64::from(pair[1]) + clean).abs());
        }
        assert!(worst < 1e-6, "alternating content moved by {worst}");
    }

    #[test]
    fn audio_clean_silence_bounds_are_exact_and_stereo_aware() {
        let mut data = vec![0.0_f32; 200];
        data[2 * 30 + 1] = 0.01; // right only
        data[2 * 69] = -0.01; // left only
        data[2 * 80] = 0.0005; // below -60 dBFS
        assert_eq!(
            silence_bounds(&data, 2, 0, 100, -60.0).unwrap(),
            Some((30, 70))
        );
        assert_eq!(
            silence_bounds(&data, 2, 0, 100, -80.0).unwrap(),
            Some((30, 81))
        );
        assert_eq!(silence_bounds(&data, 2, 40, 60, -60.0).unwrap(), None);
        assert_eq!(
            silence_bounds(&data, 2, 30, 31, -60.0).unwrap(),
            Some((30, 31))
        );
        assert!(silence_bounds(&data, 2, 0, 100, 0.0).is_err());
        assert!(silence_bounds(&data, 2, 0, 100, f32::NAN).is_err());
    }

    fn clicked(clean: &[f32]) -> (Vec<f32>, usize) {
        let mut noise = Noise(7);
        let mut dirty = clean.to_vec();
        let mut count = 0;
        // 40 clicks: alternating single samples and three-sample bursts.
        for click in 0..40 {
            let at = 600 + click * 1150 + (noise.next().abs() * 200.0) as usize;
            let width = if click % 2 == 0 { 1 } else { 3 };
            for i in 0..width {
                let sign = if (click + i) % 2 == 0 { 1.0 } else { -1.0 };
                dirty[at + i] += sign * (0.35 + 0.4 * noise.next().abs());
                count += 1;
            }
        }
        (dirty, count)
    }

    #[test]
    fn audio_clean_declick_repairs_injected_clicks_on_a_sine() {
        let clean = sine(48_000, 440.0, 0.5);
        let (dirty, injected) = clicked(&clean);
        let before = clean
            .iter()
            .zip(&dirty)
            .fold(0.0_f32, |worst, (a, b)| worst.max((a - b).abs()));
        assert!(before > 0.35);
        let mut data = stereo(&dirty, &clean);
        let repaired = declick(&mut data, 2, RATE, 0, 48_000, 0.5).unwrap();
        assert!(repaired >= injected, "{repaired} of {injected}");
        assert!(repaired <= injected * 4, "{repaired} of {injected}");
        let mut worst = 0.0_f32;
        let mut square = 0.0_f64;
        for (frame, pair) in data.as_chunks::<2>().0.iter().enumerate() {
            worst = worst.max((pair[0] - clean[frame]).abs());
            square += f64::from(pair[0] - clean[frame]).powi(2);
            // The clean right channel is untouched, bit for bit.
            assert_eq!(pair[1].to_bits(), clean[frame].to_bits());
        }
        let rms = (square / 48_000.0).sqrt();
        println!("declick: worst residual {worst:e}, rms residual {rms:e}, repaired {repaired}");
        assert!(worst < 1e-3, "worst residual {worst}");
        assert!(rms < 1e-5, "rms residual {rms}");
    }

    #[test]
    fn audio_clean_declick_repairs_clicks_in_a_noisy_chord_to_its_noise_level() {
        let mut noise = Noise(5);
        let clean: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                let sum: f64 = [220.0, 277.18, 329.63, 440.0, 1760.0]
                    .iter()
                    .map(|hz| (std::f64::consts::TAU * hz * t).sin())
                    .sum();
                (sum * 0.12) as f32 + noise.gaussian() * 0.003
            })
            .collect();
        let (dirty, injected) = clicked(&clean);
        let mut data = stereo(&dirty, &dirty);
        let repaired = declick(&mut data, 2, RATE, 0, 48_000, 0.5).unwrap();
        let (mut worst, mut square, mut changed) = (0.0_f32, 0.0_f64, 0);
        for (frame, pair) in data.as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(pair[0].to_bits(), pair[1].to_bits());
            worst = worst.max((pair[0] - clean[frame]).abs());
            square += f64::from(pair[0] - clean[frame]).powi(2);
            changed += usize::from(pair[0] != dirty[frame]);
        }
        let rms = (square / changed as f64).sqrt();
        println!(
            "declick chord: worst residual {worst:e}, rms over {changed} repaired samples {rms:e}, {injected} injected"
        );
        assert_eq!(repaired, changed * 2);
        assert!(changed >= injected && changed <= injected * 3, "{changed}");
        // The unpredictable part of this signal is its 0.003 RMS noise.
        assert!(worst < 0.02, "worst residual {worst}");
        assert!(rms < 0.006, "rms residual {rms}");
    }

    #[test]
    fn audio_clean_declick_leaves_clean_audio_and_the_outside_untouched() {
        // A pure tone, a harmonic chord with a sharp onset, and steady noise.
        let tone = sine(24_000, 1000.0, 0.8);
        let mut noise = Noise(11);
        let chord: Vec<f32> = (0..24_000)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                let gate = if i < 6000 {
                    0.0
                } else {
                    (-(t - 0.125) * 6.0).exp()
                };
                let sum: f64 = [220.0, 277.18, 329.63, 440.0, 880.0]
                    .iter()
                    .map(|hz| (std::f64::consts::TAU * hz * t).sin())
                    .sum();
                (sum * 0.15 * gate) as f32 + noise.gaussian() * 0.003
            })
            .collect();
        for strength in [0.0, 0.5, 1.0] {
            let mut data = stereo(&tone, &chord);
            let original = data.clone();
            let repaired = declick(&mut data, 2, RATE, 0, 24_000, strength).unwrap();
            let worst = data
                .iter()
                .zip(&original)
                .fold(0.0_f32, |worst, (a, b)| worst.max((a - b).abs()));
            println!(
                "declick clean: strength {strength}, repaired {repaired}, worst change {worst:e}"
            );
            if strength <= 0.5 {
                assert_eq!(repaired, 0);
                assert!(
                    data.iter()
                        .zip(&original)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            } else {
                // The most sensitive setting may touch isolated noise peaks.
                assert!(repaired <= 24, "{repaired}");
                assert!(worst < 0.02, "{worst}");
            }
        }
        // Clicks outside the selection stay, and nothing outside is written.
        let clean = sine(48_000, 440.0, 0.5);
        let (dirty, _) = clicked(&clean);
        let mut data = stereo(&dirty, &dirty);
        let original = data.clone();
        declick(&mut data, 2, RATE, 12_000, 24_000, 0.5).unwrap();
        assert_eq!(&data[..24_000], &original[..24_000]);
        assert_eq!(&data[48_000..], &original[48_000..]);
        assert_ne!(&data[24_000..48_000], &original[24_000..48_000]);
        for (frame, pair) in data
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .skip(12_000)
            .take(12_000)
        {
            assert!((pair[0] - clean[frame]).abs() < 1e-3);
        }
        assert!(declick(&mut data, 2, RATE, 0, 10, 1.5).is_err());
        assert!(declick(&mut data, 2, RATE, 0, 10, f32::NAN).is_err());
    }

    /// Level of `hz` in the frames, and the power of everything else.
    fn tone_and_rest(samples: &[f32], hz: f64) -> (f64, f64) {
        let (mut sin, mut cos, mut total) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (i, s) in samples.iter().enumerate() {
            let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
            sin += f64::from(*s) * phase.sin();
            cos += f64::from(*s) * phase.cos();
            total += f64::from(*s).powi(2);
        }
        let n = samples.len() as f64;
        let amplitude = 2.0 * (sin * sin + cos * cos).sqrt() / n;
        let tone = amplitude * amplitude / 2.0;
        (amplitude, total / n - tone)
    }

    /// Two seconds: half a second of noise alone, then tone plus noise.
    fn noisy() -> (Vec<f32>, Vec<f32>) {
        let mut noise = Noise(3);
        let tone = sine(96_000, 1000.0, 0.5);
        let mut left = Vec::with_capacity(96_000);
        let mut right = Vec::with_capacity(96_000);
        for (i, tone) in tone.iter().enumerate() {
            let signal = if i < 24_000 { 0.0 } else { *tone };
            left.push(signal + noise.gaussian() * 0.02);
            right.push(signal + noise.gaussian() * 0.02);
        }
        (left, right)
    }

    #[test]
    fn audio_clean_noise_reduction_improves_snr_in_profile_and_automatic_modes() {
        let (left, right) = noisy();
        let input = stereo(&left, &right);
        // Measured over one second well inside the processed range.
        let measure = |data: &[f32]| {
            let left: Vec<f32> = data[2 * 36_000..2 * 84_000]
                .iter()
                .step_by(2)
                .copied()
                .collect();
            let (amplitude, rest) = tone_and_rest(&left, 1000.0);
            (amplitude, db(amplitude * amplitude / 2.0 / rest))
        };
        let (level_before, snr_before) = measure(&input);
        for (name, profile) in [("profile", Some((0, 24_000))), ("automatic", None)] {
            let mut data = input.clone();
            reduce_noise(&mut data, 2, RATE, 24_000, 90_000, 20.0, profile).unwrap();
            assert_eq!(&data[..48_000], &input[..48_000]);
            assert_eq!(&data[180_000..], &input[180_000..]);
            let (level, snr) = measure(&data);
            let change = 20.0 * (level / level_before).log10();
            println!(
                "noise reduction ({name}): SNR {snr_before:.2} -> {snr:.2} dB, tone level {change:+.3} dB"
            );
            assert!(snr - snr_before > 15.0, "{name}: {snr_before} -> {snr}");
            assert!(change.abs() < 0.2, "{name}: tone level moved {change} dB");
            let mut again = input.clone();
            reduce_noise(&mut again, 2, RATE, 24_000, 90_000, 20.0, profile).unwrap();
            assert!(
                data.iter()
                    .zip(&again)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
    }

    #[test]
    fn audio_clean_noise_reduction_turns_noise_down_by_the_requested_amount() {
        let (left, right) = noisy();
        let input = stereo(&left, &right);
        let power = |data: &[f32]| {
            data[2 * 4000..2 * 20_000]
                .iter()
                .map(|s| f64::from(*s).powi(2))
                .sum::<f64>()
        };
        for amount in [6.0_f32, 12.0, 24.0] {
            let mut data = input.clone();
            // Learn from the second half of the noise, reduce the first half.
            reduce_noise(
                &mut data,
                2,
                RATE,
                0,
                24_000,
                amount,
                Some((12_000, 24_000)),
            )
            .unwrap();
            let reduced = db(power(&input) / power(&data));
            println!("noise reduction: requested {amount} dB, measured {reduced:.2} dB");
            assert!(
                (reduced - f64::from(amount)).abs() < 1.5,
                "{amount}: {reduced}"
            );
        }
    }

    #[test]
    fn audio_clean_automatic_noise_estimate_matches_the_true_floor_and_ignores_a_tone() {
        let (left, right) = noisy();
        let data = stereo(&left, &right);
        let stft = Stft::new(noise_window(RATE));
        let truth = profile_noise(&stft, &data, 2, 0, 24_000).unwrap();
        let estimate = automatic_noise(&stft, &data, 2, 24_000, 96_000).unwrap();
        let mean = |values: &[f32]| {
            values.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64
        };
        let error = db(mean(&estimate) / mean(&truth));
        // Bin 43 holds the 1 kHz tone at 2048 points and 48 kHz.
        let tone = db(f64::from(estimate[43]) / mean(&truth));
        println!("automatic estimate: {error:+.2} dB overall, {tone:+.2} dB at the tone");
        assert!(error.abs() < 1.5, "{error}");
        assert!(tone.abs() < 3.0, "{tone}");
    }

    #[test]
    fn audio_clean_noise_reduction_refuses_bad_amounts_profiles_and_short_ranges() {
        let (left, right) = noisy();
        let input = stereo(&left, &right);
        let mut data = input.clone();
        for amount in [0.0, -3.0, 60.0, f32::NAN] {
            assert!(reduce_noise(&mut data, 2, RATE, 0, 96_000, amount, None).is_err());
        }
        for profile in [(0, 2047), (10, 10), (0, 96_001)] {
            assert!(reduce_noise(&mut data, 2, RATE, 0, 96_000, 12.0, Some(profile)).is_err());
        }
        assert!(
            reduce_noise(&mut data, 2, RATE, 0, 9215, 12.0, None)
                .unwrap_err()
                .contains("9216 frames")
        );
        let mut silent = vec![0.0_f32; 40_000];
        assert!(
            reduce_noise(&mut silent, 2, RATE, 0, 20_000, 12.0, Some((0, 4096)))
                .unwrap_err()
                .contains("silent")
        );
        assert_eq!(data, input);
        assert_eq!(noise_window(44_100), 2048);
        assert_eq!(noise_window(96_000), 4096);
        assert_eq!(noise_window(192_000), 8192);
    }
}
