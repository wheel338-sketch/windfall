//! Standard MIDI Files for Windfall.
//!
//! [`read`] accepts formats 0, 1 and 2, with PPQ or SMPTE time, into a
//! [`MidiSong`] at 960 ticks per quarter note. PPQ scaling uses exact
//! rational arithmetic with each absolute time rounded to the nearest tick,
//! halves up; note lengths are differences of rounded endpoints, at least
//! one tick. Format 2 sequences are placed consecutively. See [`read`] for
//! SMPTE conversion and note pairing rules.
//!
//! [`import`] builds a reviewable [`ImportPlan`]. Dispatch
//! `plan.command(document.project())` immediately to apply it as one undo
//! step. Each track/channel with notes becomes a default subtractive synth,
//! a playlist track and patterns. Long parts split at pattern limits and
//! notes crossing a split retrigger. [`PatternStrategy::Bars`] can share
//! identical sections. Channel 10 stays a synth unless a [`DrumKit`] maps
//! its keys to samples. Limits and unsupported data appear in
//! [`ImportPlan::adjustments`]. Ordered signature events set the song meter map;
//! tempo changes become stepped automation. Other controllers, program
//! changes, bend, pressure, markers and key signatures remain in the MIDI
//! representation but are not imported into the project. Only the first
//! channel volume and pan are imported.
//!
//! [`export_song`] expands playlist clips with offsets, loops and playlist
//! mutes, preserving swing by default; [`export_pattern`] exports one pass.
//! Tempo curves become sampled MIDI steps, with exact events at jumps.
//! MIDI cannot reproduce audio clips, synth settings, effects, mixer
//! routing, per-note pan or sampler tuning. Channel mutes are mixing
//! decisions and do not discard notes. MIDI channels are reused after the
//! fifteenth melodic track. [`write`] emits deterministic format 1 by
//! default, or format 0, at 960 or 480 PPQ, optionally with running status.
//! Write/read equality is equality to [`MidiSong::normalized`]; ambiguous
//! overlapping same-key notes are paired in first-in, first-out order.
//!
//! Core operations only use memory and compile for WebAssembly. [`read_file`]
//! and [`write_file`] are thin host filesystem wrappers. The reader caps
//! input at 16 MiB, imports at one million laid-out notes, and exports at
//! four million notes. No third-party MIDI parser or writer is used.

pub mod error;
pub mod export;
pub mod expression;
pub mod file;
pub mod gm;
pub mod import;
mod import_expression;
mod mix;
pub mod read;
pub mod score;
pub mod song;
pub mod write;

pub use error::MidiError;
pub use export::{
    ExportError, ExportOptions, Exported, MAX_EXPORTED_NOTES, MAX_EXPRESSION_MESSAGES,
    export_pattern, export_pattern_reported, export_song, export_song_reported,
};
pub use expression::{ExportReport, ExpressionMode, ExpressionOptions};
pub use file::{read_file, write_file};
pub use gm::{DrumKit, DrumPad, program_name};
pub use import::{
    Adjustment, ImportOptions, ImportPlan, MAX_IMPORTED_NOTES, PatternStrategy, PlannedChannel,
    PlannedClip, PlannedLane, PlannedPattern, PlannedSample, PlannedSound, Unsupported, import,
};
pub use read::{MAX_FILE_BYTES, read};
pub use score::{score_from_song, score_to_song};
pub use song::*;
pub use write::{Resolution, SmfFormat, WriteOptions, write};
