//! The Windfall project model, edit commands, undo history and file format.
//!
//! One [`Document`] holds the single copy of a project that the UI and the
//! audio engine both follow. Every edit is a [`Command`] dispatched to it.

pub mod arrangement;
pub mod audio_analysis;
pub use audio_analysis::*;
pub use arrangement::ArrangementBook;
mod audio_comp;
pub mod automation;
pub use audio_comp::AudioCompSegment;
mod take_groups;
pub use take_groups::{AudioTakeGroup, AudioTakeLane, AudioTakeRef, TakeCompRange};
pub mod curve_lfo;
pub use curve_lfo::{CurveLfo, CurveLfoWave};
pub mod channel_voice;
pub use channel_voice::*;
pub mod controller_links;
pub mod internal_controllers;
pub mod layer;
pub use controller_links::{
    ControlAction, ControlMap, ControlSource, ControllerLink, ControllerProfile, LinkTarget,
    MidiControl, PickupMode, ProfileControl, SlotTarget, SourceMode,
};
pub use internal_controllers::{
    ControlPoint, InternalControllerSettings, SurfaceControl, SurfaceControlKind, VoltageRoute,
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
mod note_curve_lfo;
pub mod note_curves;
pub mod note_timing;
pub use note_curves::{NoteCurveInsert, NoteCurveParameter, NoteCurvePoint, NoteExpressionCurve};
pub mod patch;
pub mod pattern_timeline;
pub mod piano_tools;
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
pub use patch::*;
pub use piano_tools::{NoteEdge, NoteGroove, NoteTransform};
pub use windfall_dsp::{
    ExpressionDestination, ExpressionMap, ExpressionRoute, PluginExpressionDestination,
};
pub use windfall_dsp::{NoteArticulation, NoteExpression};
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

mod convolver;
pub use convolver::ConvolverImpulse;
pub mod sample_playback;
pub use sample_playback::{KeyPlaybackCurve, SamplePlayback};

mod legacy_sample_playback;
pub use sample_playback::InlinePlaybackSource;
pub mod rack;
pub use rack::{
    ChannelRack, RackEdge, RackEndpoint, RackGraph, RackNode, RackPreset, RackProcessor,
};
pub mod assistant;
pub mod assistant_protocol;
pub mod proposal;
pub mod scripting;
pub mod sample_instrument;
pub use sample_instrument::SampleInstrumentAsset;

pub use score::ScoreEventLane;
pub mod stage_media;
pub use stage_media::{StageMedia, PerformancePad, VideoPlacement, VisualSettings, VisualScene, LaunchQuantization};

pub mod effect_examples;
