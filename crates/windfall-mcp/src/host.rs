//! What the protocol layer asks of the app it runs in. The desktop shell
//! implements [`Host`] over its session; the tests implement it over a
//! bare [`Document`].
//!
//! Nothing here hands out mutable access to the project. The one way a
//! host changes it is [`Host::apply`], with a [`CommandProposal`] the
//! proposal registry has narrowed, as the built-in assistant does.

use windfall_project::proposal::{CommandProposal, ProposalStamp};
use windfall_project::{ChannelId, Document, PatternId, PlayMode, TrackId};

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
}

pub trait Host: Send + Sync {
    fn snapshot(&self) -> Result<Snapshot, String>;

    fn focus(&self) -> Focus;

    /// Applies an approved proposal as one undo step through the same
    /// checked path the assistant's review uses. Returns the revision the
    /// project has afterwards.
    fn apply(&self, proposal: CommandProposal) -> Result<u64, String>;

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
