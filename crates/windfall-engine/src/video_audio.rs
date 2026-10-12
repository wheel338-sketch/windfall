//! Video sound is read from prepared PCM at the picture's source time.
//! Stateless addressing makes seeks/regions/loops independent of callback
//! partitions. Mixer routing supplies track effects, sends, metering and PDC.
use crate::{plan::{IdIndex, Plan}, pool::SamplePool, sequencer::Sequencer};
use windfall_core::AudioBuffer;

#[derive(Debug, Clone)]
pub(crate) struct PreparedVideo {
    pub start: u32,
    pub end: u32,
    pub offset: f64,
    pub track: usize,
    pub audio: AudioBuffer,
}
pub(crate) fn compile(project: &windfall_project::Project, pool: &SamplePool, tracks: &IdIndex) -> Vec<PreparedVideo> {
    project.settings.stage_media.videos.iter().filter(|video| !video.muted).filter_map(|video| {
        Some(PreparedVideo {
            start: video.start_tick,
            end: video.start_tick + video.length_ticks,
            offset: video.offset_seconds,
            track: tracks.get(video.mixer_track.0)?,
            audio: pool.video_audio(&video.path)?.clone(),
        })
    }).collect()
}
impl PreparedVideo {
    fn sample(&self, plan: &Plan, tick: f64) -> [f32; 2] {
        if tick < f64::from(self.start) || tick >= f64::from(self.end) { return [0.0; 2]; }
        let seconds = (plan.warp(tick) - plan.warp(f64::from(self.start))) * 60.0 / (plan.tempo_bpm * f64::from(windfall_core::PPQ));
        let position = (self.offset + seconds) * f64::from(self.audio.sample_rate());
        let first = position.floor() as usize;
        if first >= self.audio.frames() { return [0.0; 2]; }
        let channels = usize::from(self.audio.channels());
        if channels == 0 { return [0.0; 2]; }
        let fraction = (position - first as f64) as f32;
        std::array::from_fn(|side| {
            let channel = side.min(channels - 1);
            let a = self.audio.samples()[first * channels + channel];
            let b = self.audio.samples().get((first + 1) * channels + channel).copied().unwrap_or(a);
            let value = a + (b - a) * fraction;
            if value.is_finite() { value } else { 0.0 }
        })
    }
}
pub(crate) fn render(plan: &Plan, sequencer: &Sequencer, mixer: &mut crate::mixer::Mixer, base: u64, from: usize, to: usize) {
    for video in &plan.videos {
        for frame in from..to {
            let Some(tick) = sequencer.song_tick_at(plan, base + frame as u64) else { continue; };
            let value = video.sample(plan, tick);
            let out = &mut mixer.track_mut(video.track)[frame];
            out[0] += value[0]; out[1] += value[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_source_time_is_half_open_offset_exact_and_seek_safe() {
        let project = windfall_project::Project::new("video clock");
        let plan = crate::plan::compile(&project, &SamplePool::new());
        let video = PreparedVideo { start: 960, end: 1920, offset: 0.25, track: 0,
            audio: AudioBuffer::from_interleaved(8, 2, (0..16).map(|n| n as f32 / 16.0).collect()) };
        assert_eq!(video.sample(&plan, 959.0), [0.0; 2]);
        assert_eq!(video.sample(&plan, 960.0), [4.0 / 16.0, 5.0 / 16.0]);
        assert_eq!(video.sample(&plan, 1440.0), [8.0 / 16.0, 9.0 / 16.0]);
        assert_eq!(video.sample(&plan, 1920.0), [0.0; 2]);
        assert_eq!(video.sample(&plan, 960.0), [4.0 / 16.0, 5.0 / 16.0]);
    }
}
