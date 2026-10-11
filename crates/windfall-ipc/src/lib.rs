//! Runtime state that is not part of the saved project: transport, meters,
//! audio devices, the file browser and export.
//!
//! The engine produces these, the app shell passes them on, and the UI reads
//! them through the TypeScript types `ts-rs` exports. docs/ARCHITECTURE.md
//! lists the IPC calls that carry them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
mod analysis;
pub use analysis::*;
mod audio_edit;
pub use audio_edit::*;
pub mod library;
pub use library::*;
mod midi;
mod slicer;
pub use midi::*;
mod midi_hardware;
pub use midi_hardware::*;
pub use slicer::{SliceOptions, SliceReview};
mod plugins;
pub use plugins::*;
mod sampler_preprocess;
pub use sampler_preprocess::*;
pub use windfall_project::PlayMode;
use windfall_project::{AutomationId, EffectId, PatternId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MetronomeSettings { pub enabled: bool, pub gain: f32, pub accent: bool }
impl Default for MetronomeSettings {
    fn default() -> Self { Self { enabled: false, gain: 0.25, accent: true } }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TransportState {
    pub playing: bool,
    pub mode: PlayMode,
    /// The pattern that plays in pattern mode.
    pub pattern: PatternId,
    /// In song mode, jump back to the start at the end of the last clip.
    pub loop_song: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub metronome: Option<MetronomeSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_in_bars: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_in_remaining: Option<u32>,
}

/// Session-only timeline playback state; no musical edit or undo entry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimelinePlaybackState {
    /// Transient, exact JS-safe monotonic publication number. Never wraps.
    #[ts(type = "number")]
    pub request: u64,
    #[ts(type = "number")]
    pub generation: u64,
    #[ts(type = "number")]
    pub revision: u64,
    pub region: Option<windfall_project::TickRange>,
    pub navigation_overflows: u32,
}

/// A change to the transport. `None` leaves a field alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct TransportPatch {
    #[ts(optional)]
    pub mode: Option<PlayMode>,
    #[ts(optional)]
    pub pattern: Option<PatternId>,
    #[ts(optional)]
    pub loop_song: Option<bool>,
    #[ts(optional)]
    pub metronome: Option<MetronomeSettings>,
    #[ts(optional)]
    pub count_in_bars: Option<u8>,
}

/// What the engine reports to the UI 60 times a second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RealtimeFrame {
    /// Existing analyzer's uncentered, normalized zero-lag stereo correlation,
    /// finite and in -1..=1. Absent when no valid correlation is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub correlation: Option<f32>,
    /// At most 64 ascending, uniformly grouped FFT bands from DC through
    /// Nyquist. Each is summed mean-square power averaged across valid
    /// stereo channels, not dB. Empty when no valid snapshot is available.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<f32>>", optional)]
    pub spectrum: Vec<f32>,
    /// Short analyzer history, flattened row-major with the oldest valid row
    /// first. At most 16 rows and 64 columns; each row follows `spectrum`'s
    /// finite, nonnegative mean-square band contract. Invalid slices are
    /// omitted. Empty (and omitted on the wire) when no slice is valid.
    /// Columns match nonempty `spectrum`; the current engine uses 64 columns
    /// even when the current spectrum is invalid and only older rows remain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<f32>>", optional)]
    pub spectrogram: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<MixerWaveform>>", optional)]
    pub waveforms: Vec<MixerWaveform>,
    pub playing: bool,
    /// Playhead in ticks. In pattern mode it is the position inside the
    /// pattern; in song mode it is the position on the playlist.
    pub tick: f64,
    /// Peak levels since the last frame as linear gain, two per mixer track
    /// in mixer order: left, right, left, right. The master comes first.
    pub meters: Vec<f32>,
    /// Share of the audio buffer's time the engine spent computing it, 0 to
    /// 1. Near 1 means dropouts are close.
    pub cpu: f32,
    /// Audio buffers that arrived late since the stream started.
    pub xruns: u32,
    /// Voices sounding right now: the samplers' and the instruments'.
    /// Audio clips are counted in `audio_clips`.
    pub voices: u32,
    /// Audio clips of the playlist playing right now, at most 128. A clip
    /// that is fading out after a stop, a seek or an edit is not counted.
    #[serde(default)]
    pub audio_clips: u32,
    /// Audio clips that should be sounding right now and are silent,
    /// because 128 were playing already when each was to start. Such a
    /// clip stays silent for its whole length. Zero while the song is not
    /// playing. No more than 1,024 are counted.
    #[serde(default)]
    pub dropped_clips: u32,
    /// How far each compressor and limiter of the project turned its
    /// signal down since the last frame, in mixer order and, on one track,
    /// in chain order. Effects of other kinds are not listed.
    #[serde(default)]
    pub gain_reductions: Vec<GainReduction>,
    /// The automations that have their target in hand right now, in the
    /// order of the project's automations, with the value each is giving
    /// it. An automation is listed from the start of its first clip that
    /// plays: while a clip of it plays, while its target holds the value
    /// the last clip left, and after the song has stopped for as long as
    /// it is still ringing out with the target held there. So this can
    /// have entries while `playing` is false. Where several automations
    /// move one target, the one whose clip decides is listed. Empty in
    /// pattern mode, and once a stopped song has let go of its targets. At
    /// most 128 are listed.
    ///
    /// A control whose target is listed here is being moved by the song:
    /// show it at this value instead of the one stored in the project.
    #[serde(default)]
    pub automated: Vec<AutomatedValue>,
}

/// Stereo signed extrema, oldest first, from the track's post-fader meter tap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MixerWaveform {
    pub track: windfall_project::TrackId,
    pub epoch: u64,
    pub serial: u64,
    pub sample_rate: u32,
    pub bucket_frames: u32,
    /// [left min, left max, right min, right max], at most 64 buckets.
    pub points: Vec<[f32; 4]>,
}

/// What one automation is doing to its target at this moment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutomatedValue {
    pub automation: AutomationId,
    /// 0 to 1 across the target's range, like the value of a point of the
    /// automation's curve.
    pub value: f32,
}

/// The reading of one compressor's or limiter's gain reduction meter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GainReduction {
    pub effect: EffectId,
    /// The deepest reduction since the last frame, in dB: 0 means the
    /// signal was not turned down, 6 means it was turned down by 6 dB.
    pub db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineStatus {
    /// An output stream is open and running.
    pub running: bool,
    /// Driver family, such as "WASAPI", "ASIO" or "CoreAudio".
    pub host: String,
    pub device: Option<String>,
    pub sample_rate: u32,
    #[serde(default)]
    #[ts(as = "Option<u16>", optional)]
    pub output_channels: u16,
    /// Frames per audio callback.
    pub buffer_frames: u32,
    /// Output latency of one buffer in milliseconds.
    pub latency_ms: f32,
    /// Frames by which the project's instruments and effects delay the
    /// output, at `sample_rate`: a limiter by its look-ahead, for one. The
    /// engine lines every path up with the slowest, so this is one figure
    /// for the whole mix. It comes on top of `latency_ms`, and it changes
    /// with the project, so ask for the status again after an edit to an
    /// effect.
    #[serde(default)]
    pub latency_frames: u32,
    /// Why the stream is not running, when it is not.
    pub error: Option<String>,
}

/// The audio output the user asked for. `None` means the system default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AudioSettings {
    #[ts(optional)]
    pub host: Option<String>,
    #[ts(optional)]
    pub device: Option<String>,
    #[ts(optional)]
    pub sample_rate: Option<u32>,
    #[ts(optional)]
    pub buffer_frames: Option<u32>,
    #[ts(optional)]
    pub output_channels: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioHost {
    pub name: String,
    pub is_default: bool,
    pub devices: Vec<AudioDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
    /// Sample rates the device accepts, ascending.
    pub sample_rates: Vec<u32>,
    /// Smallest and largest buffer size in frames, when the driver says.
    pub min_buffer_frames: Option<u32>,
    pub max_buffer_frames: Option<u32>,
    /// Channel counts the driver offers across its writable configurations.
    #[serde(default)]
    #[ts(as = "Option<Vec<u16>>", optional)]
    pub output_channels: Vec<u16>,
}

/// A top-level folder in the browser panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserRoot {
    pub name: String,
    pub path: String,
    pub kind: BrowserRootKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowserRootKind {
    /// Content shipped with Windfall.
    Factory,
    /// A folder the user added.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserEntry {
    pub name: String,
    /// Absolute path.
    pub path: String,
    pub kind: BrowserEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowserEntryKind {
    Folder,
    /// An audio file Windfall can decode.
    Audio,
    /// A `.windfall` project file.
    Project,
    Other,
}

/// Facts about an audio file, with a waveform overview for drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SampleInfo {
    pub path: String,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    #[ts(type = "number")]
    pub frames: u64,
    pub duration_secs: f64,
    /// Waveform overview as minimum and maximum pairs, one pair per bucket
    /// across the file, all channels mixed: min, max, min, max.
    pub peaks: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ExportFormat {
    /// Uncompressed, at any of the bit depths.
    Wav,
    /// Lossless and about half the size of WAV, at 16 or 24 bits.
    Flac,
    /// Lossy Ogg Vorbis; bit depth is ignored.
    Ogg,
    /// Lossy MPEG Layer III; bit depth is ignored.
    Mp3,
}

/// MP3 rate policy. VBR quality runs from 0 (best) to 9 (smallest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum Mp3Rate {
    Cbr { bitrate: u16 },
    Vbr { quality: u8 },
}

/// Channel coding of an MP3 export. Mono averages the rendered stereo pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Mp3Channels {
    Mono,
    Stereo,
    JointStereo,
}

/// MP3 settings. Absent settings mean 192 kbit/s joint stereo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Mp3Settings {
    pub rate: Mp3Rate,
    pub channels: Mp3Channels,
}

/// How samples are stored in a WAV or FLAC file. The other formats have no
/// bit depth and ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BitDepth {
    Int16,
    Int24,
    /// WAV only.
    Float32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportOptions {
    /// Optional linear once-only song region; absent retains whole-song export.
    #[serde(default)]
    #[ts(optional)]
    pub region: Option<windfall_project::TickRange>,
    /// Optional selected-export source guard, checked before snapshot preparation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub region_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub region_revision: Option<u64>,
    /// Absolute path of the file to write. With `stems` no file of this
    /// name is written: for `Song.flac` the files are `Song - Mix.flac`,
    /// `Song - Bass.flac` and so on, in a folder `Song` beside it or,
    /// without `stems.folder`, where `Song.flac` would have been.
    pub path: String,
    pub format: ExportFormat,
    /// Used by WAV and FLAC. FLAC has no 32-bit float.
    pub bit_depth: BitDepth,
    pub sample_rate: u32,
    /// Pattern mode renders the transport's pattern `pattern_loops` times.
    /// Song mode renders the playlist from the start to the last clip's end.
    pub mode: PlayMode,
    pub pattern_loops: u32,
    /// Extra time rendered after the end so reverb and release tails finish.
    /// With `auto_tail` it is the longest the tail may get.
    pub tail_secs: f32,
    /// Ends the tail as soon as everything has rung out: every note, every
    /// instrument and every effect is done and the output has fallen
    /// silent. The file is then as long as the sound, up to `tail_secs`
    /// past the end. Without it the tail is always `tail_secs` long.
    #[serde(default)]
    pub auto_tail: bool,
    /// FLAC compression level, 0 to 8. Every level is lossless; a higher
    /// one takes longer and gives a slightly smaller file. Absent means 5.
    #[serde(default)]
    #[ts(optional)]
    pub flac_level: Option<u8>,
    /// Vorbis quality from -1 to 10. Absent means 6.
    #[serde(default)]
    #[ts(optional)]
    pub ogg_quality: Option<f32>,
    /// MP3 rate and channel coding; absent means 192 kbit/s joint stereo.
    #[serde(default)]
    #[ts(optional)]
    pub mp3: Option<Mp3Settings>,
    /// Exports stems instead of one file. Absent means one file, the mix.
    #[serde(default)]
    #[ts(optional)]
    pub stems: Option<ExportStems>,
}

impl Default for ExportOptions {
    /// The song as one 24-bit WAV file at 48 kHz, with no path yet and no
    /// tail.
    fn default() -> Self {
        Self {
            path: String::new(),
            region: None,
            region_generation: None,
            region_revision: None,
            format: ExportFormat::Wav,
            bit_depth: BitDepth::Int24,
            sample_rate: 48_000,
            mode: PlayMode::Song,
            pattern_loops: 1,
            tail_secs: 0.0,
            auto_tail: false,
            flac_level: None,
            ogg_quality: None,
            mp3: None,
            stems: None,
        }
    }
}

/// What the stems of an export hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StemMode {
    /// What leaves each mixer track, as its meter shows it: after its
    /// effects, its fader and its pan, and before whatever a bus or the
    /// master does to it. A bus is a stem of its own. The stems add up to
    /// the mix only when no bus and no master effect comes after them.
    /// All stems are rendered in one pass.
    TrackOutputs,
    /// What each mixer track adds to the mix: the song with only what
    /// plays straight into that track sounding, heard at the output
    /// through its sends, its buses and the master's effects. The stems
    /// add up to the mix as long as everything on the way is linear; a
    /// compressor or limiter on a bus or the master reacts to each stem
    /// alone. The song is rendered once for every stem.
    ToMaster,
}

/// Exports the mixer tracks apart, each to a file of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportStems {
    pub mode: StemMode,
    /// The mixer tracks to export. Absent means every track that has
    /// something to give: with `trackOutputs` every track a channel or an
    /// audio clip plays into, straight or by way of other tracks, and with
    /// `toMaster` every track one plays straight into. The master cannot
    /// be listed; its sound is the mix.
    #[serde(default)]
    #[ts(optional)]
    pub tracks: Option<Vec<TrackId>>,
    /// Writes the whole mix as well, as one more file.
    pub include_mix: bool,
    /// Puts each track's place in the mixer in front of its name, as in
    /// "03 Bass", so the files sort the way the mixer shows the tracks.
    pub numbered: bool,
    /// Puts the files in a folder of their own, named like the export.
    /// Without it they are written where the export's path points.
    pub folder: bool,
}

/// One file an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportedFile {
    /// Absolute path of the file.
    pub path: String,
    /// The mixer track the file is the stem of. `None` for the mix.
    pub track: Option<TrackId>,
    /// Length of the audio in frames, at the export's sample rate.
    #[ts(type = "number")]
    pub frames: u64,
    /// Size of the file in bytes.
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportProgress {
    pub path: String,
    /// 0 to 1.
    pub fraction: f32,
    pub done: bool,
    pub error: Option<String>,
    /// Audio clips that are not in the file, because more than 128 would
    /// have played at once and the ones that started last were left out.
    /// Only the last event, the one with `done` set, says: it is zero on
    /// every event before it.
    #[serde(default)]
    pub dropped_clips: u32,
    /// The files the export wrote: the mix first, then the stems in mixer
    /// order. Only on the last event of an export that went through. An
    /// export that failed or was cancelled leaves no file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files: Option<Vec<ExportedFile>>,
    /// Set on the last event of an export that `export_cancel` stopped. It
    /// has no `error`: nothing went wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cancelled: Option<bool>,
}

/// An input's channel count and common rates, without opening capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingInput {
    pub host: String,
    pub device: String,
    pub channels: u16,
    pub sample_rates: Vec<u32>,
}

/// Runtime-only standalone input monitoring. Saved routes belong to the mixer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InputMonitorState {
    pub active: bool,
    pub sample_rate: u32,
    pub tracks: Vec<InputMonitorTrackInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InputMonitorTrackInfo {
    pub mixer_track: windfall_project::TrackId,
    pub name: String,
    pub alignment: RecordingAlignmentStatus,
    pub monitor: RecordingMonitorStatus,
}
/// Zero-based hardware channels. Missing right duplicates left into stereo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingSource {
    pub host: String,
    pub device: String,
    pub left: u16,
    pub right: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub alignment: Option<RecordingAlignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loop_recording: Option<RecordingLoopOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<RecordingMonitorSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub armed_tracks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mixer_tap: Option<RecordingMixerTap>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingMixerTap { pub track: windfall_project::TrackId, pub mode: windfall_project::MixerRecordMode }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingTrackInfo {
    pub mixer_track: windfall_project::TrackId,
    pub name: String,
    #[ts(type = "number")]
    pub frames: u64,
    pub alignment: RecordingAlignmentStatus,
    pub monitor: Option<RecordingMonitorStatus>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingMonitorSettings { pub track: windfall_project::TrackId, pub gain: f32, pub buffer_ms: u16 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingMonitorStatus {
    pub buffered_ms: f64,
    #[ts(type = "number")]
    pub dropped_frames: u64,
    #[ts(type = "number")]
    pub starved_frames: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingLoopOptions { pub region: windfall_project::TickRange }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingTakeSelection {
    All,
    Latest,
    Only { indices: Vec<u32> },
    Except { indices: Vec<u32> },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingTakeInfo {
    pub index: u32,
    #[ts(type = "number")]
    pub frames: u64,
    pub complete: bool,
}
/// Positive offsets advance the take relative to the playback clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingAlignment {
    pub synchronize: bool,
    pub drift_correction: bool,
    pub offset_ms: f64,
    pub input_sample_rate: Option<u32>,
}
impl Default for RecordingAlignment {
    fn default() -> Self { Self { synchronize: false, drift_correction: true, offset_ms: 0.0, input_sample_rate: None } }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingAlignmentStatus {
    pub input_sample_rate: u32,
    pub input_latency_ms: f64,
    pub output_latency_ms: f64,
    pub drift_ppm: f64,
    pub measured_timestamps: bool,
    #[ts(type = "number")]
    pub trimmed_frames: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingState {
    pub active: bool,
    #[ts(type = "number")]
    pub frames: u64,
    pub sample_rate: u32,
    pub start_tick: u32,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub alignment: Option<RecordingAlignmentStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub takes: Option<Vec<RecordingTakeInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<RecordingMonitorStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tracks: Option<Vec<RecordingTrackInfo>>,
}
mod flp;
pub use flp::*;
/// Relative tempo hypotheses, not calibrated probabilities. Half/double tempo can be ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipTempoCandidate {
    pub bpm: f64,
    pub confidence: f64,
}
/// Worker progress is session state, never project musical data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SamplerPreparationProgress {
    #[ts(type = "number")]
    pub request: u64,
    pub completed: u32,
    pub total: u32,
    pub current: bool,
}
