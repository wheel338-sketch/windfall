//! A bounded monophonic tuner that observes the effect's stereo audio.
//! Detection never writes to the audio buffers. See the integration seam
//! for the analysis range, settling time and stereo downmix limits.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::effect::Effect;
use crate::param::{ParamSet, param_set};

const WINDOW: usize = 1024;
const MAX_LAG: usize = 256;
const HOP: usize = 256;
const MIN_ENERGY: f64 = 1.0e-10;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TunerParams {
    /// Frequency of MIDI note 69 in Hz, 400 to 480; default 440.
    pub reference_hz: f32,
}

impl Default for TunerParams {
    fn default() -> Self {
        Self {
            reference_hz: 440.0,
        }
    }
}

param_set!(TunerParams, "Tuner", {
    float [reference_hz] "referenceHz" "Reference" { Hertz, Linear, 400.0, 480.0, 440.0 }
});

/// Snapshot available after every block. No cross-thread publication is
/// implied: the host must copy this through its own realtime-safe seam.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TunerReadout {
    /// Estimated fundamental in Hz; zero when no reliable pitch is found.
    pub frequency_hz: f32,
    /// Nearest MIDI note, or None when no reliable pitch is found.
    pub midi_note: Option<u8>,
    /// Signed cents relative to the nearest note and current reference.
    /// Zero when no reliable pitch is found.
    pub cents_error: f32,
    /// Periodicity confidence in 0..1; this is not a calibrated probability.
    pub confidence: f32,
}

/// Unity stereo passthrough with fixed storage for monophonic analysis.
/// No device access, audio latency, heap activity or locking is required.
pub struct Tuner {
    params: TunerParams,
    readout: TunerReadout,
    history: [f32; WINDOW],
    write: usize,
    filled: usize,
    since_analysis: usize,
    decimation: usize,
    decimation_count: usize,
    decimation_sum: f64,
    analysis_rate: f64,
    /// Blocks processed, so a reader can tell a new readout from an old one.
    blocks: u64,
}

impl Default for Tuner {
    fn default() -> Self {
        Self {
            params: TunerParams::default(),
            readout: TunerReadout::default(),
            history: [0.0; WINDOW],
            write: 0,
            filled: 0,
            since_analysis: 0,
            decimation: 4,
            decimation_count: 0,
            decimation_sum: 0.0,
            analysis_rate: 12_000.0,
            blocks: 0,
        }
    }
}

impl Tuner {
    pub fn readout(&self) -> TunerReadout {
        self.readout
    }

    /// The readout together with the count of blocks processed so far. A
    /// tuner that is switched off stops counting, which is how a reader
    /// tells a readout of this block from one left over.
    pub fn observation(&self) -> (u64, TunerReadout) {
        (self.blocks, self.readout)
    }

    fn clear_analysis(&mut self) {
        // The fill count prevents old samples from being observed, so reset
        // need not clear the storage or pay a window-sized callback cost.
        self.readout = TunerReadout::default();
        self.write = 0;
        self.filled = 0;
        self.since_analysis = 0;
        self.decimation_count = 0;
        self.decimation_sum = 0.0;
    }

    fn update_note(&mut self) {
        if self.readout.frequency_hz > 0.0 {
            let note = 69.0 + 12.0 * (self.readout.frequency_hz / self.params.reference_hz).log2();
            let nearest = note.round().clamp(0.0, 127.0);
            self.readout.midi_note = Some(nearest as u8);
            self.readout.cents_error = (note - nearest) * 100.0;
        }
    }

    fn analyze(&mut self) {
        let frame: [f64; WINDOW] =
            std::array::from_fn(|i| self.history[(self.write + i) % WINDOW] as f64);
        let mean = frame.iter().sum::<f64>() / WINDOW as f64;
        let variance = frame.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / WINDOW as f64;
        self.readout = TunerReadout::default();
        if variance < MIN_ENERGY {
            return;
        }

        // YIN cumulative-mean normalized difference, using equal-length
        // comparisons for every lag. The first convincing local minimum
        // avoids reporting an integer subharmonic of a clean tone.
        let max_lag = ((self.analysis_rate / 50.0).ceil() as usize).min(MAX_LAG - 1);
        let min_lag = ((self.analysis_rate / 2000.0).floor() as usize).max(2);
        let mut difference = [0.0_f64; MAX_LAG + 1];
        let mut normalized = [1.0_f64; MAX_LAG + 1];
        let mut sum = 0.0;
        for lag in 1..=max_lag + 1 {
            let mut value = 0.0;
            for i in 0..WINDOW - MAX_LAG {
                let delta = frame[i] - frame[i + lag];
                value += delta * delta;
            }
            difference[lag] = value;
            sum += value;
            normalized[lag] = if sum > 0.0 {
                value * lag as f64 / sum
            } else {
                1.0
            };
        }
        let mut best = 1.0_f64;
        for lag in min_lag..=max_lag {
            best = best.min(normalized[lag]);
            if normalized[lag] < 0.15 && normalized[lag] <= normalized[lag + 1] {
                // Interpolate the unnormalized difference to avoid the
                // cumulative normalization's bias in fractional periods.
                let [a, b, c] = [difference[lag - 1], difference[lag], difference[lag + 1]];
                let curvature = a - 2.0 * b + c;
                let offset = if curvature > 0.0 {
                    (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
                } else {
                    0.0
                };
                let frequency = self.analysis_rate / (lag as f64 + offset);
                if !(50.0..=2000.0).contains(&frequency) {
                    return;
                }
                self.readout.frequency_hz = frequency as f32;
                self.readout.confidence = (1.0 - normalized[lag]).clamp(0.0, 1.0) as f32;
                self.update_note();
                return;
            }
        }
        // Unvoiced input can expose weak periodicity without inventing a
        // note. Only the threshold above can publish frequency/note fields.
        self.readout.confidence = (1.0 - best).clamp(0.0, 0.85) as f32;
    }
}

impl Effect for Tuner {
    type Params = TunerParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        let rate = crate::blocks::math::clean(sample_rate, 8000.0, 384_000.0, 48_000.0);
        self.decimation = (rate as f64 / 12_000.0).ceil() as usize;
        self.analysis_rate = rate as f64 / self.decimation as f64;
        self.clear_analysis();
    }

    fn reset(&mut self) {
        self.clear_analysis();
    }

    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        // This changes only the display's reference, never the audio.
        self.update_note();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.blocks = self.blocks.wrapping_add(1);
        let mut energy = 0.0;
        let mut count = 0;
        let mut invalid = false;
        for (&l, &r) in left.iter().zip(right.iter()) {
            let sample = if l.is_finite() && r.is_finite() {
                0.5 * (l as f64 + r as f64)
            } else {
                invalid = true;
                0.0
            };
            energy += sample * sample;
            count += 1;
            self.decimation_sum += sample;
            self.decimation_count += 1;
            if self.decimation_count == self.decimation {
                self.history[self.write] = (self.decimation_sum / self.decimation as f64) as f32;
                self.write = (self.write + 1) % WINDOW;
                self.filled = (self.filled + 1).min(WINDOW);
                self.since_analysis += 1;
                self.decimation_count = 0;
                self.decimation_sum = 0.0;
                if self.filled == WINDOW && self.since_analysis >= HOP {
                    self.analyze();
                    self.since_analysis = 0;
                }
            }
        }
        if count == 0 || invalid || energy / (count as f64) < MIN_ENERGY {
            self.clear_analysis();
        }
    }
}

#[cfg(test)]
mod tests;
