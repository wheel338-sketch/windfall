//! Windfall's Model Context Protocol server, without its sockets.
//!
//! An AI agent connected over MCP reads the open project through the read
//! tools, and changes it only by filing [`CommandProposal`]s: the same
//! checked, bounded, undoable batches the built-in assistant makes. The
//! user applies or rejects each in Windfall, or lets a trusted agent apply
//! its own. docs/MCP.md describes it for users.
//!
//! - [`protocol`]: JSON-RPC and the MCP methods, both protocol eras.
//! - [`http`]: the Streamable HTTP endpoint's checks and wire format.
//! - [`bridge`]: the stdio bridge a client starts as a command.
//! - [`tools`]: the tools; [`summary`], [`edits`] and [`listen`] are what
//!   they do, [`music`] reads positions and pitches.
//! - [`approvals`]: proposals waiting for the user.
//! - [`Host`]: what the app gives the server.
//!
//! [`CommandProposal`]: windfall_project::proposal::CommandProposal

pub mod approvals;
mod args;
pub mod bridge;
pub mod edits;
mod host;
pub mod http;
pub mod library;
pub mod listen;
pub mod music;
mod plugins;
pub mod protocol;
pub mod summary;
pub mod tools;

pub use approvals::{Approvals, Approver, Entry, Preview, Status};
pub use host::{
    ChannelPresetInfo, Focus, Host, LibraryFile, LibraryPage, NO_FILES, NativeEdit, PluginInfo,
    RenderRegion, RenderRequest, RenderedAudio, Snapshot, SoundFontPreset, Transport,
};
pub use protocol::Server;

#[cfg(test)]
mod tests;
