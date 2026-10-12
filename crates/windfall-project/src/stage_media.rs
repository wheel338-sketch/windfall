//! Saved performance pads and audiovisual presentation. All edits use one
//! checked settings command; clocks and active launches are runtime state.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ChannelId, MAX_SONG_TICKS, PatternId, Project};

pub const PERFORMANCE_TRACKS: usize = 32;
pub const PERFORMANCE_SCENES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceLogEvent {
    pub frame: u32,
    pub track: u8,
    pub pattern: Option<PatternId>,
    pub quantization: LaunchQuantization,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceTake {
    /// Authoritative final-output PCM, including native plugins. The normal
    /// sample asset pipeline owns portability, missing files and history.
    pub sample: crate::SampleId,
    pub sample_rate: u32,
    pub frames: u32,
    pub initial_tick: f64,
    pub events: Vec<PerformanceLogEvent>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LaunchQuantization {
    Immediate,
    Beat,
    #[default]
    Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformancePad {
    pub track: u8,
    pub scene: u8,
    pub pattern: PatternId,
    /// An audio/automation playlist clip overrides the legacy pattern source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub clip: Option<crate::ClipId>,
}
impl PerformancePad {
    pub fn launch_pattern(self) -> PatternId {
        self.clip.map_or(self.pattern, |clip| PatternId(clip.0))
    }
    fn exists(self, project: &Project) -> bool {
        self.clip.map_or_else(
            || project.pattern(self.pattern).is_some(),
            |id| {
                project.playlist.clips.iter().any(|clip| {
                    clip.id == id
                        && matches!(
                            clip.content,
                            crate::ClipContent::Audio { .. }
                                | crate::ClipContent::Automation { .. }
                        )
                })
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VideoPlacement {
    /// A user-owned local file. No decoder or copied art is bundled.
    pub path: String,
    pub start_tick: u32,
    pub length_ticks: u32,
    /// Source seconds at the clip's first timeline tick.
    pub offset_seconds: f64,
    pub muted: bool,
    /// Legacy placements route to master. Audio is decoded by the native host.
    #[serde(default = "master_track")]
    pub mixer_track: crate::TrackId,
}
fn master_track() -> crate::TrackId {
    crate::TrackId::MASTER
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VisualScene {
    #[default]
    Orbit,
    Bars,
    Ribbon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VisualSettings {
    pub scene: VisualScene,
    pub intensity: f32,
    /// Hue, in degrees. Artwork is generated from authored geometry.
    pub hue: f32,
}
impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            scene: VisualScene::Orbit,
            intensity: 1.0,
            hue: 180.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StageMedia {
    #[serde(default)]
    pub takes: Vec<PerformanceTake>,
    #[serde(default)]
    pub pads: Vec<PerformancePad>,
    #[serde(default)]
    pub quantization: LaunchQuantization,
    #[serde(default)]
    pub videos: Vec<VideoPlacement>,
    #[serde(default)]
    pub visual: VisualSettings,
    #[serde(default)]
    pub dance_channel: Option<ChannelId>,
}
impl StageMedia {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn check(&self, project: &Project) -> Result<(), String> {
        if self.takes.len() > 16 {
            return Err("At most 16 saved performances are supported".into());
        }
        for take in &self.takes {
            if project
                .samples
                .iter()
                .all(|sample| sample.id != take.sample)
                || !(8000..=192000).contains(&take.sample_rate)
                || take.frames == 0
                || take.frames > take.sample_rate * 60
                || !take.initial_tick.is_finite()
                || take.initial_tick < 0.0
                || take.events.len() > 8192
                || take
                    .events
                    .windows(2)
                    .any(|pair| pair[0].frame > pair[1].frame)
                || take.events.iter().any(|event| {
                    event.frame > take.frames || usize::from(event.track) >= PERFORMANCE_TRACKS
                })
            {
                return Err("Invalid recorded performance sample, clock or launch log".into());
            }
        }
        if self.pads.len() > PERFORMANCE_TRACKS * PERFORMANCE_SCENES || self.videos.len() > 16 {
            return Err(
                "At most 512 performance pads and 16 video placements are supported".into(),
            );
        }
        let mut seen = std::collections::HashSet::new();
        for pad in &self.pads {
            if usize::from(pad.track) >= PERFORMANCE_TRACKS
                || usize::from(pad.scene) >= PERFORMANCE_SCENES
                || !seen.insert((pad.track, pad.scene))
                || !pad.exists(project)
            {
                return Err(
                    "A performance pad needs a unique in-range position and an existing pattern"
                        .into(),
                );
            }
        }
        for video in &self.videos {
            if video.path.is_empty()
                || video.path.len() > 4096
                || video.path.contains('\0')
                || video.length_ticks == 0
                || !project
                    .mixer
                    .tracks
                    .iter()
                    .any(|track| track.id == video.mixer_track)
                || video
                    .start_tick
                    .checked_add(video.length_ticks)
                    .is_none_or(|end| end > MAX_SONG_TICKS)
                || !video.offset_seconds.is_finite()
                || !(0.0..=86400.0).contains(&video.offset_seconds)
            {
                return Err(
                    "Video needs a local path, a bounded timeline range and a finite source offset"
                        .into(),
                );
            }
        }
        if !self.visual.intensity.is_finite()
            || !(0.0..=4.0).contains(&self.visual.intensity)
            || !self.visual.hue.is_finite()
            || !(0.0..=360.0).contains(&self.visual.hue)
            || self
                .dance_channel
                .is_some_and(|id| project.channel(id).is_none())
        {
            return Err("Invalid visualizer settings or missing character channel".into());
        }
        Ok(())
    }

    /// Deletion follows the referenced entities in the same undo transaction.
    pub(crate) fn followed(&self, project: &Project) -> Self {
        let mut next = self.clone();
        next.takes.retain(|take| {
            project
                .samples
                .iter()
                .any(|sample| sample.id == take.sample)
        });
        next.pads.retain(|pad| pad.exists(project));
        for video in &mut next.videos {
            if !project
                .mixer
                .tracks
                .iter()
                .any(|track| track.id == video.mixer_track)
            {
                video.mixer_track = crate::TrackId::MASTER;
            }
        }
        if next
            .dance_channel
            .is_some_and(|id| project.channel(id).is_none())
        {
            next.dance_channel = None;
        }
        next
    }
}
