//! Saved performance pads and audiovisual presentation. All edits use one
//! checked settings command; clocks and active launches are runtime state.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ChannelId, MAX_SONG_TICKS, PatternId, Project};

pub const PERFORMANCE_TRACKS: usize = 32;
pub const PERFORMANCE_SCENES: usize = 16;

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
fn master_track() -> crate::TrackId { crate::TrackId::MASTER }

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
                || project.pattern(pad.pattern).is_none()
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
                || !project.mixer.tracks.iter().any(|track| track.id == video.mixer_track)
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
        next.pads
            .retain(|pad| project.pattern(pad.pattern).is_some());
        for video in &mut next.videos {
            if !project.mixer.tracks.iter().any(|track| track.id == video.mixer_track) {
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
