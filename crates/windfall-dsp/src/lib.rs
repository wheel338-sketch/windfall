//! Windfall's built-in signal processing: the effects and instruments that
//! ship with the app, and the blocks they are made of.
//!
//! This crate knows nothing about audio devices, projects or the engine.
//! It turns numbers into numbers, which is what lets every processor be
//! tested by measurement.
//!
//! # The plugin interface
//!
//! Every effect implements [`Effect`] and every instrument implements
//! [`Instrument`]. A host that picks processors at run time holds them as
//! [`AnyEffect`] (usually inside an [`EffectSlot`], which adds bypass and
//! mix) and [`AnyInstrument`], with their settings in the matching
//! [`EffectParams`] and [`InstrumentParams`].
//!
//! The rules are the same for all of them:
//!
//! - **Allocation.** Building a processor and calling `prepare` allocate.
//!   Nothing else does: `process`, `set_params`, `set_tempo`, `reset` and
//!   the note calls are safe on a realtime thread. Dropping a processor
//!   frees memory, so the host sends it back to another thread to be
//!   dropped.
//! - **Parameters** are plain structs, one per processor, that can be
//!   saved, copied to the audio thread and exported to TypeScript. Each has
//!   sensible defaults, forces whatever it is given into range, and comes
//!   with a table of [`ParamInfo`] that describes every control for the UI
//!   and for automation (see [`ParamSet`]). A change glides, so it never
//!   clicks.
//! - **Blocks.** A processor's output does not depend on how the host
//!   divides the audio into blocks, and any block length from one sample up
//!   is fine.
//! - **Latency and tails** are reported, so a host can line tracks up and
//!   knows when a track has gone silent.
//! - **Bypass and mix.** No effect has a bypass of its own: switching an
//!   effect off is the [`EffectSlot`]'s job, which crossfades and then stops
//!   running it. The compressor, reverb and delay each have a `mix` control
//!   because blending them with the dry signal is part of how they are
//!   used; the slot adds a second, generic mix for every effect, with the
//!   dry signal delayed to match the effect's latency.
//!
//! # Units
//!
//! Levels a mixer shares are linear gain, where 1.0 is unity. Thresholds
//! and gains inside dynamics and equalisers are in dB, because that is how
//! they are thought about. Frequencies are in Hz, times in milliseconds
//! (reverb decay in seconds), mixes and amounts run from 0 to 1, and pan
//! from -1 to 1. Each field's doc comment gives its unit, range and
//! default.

pub mod additive;
pub mod asset;
mod balance;
pub mod blocks;
mod channel_mute;
mod compressor;
mod dc_block;
mod delay;
mod distortion;
pub mod drive;
pub mod drums;
pub mod echo_bank;
mod effect;
mod eq;
pub mod expression_map;
pub mod filter_family;
pub mod fm;
pub mod frequency_delay;
pub mod granular;
mod instrument;
mod limiter;
pub mod lofi;
pub mod modulation;
pub mod multiband;
mod note_expression;
mod param;
pub mod performance;
pub mod physical;
mod polarity;
mod reverb;
mod soft_clipper;
pub mod spatial;
pub mod spectral;
mod stereo_matrix;
mod synth;
mod track;
pub use track::{TrackParams, TrackProcessor};

pub use balance::{Balance, BalanceParams};
pub use blocks::lfo::LfoShape;
pub use blocks::oscillator::Waveform;
pub use channel_mute::{ChannelMute, ChannelMuteParams};
pub use compressor::{COMPRESSOR_MAX_RATIO, Compressor, CompressorParams, DetectorMode};
pub use dc_block::{DcBlock, DcBlockParams};
pub use delay::{Delay, DelayMode, DelayParams, NoteDivision};
pub use distortion::{DISTORTION_LATENCY_SAMPLES, Distortion, DistortionParams};
pub use effect::{AnyEffect, Effect, EffectKind, EffectParams, EffectSlot, GainReductionMeter};
pub use effect::{AuxBlock, AuxOutputInfo, MAX_AUX_OUTPUTS};
pub use eq::{CutSlope, EqBand, EqCutBand, EqParams, ParametricEq};
pub use expression_map::{
    ExpressionDestination, ExpressionMap, ExpressionRoute, PluginControls,
    PluginExpressionDestination, VoiceControls,
};
pub use filter_family::{
    BassShelf, BassShelfParams, FastLowpass, FastLowpassParams, SelectableFilter,
    SelectableFilterMode, SelectableFilterParams,
};
pub use instrument::{AnyInstrument, Instrument, InstrumentKind, InstrumentParams};
pub use limiter::{GATE_OFF_DB, Limiter, LimiterMode, LimiterParams};
pub use lofi::{FilterPlacement, Lofi, LofiParams, RunRelation};
pub use modulation::{Chorus, ChorusParams, Flanger, FlangerParams, Phaser, PhaserParams};
pub use note_expression::{NoteArticulation, NoteExpression, NoteInstanceId};
pub use param::{ParamChoice, ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit};
pub use polarity::{Polarity, PolarityParams};
pub use reverb::{Reverb, ReverbParams};
pub use soft_clipper::{SoftClipper, SoftClipperParams};
pub use stereo_matrix::{MATRIX_MAX_DELAY_MS, MatrixMode, StereoMatrix, StereoMatrixParams};
pub use synth::{
    EnvelopeParams, FilterMode, FilterParams, FilterSlope, LfoParams, MAX_POLYPHONY, MAX_UNISON,
    OscillatorParams, SubtractiveSynth, SynthParams, VoiceMode,
};

pub use additive::{
    HarmonicStack, HarmonicStackParams, Inharmonic, InharmonicParams, PartialMorph,
    PartialMorphParams, Resynth, ResynthParams, ScanSynth, ScanSynthParams, SeedPatch,
    SeedPatchParams,
};
pub use drive::{
    DriveChain, DriveChainParams, GuitarRack, GuitarRackParams, Overdrive, OverdriveParams,
    Waveshaper, WaveshaperParams,
};
pub use drums::{
    DrumRack, DrumRackParams, DrumVoice, DrumVoiceParams, Kick, KickParams, Membrane,
    MembraneParams,
};
pub use fm::{FourOp, FourOpParams, MatrixFm, MatrixFmParams, RingHybrid, RingHybridParams};
pub use multiband::{
    BandSplit, BandSplitParams, BassHarmonics, BassHarmonicsParams, Exciter, ExciterParams,
    MultibandCompressor, MultibandCompressorParams, MultibandMaximizer, MultibandMaximizerParams,
    OneKnob, OneKnobParams, TransientShaper, TransientShaperParams, TransientSplit,
    TransientSplitParams,
};
pub use physical::{
    AcousticString, AcousticStringParams, FingerBass, FingerBassParams, Pluck, PluckParams,
};
pub use spatial::{
    BandDelay, BandDelayParams, HyperChorus, HyperChorusParams, Room, RoomParams, Spreader,
    SpreaderParams, StackedFlanger, StackedFlangerParams, StereoEnhancer, StereoEnhancerParams,
    VintageChorus, VintageChorusParams, VintagePhaser, VintagePhaserParams,
};

pub mod control;

pub mod eqbank;

pub mod analog;

pub mod lush;

pub mod speech;

pub mod midi_out;
pub use midi_out::{MidiOut, MidiOutController, MidiOutEvent, MidiOutParams};

pub mod tuner;

pub mod observe;
pub use observe::{DynamicsObservation, EffectObservation, GATE_FLOOR_DB};

pub mod mastering;

pub mod surface;

pub mod sendtap;

pub mod notemap;

pub mod zones;

pub mod polysynth;
pub use polysynth::{CompactSynth, CompactSynthParams};
pub use polysynth::{DualPoly, DualPolyParams};
pub use polysynth::{GrooveSynth, GrooveSynthParams};
pub use polysynth::{HybridSeq, HybridSeqParams};
pub use polysynth::{MatrixSynth, MatrixSynthParams};
pub use polysynth::{PackSynth, PackSynthParams};
pub use polysynth::{PocketSynth, PocketSynthParams};
pub use polysynth::{VintagePoly, VintagePolyParams};
pub use polysynth::{VintageSaw, VintageSawParams};
pub mod instrument_asset;
pub use instrument_asset::{DrumKeyPatch, InstrumentAsset};

pub use drums::{LayerKick, LayerKickParams};

pub use additive::{SnapshotMorph, SnapshotMorphParams};

pub use additive::{HarmonicFilter, HarmonicFilterParams, HarmonicShape};

pub use additive::{MetalVoice, MetalVoiceParams};

pub use additive::{NumberVoice, NumberVoiceParams};

pub use additive::{SpectralResynth, SpectralResynthParams};

pub use additive::{ColorScan, ColorScanParams};
