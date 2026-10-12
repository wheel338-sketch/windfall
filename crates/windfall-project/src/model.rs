//! The project model: everything that is saved in a `.windfall` file.
//!
//! These types are the contract between the engine, the app shell and the UI.
//! They serialize to camelCase JSON, and `ts-rs` exports matching TypeScript
//! types. New fields must carry `#[serde(default)]` so older files still load;
//! anything that cannot be expressed that way needs a [`FORMAT_VERSION`] bump
//! and a migration.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use windfall_core::{PPQ, TICKS_PER_STEP};
pub use windfall_dsp::{EffectKind, EffectParams, InstrumentKind, InstrumentParams};

/// Version of the on-disk project format.
pub const FORMAT_VERSION: u32 = 1;

/// Five hundred inserts, Master and one Current utility. The engine sizes its
/// bounded mixer buffers and meter storage from this hard limit.
pub const MAX_MIXER_TRACKS: usize = 502;
/// Master plus the five hundred audible inserts; Current has its own seat.
pub const MAX_MIXER_SIGNAL_TRACKS: usize = 501;

/// Effects one mixer track can hold.
pub const MAX_EFFECT_SLOTS: usize = 10;

/// MIDI key a step-sequencer step plays, and the default root key of a
/// sampler. FL Studio labels this key C5.
pub const DEFAULT_KEY: u8 = 60;

pub const DEFAULT_VELOCITY: f32 = 0.8;
pub const DEFAULT_CHANNEL_VOLUME: f32 = 0.8;
pub const DEFAULT_PATTERN_STEPS: u32 = 16;

/// Longest pattern, in sixteenth-note steps.
pub const MAX_PATTERN_STEPS: u32 = 1024;

/// UTF-8 bytes in a channel group name. Empty means no group.
pub const MAX_CHANNEL_GROUP_NAME_BYTES: usize = 128;

/// Length of the longest pattern in ticks. A note that starts at or past
/// this tick could never play in any pattern, so no edit may put one there.
pub const MAX_PATTERN_TICKS: u32 = MAX_PATTERN_STEPS * TICKS_PER_STEP;

/// Length of the longest song in ticks: a million quarter notes, which is
/// 250,000 bars of 4/4 and about 139 hours at 120 bpm. Every clip ends at
/// or before this tick, and no clip offset, fade or automation point lies
/// past it. It is well under a quarter of what a `u32` holds, so a start,
/// a length and an offset can be added up in a `u32` without overflowing.
pub const MAX_SONG_TICKS: u32 = 1_000_000 * PPQ;

pub const MIN_TEMPO_BPM: f64 = 10.0;
pub const MAX_TEMPO_BPM: f64 = 522.0;

/// Highest linear gain a channel or mixer fader allows, about +6 dB.
pub const MAX_GAIN: f32 = 2.0;

/// Furthest a sampler can be tuned, and an audio clip pitched, up or down,
/// in semitones.
pub const MAX_TUNE_SEMITONES: f32 = 48.0;

/// Longest attack, decay or release time of an envelope, in milliseconds.
pub const MAX_ENVELOPE_MS: f32 = 60_000.0;

/// Highest MIDI key.
pub const MAX_KEY: u8 = 127;

/// Most points one automation curve can have.
pub const MAX_AUTOMATION_POINTS: usize = 4096;

/// Colors handed to new channels, patterns, mixer tracks and automations,
/// as 0xRRGGBB.
/// Neighbours are far apart in hue so adjacent rows are easy to tell apart.
pub const PALETTE: [u32; 12] = [
    0xE5484D, 0x12A594, 0xFFB224, 0x6E56CF, 0x46A758, 0xD6409F, 0x05A2C2, 0xF76B15, 0x3E63DD,
    0x99D52A, 0x8E4EC6, 0xE93D82,
];

/// Color of the master mixer track in a new project.
pub const MASTER_COLOR: u32 = 0x8B8D98;

/// The palette color for the `index`th item of a list. Cycles.
pub fn palette_color(index: usize) -> u32 {
    PALETTE[index % PALETTE.len()]
}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS,
        )]
        #[serde(transparent)]
        #[ts(export)]
        pub struct $name(pub u32);

        impl From<u32> for $name {
            fn from(value: u32) -> Self {
                Self(value)
            }
        }

        impl From<$name> for u32 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

id_type!(
    /// Identifies a sample in the project's sample pool.
    SampleId
);
id_type!(
    /// Identifies a channel in the channel rack.
    ChannelId
);
id_type!(
    /// Identifies a pattern.
    PatternId
);
id_type!(
    /// Identifies a note. Unique across the whole project.
    NoteId
);
id_type!(
    /// Identifies a mixer track. The master is always [`TrackId::MASTER`].
    TrackId
);
id_type!(
    /// Identifies an effect on a mixer track. Unique across the whole
    /// project, so an effect keeps its id when it moves to another track.
    EffectId
);
id_type!(
    /// Identifies a playlist track (a lane of the song timeline).
    PlaylistTrackId
);
id_type!(
    /// Identifies a clip on the playlist.
    ClipId
);
id_type!(
    /// Identifies an automation: a curve and what it moves.
    AutomationId
);

impl TrackId {
    pub const MASTER: TrackId = TrackId(0);
}

/// What the transport plays. It is not part of a [`Project`]: the transport
/// has it, and a project file keeps it beside the project
/// ([`ProjectSession`](crate::file::ProjectSession)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PlayMode {
    /// Loops the selected pattern.
    Pattern,
    /// Plays the playlist.
    Song,
}

/// A whole project. Ids are allocated from `next_id` and never reused, so an
/// id means the same thing for the life of the project, across undo and redo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Project {
    pub format_version: u32,
    /// The next id to hand out. Id 0 is reserved for the master mixer track.
    pub next_id: u32,
    pub settings: ProjectSettings,
    /// Up to eight saved plain-text song notebook pages.
    #[serde(default, skip_serializing_if = "crate::Notebook::is_empty")]
    #[ts(as = "Option<crate::Notebook>", optional)]
    pub notebook: crate::Notebook,
    pub samples: Vec<SampleAsset>,
    /// Channel rack, in display order.
    pub channels: Vec<Channel>,
    /// Patterns, in display order. A project always has at least one.
    pub patterns: Vec<Pattern>,
    pub mixer: Mixer,
    pub playlist: Playlist,
    /// The automation curves, in the order they were made. A curve plays
    /// where a clip on the playlist puts it.
    #[serde(default)]
    pub automations: Vec<Automation>,
    /// Native plugin instances, saved even when their files are unavailable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::PluginBinding>>", optional)]
    pub plugins: Vec<crate::PluginBinding>,
    /// Opaque states from imported plugins, kept even before a compatible host exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_plugins: Vec<RetainedPluginState>,
}

/// A plugin state the importer understands only as opaque bytes. Source ids
/// remain descriptive metadata after a channel or track is removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RetainedPluginState {
    pub source: String,
    pub internal_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub vendor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<ChannelId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub track: Option<TrackId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub slot: Option<u8>,
    /// The original wrapper/state; it is not executable code.
    pub state: Vec<u8>,
}
impl Project {
    /// An empty project: 120 bpm in 4/4, one empty pattern, and a mixer that
    /// holds only the master track.
    pub fn new(name: impl Into<String>) -> Self {
        let pattern = PatternId(1);
        Self {
            format_version: FORMAT_VERSION,
            next_id: pattern.0 + 1,
            settings: ProjectSettings {
                name: name.into(),
                author: String::new(),
                genre: String::new(),
                comments: String::new(),
                tempo_bpm: 120.0,
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                swing: 0.0,
                pan_law: crate::PanLaw::default(),
                timebase_ppq: crate::timebase::DEFAULT_TIMEBASE_PPQ,
                controller_links: Vec::new(),
                internal_controllers: Vec::new(),
                modular_racks: Vec::new(),
            },
            samples: Vec::new(),
            notebook: crate::Notebook::default(),
            channels: Vec::new(),
            patterns: vec![Pattern {
                note_curves: Vec::new(),
                time_signature: None,
                timeline: Default::default(),
                id: pattern,
                name: "Pattern 1".to_owned(),
                color: palette_color(0),
                length_steps: DEFAULT_PATTERN_STEPS,
                lanes: Vec::new(),
                events: Vec::new(),
            }],
            mixer: Mixer {
                effect_outputs: Vec::new(),
                tracks: vec![MixerTrack {
                    dock: crate::MixerDock::default(),
                    external_output: None,
                    processing: windfall_dsp::TrackParams::default(),
                    current: false,
                    latency_offset_ms: 0.0,
                    id: TrackId::MASTER,
                    name: "Master".to_owned(),
                    color: MASTER_COLOR,
                    volume: 1.0,
                    pan: 0.0,
                    muted: false,
                    solo: false,
                    output: None,
                    sidechains: Vec::new(),
                    sends: Vec::new(),
                    effects: Vec::new(),
                    recording: None,
                }],
            },
            playlist: Playlist {
                arrangement_book: Default::default(),
                tracks: Vec::new(),
                clips: Vec::new(),
                timeline: crate::Timeline::default(),
                take_groups: Vec::new(),
            },
            automations: Vec::new(),
            plugins: Vec::new(),
            retained_plugins: Vec::new(),
        }
    }

    pub fn sample(&self, id: SampleId) -> Option<&SampleAsset> {
        self.samples.iter().find(|sample| sample.id == id)
    }

    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|channel| channel.id == id)
    }

    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|pattern| pattern.id == id)
    }

    pub fn automation(&self, id: AutomationId) -> Option<&Automation> {
        self.automations
            .iter()
            .find(|automation| automation.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSettings {
    pub name: String,
    /// Project author, at most 256 UTF-8 bytes after trimming.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub author: String,
    /// Project genre, at most 128 UTF-8 bytes after trimming.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub genre: String,
    /// Project comments, at most 16,384 UTF-8 bytes after trimming.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub comments: String,
    /// Tempo in beats per minute, [`MIN_TEMPO_BPM`] to [`MAX_TEMPO_BPM`].
    pub tempo_bpm: f64,
    pub time_signature: TimeSignature,
    /// Swing amount from 0 to 1. It delays every second sixteenth-note step.
    /// At 1 that step lands two thirds of the way through its pair of steps,
    /// which is a full triplet feel.
    pub swing: f32,
    /// How panning changes level in playback and export. A file that names
    /// none has [`PanLaw::Balance`](crate::PanLaw::Balance).
    #[serde(default, skip_serializing_if = "crate::PanLaw::is_default")]
    #[ts(as = "Option<crate::PanLaw>", optional)]
    pub pan_law: crate::PanLaw,
    /// Ticks per quarter note that playback and export count time in: one
    /// of [`TIMEBASES`](crate::timebase::TIMEBASES). Stored positions stay
    /// at 960; see [`crate::timebase`]. A file that names none has 960.
    #[serde(
        default = "crate::timebase::default_ppq",
        skip_serializing_if = "crate::timebase::is_default_ppq"
    )]
    #[ts(as = "Option<u16>", optional)]
    pub timebase_ppq: u16,
    /// What hardware and internal controllers move in this project, at most
    /// [`MAX_CONTROLLER_LINKS`](crate::controller_links::MAX_CONTROLLER_LINKS).
    /// Edited by [`Command::SetControllerLinks`](crate::Command).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::ControllerLink>>", optional)]
    pub controller_links: Vec<crate::ControllerLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::InternalControllerSettings>>", optional)]
    pub internal_controllers: Vec<crate::InternalControllerSettings>,
    /// Saved modular graphs by channel; legacy projects omit the empty table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::ChannelRack>>", optional)]
    pub modular_racks: Vec<crate::ChannelRack>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimeSignature {
    /// Beats per bar, 1 to 16.
    pub numerator: u8,
    /// Beat unit: 2, 4, 8 or 16.
    pub denominator: u8,
}

impl TimeSignature {
    /// Length of the notated beat in ticks.
    pub fn ticks_per_beat(self) -> u32 {
        PPQ * 4 / self.denominator as u32
    }
    /// Length of one bar in ticks.
    pub fn ticks_per_bar(self) -> u32 {
        self.numerator as u32 * PPQ * 4 / self.denominator as u32
    }
}

/// An audio file the project uses: as the sample of a sampler channel, as
/// the audio of a clip on the playlist, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SampleAsset {
    pub id: SampleId,
    pub name: String,
    pub path: SamplePath,
}

/// Where a sample's audio file lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "path", rename_all = "camelCase")]
#[ts(export)]
pub enum SamplePath {
    /// Shipped with Windfall. The path is relative to the factory content
    /// folder and uses forward slashes.
    Factory(String),
    /// Inside the project folder. The path is relative to it and uses forward
    /// slashes.
    Project(String),
    /// Anywhere else on disk, as an absolute path.
    External(String),
}

impl SamplePath {
    /// Says what is wrong with the stored path, or `None` when it is well
    /// formed. Relative paths must use forward slashes and stay inside their
    /// folder. Whether the file exists is not checked.
    pub fn problem(&self) -> Option<&'static str> {
        match self {
            SamplePath::External(path) if path.is_empty() => Some("the path is empty"),
            SamplePath::External(_) => None,
            SamplePath::Factory(path) | SamplePath::Project(path) => relative_path_problem(path),
        }
    }
}

/// Says what stops `path` being stored as a relative sample path.
pub(crate) fn relative_path_problem(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        Some("the path is empty")
    } else if path.contains('\\') {
        Some("a relative path must use forward slashes")
    } else if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        Some("a relative path must stay inside its folder")
    } else {
        None
    }
}

/// One row of the channel rack.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Channel {
    /// Immutable source/marker/curve authoring data, separate from Copy controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sample_playback: Option<crate::SamplePlayback>,
    /// Additional note generators and modulation; absent in older files.
    #[serde(
        default,
        deserialize_with = "crate::channel_voice::deserialize_settings"
    )]
    #[ts(as = "Option<crate::ChannelVoiceSettings>", optional)]
    pub voice: crate::ChannelVoiceSettings,
    pub id: ChannelId,
    pub name: String,
    /// Named rack group. The empty string is the ungrouped category.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub group: String,
    /// Non-destructive gate, shift and per-channel swing.
    #[serde(default, skip_serializing_if = "crate::ChannelTiming::is_default")]
    #[ts(as = "Option<crate::ChannelTiming>", optional)]
    pub timing: crate::ChannelTiming,
    /// Display color as 0xRRGGBB.
    pub color: u32,
    /// Linear gain, 0 to [`MAX_GAIN`].
    pub volume: f32,
    /// -1 is hard left, 1 is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    /// The mixer track this channel plays into.
    pub mixer_track: TrackId,
    pub source: ChannelSource,
    /// What modulation X, modulation Y and release drive in this channel's
    /// voices. Absent in older files, which is the fixed mapping they had.
    #[serde(default, skip_serializing_if = "crate::ExpressionMap::is_default")]
    #[ts(as = "Option<crate::ExpressionMap>", optional)]
    pub expression_map: crate::ExpressionMap,
}

/// What makes the sound of a channel: a sample or an instrument. Hosted
/// plugins are added as new variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
// Instrument settings remain inline in the public project model, preserving
// callers' value construction and avoiding a separate settings allocation.
#[allow(clippy::large_enum_variant)]
pub enum ChannelSource {
    Sampler(SamplerSettings),
    /// A built-in instrument that turns the channel's notes into sound. A
    /// note's velocity, pan and expression are sent to the instrument's voices.
    #[serde(rename_all = "camelCase")]
    Instrument {
        /// The instrument and its settings, with every value inside its
        /// range.
        params: InstrumentParams,
        /// Prepared immutable spectra, key programs or sample-layer PCM.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        asset: Option<windfall_dsp::InstrumentAsset>,
    },
}

impl ChannelSource {
    /// The sample a sampler plays. `None` for an instrument, and for a
    /// sampler with no sample.
    pub fn sample(&self) -> Option<SampleId> {
        match self {
            ChannelSource::Sampler(sampler) => sampler.sample,
            ChannelSource::Instrument { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SamplerSettings {
    pub sample: Option<SampleId>,
    /// Independent duration/pitch is prepared for an explicit inclusive MIDI range.
    #[serde(default, skip_serializing_if = "SamplerStretch::is_tape")]
    #[ts(as = "Option<SamplerStretch>", optional)]
    pub stretch: SamplerStretch,
    /// The key that plays the sample at its recorded pitch.
    pub root_key: u8,
    /// Tuning offset in semitones, -48 to 48. Fractions are fine tuning.
    pub tune: f32,
    /// Extra linear gain applied to the sample, 0 to [`MAX_GAIN`].
    pub gain: f32,
    /// Playback start as a fraction of the sample, 0 to 1.
    pub start: f32,
    /// Playback end as a fraction of the sample, 0 to 1, greater than `start`.
    pub end: f32,
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "SamplerLoopMode::is_off")]
    #[ts(as = "Option<SamplerLoopMode>", optional)]
    pub loop_mode: SamplerLoopMode,
    /// Loop start as a fraction of the trimmed region, in source order.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(as = "Option<f32>", optional)]
    pub loop_start: f32,
    /// Exclusive loop end as a fraction of the trimmed region.
    #[serde(default = "loop_end_default", skip_serializing_if = "is_one")]
    #[ts(as = "Option<f32>", optional)]
    pub loop_end: f32,
    /// Playback crossfade at the loop seam, 0 to 1. Zero preserves the dry loop.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(as = "Option<f32>", optional)]
    pub loop_crossfade: f32,
    /// With no envelope a one-shot ignores note length, which drum hits want.
    /// Loops use a short release when this is absent; an envelope gates either.
    pub envelope: Option<Envelope>,
    /// A new note on this channel stops the notes already playing on it.
    pub cut_self: bool,
    /// Channels that share a nonzero cut group stop each other, such as an
    /// open and a closed hi-hat. 0 means no group.
    pub cut_group: u8,
}

impl Default for SamplerSettings {
    fn default() -> Self {
        Self {
            sample: None,
            stretch: SamplerStretch::Tape,
            root_key: DEFAULT_KEY,
            tune: 0.0,
            gain: 1.0,
            start: 0.0,
            end: 1.0,
            reverse: false,
            loop_mode: SamplerLoopMode::Off,
            loop_start: 0.0,
            loop_end: 1.0,
            loop_crossfade: 0.0,
            envelope: None,
            cut_self: false,
            cut_group: 0,
        }
    }
}

/// Musical settings only; rendered banks are never serialized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum SamplerStretch {
    #[default]
    Tape,
    Spectral {
        ratio: f64,
        quality: ClipStretchQuality,
        formants: bool,
        range: SamplerKeyRange,
    },
}

impl SamplerStretch {
    fn is_tape(&self) -> bool {
        matches!(self, Self::Tape)
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if let Self::Spectral { ratio, range, .. } = self {
            if !ratio.is_finite() || !(0.25..=4.0).contains(&ratio) {
                return Err("the sampler duration ratio must be finite and between 0.25 and 4");
            }
            if range.first > range.last || range.last > MAX_KEY {
                return Err("the prepared sampler key range must be ordered within MIDI 0 to 127");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SamplerKeyRange {
    pub first: u8,
    pub last: u8,
}

impl SamplerKeyRange {
    /// Twelve playable keys, centered near the root and kept inside MIDI bounds.
    pub fn around_root(root: u8) -> Self {
        let first = root.min(MAX_KEY).saturating_sub(6).min(MAX_KEY - 11);
        Self {
            first,
            last: first + 11,
        }
    }
}

/// A loop repeats while a note is held and during its envelope release.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SamplerLoopMode {
    #[default]
    Off,
    Forward,
    PingPong,
}

impl SamplerLoopMode {
    fn is_off(&self) -> bool {
        *self == Self::Off
    }
}

fn loop_end_default() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

/// An attack, decay, sustain, release volume envelope. The three times run
/// from 0 to [`MAX_ENVELOPE_MS`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Envelope {
    pub attack_ms: f32,
    pub decay_ms: f32,
    /// Sustain level, 0 to 1.
    pub sustain: f32,
    pub release_ms: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack_ms: 1.0,
            decay_ms: 200.0,
            sustain: 1.0,
            release_ms: 50.0,
        }
    }
}

/// A short musical idea: notes for any number of channels.
///
/// The step sequencer and the piano roll edit the same notes. A step that is
/// on is a note at `step * TICKS_PER_STEP` with key [`DEFAULT_KEY`] and a
/// length of one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Pattern {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::NoteExpressionCurve>>", optional)]
    pub note_curves: Vec<crate::NoteExpressionCurve>,
    pub id: PatternId,
    pub name: String,
    /// Display color as 0xRRGGBB.
    pub color: u32,
    /// Length in sixteenth-note steps, 1 to 1024. The pattern loops at this
    /// length, and notes that start at or after it do not play. They are
    /// kept, so making the pattern longer again brings them back.
    pub length_steps: u32,
    /// None inherits the project's base meter. Tick-zero changes may override it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub time_signature: Option<TimeSignature>,
    #[serde(default, skip_serializing_if = "crate::Timeline::is_empty")]
    #[ts(as = "Option<crate::Timeline>", optional)]
    pub timeline: crate::Timeline,
    /// Notes per channel, sorted by channel id. Channels with no notes have
    /// no lane.
    pub lanes: Vec<Lane>,
    /// Event automation: curves in pattern ticks that move a control
    /// wherever this pattern plays. At most one lane per target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::PatternEventLane>>", optional)]
    pub events: Vec<crate::PatternEventLane>,
}

impl Pattern {
    pub fn effective_signature(&self, legacy: TimeSignature) -> TimeSignature {
        self.time_signature.unwrap_or(legacy)
    }

    pub fn check_musical(&self, legacy: TimeSignature, next_id: u32) -> Result<(), String> {
        crate::note_curves::check(self)?;
        self.timeline
            .check(self.effective_signature(legacy), next_id)?;
        if self
            .timeline
            .meters
            .iter()
            .any(|meter| meter.tick > MAX_PATTERN_TICKS)
            || self.timeline.markers.iter().any(|marker| {
                marker.tick > MAX_PATTERN_TICKS || marker.kind != crate::MarkerKind::Named
            })
        {
            return Err(
                "pattern meters and named markers must fit the maximum pattern span".to_owned(),
            );
        }
        Ok(())
    }
    pub fn length_ticks(&self) -> u32 {
        self.length_steps * TICKS_PER_STEP
    }

    pub fn lane(&self, channel: ChannelId) -> Option<&Lane> {
        self.lanes.iter().find(|lane| lane.channel == channel)
    }
}

impl Note {
    /// The order notes keep inside a lane.
    pub fn sort_key(&self) -> (u32, u8, NoteId) {
        (self.start, self.key, self.id)
    }
}

/// The notes one channel plays in one pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Lane {
    pub channel: ChannelId,
    /// Sorted by `start`, then `key`, then `id`.
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Note {
    pub id: NoteId,
    /// Start in ticks from the beginning of the pattern. An edit can put a
    /// note anywhere before [`MAX_PATTERN_TICKS`], the end of the longest
    /// pattern.
    pub start: u32,
    /// Length in ticks, at least 1.
    pub length: u32,
    /// MIDI key, 0 to 127.
    pub key: u8,
    /// 0 to 1.
    pub velocity: f32,
    /// -1 is hard left, 1 is hard right. Added to the channel pan.
    pub pan: f32,
    /// Per-note release, fine pitch and two modulation controls.
    #[serde(default, skip_serializing_if = "crate::NoteExpression::is_default")]
    #[ts(as = "Option<crate::NoteExpression>", optional)]
    pub expression: crate::NoteExpression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Mixer {
    /// `tracks[0]` is always the master. At most [`MAX_MIXER_TRACKS`].
    pub tracks: Vec<MixerTrack>,
    /// Separately routable effect outputs sent to other tracks, such as a
    /// Send Tap's copy or the two parts of a Transient Split.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::EffectOutputRoute>>", optional)]
    pub effect_outputs: Vec<crate::EffectOutputRoute>,
}

impl Mixer {
    pub fn track(&self, id: TrackId) -> Option<&MixerTrack> {
        self.tracks.iter().find(|track| track.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MixerTrack {
    #[serde(default, skip_serializing_if = "MixerDock::is_middle")]
    #[ts(as = "Option<MixerDock>", optional)]
    pub dock: MixerDock,
    /// Integrated post-slot EQ and stereo utilities, before the fader.
    #[serde(default, skip_serializing_if = "windfall_dsp::TrackParams::is_default")]
    #[ts(as = "Option<windfall_dsp::TrackParams>", optional)]
    pub processing: windfall_dsp::TrackParams,
    /// Utility-only strip fed by the current selection, with no audible routes.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub current: bool,
    pub id: TrackId,
    pub name: String,
    /// Display color as 0xRRGGBB.
    pub color: u32,
    /// Fader as linear gain, 0 to [`MAX_GAIN`]. 1 is 0 dB.
    pub volume: f32,
    /// -1 is hard left, 1 is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    /// Where the track's output goes. `None` on the master, and on a track
    /// that only feeds its sends. Routing never forms a cycle.
    pub output: Option<TrackId>,
    /// Extra copies of the post-fader signal sent to other tracks.
    pub sends: Vec<Send>,
    /// Post-fader detector-only copies; they are never summed into the audible input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Send>>", optional)]
    pub sidechains: Vec<Send>,
    /// The track's effects, in the order the signal passes through them,
    /// at most [`MAX_EFFECT_SLOTS`]. They come before the fader: what
    /// arrives on the track runs through the effects, then the fader and
    /// pan, and from there to the meter, the output and the sends. The
    /// master has effects like any other track.
    #[serde(default)]
    pub effects: Vec<EffectSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recording: Option<MixerRecording>,
    /// Post-fader hardware destination on the currently selected output device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub external_output: Option<ExternalOutputRoute>,
    /// Signed correction to the track's declared output latency, in milliseconds.
    /// This compensates other routes; it does not add an audible delay here.
    #[serde(default)]
    #[ts(as = "Option<f64>", optional)]
    pub latency_offset_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExternalOutputRoute {
    pub left: u16,
    /// None folds stereo to mono on `left`.
    pub right: Option<u16>,
    /// Suppress the ordinary output edge; explicit sends remain available.
    pub exclusive: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MixerDock {
    Left,
    #[default]
    Middle,
    Right,
}
impl MixerDock {
    pub fn is_middle(&self) -> bool {
        *self == Self::Middle
    }
}
impl ExternalOutputRoute {
    pub fn check(&self) -> Result<(), String> {
        if self.left >= 256
            || self
                .right
                .is_some_and(|right| right >= 256 || right == self.left)
        {
            return Err("Choose distinct hardware output ports below 256, or mono".into());
        }
        Ok(())
    }
}

/// A saved hardware route. Device availability is resolved when opening input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioInputRoute {
    pub host: String,
    pub device: String,
    pub left: u16,
    pub right: Option<u16>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MixerRecording {
    pub input: Option<AudioInputRoute>,
    pub armed: bool,
    pub monitor: bool,
    pub monitor_gain: f32,
    pub monitor_buffer_ms: u16,
    pub offset_ms: f64,
    #[serde(default, skip_serializing_if = "MixerRecordMode::is_input")]
    #[ts(as = "Option<MixerRecordMode>", optional)]
    pub mode: MixerRecordMode,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MixerRecordMode {
    #[default]
    Input,
    PostEffects,
    PostFader,
}
impl MixerRecordMode {
    pub fn is_input(&self) -> bool {
        *self == Self::Input
    }
}
impl Default for MixerRecording {
    fn default() -> Self {
        Self {
            input: None,
            armed: false,
            monitor: false,
            monitor_gain: 0.5,
            monitor_buffer_ms: 20,
            offset_ms: 0.0,
            mode: MixerRecordMode::Input,
        }
    }
}
impl MixerRecording {
    pub fn check(&self) -> Result<(), String> {
        if !self.monitor_gain.is_finite()
            || !(0.0..=1.0).contains(&self.monitor_gain)
            || !(5..=100).contains(&self.monitor_buffer_ms)
        {
            return Err("monitor gain must be 0–1 and buffering 5–100 ms".into());
        }
        if !self.offset_ms.is_finite() || self.offset_ms.abs() > 1000.0 {
            return Err("recording offset must be -1000 to 1000 ms".into());
        }
        if let Some(input) = &self.input {
            if input.host.trim().is_empty()
                || input.device.trim().is_empty()
                || input.host.len() > 1024
                || input.device.len() > 4096
            {
                return Err("input host and device must have usable names".into());
            }
            if input.left >= 256 || input.right.is_some_and(|right| right >= 256) {
                return Err("hardware input channels must be below 256".into());
            }
        }
        Ok(())
    }
}

impl MixerTrack {
    pub fn effect(&self, id: EffectId) -> Option<&EffectSlot> {
        self.effects.iter().find(|effect| effect.id == id)
    }
}

/// One effect on a mixer track.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EffectSlot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub impulse: Option<crate::ConvolverImpulse>,
    pub id: EffectId,
    /// Off lets the signal pass untouched. Switching crossfades, so it
    /// never clicks, and an effect that is off costs no CPU.
    pub enabled: bool,
    /// Balance between the untouched signal (0) and the effect's output
    /// (1). A new effect starts at 1.
    pub mix: f32,
    /// Which effect this is and its settings, with every value inside its
    /// range. The kind never changes for the life of the slot.
    pub params: EffectParams,
}

impl EffectSlot {
    pub fn kind(&self) -> EffectKind {
        self.params.kind()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Send {
    pub target: TrackId,
    /// Linear gain, 0 to [`MAX_GAIN`].
    pub gain: f32,
}

/// The song timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Playlist {
    /// Saved arrangement references and grouping metadata.
    #[serde(default, skip_serializing_if = "crate::ArrangementBook::is_empty")]
    #[ts(as = "Option<crate::ArrangementBook>", optional)]
    pub arrangement_book: crate::ArrangementBook,
    /// Lanes of the timeline, in display order.
    pub tracks: Vec<PlaylistTrack>,
    /// Sorted by `start`, then `id`.
    pub clips: Vec<Clip>,
    /// Song meter changes and navigation markers. Absent in legacy v1 files.
    #[serde(default, skip_serializing_if = "crate::Timeline::is_empty")]
    #[ts(as = "Option<crate::Timeline>", optional)]
    pub timeline: crate::Timeline,
    /// Retained recording passes and the latest editable composite.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::AudioTakeGroup>>", optional)]
    pub take_groups: Vec<crate::AudioTakeGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaylistTrack {
    pub id: PlaylistTrackId,
    pub name: String,
    pub muted: bool,
    /// Limits playlist playback to solo tracks; mute still wins.
    #[serde(default, skip_serializing_if = "playlist_track_solo_is_false")]
    #[ts(as = "Option<bool>", optional)]
    pub solo: bool,
    /// 0 means uncolored; other values are 0xRRGGBB.
    #[serde(default, skip_serializing_if = "playlist_track_color_is_zero")]
    #[ts(as = "Option<u32>", optional)]
    pub color: u32,
    /// Row height in CSS pixels; 0 follows the global row height.
    #[serde(default, skip_serializing_if = "playlist_track_height_is_zero")]
    #[ts(as = "Option<u32>", optional)]
    pub height: u32,
}

fn playlist_track_solo_is_false(solo: &bool) -> bool {
    !*solo
}

fn playlist_track_color_is_zero(color: &u32) -> bool {
    *color == 0
}

fn playlist_track_height_is_zero(height: &u32) -> bool {
    *height == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Clip {
    pub id: ClipId,
    pub track: PlaylistTrackId,
    /// Start on the timeline in ticks.
    pub start: u32,
    /// Length in ticks, at least 1. The clip ends at or before
    /// [`MAX_SONG_TICKS`].
    pub length: u32,
    /// How far into its content the clip starts, in ticks, at most
    /// [`MAX_SONG_TICKS`]. A pattern clip loops its pattern, so this is
    /// taken modulo the pattern length.
    pub offset: u32,
    pub muted: bool,
    pub content: ClipContent,
}

impl Clip {
    /// The order clips keep on the playlist.
    pub fn sort_key(&self) -> (u32, ClipId) {
        (self.start, self.id)
    }
}

/// What a clip plays.
/// Offline-prepared spectral quality for playlist audio clips.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ClipStretchQuality {
    Fast,
    #[default]
    Standard,
    High,
}

/// How a playlist audio clip changes time and pitch. Source files remain intact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
#[ts(export)]
pub enum ClipStretch {
    /// Existing tape playback: pitch also changes speed.
    #[default]
    Tape,
    /// Cached spectral rendering: ratio changes duration and pitch stays independent.
    Spectral {
        /// Output duration divided by source duration, from 0.25 to 4.
        ratio: f64,
        quality: ClipStretchQuality,
        /// Approximate voiced-spectrum formant preservation.
        formants: bool,
    },
}

impl ClipStretch {
    fn is_tape(&self) -> bool {
        matches!(self, Self::Tape)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ClipContent {
    #[serde(rename_all = "camelCase")]
    Pattern { pattern: PatternId },
    /// Plays an audio file straight onto the timeline: a vocal take, a
    /// loop, a riser. It is heard in song mode only.
    ///
    /// # Timing
    ///
    /// The audio starts on the clip's `start` tick and then runs at its
    /// own speed, in seconds. It does not follow the tempo. For every
    /// second of the song, `2^(pitch / 12)` seconds of the file go by,
    /// read at the file's own sample rate, so a file plays in tune at any
    /// device rate.
    ///
    /// The clip's `offset` skips the beginning of the audio: the clip
    /// starts as far into it as would have played by then, had the clip
    /// begun `offset` ticks earlier at the project's tempo. In seconds of
    /// the file that is `offset * 60 / (tempoBpm * 960) * 2^(pitch / 12)`,
    /// In spectral mode the original source advances at `1 / ratio` instead,
    /// independently of pitch; the prepared source plays at unity speed.
    /// with `tempoBpm` the tempo stored in the project's settings. Dragging
    /// the clip's left edge to the right by `n` ticks (`start + n`,
    /// `offset + n`, `length - n`) therefore leaves the rest of the audio
    /// where it was. A reversed clip is skipped into from the end of the
    /// file, which is where it starts playing.
    ///
    /// The clip ends on tick `start + length`, or sooner if the audio runs
    /// out first. `length` is an upper bound that can cut the audio short,
    /// never a stretch. The natural length of a clip, the one that ends
    /// exactly with the audio, is
    /// `duration / 2^(pitch / 12) * tempoBpm * 16 - offset` ticks, with
    /// `duration` the file's length in seconds: 960 ticks a beat are 16
    /// ticks a second for each beat a minute.
    ///
    /// Changing the tempo leaves `start`, `length` and `offset` as they
    /// are, in ticks. The clip still starts on the same beat and the audio
    /// still takes as many seconds as it did, so at a faster tempo it
    /// reaches further along the timeline and `length` cuts off more of
    /// its end, and at a slower tempo it ends before the clip does. What
    /// `offset` skips is as many beats as before, which is fewer or more
    /// seconds.
    ///
    /// # Level
    ///
    /// The clip fades in from silence over the `fade_in` ticks after its
    /// start and out to silence over the `fade_out` ticks before tick
    /// `start + length`. Both fades are equal-power curves (a quarter of a
    /// sine wave), so two clips that fade into each other keep their
    /// loudness through the overlap. Apart from them, the engine fades
    /// every clip in and out over 3 ms wherever it starts or stops, so a
    /// clip cut in the middle of a wave does not click.
    #[serde(rename_all = "camelCase")]
    Audio {
        /// The audio to play. The sample cannot be removed from the pool
        /// while a clip uses it.
        sample: SampleId,
        /// The mixer track the clip plays into, through its effects, its
        /// fader and its sends.
        mixer_track: TrackId,
        /// Mixer routing, or a finished print summed after the Master strip.
        #[serde(default, skip_serializing_if = "ClipAudioOutput::is_mixer")]
        #[ts(as = "Option<ClipAudioOutput>", optional)]
        output: ClipAudioOutput,
        /// Peak normalization applied during plan construction; the gain knob stays unchanged.
        #[serde(default, skip_serializing_if = "audio_clip_normalize_is_false")]
        #[ts(as = "Option<bool>", optional)]
        normalize: bool,
        /// Linear gain, 0 to [`MAX_GAIN`].
        gain: f32,
        /// -1 is hard left, 1 is hard right.
        pan: f32,
        /// Length of the fade in, in ticks from the clip's start. At most
        /// [`MAX_SONG_TICKS`]; one that is longer than the clip never
        /// reaches full level.
        fade_in: u32,
        /// Length of the fade out, in ticks before the clip's end. At most
        /// [`MAX_SONG_TICKS`].
        fade_out: u32,
        /// Plays the audio backwards, from its last frame.
        reverse: bool,
        /// Pitch in semitones, -48 to 48 for tape, -24 to 24 for spectral.
        /// Tape changes speed and pitch together: 12 plays an octave up
        /// in half the time. Spectral pitch does not change duration.
        pitch: f32,
        /// Defaults to tape for older projects. Spectral mode supports +/-24 semitones.
        #[serde(default, skip_serializing_if = "ClipStretch::is_tape")]
        #[ts(as = "Option<ClipStretch>", optional)]
        stretch: ClipStretch,
    },
    /// Puts an [`Automation`] on the timeline. While the song plays through
    /// the clip, the automation's target follows its curve. It does
    /// nothing in pattern mode.
    ///
    /// The clip is a window onto the curve: tick `start` of the song is
    /// tick `offset` of the curve, and the clip shows `length` ticks of it.
    /// A curve does not loop. Past its last point it stays on that point's
    /// value. Several clips can show the same automation, at different
    /// places and with different windows.
    ///
    /// # What the target does
    ///
    /// While the song plays, the value of a target at any place in the
    /// song depends on that place alone, not on how playback got there:
    ///
    /// - Inside a clip the target follows the curve.
    /// - After a clip the target holds the value the curve has at the
    ///   clip's end, until another clip of that target begins, and after
    ///   its last clip to the end of the song.
    /// - Before the first clip of a target it has the value stored in the
    ///   project.
    ///
    /// Playback that begins in the middle of the song therefore finds
    /// every target where it would be had the song played from the start,
    /// and a song that loops begins each time around from the values that
    /// playing from the start gives.
    ///
    /// A song that stops, at its end or because it is told to, leaves its
    /// targets where they were for as long as anything is still sounding,
    /// so that a fade out keeps a reverb's tail faded. They return to
    /// their stored values once that has rung out, or sooner when the
    /// transport is used again or something is played by hand.
    ///
    /// Where clips of one target overlap, whether they show one automation
    /// or several, one of them wins: the clip on the playlist track nearest
    /// the top, then the clip that starts later, then the clip with the
    /// lower id. A muted clip, or one on a muted playlist track, counts for
    /// nothing, neither while it lasts nor afterwards.
    ///
    /// Automation is never written into the project. The stored value of a
    /// target is what its fader or knob shows when no automation has it in
    /// hand.
    #[serde(rename_all = "camelCase")]
    Automation { automation: AutomationId },
}

fn audio_clip_normalize_is_false(normalize: &bool) -> bool {
    !*normalize
}

/// Playback destination for audio clips. Direct prints retain their own gain,
/// pan, fades and playlist mute, and bypass all mixer strips and effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ClipAudioOutput {
    #[default]
    Mixer,
    Direct,
}

impl ClipAudioOutput {
    pub fn is_mixer(&self) -> bool {
        *self == Self::Mixer
    }
}

impl ClipContent {
    /// The sample an audio clip plays. `None` for any other clip.
    pub fn sample(&self) -> Option<SampleId> {
        match self {
            ClipContent::Audio { sample, .. } => Some(*sample),
            ClipContent::Pattern { .. } | ClipContent::Automation { .. } => None,
        }
    }

    /// The automation an automation clip shows. `None` for any other clip.
    pub fn automation(&self) -> Option<AutomationId> {
        match self {
            ClipContent::Automation { automation } => Some(*automation),
            ClipContent::Pattern { .. } | ClipContent::Audio { .. } => None,
        }
    }
}

/// A curve that moves one thing in the project while the song plays: a
/// filter sweep, a volume ride, a send that opens for one word.
///
/// An automation does nothing by itself. Clips on the playlist
/// ([`ClipContent::Automation`]) say where in the song it plays, and
/// [`ClipContent::Automation`] says what the target does there and
/// elsewhere. Several automations may have the same target.
///
/// An automation lives as long as its target does: deleting the channel,
/// mixer track, send or effect it moves deletes the automation and its
/// clips with it, and undo brings all of it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Automation {
    pub id: AutomationId,
    pub name: String,
    /// Display color as 0xRRGGBB.
    pub color: u32,
    pub target: AutomationTarget,
    /// The curve: at least one point and at most
    /// [`MAX_AUTOMATION_POINTS`], in order of their ticks. Two points may
    /// share a tick, which makes the curve jump there.
    /// [`curve_value`](crate::automation::curve_value) reads it.
    pub points: Vec<AutomationPoint>,
}

/// One point of an automation curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutomationPoint {
    /// Ticks from the start of the curve, at most [`MAX_SONG_TICKS`].
    pub tick: u32,
    /// 0 to 1 across the target's range. What that is in the target's own
    /// unit depends on the target: see [`AutomationTarget`] and
    /// [`AutomationRange`](crate::automation::AutomationRange).
    pub value: f32,
    /// How the curve bends on its way from this point to the next, from
    /// -1 to 1, with 0 a straight line. A positive value holds back and
    /// catches up at the end, a negative one moves fast first and settles:
    /// [`curve_shape`](crate::automation::curve_shape) has the formula.
    #[serde(default)]
    pub curve: f32,
    /// Makes the curve a step: it stays on this point's value until the
    /// next point and jumps there.
    #[serde(default)]
    pub hold: bool,
}

/// What an automation moves. Each kind says what a point's value of 0 to 1
/// means for it; [`Project::automation_range`] gives the same as numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AutomationTarget {
    /// A channel's volume: linear gain `2 * value * value`, so 0 is
    /// silence, 0.7071 is 0 dB and 1 is +6 dB ([`MAX_GAIN`]). A channel
    /// that is muted, or silenced by another channel's solo, stays silent.
    #[serde(rename_all = "camelCase")]
    ChannelVolume {
        channel: ChannelId,
    },
    /// A channel's pan: `2 * value - 1`, from hard left at 0 through the
    /// center at 0.5 to hard right at 1.
    #[serde(rename_all = "camelCase")]
    ChannelPan {
        channel: ChannelId,
    },
    /// A mixer track's fader, the master's included: linear gain
    /// `2 * value * value`, as for a channel's volume. A track that is
    /// muted, or silenced by another track's solo, stays silent.
    #[serde(rename_all = "camelCase")]
    TrackVolume {
        track: TrackId,
    },
    /// A mixer track's pan: `2 * value - 1`.
    #[serde(rename_all = "camelCase")]
    TrackPan {
        track: TrackId,
    },
    #[serde(rename_all = "camelCase")]
    TrackParam {
        track: TrackId,
        param: u32,
    },
    /// The level of the send from `track` to `target`: linear gain
    /// `2 * value * value`. The send has to exist.
    #[serde(rename_all = "camelCase")]
    SendGain {
        track: TrackId,
        target: TrackId,
    },
    SidechainGain {
        track: TrackId,
        target: TrackId,
    },
    /// One setting of an effect. `param` is its index in the descriptors
    /// of the effect's kind, and the value maps onto the descriptor's
    /// `min` to `max`: linearly, or in equal ratios
    /// (`min * (max / min)^value`) where the descriptor's scale is
    /// logarithmic. A whole number or a choice takes the nearest step, and
    /// a toggle is on from 0.5 up. `track` is the track the effect is on,
    /// and follows the effect when it is moved to another.
    #[serde(rename_all = "camelCase")]
    EffectParam {
        track: TrackId,
        effect: EffectId,
        param: u32,
    },
    /// The mix of an effect slot: 0 is the untouched signal and 1 is the
    /// effect alone.
    #[serde(rename_all = "camelCase")]
    EffectMix {
        track: TrackId,
        effect: EffectId,
    },
    /// One setting of a channel's instrument, addressed and mapped like
    /// [`AutomationTarget::EffectParam`].
    #[serde(rename_all = "camelCase")]
    InstrumentParam {
        channel: ChannelId,
        param: u32,
    },
    /// The tempo: `10 + 512 * value` beats per minute, from
    /// [`MIN_TEMPO_BPM`] to [`MAX_TEMPO_BPM`], so 120 bpm is 0.21484375.
    /// It moves the song's own clock: notes, clip edges and the playhead
    /// all follow it. Audio clips keep their own speed.
    Tempo,
}
