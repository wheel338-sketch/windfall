//! Level, channel and time processing for the audio editor. Worker-only.
//!
//! In-place functions receive only the selected interleaved samples, so they
//! cannot write outside the selection.
use windfall_core::{AudioBuffer, db_to_gain};
use windfall_ipc::{AudioEditChannels, AudioFadeShape, AudioNormalizeMode};
use windfall_project::{ClipStretch, ClipStretchQuality};

use crate::clips::equal_power;

fn peak(selected: &[f32]) -> f32 {
    selected.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

fn scale(selected: &mut [f32], factor: f64) {
    for s in selected {
        *s = (f64::from(*s) * factor) as f32;
    }
}

/// Linked gain. A boost that would pass full scale is refused with the
/// largest boost that fits, rather than clipped or silently stored over-range.
pub fn gain(selected: &mut [f32], db: f32) -> Result<(), String> {
    if !db.is_finite() || !(-96.0..=96.0).contains(&db) || db == 0.0 {
        return Err("Gain must be a nonzero amount between -96 and +96 dB.".into());
    }
    let factor = 10.0_f64.powf(f64::from(db) / 20.0);
    let peak = f64::from(peak(selected));
    if db > 0.0 && peak * factor > 1.0 {
        return Err(format!(
            "A gain of {db:+.2} dB would clip: the selection would peak at {:+.2} dBFS. The largest gain without clipping is {:+.2} dB.",
            20.0 * (peak * factor).log10(),
            -20.0 * peak.log10(),
        ));
    }
    scale(selected, factor);
    Ok(())
}

/// Linked normalization to a peak or RMS target in dBFS.
pub fn normalize(
    selected: &mut [f32],
    target_db: f32,
    mode: AudioNormalizeMode,
) -> Result<(), String> {
    if !target_db.is_finite() || !(-60.0..=0.0).contains(&target_db) {
        return Err("The normalize target must be between -60 and 0 dBFS.".into());
    }
    let peak = peak(selected);
    if peak == 0.0 {
        return Err("The selection is silent and cannot be normalized.".into());
    }
    let target = f64::from(db_to_gain(target_db));
    match mode {
        AudioNormalizeMode::Peak => {
            for s in selected {
                *s = ((f64::from(*s) / f64::from(peak)) * target) as f32;
            }
        }
        AudioNormalizeMode::Rms => {
            let rms = (selected.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
                / selected.len() as f64)
                .sqrt();
            let factor = target / rms;
            if f64::from(peak) * factor > 1.0 {
                return Err(format!(
                    "An RMS level of {target_db:.2} dBFS would clip this selection. The highest RMS target without clipping is {:.2} dBFS.",
                    20.0 * (rms / f64::from(peak)).log10(),
                ));
            }
            scale(selected, factor);
        }
    }
    Ok(())
}

/// Fades reach exact silence and exact full level at the selection's ends.
pub fn fade(selected: &mut [f32], channels: usize, out: bool, shape: AudioFadeShape) {
    let frames = selected.len() / channels;
    let denominator = (frames - 1).max(1) as f64;
    for (frame, samples) in selected.chunks_exact_mut(channels).enumerate() {
        let part = if out {
            (frames - 1 - frame) as f64 / denominator
        } else {
            frame as f64 / denominator
        };
        let level = match shape {
            AudioFadeShape::EqualPower => equal_power(part),
            AudioFadeShape::Linear => part as f32,
            AudioFadeShape::SCurve => (0.5 - 0.5 * (part * std::f64::consts::PI).cos()) as f32,
        };
        for s in samples {
            *s *= level;
        }
    }
}

fn stereo(channels: usize) -> Result<(), String> {
    if channels == 2 {
        Ok(())
    } else {
        Err("This tool needs stereo audio.".into())
    }
}

pub fn swap_channels(selected: &mut [f32], channels: usize) -> Result<(), String> {
    stereo(channels)?;
    for pair in selected.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
    Ok(())
}

/// Both channels become their average, which cannot exceed the louder one.
pub fn mono_sum(selected: &mut [f32], channels: usize) -> Result<(), String> {
    stereo(channels)?;
    for pair in selected.chunks_exact_mut(2) {
        let mono = ((f64::from(pair[0]) + f64::from(pair[1])) * 0.5) as f32;
        pair.fill(mono);
    }
    Ok(())
}

pub fn invert_phase(
    selected: &mut [f32],
    channels: usize,
    which: AudioEditChannels,
) -> Result<(), String> {
    if which != AudioEditChannels::Both {
        stereo(channels)?;
    }
    for frame in selected.chunks_exact_mut(channels) {
        for (channel, s) in frame.iter_mut().enumerate() {
            let chosen = match which {
                AudioEditChannels::Both => true,
                AudioEditChannels::Left => channel == 0,
                AudioEditChannels::Right => channel == 1,
            };
            if chosen {
                *s = -*s;
            }
        }
    }
    Ok(())
}

/// Frames a stretched selection will have.
pub fn stretched_frames(frames: usize, ratio: f64) -> usize {
    windfall_stretch::stretched_frames(frames, ratio)
}

pub fn check_stretch(ratio: f64, semitones: f32) -> Result<(), String> {
    if !ratio.is_finite() || !(0.25..=4.0).contains(&ratio) {
        return Err("The stretch ratio must be between 0.25 and 4.".into());
    }
    if !semitones.is_finite() || !(-24.0..=24.0).contains(&semitones) {
        return Err("The pitch shift must be between -24 and +24 semitones.".into());
    }
    if ratio == 1.0 && semitones == 0.0 {
        return Err("Choose a stretch ratio other than 1 or a pitch shift other than 0.".into());
    }
    Ok(())
}

/// The selection, time-stretched and pitch-shifted independently by the same
/// spectral preparation a stretched playlist clip uses.
pub fn stretch(
    selected: &[f32],
    rate: u32,
    channels: u16,
    ratio: f64,
    semitones: f32,
    quality: ClipStretchQuality,
    formants: bool,
) -> Result<Vec<f32>, String> {
    check_stretch(ratio, semitones)?;
    let source = AudioBuffer::from_interleaved(rate, channels, selected.to_vec());
    let rendered = crate::clip_processing::render(
        &source,
        ClipStretch::Spectral {
            ratio,
            quality,
            formants,
        },
        semitones,
    );
    if rendered.frames() == 0 {
        return Err("The stretched selection would be empty. Select a longer range.".into());
    }
    Ok(rendered.samples().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, hz: f64, level: f64) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = (std::f64::consts::TAU * hz * i as f64 / 48_000.0).sin() * level;
                [s as f32, (s * 0.5) as f32]
            })
            .collect()
    }

    /// Strongest frequency of the left channel between 200 Hz and 1 kHz, by
    /// a 1 Hz Goertzel-style scan refined to 0.1 Hz.
    fn dominant(samples: &[f32]) -> f64 {
        let left: Vec<f64> = samples.iter().step_by(2).map(|s| f64::from(*s)).collect();
        let power = |hz: f64| {
            let (mut sin, mut cos) = (0.0, 0.0);
            for (i, s) in left.iter().enumerate() {
                let phase = std::f64::consts::TAU * hz * i as f64 / 48_000.0;
                sin += s * phase.sin();
                cos += s * phase.cos();
            }
            sin * sin + cos * cos
        };
        let best = |candidates: &mut dyn Iterator<Item = f64>| {
            candidates
                .map(|hz| (hz, power(hz)))
                .fold(
                    (0.0, 0.0),
                    |best, now| if now.1 > best.1 { now } else { best },
                )
                .0
        };
        let coarse = best(&mut (200..1000).map(f64::from));
        best(&mut (-40..=40).map(|step| coarse + f64::from(step) * 0.1))
    }

    #[test]
    fn audio_process_gain_is_exact_and_refuses_to_clip() {
        let original = vec![0.5_f32, -0.25, 0.125, -0.0625];
        let mut data = original.clone();
        gain(&mut data, -6.0).unwrap();
        let factor = 10.0_f64.powf(-6.0 / 20.0);
        for (out, input) in data.iter().zip(&original) {
            assert_eq!(*out, (f64::from(*input) * factor) as f32);
        }
        let mut data = original.clone();
        gain(&mut data, 6.0).unwrap();
        assert!((data[0] - 0.997_631_2).abs() < 1e-6);
        assert_eq!(data[1], -data[0] / 2.0);
        // 0.5 peaks at -6.02 dBFS: +6.03 dB passes full scale.
        let mut data = original.clone();
        let refusal = gain(&mut data, 6.03).unwrap_err();
        assert!(refusal.contains("would clip"), "{refusal}");
        assert!(refusal.contains("+6.02 dB"), "{refusal}");
        assert_eq!(data, original);
        // Reducing an over-range selection is always allowed.
        let mut hot = vec![1.5_f32, -2.0];
        gain(&mut hot, -12.0).unwrap();
        assert!(hot[1].abs() < 0.51);
        for bad in [0.0, f32::NAN, f32::INFINITY, 97.0, -97.0] {
            assert!(gain(&mut data, bad).is_err());
        }
        assert_eq!(data, original);
    }

    #[test]
    fn audio_process_normalize_reaches_peak_and_rms_targets_or_refuses() {
        let original = tone(4800, 1000.0, 0.2);
        let mut data = original.clone();
        normalize(&mut data, -6.0, AudioNormalizeMode::Peak).unwrap();
        assert!((peak(&data) - db_to_gain(-6.0)).abs() < 1e-6);
        // Linked: the right channel stays at half the left.
        assert!((data[2 * 12 + 1] / data[2 * 12] - 0.5).abs() < 1e-6);
        let mut data = original.clone();
        normalize(&mut data, -18.0, AudioNormalizeMode::Rms).unwrap();
        let rms =
            (data.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / data.len() as f64).sqrt();
        assert!((20.0 * rms.log10() + 18.0).abs() < 1e-4, "{rms}");
        // Left 0.2 and right 0.1: the RMS is 5.05 dB under the peak.
        let mut data = original.clone();
        let refusal = normalize(&mut data, -3.0, AudioNormalizeMode::Rms).unwrap_err();
        assert!(refusal.contains("-5.05 dBFS"), "{refusal}");
        assert_eq!(data, original);
        for target in [0.5, -61.0, f32::NAN] {
            assert!(normalize(&mut data, target, AudioNormalizeMode::Peak).is_err());
        }
        let mut silent = vec![0.0_f32; 8];
        assert!(normalize(&mut silent, -1.0, AudioNormalizeMode::Rms).is_err());
    }

    #[test]
    fn audio_process_fade_shapes_have_exact_ends_and_expected_midpoints() {
        for (shape, middle) in [
            (AudioFadeShape::EqualPower, std::f32::consts::FRAC_1_SQRT_2),
            (AudioFadeShape::Linear, 0.5),
            (AudioFadeShape::SCurve, 0.5),
        ] {
            let mut data = vec![1.0_f32; 10];
            fade(&mut data, 2, false, shape);
            assert_eq!(&data[..2], &[0.0, 0.0]);
            assert!((data[4] - middle).abs() < 1e-6, "{shape:?} {}", data[4]);
            assert_eq!(&data[8..], &[1.0, 1.0]);
            let mut out = vec![1.0_f32; 10];
            fade(&mut out, 2, true, shape);
            data.reverse();
            assert_eq!(out, data);
        }
        // The raised cosine starts slower than the line.
        let mut s = vec![1.0_f32; 9];
        fade(&mut s, 1, false, AudioFadeShape::SCurve);
        assert!((s[2] - 0.146_446_6).abs() < 1e-6);
        let mut one = vec![0.7_f32, 0.7];
        fade(&mut one, 2, false, AudioFadeShape::Linear);
        assert_eq!(one, [0.0, 0.0]);
    }

    #[test]
    fn audio_process_channel_tools_are_exact() {
        let original = vec![0.125_f32, -0.375, 0.5, 0.25];
        let mut data = original.clone();
        swap_channels(&mut data, 2).unwrap();
        assert_eq!(data, [-0.375, 0.125, 0.25, 0.5]);
        let mut data = original.clone();
        mono_sum(&mut data, 2).unwrap();
        assert_eq!(data, [-0.125, -0.125, 0.375, 0.375]);
        for (which, expected) in [
            (AudioEditChannels::Both, [-0.125, 0.375, -0.5, -0.25]),
            (AudioEditChannels::Left, [-0.125, -0.375, -0.5, 0.25]),
            (AudioEditChannels::Right, [0.125, 0.375, 0.5, -0.25]),
        ] {
            let mut data = original.clone();
            invert_phase(&mut data, 2, which).unwrap();
            assert_eq!(data, expected);
            invert_phase(&mut data, 2, which).unwrap();
            assert_eq!(data, original);
        }
        let mut mono = vec![0.2_f32, -0.4];
        invert_phase(&mut mono, 1, AudioEditChannels::Both).unwrap();
        assert_eq!(mono, [-0.2, 0.4]);
        assert!(invert_phase(&mut mono, 1, AudioEditChannels::Left).is_err());
        assert!(swap_channels(&mut mono, 1).is_err());
        assert!(mono_sum(&mut mono, 1).is_err());
    }

    #[test]
    fn audio_process_stretch_changes_duration_and_pitch_independently() {
        let source = tone(24_000, 440.0, 0.4);
        let quality = ClipStretchQuality::Standard;
        let longer = stretch(&source, 48_000, 2, 1.5, 0.0, quality, false).unwrap();
        assert_eq!(longer.len(), 36_000 * 2);
        let hz = dominant(&longer[2 * 6000..2 * 30_000]);
        println!("stretch 1.5x: {hz:.1} Hz");
        assert!((hz - 440.0).abs() < 2.0, "{hz}");
        let shorter = stretch(&source, 48_000, 2, 0.5, 0.0, quality, false).unwrap();
        assert_eq!(shorter.len(), 12_000 * 2);
        let hz = dominant(&shorter[2 * 3000..2 * 9000]);
        println!("stretch 0.5x: {hz:.1} Hz");
        assert!((hz - 440.0).abs() < 4.0, "{hz}");
        let higher = stretch(&source, 48_000, 2, 1.0, 12.0, quality, false).unwrap();
        assert_eq!(higher.len(), source.len());
        let hz = dominant(&higher[2 * 6000..2 * 18_000]);
        println!("pitch +12: {hz:.1} Hz");
        assert!((hz - 880.0).abs() < 4.0, "{hz}");
        let lower = stretch(&source, 48_000, 2, 2.0, -7.0, quality, true).unwrap();
        assert_eq!(lower.len(), 48_000 * 2);
        let hz = dominant(&lower[2 * 12_000..2 * 36_000]);
        let expected = 440.0 * 2.0_f64.powf(-7.0 / 12.0);
        println!("stretch 2x, pitch -7: {hz:.1} Hz (expected {expected:.1})");
        assert!((hz - expected).abs() < 3.0, "{hz}");
        // Same input, same bytes.
        let again = stretch(&source, 48_000, 2, 1.5, 0.0, quality, false).unwrap();
        assert!(
            longer
                .iter()
                .zip(&again)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        assert_eq!(stretched_frames(24_000, 1.5), 36_000);
        for (ratio, semitones) in [
            (1.0, 0.0),
            (0.2, 0.0),
            (4.5, 0.0),
            (1.0, 25.0),
            (f64::NAN, 0.0),
        ] {
            assert!(stretch(&source, 48_000, 2, ratio, semitones, quality, false).is_err());
        }
        assert!(stretch(&source, 48_000, 2, 1.0, f32::NAN, quality, false).is_err());
    }
}
