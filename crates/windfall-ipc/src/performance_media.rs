use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{PatternId, PlayMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActiveNotePose {
    pub channel: windfall_project::ChannelId,
    pub key: u8,
    pub velocity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaRenderAudio {
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceTrackState {
    pub active: Option<PatternId>,
    pub queued: Option<PatternId>,
    pub queued_stop: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceState {
    pub available: bool,
    pub tick: f64,
    pub dropped_notes: String,
    pub tracks: Vec<PerformanceTrackState>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceRecordingState {
    pub active: bool,
    pub finished: bool,
    pub frames: u32,
    pub sample_rate: u32,
    pub error: Option<String>,
}
/// PDC-aligned audio presentation cursor. Device latency is explicitly
/// unknown here: do not fabricate driver timestamps from wall time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaClock {
    pub epoch: String,
    pub output_frame: String,
    pub sample_rate: u32,
    pub pdc_frames: u32,
    pub device_latency_frames: Option<u32>,
    pub playing: bool,
    pub mode: PlayMode,
    pub tick: f64,
}
