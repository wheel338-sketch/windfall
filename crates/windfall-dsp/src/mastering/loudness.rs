//! Programme loudness and true peak, measured the way a mastering target is
//! stated.
//!
//! Loudness follows ITU-R BS.1770-4: the signal is K-weighted (a high shelf
//! that models the head, then a high-pass), its energy is averaged over
//! 400 ms blocks that overlap by 75%, and blocks that are silent or far
//! quieter than the rest are left out (gating) before the blocks are
//! averaged into one figure in LUFS. Loudness range follows EBU Tech 3342
//! and the short-term and momentary figures EBU Tech 3341.
//!
//! True peak is the peak of the waveform the samples describe, not of the
//! samples themselves: a converter can put out more than the largest sample.
//! It is estimated by reading the waveform at four times the sample rate
//! (BS.1770-4 Annex 2).
//!
//! The K-weighting coefficients for sample rates other than 48 kHz are
//! derived from the analog prototype the 48 kHz table in the standard
//! corresponds to, with the constants libebur128 (MIT) documents.

use std::f64::consts::PI;

use crate::blocks::biquad::{Biquad, BiquadCoeffs};

/// The figure reported in place of minus infinity: for silence, and for a
/// programme with no block above the absolute gate.
pub const LOUDNESS_FLOOR: f32 = -120.0;

/// Blocks quieter than this are never part of the programme (BS.1770-4).
const ABSOLUTE_GATE_LUFS: f64 = -70.0;

/// Sub-blocks in a momentary block (4 x 100 ms) and a short-term block
/// (30 x 100 ms).
const MOMENTARY_HOPS: usize = 4;
const SHORT_TERM_HOPS: usize = 30;

/// Input samples the interpolator reads either side of the point it
/// estimates, and so the delay of a true-peak reading in samples.
pub const PEAK_DELAY: usize = 6;
const PEAK_TAPS: usize = 2 * PEAK_DELAY;

/// The two K-weighting sections at `sample_rate`: the high shelf, then the
/// high-pass. At 48 kHz these are the coefficients BS.1770-4 tabulates.
pub fn k_weighting(sample_rate: f32) -> [BiquadCoeffs; 2] {
    let rate = if sample_rate.is_finite() {
        f64::from(sample_rate).clamp(8_000.0, 384_000.0)
    } else {
        48_000.0
    };

    let k = (PI * 1_681.974_450_955_533 / rate).tan();
    let high = 10.0_f64.powf(3.999_843_853_973_347 / 20.0);
    let band = high.powf(0.499_666_774_154_541_6);
    let q = 0.707_175_236_955_419_6;
    let a0 = 1.0 + k / q + k * k;
    let shelf = BiquadCoeffs {
        b0: (high + band * k / q + k * k) / a0,
        b1: 2.0 * (k * k - high) / a0,
        b2: (high - band * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };

    let k = (PI * 38.135_470_876_024_44 / rate).tan();
    let q = 0.500_327_037_323_877_3;
    let a0 = 1.0 + k / q + k * k;
    let high_pass = BiquadCoeffs {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    [shelf, high_pass]
}

/// Loudness in LUFS of a channel-summed K-weighted mean square.
fn lufs(mean_square: f64) -> f64 {
    if mean_square > 0.0 {
        -0.691 + 10.0 * mean_square.log10()
    } else {
        f64::NEG_INFINITY
    }
}

/// The mean square a loudness corresponds to: the inverse of [`lufs`].
fn mean_square_at(loudness: f64) -> f64 {
    10.0_f64.powf((loudness + 0.691) / 10.0)
}

fn floored(value: f64) -> f32 {
    if value.is_finite() {
        (value as f32).max(LOUDNESS_FLOOR)
    } else {
        LOUDNESS_FLOOR
    }
}

/// A level in dB relative to full scale, floored at [`LOUDNESS_FLOOR`].
fn level_db(level: f32) -> f32 {
    if level > 0.0 {
        floored(20.0 * f64::from(level).log10())
    } else {
        LOUDNESS_FLOOR
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1.0e-9 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Reads one channel between its samples: the waveform at a quarter, a half
/// and three quarters of the way from a sample to the next.
///
/// Each point is a windowed-sinc sum over the twelve nearest samples
/// (Lanczos, six lobes), normalized so a constant reads as itself. The
/// reading is of the sample [`PEAK_DELAY`] pushes ago and the span up to the
/// sample after it, so a caller that needs the peak before the sample
/// leaves delays the audio by that much.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakInterpolator {
    taps: [[f32; PEAK_TAPS]; 3],
    history: [f32; PEAK_TAPS],
}

impl Default for PeakInterpolator {
    fn default() -> Self {
        let mut taps = [[0.0_f32; PEAK_TAPS]; 3];
        for (phase, row) in taps.iter_mut().enumerate() {
            let offset = (phase + 1) as f64 / 4.0;
            let mut weights = [0.0_f64; PEAK_TAPS];
            let mut sum = 0.0;
            for (index, weight) in weights.iter_mut().enumerate() {
                // `history[index]` is the sample `index - 5` away from the
                // one being read.
                let distance = offset - (index as f64 - (PEAK_DELAY as f64 - 1.0));
                *weight = sinc(distance) * sinc(distance / PEAK_DELAY as f64);
                sum += *weight;
            }
            for (tap, weight) in row.iter_mut().zip(weights) {
                *tap = (weight / sum) as f32;
            }
        }
        Self {
            taps,
            history: [0.0; PEAK_TAPS],
        }
    }
}

impl PeakInterpolator {
    /// Forgets the past without touching the tap table.
    pub fn reset(&mut self) {
        self.history = [0.0; PEAK_TAPS];
    }

    /// Takes the next sample. Returns the magnitude of the sample
    /// [`PEAK_DELAY`] pushes ago, and the largest magnitude of the waveform
    /// from that sample up to the next one: never less than the first.
    #[inline]
    pub fn tick(&mut self, input: f32) -> (f32, f32) {
        self.history.copy_within(1.., 0);
        self.history[PEAK_TAPS - 1] = input;
        let sample = self.history[PEAK_DELAY - 1].abs();
        let mut peak = sample;
        for row in &self.taps {
            let mut value = 0.0_f32;
            for (tap, past) in row.iter().zip(&self.history) {
                value += tap * past;
            }
            peak = peak.max(value.abs());
        }
        (sample, peak)
    }
}

/// What a programme measures: how loud it is, how much its loudness moves,
/// and how high it peaks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessMetrics {
    /// Gated programme loudness in LUFS (BS.1770-4), or
    /// [`LOUDNESS_FLOOR`] when nothing passed the gate.
    pub integrated_lufs: f32,
    /// Loudness range in LU (EBU Tech 3342): the spread between the quiet
    /// and the loud passages, with the extremes left out. Zero for a
    /// programme shorter than three seconds.
    pub loudness_range_lu: f32,
    /// The loudest 400 ms, in LUFS.
    pub momentary_max_lufs: f32,
    /// The loudest three seconds, in LUFS. For a programme shorter than
    /// that it is the loudness of the whole.
    pub short_term_max_lufs: f32,
    /// The peak of the waveform between and at its samples, in dBTP.
    pub true_peak_dbtp: f32,
    /// The largest sample, in dBFS.
    pub sample_peak_db: f32,
}

impl LoudnessMetrics {
    /// True when the programme has a loudness: something passed the gate.
    pub fn is_audible(&self) -> bool {
        self.integrated_lufs > LOUDNESS_FLOOR
    }

    /// Peak to loudness ratio in LU: how far the true peak stands above the
    /// programme loudness. Squashing a mix lowers it.
    pub fn peak_to_loudness_lu(&self) -> f32 {
        if self.is_audible() {
            self.true_peak_dbtp - self.integrated_lufs
        } else {
            0.0
        }
    }
}

/// Measures a stereo programme as it is fed, in pieces of any length.
///
/// This is for work away from the audio thread: it keeps one number for
/// every 100 ms it has heard, so feeding it allocates now and then.
#[derive(Debug, Clone)]
pub struct LoudnessMeter {
    coefficients: [BiquadCoeffs; 2],
    /// `[channel][section]`.
    filters: [[Biquad; 2]; 2],
    /// Frames in 100 ms.
    hop: usize,
    /// Frames of the hop being filled, and their K-weighted energy summed
    /// over both channels.
    filled: usize,
    energy: f64,
    /// The channel-summed mean square of every finished hop.
    hops: Vec<f64>,
    /// Energy and frames of everything fed, for programmes shorter than one
    /// block.
    total_energy: f64,
    frames: u64,
    peaks: [PeakInterpolator; 2],
    true_peak: f32,
    sample_peak: f32,
}

impl LoudnessMeter {
    /// A meter for audio at `sample_rate`, which is held to 8 kHz to
    /// 384 kHz.
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.clamp(8_000, 384_000);
        Self {
            coefficients: k_weighting(rate as f32),
            filters: [[Biquad::default(); 2]; 2],
            hop: (rate as usize).div_ceil(10).max(1),
            filled: 0,
            energy: 0.0,
            hops: Vec::new(),
            total_energy: 0.0,
            frames: 0,
            peaks: [PeakInterpolator::default(); 2],
            true_peak: 0.0,
            sample_peak: 0.0,
        }
    }

    /// Forgets everything heard so far.
    pub fn reset(&mut self) {
        self.filters = [[Biquad::default(); 2]; 2];
        self.filled = 0;
        self.energy = 0.0;
        self.hops.clear();
        self.total_energy = 0.0;
        self.frames = 0;
        for peak in &mut self.peaks {
            peak.reset();
        }
        self.true_peak = 0.0;
        self.sample_peak = 0.0;
    }

    /// Frames fed so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Takes more of the programme: interleaved stereo, left first. A
    /// sample that is not a number counts as silence.
    pub fn feed(&mut self, interleaved: &[f32]) {
        for frame in interleaved.chunks_exact(2) {
            let mut weighted = 0.0_f64;
            for (channel, sample) in frame.iter().enumerate() {
                let sample = if sample.is_finite() { *sample } else { 0.0 };
                self.sample_peak = self.sample_peak.max(sample.abs());
                let (_, peak) = self.peaks[channel].tick(sample);
                self.true_peak = self.true_peak.max(peak);
                let shelved =
                    self.filters[channel][0].tick(&self.coefficients[0], f64::from(sample));
                let passed = self.filters[channel][1].tick(&self.coefficients[1], shelved);
                weighted += passed * passed;
            }
            self.energy += weighted;
            self.total_energy += weighted;
            self.frames += 1;
            self.filled += 1;
            if self.filled == self.hop {
                self.hops.push(self.energy / self.hop as f64);
                self.energy = 0.0;
                self.filled = 0;
                for channel in &mut self.filters {
                    for section in channel {
                        section.flush();
                    }
                }
            }
        }
    }

    /// The mean squares of every block `length` hops long, a hop apart.
    fn blocks(&self, length: usize) -> Vec<f64> {
        if self.hops.len() < length {
            return Vec::new();
        }
        self.hops
            .windows(length)
            .map(|window| window.iter().sum::<f64>() / length as f64)
            .collect()
    }

    /// What has been fed so far measures. Feeding may go on afterwards.
    pub fn metrics(&self) -> LoudnessMetrics {
        let absolute = mean_square_at(ABSOLUTE_GATE_LUFS);
        let whole = if self.frames > 0 {
            self.total_energy / self.frames as f64
        } else {
            0.0
        };

        let mut momentary = self.blocks(MOMENTARY_HOPS);
        if momentary.is_empty() && self.frames > 0 {
            // Shorter than one block: the whole of it is the only block.
            momentary.push(whole);
        }
        let momentary_max = momentary.iter().copied().fold(0.0_f64, f64::max);
        let gated = |blocks: &[f64], floor: f64| {
            let mut sum = 0.0;
            let mut count = 0_usize;
            for &block in blocks {
                if block > floor {
                    sum += block;
                    count += 1;
                }
            }
            if count == 0 { 0.0 } else { sum / count as f64 }
        };
        // First the absolute gate, then a gate 10 LU under what passed it.
        let ungated = gated(&momentary, absolute);
        let integrated = gated(&momentary, absolute.max(ungated * 0.1));

        let short_term = self.blocks(SHORT_TERM_HOPS);
        let short_term_max = if short_term.is_empty() {
            whole
        } else {
            short_term.iter().copied().fold(0.0_f64, f64::max)
        };
        let range = {
            // A gate 20 LU under the absolute-gated mean, then the spread
            // from the 10th to the 95th percentile of what is left.
            let floor = absolute.max(gated(&short_term, absolute) * 0.01);
            let mut kept: Vec<f64> = short_term
                .iter()
                .copied()
                .filter(|block| *block > floor)
                .map(lufs)
                .collect();
            kept.sort_by(|a, b| a.total_cmp(b));
            if kept.len() < 2 {
                0.0
            } else {
                let at = |share: f64| kept[((kept.len() - 1) as f64 * share).round() as usize];
                at(0.95) - at(0.10)
            }
        };

        // The interpolators still hold the last few samples.
        let mut true_peak = self.true_peak;
        for mut tail in self.peaks {
            for _ in 0..PEAK_DELAY {
                true_peak = true_peak.max(tail.tick(0.0).1);
            }
        }

        LoudnessMetrics {
            integrated_lufs: floored(lufs(integrated)),
            loudness_range_lu: range.max(0.0) as f32,
            momentary_max_lufs: floored(lufs(momentary_max)),
            short_term_max_lufs: floored(lufs(short_term_max)),
            true_peak_dbtp: level_db(true_peak),
            sample_peak_db: level_db(self.sample_peak),
        }
    }
}

/// Measures a whole programme held in memory: interleaved stereo.
pub fn measure(interleaved: &[f32], sample_rate: u32) -> LoudnessMetrics {
    let mut meter = LoudnessMeter::new(sample_rate);
    meter.feed(interleaved);
    meter.metrics()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_48_khz_weighting_is_the_table_of_the_standard() {
        let [shelf, high_pass] = k_weighting(48_000.0);
        let close = |a: f64, b: f64| assert!((a - b).abs() < 1.0e-6, "{a} against {b}");
        close(shelf.b0, 1.535_124_859_586_97);
        close(shelf.b1, -2.691_696_189_406_38);
        close(shelf.b2, 1.198_392_810_852_85);
        close(shelf.a1, -1.690_659_293_182_41);
        close(shelf.a2, 0.732_480_774_215_85);
        close(high_pass.b0, 1.0);
        close(high_pass.b1, -2.0);
        close(high_pass.b2, 1.0);
        close(high_pass.a1, -1.990_047_454_833_98);
        close(high_pass.a2, 0.990_072_250_366_21);
    }

    #[test]
    fn the_interpolator_reads_samples_exactly_and_constants_as_themselves() {
        let mut interpolator = PeakInterpolator::default();
        let mut last = (0.0, 0.0);
        for _ in 0..40 {
            last = interpolator.tick(0.25);
        }
        assert_eq!(last.0, 0.25);
        assert!((last.1 - 0.25).abs() < 1.0e-6, "{}", last.1);
        // A lone sample comes back PEAK_DELAY pushes later, and the
        // waveform around it never reads below the sample itself.
        interpolator.reset();
        let mut seen = Vec::new();
        for index in 0..20 {
            seen.push(interpolator.tick(if index == 0 { 1.0 } else { 0.0 }));
        }
        assert_eq!(seen[PEAK_DELAY].0, 1.0);
        assert!(seen.iter().all(|(sample, peak)| peak >= sample));
        assert_eq!(
            seen.iter().filter(|(sample, _)| *sample != 0.0).count(),
            1
        );
    }
}
