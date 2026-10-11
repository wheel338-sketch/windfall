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
mod balance;
pub mod drums;
pub mod drive;
pub mod fm;
pub mod multiband;
pub mod physical;
pub mod spatial;
pub mod performance;
pub mod granular;
pub mod spectral;
pub mod blocks;
mod channel_mute;
mod compressor;
mod dc_block;
mod delay;
mod distortion;
pub mod echo_bank;
mod effect;
mod eq;
pub mod filter_family;
pub mod frequency_delay;
mod instrument;
mod note_expression;
mod limiter;
pub mod lofi;
pub mod modulation;
mod param;
mod polarity;
mod reverb;
mod soft_clipper;
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
pub use eq::{CutSlope, EqBand, EqCutBand, EqParams, ParametricEq};
pub use filter_family::{
    BassShelf, BassShelfParams, FastLowpass, FastLowpassParams, SelectableFilter,
    SelectableFilterMode, SelectableFilterParams,
};
pub use instrument::{AnyInstrument, Instrument, InstrumentKind, InstrumentParams};
pub use note_expression::{NoteArticulation, NoteExpression, NoteInstanceId};
pub use limiter::{Limiter, LimiterParams};
pub use lofi::{FilterPlacement, Lofi, LofiParams, RunRelation};
pub use modulation::{Chorus, ChorusParams, Flanger, FlangerParams, Phaser, PhaserParams};
pub use param::{ParamChoice, ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit};
pub use polarity::{Polarity, PolarityParams};
pub use reverb::{Reverb, ReverbParams};
pub use soft_clipper::{SoftClipper, SoftClipperParams};
pub use stereo_matrix::{MATRIX_MAX_DELAY_MS, MatrixMode, StereoMatrix, StereoMatrixParams};
pub use synth::{
    EnvelopeParams, FilterMode, FilterParams, FilterSlope, LfoParams, MAX_POLYPHONY, MAX_UNISON,
    OscillatorParams, SubtractiveSynth, SynthParams, VoiceMode,
};

pub use fm::{FourOp, FourOpParams, MatrixFm, MatrixFmParams, RingHybrid, RingHybridParams};
pub use additive::{HarmonicStack, HarmonicStackParams, PartialMorph, PartialMorphParams, Inharmonic, InharmonicParams, Resynth, ResynthParams, SeedPatch, SeedPatchParams, ScanSynth, ScanSynthParams};
pub use physical::{Pluck, PluckParams, FingerBass, FingerBassParams, AcousticString, AcousticStringParams};
pub use drums::{Membrane, MembraneParams, DrumRack, DrumRackParams, Kick, KickParams, DrumVoice, DrumVoiceParams};
pub use multiband::{BandSplit, BandSplitParams, MultibandCompressor, MultibandCompressorParams, MultibandMaximizer, MultibandMaximizerParams, TransientShaper, TransientShaperParams, TransientSplit, TransientSplitParams, OneKnob, OneKnobParams, BassHarmonics, BassHarmonicsParams, Exciter, ExciterParams};
pub use drive::{Waveshaper, WaveshaperParams, Overdrive, OverdriveParams, GuitarRack, GuitarRackParams, DriveChain, DriveChainParams};
pub use spatial::{VintageChorus, VintageChorusParams, HyperChorus, HyperChorusParams, VintagePhaser, VintagePhaserParams, StackedFlanger, StackedFlangerParams, BandDelay, BandDelayParams, Room, RoomParams, Spreader, SpreaderParams, StereoEnhancer, StereoEnhancerParams};


pub mod control;

pub mod eqbank;

pub mod analog;

pub mod lush;

pub mod speech;

pub mod midi_out;
pub use midi_out::{MidiOut, MidiOutController, MidiOutEvent, MidiOutParams};

pub mod tuner;

pub mod mastering;

pub mod surface;

pub mod sendtap;

pub mod notemap;

pub mod zones;
