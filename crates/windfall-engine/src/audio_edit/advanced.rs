//! Worker-only bounded spectral processing and repair. No callback entry points.
use rustfft::{FftPlanner, num_complex::Complex};
use windfall_core::AudioBuffer;

const FFT: usize = 1024;
const HOP: usize = FFT / 4;

fn finite_range(value: f32, first: f32, last: f32, name: &str) -> Result<(), String> {
    if value.is_finite() && (first..=last).contains(&value) {
        Ok(())
    } else {
        Err(windfall_core::notice!(
            "engine.edit.between",
            what = windfall_core::notice::term(name),
            first = first,
            last = last
        )
        .to_string())
    }
}

/// Hann overlap-add with a normalizing window. Only the supplied selection is
/// written; leading/trailing windows are zero-padded, never circularly wrapped.
fn spectral(data: &mut [f32], channels: usize, mut bins: impl FnMut(&mut [Complex<f32>])) {
    let frames = data.len() / channels;
    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(FFT);
    let inverse = planner.plan_fft_inverse(FFT);
    let window: Vec<_> = (0..FFT)
        .map(|i| (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2))
        .collect();
    let mut spectrum = vec![Complex::default(); FFT];
    let mut scratch = vec![
        Complex::default();
        forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
    ];
    let mut output = vec![0.0; data.len()];
    let mut weight = vec![0.0_f32; frames];
    let mut at = -(FFT as isize) + HOP as isize;
    while at < frames as isize {
        for (i, w) in window.iter().enumerate() {
            let frame = at + i as isize;
            if (0..frames as isize).contains(&frame) {
                weight[frame as usize] += w * w;
            }
        }
        for channel in 0..channels {
            for (i, slot) in spectrum.iter_mut().enumerate() {
                let frame = at + i as isize;
                *slot = Complex::new(
                    if (0..frames as isize).contains(&frame) {
                        data[frame as usize * channels + channel] * window[i]
                    } else {
                        0.0
                    },
                    0.0,
                );
            }
            forward.process_with_scratch(&mut spectrum, &mut scratch);
            bins(&mut spectrum);
            inverse.process_with_scratch(&mut spectrum, &mut scratch);
            for (i, slot) in spectrum.iter().enumerate() {
                let frame = at + i as isize;
                if (0..frames as isize).contains(&frame) {
                    output[frame as usize * channels + channel] += slot.re * window[i] / FFT as f32;
                }
            }
        }
        at += HOP as isize;
    }
    for (i, value) in data.iter_mut().enumerate() {
        *value = output[i] / weight[i / channels].max(1e-12);
    }
}

pub fn equalize(
    data: &mut [f32],
    channels: usize,
    rate: u32,
    gains: [f32; 3],
) -> Result<(), String> {
    for gain in gains {
        finite_range(gain, -24.0, 24.0, "EQ gain")?;
    }
    if gains == [0.0; 3] {
        return Err("Choose a nonzero EQ gain.".into());
    }
    spectral(data, channels, |spectrum| {
        for (bin, value) in spectrum.iter_mut().enumerate() {
            let hz = bin.min(FFT - bin) as f32 * rate as f32 / FFT as f32;
            let low = 1.0 / (1.0 + (hz / 250.0).powi(4));
            let high = 1.0 - 1.0 / (1.0 + (hz / 4000.0).powi(4));
            let db = gains[0] * low + gains[1] * (1.0 - low - high) + gains[2] * high;
            *value *= 10.0_f32.powf(db / 20.0);
        }
    });
    if data.iter().any(|s| s.abs() > 1.0) {
        return Err("EQ would clip. Lower its gains or the selection level.".into());
    }
    Ok(())
}

pub fn blur(data: &mut [f32], channels: usize, amount: f32) -> Result<(), String> {
    finite_range(amount, 0.01, 1.0, "Blur amount")?;
    let radius = (amount * 16.0).ceil() as usize;
    let mut magnitudes = vec![0.0; FFT / 2 + 1];
    spectral(data, channels, |spectrum| {
        for (bin, magnitude) in magnitudes.iter_mut().enumerate() {
            *magnitude = spectrum[bin].norm();
        }
        for bin in 0..=FFT / 2 {
            let a = bin.saturating_sub(radius);
            let b = (bin + radius + 1).min(magnitudes.len());
            let blurred = magnitudes[a..b].iter().sum::<f32>() / (b - a) as f32;
            let magnitude = magnitudes[bin] * (1.0 - amount) + blurred * amount;
            let original = spectrum[bin];
            let phase = if original.norm() > 1e-12 {
                original / original.norm()
            } else {
                Complex::new(1.0, 0.0)
            };
            spectrum[bin] = phase * magnitude;
            if bin > 0 && bin < FFT / 2 {
                spectrum[FFT - bin] = spectrum[bin].conj();
            }
        }
    });
    Ok(())
}

/// Linear FFT overlap-add convolution, independent IR per channel. The
/// selection retains its length: the convolution tail outside it is discarded.
pub fn convolve(
    data: &[f32],
    impulse: &[f32],
    channels: usize,
    wet: f32,
) -> Result<Vec<f32>, String> {
    finite_range(wet, 0.01, 1.0, "Convolution mix")?;
    let ir_frames = impulse.len() / channels;
    if ir_frames == 0 || ir_frames > 32768 || impulse.iter().any(|s| !s.is_finite()) {
        return Err("The impulse must contain 1 to 32768 finite frames.".into());
    }
    if impulse.iter().all(|s| *s == 0.0) {
        return Err("A silent impulse cannot be used.".into());
    }
    const BLOCK: usize = 16384;
    let size = (BLOCK + ir_frames - 1).next_power_of_two();
    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let mut scratch = vec![
        Complex::default();
        forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
    ];
    let mut ir = vec![Complex::default(); size];
    let mut block = ir.clone();
    let mut output = vec![0.0_f32; data.len()];
    let frames = data.len() / channels;
    for channel in 0..channels {
        ir.fill(Complex::default());
        for frame in 0..ir_frames {
            ir[frame].re = impulse[frame * channels + channel];
        }
        forward.process_with_scratch(&mut ir, &mut scratch);
        for at in (0..frames).step_by(BLOCK) {
            block.fill(Complex::default());
            let count = BLOCK.min(frames - at);
            for frame in 0..count {
                block[frame].re = data[(at + frame) * channels + channel];
            }
            forward.process_with_scratch(&mut block, &mut scratch);
            for (value, response) in block.iter_mut().zip(&ir) {
                *value *= response;
            }
            inverse.process_with_scratch(&mut block, &mut scratch);
            for (i, value) in block
                .iter()
                .take((count + ir_frames - 1).min(frames - at))
                .enumerate()
            {
                output[(at + i) * channels + channel] += value.re / size as f32;
            }
        }
    }
    for (out, dry) in output.iter_mut().zip(data) {
        *out = dry * (1.0 - wet) + *out * wet;
    }
    Ok(output)
}

/// Reconstruct short clipped plateaus with cubic Hermite interpolation from
/// the slopes on both sides. Longer plateaus lack enough information and refuse.
pub fn repair_clipping(data: &mut [f32], channels: usize, threshold: f32) -> Result<(), String> {
    finite_range(threshold, 0.5, 1.0, "Clipping threshold")?;
    let frames = data.len() / channels;
    let mut repaired = 0;
    for channel in 0..channels {
        let mut i = 0;
        while i < frames {
            if data[i * channels + channel].abs() < threshold {
                i += 1;
                continue;
            }
            let first = i;
            while i < frames && data[i * channels + channel].abs() >= threshold {
                i += 1;
            }
            if first < 2 || i + 1 >= frames || i - first > 256 {
                return Err("Clipped regions need two clean frames on each side and at most 256 clipped frames.".into());
            }
            let a = data[(first - 1) * channels + channel];
            let b = data[i * channels + channel];
            let span = (i - first + 1) as f32;
            let da = (a - data[(first - 2) * channels + channel]) * span;
            let db = (data[(i + 1) * channels + channel] - b) * span;
            for frame in first..i {
                let t = (frame - first + 1) as f32 / span;
                data[frame * channels + channel] = (2.0 * t.powi(3) - 3.0 * t * t + 1.0) * a
                    + (t.powi(3) - 2.0 * t * t + t) * da
                    + (-2.0 * t.powi(3) + 3.0 * t * t) * b
                    + (t.powi(3) - t * t) * db;
            }
            repaired += i - first;
        }
    }
    if repaired == 0 {
        return Err("No clipping was found at that threshold.".into());
    }
    Ok(())
}

/// Blend the loop's ending toward its head, ending exactly at the first
/// sample. This keeps its duration and both channels' seam positions linked.
pub fn loop_tune(data: &mut [f32], channels: usize, crossfade_frames: u32) -> Result<(), String> {
    let frames = data.len() / channels;
    let count = crossfade_frames as usize;
    if count < 2 || count > frames / 2 {
        return Err("Loop crossfade needs 2 frames to half the selection.".into());
    }
    let head = data[..count * channels].to_vec();
    for i in 0..count {
        let t = i as f32 / (count - 1) as f32;
        let blend = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
        // Head context approaches frame zero at the seam rather than moving
        // the loop's first sample or introducing a second playback crossfade.
        let target = count - 1 - i;
        for ch in 0..channels {
            let index = (frames - count + i) * channels + ch;
            data[index] = data[index] * (1.0 - blend) + head[target * channels + ch] * blend;
        }
    }
    Ok(())
}

/// Retain quiet runs shorter than the minimum; separate longer runs into
/// gaps. Outside-selection ranges are retained intact. Linked stereo gate.
pub fn sound_regions(
    view: &AudioBuffer,
    start: usize,
    end: usize,
    threshold_db: f32,
    minimum: u32,
) -> Result<Vec<(usize, usize)>, String> {
    finite_range(threshold_db, -120.0, -6.0, "Silence threshold")?;
    if start >= end || end > view.frames() || minimum == 0 {
        return Err("Choose a valid selection and positive silence duration.".into());
    }
    let threshold = 10.0_f32.powf(threshold_db / 20.0);
    let channels = usize::from(view.channels());
    let quiet = |i: usize| {
        view.samples()[i * channels..(i + 1) * channels]
            .iter()
            .all(|s| s.abs() < threshold)
    };
    let mut regions = Vec::new();
    let mut keep = 0;
    let mut i = start;
    let mut found = false;
    while i < end {
        if !quiet(i) {
            i += 1;
            continue;
        }
        let first = i;
        while i < end && quiet(i) {
            i += 1;
        }
        if i - first >= minimum as usize {
            if keep < first {
                regions.push((keep, first));
            }
            keep = i;
            found = true;
        }
    }
    if keep < view.frames() {
        regions.push((keep, view.frames()));
    }
    if !found || regions.is_empty() {
        return Err("No retained sound separated by qualifying silence was found.".into());
    }
    if regions.len() > 128 {
        return Err("Strip silence supports at most 128 resulting clips per edit.".into());
    }
    Ok(regions)
}

/// Fixed-size worker spectrogram, maximum 128 time columns by 64 frequency
/// bins. Values are normalized dBFS (-96..0); opposite-phase stereo is retained.
pub fn spectrogram(view: &AudioBuffer) -> Vec<f32> {
    let columns = 128;
    let bins = 64;
    let channels = usize::from(view.channels());
    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(FFT);
    let mut block = vec![Complex::default(); FFT];
    let mut scratch = vec![Complex::default(); forward.get_inplace_scratch_len()];
    let mut result = vec![0.0_f32; columns * bins];
    for column in 0..columns {
        let center = column * view.frames() / columns;
        for channel in 0..channels {
            for (i, slot) in block.iter_mut().enumerate() {
                let frame = center as isize + i as isize - FFT as isize / 2;
                let sample = if (0..view.frames() as isize).contains(&frame) {
                    view.samples()[frame as usize * channels + channel]
                } else {
                    0.0
                };
                *slot = Complex::new(
                    sample * (std::f32::consts::PI * i as f32 / FFT as f32).sin().powi(2),
                    0.0,
                );
            }
            forward.process_with_scratch(&mut block, &mut scratch);
            for bin in 0..bins {
                let magnitude = block[bin * (FFT / 2) / bins..(bin + 1) * (FFT / 2) / bins]
                    .iter()
                    .map(|s| s.norm() * 4.0 / FFT as f32)
                    .fold(0.0_f32, f32::max);
                let value = ((20.0 * magnitude.max(1e-12).log10() + 96.0) / 96.0).clamp(0.0, 1.0);
                result[column * bins + bin] = result[column * bins + bin].max(value);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advanced_convolution_matches_direct_linear_reference_without_wrap_or_channel_mix() {
        let data = [0.2, -0.1, 0.3, 0.1, -0.1, 0.4, 0.0, 0.2];
        let ir = [1.0, 0.5, 0.25, -0.5];
        let out = convolve(&data, &ir, 2, 1.0).unwrap();
        for frame in 0..4 {
            for ch in 0..2 {
                let expected = data[frame * 2 + ch] * ir[ch]
                    + if frame > 0 {
                        data[(frame - 1) * 2 + ch] * ir[2 + ch]
                    } else {
                        0.0
                    };
                assert!((out[frame * 2 + ch] - expected).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn advanced_flat_eq_has_independent_gain_reference_and_spectrum_keeps_antiphase() {
        let mut data: Vec<_> = (0..8192)
            .flat_map(|i| {
                let s = 0.2 * (std::f32::consts::TAU * i as f32 * 1000.0 / 48000.0).sin();
                [s, -s]
            })
            .collect();
        let original = data.clone();
        equalize(&mut data, 2, 48000, [-6.0; 3]).unwrap();
        let gain = 10.0_f32.powf(-6.0 / 20.0);
        assert!(
            data.iter()
                .zip(&original)
                .all(|(a, b)| (a - b * gain).abs() < 1e-6)
        );
        let view = AudioBuffer::from_interleaved(48000, 2, original);
        let spectrum = spectrogram(&view);
        assert_eq!(spectrum.len(), 8192);
        assert!(spectrum.iter().any(|v| *v > 0.8));
    }
    #[test]
    fn advanced_repair_loop_and_silence_regions_are_bounded_and_linked() {
        let mut clipped = vec![0.0, 0.1, 0.2, 0.99, 0.99, 0.99, 0.6, 0.7, 0.8];
        repair_clipping(&mut clipped, 1, 0.98).unwrap();
        for (i, s) in clipped.iter().enumerate() {
            assert!((*s - i as f32 * 0.1).abs() < 1e-6);
        }
        let mut looped = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        loop_tune(&mut looped, 1, 3).unwrap();
        assert_eq!(looped[7], looped[0]);
        assert_eq!(&looped[..5], &[0.1, 0.2, 0.3, 0.4, 0.5]);
        let view = AudioBuffer::from_interleaved(
            48000,
            1,
            vec![0.3, 0.3, 0.0, 0.0, 0.0, 0.2, 0.2, 0.0, 0.2],
        );
        assert_eq!(
            sound_regions(&view, 0, 9, -60.0, 2).unwrap(),
            [(0, 2), (5, 9)]
        );
        assert!(repair_clipping(&mut [0.99; 4], 1, 0.98).is_err());
    }
    #[test]
    fn advanced_blur_changes_tone_energy_and_is_deterministic_and_finite() {
        let mut data: Vec<_> = (0..8192)
            .map(|i| 0.2 * (std::f32::consts::TAU * i as f32 * 32.0 / 1024.0).sin())
            .collect();
        let mut other = data.clone();
        let original_energy = data.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
        blur(&mut data, 1, 0.5).unwrap();
        blur(&mut other, 1, 0.5).unwrap();
        assert_eq!(data, other);
        let blurred_energy = data.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
        assert!(blurred_energy > original_energy * 0.1);
        assert!(blurred_energy < original_energy * 0.95);
        assert!(data.iter().all(|s| s.is_finite()));
    }
}
