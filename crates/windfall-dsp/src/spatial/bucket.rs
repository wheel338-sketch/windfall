//! A bucket-brigade delay line and the three published sweeps of the
//! two-line chorus built into a classic analog polysynth.
//!
//! The behaviour modelled here is the public description in
//! docs/MODULATION-EFFECTS.md: two 256-stage lines, one per output side,
//! swept by one triangle oscillator whose right side is inverted, with a
//! two-pole lowpass ahead of each line. A bucket-brigade line holds half
//! as many samples as it has stages and is as long as its clock makes it:
//! the delay is set by how fast the samples are handed on, not by where
//! they are read. So the line's own sample rate moves with the sweep,
//! which is what gives the circuit its sound, and what a moving read
//! position in an ordinary delay line does not do.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Samples in transit in a 256-stage line: every second stage holds one.
pub const BUCKET_SAMPLES: usize = 128;

/// Corner of the two-pole lowpass ahead of each line, in Hz. The slowest
/// clock of the published sweeps is 23.9 kHz, so this keeps what the line
/// samples below half of it.
pub const BUCKET_GUARD_HZ: f32 = 9_000.0;

/// Most clock periods one frame may hold. It bounds the work per frame at
/// very low sample rates, where it also keeps the line at least eight
/// frames long.
const MAX_TICKS: f32 = 16.0;

/// Which circuit the chorus runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BucketMode {
    /// Four free voices set by Delay, Depth, Rate and Spread. This is the
    /// circuit the effect had before the modelled sweeps were added.
    #[default]
    Free,
    /// The slow sweep.
    One,
    /// The faster sweep.
    Two,
    /// Both switches together: a fast, shallow vibrato on both sides alike.
    Both,
}

/// One of the three modelled sweeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketSweep {
    /// Speed of the triangle, in Hz.
    pub rate_hz: f32,
    /// Shortest delay of the sweep, in ms.
    pub min_ms: f32,
    /// Longest delay of the sweep, in ms.
    pub max_ms: f32,
    /// Whether the right line moves against the left. Otherwise the two
    /// move together and a mono input stays mono.
    pub stereo: bool,
}

impl BucketMode {
    /// The sweep of a modelled mode, or `None` for the free voices.
    pub fn sweep(self) -> Option<BucketSweep> {
        match self {
            BucketMode::Free => None,
            BucketMode::One => Some(BucketSweep::ONE),
            BucketMode::Two => Some(BucketSweep::TWO),
            BucketMode::Both => Some(BucketSweep::BOTH),
        }
    }
}

impl BucketSweep {
    /// Mode I: half a cycle a second, from 1.66 to 5.35 ms, in stereo.
    pub const ONE: Self = Self {
        rate_hz: 0.513,
        min_ms: 1.66,
        max_ms: 5.35,
        stereo: true,
    };
    /// Mode II: the same range, faster.
    pub const TWO: Self = Self {
        rate_hz: 0.863,
        min_ms: 1.66,
        max_ms: 5.35,
        stereo: true,
    };
    /// Both switches: nearly ten cycles a second over 0.4 ms, in mono.
    pub const BOTH: Self = Self {
        rate_hz: 9.75,
        min_ms: 3.3,
        max_ms: 3.7,
        stereo: false,
    };

    /// The delay of one side, in ms, at `phase` cycles into the sweep. The
    /// left line starts at its shortest and rises to its longest half way.
    pub fn delay_ms(&self, phase: f64, right: bool) -> f32 {
        let rise = super::common::triangle(phase);
        let position = if right && self.stereo { 1.0 - rise } else { rise };
        self.min_ms + (self.max_ms - self.min_ms) * position
    }
}

/// Clock periods per frame that make the line `delay` frames long.
pub(super) fn ticks_for(delay: f32) -> f32 {
    let shortest = BUCKET_SAMPLES as f32 / MAX_TICKS;
    BUCKET_SAMPLES as f32 / delay.max(shortest)
}

/// One bucket-brigade line. It has no heap storage.
#[derive(Clone, Copy)]
pub(super) struct Bucket {
    cells: [f32; BUCKET_SAMPLES],
    /// The cell handed on next, which holds the oldest sample.
    head: usize,
    /// How far into the current clock period the last frame ended, 0 to 1.
    clock: f32,
    /// The input of the last frame, for sampling between frames.
    last: f32,
    /// What the line's output stage holds.
    held: f32,
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            cells: [0.0; BUCKET_SAMPLES],
            head: 0,
            clock: 0.0,
            last: 0.0,
            held: 0.0,
        }
    }
}

impl Bucket {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Runs the line for one frame at `ticks` clock periods per frame and
    /// returns the average of what its output held over that frame.
    ///
    /// On every clock the line takes in the input as it was at that
    /// instant, on the straight line between the last frame and this one,
    /// and its output takes on the sample that went in
    /// [`BUCKET_SAMPLES`] clocks before.
    pub fn tick(&mut self, input: f32, ticks: f32) -> f32 {
        let ticks = if ticks.is_finite() {
            ticks.clamp(1.0e-3, MAX_TICKS)
        } else {
            1.0
        };
        let start = self.clock;
        let end = start + ticks;
        let mut area = 0.0;
        let mut at = start;
        let mut clock = 1.0_f32;
        while clock <= end {
            area += self.held * (clock - at);
            at = clock;
            let instant = (clock - start) / ticks;
            let sampled = self.last + (input - self.last) * instant;
            self.held = self.cells[self.head];
            self.cells[self.head] = super::common::bounded(sampled);
            self.head = (self.head + 1) % BUCKET_SAMPLES;
            clock += 1.0;
        }
        area += self.held * (end - at);
        // `clock - 1` clocks happened in this frame.
        self.clock = (end - (clock - 1.0)).clamp(0.0, 1.0);
        self.last = input;
        super::common::bounded(area / ticks)
    }
}
