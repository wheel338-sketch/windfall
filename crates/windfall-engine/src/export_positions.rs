//! Control-side positions on the same sample grid as song audio export.
use crate::{SamplePool, plan::compile, tempo::TempoMap};
use windfall_project::Project;

/// The clocks actually used by an offline pass, in output-frame coordinates.
/// Latency compensation removes delayed audio, not musical clock frames: an
/// event generated at frame N is heard at output frame N after compensation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderFrameMap {
    clocks: Vec<(u64, crate::sequencer::Clock)>,
    map: Option<TempoMap>,
    rate: u32,
    first: f64,
    pub end_tick: f64,
}
impl RenderFrameMap {
    pub(crate) fn new(plan: &crate::plan::Plan, options: &crate::RenderOptions) -> Self {
        let song = options.mode == windfall_ipc::PlayMode::Song;
        let length = options
            .pattern
            .and_then(|id| plan.pattern_ids.get(id.0))
            .or_else(|| (!plan.patterns.is_empty()).then_some(0))
            .map_or(0, |i| plan.patterns[i].length);
        Self {
            clocks: Vec::new(),
            map: (song && !plan.linked_tempo())
                .then(|| plan.tempo_map.clone())
                .flatten(),
            rate: options.sample_rate.max(1),
            first: options.region.map_or(0.0, |r| f64::from(r.start)),
            end_tick: if song {
                options
                    .region
                    .map_or(f64::from(plan.song_end), |r| f64::from(r.end))
            } else {
                f64::from(length) * f64::from(options.pattern_loops)
            },
        }
    }
    pub(crate) fn record(&mut self, frame: u64, clock: crate::sequencer::Clock) {
        // An unchanged clock needs no extra storage. All heap work is here,
        // after Processor::process has returned on the offline caller.
        if self.clocks.last().is_none_or(|(_, old)| *old != clock) {
            self.clocks.push((frame, clock));
        }
    }
    pub fn frame(&self, tick: f64) -> Result<u64, String> {
        if !tick.is_finite() || tick < self.first || tick > self.end_tick {
            return Err("A WAV position lies outside the rendered tick range.".into());
        }
        let tick = self.map.as_ref().map_or(tick, |map| map.warp(tick));
        let after = self
            .clocks
            .partition_point(|(frame, clock)| clock.tick_at(*frame) <= tick);
        let (_, clock) = self
            .clocks
            .get(after.saturating_sub(1))
            .ok_or("The render did not capture a musical clock.")?;
        Ok(clock.frame_of(tick))
    }
    /// Only the body counts; tail processing does not describe musical beats.
    pub fn constant_tempo(&self) -> Option<f64> {
        if let Some(map) = &self.map {
            return map.constant_tempo(self.first, self.end_tick);
        }
        let (_, first) = self.clocks.first()?;
        let slope = first.frames_between(0.0, 1.0);
        self.clocks
            .iter()
            .all(|(_, clock)| clock.frames_between(0.0, 1.0) == slope)
            .then(|| 60.0 * f64::from(self.rate) / (f64::from(windfall_project::PPQ) * slope))
    }
}

pub struct SongFrameMap {
    rate: u32,
    base: f64,
    map: Option<TempoMap>,
    pub end_tick: u32,
}
impl SongFrameMap {
    pub fn new(project: &Project, pool: &SamplePool, rate: u32) -> Self {
        let plan = compile(project, pool);
        Self {
            rate,
            base: plan.tempo_bpm,
            map: plan.tempo_map.clone(),
            end_tick: plan.song_end,
        }
    }
    pub fn frame(&self, tick: u32) -> u64 {
        let warped = self
            .map
            .as_ref()
            .map_or(f64::from(tick), |m| m.warp(f64::from(tick)));
        (warped * windfall_core::samples_per_tick(self.base, f64::from(self.rate))).ceil() as u64
    }
    pub fn has_tempo_map(&self) -> bool {
        self.map.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_rounding_preserves_adjacent_region_endpoints() {
        let project = Project::new("Original frame-grid fixture");
        let map = SongFrameMap::new(&project, &SamplePool::new(), 44100);
        assert_eq!(map.frame(960), 22050);
        assert_eq!(map.frame(1), 23);
        assert_eq!(
            map.frame(962) - map.frame(1),
            (map.frame(480) - map.frame(1)) + (map.frame(962) - map.frame(480))
        );
    }
}
