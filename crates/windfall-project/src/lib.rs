//! The Windfall project model, edit commands, undo history and file format.
//!
//! One [`Document`] holds the single copy of a project that the UI and the
//! audio engine both follow. Every edit is a [`Command`] dispatched to it.

pub mod arrangement;
pub use arrangement::ArrangementBook;
pub mod automation;
mod audio_comp;
pub use audio_comp::AudioCompSegment;
mod take_groups;
pub use take_groups::{AudioTakeGroup, AudioTakeLane, AudioTakeRef, TakeCompRange};
pub mod curve_lfo;
pub use curve_lfo::{CurveLfo, CurveLfoWave};
pub mod channel_voice;
pub use channel_voice::*;
pub mod layer;
pub mod controller_links;
pub use controller_links::{
    ControlAction, ControlMap, ControlSource, ControllerLink, ControllerProfile, LinkTarget,
    MidiControl, PickupMode, ProfileControl, SlotTarget, SourceMode,
};
mod check;
pub mod command;
pub mod document;
mod edit;
pub mod error;
mod expression_map;
pub mod file;
mod lower;
mod mixer_preset;
pub use mixer_preset::MixerTrackPreset;
pub mod effect_outputs;
pub use effect_outputs::EffectOutputRoute;
pub mod model;
pub mod notebook;
pub use notebook::{Notebook, NotebookPage};
pub mod note_timing;
pub mod note_curves;
mod note_curve_lfo;
pub use note_curves::{NoteCurveInsert, NoteCurveParameter, NoteCurvePoint, NoteExpressionCurve};
pub mod patch;
pub mod piano_tools;
pub mod pattern_timeline;
pub use pattern_timeline::PatternTimelineEdit;
pub mod pattern_events;
pub use pattern_events::{MAX_PATTERN_EVENT_LANES, PatternEventLane};
pub mod note_grid;
pub use note_grid::{NoteGridUnit, NoteMusicalGrid};
pub mod plugin;
pub mod slicer;
pub mod timeline;
pub use plugin::*;
pub use timeline::{
    MAX_MARKER_NAME_BYTES, MAX_TIMELINE_ITEMS, MarkerKind, MeterChange, MeterChangeId, MeterMap,
    MusicalPosition, TickRange, Timeline, TimelineMarker, TimelineMarkerId,
};

pub use automation::{AutomationRange, AutomationTaper, curve_shape, curve_value};
pub use command::*;
pub use document::{Applied, Document};
pub use error::CommandError;
pub use file::{LoadError, ProjectSession, SaveError};
pub use model::*;
pub use note_timing::{ChannelTiming, MAX_CHANNEL_SHIFT_TICKS};
pub use windfall_dsp::{NoteArticulation, NoteExpression};
pub use windfall_dsp::{
    ExpressionDestination, ExpressionMap, ExpressionRoute, PluginExpressionDestination,
};
pub use patch::*;
pub use piano_tools::{NoteEdge, NoteGroove, NoteTransform};
pub mod preset;
pub use preset::{
    ChannelPreset, PluginPreset, PluginPresetState, Preset, PresetAsset, PresetAudio, PresetKind,
    PresetPluginIdentity,
};
mod project_settings;
pub use project_settings::PanLaw;
pub mod score;
pub use score::{Score, ScoreLoadMode, ScoreNote};
pub mod timebase;
pub mod rack;
pub use rack::{ChannelRack, RackEdge, RackEndpoint, RackGraph, RackNode, RackPreset, RackProcessor};
