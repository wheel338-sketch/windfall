//! The MCP server's settings and its approval panel, as the interface sees
//! them. The server is off until the user turns it on; it listens on the
//! loopback address only.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{ChannelId, Command, TrackId};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpSettings {
    pub enabled: bool,
    /// Listening right now.
    pub running: bool,
    pub port: u16,
    /// The install's bearer token, shown so the user can give it to a
    /// client. It never leaves this computer otherwise.
    pub token: String,
    /// The endpoint, `http://127.0.0.1:{port}/mcp`.
    pub url: String,
    /// The Windfall executable, which the stdio bridge runs as.
    pub executable: String,
    /// Why the server is not listening although it is turned on.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct McpSettingsUpdate {
    pub enabled: bool,
    pub port: u16,
}

/// How many of one kind of thing a proposal leaves, before and after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpCount {
    /// channels, patterns, notes, mixerTracks, effects, playlistTracks,
    /// clips, automations, tempoBpm or meter.
    pub kind: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpProposal {
    pub id: String,
    pub agent: String,
    /// The undo step's name: the agent's name, then what it does.
    pub label: String,
    /// The agent's own words, as it wrote them.
    pub explanation: String,
    /// pending, applying, applied, rejected, withdrawn or failed.
    pub status: String,
    pub error: Option<String>,
    /// The project changed since the proposal was made.
    pub stale: bool,
    /// Applied without asking, because the user allowed the agent to.
    pub automatic: bool,
    pub counts: Vec<McpCount>,
    /// The exact checked commands, for review.
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpAgent {
    pub name: String,
    /// May apply its own proposals without asking, until Windfall quits
    /// or the server is turned off.
    pub trusted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpPanel {
    pub proposals: Vec<McpProposal>,
    pub agents: Vec<McpAgent>,
    /// An agent's render is running.
    pub rendering: bool,
}

/// What the user is looking at, reported while the server is on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct McpFocus {
    pub channel: Option<ChannelId>,
    pub track: Option<TrackId>,
    pub view: Option<String>,
}
