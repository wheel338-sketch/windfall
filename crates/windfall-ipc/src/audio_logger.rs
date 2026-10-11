//! The audio logger: a bounded background history of what the engine played,
//! of chosen mixer tracks after their effects and of one hardware input, from
//! which an unplanned take can be put on the playlist afterwards.
//!
//! The settings are app settings. Nothing here is part of a project.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{PlaylistTrackId, TrackId};

/// Shortest history the logger keeps, in seconds.
pub const AUDIO_LOGGER_MIN_SECONDS: u32 = 30;
/// Longest history the logger keeps, in seconds.
pub const AUDIO_LOGGER_MAX_SECONDS: u32 = 600;
/// History length of a logger that was never configured, in seconds.
pub const AUDIO_LOGGER_DEFAULT_SECONDS: u32 = 120;
/// Mixer tracks that can be logged at once, besides the master output.
pub const AUDIO_LOGGER_MAX_TRACKS: usize = 4;

/// The hardware input logged in the background. Mono when `right` is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioLoggerInput {
    pub host: String,
    pub device: String,
    pub left: u16,
    pub right: Option<u16>,
}

/// What the logger is asked to keep. Every field has a default, so a
/// settings file written before the logger existed still loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AudioLoggerSettings {
    pub enabled: bool,
    /// History length in seconds, 30 to 600.
    pub seconds: u32,
    /// Log the final stereo mix the engine plays.
    pub master: bool,
    /// Mixer tracks logged after their effects, at most four. An id that
    /// names no track of the open project logs nothing.
    pub tracks: Vec<TrackId>,
    /// A hardware input captured continuously. Off unless chosen.
    pub input: Option<AudioLoggerInput>,
}

impl Default for AudioLoggerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            seconds: AUDIO_LOGGER_DEFAULT_SECONDS,
            master: true,
            tracks: Vec::new(),
            input: None,
        }
    }
}

/// One history of the logger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "type")]
#[ts(export)]
pub enum AudioLoggerSource {
    /// The final stereo mix, as it left the engine.
    Master,
    /// A mixer track after its effects, before its fader.
    Track { track: TrackId },
    /// The chosen hardware input.
    Input,
}

/// How much one source holds right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioLoggerSourceStatus {
    pub source: AudioLoggerSource,
    /// What to call the source: the track's name or the input device.
    pub label: String,
    /// The rate the history was captured at. An input keeps its own.
    pub sample_rate: u32,
    /// The most this history holds before the oldest audio is overwritten.
    pub capacity_seconds: f64,
    pub available_seconds: f64,
    #[ts(type = "number")]
    pub available_frames: u64,
    /// Frames lost because the history worker fell behind the audio thread.
    #[ts(type = "number")]
    pub dropped_frames: u64,
    /// Memory this source holds: its history and its transfer queue.
    #[ts(type = "number")]
    pub memory_bytes: u64,
    /// Whether audio is arriving: false for a track the project does not
    /// have, an input that is closed, or a logger with no audio output.
    pub active: bool,
    pub error: Option<String>,
}

/// The logger as it is right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioLoggerState {
    pub settings: AudioLoggerSettings,
    /// The engine is feeding the master and track histories.
    pub running: bool,
    pub sources: Vec<AudioLoggerSourceStatus>,
    #[ts(type = "number")]
    pub memory_bytes: u64,
    /// The most one source may hold; a long history at a high sample rate is
    /// shortened to fit.
    #[ts(type = "number")]
    pub max_source_bytes: u64,
    pub min_seconds: u32,
    pub max_seconds: u32,
    pub max_tracks: u32,
    pub error: Option<String>,
}

/// Puts the end of one history on the playlist as an audio clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioLoggerDumpRequest {
    pub source: AudioLoggerSource,
    /// How many trailing seconds to keep. Absent keeps all there is.
    pub seconds: Option<f64>,
    /// The tick the clip starts on. Absent uses the playhead.
    pub start: Option<u32>,
    /// The playlist track. Absent adds one for the clip.
    pub track: Option<PlaylistTrackId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_written_before_the_logger_existed_load_with_defaults() {
        let settings: AudioLoggerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, AudioLoggerSettings::default());
        assert!(settings.enabled && settings.master);
        assert_eq!(settings.seconds, AUDIO_LOGGER_DEFAULT_SECONDS);
        let partial: AudioLoggerSettings =
            serde_json::from_str(r#"{"seconds":300,"tracks":[4]}"#).unwrap();
        assert_eq!(partial.seconds, 300);
        assert_eq!(partial.tracks, vec![TrackId(4)]);
        assert!(partial.enabled && partial.input.is_none());
    }

    #[test]
    fn sources_round_trip_through_their_tagged_json() {
        for (source, json) in [
            (AudioLoggerSource::Master, r#"{"type":"master"}"#),
            (
                AudioLoggerSource::Track { track: TrackId(7) },
                r#"{"type":"track","track":7}"#,
            ),
            (AudioLoggerSource::Input, r#"{"type":"input"}"#),
        ] {
            assert_eq!(serde_json::to_string(&source).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<AudioLoggerSource>(json).unwrap(),
                source
            );
        }
    }
}
