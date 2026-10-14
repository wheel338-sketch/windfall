//! What a render measures: loudness and peaks the way a mastering target
//! states them, the level over time, the energy per octave, the tempo and
//! the key. The loudness is [`windfall_dsp::mastering`]'s meter and the
//! tempo [`windfall_stretch::estimate_tempo`], the same code Windfall's
//! own meters and tempo detection run.

use serde::Serialize;
use windfall_core::AudioBuffer;

use crate::music::{self, KeyEstimate};

/// The longest render an agent may ask for, in seconds.
pub const MAX_RENDER_SECONDS: f64 = 120.0;
/// Windows of the level-over-time list at most.
const LEVEL_WINDOWS: usize = 48;
/// Octave band centres, in hertz.
pub const OCTAVES: [f64; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// Spectrum frames analysed at most; a longer render is sampled evenly.
const MAX_FRAMES: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurements {
    pub seconds: f64,
    pub sample_rate: u32,
    /// Gated programme loudness (ITU-R BS.1770-4), or -120 for silence.
    pub integrated_lufs: f32,
    pub loudness_range_lu: f32,
    pub short_term_max_lufs: f32,
    pub momentary_max_lufs: f32,
    pub true_peak_dbtp: f32,
    pub sample_peak_dbfs: f32,
    /// Samples at or above full scale.
    pub clipped_samples: u64,
    /// True when a sample reached full scale or the true peak passed 0 dBTP.
    pub clipping: bool,
    pub rms_dbfs: f32,
    /// -1 to 1: 1 is mono, 0 unrelated channels, below 0 out of phase.
    pub stereo_correlation: f32,
    pub level_over_time: Vec<LevelWindow>,
    pub octave_bands: Vec<OctaveBand>,
    pub tempo: Option<Tempo>,
    pub key: Option<KeyEstimate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelWindow {
    pub from_seconds: f64,
    pub to_seconds: f64,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OctaveBand {
    pub center_hz: f64,
    /// Energy relative to the loudest band, in dB: 0 for the loudest.
    pub relative_db: f32,
    /// Share of the energy of all bands, 0 to 1.
    pub share: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tempo {
    pub bpm: f64,
    /// Onset periodicity, not a probability.
    pub confidence: f64,
    /// Other candidates, often the double or half tempo.
    pub alternatives: Vec<f64>,
}

fn db(level: f64) -> f32 {
    if level > 0.0 {
        ((20.0 * level.log10()) * 100.0).round() as f32 / 100.0
    } else {
        -120.0
    }
}

fn rounded(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

/// Measures interleaved stereo.
pub fn measure(samples: &[f32], sample_rate: u32) -> Measurements {
    let rate = sample_rate.max(1);
    let frames = samples.len() / 2;
    let stereo = &samples[..frames * 2];
    let loudness = windfall_dsp::mastering::measure(stereo, rate);
    let clipped = stereo.iter().filter(|s| s.abs() >= 1.0).count() as u64;
    let (mut left, mut right, mut cross, mut total) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for frame in stereo.chunks_exact(2) {
        let (l, r) = (f64::from(frame[0]), f64::from(frame[1]));
        left += l * l;
        right += r * r;
        cross += l * r;
        total += l * l + r * r;
    }
    let correlation = if left > 0.0 && right > 0.0 {
        (cross / (left * right).sqrt()) as f32
    } else {
        0.0
    };
    let mono: Vec<f32> = stereo
        .chunks_exact(2)
        .map(|frame| 0.5 * (frame[0] + frame[1]))
        .collect();
    let tempo =
        windfall_stretch::estimate_tempo(&AudioBuffer::from_interleaved(rate, 1, mono.clone()))
            .and_then(|candidates| {
                let best = candidates.first()?;
                Some(Tempo {
                    bpm: (best.bpm * 10.0).round() / 10.0,
                    confidence: (best.confidence * 1000.0).round() / 1000.0,
                    alternatives: candidates[1..]
                        .iter()
                        .map(|c| (c.bpm * 10.0).round() / 10.0)
                        .collect(),
                })
            });
    let (octave_bands, chroma) = spectrum(&mono, rate);
    Measurements {
        seconds: frames as f64 / f64::from(rate),
        sample_rate: rate,
        integrated_lufs: rounded(loudness.integrated_lufs),
        loudness_range_lu: rounded(loudness.loudness_range_lu),
        short_term_max_lufs: rounded(loudness.short_term_max_lufs),
        momentary_max_lufs: rounded(loudness.momentary_max_lufs),
        true_peak_dbtp: rounded(loudness.true_peak_dbtp),
        sample_peak_dbfs: rounded(loudness.sample_peak_db),
        clipped_samples: clipped,
        clipping: clipped > 0 || loudness.true_peak_dbtp > 0.0,
        rms_dbfs: db((total / (2 * frames.max(1)) as f64).sqrt()),
        stereo_correlation: rounded(correlation),
        level_over_time: levels(stereo, rate),
        octave_bands,
        tempo,
        key: music::estimate_key(&chroma),
    }
}

fn levels(stereo: &[f32], rate: u32) -> Vec<LevelWindow> {
    let frames = stereo.len() / 2;
    if frames == 0 {
        return Vec::new();
    }
    let half_second = (rate as usize / 2).max(1);
    let window = frames.div_ceil(LEVEL_WINDOWS).max(half_second);
    stereo
        .chunks(window * 2)
        .enumerate()
        .map(|(index, chunk)| {
            let energy: f64 = chunk.iter().map(|s| f64::from(*s).powi(2)).sum();
            let peak = chunk.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
            let start = index * window;
            LevelWindow {
                from_seconds: (start as f64 / f64::from(rate) * 100.0).round() / 100.0,
                to_seconds: (((start + chunk.len() / 2) as f64) / f64::from(rate) * 100.0).round()
                    / 100.0,
                rms_dbfs: db((energy / chunk.len() as f64).sqrt()),
                peak_dbfs: db(f64::from(peak)),
            }
        })
        .collect()
}

/// In-place radix-2 FFT of `re` and `im`, whose length is a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut length = 2;
    while length <= n {
        let angle = -std::f64::consts::TAU / length as f64;
        let (step_re, step_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(length) {
            let (mut w_re, mut w_im) = (1.0, 0.0);
            for k in 0..length / 2 {
                let (a, b) = (start + k, start + k + length / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next = w_re * step_re - w_im * step_im;
                w_im = w_re * step_im + w_im * step_re;
                w_re = next;
            }
        }
        length <<= 1;
    }
}

/// Octave band energies and pitch-class weights of a mono signal.
fn spectrum(mono: &[f32], rate: u32) -> (Vec<OctaveBand>, [f64; 12]) {
    let size = (rate as usize / 3).next_power_of_two().clamp(1024, 65_536);
    let mut bands = [0.0f64; OCTAVES.len()];
    let mut chroma = [0.0f64; 12];
    let count = mono.len() / size;
    if count > 0 {
        let stride = count.div_ceil(MAX_FRAMES);
        let window: Vec<f64> = (0..size)
            .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / size as f64).cos())
            .collect();
        let bin_hz = f64::from(rate) / size as f64;
        let (mut re, mut im) = (vec![0.0; size], vec![0.0; size]);
        for frame in (0..count).step_by(stride) {
            let block = &mono[frame * size..(frame + 1) * size];
            for (n, sample) in block.iter().enumerate() {
                re[n] = f64::from(*sample) * window[n];
                im[n] = 0.0;
            }
            fft(&mut re, &mut im);
            for k in 1..size / 2 {
                let hz = k as f64 * bin_hz;
                let power = re[k] * re[k] + im[k] * im[k];
                if let Some(band) = OCTAVES
                    .iter()
                    .position(|center| hz >= center / 2f64.sqrt() && hz < center * 2f64.sqrt())
                {
                    bands[band] += power;
                }
                if (100.0..=4200.0).contains(&hz) {
                    let pitch = (12.0 * (hz / 440.0).log2() + 69.0).round();
                    chroma[pitch.rem_euclid(12.0) as usize] += power.sqrt();
                }
            }
        }
    }
    let total: f64 = bands.iter().sum();
    let loudest = bands.iter().copied().fold(0.0, f64::max);
    let octave_bands = OCTAVES
        .iter()
        .zip(bands)
        .map(|(center, energy)| OctaveBand {
            center_hz: *center,
            relative_db: if loudest > 0.0 && energy > 0.0 {
                ((10.0 * (energy / loudest).log10()) * 10.0).round() as f32 / 10.0
            } else {
                -120.0
            },
            share: if total > 0.0 {
                ((energy / total) * 1000.0).round() as f32 / 1000.0
            } else {
                0.0
            },
        })
        .collect();
    (octave_bands, chroma)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: u32 = 48_000;

    fn sine(hz: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        (0..frames)
            .flat_map(|n| {
                let s = (amplitude * (TAU * hz * n as f64 / f64::from(RATE)).sin()) as f32;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn a_sine_at_a_known_level_measures_that_loudness_and_peak() {
        // A 997 Hz sine at half scale in both channels: BS.1770 puts a
        // full-scale 1 kHz sine in one channel at -3.01 LUFS, so this one
        // is 20 log10(0.5) = -6.02 LUFS, peaking at -6.02 dBTP.
        let measured = measure(&sine(997.0, 0.5, 10.0), RATE);
        assert!(
            (measured.integrated_lufs + 6.02).abs() < 0.2,
            "{measured:?}"
        );
        assert!(
            (measured.true_peak_dbtp + 6.02).abs() < 0.15,
            "{measured:?}"
        );
        assert!(
            (measured.sample_peak_dbfs + 6.02).abs() < 0.15,
            "{measured:?}"
        );
        assert!((measured.rms_dbfs + 9.03).abs() < 0.1, "{measured:?}");
        assert!(!measured.clipping);
        assert_eq!(measured.clipped_samples, 0);
        assert!((measured.stereo_correlation - 1.0).abs() < 1e-3);
        assert!((measured.seconds - 10.0).abs() < 1e-9);
        let band = measured
            .octave_bands
            .iter()
            .find(|band| band.center_hz == 1000.0)
            .unwrap();
        assert_eq!(band.relative_db, 0.0);
        assert!(band.share > 0.9, "{:?}", measured.octave_bands);
        assert!(measured.level_over_time.len() <= LEVEL_WINDOWS);
        for window in &measured.level_over_time {
            assert!((window.rms_dbfs + 9.03).abs() < 0.2, "{window:?}");
        }
        // A steady tone has no beat.
        assert_eq!(measured.tempo, None);
    }

    #[test]
    fn full_scale_samples_are_clipping_and_silence_is_the_floor() {
        let hot: Vec<f32> = sine(100.0, 1.5, 2.0)
            .into_iter()
            .map(|s| s.clamp(-1.0, 1.0))
            .collect();
        let measured = measure(&hot, RATE);
        assert!(measured.clipping);
        assert!(measured.clipped_samples > 1000);
        let silent = measure(&vec![0.0; 2 * RATE as usize], RATE);
        assert_eq!(silent.integrated_lufs, -120.0);
        assert!(!silent.clipping);
        assert_eq!(silent.key, None);
        assert_eq!(silent.tempo, None);
        assert!(measure(&[], RATE).level_over_time.is_empty());
    }

    #[test]
    fn a_beat_at_a_known_tempo_is_detected() {
        // A pitched thump on every beat at 120 bpm for 16 beats.
        let beat = (0.5 * f64::from(RATE)) as usize;
        let frames = 16 * beat;
        let mut samples = vec![0.0f32; frames * 2];
        for start in (0..frames).step_by(beat) {
            for n in start..(start + beat).min(frames) {
                let since = (n - start) as f64 / f64::from(RATE);
                let s = (0.8 * (TAU * 80.0 * since).sin() * (-since * 12.0).exp()) as f32;
                samples[2 * n] = s;
                samples[2 * n + 1] = s;
            }
        }
        let tempo = measure(&samples, RATE).tempo.expect("a periodic beat");
        let all: Vec<f64> = std::iter::once(tempo.bpm)
            .chain(tempo.alternatives.iter().copied())
            .collect();
        assert!(all.iter().any(|bpm| (bpm - 120.0).abs() < 1.0), "{tempo:?}");
    }

    #[test]
    fn a_chord_s_pitches_give_its_key() {
        // A minor triad over its octave-lower tonic, then the scale's
        // other notes more quietly: A3, C4, E4 loud; B, D, F, G soft.
        let seconds = 4.0;
        let frames = (seconds * f64::from(RATE)) as usize;
        let tones = [
            (220.0, 0.3),
            (261.63, 0.25),
            (329.63, 0.25),
            (110.0, 0.2),
            (246.94, 0.06),
            (293.66, 0.06),
            (349.23, 0.06),
            (392.0, 0.06),
        ];
        let samples: Vec<f32> = (0..frames)
            .flat_map(|n| {
                let t = n as f64 / f64::from(RATE);
                let s: f64 = tones
                    .iter()
                    .map(|(hz, amplitude)| amplitude * (TAU * hz * t).sin())
                    .sum();
                [s as f32, s as f32]
            })
            .collect();
        let key = measure(&samples, RATE).key.expect("pitched audio");
        assert_eq!(key.name, "A minor", "{key:?}");
    }

    #[test]
    fn the_fft_matches_a_direct_transform() {
        let n = 16;
        let input: Vec<f64> = (0..n).map(|i| ((i * 7 % 5) as f64) - 2.0).collect();
        let (mut re, mut im) = (input.clone(), vec![0.0; n]);
        fft(&mut re, &mut im);
        for k in 0..n {
            let (mut dr, mut di) = (0.0, 0.0);
            for (i, x) in input.iter().enumerate() {
                let angle = -TAU * (k * i) as f64 / n as f64;
                dr += x * angle.cos();
                di += x * angle.sin();
            }
            assert!((re[k] - dr).abs() < 1e-9 && (im[k] - di).abs() < 1e-9);
        }
    }
}
