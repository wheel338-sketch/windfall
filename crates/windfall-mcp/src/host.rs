//! What the protocol layer asks of the app it runs in. The desktop shell
//! implements [`Host`] over its session; the tests implement it over a
//! bare [`Document`].
//!
//! Nothing here hands out mutable access to the project. The one way a
//! host changes it is [`Host::apply`], with a [`CommandProposal`] the
//! proposal registry has narrowed, as the built-in assistant does.

use serde::Serialize;
use windfall_project::proposal::{CommandProposal, ProposalStamp};
use windfall_project::{ChannelId, Document, PatternId, PlayMode, TrackId};

pub use windfall_dsp::sample_bank::soundfont::SoundFontPreset;

/// The project as it is at one moment, with the stamp a proposal made
/// from it carries.
#[derive(Clone)]
pub struct Snapshot {
    pub document: Document,
    pub stamp: ProposalStamp,
    /// True while audio or a performance is being recorded: proposals are
    /// refused until it stops.
    pub recording: bool,
    pub transport: Transport,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    pub playing: bool,
    pub mode: PlayMode,
    /// The pattern the step sequencer and piano roll are editing, and the
    /// one that plays in pattern mode.
    pub pattern: PatternId,
}

/// What the user is looking at, as the interface last reported it. The
/// interface only reports while the MCP server is on.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub channel: Option<ChannelId>,
    pub track: Option<TrackId>,
    /// The view in the center of the window: "playlist", "pianoRoll", ...
    pub view: Option<String>,
}

/// The part of the project a render plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderRegion {
    /// The playlist from its start to the end of its last clip.
    Song,
    /// Song ticks `start..end`.
    Range { start: u32, end: u32 },
    /// One pattern, looped.
    Pattern { pattern: PatternId, loops: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    pub region: RenderRegion,
    /// Renders this mixer track alone, as its solo button does.
    pub solo: Option<TrackId>,
    /// Longest audio the render may make. Longer regions are refused
    /// before any audio is made.
    pub max_seconds: f64,
}

/// Interleaved stereo.
pub struct RenderedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// The WAV file the render was kept in, inside a folder Windfall owns.
    pub path: Option<String>,
    /// Channels that have notes in the rendered region, are not muted, and
    /// made no sound when rendered alone.
    pub silent_channels: Vec<ChannelId>,
}

/// A file in the browser's folders, as `search_library` lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFile {
    pub path: String,
    /// "audio", "soundFont", "project", "preset" or "other".
    pub kind: String,
    pub bytes: u64,
    /// The path inside its browser folder, with `/`.
    pub relative_path: String,
}

/// One page of a library search.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPage {
    pub total: usize,
    pub items: Vec<LibraryFile>,
    /// The browser is still reading its folders: more may match later.
    pub indexing: bool,
    /// More files matched than a search returns.
    pub truncated: bool,
}

/// A channel preset Windfall can load onto a channel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPresetInfo {
    pub name: String,
    pub path: String,
    /// "factory" or "library" (the user's own).
    pub origin: String,
    /// The instrument it plays ("sampler" for a sampler preset).
    pub instrument: String,
}

/// What a proposal does besides its commands: an edit only Windfall itself
/// can make, because it reads a file. The agent names the file; Windfall
/// checks it is inside the browser's folders before the proposal is filed,
/// and reads it only when the user applies it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NativeEdit {
    /// Load one preset of a SoundFont into a channel's SoundFont player.
    /// With `replace_instrument` the channel's instrument becomes a
    /// SoundFont player first, in the same undo step.
    #[serde(rename_all = "camelCase")]
    SoundFont {
        channel: ChannelId,
        path: String,
        bank: u16,
        program: u16,
        preset: String,
        replace_instrument: bool,
    },
    /// Load a channel preset file onto a channel.
    #[serde(rename_all = "camelCase")]
    ChannelPreset {
        channel: ChannelId,
        path: String,
        name: String,
    },
}

/// The answer of a host that has no files to offer.
pub const NO_FILES: &str = "this Windfall offers no files to agents";

pub trait Host: Send + Sync {
    fn snapshot(&self) -> Result<Snapshot, String>;

    fn focus(&self) -> Focus;

    /// Applies an approved proposal as one undo step through the same
    /// checked path the assistant's review uses. Returns the revision the
    /// project has afterwards.
    fn apply(&self, proposal: CommandProposal) -> Result<u64, String>;

    /// Applies an approved proposal whose edit Windfall makes itself: the
    /// proposal's commands (which may be none) and `edit`, together as one
    /// undo step, only if the project is still as the proposal's stamp
    /// says.
    fn apply_native(&self, proposal: CommandProposal, edit: NativeEdit) -> Result<u64, String> {
        let _ = (proposal, edit);
        Err(NO_FILES.into())
    }

    /// The file an agent named, if it is a file inside one of the
    /// browser's folders or Windfall's factory content, as an absolute
    /// path with every link resolved. Anything else is refused.
    fn library_file(&self, path: &str) -> Result<String, String> {
        let _ = path;
        Err(NO_FILES.into())
    }

    /// The presets of a SoundFont that [`library_file`](Self::library_file)
    /// allowed.
    fn soundfont_presets(&self, path: &str) -> Result<Vec<SoundFontPreset>, String> {
        let _ = path;
        Err(NO_FILES.into())
    }

    /// Searches the browser's folders with the browser's own search. `kind`
    /// keeps only files of that kind.
    fn search_library(
        &self,
        query: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<LibraryPage, String> {
        let _ = (query, kind, offset, limit);
        Err(NO_FILES.into())
    }

    /// The channel presets of the factory content and the user's library.
    fn channel_presets(&self) -> Result<Vec<ChannelPresetInfo>, String> {
        Err(NO_FILES.into())
    }

    /// Renders offline. `cancelled` is polled while rendering; once it
    /// returns true the render stops with an error.
    fn render(
        &self,
        request: RenderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RenderedAudio, String>;

    /// The list of proposals, or whether agents may apply without asking,
    /// changed. The desktop tells its window.
    fn proposals_changed(&self) {}

    /// Unpredictable text for session ids: hex digits.
    fn random_hex(&self) -> String;
}
