//! Control-side positions on the same sample grid as song audio export.
use crate::{SamplePool, plan::compile, tempo::TempoMap};
use windfall_project::Project;

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
