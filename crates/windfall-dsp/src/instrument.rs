//! The interface every built-in instrument implements, and one type that
//! can hold any of them.

use crate::additive::{ColorScan, ColorScanParams};
use crate::additive::{HarmonicFilter, HarmonicFilterParams};
use crate::additive::{MetalVoice, MetalVoiceParams};
use crate::additive::{NumberVoice, NumberVoiceParams};
use crate::additive::{SnapshotMorph, SnapshotMorphParams};
use crate::additive::{SpectralResynth, SpectralResynthParams};
use crate::analog::{AcidLine, AcidLineParams};
use crate::analog::{MacroVoice, MacroVoiceParams};
use crate::analog::{TripleOsc, TripleOscParams};
use crate::analog::{WaveLane, WaveLaneParams};
use crate::drums::{LayerKick, LayerKickParams};
use crate::midi_out::{MidiOut, MidiOutEvent, MidiOutParams};
use crate::polysynth::{CompactSynth, CompactSynthParams};
use crate::polysynth::{DualPoly, DualPolyParams};
use crate::polysynth::{GrooveSynth, GrooveSynthParams};
use crate::polysynth::{HybridSeq, HybridSeqParams};
use crate::polysynth::{MatrixSynth, MatrixSynthParams};
use crate::polysynth::{PackSynth, PackSynthParams};
use crate::polysynth::{PocketSynth, PocketSynthParams};
use crate::polysynth::{VintagePoly, VintagePolyParams};
use crate::polysynth::{VintageSaw, VintageSawParams};
use crate::sample_bank::{
    MultiSampler, MultiSamplerParams, PadBank, PadBankParams, PianoKeys, PianoKeysParams,
    SamplePlayer, SamplePlayerParams, SoundFont, SoundFontParams, TextVoice, TextVoiceParams,
};
use crate::speech::{SpeechVoice, SpeechVoiceParams};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::additive::{
    HarmonicStack, HarmonicStackParams, Inharmonic, InharmonicParams, PartialMorph,
    PartialMorphParams, Resynth, ResynthParams, ScanSynth, ScanSynthParams, SeedPatch,
    SeedPatchParams,
};
use crate::drums::{
    DrumRack, DrumRackParams, DrumVoice, DrumVoiceParams, Kick, KickParams, Membrane,
    MembraneParams,
};
use crate::fm::{FourOp, FourOpParams, MatrixFm, MatrixFmParams, RingHybrid, RingHybridParams};
use crate::granular::{
    GrainCloud, GrainCloudParams, SliceDeck, SliceDeckParams, SliceMap, SliceMapParams, WaveRide,
    WaveRideParams,
};
use crate::physical::{
    AcousticString, AcousticStringParams, FingerBass, FingerBassParams, Pluck, PluckParams,
};

use crate::param::{ParamInfo, ParamSet};
use crate::synth::{SubtractiveSynth, SynthParams};
use crate::{NoteExpression, NoteInstanceId};

/// A stereo instrument that turns notes into audio.
///
/// The threading rules are those of [`Effect`](crate::Effect): build,
/// [`prepare`](Instrument::prepare) and drop it away from the audio thread;
/// everything else is safe on it and never allocates, locks or blocks.
///
/// Notes carry no position inside a block. A host that wants a note to
/// start on an exact sample processes up to that sample, calls
/// [`note_on`](Instrument::note_on), and processes the rest. The output
/// does not otherwise depend on how the audio is divided into blocks.
pub trait Instrument: Send {
    type Params: ParamSet;

    /// Sets the sample rate and readies the instrument to play.
    /// `max_block` is the longest block the host will pass. It also
    /// resets.
    fn prepare(&mut self, sample_rate: f32, max_block: usize);

    /// Silences every voice at once and returns to the state the
    /// instrument had after `prepare`, with its current parameters.
    fn reset(&mut self);

    /// Takes new parameter values. They are forced into range first, and
    /// sounding notes glide to them. Values set before the first block
    /// after `prepare` or `reset` apply at once.
    fn set_params(&mut self, params: &Self::Params);

    /// Tells the instrument the project tempo in beats per minute.
    fn set_tempo(&mut self, bpm: f32) {
        let _ = bpm;
    }

    /// Selects the project law for per-note panning. Implementations ramp live changes.
    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        let _ = law;
    }

    /// Starts a note. `key` is a MIDI key number, where 60 is middle C and
    /// 69 is the A at 440 Hz. `velocity` runs from 0 to 1; zero or less is
    /// taken as a note-off.
    fn note_on(&mut self, key: u8, velocity: f32);

    /// Starts a note with per-voice expression. Legacy instruments can
    /// retain their key/velocity implementation until they support it.
    fn note_on_expression(&mut self, key: u8, velocity: f32, pan: f32, expression: NoteExpression) {
        let _ = (pan, expression);
        self.note_on(key, velocity);
    }

    /// Lets go of a note, which starts its release.
    fn note_off(&mut self, key: u8);

    /// Whether instance-specific release and expression are supported.
    fn supports_note_instances(&self) -> bool {
        false
    }

    fn note_on_instance(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
    ) {
        let _ = id;
        self.note_on_expression(key, velocity, pan, expression);
    }

    fn note_off_instance(&mut self, id: NoteInstanceId, key: u8) {
        let _ = id;
        self.note_off(key);
    }

    /// Changes one held or releasing instance, leaving the other voices alone.
    fn set_note_expression(&mut self, id: NoteInstanceId, pan: f32, expression: NoteExpression) {
        let _ = (id, pan, expression);
    }

    /// Effective pitch in fractional MIDI keys, including per-note tuning.
    fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        let _ = (id, pitch);
    }

    /// Linear gain of one instance on top of its velocity, as the expression
    /// map of its channel asks. 1 is unity, which every note starts at.
    fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
        let _ = (id, gain);
    }

    /// Stops every note with a fade of a few milliseconds, skipping their
    /// releases. For stopping the transport without a click.
    fn all_notes_off(&mut self);

    /// Writes the next block of output, replacing whatever the two slices
    /// held. The slices have the same length, which may be anything from 1
    /// up.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);

    /// Voices sounding right now, including ones fading out.
    fn active_voices(&self) -> usize;

    /// Samples by which the output lags the notes, for delay compensation.
    fn latency_samples(&self) -> usize {
        0
    }

    /// Samples for which output can continue after
    /// [`active_voices`](Instrument::active_voices) has dropped to zero.
    /// After that the instrument puts out exact silence until the next
    /// note.
    fn tail_samples(&self) -> usize {
        0
    }
}

/// Which instrument an [`AnyInstrument`] or an [`InstrumentParams`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InstrumentKind {
    SubtractiveSynth,
    FourOp,
    MatrixFm,
    RingHybrid,
    HarmonicStack,
    PartialMorph,
    Inharmonic,
    Resynth,
    SeedPatch,
    ScanSynth,
    Pluck,
    FingerBass,
    AcousticString,
    Membrane,
    DrumRack,
    Kick,
    DrumVoice,
    SliceMap,
    SliceDeck,
    GrainCloud,
    WaveRide,
    AcidLine,
    TripleOsc,
    WaveLane,
    MacroVoice,
    SpeechVoice,
    MidiOut,
    VintagePoly,
    DualPoly,
    CompactSynth,
    VintageSaw,
    PocketSynth,
    GrooveSynth,
    HybridSeq,
    MatrixSynth,
    PackSynth,
    LayerKick,
    SnapshotMorph,
    HarmonicFilter,
    MetalVoice,
    NumberVoice,
    SpectralResynth,
    ColorScan,
    Dashboard,
    MultiSampler,
    SamplePlayer,
    SoundFont,
    PadBank,
    PianoKeys,
    TextVoice,
}

impl InstrumentKind {
    pub const ALL: [InstrumentKind; 50] = [
        InstrumentKind::SubtractiveSynth,
        InstrumentKind::FourOp,
        InstrumentKind::MatrixFm,
        InstrumentKind::RingHybrid,
        InstrumentKind::HarmonicStack,
        InstrumentKind::PartialMorph,
        InstrumentKind::Inharmonic,
        InstrumentKind::Resynth,
        InstrumentKind::SeedPatch,
        InstrumentKind::ScanSynth,
        InstrumentKind::Pluck,
        InstrumentKind::FingerBass,
        InstrumentKind::AcousticString,
        InstrumentKind::Membrane,
        InstrumentKind::DrumRack,
        InstrumentKind::Kick,
        InstrumentKind::DrumVoice,
        InstrumentKind::SliceMap,
        InstrumentKind::SliceDeck,
        InstrumentKind::GrainCloud,
        InstrumentKind::WaveRide,
        InstrumentKind::AcidLine,
        InstrumentKind::TripleOsc,
        InstrumentKind::WaveLane,
        InstrumentKind::MacroVoice,
        InstrumentKind::SpeechVoice,
        InstrumentKind::MidiOut,
        InstrumentKind::VintagePoly,
        InstrumentKind::DualPoly,
        InstrumentKind::CompactSynth,
        InstrumentKind::VintageSaw,
        InstrumentKind::PocketSynth,
        InstrumentKind::GrooveSynth,
        InstrumentKind::HybridSeq,
        InstrumentKind::MatrixSynth,
        InstrumentKind::PackSynth,
        InstrumentKind::LayerKick,
        InstrumentKind::SnapshotMorph,
        InstrumentKind::HarmonicFilter,
        InstrumentKind::MetalVoice,
        InstrumentKind::NumberVoice,
        InstrumentKind::SpectralResynth,
        InstrumentKind::ColorScan,
        InstrumentKind::Dashboard,
        InstrumentKind::MultiSampler,
        InstrumentKind::SamplePlayer,
        InstrumentKind::SoundFont,
        InstrumentKind::PadBank,
        InstrumentKind::PianoKeys,
        InstrumentKind::TextVoice,
    ];

    /// The instrument's name as shown to the user.
    pub fn name(self) -> &'static str {
        match self {
            InstrumentKind::SubtractiveSynth => SynthParams::NAME,
            InstrumentKind::FourOp => FourOpParams::NAME,
            InstrumentKind::MatrixFm => MatrixFmParams::NAME,
            InstrumentKind::RingHybrid => RingHybridParams::NAME,
            InstrumentKind::HarmonicStack => HarmonicStackParams::NAME,
            InstrumentKind::PartialMorph => PartialMorphParams::NAME,
            InstrumentKind::Inharmonic => InharmonicParams::NAME,
            InstrumentKind::Resynth => ResynthParams::NAME,
            InstrumentKind::SeedPatch => SeedPatchParams::NAME,
            InstrumentKind::ScanSynth => ScanSynthParams::NAME,
            InstrumentKind::Pluck => PluckParams::NAME,
            InstrumentKind::FingerBass => FingerBassParams::NAME,
            InstrumentKind::AcousticString => AcousticStringParams::NAME,
            InstrumentKind::Membrane => MembraneParams::NAME,
            InstrumentKind::DrumRack => DrumRackParams::NAME,
            InstrumentKind::Kick => KickParams::NAME,
            InstrumentKind::DrumVoice => DrumVoiceParams::NAME,
            InstrumentKind::SliceMap => SliceMapParams::NAME,
            InstrumentKind::SliceDeck => SliceDeckParams::NAME,
            InstrumentKind::GrainCloud => GrainCloudParams::NAME,
            InstrumentKind::WaveRide => WaveRideParams::NAME,
            InstrumentKind::AcidLine => AcidLineParams::NAME,
            InstrumentKind::TripleOsc => TripleOscParams::NAME,
            InstrumentKind::WaveLane => WaveLaneParams::NAME,
            InstrumentKind::MacroVoice => MacroVoiceParams::NAME,
            InstrumentKind::SpeechVoice => SpeechVoiceParams::NAME,
            InstrumentKind::MidiOut => MidiOutParams::NAME,
            InstrumentKind::VintagePoly => VintagePolyParams::NAME,
            InstrumentKind::DualPoly => DualPolyParams::NAME,
            InstrumentKind::CompactSynth => CompactSynthParams::NAME,
            InstrumentKind::VintageSaw => VintageSawParams::NAME,
            InstrumentKind::PocketSynth => PocketSynthParams::NAME,
            InstrumentKind::GrooveSynth => GrooveSynthParams::NAME,
            InstrumentKind::HybridSeq => HybridSeqParams::NAME,
            InstrumentKind::MatrixSynth => MatrixSynthParams::NAME,
            InstrumentKind::PackSynth => PackSynthParams::NAME,
            InstrumentKind::LayerKick => LayerKickParams::NAME,
            InstrumentKind::SnapshotMorph => SnapshotMorphParams::NAME,
            InstrumentKind::HarmonicFilter => HarmonicFilterParams::NAME,
            InstrumentKind::MetalVoice => MetalVoiceParams::NAME,
            InstrumentKind::NumberVoice => NumberVoiceParams::NAME,
            InstrumentKind::SpectralResynth => SpectralResynthParams::NAME,
            InstrumentKind::ColorScan => ColorScanParams::NAME,
            InstrumentKind::Dashboard => "Dashboard",
            InstrumentKind::MultiSampler => MultiSamplerParams::NAME,
            InstrumentKind::SamplePlayer => SamplePlayerParams::NAME,
            InstrumentKind::SoundFont => SoundFontParams::NAME,
            InstrumentKind::PadBank => PadBankParams::NAME,
            InstrumentKind::PianoKeys => PianoKeysParams::NAME,
            InstrumentKind::TextVoice => TextVoiceParams::NAME,
        }
    }

    /// The instrument's controls.
    pub fn descriptors(self) -> &'static [ParamInfo] {
        match self {
            InstrumentKind::SubtractiveSynth => SynthParams::descriptors(),
            InstrumentKind::FourOp => FourOpParams::descriptors(),
            InstrumentKind::MatrixFm => MatrixFmParams::descriptors(),
            InstrumentKind::RingHybrid => RingHybridParams::descriptors(),
            InstrumentKind::HarmonicStack => HarmonicStackParams::descriptors(),
            InstrumentKind::PartialMorph => PartialMorphParams::descriptors(),
            InstrumentKind::Inharmonic => InharmonicParams::descriptors(),
            InstrumentKind::Resynth => ResynthParams::descriptors(),
            InstrumentKind::SeedPatch => SeedPatchParams::descriptors(),
            InstrumentKind::ScanSynth => ScanSynthParams::descriptors(),
            InstrumentKind::Pluck => PluckParams::descriptors(),
            InstrumentKind::FingerBass => FingerBassParams::descriptors(),
            InstrumentKind::AcousticString => AcousticStringParams::descriptors(),
            InstrumentKind::Membrane => MembraneParams::descriptors(),
            InstrumentKind::DrumRack => DrumRackParams::descriptors(),
            InstrumentKind::Kick => KickParams::descriptors(),
            InstrumentKind::DrumVoice => DrumVoiceParams::descriptors(),
            InstrumentKind::SliceMap => SliceMapParams::descriptors(),
            InstrumentKind::SliceDeck => SliceDeckParams::descriptors(),
            InstrumentKind::GrainCloud => GrainCloudParams::descriptors(),
            InstrumentKind::WaveRide => WaveRideParams::descriptors(),
            InstrumentKind::AcidLine => AcidLineParams::descriptors(),
            InstrumentKind::TripleOsc => TripleOscParams::descriptors(),
            InstrumentKind::WaveLane => WaveLaneParams::descriptors(),
            InstrumentKind::MacroVoice => MacroVoiceParams::descriptors(),
            InstrumentKind::SpeechVoice => SpeechVoiceParams::descriptors(),
            InstrumentKind::MidiOut => MidiOutParams::descriptors(),
            InstrumentKind::VintagePoly => VintagePolyParams::descriptors(),
            InstrumentKind::DualPoly => DualPolyParams::descriptors(),
            InstrumentKind::CompactSynth => CompactSynthParams::descriptors(),
            InstrumentKind::VintageSaw => VintageSawParams::descriptors(),
            InstrumentKind::PocketSynth => PocketSynthParams::descriptors(),
            InstrumentKind::GrooveSynth => GrooveSynthParams::descriptors(),
            InstrumentKind::HybridSeq => HybridSeqParams::descriptors(),
            InstrumentKind::MatrixSynth => MatrixSynthParams::descriptors(),
            InstrumentKind::PackSynth => PackSynthParams::descriptors(),
            InstrumentKind::LayerKick => LayerKickParams::descriptors(),
            InstrumentKind::SnapshotMorph => SnapshotMorphParams::descriptors(),
            InstrumentKind::HarmonicFilter => HarmonicFilterParams::descriptors(),
            InstrumentKind::MetalVoice => MetalVoiceParams::descriptors(),
            InstrumentKind::NumberVoice => NumberVoiceParams::descriptors(),
            InstrumentKind::SpectralResynth => SpectralResynthParams::descriptors(),
            InstrumentKind::ColorScan => ColorScanParams::descriptors(),
            InstrumentKind::Dashboard => MidiOutParams::descriptors(),
            InstrumentKind::MultiSampler => MultiSamplerParams::descriptors(),
            InstrumentKind::SamplePlayer => SamplePlayerParams::descriptors(),
            InstrumentKind::SoundFont => SoundFontParams::descriptors(),
            InstrumentKind::PadBank => PadBankParams::descriptors(),
            InstrumentKind::PianoKeys => PianoKeysParams::descriptors(),
            InstrumentKind::TextVoice => TextVoiceParams::descriptors(),
        }
    }

    /// The instrument's default settings.
    pub fn default_params(self) -> InstrumentParams {
        match self {
            InstrumentKind::SubtractiveSynth => {
                InstrumentParams::SubtractiveSynth(SynthParams::default())
            }
            InstrumentKind::FourOp => InstrumentParams::FourOp(FourOpParams::default()),
            InstrumentKind::MatrixFm => InstrumentParams::MatrixFm(MatrixFmParams::default()),
            InstrumentKind::RingHybrid => InstrumentParams::RingHybrid(RingHybridParams::default()),
            InstrumentKind::HarmonicStack => {
                InstrumentParams::HarmonicStack(HarmonicStackParams::default())
            }
            InstrumentKind::PartialMorph => {
                InstrumentParams::PartialMorph(PartialMorphParams::default())
            }
            InstrumentKind::Inharmonic => InstrumentParams::Inharmonic(InharmonicParams::default()),
            InstrumentKind::Resynth => InstrumentParams::Resynth(ResynthParams::default()),
            InstrumentKind::SeedPatch => InstrumentParams::SeedPatch(SeedPatchParams::default()),
            InstrumentKind::ScanSynth => InstrumentParams::ScanSynth(ScanSynthParams::default()),
            InstrumentKind::Pluck => InstrumentParams::Pluck(PluckParams::default()),
            InstrumentKind::FingerBass => InstrumentParams::FingerBass(FingerBassParams::default()),
            InstrumentKind::AcousticString => {
                InstrumentParams::AcousticString(AcousticStringParams::default())
            }
            InstrumentKind::Membrane => InstrumentParams::Membrane(MembraneParams::default()),
            InstrumentKind::DrumRack => InstrumentParams::DrumRack(DrumRackParams::default()),
            InstrumentKind::Kick => InstrumentParams::Kick(KickParams::default()),
            InstrumentKind::DrumVoice => InstrumentParams::DrumVoice(DrumVoiceParams::default()),
            InstrumentKind::SliceMap => InstrumentParams::SliceMap(SliceMapParams::default()),
            InstrumentKind::SliceDeck => InstrumentParams::SliceDeck(SliceDeckParams::default()),
            InstrumentKind::GrainCloud => InstrumentParams::GrainCloud(GrainCloudParams::default()),
            InstrumentKind::WaveRide => InstrumentParams::WaveRide(WaveRideParams::default()),
            InstrumentKind::AcidLine => InstrumentParams::AcidLine(AcidLineParams::default()),
            InstrumentKind::TripleOsc => InstrumentParams::TripleOsc(TripleOscParams::default()),
            InstrumentKind::WaveLane => InstrumentParams::WaveLane(WaveLaneParams::default()),
            InstrumentKind::MacroVoice => InstrumentParams::MacroVoice(MacroVoiceParams::default()),
            InstrumentKind::SpeechVoice => {
                InstrumentParams::SpeechVoice(SpeechVoiceParams::default())
            }
            InstrumentKind::MidiOut => InstrumentParams::MidiOut(MidiOutParams::default()),
            InstrumentKind::VintagePoly => {
                InstrumentParams::VintagePoly(VintagePolyParams::default())
            }
            InstrumentKind::DualPoly => InstrumentParams::DualPoly(DualPolyParams::default()),
            InstrumentKind::CompactSynth => {
                InstrumentParams::CompactSynth(CompactSynthParams::default())
            }
            InstrumentKind::VintageSaw => InstrumentParams::VintageSaw(VintageSawParams::default()),
            InstrumentKind::PocketSynth => {
                InstrumentParams::PocketSynth(PocketSynthParams::default())
            }
            InstrumentKind::GrooveSynth => {
                InstrumentParams::GrooveSynth(GrooveSynthParams::default())
            }
            InstrumentKind::HybridSeq => InstrumentParams::HybridSeq(HybridSeqParams::default()),
            InstrumentKind::MatrixSynth => {
                InstrumentParams::MatrixSynth(MatrixSynthParams::default())
            }
            InstrumentKind::PackSynth => InstrumentParams::PackSynth(PackSynthParams::default()),
            InstrumentKind::LayerKick => InstrumentParams::LayerKick(LayerKickParams::default()),
            InstrumentKind::SnapshotMorph => {
                InstrumentParams::SnapshotMorph(SnapshotMorphParams::default())
            }
            InstrumentKind::HarmonicFilter => {
                InstrumentParams::HarmonicFilter(HarmonicFilterParams::default())
            }
            InstrumentKind::MetalVoice => InstrumentParams::MetalVoice(MetalVoiceParams::default()),
            InstrumentKind::NumberVoice => {
                InstrumentParams::NumberVoice(NumberVoiceParams::default())
            }
            InstrumentKind::SpectralResynth => {
                InstrumentParams::SpectralResynth(SpectralResynthParams::default())
            }
            InstrumentKind::ColorScan => InstrumentParams::ColorScan(ColorScanParams::default()),
            InstrumentKind::Dashboard => InstrumentParams::Dashboard(MidiOutParams::default()),
            InstrumentKind::MultiSampler => {
                InstrumentParams::MultiSampler(MultiSamplerParams::default())
            }
            InstrumentKind::SamplePlayer => {
                InstrumentParams::SamplePlayer(SamplePlayerParams::default())
            }
            InstrumentKind::SoundFont => InstrumentParams::SoundFont(SoundFontParams::default()),
            InstrumentKind::PadBank => InstrumentParams::PadBank(PadBankParams::default()),
            InstrumentKind::PianoKeys => InstrumentParams::PianoKeys(PianoKeysParams::default()),
            InstrumentKind::TextVoice => InstrumentParams::TextVoice(TextVoiceParams::default()),
        }
    }
}

/// The settings of any one instrument. This is what a project stores for
/// an instrument channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum InstrumentParams {
    SubtractiveSynth(SynthParams),
    FourOp(FourOpParams),
    MatrixFm(MatrixFmParams),
    RingHybrid(RingHybridParams),
    HarmonicStack(HarmonicStackParams),
    PartialMorph(PartialMorphParams),
    Inharmonic(InharmonicParams),
    Resynth(ResynthParams),
    SeedPatch(SeedPatchParams),
    ScanSynth(ScanSynthParams),
    Pluck(PluckParams),
    FingerBass(FingerBassParams),
    AcousticString(AcousticStringParams),
    Membrane(MembraneParams),
    DrumRack(DrumRackParams),
    Kick(KickParams),
    DrumVoice(DrumVoiceParams),
    SliceMap(SliceMapParams),
    SliceDeck(SliceDeckParams),
    GrainCloud(GrainCloudParams),
    WaveRide(WaveRideParams),
    AcidLine(AcidLineParams),
    TripleOsc(TripleOscParams),
    WaveLane(WaveLaneParams),
    MacroVoice(MacroVoiceParams),
    SpeechVoice(SpeechVoiceParams),
    MidiOut(MidiOutParams),
    VintagePoly(VintagePolyParams),
    DualPoly(DualPolyParams),
    CompactSynth(CompactSynthParams),
    VintageSaw(VintageSawParams),
    PocketSynth(PocketSynthParams),
    GrooveSynth(GrooveSynthParams),
    HybridSeq(HybridSeqParams),
    MatrixSynth(MatrixSynthParams),
    PackSynth(PackSynthParams),
    LayerKick(LayerKickParams),
    SnapshotMorph(SnapshotMorphParams),
    HarmonicFilter(HarmonicFilterParams),
    MetalVoice(MetalVoiceParams),
    NumberVoice(NumberVoiceParams),
    SpectralResynth(SpectralResynthParams),
    ColorScan(ColorScanParams),
    Dashboard(MidiOutParams),
    MultiSampler(MultiSamplerParams),
    SamplePlayer(SamplePlayerParams),
    SoundFont(SoundFontParams),
    PadBank(PadBankParams),
    PianoKeys(PianoKeysParams),
    TextVoice(TextVoiceParams),
}

impl InstrumentParams {
    pub fn kind(&self) -> InstrumentKind {
        match self {
            InstrumentParams::SubtractiveSynth(_) => InstrumentKind::SubtractiveSynth,
            InstrumentParams::FourOp(_) => InstrumentKind::FourOp,
            InstrumentParams::MatrixFm(_) => InstrumentKind::MatrixFm,
            InstrumentParams::RingHybrid(_) => InstrumentKind::RingHybrid,
            InstrumentParams::HarmonicStack(_) => InstrumentKind::HarmonicStack,
            InstrumentParams::PartialMorph(_) => InstrumentKind::PartialMorph,
            InstrumentParams::Inharmonic(_) => InstrumentKind::Inharmonic,
            InstrumentParams::Resynth(_) => InstrumentKind::Resynth,
            InstrumentParams::SeedPatch(_) => InstrumentKind::SeedPatch,
            InstrumentParams::ScanSynth(_) => InstrumentKind::ScanSynth,
            InstrumentParams::Pluck(_) => InstrumentKind::Pluck,
            InstrumentParams::FingerBass(_) => InstrumentKind::FingerBass,
            InstrumentParams::AcousticString(_) => InstrumentKind::AcousticString,
            InstrumentParams::Membrane(_) => InstrumentKind::Membrane,
            InstrumentParams::DrumRack(_) => InstrumentKind::DrumRack,
            InstrumentParams::Kick(_) => InstrumentKind::Kick,
            InstrumentParams::DrumVoice(_) => InstrumentKind::DrumVoice,
            InstrumentParams::SliceMap(_) => InstrumentKind::SliceMap,
            InstrumentParams::SliceDeck(_) => InstrumentKind::SliceDeck,
            InstrumentParams::GrainCloud(_) => InstrumentKind::GrainCloud,
            InstrumentParams::WaveRide(_) => InstrumentKind::WaveRide,
            InstrumentParams::AcidLine(_) => InstrumentKind::AcidLine,
            InstrumentParams::TripleOsc(_) => InstrumentKind::TripleOsc,
            InstrumentParams::WaveLane(_) => InstrumentKind::WaveLane,
            InstrumentParams::MacroVoice(_) => InstrumentKind::MacroVoice,
            InstrumentParams::SpeechVoice(_) => InstrumentKind::SpeechVoice,
            InstrumentParams::MidiOut(_) => InstrumentKind::MidiOut,
            InstrumentParams::VintagePoly(_) => InstrumentKind::VintagePoly,
            InstrumentParams::DualPoly(_) => InstrumentKind::DualPoly,
            InstrumentParams::CompactSynth(_) => InstrumentKind::CompactSynth,
            InstrumentParams::VintageSaw(_) => InstrumentKind::VintageSaw,
            InstrumentParams::PocketSynth(_) => InstrumentKind::PocketSynth,
            InstrumentParams::GrooveSynth(_) => InstrumentKind::GrooveSynth,
            InstrumentParams::HybridSeq(_) => InstrumentKind::HybridSeq,
            InstrumentParams::MatrixSynth(_) => InstrumentKind::MatrixSynth,
            InstrumentParams::PackSynth(_) => InstrumentKind::PackSynth,
            InstrumentParams::LayerKick(_) => InstrumentKind::LayerKick,
            InstrumentParams::SnapshotMorph(_) => InstrumentKind::SnapshotMorph,
            InstrumentParams::HarmonicFilter(_) => InstrumentKind::HarmonicFilter,
            InstrumentParams::MetalVoice(_) => InstrumentKind::MetalVoice,
            InstrumentParams::NumberVoice(_) => InstrumentKind::NumberVoice,
            InstrumentParams::SpectralResynth(_) => InstrumentKind::SpectralResynth,
            InstrumentParams::ColorScan(_) => InstrumentKind::ColorScan,
            InstrumentParams::Dashboard(_) => InstrumentKind::Dashboard,
            InstrumentParams::MultiSampler(_) => InstrumentKind::MultiSampler,
            InstrumentParams::SamplePlayer(_) => InstrumentKind::SamplePlayer,
            InstrumentParams::SoundFont(_) => InstrumentKind::SoundFont,
            InstrumentParams::PadBank(_) => InstrumentKind::PadBank,
            InstrumentParams::PianoKeys(_) => InstrumentKind::PianoKeys,
            InstrumentParams::TextVoice(_) => InstrumentKind::TextVoice,
        }
    }

    /// A copy with every value forced into its range.
    pub fn sanitized(&self) -> Self {
        match self {
            InstrumentParams::SubtractiveSynth(params) => {
                InstrumentParams::SubtractiveSynth(params.sanitized())
            }
            InstrumentParams::FourOp(params) => InstrumentParams::FourOp(params.sanitized()),
            InstrumentParams::MatrixFm(params) => InstrumentParams::MatrixFm(params.sanitized()),
            InstrumentParams::RingHybrid(params) => {
                InstrumentParams::RingHybrid(params.sanitized())
            }
            InstrumentParams::HarmonicStack(params) => {
                InstrumentParams::HarmonicStack(params.sanitized())
            }
            InstrumentParams::PartialMorph(params) => {
                InstrumentParams::PartialMorph(params.sanitized())
            }
            InstrumentParams::Inharmonic(params) => {
                InstrumentParams::Inharmonic(params.sanitized())
            }
            InstrumentParams::Resynth(params) => InstrumentParams::Resynth(params.sanitized()),
            InstrumentParams::SeedPatch(params) => InstrumentParams::SeedPatch(params.sanitized()),
            InstrumentParams::ScanSynth(params) => InstrumentParams::ScanSynth(params.sanitized()),
            InstrumentParams::Pluck(params) => InstrumentParams::Pluck(params.sanitized()),
            InstrumentParams::FingerBass(params) => {
                InstrumentParams::FingerBass(params.sanitized())
            }
            InstrumentParams::AcousticString(params) => {
                InstrumentParams::AcousticString(params.sanitized())
            }
            InstrumentParams::Membrane(params) => InstrumentParams::Membrane(params.sanitized()),
            InstrumentParams::DrumRack(params) => InstrumentParams::DrumRack(params.sanitized()),
            InstrumentParams::Kick(params) => InstrumentParams::Kick(params.sanitized()),
            InstrumentParams::DrumVoice(params) => InstrumentParams::DrumVoice(params.sanitized()),
            InstrumentParams::SliceMap(params) => InstrumentParams::SliceMap(params.sanitized()),
            InstrumentParams::SliceDeck(params) => InstrumentParams::SliceDeck(params.sanitized()),
            InstrumentParams::GrainCloud(params) => {
                InstrumentParams::GrainCloud(params.sanitized())
            }
            InstrumentParams::WaveRide(params) => InstrumentParams::WaveRide(params.sanitized()),
            InstrumentParams::AcidLine(params) => InstrumentParams::AcidLine(params.sanitized()),
            InstrumentParams::TripleOsc(params) => InstrumentParams::TripleOsc(params.sanitized()),
            InstrumentParams::WaveLane(params) => InstrumentParams::WaveLane(params.sanitized()),
            InstrumentParams::MacroVoice(params) => {
                InstrumentParams::MacroVoice(params.sanitized())
            }
            InstrumentParams::SpeechVoice(params) => {
                InstrumentParams::SpeechVoice(params.sanitized())
            }
            InstrumentParams::MidiOut(params) => InstrumentParams::MidiOut(params.sanitized()),
            InstrumentParams::VintagePoly(params) => {
                InstrumentParams::VintagePoly(params.sanitized())
            }
            InstrumentParams::DualPoly(params) => InstrumentParams::DualPoly(params.sanitized()),
            InstrumentParams::CompactSynth(params) => {
                InstrumentParams::CompactSynth(params.sanitized())
            }
            InstrumentParams::VintageSaw(params) => {
                InstrumentParams::VintageSaw(params.sanitized())
            }
            InstrumentParams::PocketSynth(params) => {
                InstrumentParams::PocketSynth(params.sanitized())
            }
            InstrumentParams::GrooveSynth(params) => {
                InstrumentParams::GrooveSynth(params.sanitized())
            }
            InstrumentParams::HybridSeq(params) => InstrumentParams::HybridSeq(params.sanitized()),
            InstrumentParams::MatrixSynth(params) => {
                InstrumentParams::MatrixSynth(params.sanitized())
            }
            InstrumentParams::PackSynth(params) => InstrumentParams::PackSynth(params.sanitized()),
            InstrumentParams::LayerKick(params) => InstrumentParams::LayerKick(params.sanitized()),
            InstrumentParams::SnapshotMorph(params) => {
                InstrumentParams::SnapshotMorph(params.sanitized())
            }
            InstrumentParams::HarmonicFilter(params) => {
                InstrumentParams::HarmonicFilter(params.sanitized())
            }
            InstrumentParams::MetalVoice(params) => {
                InstrumentParams::MetalVoice(params.sanitized())
            }
            InstrumentParams::NumberVoice(params) => {
                InstrumentParams::NumberVoice(params.sanitized())
            }
            InstrumentParams::SpectralResynth(params) => {
                InstrumentParams::SpectralResynth(params.sanitized())
            }
            InstrumentParams::ColorScan(params) => InstrumentParams::ColorScan(params.sanitized()),
            InstrumentParams::Dashboard(params) => InstrumentParams::Dashboard(params.sanitized()),
            InstrumentParams::MultiSampler(params) => {
                InstrumentParams::MultiSampler(params.sanitized())
            }
            InstrumentParams::SamplePlayer(params) => {
                InstrumentParams::SamplePlayer(params.sanitized())
            }
            InstrumentParams::SoundFont(params) => InstrumentParams::SoundFont(params.sanitized()),
            InstrumentParams::PadBank(params) => InstrumentParams::PadBank(params.sanitized()),
            InstrumentParams::PianoKeys(params) => InstrumentParams::PianoKeys(params.sanitized()),
            InstrumentParams::TextVoice(params) => InstrumentParams::TextVoice(params.sanitized()),
        }
    }

    /// The latency a prepared instrument has with these settings, which is
    /// what its [`latency_samples`](Instrument::latency_samples) reports. A
    /// host can plan delay compensation from this without holding the
    /// instrument.
    pub fn latency_samples(&self, sample_rate: f32) -> usize {
        let _ = sample_rate;
        match self {
            InstrumentParams::SubtractiveSynth(_) => SubtractiveSynth::LATENCY_SAMPLES,
            InstrumentParams::FourOp(_) => 0,
            InstrumentParams::MatrixFm(_) => 0,
            InstrumentParams::RingHybrid(_) => 0,
            InstrumentParams::HarmonicStack(_) => 0,
            InstrumentParams::PartialMorph(_) => 0,
            InstrumentParams::Inharmonic(_) => 0,
            InstrumentParams::Resynth(_) => 0,
            InstrumentParams::SeedPatch(_) => 0,
            InstrumentParams::ScanSynth(_) => 0,
            InstrumentParams::Pluck(_) => 0,
            InstrumentParams::FingerBass(_) => 0,
            InstrumentParams::AcousticString(_) => 0,
            InstrumentParams::Membrane(_) => 0,
            InstrumentParams::DrumRack(_) => 0,
            InstrumentParams::Kick(_) => 0,
            InstrumentParams::DrumVoice(_) => 0,
            InstrumentParams::SliceMap(_) => 0,
            InstrumentParams::SliceDeck(_) => 0,
            InstrumentParams::GrainCloud(_) => 0,
            InstrumentParams::WaveRide(_) => 0,
            InstrumentParams::AcidLine(_) => 0,
            InstrumentParams::TripleOsc(_) => 0,
            InstrumentParams::WaveLane(_) => 0,
            InstrumentParams::MacroVoice(_) => 0,
            InstrumentParams::SpeechVoice(_) => 0,
            InstrumentParams::MidiOut(_) => 0,
            InstrumentParams::VintagePoly(_) => 0,
            InstrumentParams::DualPoly(_) => 0,
            InstrumentParams::CompactSynth(_) => 0,
            InstrumentParams::VintageSaw(_) => 0,
            InstrumentParams::PocketSynth(_) => 0,
            InstrumentParams::GrooveSynth(_) => 0,
            InstrumentParams::HybridSeq(_) => 0,
            InstrumentParams::MatrixSynth(_) => 0,
            InstrumentParams::PackSynth(_) => 0,
            InstrumentParams::LayerKick(_) => 0,
            InstrumentParams::SnapshotMorph(_) => 0,
            InstrumentParams::HarmonicFilter(_) => 0,
            InstrumentParams::MetalVoice(_) => 0,
            InstrumentParams::NumberVoice(_) => 0,
            InstrumentParams::SpectralResynth(_) => 0,
            InstrumentParams::ColorScan(_) => 0,
            InstrumentParams::Dashboard(_) => 0,
            InstrumentParams::MultiSampler(_) => 0,
            InstrumentParams::SamplePlayer(_) => 0,
            InstrumentParams::SoundFont(_) => 0,
            InstrumentParams::PadBank(_) => 0,
            InstrumentParams::PianoKeys(_) => 0,
            InstrumentParams::TextVoice(_) => 0,
        }
    }

    /// The value of the control at `index` of the instrument's
    /// [descriptors](InstrumentKind::descriptors).
    pub fn get(&self, index: usize) -> Option<f32> {
        match self {
            InstrumentParams::SubtractiveSynth(params) => params.get(index),
            InstrumentParams::FourOp(params) => params.get(index),
            InstrumentParams::MatrixFm(params) => params.get(index),
            InstrumentParams::RingHybrid(params) => params.get(index),
            InstrumentParams::HarmonicStack(params) => params.get(index),
            InstrumentParams::PartialMorph(params) => params.get(index),
            InstrumentParams::Inharmonic(params) => params.get(index),
            InstrumentParams::Resynth(params) => params.get(index),
            InstrumentParams::SeedPatch(params) => params.get(index),
            InstrumentParams::ScanSynth(params) => params.get(index),
            InstrumentParams::Pluck(params) => params.get(index),
            InstrumentParams::FingerBass(params) => params.get(index),
            InstrumentParams::AcousticString(params) => params.get(index),
            InstrumentParams::Membrane(params) => params.get(index),
            InstrumentParams::DrumRack(params) => params.get(index),
            InstrumentParams::Kick(params) => params.get(index),
            InstrumentParams::DrumVoice(params) => params.get(index),
            InstrumentParams::SliceMap(params) => params.get(index),
            InstrumentParams::SliceDeck(params) => params.get(index),
            InstrumentParams::GrainCloud(params) => params.get(index),
            InstrumentParams::WaveRide(params) => params.get(index),
            InstrumentParams::AcidLine(params) => params.get(index),
            InstrumentParams::TripleOsc(params) => params.get(index),
            InstrumentParams::WaveLane(params) => params.get(index),
            InstrumentParams::MacroVoice(params) => params.get(index),
            InstrumentParams::SpeechVoice(params) => params.get(index),
            InstrumentParams::MidiOut(params) => params.get(index),
            InstrumentParams::VintagePoly(params) => params.get(index),
            InstrumentParams::DualPoly(params) => params.get(index),
            InstrumentParams::CompactSynth(params) => params.get(index),
            InstrumentParams::VintageSaw(params) => params.get(index),
            InstrumentParams::PocketSynth(params) => params.get(index),
            InstrumentParams::GrooveSynth(params) => params.get(index),
            InstrumentParams::HybridSeq(params) => params.get(index),
            InstrumentParams::MatrixSynth(params) => params.get(index),
            InstrumentParams::PackSynth(params) => params.get(index),
            InstrumentParams::LayerKick(params) => params.get(index),
            InstrumentParams::SnapshotMorph(params) => params.get(index),
            InstrumentParams::HarmonicFilter(params) => params.get(index),
            InstrumentParams::MetalVoice(params) => params.get(index),
            InstrumentParams::NumberVoice(params) => params.get(index),
            InstrumentParams::SpectralResynth(params) => params.get(index),
            InstrumentParams::ColorScan(params) => params.get(index),
            InstrumentParams::Dashboard(params) => params.get(index),
            InstrumentParams::MultiSampler(params) => params.get(index),
            InstrumentParams::SamplePlayer(params) => params.get(index),
            InstrumentParams::SoundFont(params) => params.get(index),
            InstrumentParams::PadBank(params) => params.get(index),
            InstrumentParams::PianoKeys(params) => params.get(index),
            InstrumentParams::TextVoice(params) => params.get(index),
        }
    }

    /// Sets the control at `index`, forcing the value into range. Returns
    /// false if there is no such control. Does not allocate.
    pub fn set(&mut self, index: usize, value: f32) -> bool {
        match self {
            InstrumentParams::SubtractiveSynth(params) => params.set(index, value),
            InstrumentParams::FourOp(params) => params.set(index, value),
            InstrumentParams::MatrixFm(params) => params.set(index, value),
            InstrumentParams::RingHybrid(params) => params.set(index, value),
            InstrumentParams::HarmonicStack(params) => params.set(index, value),
            InstrumentParams::PartialMorph(params) => params.set(index, value),
            InstrumentParams::Inharmonic(params) => params.set(index, value),
            InstrumentParams::Resynth(params) => params.set(index, value),
            InstrumentParams::SeedPatch(params) => params.set(index, value),
            InstrumentParams::ScanSynth(params) => params.set(index, value),
            InstrumentParams::Pluck(params) => params.set(index, value),
            InstrumentParams::FingerBass(params) => params.set(index, value),
            InstrumentParams::AcousticString(params) => params.set(index, value),
            InstrumentParams::Membrane(params) => params.set(index, value),
            InstrumentParams::DrumRack(params) => params.set(index, value),
            InstrumentParams::Kick(params) => params.set(index, value),
            InstrumentParams::DrumVoice(params) => params.set(index, value),
            InstrumentParams::SliceMap(params) => params.set(index, value),
            InstrumentParams::SliceDeck(params) => params.set(index, value),
            InstrumentParams::GrainCloud(params) => params.set(index, value),
            InstrumentParams::WaveRide(params) => params.set(index, value),
            InstrumentParams::AcidLine(params) => params.set(index, value),
            InstrumentParams::TripleOsc(params) => params.set(index, value),
            InstrumentParams::WaveLane(params) => params.set(index, value),
            InstrumentParams::MacroVoice(params) => params.set(index, value),
            InstrumentParams::SpeechVoice(params) => params.set(index, value),
            InstrumentParams::MidiOut(params) => params.set(index, value),
            InstrumentParams::VintagePoly(params) => params.set(index, value),
            InstrumentParams::DualPoly(params) => params.set(index, value),
            InstrumentParams::CompactSynth(params) => params.set(index, value),
            InstrumentParams::VintageSaw(params) => params.set(index, value),
            InstrumentParams::PocketSynth(params) => params.set(index, value),
            InstrumentParams::GrooveSynth(params) => params.set(index, value),
            InstrumentParams::HybridSeq(params) => params.set(index, value),
            InstrumentParams::MatrixSynth(params) => params.set(index, value),
            InstrumentParams::PackSynth(params) => params.set(index, value),
            InstrumentParams::LayerKick(params) => params.set(index, value),
            InstrumentParams::SnapshotMorph(params) => params.set(index, value),
            InstrumentParams::HarmonicFilter(params) => params.set(index, value),
            InstrumentParams::MetalVoice(params) => params.set(index, value),
            InstrumentParams::NumberVoice(params) => params.set(index, value),
            InstrumentParams::SpectralResynth(params) => params.set(index, value),
            InstrumentParams::ColorScan(params) => params.set(index, value),
            InstrumentParams::Dashboard(params) => params.set(index, value),
            InstrumentParams::MultiSampler(params) => params.set(index, value),
            InstrumentParams::SamplePlayer(params) => params.set(index, value),
            InstrumentParams::SoundFont(params) => params.set(index, value),
            InstrumentParams::PadBank(params) => params.set(index, value),
            InstrumentParams::PianoKeys(params) => params.set(index, value),
            InstrumentParams::TextVoice(params) => params.set(index, value),
        }
    }
}

/// Any one of the built-in instruments behind one type, so a host can
/// choose what a channel plays at run time.
///
/// The instrument lives on the heap. Create it, prepare it and set its
/// parameters off the audio thread, and send it back off the audio thread
/// to be dropped.
pub enum AnyInstrument {
    SubtractiveSynth(Box<SubtractiveSynth>),
    FourOp(Box<FourOp>),
    MatrixFm(Box<MatrixFm>),
    RingHybrid(Box<RingHybrid>),
    HarmonicStack(Box<HarmonicStack>),
    PartialMorph(Box<PartialMorph>),
    Inharmonic(Box<Inharmonic>),
    Resynth(Box<Resynth>),
    SeedPatch(Box<SeedPatch>),
    ScanSynth(Box<ScanSynth>),
    Pluck(Box<Pluck>),
    FingerBass(Box<FingerBass>),
    AcousticString(Box<AcousticString>),
    Membrane(Box<Membrane>),
    DrumRack(Box<DrumRack>),
    Kick(Box<Kick>),
    DrumVoice(Box<DrumVoice>),
    SliceMap(Box<SliceMap>),
    SliceDeck(Box<SliceDeck>),
    GrainCloud(Box<GrainCloud>),
    WaveRide(Box<WaveRide>),
    AcidLine(Box<AcidLine>),
    TripleOsc(Box<TripleOsc>),
    WaveLane(Box<WaveLane>),
    MacroVoice(Box<MacroVoice>),
    SpeechVoice(Box<SpeechVoice>),
    MidiOut(Box<MidiOut>),
    VintagePoly(Box<VintagePoly>),
    DualPoly(Box<DualPoly>),
    CompactSynth(Box<CompactSynth>),
    VintageSaw(Box<VintageSaw>),
    PocketSynth(Box<PocketSynth>),
    GrooveSynth(Box<GrooveSynth>),
    HybridSeq(Box<HybridSeq>),
    MatrixSynth(Box<MatrixSynth>),
    PackSynth(Box<PackSynth>),
    LayerKick(Box<LayerKick>),
    SnapshotMorph(Box<SnapshotMorph>),
    HarmonicFilter(Box<HarmonicFilter>),
    MetalVoice(Box<MetalVoice>),
    NumberVoice(Box<NumberVoice>),
    SpectralResynth(Box<SpectralResynth>),
    ColorScan(Box<ColorScan>),
    Dashboard(Box<MidiOut>),
    MultiSampler(Box<MultiSampler>),
    SamplePlayer(Box<SamplePlayer>),
    SoundFont(Box<SoundFont>),
    PadBank(Box<PadBank>),
    PianoKeys(Box<PianoKeys>),
    TextVoice(Box<TextVoice>),
}

impl AnyInstrument {
    /// Worker-side asset installation before publication. Does not run on audio.
    pub fn install_sample_bank(
        &mut self,
        bank: std::sync::Arc<crate::sample_bank::SampleBank>,
    ) -> bool {
        match self {
            Self::MultiSampler(unit) => unit.install_bank(bank),
            Self::SamplePlayer(unit) => unit.install_bank(bank),
            Self::SoundFont(unit) => unit.install_bank(bank),
            Self::PadBank(unit) => unit.install_bank(bank),
            Self::PianoKeys(unit) => unit.install_bank(bank),
            Self::TextVoice(unit) => unit.install_bank(bank),
            _ => return false,
        }
        true
    }
    /// Moves held bank voices and owners without callback allocation or free.
    pub fn inherit_sample_bank_state(&mut self, previous: &mut Self) -> bool {
        match (self, previous) {
            (Self::MultiSampler(a), Self::MultiSampler(b)) => a.inherit_bank_state(b),
            (Self::SamplePlayer(a), Self::SamplePlayer(b)) => a.inherit_bank_state(b),
            (Self::SoundFont(a), Self::SoundFont(b)) => a.inherit_bank_state(b),
            (Self::PadBank(a), Self::PadBank(b)) => a.inherit_bank_state(b),
            (Self::PianoKeys(a), Self::PianoKeys(b)) => a.inherit_bank_state(b),
            (Self::TextVoice(a), Self::TextVoice(b)) => a.inherit_bank_state(b),
            _ => return false,
        }
        true
    }
    pub fn take_unused_sample_bank(
        &mut self,
    ) -> Option<std::sync::Arc<crate::sample_bank::SampleBank>> {
        match self {
            Self::MultiSampler(a) => a.take_unused_bank(),
            Self::SamplePlayer(a) => a.take_unused_bank(),
            Self::SoundFont(a) => a.take_unused_bank(),
            Self::PadBank(a) => a.take_unused_bank(),
            Self::PianoKeys(a) => a.take_unused_bank(),
            Self::TextVoice(a) => a.take_unused_bank(),
            _ => None,
        }
    }
    /// Fixed-storage voice transfer when a prebuilt sample asset unit replaces
    /// its predecessor. New sources/curves are retained by the incoming unit.
    pub fn inherit_playback_state(&mut self, previous: &Self) -> bool {
        match (self, previous) {
            (Self::MidiOut(a), Self::MidiOut(b)) => a.adopt(b),
            (Self::SliceMap(a), Self::SliceMap(b)) => a.adopt(b),
            (Self::SliceDeck(a), Self::SliceDeck(b)) => a.adopt(b),
            (Self::GrainCloud(a), Self::GrainCloud(b)) => a.adopt(b),
            (Self::WaveRide(a), Self::WaveRide(b)) => a.adopt(b),
            _ => return false,
        }
        true
    }
    /// Called only while constructing a control-side instrument unit.
    pub fn set_playback_asset(
        &mut self,
        asset: std::sync::Arc<crate::granular::PlaybackAsset>,
    ) -> bool {
        match self {
            Self::SliceMap(i) => i.set_asset(asset),
            Self::SliceDeck(i) => i.set_asset(asset),
            Self::GrainCloud(i) => i.set_asset(asset),
            Self::WaveRide(i) => i.set_asset(asset),
            _ => return false,
        }
        true
    }

    /// Gives the messages a MIDI output channel made since the last block
    /// the frame of the block about to be processed. Nothing for the rest.
    pub fn stamp_midi(&mut self, frame: u64) {
        if let AnyInstrument::MidiOut(unit) | AnyInstrument::Dashboard(unit) = self {
            unit.stamp(frame);
        }
    }

    /// Hands over the messages a MIDI output channel has waiting, oldest
    /// first. Nothing for the rest. Never allocates.
    pub fn drain_midi(&mut self, out: &mut dyn FnMut(MidiOutEvent)) {
        if let AnyInstrument::MidiOut(unit) | AnyInstrument::Dashboard(unit) = self {
            unit.drain(out);
        }
    }

    /// Build with immutable data on the control/worker side.
    pub fn new_with_asset(
        params: &InstrumentParams,
        asset: Option<&crate::InstrumentAsset>,
    ) -> Self {
        let asset = asset.filter(|asset| asset.validate(params.kind()).is_ok());
        let mut instrument = Self::new(params);
        if let (Self::DrumVoice(unit), Some(crate::InstrumentAsset::DrumKeys { patches })) =
            (&mut instrument, asset)
        {
            unit.set_key_patches(patches);
        }
        if let (Self::LayerKick(unit), Some(crate::InstrumentAsset::Pcm(pcm))) =
            (&mut instrument, asset)
        {
            unit.set_layer(pcm);
        }
        if let (Self::SnapshotMorph(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        if let (Self::HarmonicFilter(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        if let (Self::MetalVoice(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        if let (Self::NumberVoice(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        if let (Self::SpectralResynth(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        if let (Self::ColorScan(unit), Some(crate::InstrumentAsset::Spectra(bank))) =
            (&mut instrument, asset)
        {
            unit.set_bank(bank);
        }
        instrument
    }

    /// Builds the instrument `params` belongs to, with those settings. It
    /// still needs [`AnyInstrument::prepare`].
    pub fn new(params: &InstrumentParams) -> Self {
        match params {
            InstrumentParams::SubtractiveSynth(params) => {
                let mut synth = Box::<SubtractiveSynth>::default();
                synth.set_params(params);
                AnyInstrument::SubtractiveSynth(synth)
            }
            InstrumentParams::FourOp(params) => {
                let mut synth = Box::<FourOp>::default();
                synth.set_params(params);
                AnyInstrument::FourOp(synth)
            }
            InstrumentParams::MatrixFm(params) => {
                let mut synth = Box::<MatrixFm>::default();
                synth.set_params(params);
                AnyInstrument::MatrixFm(synth)
            }
            InstrumentParams::RingHybrid(params) => {
                let mut synth = Box::<RingHybrid>::default();
                synth.set_params(params);
                AnyInstrument::RingHybrid(synth)
            }
            InstrumentParams::HarmonicStack(params) => {
                let mut synth = Box::<HarmonicStack>::default();
                synth.set_params(params);
                AnyInstrument::HarmonicStack(synth)
            }
            InstrumentParams::PartialMorph(params) => {
                let mut synth = Box::<PartialMorph>::default();
                synth.set_params(params);
                AnyInstrument::PartialMorph(synth)
            }
            InstrumentParams::Inharmonic(params) => {
                let mut synth = Box::<Inharmonic>::default();
                synth.set_params(params);
                AnyInstrument::Inharmonic(synth)
            }
            InstrumentParams::Resynth(params) => {
                let mut synth = Box::<Resynth>::default();
                synth.set_params(params);
                AnyInstrument::Resynth(synth)
            }
            InstrumentParams::SeedPatch(params) => {
                let mut synth = Box::<SeedPatch>::default();
                synth.set_params(params);
                AnyInstrument::SeedPatch(synth)
            }
            InstrumentParams::ScanSynth(params) => {
                let mut synth = Box::<ScanSynth>::default();
                synth.set_params(params);
                AnyInstrument::ScanSynth(synth)
            }
            InstrumentParams::Pluck(params) => {
                let mut synth = Box::<Pluck>::default();
                synth.set_params(params);
                AnyInstrument::Pluck(synth)
            }
            InstrumentParams::FingerBass(params) => {
                let mut synth = Box::<FingerBass>::default();
                synth.set_params(params);
                AnyInstrument::FingerBass(synth)
            }
            InstrumentParams::AcousticString(params) => {
                let mut synth = Box::<AcousticString>::default();
                synth.set_params(params);
                AnyInstrument::AcousticString(synth)
            }
            InstrumentParams::Membrane(params) => {
                let mut synth = Box::<Membrane>::default();
                synth.set_params(params);
                AnyInstrument::Membrane(synth)
            }
            InstrumentParams::DrumRack(params) => {
                let mut synth = Box::<DrumRack>::default();
                synth.set_params(params);
                AnyInstrument::DrumRack(synth)
            }
            InstrumentParams::Kick(params) => {
                let mut synth = Box::<Kick>::default();
                synth.set_params(params);
                AnyInstrument::Kick(synth)
            }
            InstrumentParams::DrumVoice(params) => {
                let mut synth = Box::<DrumVoice>::default();
                synth.set_params(params);
                AnyInstrument::DrumVoice(synth)
            }
            InstrumentParams::SliceMap(params) => {
                let mut synth = Box::<SliceMap>::default();
                synth.set_params(params);
                AnyInstrument::SliceMap(synth)
            }
            InstrumentParams::SliceDeck(params) => {
                let mut synth = Box::<SliceDeck>::default();
                synth.set_params(params);
                AnyInstrument::SliceDeck(synth)
            }
            InstrumentParams::GrainCloud(params) => {
                let mut synth = Box::<GrainCloud>::default();
                synth.set_params(params);
                AnyInstrument::GrainCloud(synth)
            }
            InstrumentParams::WaveRide(params) => {
                let mut synth = Box::<WaveRide>::default();
                synth.set_params(params);
                AnyInstrument::WaveRide(synth)
            }
            InstrumentParams::AcidLine(params) => {
                let mut synth = Box::<AcidLine>::default();
                synth.set_params(params);
                AnyInstrument::AcidLine(synth)
            }
            InstrumentParams::TripleOsc(params) => {
                let mut synth = Box::<TripleOsc>::default();
                synth.set_params(params);
                AnyInstrument::TripleOsc(synth)
            }
            InstrumentParams::WaveLane(params) => {
                let mut synth = Box::<WaveLane>::default();
                synth.set_params(params);
                AnyInstrument::WaveLane(synth)
            }
            InstrumentParams::MacroVoice(params) => {
                let mut synth = Box::<MacroVoice>::default();
                synth.set_params(params);
                AnyInstrument::MacroVoice(synth)
            }
            InstrumentParams::SpeechVoice(params) => {
                let mut synth = Box::<SpeechVoice>::default();
                synth.set_params(params);
                AnyInstrument::SpeechVoice(synth)
            }
            InstrumentParams::MidiOut(params) => {
                let mut synth = Box::<MidiOut>::default();
                synth.set_params(params);
                AnyInstrument::MidiOut(synth)
            }
            InstrumentParams::VintagePoly(params) => {
                let mut synth = Box::<VintagePoly>::default();
                synth.set_params(params);
                AnyInstrument::VintagePoly(synth)
            }
            InstrumentParams::DualPoly(params) => {
                let mut synth = Box::<DualPoly>::default();
                synth.set_params(params);
                AnyInstrument::DualPoly(synth)
            }
            InstrumentParams::CompactSynth(params) => {
                let mut synth = Box::<CompactSynth>::default();
                synth.set_params(params);
                AnyInstrument::CompactSynth(synth)
            }
            InstrumentParams::VintageSaw(params) => {
                let mut synth = Box::<VintageSaw>::default();
                synth.set_params(params);
                AnyInstrument::VintageSaw(synth)
            }
            InstrumentParams::PocketSynth(params) => {
                let mut synth = Box::<PocketSynth>::default();
                synth.set_params(params);
                AnyInstrument::PocketSynth(synth)
            }
            InstrumentParams::GrooveSynth(params) => {
                let mut synth = Box::<GrooveSynth>::default();
                synth.set_params(params);
                AnyInstrument::GrooveSynth(synth)
            }
            InstrumentParams::HybridSeq(params) => {
                let mut synth = Box::<HybridSeq>::default();
                synth.set_params(params);
                AnyInstrument::HybridSeq(synth)
            }
            InstrumentParams::MatrixSynth(params) => {
                let mut synth = Box::<MatrixSynth>::default();
                synth.set_params(params);
                AnyInstrument::MatrixSynth(synth)
            }
            InstrumentParams::PackSynth(params) => {
                let mut synth = Box::<PackSynth>::default();
                synth.set_params(params);
                AnyInstrument::PackSynth(synth)
            }
            InstrumentParams::LayerKick(params) => {
                let mut synth = Box::<LayerKick>::default();
                synth.set_params(params);
                AnyInstrument::LayerKick(synth)
            }
            InstrumentParams::SnapshotMorph(params) => {
                let mut synth = Box::<SnapshotMorph>::default();
                synth.set_params(params);
                AnyInstrument::SnapshotMorph(synth)
            }
            InstrumentParams::HarmonicFilter(params) => {
                let mut synth = Box::<HarmonicFilter>::default();
                synth.set_params(params);
                AnyInstrument::HarmonicFilter(synth)
            }
            InstrumentParams::MetalVoice(params) => {
                let mut synth = Box::<MetalVoice>::default();
                synth.set_params(params);
                AnyInstrument::MetalVoice(synth)
            }
            InstrumentParams::NumberVoice(params) => {
                let mut synth = Box::<NumberVoice>::default();
                synth.set_params(params);
                AnyInstrument::NumberVoice(synth)
            }
            InstrumentParams::SpectralResynth(params) => {
                let mut synth = Box::<SpectralResynth>::default();
                synth.set_params(params);
                AnyInstrument::SpectralResynth(synth)
            }
            InstrumentParams::ColorScan(params) => {
                let mut synth = Box::<ColorScan>::default();
                synth.set_params(params);
                AnyInstrument::ColorScan(synth)
            }
            InstrumentParams::Dashboard(params) => {
                let mut synth = Box::<MidiOut>::default();
                synth.set_params(params);
                AnyInstrument::Dashboard(synth)
            }
            InstrumentParams::MultiSampler(params) => {
                let mut synth = Box::<MultiSampler>::default();
                synth.set_params(params);
                AnyInstrument::MultiSampler(synth)
            }
            InstrumentParams::SamplePlayer(params) => {
                let mut synth = Box::<SamplePlayer>::default();
                synth.set_params(params);
                AnyInstrument::SamplePlayer(synth)
            }
            InstrumentParams::SoundFont(params) => {
                let mut synth = Box::<SoundFont>::default();
                synth.set_params(params);
                AnyInstrument::SoundFont(synth)
            }
            InstrumentParams::PadBank(params) => {
                let mut synth = Box::<PadBank>::default();
                synth.set_params(params);
                AnyInstrument::PadBank(synth)
            }
            InstrumentParams::PianoKeys(params) => {
                let mut synth = Box::<PianoKeys>::default();
                synth.set_params(params);
                AnyInstrument::PianoKeys(synth)
            }
            InstrumentParams::TextVoice(params) => {
                let mut synth = Box::<TextVoice>::default();
                synth.set_params(params);
                AnyInstrument::TextVoice(synth)
            }
        }
    }

    pub fn kind(&self) -> InstrumentKind {
        match self {
            AnyInstrument::SubtractiveSynth(_) => InstrumentKind::SubtractiveSynth,
            AnyInstrument::FourOp(_) => InstrumentKind::FourOp,
            AnyInstrument::MatrixFm(_) => InstrumentKind::MatrixFm,
            AnyInstrument::RingHybrid(_) => InstrumentKind::RingHybrid,
            AnyInstrument::HarmonicStack(_) => InstrumentKind::HarmonicStack,
            AnyInstrument::PartialMorph(_) => InstrumentKind::PartialMorph,
            AnyInstrument::Inharmonic(_) => InstrumentKind::Inharmonic,
            AnyInstrument::Resynth(_) => InstrumentKind::Resynth,
            AnyInstrument::SeedPatch(_) => InstrumentKind::SeedPatch,
            AnyInstrument::ScanSynth(_) => InstrumentKind::ScanSynth,
            AnyInstrument::Pluck(_) => InstrumentKind::Pluck,
            AnyInstrument::FingerBass(_) => InstrumentKind::FingerBass,
            AnyInstrument::AcousticString(_) => InstrumentKind::AcousticString,
            AnyInstrument::Membrane(_) => InstrumentKind::Membrane,
            AnyInstrument::DrumRack(_) => InstrumentKind::DrumRack,
            AnyInstrument::Kick(_) => InstrumentKind::Kick,
            AnyInstrument::DrumVoice(_) => InstrumentKind::DrumVoice,
            AnyInstrument::SliceMap(_) => InstrumentKind::SliceMap,
            AnyInstrument::SliceDeck(_) => InstrumentKind::SliceDeck,
            AnyInstrument::GrainCloud(_) => InstrumentKind::GrainCloud,
            AnyInstrument::WaveRide(_) => InstrumentKind::WaveRide,
            AnyInstrument::AcidLine(_) => InstrumentKind::AcidLine,
            AnyInstrument::TripleOsc(_) => InstrumentKind::TripleOsc,
            AnyInstrument::WaveLane(_) => InstrumentKind::WaveLane,
            AnyInstrument::MacroVoice(_) => InstrumentKind::MacroVoice,
            AnyInstrument::SpeechVoice(_) => InstrumentKind::SpeechVoice,
            AnyInstrument::MidiOut(_) => InstrumentKind::MidiOut,
            AnyInstrument::VintagePoly(_) => InstrumentKind::VintagePoly,
            AnyInstrument::DualPoly(_) => InstrumentKind::DualPoly,
            AnyInstrument::CompactSynth(_) => InstrumentKind::CompactSynth,
            AnyInstrument::VintageSaw(_) => InstrumentKind::VintageSaw,
            AnyInstrument::PocketSynth(_) => InstrumentKind::PocketSynth,
            AnyInstrument::GrooveSynth(_) => InstrumentKind::GrooveSynth,
            AnyInstrument::HybridSeq(_) => InstrumentKind::HybridSeq,
            AnyInstrument::MatrixSynth(_) => InstrumentKind::MatrixSynth,
            AnyInstrument::PackSynth(_) => InstrumentKind::PackSynth,
            AnyInstrument::LayerKick(_) => InstrumentKind::LayerKick,
            AnyInstrument::SnapshotMorph(_) => InstrumentKind::SnapshotMorph,
            AnyInstrument::HarmonicFilter(_) => InstrumentKind::HarmonicFilter,
            AnyInstrument::MetalVoice(_) => InstrumentKind::MetalVoice,
            AnyInstrument::NumberVoice(_) => InstrumentKind::NumberVoice,
            AnyInstrument::SpectralResynth(_) => InstrumentKind::SpectralResynth,
            AnyInstrument::ColorScan(_) => InstrumentKind::ColorScan,
            AnyInstrument::Dashboard(_) => InstrumentKind::Dashboard,
            AnyInstrument::MultiSampler(_) => InstrumentKind::MultiSampler,
            AnyInstrument::SamplePlayer(_) => InstrumentKind::SamplePlayer,
            AnyInstrument::SoundFont(_) => InstrumentKind::SoundFont,
            AnyInstrument::PadBank(_) => InstrumentKind::PadBank,
            AnyInstrument::PianoKeys(_) => InstrumentKind::PianoKeys,
            AnyInstrument::TextVoice(_) => InstrumentKind::TextVoice,
        }
    }

    /// See [`Instrument::prepare`].
    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::FourOp(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MatrixFm(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::RingHybrid(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::HarmonicStack(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::PartialMorph(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Inharmonic(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Resynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SeedPatch(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::ScanSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Pluck(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::FingerBass(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::AcousticString(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Membrane(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::DrumRack(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Kick(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::DrumVoice(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SliceMap(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SliceDeck(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::GrainCloud(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::WaveRide(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::AcidLine(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::TripleOsc(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::WaveLane(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MacroVoice(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SpeechVoice(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MidiOut(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::VintagePoly(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::DualPoly(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::CompactSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::VintageSaw(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::PocketSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::GrooveSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::HybridSeq(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MatrixSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::PackSynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::LayerKick(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SnapshotMorph(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::HarmonicFilter(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MetalVoice(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::NumberVoice(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SpectralResynth(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::ColorScan(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::Dashboard(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::MultiSampler(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SamplePlayer(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::SoundFont(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::PadBank(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::PianoKeys(synth) => synth.prepare(sample_rate, max_block),
            AnyInstrument::TextVoice(synth) => synth.prepare(sample_rate, max_block),
        }
    }

    /// See [`Instrument::reset`].
    pub fn reset(&mut self) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.reset(),
            AnyInstrument::FourOp(synth) => synth.reset(),
            AnyInstrument::MatrixFm(synth) => synth.reset(),
            AnyInstrument::RingHybrid(synth) => synth.reset(),
            AnyInstrument::HarmonicStack(synth) => synth.reset(),
            AnyInstrument::PartialMorph(synth) => synth.reset(),
            AnyInstrument::Inharmonic(synth) => synth.reset(),
            AnyInstrument::Resynth(synth) => synth.reset(),
            AnyInstrument::SeedPatch(synth) => synth.reset(),
            AnyInstrument::ScanSynth(synth) => synth.reset(),
            AnyInstrument::Pluck(synth) => synth.reset(),
            AnyInstrument::FingerBass(synth) => synth.reset(),
            AnyInstrument::AcousticString(synth) => synth.reset(),
            AnyInstrument::Membrane(synth) => synth.reset(),
            AnyInstrument::DrumRack(synth) => synth.reset(),
            AnyInstrument::Kick(synth) => synth.reset(),
            AnyInstrument::DrumVoice(synth) => synth.reset(),
            AnyInstrument::SliceMap(synth) => synth.reset(),
            AnyInstrument::SliceDeck(synth) => synth.reset(),
            AnyInstrument::GrainCloud(synth) => synth.reset(),
            AnyInstrument::WaveRide(synth) => synth.reset(),
            AnyInstrument::AcidLine(synth) => synth.reset(),
            AnyInstrument::TripleOsc(synth) => synth.reset(),
            AnyInstrument::WaveLane(synth) => synth.reset(),
            AnyInstrument::MacroVoice(synth) => synth.reset(),
            AnyInstrument::SpeechVoice(synth) => synth.reset(),
            AnyInstrument::MidiOut(synth) => synth.reset(),
            AnyInstrument::VintagePoly(synth) => synth.reset(),
            AnyInstrument::DualPoly(synth) => synth.reset(),
            AnyInstrument::CompactSynth(synth) => synth.reset(),
            AnyInstrument::VintageSaw(synth) => synth.reset(),
            AnyInstrument::PocketSynth(synth) => synth.reset(),
            AnyInstrument::GrooveSynth(synth) => synth.reset(),
            AnyInstrument::HybridSeq(synth) => synth.reset(),
            AnyInstrument::MatrixSynth(synth) => synth.reset(),
            AnyInstrument::PackSynth(synth) => synth.reset(),
            AnyInstrument::LayerKick(synth) => synth.reset(),
            AnyInstrument::SnapshotMorph(synth) => synth.reset(),
            AnyInstrument::HarmonicFilter(synth) => synth.reset(),
            AnyInstrument::MetalVoice(synth) => synth.reset(),
            AnyInstrument::NumberVoice(synth) => synth.reset(),
            AnyInstrument::SpectralResynth(synth) => synth.reset(),
            AnyInstrument::ColorScan(synth) => synth.reset(),
            AnyInstrument::Dashboard(synth) => synth.reset(),
            AnyInstrument::MultiSampler(synth) => synth.reset(),
            AnyInstrument::SamplePlayer(synth) => synth.reset(),
            AnyInstrument::SoundFont(synth) => synth.reset(),
            AnyInstrument::PadBank(synth) => synth.reset(),
            AnyInstrument::PianoKeys(synth) => synth.reset(),
            AnyInstrument::TextVoice(synth) => synth.reset(),
        }
    }

    /// See [`Instrument::set_params`]. Returns false, and changes nothing,
    /// if `params` are for a different instrument.
    pub fn set_params(&mut self, params: &InstrumentParams) -> bool {
        match (self, params) {
            (
                AnyInstrument::SubtractiveSynth(synth),
                InstrumentParams::SubtractiveSynth(params),
            ) => {
                synth.set_params(params);
            }
            (AnyInstrument::FourOp(synth), InstrumentParams::FourOp(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MatrixFm(synth), InstrumentParams::MatrixFm(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::RingHybrid(synth), InstrumentParams::RingHybrid(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::HarmonicStack(synth), InstrumentParams::HarmonicStack(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::PartialMorph(synth), InstrumentParams::PartialMorph(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Inharmonic(synth), InstrumentParams::Inharmonic(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Resynth(synth), InstrumentParams::Resynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SeedPatch(synth), InstrumentParams::SeedPatch(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::ScanSynth(synth), InstrumentParams::ScanSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Pluck(synth), InstrumentParams::Pluck(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::FingerBass(synth), InstrumentParams::FingerBass(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::AcousticString(synth), InstrumentParams::AcousticString(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Membrane(synth), InstrumentParams::Membrane(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::DrumRack(synth), InstrumentParams::DrumRack(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Kick(synth), InstrumentParams::Kick(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::DrumVoice(synth), InstrumentParams::DrumVoice(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SliceMap(synth), InstrumentParams::SliceMap(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SliceDeck(synth), InstrumentParams::SliceDeck(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::GrainCloud(synth), InstrumentParams::GrainCloud(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::WaveRide(synth), InstrumentParams::WaveRide(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::AcidLine(synth), InstrumentParams::AcidLine(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::TripleOsc(synth), InstrumentParams::TripleOsc(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::WaveLane(synth), InstrumentParams::WaveLane(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MacroVoice(synth), InstrumentParams::MacroVoice(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SpeechVoice(synth), InstrumentParams::SpeechVoice(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MidiOut(synth), InstrumentParams::MidiOut(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::VintagePoly(synth), InstrumentParams::VintagePoly(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::DualPoly(synth), InstrumentParams::DualPoly(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::CompactSynth(synth), InstrumentParams::CompactSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::VintageSaw(synth), InstrumentParams::VintageSaw(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::PocketSynth(synth), InstrumentParams::PocketSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::GrooveSynth(synth), InstrumentParams::GrooveSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::HybridSeq(synth), InstrumentParams::HybridSeq(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MatrixSynth(synth), InstrumentParams::MatrixSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::PackSynth(synth), InstrumentParams::PackSynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::LayerKick(synth), InstrumentParams::LayerKick(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SnapshotMorph(synth), InstrumentParams::SnapshotMorph(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::HarmonicFilter(synth), InstrumentParams::HarmonicFilter(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MetalVoice(synth), InstrumentParams::MetalVoice(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::NumberVoice(synth), InstrumentParams::NumberVoice(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SpectralResynth(synth), InstrumentParams::SpectralResynth(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::ColorScan(synth), InstrumentParams::ColorScan(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::Dashboard(synth), InstrumentParams::Dashboard(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::MultiSampler(synth), InstrumentParams::MultiSampler(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SamplePlayer(synth), InstrumentParams::SamplePlayer(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::SoundFont(synth), InstrumentParams::SoundFont(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::PadBank(synth), InstrumentParams::PadBank(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::PianoKeys(synth), InstrumentParams::PianoKeys(params)) => {
                synth.set_params(params);
            }
            (AnyInstrument::TextVoice(synth), InstrumentParams::TextVoice(params)) => {
                synth.set_params(params);
            }
            _ => return false,
        }
        true
    }

    /// See [`Instrument::set_tempo`].
    pub fn set_tempo(&mut self, bpm: f32) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::FourOp(synth) => synth.set_tempo(bpm),
            AnyInstrument::MatrixFm(synth) => synth.set_tempo(bpm),
            AnyInstrument::RingHybrid(synth) => synth.set_tempo(bpm),
            AnyInstrument::HarmonicStack(synth) => synth.set_tempo(bpm),
            AnyInstrument::PartialMorph(synth) => synth.set_tempo(bpm),
            AnyInstrument::Inharmonic(synth) => synth.set_tempo(bpm),
            AnyInstrument::Resynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::SeedPatch(synth) => synth.set_tempo(bpm),
            AnyInstrument::ScanSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::Pluck(synth) => synth.set_tempo(bpm),
            AnyInstrument::FingerBass(synth) => synth.set_tempo(bpm),
            AnyInstrument::AcousticString(synth) => synth.set_tempo(bpm),
            AnyInstrument::Membrane(synth) => synth.set_tempo(bpm),
            AnyInstrument::DrumRack(synth) => synth.set_tempo(bpm),
            AnyInstrument::Kick(synth) => synth.set_tempo(bpm),
            AnyInstrument::DrumVoice(synth) => synth.set_tempo(bpm),
            AnyInstrument::SliceMap(synth) => synth.set_tempo(bpm),
            AnyInstrument::SliceDeck(synth) => synth.set_tempo(bpm),
            AnyInstrument::GrainCloud(synth) => synth.set_tempo(bpm),
            AnyInstrument::WaveRide(synth) => synth.set_tempo(bpm),
            AnyInstrument::AcidLine(synth) => synth.set_tempo(bpm),
            AnyInstrument::TripleOsc(synth) => synth.set_tempo(bpm),
            AnyInstrument::WaveLane(synth) => synth.set_tempo(bpm),
            AnyInstrument::MacroVoice(synth) => synth.set_tempo(bpm),
            AnyInstrument::SpeechVoice(synth) => synth.set_tempo(bpm),
            AnyInstrument::MidiOut(synth) => synth.set_tempo(bpm),
            AnyInstrument::VintagePoly(synth) => synth.set_tempo(bpm),
            AnyInstrument::DualPoly(synth) => synth.set_tempo(bpm),
            AnyInstrument::CompactSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::VintageSaw(synth) => synth.set_tempo(bpm),
            AnyInstrument::PocketSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::GrooveSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::HybridSeq(synth) => synth.set_tempo(bpm),
            AnyInstrument::MatrixSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::PackSynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::LayerKick(synth) => synth.set_tempo(bpm),
            AnyInstrument::SnapshotMorph(synth) => synth.set_tempo(bpm),
            AnyInstrument::HarmonicFilter(synth) => synth.set_tempo(bpm),
            AnyInstrument::MetalVoice(synth) => synth.set_tempo(bpm),
            AnyInstrument::NumberVoice(synth) => synth.set_tempo(bpm),
            AnyInstrument::SpectralResynth(synth) => synth.set_tempo(bpm),
            AnyInstrument::ColorScan(synth) => synth.set_tempo(bpm),
            AnyInstrument::Dashboard(synth) => synth.set_tempo(bpm),
            AnyInstrument::MultiSampler(synth) => synth.set_tempo(bpm),
            AnyInstrument::SamplePlayer(synth) => synth.set_tempo(bpm),
            AnyInstrument::SoundFont(synth) => synth.set_tempo(bpm),
            AnyInstrument::PadBank(synth) => synth.set_tempo(bpm),
            AnyInstrument::PianoKeys(synth) => synth.set_tempo(bpm),
            AnyInstrument::TextVoice(synth) => synth.set_tempo(bpm),
        }
    }

    /// See [`Instrument::note_on`].
    pub fn note_on(&mut self, key: u8, velocity: f32) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::FourOp(synth) => synth.note_on(key, velocity),
            AnyInstrument::MatrixFm(synth) => synth.note_on(key, velocity),
            AnyInstrument::RingHybrid(synth) => synth.note_on(key, velocity),
            AnyInstrument::HarmonicStack(synth) => synth.note_on(key, velocity),
            AnyInstrument::PartialMorph(synth) => synth.note_on(key, velocity),
            AnyInstrument::Inharmonic(synth) => synth.note_on(key, velocity),
            AnyInstrument::Resynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::SeedPatch(synth) => synth.note_on(key, velocity),
            AnyInstrument::ScanSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::Pluck(synth) => synth.note_on(key, velocity),
            AnyInstrument::FingerBass(synth) => synth.note_on(key, velocity),
            AnyInstrument::AcousticString(synth) => synth.note_on(key, velocity),
            AnyInstrument::Membrane(synth) => synth.note_on(key, velocity),
            AnyInstrument::DrumRack(synth) => synth.note_on(key, velocity),
            AnyInstrument::Kick(synth) => synth.note_on(key, velocity),
            AnyInstrument::DrumVoice(synth) => synth.note_on(key, velocity),
            AnyInstrument::SliceMap(synth) => synth.note_on(key, velocity),
            AnyInstrument::SliceDeck(synth) => synth.note_on(key, velocity),
            AnyInstrument::GrainCloud(synth) => synth.note_on(key, velocity),
            AnyInstrument::WaveRide(synth) => synth.note_on(key, velocity),
            AnyInstrument::AcidLine(synth) => synth.note_on(key, velocity),
            AnyInstrument::TripleOsc(synth) => synth.note_on(key, velocity),
            AnyInstrument::WaveLane(synth) => synth.note_on(key, velocity),
            AnyInstrument::MacroVoice(synth) => synth.note_on(key, velocity),
            AnyInstrument::SpeechVoice(synth) => synth.note_on(key, velocity),
            AnyInstrument::MidiOut(synth) => synth.note_on(key, velocity),
            AnyInstrument::VintagePoly(synth) => synth.note_on(key, velocity),
            AnyInstrument::DualPoly(synth) => synth.note_on(key, velocity),
            AnyInstrument::CompactSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::VintageSaw(synth) => synth.note_on(key, velocity),
            AnyInstrument::PocketSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::GrooveSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::HybridSeq(synth) => synth.note_on(key, velocity),
            AnyInstrument::MatrixSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::PackSynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::LayerKick(synth) => synth.note_on(key, velocity),
            AnyInstrument::SnapshotMorph(synth) => synth.note_on(key, velocity),
            AnyInstrument::HarmonicFilter(synth) => synth.note_on(key, velocity),
            AnyInstrument::MetalVoice(synth) => synth.note_on(key, velocity),
            AnyInstrument::NumberVoice(synth) => synth.note_on(key, velocity),
            AnyInstrument::SpectralResynth(synth) => synth.note_on(key, velocity),
            AnyInstrument::ColorScan(synth) => synth.note_on(key, velocity),
            AnyInstrument::Dashboard(synth) => synth.note_on(key, velocity),
            AnyInstrument::MultiSampler(synth) => synth.note_on(key, velocity),
            AnyInstrument::SamplePlayer(synth) => synth.note_on(key, velocity),
            AnyInstrument::SoundFont(synth) => synth.note_on(key, velocity),
            AnyInstrument::PadBank(synth) => synth.note_on(key, velocity),
            AnyInstrument::PianoKeys(synth) => synth.note_on(key, velocity),
            AnyInstrument::TextVoice(synth) => synth.note_on(key, velocity),
        }
    }

    pub fn note_on_expression(
        &mut self,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
    ) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::FourOp(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MatrixFm(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::RingHybrid(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::HarmonicStack(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::PartialMorph(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Inharmonic(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Resynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SeedPatch(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::ScanSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Pluck(synth) => synth.note_on_expression(key, velocity, pan, expression),
            AnyInstrument::FingerBass(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::AcousticString(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Membrane(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::DrumRack(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Kick(synth) => synth.note_on_expression(key, velocity, pan, expression),
            AnyInstrument::DrumVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SliceMap(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SliceDeck(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::GrainCloud(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::WaveRide(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::AcidLine(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::TripleOsc(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::WaveLane(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MacroVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SpeechVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MidiOut(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::VintagePoly(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::DualPoly(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::CompactSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::VintageSaw(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::PocketSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::GrooveSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::HybridSeq(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MatrixSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::PackSynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::LayerKick(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SnapshotMorph(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::HarmonicFilter(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MetalVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::NumberVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SpectralResynth(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::ColorScan(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::Dashboard(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MultiSampler(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SamplePlayer(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::SoundFont(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::PadBank(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::PianoKeys(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::TextVoice(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
        }
    }

    /// See [`Instrument::note_off`].
    pub fn note_off(&mut self, key: u8) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.note_off(key),
            AnyInstrument::FourOp(synth) => synth.note_off(key),
            AnyInstrument::MatrixFm(synth) => synth.note_off(key),
            AnyInstrument::RingHybrid(synth) => synth.note_off(key),
            AnyInstrument::HarmonicStack(synth) => synth.note_off(key),
            AnyInstrument::PartialMorph(synth) => synth.note_off(key),
            AnyInstrument::Inharmonic(synth) => synth.note_off(key),
            AnyInstrument::Resynth(synth) => synth.note_off(key),
            AnyInstrument::SeedPatch(synth) => synth.note_off(key),
            AnyInstrument::ScanSynth(synth) => synth.note_off(key),
            AnyInstrument::Pluck(synth) => synth.note_off(key),
            AnyInstrument::FingerBass(synth) => synth.note_off(key),
            AnyInstrument::AcousticString(synth) => synth.note_off(key),
            AnyInstrument::Membrane(synth) => synth.note_off(key),
            AnyInstrument::DrumRack(synth) => synth.note_off(key),
            AnyInstrument::Kick(synth) => synth.note_off(key),
            AnyInstrument::DrumVoice(synth) => synth.note_off(key),
            AnyInstrument::SliceMap(synth) => synth.note_off(key),
            AnyInstrument::SliceDeck(synth) => synth.note_off(key),
            AnyInstrument::GrainCloud(synth) => synth.note_off(key),
            AnyInstrument::WaveRide(synth) => synth.note_off(key),
            AnyInstrument::AcidLine(synth) => synth.note_off(key),
            AnyInstrument::TripleOsc(synth) => synth.note_off(key),
            AnyInstrument::WaveLane(synth) => synth.note_off(key),
            AnyInstrument::MacroVoice(synth) => synth.note_off(key),
            AnyInstrument::SpeechVoice(synth) => synth.note_off(key),
            AnyInstrument::MidiOut(synth) => synth.note_off(key),
            AnyInstrument::VintagePoly(synth) => synth.note_off(key),
            AnyInstrument::DualPoly(synth) => synth.note_off(key),
            AnyInstrument::CompactSynth(synth) => synth.note_off(key),
            AnyInstrument::VintageSaw(synth) => synth.note_off(key),
            AnyInstrument::PocketSynth(synth) => synth.note_off(key),
            AnyInstrument::GrooveSynth(synth) => synth.note_off(key),
            AnyInstrument::HybridSeq(synth) => synth.note_off(key),
            AnyInstrument::MatrixSynth(synth) => synth.note_off(key),
            AnyInstrument::PackSynth(synth) => synth.note_off(key),
            AnyInstrument::LayerKick(synth) => synth.note_off(key),
            AnyInstrument::SnapshotMorph(synth) => synth.note_off(key),
            AnyInstrument::HarmonicFilter(synth) => synth.note_off(key),
            AnyInstrument::MetalVoice(synth) => synth.note_off(key),
            AnyInstrument::NumberVoice(synth) => synth.note_off(key),
            AnyInstrument::SpectralResynth(synth) => synth.note_off(key),
            AnyInstrument::ColorScan(synth) => synth.note_off(key),
            AnyInstrument::Dashboard(synth) => synth.note_off(key),
            AnyInstrument::MultiSampler(synth) => synth.note_off(key),
            AnyInstrument::SamplePlayer(synth) => synth.note_off(key),
            AnyInstrument::SoundFont(synth) => synth.note_off(key),
            AnyInstrument::PadBank(synth) => synth.note_off(key),
            AnyInstrument::PianoKeys(synth) => synth.note_off(key),
            AnyInstrument::TextVoice(synth) => synth.note_off(key),
        }
    }

    pub fn supports_note_instances(&self) -> bool {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::FourOp(synth) => synth.supports_note_instances(),
            AnyInstrument::MatrixFm(synth) => synth.supports_note_instances(),
            AnyInstrument::RingHybrid(synth) => synth.supports_note_instances(),
            AnyInstrument::HarmonicStack(synth) => synth.supports_note_instances(),
            AnyInstrument::PartialMorph(synth) => synth.supports_note_instances(),
            AnyInstrument::Inharmonic(synth) => synth.supports_note_instances(),
            AnyInstrument::Resynth(synth) => synth.supports_note_instances(),
            AnyInstrument::SeedPatch(synth) => synth.supports_note_instances(),
            AnyInstrument::ScanSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::Pluck(synth) => synth.supports_note_instances(),
            AnyInstrument::FingerBass(synth) => synth.supports_note_instances(),
            AnyInstrument::AcousticString(synth) => synth.supports_note_instances(),
            AnyInstrument::Membrane(synth) => synth.supports_note_instances(),
            AnyInstrument::DrumRack(synth) => synth.supports_note_instances(),
            AnyInstrument::Kick(synth) => synth.supports_note_instances(),
            AnyInstrument::DrumVoice(synth) => synth.supports_note_instances(),
            AnyInstrument::SliceMap(synth) => synth.supports_note_instances(),
            AnyInstrument::SliceDeck(synth) => synth.supports_note_instances(),
            AnyInstrument::GrainCloud(synth) => synth.supports_note_instances(),
            AnyInstrument::WaveRide(synth) => synth.supports_note_instances(),
            AnyInstrument::AcidLine(synth) => synth.supports_note_instances(),
            AnyInstrument::TripleOsc(synth) => synth.supports_note_instances(),
            AnyInstrument::WaveLane(synth) => synth.supports_note_instances(),
            AnyInstrument::MacroVoice(synth) => synth.supports_note_instances(),
            AnyInstrument::SpeechVoice(synth) => synth.supports_note_instances(),
            AnyInstrument::MidiOut(synth) => synth.supports_note_instances(),
            AnyInstrument::VintagePoly(synth) => synth.supports_note_instances(),
            AnyInstrument::DualPoly(synth) => synth.supports_note_instances(),
            AnyInstrument::CompactSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::VintageSaw(synth) => synth.supports_note_instances(),
            AnyInstrument::PocketSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::GrooveSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::HybridSeq(synth) => synth.supports_note_instances(),
            AnyInstrument::MatrixSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::PackSynth(synth) => synth.supports_note_instances(),
            AnyInstrument::LayerKick(synth) => synth.supports_note_instances(),
            AnyInstrument::SnapshotMorph(synth) => synth.supports_note_instances(),
            AnyInstrument::HarmonicFilter(synth) => synth.supports_note_instances(),
            AnyInstrument::MetalVoice(synth) => synth.supports_note_instances(),
            AnyInstrument::NumberVoice(synth) => synth.supports_note_instances(),
            AnyInstrument::SpectralResynth(synth) => synth.supports_note_instances(),
            AnyInstrument::ColorScan(synth) => synth.supports_note_instances(),
            AnyInstrument::Dashboard(synth) => synth.supports_note_instances(),
            AnyInstrument::MultiSampler(synth) => synth.supports_note_instances(),
            AnyInstrument::SamplePlayer(synth) => synth.supports_note_instances(),
            AnyInstrument::SoundFont(synth) => synth.supports_note_instances(),
            AnyInstrument::PadBank(synth) => synth.supports_note_instances(),
            AnyInstrument::PianoKeys(synth) => synth.supports_note_instances(),
            AnyInstrument::TextVoice(synth) => synth.supports_note_instances(),
        }
    }

    pub fn note_on_instance(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
    ) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::FourOp(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::MatrixFm(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::RingHybrid(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::HarmonicStack(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::PartialMorph(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Inharmonic(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Resynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SeedPatch(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::ScanSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Pluck(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::FingerBass(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::AcousticString(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Membrane(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::DrumRack(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Kick(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::DrumVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SliceMap(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SliceDeck(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::GrainCloud(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::WaveRide(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::AcidLine(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::TripleOsc(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::WaveLane(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::MacroVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SpeechVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::MidiOut(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::VintagePoly(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::DualPoly(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::CompactSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::VintageSaw(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::PocketSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::GrooveSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::HybridSeq(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::MatrixSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::PackSynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::LayerKick(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SnapshotMorph(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::HarmonicFilter(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::MetalVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::NumberVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SpectralResynth(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::ColorScan(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::Dashboard(synth) => {
                synth.note_on_expression(key, velocity, pan, expression)
            }
            AnyInstrument::MultiSampler(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SamplePlayer(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::SoundFont(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::PadBank(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::PianoKeys(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
            AnyInstrument::TextVoice(synth) => {
                synth.note_on_instance(id, key, velocity, pan, expression)
            }
        }
    }

    pub fn note_off_instance(&mut self, id: NoteInstanceId, key: u8) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::FourOp(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MatrixFm(synth) => synth.note_off_instance(id, key),
            AnyInstrument::RingHybrid(synth) => synth.note_off_instance(id, key),
            AnyInstrument::HarmonicStack(synth) => synth.note_off_instance(id, key),
            AnyInstrument::PartialMorph(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Inharmonic(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Resynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SeedPatch(synth) => synth.note_off_instance(id, key),
            AnyInstrument::ScanSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Pluck(synth) => synth.note_off_instance(id, key),
            AnyInstrument::FingerBass(synth) => synth.note_off_instance(id, key),
            AnyInstrument::AcousticString(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Membrane(synth) => synth.note_off_instance(id, key),
            AnyInstrument::DrumRack(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Kick(synth) => synth.note_off_instance(id, key),
            AnyInstrument::DrumVoice(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SliceMap(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SliceDeck(synth) => synth.note_off_instance(id, key),
            AnyInstrument::GrainCloud(synth) => synth.note_off_instance(id, key),
            AnyInstrument::WaveRide(synth) => synth.note_off_instance(id, key),
            AnyInstrument::AcidLine(synth) => synth.note_off_instance(id, key),
            AnyInstrument::TripleOsc(synth) => synth.note_off_instance(id, key),
            AnyInstrument::WaveLane(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MacroVoice(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SpeechVoice(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MidiOut(synth) => synth.note_off_instance(id, key),
            AnyInstrument::VintagePoly(synth) => synth.note_off_instance(id, key),
            AnyInstrument::DualPoly(synth) => synth.note_off_instance(id, key),
            AnyInstrument::CompactSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::VintageSaw(synth) => synth.note_off_instance(id, key),
            AnyInstrument::PocketSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::GrooveSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::HybridSeq(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MatrixSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::PackSynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::LayerKick(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SnapshotMorph(synth) => synth.note_off_instance(id, key),
            AnyInstrument::HarmonicFilter(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MetalVoice(synth) => synth.note_off_instance(id, key),
            AnyInstrument::NumberVoice(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SpectralResynth(synth) => synth.note_off_instance(id, key),
            AnyInstrument::ColorScan(synth) => synth.note_off_instance(id, key),
            AnyInstrument::Dashboard(synth) => synth.note_off_instance(id, key),
            AnyInstrument::MultiSampler(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SamplePlayer(synth) => synth.note_off_instance(id, key),
            AnyInstrument::SoundFont(synth) => synth.note_off_instance(id, key),
            AnyInstrument::PadBank(synth) => synth.note_off_instance(id, key),
            AnyInstrument::PianoKeys(synth) => synth.note_off_instance(id, key),
            AnyInstrument::TextVoice(synth) => synth.note_off_instance(id, key),
        }
    }

    pub fn set_note_expression(
        &mut self,
        id: NoteInstanceId,
        pan: f32,
        expression: NoteExpression,
    ) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => {
                synth.set_note_expression(id, pan, expression)
            }
            AnyInstrument::FourOp(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MatrixFm(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::RingHybrid(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::HarmonicStack(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::PartialMorph(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Inharmonic(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Resynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SeedPatch(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::ScanSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Pluck(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::FingerBass(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::AcousticString(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Membrane(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::DrumRack(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Kick(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::DrumVoice(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SliceMap(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SliceDeck(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::GrainCloud(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::WaveRide(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::AcidLine(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::TripleOsc(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::WaveLane(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MacroVoice(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SpeechVoice(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MidiOut(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::VintagePoly(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::DualPoly(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::CompactSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::VintageSaw(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::PocketSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::GrooveSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::HybridSeq(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MatrixSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::PackSynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::LayerKick(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SnapshotMorph(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::HarmonicFilter(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MetalVoice(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::NumberVoice(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SpectralResynth(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::ColorScan(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::Dashboard(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::MultiSampler(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SamplePlayer(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::SoundFont(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::PadBank(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::PianoKeys(synth) => synth.set_note_expression(id, pan, expression),
            AnyInstrument::TextVoice(synth) => synth.set_note_expression(id, pan, expression),
        }
    }

    pub fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::FourOp(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MatrixFm(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::RingHybrid(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::HarmonicStack(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::PartialMorph(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Inharmonic(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Resynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SeedPatch(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::ScanSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Pluck(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::FingerBass(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::AcousticString(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Membrane(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::DrumRack(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Kick(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::DrumVoice(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SliceMap(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SliceDeck(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::GrainCloud(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::WaveRide(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::AcidLine(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::TripleOsc(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::WaveLane(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MacroVoice(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SpeechVoice(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MidiOut(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::VintagePoly(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::DualPoly(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::CompactSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::VintageSaw(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::PocketSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::GrooveSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::HybridSeq(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MatrixSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::PackSynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::LayerKick(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SnapshotMorph(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::HarmonicFilter(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MetalVoice(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::NumberVoice(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SpectralResynth(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::ColorScan(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::Dashboard(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::MultiSampler(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SamplePlayer(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::SoundFont(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::PadBank(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::PianoKeys(synth) => synth.set_note_pitch(id, pitch),
            AnyInstrument::TextVoice(synth) => synth.set_note_pitch(id, pitch),
        }
    }

    /// See [`Instrument::set_note_pan_law`].
    pub fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        match self {
            Self::SubtractiveSynth(unit) => unit.set_note_pan_law(law),
            Self::FourOp(unit) => unit.set_note_pan_law(law),
            Self::MatrixFm(unit) => unit.set_note_pan_law(law),
            Self::RingHybrid(unit) => unit.set_note_pan_law(law),
            Self::HarmonicStack(unit) => unit.set_note_pan_law(law),
            Self::PartialMorph(unit) => unit.set_note_pan_law(law),
            Self::Inharmonic(unit) => unit.set_note_pan_law(law),
            Self::Resynth(unit) => unit.set_note_pan_law(law),
            Self::SeedPatch(unit) => unit.set_note_pan_law(law),
            Self::ScanSynth(unit) => unit.set_note_pan_law(law),
            Self::Pluck(unit) => unit.set_note_pan_law(law),
            Self::FingerBass(unit) => unit.set_note_pan_law(law),
            Self::AcousticString(unit) => unit.set_note_pan_law(law),
            Self::Membrane(unit) => unit.set_note_pan_law(law),
            Self::DrumRack(unit) => unit.set_note_pan_law(law),
            Self::Kick(unit) => unit.set_note_pan_law(law),
            Self::DrumVoice(unit) => unit.set_note_pan_law(law),
            Self::SliceMap(unit) => unit.set_note_pan_law(law),
            Self::SliceDeck(unit) => unit.set_note_pan_law(law),
            Self::GrainCloud(unit) => unit.set_note_pan_law(law),
            Self::WaveRide(unit) => unit.set_note_pan_law(law),
            Self::AcidLine(unit) => unit.set_note_pan_law(law),
            Self::TripleOsc(unit) => unit.set_note_pan_law(law),
            Self::WaveLane(unit) => unit.set_note_pan_law(law),
            Self::MacroVoice(unit) => unit.set_note_pan_law(law),
            Self::SpeechVoice(unit) => unit.set_note_pan_law(law),
            Self::MidiOut(unit) => unit.set_note_pan_law(law),
            Self::VintagePoly(unit) => unit.set_note_pan_law(law),
            Self::DualPoly(unit) => unit.set_note_pan_law(law),
            Self::CompactSynth(unit) => unit.set_note_pan_law(law),
            Self::VintageSaw(unit) => unit.set_note_pan_law(law),
            Self::PocketSynth(unit) => unit.set_note_pan_law(law),
            Self::GrooveSynth(unit) => unit.set_note_pan_law(law),
            Self::HybridSeq(unit) => unit.set_note_pan_law(law),
            Self::MatrixSynth(unit) => unit.set_note_pan_law(law),
            Self::PackSynth(unit) => unit.set_note_pan_law(law),
            Self::LayerKick(unit) => unit.set_note_pan_law(law),
            Self::SnapshotMorph(unit) => unit.set_note_pan_law(law),
            Self::HarmonicFilter(unit) => unit.set_note_pan_law(law),
            Self::MetalVoice(unit) => unit.set_note_pan_law(law),
            Self::NumberVoice(unit) => unit.set_note_pan_law(law),
            Self::SpectralResynth(unit) => unit.set_note_pan_law(law),
            Self::ColorScan(unit) => unit.set_note_pan_law(law),
            Self::Dashboard(unit) => unit.set_note_pan_law(law),
            Self::MultiSampler(unit) => unit.set_note_pan_law(law),
            Self::SamplePlayer(unit) => unit.set_note_pan_law(law),
            Self::SoundFont(unit) => unit.set_note_pan_law(law),
            Self::PadBank(unit) => unit.set_note_pan_law(law),
            Self::PianoKeys(unit) => unit.set_note_pan_law(law),
            Self::TextVoice(unit) => unit.set_note_pan_law(law),
        }
    }

    /// See [`Instrument::set_note_gain`]. Instruments without per-note controls ignore it.
    pub fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::SliceMap(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::SliceDeck(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::GrainCloud(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::WaveRide(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::MultiSampler(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::SamplePlayer(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::SoundFont(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::PadBank(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::PianoKeys(synth) => synth.set_note_gain(id, gain),
            AnyInstrument::TextVoice(synth) => synth.set_note_gain(id, gain),
            _ => {}
        }
    }

    /// See [`Instrument::all_notes_off`].
    pub fn all_notes_off(&mut self) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.all_notes_off(),
            AnyInstrument::FourOp(synth) => synth.all_notes_off(),
            AnyInstrument::MatrixFm(synth) => synth.all_notes_off(),
            AnyInstrument::RingHybrid(synth) => synth.all_notes_off(),
            AnyInstrument::HarmonicStack(synth) => synth.all_notes_off(),
            AnyInstrument::PartialMorph(synth) => synth.all_notes_off(),
            AnyInstrument::Inharmonic(synth) => synth.all_notes_off(),
            AnyInstrument::Resynth(synth) => synth.all_notes_off(),
            AnyInstrument::SeedPatch(synth) => synth.all_notes_off(),
            AnyInstrument::ScanSynth(synth) => synth.all_notes_off(),
            AnyInstrument::Pluck(synth) => synth.all_notes_off(),
            AnyInstrument::FingerBass(synth) => synth.all_notes_off(),
            AnyInstrument::AcousticString(synth) => synth.all_notes_off(),
            AnyInstrument::Membrane(synth) => synth.all_notes_off(),
            AnyInstrument::DrumRack(synth) => synth.all_notes_off(),
            AnyInstrument::Kick(synth) => synth.all_notes_off(),
            AnyInstrument::DrumVoice(synth) => synth.all_notes_off(),
            AnyInstrument::SliceMap(synth) => synth.all_notes_off(),
            AnyInstrument::SliceDeck(synth) => synth.all_notes_off(),
            AnyInstrument::GrainCloud(synth) => synth.all_notes_off(),
            AnyInstrument::WaveRide(synth) => synth.all_notes_off(),
            AnyInstrument::AcidLine(synth) => synth.all_notes_off(),
            AnyInstrument::TripleOsc(synth) => synth.all_notes_off(),
            AnyInstrument::WaveLane(synth) => synth.all_notes_off(),
            AnyInstrument::MacroVoice(synth) => synth.all_notes_off(),
            AnyInstrument::SpeechVoice(synth) => synth.all_notes_off(),
            AnyInstrument::MidiOut(synth) => synth.all_notes_off(),
            AnyInstrument::VintagePoly(synth) => synth.all_notes_off(),
            AnyInstrument::DualPoly(synth) => synth.all_notes_off(),
            AnyInstrument::CompactSynth(synth) => synth.all_notes_off(),
            AnyInstrument::VintageSaw(synth) => synth.all_notes_off(),
            AnyInstrument::PocketSynth(synth) => synth.all_notes_off(),
            AnyInstrument::GrooveSynth(synth) => synth.all_notes_off(),
            AnyInstrument::HybridSeq(synth) => synth.all_notes_off(),
            AnyInstrument::MatrixSynth(synth) => synth.all_notes_off(),
            AnyInstrument::PackSynth(synth) => synth.all_notes_off(),
            AnyInstrument::LayerKick(synth) => synth.all_notes_off(),
            AnyInstrument::SnapshotMorph(synth) => synth.all_notes_off(),
            AnyInstrument::HarmonicFilter(synth) => synth.all_notes_off(),
            AnyInstrument::MetalVoice(synth) => synth.all_notes_off(),
            AnyInstrument::NumberVoice(synth) => synth.all_notes_off(),
            AnyInstrument::SpectralResynth(synth) => synth.all_notes_off(),
            AnyInstrument::ColorScan(synth) => synth.all_notes_off(),
            AnyInstrument::Dashboard(synth) => synth.all_notes_off(),
            AnyInstrument::MultiSampler(synth) => synth.all_notes_off(),
            AnyInstrument::SamplePlayer(synth) => synth.all_notes_off(),
            AnyInstrument::SoundFont(synth) => synth.all_notes_off(),
            AnyInstrument::PadBank(synth) => synth.all_notes_off(),
            AnyInstrument::PianoKeys(synth) => synth.all_notes_off(),
            AnyInstrument::TextVoice(synth) => synth.all_notes_off(),
        }
    }

    /// See [`Instrument::process`].
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.process(left, right),
            AnyInstrument::FourOp(synth) => synth.process(left, right),
            AnyInstrument::MatrixFm(synth) => synth.process(left, right),
            AnyInstrument::RingHybrid(synth) => synth.process(left, right),
            AnyInstrument::HarmonicStack(synth) => synth.process(left, right),
            AnyInstrument::PartialMorph(synth) => synth.process(left, right),
            AnyInstrument::Inharmonic(synth) => synth.process(left, right),
            AnyInstrument::Resynth(synth) => synth.process(left, right),
            AnyInstrument::SeedPatch(synth) => synth.process(left, right),
            AnyInstrument::ScanSynth(synth) => synth.process(left, right),
            AnyInstrument::Pluck(synth) => synth.process(left, right),
            AnyInstrument::FingerBass(synth) => synth.process(left, right),
            AnyInstrument::AcousticString(synth) => synth.process(left, right),
            AnyInstrument::Membrane(synth) => synth.process(left, right),
            AnyInstrument::DrumRack(synth) => synth.process(left, right),
            AnyInstrument::Kick(synth) => synth.process(left, right),
            AnyInstrument::DrumVoice(synth) => synth.process(left, right),
            AnyInstrument::SliceMap(synth) => synth.process(left, right),
            AnyInstrument::SliceDeck(synth) => synth.process(left, right),
            AnyInstrument::GrainCloud(synth) => synth.process(left, right),
            AnyInstrument::WaveRide(synth) => synth.process(left, right),
            AnyInstrument::AcidLine(synth) => synth.process(left, right),
            AnyInstrument::TripleOsc(synth) => synth.process(left, right),
            AnyInstrument::WaveLane(synth) => synth.process(left, right),
            AnyInstrument::MacroVoice(synth) => synth.process(left, right),
            AnyInstrument::SpeechVoice(synth) => synth.process(left, right),
            AnyInstrument::MidiOut(synth) => synth.process(left, right),
            AnyInstrument::VintagePoly(synth) => synth.process(left, right),
            AnyInstrument::DualPoly(synth) => synth.process(left, right),
            AnyInstrument::CompactSynth(synth) => synth.process(left, right),
            AnyInstrument::VintageSaw(synth) => synth.process(left, right),
            AnyInstrument::PocketSynth(synth) => synth.process(left, right),
            AnyInstrument::GrooveSynth(synth) => synth.process(left, right),
            AnyInstrument::HybridSeq(synth) => synth.process(left, right),
            AnyInstrument::MatrixSynth(synth) => synth.process(left, right),
            AnyInstrument::PackSynth(synth) => synth.process(left, right),
            AnyInstrument::LayerKick(synth) => synth.process(left, right),
            AnyInstrument::SnapshotMorph(synth) => synth.process(left, right),
            AnyInstrument::HarmonicFilter(synth) => synth.process(left, right),
            AnyInstrument::MetalVoice(synth) => synth.process(left, right),
            AnyInstrument::NumberVoice(synth) => synth.process(left, right),
            AnyInstrument::SpectralResynth(synth) => synth.process(left, right),
            AnyInstrument::ColorScan(synth) => synth.process(left, right),
            AnyInstrument::Dashboard(synth) => synth.process(left, right),
            AnyInstrument::MultiSampler(synth) => synth.process(left, right),
            AnyInstrument::SamplePlayer(synth) => synth.process(left, right),
            AnyInstrument::SoundFont(synth) => synth.process(left, right),
            AnyInstrument::PadBank(synth) => synth.process(left, right),
            AnyInstrument::PianoKeys(synth) => synth.process(left, right),
            AnyInstrument::TextVoice(synth) => synth.process(left, right),
        }
    }

    /// See [`Instrument::active_voices`].
    pub fn active_voices(&self) -> usize {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.active_voices(),
            AnyInstrument::FourOp(synth) => synth.active_voices(),
            AnyInstrument::MatrixFm(synth) => synth.active_voices(),
            AnyInstrument::RingHybrid(synth) => synth.active_voices(),
            AnyInstrument::HarmonicStack(synth) => synth.active_voices(),
            AnyInstrument::PartialMorph(synth) => synth.active_voices(),
            AnyInstrument::Inharmonic(synth) => synth.active_voices(),
            AnyInstrument::Resynth(synth) => synth.active_voices(),
            AnyInstrument::SeedPatch(synth) => synth.active_voices(),
            AnyInstrument::ScanSynth(synth) => synth.active_voices(),
            AnyInstrument::Pluck(synth) => synth.active_voices(),
            AnyInstrument::FingerBass(synth) => synth.active_voices(),
            AnyInstrument::AcousticString(synth) => synth.active_voices(),
            AnyInstrument::Membrane(synth) => synth.active_voices(),
            AnyInstrument::DrumRack(synth) => synth.active_voices(),
            AnyInstrument::Kick(synth) => synth.active_voices(),
            AnyInstrument::DrumVoice(synth) => synth.active_voices(),
            AnyInstrument::SliceMap(synth) => synth.active_voices(),
            AnyInstrument::SliceDeck(synth) => synth.active_voices(),
            AnyInstrument::GrainCloud(synth) => synth.active_voices(),
            AnyInstrument::WaveRide(synth) => synth.active_voices(),
            AnyInstrument::AcidLine(synth) => synth.active_voices(),
            AnyInstrument::TripleOsc(synth) => synth.active_voices(),
            AnyInstrument::WaveLane(synth) => synth.active_voices(),
            AnyInstrument::MacroVoice(synth) => synth.active_voices(),
            AnyInstrument::SpeechVoice(synth) => synth.active_voices(),
            AnyInstrument::MidiOut(synth) => synth.active_voices(),
            AnyInstrument::VintagePoly(synth) => synth.active_voices(),
            AnyInstrument::DualPoly(synth) => synth.active_voices(),
            AnyInstrument::CompactSynth(synth) => synth.active_voices(),
            AnyInstrument::VintageSaw(synth) => synth.active_voices(),
            AnyInstrument::PocketSynth(synth) => synth.active_voices(),
            AnyInstrument::GrooveSynth(synth) => synth.active_voices(),
            AnyInstrument::HybridSeq(synth) => synth.active_voices(),
            AnyInstrument::MatrixSynth(synth) => synth.active_voices(),
            AnyInstrument::PackSynth(synth) => synth.active_voices(),
            AnyInstrument::LayerKick(synth) => synth.active_voices(),
            AnyInstrument::SnapshotMorph(synth) => synth.active_voices(),
            AnyInstrument::HarmonicFilter(synth) => synth.active_voices(),
            AnyInstrument::MetalVoice(synth) => synth.active_voices(),
            AnyInstrument::NumberVoice(synth) => synth.active_voices(),
            AnyInstrument::SpectralResynth(synth) => synth.active_voices(),
            AnyInstrument::ColorScan(synth) => synth.active_voices(),
            AnyInstrument::Dashboard(synth) => synth.active_voices(),
            AnyInstrument::MultiSampler(synth) => synth.active_voices(),
            AnyInstrument::SamplePlayer(synth) => synth.active_voices(),
            AnyInstrument::SoundFont(synth) => synth.active_voices(),
            AnyInstrument::PadBank(synth) => synth.active_voices(),
            AnyInstrument::PianoKeys(synth) => synth.active_voices(),
            AnyInstrument::TextVoice(synth) => synth.active_voices(),
        }
    }

    /// See [`Instrument::latency_samples`].
    pub fn latency_samples(&self) -> usize {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.latency_samples(),
            AnyInstrument::FourOp(synth) => synth.latency_samples(),
            AnyInstrument::MatrixFm(synth) => synth.latency_samples(),
            AnyInstrument::RingHybrid(synth) => synth.latency_samples(),
            AnyInstrument::HarmonicStack(synth) => synth.latency_samples(),
            AnyInstrument::PartialMorph(synth) => synth.latency_samples(),
            AnyInstrument::Inharmonic(synth) => synth.latency_samples(),
            AnyInstrument::Resynth(synth) => synth.latency_samples(),
            AnyInstrument::SeedPatch(synth) => synth.latency_samples(),
            AnyInstrument::ScanSynth(synth) => synth.latency_samples(),
            AnyInstrument::Pluck(synth) => synth.latency_samples(),
            AnyInstrument::FingerBass(synth) => synth.latency_samples(),
            AnyInstrument::AcousticString(synth) => synth.latency_samples(),
            AnyInstrument::Membrane(synth) => synth.latency_samples(),
            AnyInstrument::DrumRack(synth) => synth.latency_samples(),
            AnyInstrument::Kick(synth) => synth.latency_samples(),
            AnyInstrument::DrumVoice(synth) => synth.latency_samples(),
            AnyInstrument::SliceMap(synth) => synth.latency_samples(),
            AnyInstrument::SliceDeck(synth) => synth.latency_samples(),
            AnyInstrument::GrainCloud(synth) => synth.latency_samples(),
            AnyInstrument::WaveRide(synth) => synth.latency_samples(),
            AnyInstrument::AcidLine(synth) => synth.latency_samples(),
            AnyInstrument::TripleOsc(synth) => synth.latency_samples(),
            AnyInstrument::WaveLane(synth) => synth.latency_samples(),
            AnyInstrument::MacroVoice(synth) => synth.latency_samples(),
            AnyInstrument::SpeechVoice(synth) => synth.latency_samples(),
            AnyInstrument::MidiOut(synth) => synth.latency_samples(),
            AnyInstrument::VintagePoly(synth) => synth.latency_samples(),
            AnyInstrument::DualPoly(synth) => synth.latency_samples(),
            AnyInstrument::CompactSynth(synth) => synth.latency_samples(),
            AnyInstrument::VintageSaw(synth) => synth.latency_samples(),
            AnyInstrument::PocketSynth(synth) => synth.latency_samples(),
            AnyInstrument::GrooveSynth(synth) => synth.latency_samples(),
            AnyInstrument::HybridSeq(synth) => synth.latency_samples(),
            AnyInstrument::MatrixSynth(synth) => synth.latency_samples(),
            AnyInstrument::PackSynth(synth) => synth.latency_samples(),
            AnyInstrument::LayerKick(synth) => synth.latency_samples(),
            AnyInstrument::SnapshotMorph(synth) => synth.latency_samples(),
            AnyInstrument::HarmonicFilter(synth) => synth.latency_samples(),
            AnyInstrument::MetalVoice(synth) => synth.latency_samples(),
            AnyInstrument::NumberVoice(synth) => synth.latency_samples(),
            AnyInstrument::SpectralResynth(synth) => synth.latency_samples(),
            AnyInstrument::ColorScan(synth) => synth.latency_samples(),
            AnyInstrument::Dashboard(synth) => synth.latency_samples(),
            AnyInstrument::MultiSampler(synth) => synth.latency_samples(),
            AnyInstrument::SamplePlayer(synth) => synth.latency_samples(),
            AnyInstrument::SoundFont(synth) => synth.latency_samples(),
            AnyInstrument::PadBank(synth) => synth.latency_samples(),
            AnyInstrument::PianoKeys(synth) => synth.latency_samples(),
            AnyInstrument::TextVoice(synth) => synth.latency_samples(),
        }
    }

    /// See [`Instrument::tail_samples`].
    pub fn tail_samples(&self) -> usize {
        match self {
            AnyInstrument::SubtractiveSynth(synth) => synth.tail_samples(),
            AnyInstrument::FourOp(synth) => synth.tail_samples(),
            AnyInstrument::MatrixFm(synth) => synth.tail_samples(),
            AnyInstrument::RingHybrid(synth) => synth.tail_samples(),
            AnyInstrument::HarmonicStack(synth) => synth.tail_samples(),
            AnyInstrument::PartialMorph(synth) => synth.tail_samples(),
            AnyInstrument::Inharmonic(synth) => synth.tail_samples(),
            AnyInstrument::Resynth(synth) => synth.tail_samples(),
            AnyInstrument::SeedPatch(synth) => synth.tail_samples(),
            AnyInstrument::ScanSynth(synth) => synth.tail_samples(),
            AnyInstrument::Pluck(synth) => synth.tail_samples(),
            AnyInstrument::FingerBass(synth) => synth.tail_samples(),
            AnyInstrument::AcousticString(synth) => synth.tail_samples(),
            AnyInstrument::Membrane(synth) => synth.tail_samples(),
            AnyInstrument::DrumRack(synth) => synth.tail_samples(),
            AnyInstrument::Kick(synth) => synth.tail_samples(),
            AnyInstrument::DrumVoice(synth) => synth.tail_samples(),
            AnyInstrument::SliceMap(synth) => synth.tail_samples(),
            AnyInstrument::SliceDeck(synth) => synth.tail_samples(),
            AnyInstrument::GrainCloud(synth) => synth.tail_samples(),
            AnyInstrument::WaveRide(synth) => synth.tail_samples(),
            AnyInstrument::AcidLine(synth) => synth.tail_samples(),
            AnyInstrument::TripleOsc(synth) => synth.tail_samples(),
            AnyInstrument::WaveLane(synth) => synth.tail_samples(),
            AnyInstrument::MacroVoice(synth) => synth.tail_samples(),
            AnyInstrument::SpeechVoice(synth) => synth.tail_samples(),
            AnyInstrument::MidiOut(synth) => synth.tail_samples(),
            AnyInstrument::VintagePoly(synth) => synth.tail_samples(),
            AnyInstrument::DualPoly(synth) => synth.tail_samples(),
            AnyInstrument::CompactSynth(synth) => synth.tail_samples(),
            AnyInstrument::VintageSaw(synth) => synth.tail_samples(),
            AnyInstrument::PocketSynth(synth) => synth.tail_samples(),
            AnyInstrument::GrooveSynth(synth) => synth.tail_samples(),
            AnyInstrument::HybridSeq(synth) => synth.tail_samples(),
            AnyInstrument::MatrixSynth(synth) => synth.tail_samples(),
            AnyInstrument::PackSynth(synth) => synth.tail_samples(),
            AnyInstrument::LayerKick(synth) => synth.tail_samples(),
            AnyInstrument::SnapshotMorph(synth) => synth.tail_samples(),
            AnyInstrument::HarmonicFilter(synth) => synth.tail_samples(),
            AnyInstrument::MetalVoice(synth) => synth.tail_samples(),
            AnyInstrument::NumberVoice(synth) => synth.tail_samples(),
            AnyInstrument::SpectralResynth(synth) => synth.tail_samples(),
            AnyInstrument::ColorScan(synth) => synth.tail_samples(),
            AnyInstrument::Dashboard(synth) => synth.tail_samples(),
            AnyInstrument::MultiSampler(synth) => synth.tail_samples(),
            AnyInstrument::SamplePlayer(synth) => synth.tail_samples(),
            AnyInstrument::SoundFont(synth) => synth.tail_samples(),
            AnyInstrument::PadBank(synth) => synth.tail_samples(),
            AnyInstrument::PianoKeys(synth) => synth.tail_samples(),
            AnyInstrument::TextVoice(synth) => synth.tail_samples(),
        }
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn every_instrument_kind_round_trips_and_dispatches() {
        assert_eq!(InstrumentKind::ALL[0], InstrumentKind::SubtractiveSynth);
        for kind in InstrumentKind::ALL {
            let params = kind.default_params();
            let tag = serde_json::to_value(kind).unwrap();
            let json = serde_json::to_value(params).unwrap();
            assert_eq!(json["type"], tag);
            let loaded: InstrumentParams = serde_json::from_value(json).unwrap();
            assert_eq!(loaded, params);
            let bare: InstrumentParams =
                serde_json::from_value(serde_json::json!({ "type": tag })).unwrap();
            assert_eq!(bare, params);
            assert_eq!(params.sanitized(), params);
            for (index, info) in kind.descriptors().iter().enumerate() {
                assert_eq!(params.get(index), Some(info.default));
                let mut changed = params;
                assert!(changed.set(index, info.default));
                // Explicit marker/pad/curve writes override immutable assets,
                // including a write of the original scalar default. Check the
                // exact activation bit as well as all unchanged scalar values.
                let mut expected = params;
                let edit = match &mut expected {
                    InstrumentParams::SliceMap(p) if index >= 3 => {
                        Some((&mut p.legacy_edits, index - 3))
                    }
                    InstrumentParams::SliceDeck(p) if index >= 3 => {
                        Some((&mut p.legacy_edits, index - 3))
                    }
                    InstrumentParams::WaveRide(p) if index >= 5 => {
                        Some((&mut p.legacy_edits, index - 5))
                    }
                    _ => None,
                };
                if let Some((mask, bit)) = edit {
                    mask[bit / 16] |= 1 << (bit % 16);
                }
                assert_eq!(changed, expected);
            }
            let mut instrument = AnyInstrument::new(&params);
            assert_eq!(instrument.kind(), kind);
            instrument.prepare(48_000.0, 64);
            assert!(instrument.set_params(&params));
            let wrong = InstrumentKind::ALL[(kind as usize + 1) % InstrumentKind::ALL.len()]
                .default_params();
            assert!(!instrument.set_params(&wrong));
            assert_eq!(
                instrument.latency_samples(),
                params.latency_samples(48_000.0)
            );
            instrument.set_tempo(120.0);
            let id = NoteInstanceId(1);
            let expression = NoteExpression::default();
            instrument.note_on_instance(id, 36, 0.8, 0.0, expression);
            instrument.set_note_expression(id, 0.0, expression);
            instrument.set_note_pitch(id, 36.0);
            let _ = instrument.supports_note_instances();
            instrument.process(&mut [0.0; 64], &mut [0.0; 64]);
            instrument.note_off_instance(id, 36);
            instrument.note_on_expression(36, 0.8, 0.0, expression);
            instrument.note_off(36);
            instrument.note_on(36, 0.8);
            instrument.all_notes_off();
            let _ = (instrument.active_voices(), instrument.tail_samples());
            instrument.reset();
            assert_eq!(instrument.active_voices(), 0);
        }
    }
}
