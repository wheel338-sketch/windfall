//! Mastering an export: what is asked for, and what is reported back for
//! review. The work is done locally by `windfall_dsp::mastering`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Brings the exported mix to a loudness target under a true-peak ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MasteringOptions {
    /// Programme loudness to arrive at, in LUFS (ITU-R BS.1770-4), from
    /// -30 to -6. -14 suits streaming services.
    pub target_lufs: f32,
    /// The true peak nothing gets past, in dBTP, from -9 to 0.
    pub ceiling_dbtp: f32,
    /// How firmly three band compressors hold the mix together, 0 to 1.
    /// At 0 only gain and limiting are applied.
    pub dynamics: f32,
}

/// What a mix measures.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoudnessReading {
    /// Gated programme loudness in LUFS. -120 stands for silence.
    pub integrated_lufs: f32,
    /// Loudness range in LU: the spread between quiet and loud passages.
    pub loudness_range_lu: f32,
    /// The loudest 400 ms, in LUFS.
    pub momentary_max_lufs: f32,
    /// The loudest three seconds, in LUFS.
    pub short_term_max_lufs: f32,
    /// The peak of the waveform at and between its samples, in dBTP.
    pub true_peak_dbtp: f32,
    /// The largest sample, in dBFS.
    pub sample_peak_db: f32,
}

/// What mastering did to an export, for review: the mix as it was rendered
/// and the master as it was written, measured the same way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MasteringReview {
    pub before: LoudnessReading,
    pub after: LoudnessReading,
    /// The target and ceiling the master was made to, held to their ranges.
    pub target_lufs: f32,
    pub ceiling_dbtp: f32,
    /// Gain applied ahead of the limiter, in dB.
    pub gain_db: f32,
    /// The deepest reduction of any band compressor, in dB.
    pub compression_db: f32,
    /// The deepest reduction of the limiter, in dB.
    pub limiting_db: f32,
    /// Passes over the mix: the trial passes and the one that was written.
    pub passes: u32,
    /// The master is on its loudness target and under its ceiling. False
    /// when both cannot be had together; the file is then the nearest the
    /// passes came, and never over the ceiling's samples.
    pub target_reached: bool,
}
