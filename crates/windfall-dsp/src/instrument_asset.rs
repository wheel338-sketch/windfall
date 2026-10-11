//! Data an instrument plays that is too large for its parameters.
//!
//! A parameter struct is `Copy` and crosses to the audio thread by value, so
//! it is no place for a picture or a recording. An [`InstrumentAsset`] is
//! prepared away from the audio thread, never changes afterwards, and is
//! handed to an instrument behind an [`Arc`]. The instrument only reads it.
//!
//! # Who frees it
//!
//! Swapping one asset for another on the audio thread is a pointer swap. The
//! asset that was swapped out must still be owned somewhere else when that
//! happens, or dropping it would free memory in the callback. The engine
//! arranges this: the project and the plan that supplied an asset both hold
//! it for at least as long as the instrument does, and a plan is retired on
//! the control side.

use std::sync::Arc;

use crate::drums::DrumVoiceParams;
use crate::{InstrumentKind, ParamSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DrumKeyPatch {
    pub key: u8,
    pub params: DrumVoiceParams,
}

/// One prepared oscillator contribution, never stored in Copy parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpectralPartial {
    pub ratio: f32,
    pub level: f32,
    pub phase: f32,
    pub pan: f32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpectralFrame {
    pub partials: Vec<SpectralPartial>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpectralBank {
    pub frames: Vec<SpectralFrame>,
    pub frame_seconds: f32,
    pub root_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preset_number: Option<u32>,
}
impl SpectralBank {
    pub fn check(&self) -> Result<(), &'static str> {
        if self.frames.is_empty() || self.frames.len() > 256 {
            return Err("a spectral bank holds 1 to 256 frames");
        }
        if !(0.001..=10.0).contains(&self.frame_seconds) || !(8.0..=20000.0).contains(&self.root_hz)
        {
            return Err("invalid spectral timing or root pitch");
        }
        let count = self.frames[0].partials.len();
        if count == 0 || count > 64 {
            return Err("a spectral frame holds 1 to 64 partials");
        }
        for frame in &self.frames {
            if frame.partials.len() != count {
                return Err("spectral frames must have equal partial counts");
            }
            for partial in &frame.partials {
                if !(0.001..=10000.0).contains(&partial.ratio)
                    || !(0.0..=1.0).contains(&partial.level)
                    || !(0.0..=1.0).contains(&partial.phase)
                    || !(-1.0..=1.0).contains(&partial.pan)
                {
                    return Err("invalid spectral partial");
                }
            }
        }
        Ok(())
    }
    /// Full-width number selection, prepared only on the control side.
    pub fn numbered(number: u32) -> Self {
        let mut state = number.wrapping_add(0x9e3779b9);
        let mut draw = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            f64::from(state) / f64::from(u32::MAX)
        };
        let partials = (0..64)
            .map(|i| {
                let n = (i + 1) as f64;
                SpectralPartial {
                    ratio: (n * (1.0 + 0.35 * (draw() * 2.0 - 1.0))) as f32,
                    level: ((0.2 + 0.8 * draw()) / n.powf(1.2)) as f32,
                    phase: draw() as f32,
                    pan: 0.0,
                }
            })
            .collect();
        Self {
            frames: vec![SpectralFrame { partials }],
            frame_seconds: 0.1,
            root_hz: 440.0,
            preset_number: Some(number),
        }
    }
    pub fn metallic() -> Self {
        Self {
            frames: vec![SpectralFrame {
                partials: (0..32)
                    .map(|i| {
                        let n = (i + 1) as f32;
                        SpectralPartial {
                            ratio: if i == 0 { 1.0 } else { n * n * 0.69 },
                            level: 1.0 / n.powf(1.4),
                            phase: 0.0,
                            pan: 0.0,
                        }
                    })
                    .collect(),
            }],
            frame_seconds: 0.1,
            root_hz: 440.0,
            preset_number: None,
        }
    }
    pub fn harmonics() -> Self {
        Self {
            frames: vec![SpectralFrame {
                partials: (0..64)
                    .map(|i| SpectralPartial {
                        ratio: (i + 1) as f32,
                        level: 1.0 / (i + 1) as f32,
                        phase: 0.0,
                        pan: 0.0,
                    })
                    .collect(),
            }],
            frame_seconds: 0.1,
            root_hz: 440.0,
            preset_number: None,
        }
    }
    pub fn harmonic_snapshots() -> Self {
        Self {
            frames: vec![
                SpectralFrame {
                    partials: (0..32)
                        .map(|i| SpectralPartial {
                            ratio: (i + 1) as f32,
                            level: if i == 0 { 1.0 } else { 0.0 },
                            phase: 0.0,
                            pan: 0.0,
                        })
                        .collect(),
                },
                SpectralFrame {
                    partials: (0..32)
                        .map(|i| SpectralPartial {
                            ratio: (i + 1) as f32,
                            level: if i % 2 == 0 {
                                1.0 / (i + 1) as f32
                            } else {
                                0.0
                            },
                            phase: 0.0,
                            pan: 0.0,
                        })
                        .collect(),
                },
            ],
            frame_seconds: 0.1,
            root_hz: 440.0,
            preset_number: None,
        }
    }
}

/// Most steps in time a [`SpectralGrid`] can hold.
pub const MAX_GRID_COLUMNS: usize = 256;

/// Most rows a [`SpectralGrid`] can hold: the width of an additive bank.
pub const MAX_GRID_ROWS: usize = 64;

/// Shortest and longest time one grid column may stand for, in seconds.
pub const MIN_COLUMN_SECONDS: f32 = 0.001;
pub const MAX_COLUMN_SECONDS: f32 = 10.0;

/// Most frames a [`LayerPcm`] can hold. At 48 kHz that is about two thirds
/// of a second, which is a drum hit and not a loop.
pub const MAX_LAYER_FRAMES: usize = 32_768;

/// Lowest and highest rate a [`LayerPcm`] may have been recorded at.
pub const MIN_LAYER_RATE: u32 = 8_000;
pub const MAX_LAYER_RATE: u32 = 192_000;

/// An asset as an instrument holds it.
pub type SharedInstrumentAsset = Arc<InstrumentAsset>;

/// Immutable data for one instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum InstrumentAsset {
    /// Levels over time and frequency: a picture, or a recording taken
    /// apart into partials.
    Grid(SpectralGrid),
    /// A short mono recording, layered under a synthesized drum.
    Pcm(LayerPcm),
    Spectra(SpectralBank),
    DrumKeys {
        patches: Vec<DrumKeyPatch>,
    },
}

impl InstrumentAsset {
    pub fn validate(&self, kind: InstrumentKind) -> Result<(), &'static str> {
        self.check()?;
        match (self, kind) {
            (Self::DrumKeys { .. }, InstrumentKind::DrumVoice) => Ok(()),
            (Self::Pcm(_), InstrumentKind::LayerKick) => Ok(()),
            (Self::Spectra(_), InstrumentKind::HarmonicFilter) => Ok(()),
            (Self::Spectra(_), InstrumentKind::MetalVoice) => Ok(()),
            (Self::Spectra(bank), InstrumentKind::NumberVoice) if bank.preset_number.is_some() => {
                Ok(())
            }
            (Self::Spectra(bank), InstrumentKind::SnapshotMorph) if bank.frames.len() == 2 => {
                Ok(())
            }
            _ => Err("this instrument cannot play that asset type"),
        }
    }

    /// Says what is wrong with the asset, if anything.
    pub fn check(&self) -> Result<(), &'static str> {
        match self {
            InstrumentAsset::Grid(grid) => grid.check(),
            InstrumentAsset::Pcm(pcm) => pcm.check(),
            InstrumentAsset::Spectra(bank) => bank.check(),
            InstrumentAsset::DrumKeys { patches } => {
                if patches.len() > 128 {
                    return Err("a drum bank holds at most 128 keys");
                }
                let mut previous = None;
                for patch in patches {
                    if patch.key > 127 || previous.is_some_and(|key| key >= patch.key) {
                        return Err("drum keys must be unique and sorted from 0 to 127");
                    }
                    if patch.params != patch.params.sanitized() {
                        return Err("a drum key patch has invalid settings");
                    }
                    previous = Some(patch.key);
                }
                Ok(())
            }
        }
    }

    /// The grid, if this is one.
    pub fn grid(&self) -> Option<&SpectralGrid> {
        match self {
            InstrumentAsset::Grid(grid) => Some(grid),
            _ => None,
        }
    }

    /// The recording, if this is one.
    pub fn pcm(&self) -> Option<&LayerPcm> {
        match self {
            InstrumentAsset::Pcm(pcm) => Some(pcm),
            _ => None,
        }
    }
}

/// A grid of levels: columns run in time, rows from the lowest to the
/// highest.
///
/// What a row means is up to the instrument that plays the grid. An additive
/// resynthesizer reads row `r` as partial `r + 1` of the note; an image
/// scanner reads the rows as frequencies spread over a range.
///
/// A level is stored in eight bits and stands for the amplitude
/// `(level / 255)^2`, so half of the range is spent on everything below
/// -12 dB and the quietest level that is not silence is about -96 dB.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpectralGrid {
    /// Steps in time, 1 to [`MAX_GRID_COLUMNS`].
    pub columns: u16,
    /// Rows, lowest first, 1 to [`MAX_GRID_ROWS`].
    pub rows: u8,
    /// Seconds one column lasts when it is played at its own speed,
    /// [`MIN_COLUMN_SECONDS`] to [`MAX_COLUMN_SECONDS`].
    pub column_seconds: f32,
    /// The pitch row 0 had in the recording the grid was taken from, in Hz.
    /// Zero for a grid that did not come from a recording.
    #[serde(default)]
    pub root_hz: f32,
    /// One level per cell, column after column: the cell at `column` and
    /// `row` is at `column * rows + row`.
    pub levels: Vec<u8>,
    /// Where each cell sits between left (0) and right (255), in the same
    /// order as `levels`. Empty when every cell is in the middle.
    #[serde(default)]
    pub pans: Vec<u8>,
}

impl SpectralGrid {
    /// The amplitude a stored level stands for.
    #[inline]
    pub fn amplitude(level: u8) -> f32 {
        let share = f32::from(level) / 255.0;
        share * share
    }

    /// The stored level nearest to `amplitude`. Anything above 1 is stored
    /// as 1, and anything that is not a positive number as silence.
    pub fn level_for(amplitude: f32) -> u8 {
        if amplitude.is_nan() || amplitude <= 0.0 {
            return 0;
        }
        (255.0 * amplitude.min(1.0).sqrt()).round() as u8
    }

    /// The stored pan nearest to `pan`, from -1 (left) to 1 (right).
    pub fn pan_for(pan: f32) -> u8 {
        if !pan.is_finite() {
            return 128;
        }
        (127.5 + 127.5 * pan.clamp(-1.0, 1.0)).round() as u8
    }

    /// A silent grid of the given size, to be filled in.
    pub fn silent(columns: usize, rows: usize, column_seconds: f32) -> Self {
        let columns = columns.clamp(1, MAX_GRID_COLUMNS);
        let rows = rows.clamp(1, MAX_GRID_ROWS);
        Self {
            columns: columns as u16,
            rows: rows as u8,
            column_seconds: if column_seconds.is_finite() {
                column_seconds.clamp(MIN_COLUMN_SECONDS, MAX_COLUMN_SECONDS)
            } else {
                0.05
            },
            root_hz: 0.0,
            levels: vec![0; columns * rows],
            pans: Vec::new(),
        }
    }

    /// Sets the amplitude of one cell. Cells outside the grid are ignored.
    pub fn set_amplitude(&mut self, column: usize, row: usize, amplitude: f32) {
        let rows = usize::from(self.rows);
        if column < usize::from(self.columns) && row < rows {
            self.levels[column * rows + row] = Self::level_for(amplitude);
        }
    }

    /// Sets where one cell sits, from -1 to 1. Gives the grid a pan for
    /// every cell if it had none.
    pub fn set_pan(&mut self, column: usize, row: usize, pan: f32) {
        let rows = usize::from(self.rows);
        if column >= usize::from(self.columns) || row >= rows {
            return;
        }
        if self.pans.len() != self.levels.len() {
            self.pans = vec![128; self.levels.len()];
        }
        self.pans[column * rows + row] = Self::pan_for(pan);
    }

    /// The amplitude of one cell. A cell outside the grid is silent, so a
    /// reader never has to trust the counts it was given.
    #[inline]
    pub fn cell(&self, column: usize, row: usize) -> f32 {
        let rows = usize::from(self.rows);
        if row >= rows {
            return 0.0;
        }
        self.levels
            .get(column * rows + row)
            .map_or(0.0, |level| Self::amplitude(*level))
    }

    /// Where one cell sits, from -1 (left) to 1 (right). The middle for a
    /// grid without pans and for a cell outside it.
    #[inline]
    pub fn cell_pan(&self, column: usize, row: usize) -> f32 {
        let rows = usize::from(self.rows);
        if row >= rows {
            return 0.0;
        }
        self.pans
            .get(column * rows + row)
            .map_or(0.0, |pan| (f32::from(*pan) - 127.5) / 127.5)
    }

    /// Seconds the whole grid lasts at its own speed.
    pub fn seconds(&self) -> f32 {
        f32::from(self.columns) * self.column_seconds
    }

    fn check(&self) -> Result<(), &'static str> {
        let columns = usize::from(self.columns);
        let rows = usize::from(self.rows);
        if columns == 0 || columns > MAX_GRID_COLUMNS {
            return Err("a spectral grid has 1 to 256 columns");
        }
        if rows == 0 || rows > MAX_GRID_ROWS {
            return Err("a spectral grid has 1 to 64 rows");
        }
        if !(MIN_COLUMN_SECONDS..=MAX_COLUMN_SECONDS).contains(&self.column_seconds) {
            return Err("a spectral grid column lasts 0.001 to 10 seconds");
        }
        if !self.root_hz.is_finite() || !(0.0..=20_000.0).contains(&self.root_hz) {
            return Err("a spectral grid root is 0 to 20000 Hz");
        }
        if self.levels.len() != columns * rows {
            return Err("a spectral grid needs one level per cell");
        }
        if !self.pans.is_empty() && self.pans.len() != self.levels.len() {
            return Err("a spectral grid needs one pan per cell, or none");
        }
        Ok(())
    }
}

/// A short mono recording at its own sample rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LayerPcm {
    /// Frames per second of `samples`, [`MIN_LAYER_RATE`] to
    /// [`MAX_LAYER_RATE`].
    pub sample_rate: u32,
    /// Up to [`MAX_LAYER_FRAMES`] frames, where 32767 is full scale.
    pub samples: Vec<i16>,
}

impl LayerPcm {
    /// Stores `samples`, which run from -1 to 1, keeping at most
    /// [`MAX_LAYER_FRAMES`] of them. The last 64 kept frames fade out when
    /// the recording was longer, so cutting it does not click.
    pub fn from_samples(sample_rate: u32, samples: &[f32]) -> Self {
        let kept = samples.len().min(MAX_LAYER_FRAMES);
        let cut = samples.len() > kept;
        let fade = 64.min(kept);
        let stored = samples[..kept]
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let gain = if cut && index + fade >= kept {
                    (kept - index) as f32 / fade as f32
                } else {
                    1.0
                };
                let sample = if sample.is_finite() { *sample } else { 0.0 };
                (sample.clamp(-1.0, 1.0) * gain * 32_767.0).round() as i16
            })
            .collect();
        Self {
            sample_rate: sample_rate.clamp(MIN_LAYER_RATE, MAX_LAYER_RATE),
            samples: stored,
        }
    }

    /// The frame at `index` as a number from -1 to 1. Silence past the end.
    #[inline]
    pub fn frame(&self, index: usize) -> f32 {
        self.samples
            .get(index)
            .map_or(0.0, |sample| f32::from(*sample) / 32_767.0)
    }

    /// The recording at a position between two frames, joined by a
    /// straight line.
    #[inline]
    pub fn at(&self, position: f64) -> f32 {
        if position.is_nan() || position < 0.0 {
            return 0.0;
        }
        let index = position as usize;
        let fraction = (position - index as f64) as f32;
        let first = self.frame(index);
        first + (self.frame(index + 1) - first) * fraction
    }

    fn check(&self) -> Result<(), &'static str> {
        if !(MIN_LAYER_RATE..=MAX_LAYER_RATE).contains(&self.sample_rate) {
            return Err("a sample layer is recorded at 8000 to 192000 Hz");
        }
        if self.samples.is_empty() {
            return Err("a sample layer needs at least one frame");
        }
        if self.samples.len() > MAX_LAYER_FRAMES {
            return Err("a sample layer holds at most 32768 frames");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_levels_round_trip_within_their_step() {
        for step in 0..=1000 {
            let amplitude = step as f32 / 1000.0;
            let stored = SpectralGrid::amplitude(SpectralGrid::level_for(amplitude));
            // One step of the eight-bit square law near this amplitude.
            let tolerance = 2.0 * amplitude.sqrt() / 255.0 + 1.0 / (255.0 * 255.0);
            assert!(
                (stored - amplitude).abs() <= tolerance,
                "{amplitude} came back as {stored}"
            );
        }
        assert_eq!(SpectralGrid::level_for(f32::NAN), 0);
        assert_eq!(SpectralGrid::level_for(-1.0), 0);
        assert_eq!(SpectralGrid::level_for(9.0), 255);
        assert_eq!(SpectralGrid::amplitude(255), 1.0);
        assert_eq!(SpectralGrid::amplitude(0), 0.0);
    }

    #[test]
    fn asset_grid_cells_are_bounded_and_checked() {
        let mut grid = SpectralGrid::silent(4, 3, 0.1);
        grid.set_amplitude(1, 2, 0.25);
        grid.set_amplitude(9, 0, 1.0);
        grid.set_amplitude(0, 9, 1.0);
        assert!((grid.cell(1, 2) - 0.25).abs() < 0.01);
        assert_eq!(grid.cell(9, 0), 0.0);
        assert_eq!(grid.cell(0, 9), 0.0);
        assert_eq!(grid.cell_pan(1, 2), 0.0);
        grid.set_pan(1, 2, -1.0);
        assert!((grid.cell_pan(1, 2) + 1.0).abs() < 1.0e-6);
        assert!(grid.cell_pan(0, 0).abs() < 0.01);
        assert!((grid.seconds() - 0.4).abs() < 1.0e-6);
        let asset = InstrumentAsset::Grid(grid.clone());
        assert_eq!(asset.check(), Ok(()));
        assert!(asset.grid().is_some() && asset.pcm().is_none());

        let mut short = grid.clone();
        short.levels.pop();
        assert!(InstrumentAsset::Grid(short).check().is_err());
        let mut uneven = grid.clone();
        uneven.pans.pop();
        assert!(InstrumentAsset::Grid(uneven).check().is_err());
        let mut slow = grid.clone();
        slow.column_seconds = f32::NAN;
        assert!(InstrumentAsset::Grid(slow).check().is_err());
        let mut empty = grid;
        empty.columns = 0;
        assert!(InstrumentAsset::Grid(empty).check().is_err());
    }

    #[test]
    fn asset_pcm_is_bounded_faded_and_checked() {
        let long = vec![1.0_f32; MAX_LAYER_FRAMES + 10];
        let pcm = LayerPcm::from_samples(1, &long);
        assert_eq!(pcm.sample_rate, MIN_LAYER_RATE);
        assert_eq!(pcm.samples.len(), MAX_LAYER_FRAMES);
        assert_eq!(pcm.frame(0), 1.0);
        assert!(pcm.frame(MAX_LAYER_FRAMES - 1) < 0.05);
        assert_eq!(pcm.frame(MAX_LAYER_FRAMES), 0.0);
        assert_eq!(InstrumentAsset::Pcm(pcm).check(), Ok(()));

        let short = LayerPcm::from_samples(44_100, &[0.0, 0.5, f32::NAN, -2.0]);
        assert_eq!(short.samples, [0, 16_384, 0, -32_767]);
        assert!((short.at(0.5) - 0.25).abs() < 1.0e-3);
        assert_eq!(short.at(-1.0), 0.0);
        assert_eq!(short.at(f64::NAN), 0.0);
        assert!(InstrumentAsset::Pcm(LayerPcm {
            sample_rate: 44_100,
            samples: Vec::new()
        })
        .check()
        .is_err());
    }

    #[test]
    fn asset_json_is_tagged_and_camel_cased() {
        let mut grid = SpectralGrid::silent(2, 2, 0.0625);
        grid.root_hz = 220.0;
        grid.set_amplitude(0, 0, 1.0);
        let asset = InstrumentAsset::Grid(grid);
        let json = serde_json::to_value(&asset).unwrap();
        assert_eq!(json["type"], "grid");
        assert_eq!(json["columnSeconds"], 0.0625);
        assert_eq!(json["rootHz"], 220.0);
        assert_eq!(json["levels"][0], 255);
        let back: InstrumentAsset = serde_json::from_value(json).unwrap();
        assert_eq!(back, asset);
        let bare: InstrumentAsset = serde_json::from_value(serde_json::json!({
            "type": "grid", "columns": 1, "rows": 1, "columnSeconds": 0.1, "levels": [9]
        }))
        .unwrap();
        assert_eq!(bare.grid().unwrap().pans, Vec::<u8>::new());
        let pcm = InstrumentAsset::Pcm(LayerPcm::from_samples(22_050, &[0.25]));
        let json = serde_json::to_value(&pcm).unwrap();
        assert_eq!(json["type"], "pcm");
        assert_eq!(json["sampleRate"], 22_050);
    }
}
