//! The tempo map: how long the song takes to get to each of its ticks when
//! automation moves the tempo along the way.
//!
//! The sequencer's clock is a straight line from ticks to frames, which is
//! what lets it place every note on its exact frame from one anchor. A
//! tempo that changes would bend that line. The map takes the bend out
//! instead: it gives every tick of the song the tick at which the same
//! moment would come if the tempo never left the project's stored one. The
//! sequencer keeps time in those *warped* ticks, on its straight clock, and
//! asks the map whenever it goes from a place in the song to a time or
//! back.
//!
//! The warped tick of a place is the time it takes to get there, counted
//! in ticks of the stored tempo: the sum, over every tick on the way, of
//! the stored tempo over the tempo at that tick. The map is a row of
//! segments in each of which the tempo moves in a straight line, where
//! that sum has a closed form, so a position is exact to the precision of
//! the arithmetic and never drifts, however the output is cut into
//! buffers. A stretch of an automation curve that is bent is followed with
//! straight pieces a sixteenth of a beat long.

use crate::automation::Lane;

/// Length of the straight pieces a bent stretch of the curve is followed
/// with, in ticks: a sixteenth of a beat.
const GRAIN_TICKS: f64 = 60.0;

/// Control-side conversion using precisely the same compiled tempo lane as
/// playback. Meter changes affect beat labels, never seconds per quarter.
#[derive(Clone)]
pub struct SongClock {
    base: f64,
    map: Option<TempoMap>,
}
impl SongClock {
    pub fn new(project: &windfall_project::Project) -> Result<Self, String> {
        project.check()?;
        let plan = crate::plan::compile(project, &crate::SamplePool::new());
        Ok(Self {
            base: plan.tempo_bpm,
            map: plan.tempo_map,
        })
    }
    pub fn seconds_at(&self, tick: f64) -> Result<f64, String> {
        if !tick.is_finite() || tick < 0.0 || tick > f64::from(windfall_project::MAX_SONG_TICKS) {
            return Err("Tick falls outside the song.".into());
        }
        let warped = self.map.as_ref().map_or(tick, |m| m.warp(tick));
        Ok(warped * 60.0 / (self.base * f64::from(windfall_core::PPQ)))
    }
    pub fn tick_after(&self, start: u32, seconds: f64) -> Result<u32, String> {
        Ok(self.tick_after_precise(start, seconds)?.round() as u32)
    }
    /// Fractional inverse for clip boundaries; callers choose their rounding.
    pub fn tick_after_precise(&self, start: u32, seconds: f64) -> Result<f64, String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Expected nonnegative finite seconds.".into());
        }
        let time = self.seconds_at(f64::from(start))? + seconds;
        let warped = time * self.base * f64::from(windfall_core::PPQ) / 60.0;
        let tick = self.map.as_ref().map_or(warped, |m| m.unwarp(warped));
        if !tick.is_finite() || tick > f64::from(windfall_project::MAX_SONG_TICKS) {
            return Err("Audio note exceeds the song; no ticks were saturated.".into());
        }
        Ok(tick)
    }
}

/// The tempo along the song, and the time it takes to get to each tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TempoMap {
    /// The project's stored tempo, which the warped ticks are counted in.
    base: f64,
    /// Sorted by `tick`, starting at 0. Each lasts until the next begins,
    /// and the last one for good, at a steady tempo.
    segments: Vec<Segment>,
}

/// A stretch of the song in which the tempo moves in a straight line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    /// Song tick the segment begins on.
    tick: f64,
    /// Warped tick it begins on.
    warped: f64,
    /// Length in ticks. Infinite for the last.
    length: f64,
    /// Tempo at its start and at its end, in beats per minute.
    from: f64,
    to: f64,
}

impl Segment {
    /// How far the tempo moves for every tick, as a share of where it
    /// started: 0 for a steady tempo.
    fn slope(&self) -> f64 {
        if self.length.is_finite() && self.length > 0.0 {
            (self.to - self.from) / (self.from * self.length)
        } else {
            0.0
        }
    }

    /// Warped ticks from the segment's start to `ticks` ticks into it:
    /// the stored tempo over the tempo, summed over those ticks.
    fn warp(&self, ticks: f64, base: f64) -> f64 {
        // With the tempo at `from * (1 + slope * x)`, the sum is
        // `base / from * ln(1 + slope * ticks) / slope`.
        let bend = self.slope() * ticks;
        let stretch = if bend.abs() < 1e-9 {
            1.0 - bend / 2.0
        } else {
            bend.ln_1p() / bend
        };
        ticks * (base / self.from) * stretch
    }

    /// The inverse of [`warp`](Self::warp): ticks into the segment after
    /// `warped` warped ticks of it.
    fn unwarp(&self, warped: f64, base: f64) -> f64 {
        let plain = warped * self.from / base;
        let bend = self.slope() * plain;
        let stretch = if bend.abs() < 1e-9 {
            1.0 + bend / 2.0
        } else {
            bend.exp_m1() / bend
        };
        plain * stretch
    }

    /// The tempo `ticks` ticks into the segment.
    fn tempo(&self, ticks: f64) -> f64 {
        if self.length.is_finite() && self.length > 0.0 {
            self.from + (self.to - self.from) * (ticks / self.length).clamp(0.0, 1.0)
        } else {
            self.from
        }
    }
}

impl TempoMap {
    /// The map for a song whose tempo follows `lane`, the lane of the
    /// tempo, and is `base` wherever the lane has nothing to say. `end` is
    /// the last tick of the song; from there on the tempo stays what it is
    /// there.
    pub fn new(lane: &Lane, base: f64, end: f64) -> Self {
        let tempo = |tick: f64| lane.real_at(tick).map_or(base, f64::from);
        let corners = lane.corners(end.max(0.0), GRAIN_TICKS);
        let mut segments = Vec::with_capacity(corners.len());
        let mut warped = 0.0;
        for pair in corners.windows(2) {
            let (start, stop) = (pair[0], pair[1]);
            let length = stop - start;
            // The tempo the segment sets out with and the tempo it arrives
            // at, so that a jump at a corner stays a jump between two
            // segments and is not smeared into either.
            let segment = Segment {
                tick: start,
                warped,
                length,
                from: tempo(start),
                to: lane.real_before(stop).map_or(base, f64::from),
            };
            warped += segment.warp(length, base);
            segments.push(segment);
        }
        let last = tempo(end.max(0.0));
        segments.push(Segment {
            tick: end.max(0.0),
            warped,
            length: f64::INFINITY,
            from: last,
            to: last,
        });
        Self { base, segments }
    }

    /// The warped tick of a tick of the song. A tick before the start of
    /// the song is itself.
    pub fn warp(&self, tick: f64) -> f64 {
        if tick <= 0.0 {
            return tick;
        }
        let after = self
            .segments
            .partition_point(|segment| segment.tick <= tick);
        let segment = &self.segments[after.saturating_sub(1)];
        segment.warped + segment.warp(tick - segment.tick, self.base)
    }

    /// The tick of the song that has this warped tick: the inverse of
    /// [`warp`](Self::warp).
    pub fn unwarp(&self, warped: f64) -> f64 {
        if warped <= 0.0 {
            return warped;
        }
        let after = self
            .segments
            .partition_point(|segment| segment.warped <= warped);
        let segment = &self.segments[after.saturating_sub(1)];
        segment.tick + segment.unwarp(warped - segment.warped, self.base)
    }

    /// The tempo at a tick of the song, in beats per minute.
    pub fn tempo_at(&self, tick: f64) -> f64 {
        let tick = tick.max(0.0);
        let after = self
            .segments
            .partition_point(|segment| segment.tick <= tick);
        let segment = &self.segments[after.saturating_sub(1)];
        segment.tempo(tick - segment.tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map made of segments given as their length and the tempo at each
    /// end, at a stored tempo of 120.
    fn map(parts: &[(f64, f64, f64)]) -> TempoMap {
        let base = 120.0;
        let mut segments = Vec::new();
        let (mut tick, mut warped) = (0.0, 0.0);
        for &(length, from, to) in parts {
            let segment = Segment {
                tick,
                warped,
                length,
                from,
                to,
            };
            warped += segment.warp(length, base);
            tick += length;
            segments.push(segment);
        }
        let last = parts.last().map_or(base, |part| part.2);
        segments.push(Segment {
            tick,
            warped,
            length: f64::INFINITY,
            from: last,
            to: last,
        });
        TempoMap { base, segments }
    }

    #[test]
    fn a_steady_tempo_stretches_time_by_its_ratio_to_the_stored_one() {
        // A bar at the stored tempo, then a bar at half of it, which takes
        // twice as long.
        let map = map(&[(3_840.0, 120.0, 120.0), (3_840.0, 60.0, 60.0)]);
        assert_eq!(map.warp(0.0), 0.0);
        assert_eq!(map.warp(1_000.0), 1_000.0);
        assert_eq!(map.warp(3_840.0), 3_840.0);
        assert_eq!(map.warp(3_840.0 + 100.0), 3_840.0 + 200.0);
        assert_eq!(map.warp(7_680.0), 3_840.0 * 3.0);
        // Past the end the last tempo carries on.
        assert_eq!(map.warp(7_680.0 + 50.0), 3_840.0 * 3.0 + 100.0);
        assert_eq!(map.unwarp(3_840.0 * 3.0 + 100.0), 7_680.0 + 50.0);
        assert_eq!(map.warp(-7.0), -7.0);
        assert_eq!(map.tempo_at(100.0), 120.0);
        assert_eq!(map.tempo_at(5_000.0), 60.0);
        assert_eq!(map.tempo_at(1e9), 60.0);
    }

    #[test]
    fn a_ramp_takes_the_time_its_logarithm_says() {
        // From 60 to 180 bpm over four bars. The time is the sum of 1 over
        // the tempo: 15360 ticks * 120 * ln(3) / (180 - 60), in ticks of
        // the stored 120.
        let map = map(&[(15_360.0, 60.0, 180.0)]);
        let expected = 15_360.0 * 3.0_f64.ln();
        assert!((map.warp(15_360.0) - expected).abs() < 1e-9);
        assert_eq!(map.tempo_at(7_680.0), 120.0);

        // Against the sum taken one small step at a time.
        let steps = 1_000_000;
        let step = 15_360.0 / f64::from(steps);
        let mut sum = 0.0;
        let mut checked = 0;
        for index in 0..steps {
            let tick = (f64::from(index) + 0.5) * step;
            sum += step * 120.0 / (60.0 + 120.0 * tick / 15_360.0);
            if (index + 1) % 100_000 == 0 {
                let at = f64::from(index + 1) * step;
                assert!((map.warp(at) - sum).abs() < 1e-6, "tick {at}");
                checked += 1;
            }
        }
        assert_eq!(checked, 10);
    }

    #[test]
    fn going_there_and_back_gives_the_same_tick() {
        let map = map(&[
            (960.0, 120.0, 120.0),
            (1_920.0, 120.0, 522.0),
            (480.0, 522.0, 10.0),
            (5_000.0, 10.0, 10.000_000_1),
            (3_000.0, 87.3, 87.3),
        ]);
        let mut last = -1.0;
        for index in 0..=2_400 {
            let tick = f64::from(index) * 5.0;
            let warped = map.warp(tick);
            assert!(warped > last, "tick {tick}");
            last = warped;
            let back = map.unwarp(warped);
            assert!(
                (back - tick).abs() < 1e-7,
                "tick {tick} came back as {back}"
            );
        }
        // A tempo that all but stands still is as good as steady.
        let slow = map.warp(960.0 + 1_920.0 + 480.0 + 5_000.0) - map.warp(960.0 + 1_920.0 + 480.0);
        assert!((slow - 5_000.0 * 12.0).abs() < 1e-3, "{slow}");
    }
}
