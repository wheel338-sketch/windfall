//! The interface every built-in effect implements, and what a host needs to
//! run a chain of them: one type that can hold any effect, and a slot that
//! adds bypass and a mix control.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::drive::{
    DriveChain, DriveChainParams, GuitarRack, GuitarRackParams, Overdrive, OverdriveParams,
    Waveshaper, WaveshaperParams,
};
use crate::multiband::{
    BandSplit, BandSplitParams, BassHarmonics, BassHarmonicsParams, Exciter, ExciterParams,
    MultibandCompressor, MultibandCompressorParams, MultibandMaximizer, MultibandMaximizerParams,
    OneKnob, OneKnobParams, TransientShaper, TransientShaperParams, TransientSplit,
    TransientSplitParams,
};
use crate::spatial::{
    BandDelay, BandDelayParams, HyperChorus, HyperChorusParams, Room, RoomParams, Spreader,
    SpreaderParams, StackedFlanger, StackedFlangerParams, StereoEnhancer, StereoEnhancerParams,
    VintageChorus, VintageChorusParams, VintagePhaser, VintagePhaserParams,
};

use crate::performance::{
    PerformanceRack, PerformanceRackParams, Scratch, ScratchParams, TimeTransport,
    TimeTransportParams, VolumeGate, VolumeGateParams,
};

use crate::balance::{Balance, BalanceParams};
use crate::channel_mute::{ChannelMute, ChannelMuteParams};
use crate::control::{EnvelopeFollower, EnvelopeFollowerParams};
use crate::control::{NoteEnvelope, NoteEnvelopeParams};
use crate::control::{PanLfo, PanLfoParams};
use crate::control::{XyPad, XyPadParams};
use crate::control::{XyzPad, XyzPadParams};
use crate::dc_block::{DcBlock, DcBlockParams};
use crate::distortion::{DISTORTION_LATENCY_SAMPLES, Distortion, DistortionParams};
use crate::echo_bank::{EchoBank, EchoBankParams};
use crate::eqbank::{FilterBank, FilterBankParams};
use crate::eqbank::{MorphEq, MorphEqParams};
use crate::eqbank::{SevenBand, SevenBandParams};
use crate::frequency_delay::{FrequencyDelay, FrequencyDelayParams};
use crate::lush::{LushSpace, LushSpaceParams};
use crate::mastering::{StageStack, StageStackParams};
use crate::mastering::{MasterLimiter, MasterLimiterParams, master_limiter_latency};
use crate::polarity::{Polarity, PolarityParams};
use crate::sendtap::{SendTap, SendTapParams};
use crate::soft_clipper::{SoftClipper, SoftClipperParams};
use crate::spectral::{
    CONVOLUTION_PARTITION, Convolver, ConvolverParams, FREQUENCY_SHIFTER_LATENCY, FrequencyShifter,
    FrequencyShifterParams, PITCH_SHIFT_LATENCY, PitchCorrect, PitchCorrectParams, PitchShift,
    PitchShiftParams, Vocoder, VocoderParams,
};
use crate::stereo_matrix::{StereoMatrix, StereoMatrixParams};
use crate::surface::{ControlSurface, ControlSurfaceParams};
use crate::tuner::{Tuner, TunerParams};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::ms_to_samples;
use crate::blocks::smooth::LinearRamp;
use crate::blocks::tap_crossfade::TapCrossfade;
use crate::compressor::{Compressor, CompressorParams};
use crate::delay::{Delay, DelayParams};
use crate::eq::{EqParams, ParametricEq};
use crate::filter_family::{
    BassShelf, BassShelfParams, FastLowpass, FastLowpassParams, SelectableFilter,
    SelectableFilterParams,
};
use crate::limiter::{LOOKAHEAD_FADE_MS, Limiter, LimiterParams};
use crate::lofi::{Lofi, LofiParams};
use crate::modulation::{Chorus, ChorusParams, Flanger, FlangerParams, Phaser, PhaserParams};
use crate::param::{ParamInfo, ParamSet};
use crate::reverb::{Reverb, ReverbParams};

/// A stereo effect that processes audio in place.
///
/// # Threads
///
/// Build an effect, [`prepare`](Effect::prepare) it and set its first
/// parameters away from the audio thread, then hand it over. On the audio
/// thread, [`set_params`](Effect::set_params),
/// [`set_tempo`](Effect::set_tempo), [`reset`](Effect::reset) and
/// [`process`](Effect::process) never allocate, free, lock or block.
/// Dropping an effect frees memory, so send it back to be dropped
/// elsewhere.
///
/// # Parameters
///
/// `set_params` may be called between any two blocks. The effect glides to
/// the new values over a few milliseconds, so a change never clicks. The
/// one exception is the first block after `prepare` or `reset`: values set
/// before it apply at once, so a freshly loaded effect starts where its
/// parameters say and not somewhere on the way from the defaults.
///
/// # Blocks
///
/// The output does not depend on how the audio is divided into blocks:
/// processing a signal in one piece or in pieces of any length gives the
/// same samples.
pub trait Effect: Send {
    type Params: ParamSet;

    /// Sets the sample rate and allocates everything `process` will need.
    /// `max_block` is the longest block the host will pass. This is the
    /// only method besides construction that allocates. It also resets.
    fn prepare(&mut self, sample_rate: f32, max_block: usize);

    /// Clears all memory of past audio, as if the effect had just been
    /// prepared and given its current parameters.
    fn reset(&mut self);

    /// Takes new parameter values. They are forced into range first.
    fn set_params(&mut self, params: &Self::Params);

    /// Tells the effect the project tempo in beats per minute. Effects with
    /// nothing synced to tempo ignore it.
    fn set_tempo(&mut self, bpm: f32) {
        let _ = bpm;
    }

    /// Musical position of the next frame. Default keeps a processor's local
    /// clock. Implementations must preserve history on ordinary block updates.
    fn set_transport(&mut self, _playing: bool, _position_beats: f64) {}

    /// Processes one block in place. The two slices have the same length,
    /// which may be anything from 1 up.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Optional detector-only stereo input; processors opt in explicitly.
    fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        _key: Option<&[[f32; 2]]>,
    ) {
        self.process(left, right);
    }

    /// Samples by which the output lags the input, for delay compensation.
    /// It can change when parameters change, so read it after
    /// `set_params`.
    fn latency_samples(&self) -> usize {
        0
    }

    /// Samples to run unheard after waking from cleared state, so every
    /// delayed output is ready before the slot fades it in. This can exceed
    /// the shared latency used for compensation, for example in a matrix.
    fn warm_up_samples(&self) -> usize {
        self.latency_samples()
    }

    /// History required before starting a live latency-tap edit. This is
    /// separate from reported PDC and includes both outputs of a matrix.
    fn delay_readiness_samples(&self) -> usize {
        self.latency_samples()
    }
    /// Remaining input/fade frames before a requested latency edit settles.
    fn latency_transition_samples_remaining(&self) -> usize {
        0
    }

    /// Samples for which the output can keep sounding after the input has
    /// gone silent, including the latency. A host can stop processing a
    /// track this long after its last input.
    fn tail_samples(&self) -> usize {
        0
    }

    /// Samples for which the output can fall silent and still come back
    /// with nothing new going in: the longest wait for something that is
    /// already inside the effect, such as a delay's next echo, a reverb's
    /// pre-delay or a compressor letting go. It includes the latency. A
    /// host that has seen silence go in and come out for this long can
    /// take the effect to be done, without waiting for all of
    /// [`tail_samples`](Effect::tail_samples).
    fn gap_samples(&self) -> usize {
        self.latency_samples()
    }
}

/// A gain reduction reading that the audio thread writes and any other
/// thread reads, without locking.
///
/// It holds the deepest reduction since it was last read, so a meter that
/// polls 60 times a second still sees a transient that lasted a
/// millisecond.
#[derive(Debug, Clone, Default)]
pub struct GainReductionMeter(Arc<AtomicU32>);

impl GainReductionMeter {
    /// Records a reduction in dB, zero or more, if it is deeper than what
    /// is already held.
    pub(crate) fn raise(&self, reduction_db: f32) {
        // Positive floats order the same way as their bit patterns. Nothing
        // else may get in: the bits of -0.0, which is what a limiter with
        // nothing to do reports, are larger than those of any reduction and
        // would hide every reading until the meter is next read.
        if reduction_db > 0.0 {
            self.0.fetch_max(reduction_db.to_bits(), Ordering::Relaxed);
        }
    }

    /// The deepest gain reduction since the last call, in dB: 0 means the
    /// signal was not turned down, 6 means it was turned down by 6 dB.
    /// Reading resets it.
    pub fn take_db(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

fn performance_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(1.0, 384_000.0)
    } else {
        48_000.0
    }
}

/// Which effect an [`AnyEffect`] or an [`EffectParams`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EffectKind {
    Eq,
    Compressor,
    Limiter,
    Reverb,
    Delay,
    Balance,
    DcBlock,
    ChannelMute,
    Polarity,
    StereoMatrix,
    SoftClipper,
    Distortion,
    FastLowpass,
    SelectableFilter,
    BassShelf,
    Lofi,
    Chorus,
    Flanger,
    Phaser,
    BandSplit,
    MultibandCompressor,
    MultibandMaximizer,
    TransientShaper,
    TransientSplit,
    OneKnob,
    BassHarmonics,
    Exciter,
    Waveshaper,
    Overdrive,
    GuitarRack,
    DriveChain,
    VintageChorus,
    HyperChorus,
    VintagePhaser,
    StackedFlanger,
    BandDelay,
    Room,
    Spreader,
    StereoEnhancer,
    VolumeGate,
    TimeTransport,
    Scratch,
    PerformanceRack,
    Convolver,
    FrequencyShifter,
    PitchShift,
    PitchCorrect,
    Vocoder,
    EchoBank,
    FrequencyDelay,
    SevenBand,
    MorphEq,
    FilterBank,
    XyPad,
    XyzPad,
    PanLfo,
    EnvelopeFollower,
    NoteEnvelope,
    LushSpace,
    Tuner,
    StageStack,
    ControlSurface,
    SendTap,
    MasterLimiter,
}

impl EffectKind {
    pub const ALL: [EffectKind; 64] = [
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Limiter,
        EffectKind::Reverb,
        EffectKind::Delay,
        EffectKind::Balance,
        EffectKind::DcBlock,
        EffectKind::ChannelMute,
        EffectKind::Polarity,
        EffectKind::StereoMatrix,
        EffectKind::SoftClipper,
        EffectKind::Distortion,
        EffectKind::FastLowpass,
        EffectKind::SelectableFilter,
        EffectKind::BassShelf,
        EffectKind::Lofi,
        EffectKind::Chorus,
        EffectKind::Flanger,
        EffectKind::Phaser,
        EffectKind::BandSplit,
        EffectKind::MultibandCompressor,
        EffectKind::MultibandMaximizer,
        EffectKind::TransientShaper,
        EffectKind::TransientSplit,
        EffectKind::OneKnob,
        EffectKind::BassHarmonics,
        EffectKind::Exciter,
        EffectKind::Waveshaper,
        EffectKind::Overdrive,
        EffectKind::GuitarRack,
        EffectKind::DriveChain,
        EffectKind::VintageChorus,
        EffectKind::HyperChorus,
        EffectKind::VintagePhaser,
        EffectKind::StackedFlanger,
        EffectKind::BandDelay,
        EffectKind::Room,
        EffectKind::Spreader,
        EffectKind::StereoEnhancer,
        EffectKind::VolumeGate,
        EffectKind::TimeTransport,
        EffectKind::Scratch,
        EffectKind::PerformanceRack,
        EffectKind::Convolver,
        EffectKind::FrequencyShifter,
        EffectKind::PitchShift,
        EffectKind::PitchCorrect,
        EffectKind::Vocoder,
        EffectKind::EchoBank,
        EffectKind::FrequencyDelay,
        EffectKind::SevenBand,
        EffectKind::MorphEq,
        EffectKind::FilterBank,
        EffectKind::XyPad,
        EffectKind::XyzPad,
        EffectKind::PanLfo,
        EffectKind::EnvelopeFollower,
        EffectKind::NoteEnvelope,
        EffectKind::LushSpace,
        EffectKind::Tuner,
        EffectKind::StageStack,
        EffectKind::ControlSurface,
        EffectKind::SendTap,
        EffectKind::MasterLimiter,
    ];

    /// The effect's name as shown to the user.
    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Eq => EqParams::NAME,
            EffectKind::Compressor => CompressorParams::NAME,
            EffectKind::Limiter => LimiterParams::NAME,
            EffectKind::Reverb => ReverbParams::NAME,
            EffectKind::Delay => DelayParams::NAME,
            EffectKind::Balance => BalanceParams::NAME,
            EffectKind::DcBlock => DcBlockParams::NAME,
            EffectKind::ChannelMute => ChannelMuteParams::NAME,
            EffectKind::Polarity => PolarityParams::NAME,
            EffectKind::StereoMatrix => StereoMatrixParams::NAME,
            EffectKind::SoftClipper => SoftClipperParams::NAME,
            EffectKind::Distortion => DistortionParams::NAME,
            EffectKind::FastLowpass => FastLowpassParams::NAME,
            EffectKind::SelectableFilter => SelectableFilterParams::NAME,
            EffectKind::BassShelf => BassShelfParams::NAME,
            EffectKind::Lofi => LofiParams::NAME,
            EffectKind::Chorus => ChorusParams::NAME,
            EffectKind::Flanger => FlangerParams::NAME,
            EffectKind::Phaser => PhaserParams::NAME,
            EffectKind::BandSplit => BandSplitParams::NAME,
            EffectKind::MultibandCompressor => MultibandCompressorParams::NAME,
            EffectKind::MultibandMaximizer => MultibandMaximizerParams::NAME,
            EffectKind::TransientShaper => TransientShaperParams::NAME,
            EffectKind::TransientSplit => TransientSplitParams::NAME,
            EffectKind::OneKnob => OneKnobParams::NAME,
            EffectKind::BassHarmonics => BassHarmonicsParams::NAME,
            EffectKind::Exciter => ExciterParams::NAME,
            EffectKind::Waveshaper => WaveshaperParams::NAME,
            EffectKind::Overdrive => OverdriveParams::NAME,
            EffectKind::GuitarRack => GuitarRackParams::NAME,
            EffectKind::DriveChain => DriveChainParams::NAME,
            EffectKind::VintageChorus => VintageChorusParams::NAME,
            EffectKind::HyperChorus => HyperChorusParams::NAME,
            EffectKind::VintagePhaser => VintagePhaserParams::NAME,
            EffectKind::StackedFlanger => StackedFlangerParams::NAME,
            EffectKind::BandDelay => BandDelayParams::NAME,
            EffectKind::Room => RoomParams::NAME,
            EffectKind::Spreader => SpreaderParams::NAME,
            EffectKind::StereoEnhancer => StereoEnhancerParams::NAME,
            EffectKind::VolumeGate => VolumeGateParams::NAME,
            EffectKind::TimeTransport => TimeTransportParams::NAME,
            EffectKind::Scratch => ScratchParams::NAME,
            EffectKind::PerformanceRack => PerformanceRackParams::NAME,
            EffectKind::Convolver => ConvolverParams::NAME,
            EffectKind::FrequencyShifter => FrequencyShifterParams::NAME,
            EffectKind::PitchShift => PitchShiftParams::NAME,
            EffectKind::PitchCorrect => PitchCorrectParams::NAME,
            EffectKind::Vocoder => VocoderParams::NAME,
            EffectKind::EchoBank => EchoBankParams::NAME,
            EffectKind::FrequencyDelay => FrequencyDelayParams::NAME,
            EffectKind::SevenBand => SevenBandParams::NAME,
            EffectKind::MorphEq => MorphEqParams::NAME,
            EffectKind::FilterBank => FilterBankParams::NAME,
            EffectKind::XyPad => XyPadParams::NAME,
            EffectKind::XyzPad => XyzPadParams::NAME,
            EffectKind::PanLfo => PanLfoParams::NAME,
            EffectKind::EnvelopeFollower => EnvelopeFollowerParams::NAME,
            EffectKind::NoteEnvelope => NoteEnvelopeParams::NAME,
            EffectKind::LushSpace => LushSpaceParams::NAME,
            EffectKind::Tuner => TunerParams::NAME,
            EffectKind::StageStack => StageStackParams::NAME,
            EffectKind::ControlSurface => ControlSurfaceParams::NAME,
            EffectKind::SendTap => SendTapParams::NAME,
            EffectKind::MasterLimiter => MasterLimiterParams::NAME,
        }
    }

    /// The effect's controls.
    pub fn descriptors(self) -> &'static [ParamInfo] {
        match self {
            EffectKind::Eq => EqParams::descriptors(),
            EffectKind::Compressor => CompressorParams::descriptors(),
            EffectKind::Limiter => LimiterParams::descriptors(),
            EffectKind::Reverb => ReverbParams::descriptors(),
            EffectKind::Delay => DelayParams::descriptors(),
            EffectKind::Balance => BalanceParams::descriptors(),
            EffectKind::DcBlock => DcBlockParams::descriptors(),
            EffectKind::ChannelMute => ChannelMuteParams::descriptors(),
            EffectKind::Polarity => PolarityParams::descriptors(),
            EffectKind::StereoMatrix => StereoMatrixParams::descriptors(),
            EffectKind::SoftClipper => SoftClipperParams::descriptors(),
            EffectKind::Distortion => DistortionParams::descriptors(),
            EffectKind::FastLowpass => FastLowpassParams::descriptors(),
            EffectKind::SelectableFilter => SelectableFilterParams::descriptors(),
            EffectKind::BassShelf => BassShelfParams::descriptors(),
            EffectKind::Lofi => LofiParams::descriptors(),
            EffectKind::Chorus => ChorusParams::descriptors(),
            EffectKind::Flanger => FlangerParams::descriptors(),
            EffectKind::Phaser => PhaserParams::descriptors(),
            EffectKind::BandSplit => BandSplitParams::descriptors(),
            EffectKind::MultibandCompressor => MultibandCompressorParams::descriptors(),
            EffectKind::MultibandMaximizer => MultibandMaximizerParams::descriptors(),
            EffectKind::TransientShaper => TransientShaperParams::descriptors(),
            EffectKind::TransientSplit => TransientSplitParams::descriptors(),
            EffectKind::OneKnob => OneKnobParams::descriptors(),
            EffectKind::BassHarmonics => BassHarmonicsParams::descriptors(),
            EffectKind::Exciter => ExciterParams::descriptors(),
            EffectKind::Waveshaper => WaveshaperParams::descriptors(),
            EffectKind::Overdrive => OverdriveParams::descriptors(),
            EffectKind::GuitarRack => GuitarRackParams::descriptors(),
            EffectKind::DriveChain => DriveChainParams::descriptors(),
            EffectKind::VintageChorus => VintageChorusParams::descriptors(),
            EffectKind::HyperChorus => HyperChorusParams::descriptors(),
            EffectKind::VintagePhaser => VintagePhaserParams::descriptors(),
            EffectKind::StackedFlanger => StackedFlangerParams::descriptors(),
            EffectKind::BandDelay => BandDelayParams::descriptors(),
            EffectKind::Room => RoomParams::descriptors(),
            EffectKind::Spreader => SpreaderParams::descriptors(),
            EffectKind::StereoEnhancer => StereoEnhancerParams::descriptors(),
            EffectKind::VolumeGate => VolumeGateParams::descriptors(),
            EffectKind::TimeTransport => TimeTransportParams::descriptors(),
            EffectKind::Scratch => ScratchParams::descriptors(),
            EffectKind::PerformanceRack => PerformanceRackParams::descriptors(),
            EffectKind::Convolver => ConvolverParams::descriptors(),
            EffectKind::FrequencyShifter => FrequencyShifterParams::descriptors(),
            EffectKind::PitchShift => PitchShiftParams::descriptors(),
            EffectKind::PitchCorrect => PitchCorrectParams::descriptors(),
            EffectKind::Vocoder => VocoderParams::descriptors(),
            EffectKind::EchoBank => EchoBankParams::descriptors(),
            EffectKind::FrequencyDelay => FrequencyDelayParams::descriptors(),
            EffectKind::SevenBand => SevenBandParams::descriptors(),
            EffectKind::MorphEq => MorphEqParams::descriptors(),
            EffectKind::FilterBank => FilterBankParams::descriptors(),
            EffectKind::XyPad => XyPadParams::descriptors(),
            EffectKind::XyzPad => XyzPadParams::descriptors(),
            EffectKind::PanLfo => PanLfoParams::descriptors(),
            EffectKind::EnvelopeFollower => EnvelopeFollowerParams::descriptors(),
            EffectKind::NoteEnvelope => NoteEnvelopeParams::descriptors(),
            EffectKind::LushSpace => LushSpaceParams::descriptors(),
            EffectKind::Tuner => TunerParams::descriptors(),
            EffectKind::StageStack => StageStackParams::descriptors(),
            EffectKind::ControlSurface => ControlSurfaceParams::descriptors(),
            EffectKind::SendTap => SendTapParams::descriptors(),
            EffectKind::MasterLimiter => MasterLimiterParams::descriptors(),
        }
    }

    /// The largest latency any setting of this effect can have at
    /// `sample_rate`.
    pub fn max_latency_samples(self, sample_rate: f32) -> usize {
        match self {
            EffectKind::Limiter => Limiter::max_latency_samples(sample_rate.max(1.0)),
            EffectKind::StereoMatrix => StereoMatrix::max_latency_samples(sample_rate),
            EffectKind::Distortion => DISTORTION_LATENCY_SAMPLES,
            EffectKind::Eq
            | EffectKind::Compressor
            | EffectKind::Reverb
            | EffectKind::Delay
            | EffectKind::Balance
            | EffectKind::DcBlock
            | EffectKind::ChannelMute
            | EffectKind::Polarity
            | EffectKind::SoftClipper
            | EffectKind::FastLowpass
            | EffectKind::SelectableFilter
            | EffectKind::BassShelf
            | EffectKind::Lofi
            | EffectKind::Chorus
            | EffectKind::Flanger
            | EffectKind::Phaser => 0,
            EffectKind::BandSplit => 0,
            EffectKind::MultibandCompressor => 0,
            EffectKind::MultibandMaximizer => {
                2 * ms_to_samples(1.0, crate::balance::rate(sample_rate).max(8.0)) as usize
            }
            EffectKind::TransientShaper => 0,
            EffectKind::TransientSplit => 0,
            EffectKind::OneKnob => {
                2 * ms_to_samples(1.0, crate::balance::rate(sample_rate).max(8.0)) as usize
            }
            EffectKind::BassHarmonics => crate::multiband::HARMONICS_LATENCY_SAMPLES,
            EffectKind::Exciter => crate::multiband::HARMONICS_LATENCY_SAMPLES,
            EffectKind::Waveshaper => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectKind::Overdrive => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectKind::GuitarRack => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectKind::DriveChain => crate::drive::CHAIN_LATENCY_SAMPLES,
            EffectKind::VintageChorus => 0,
            EffectKind::HyperChorus => 0,
            EffectKind::VintagePhaser => 0,
            EffectKind::StackedFlanger => 0,
            EffectKind::BandDelay => 0,
            EffectKind::Room => 0,
            EffectKind::Spreader => 0,
            EffectKind::StereoEnhancer => 0,
            EffectKind::VolumeGate => 0,
            EffectKind::TimeTransport => 0,
            EffectKind::Scratch => 0,
            EffectKind::PerformanceRack => performance_sample_rate(sample_rate).floor() as usize,
            EffectKind::Convolver => CONVOLUTION_PARTITION,
            EffectKind::FrequencyShifter => FREQUENCY_SHIFTER_LATENCY,
            EffectKind::PitchShift => PITCH_SHIFT_LATENCY,
            EffectKind::PitchCorrect => {
                let rate = crate::blocks::math::clean(sample_rate, 8_000.0, 384_000.0, 48_000.0);
                256 * (rate / 6_000.0).round().max(1.0) as usize + PITCH_SHIFT_LATENCY
            }
            EffectKind::Vocoder => 0,
            EffectKind::EchoBank => 0,
            EffectKind::FrequencyDelay => 0,
            EffectKind::SevenBand => 0,
            EffectKind::MorphEq => 0,
            EffectKind::FilterBank => 0,
            EffectKind::XyPad => 0,
            EffectKind::XyzPad => 0,
            EffectKind::PanLfo => 0,
            EffectKind::EnvelopeFollower => 0,
            EffectKind::NoteEnvelope => 0,
            EffectKind::LushSpace => 0,
            EffectKind::Tuner => 0,
            EffectKind::StageStack => 0,
            EffectKind::ControlSurface => 0,
            EffectKind::SendTap => 0,
            EffectKind::MasterLimiter => master_limiter_latency(sample_rate),
        }
    }

    /// The effect's default settings.
    pub fn default_params(self) -> EffectParams {
        match self {
            EffectKind::Eq => EffectParams::Eq(EqParams::default()),
            EffectKind::Compressor => EffectParams::Compressor(CompressorParams::default()),
            EffectKind::Limiter => EffectParams::Limiter(LimiterParams::default()),
            EffectKind::Reverb => EffectParams::Reverb(ReverbParams::default()),
            EffectKind::Delay => EffectParams::Delay(DelayParams::default()),
            EffectKind::Balance => EffectParams::Balance(BalanceParams::default()),
            EffectKind::DcBlock => EffectParams::DcBlock(DcBlockParams::default()),
            EffectKind::ChannelMute => EffectParams::ChannelMute(ChannelMuteParams::default()),
            EffectKind::Polarity => EffectParams::Polarity(PolarityParams::default()),
            EffectKind::StereoMatrix => EffectParams::StereoMatrix(StereoMatrixParams::default()),
            EffectKind::SoftClipper => EffectParams::SoftClipper(SoftClipperParams::default()),
            EffectKind::Distortion => EffectParams::Distortion(DistortionParams::default()),
            EffectKind::FastLowpass => EffectParams::FastLowpass(FastLowpassParams::default()),
            EffectKind::SelectableFilter => {
                EffectParams::SelectableFilter(SelectableFilterParams::default())
            }
            EffectKind::BassShelf => EffectParams::BassShelf(BassShelfParams::default()),
            EffectKind::Lofi => EffectParams::Lofi(LofiParams::default()),
            EffectKind::Chorus => EffectParams::Chorus(ChorusParams::default()),
            EffectKind::Flanger => EffectParams::Flanger(FlangerParams::default()),
            EffectKind::Phaser => EffectParams::Phaser(PhaserParams::default()),
            EffectKind::BandSplit => EffectParams::BandSplit(BandSplitParams::default()),
            EffectKind::MultibandCompressor => {
                EffectParams::MultibandCompressor(MultibandCompressorParams::default())
            }
            EffectKind::MultibandMaximizer => {
                EffectParams::MultibandMaximizer(MultibandMaximizerParams::default())
            }
            EffectKind::TransientShaper => {
                EffectParams::TransientShaper(TransientShaperParams::default())
            }
            EffectKind::TransientSplit => {
                EffectParams::TransientSplit(TransientSplitParams::default())
            }
            EffectKind::OneKnob => EffectParams::OneKnob(OneKnobParams::default()),
            EffectKind::BassHarmonics => {
                EffectParams::BassHarmonics(BassHarmonicsParams::default())
            }
            EffectKind::Exciter => EffectParams::Exciter(ExciterParams::default()),
            EffectKind::Waveshaper => EffectParams::Waveshaper(WaveshaperParams::default()),
            EffectKind::Overdrive => EffectParams::Overdrive(OverdriveParams::default()),
            EffectKind::GuitarRack => EffectParams::GuitarRack(GuitarRackParams::default()),
            EffectKind::DriveChain => EffectParams::DriveChain(DriveChainParams::default()),
            EffectKind::VintageChorus => {
                EffectParams::VintageChorus(VintageChorusParams::default())
            }
            EffectKind::HyperChorus => EffectParams::HyperChorus(HyperChorusParams::default()),
            EffectKind::VintagePhaser => {
                EffectParams::VintagePhaser(VintagePhaserParams::default())
            }
            EffectKind::StackedFlanger => {
                EffectParams::StackedFlanger(StackedFlangerParams::default())
            }
            EffectKind::BandDelay => EffectParams::BandDelay(BandDelayParams::default()),
            EffectKind::Room => EffectParams::Room(RoomParams::default()),
            EffectKind::Spreader => EffectParams::Spreader(SpreaderParams::default()),
            EffectKind::StereoEnhancer => {
                EffectParams::StereoEnhancer(StereoEnhancerParams::default())
            }
            EffectKind::VolumeGate => EffectParams::VolumeGate(VolumeGateParams::default()),
            EffectKind::TimeTransport => {
                EffectParams::TimeTransport(TimeTransportParams::default())
            }
            EffectKind::Scratch => EffectParams::Scratch(ScratchParams::default()),
            EffectKind::PerformanceRack => {
                EffectParams::PerformanceRack(PerformanceRackParams::default())
            }
            EffectKind::Convolver => EffectParams::Convolver(ConvolverParams::default()),
            EffectKind::FrequencyShifter => {
                EffectParams::FrequencyShifter(FrequencyShifterParams::default())
            }
            EffectKind::PitchShift => EffectParams::PitchShift(PitchShiftParams::default()),
            EffectKind::PitchCorrect => EffectParams::PitchCorrect(PitchCorrectParams::default()),
            EffectKind::Vocoder => EffectParams::Vocoder(VocoderParams::default()),
            EffectKind::EchoBank => EffectParams::EchoBank(EchoBankParams::default()),
            EffectKind::FrequencyDelay => {
                EffectParams::FrequencyDelay(FrequencyDelayParams::default())
            }
            EffectKind::SevenBand => EffectParams::SevenBand(SevenBandParams::default()),
            EffectKind::MorphEq => EffectParams::MorphEq(MorphEqParams::default()),
            EffectKind::FilterBank => EffectParams::FilterBank(FilterBankParams::default()),
            EffectKind::XyPad => EffectParams::XyPad(XyPadParams::default()),
            EffectKind::XyzPad => EffectParams::XyzPad(XyzPadParams::default()),
            EffectKind::PanLfo => EffectParams::PanLfo(PanLfoParams::default()),
            EffectKind::EnvelopeFollower => {
                EffectParams::EnvelopeFollower(EnvelopeFollowerParams::default())
            }
            EffectKind::NoteEnvelope => EffectParams::NoteEnvelope(NoteEnvelopeParams::default()),
            EffectKind::LushSpace => EffectParams::LushSpace(LushSpaceParams::default()),
            EffectKind::Tuner => EffectParams::Tuner(TunerParams::default()),
            EffectKind::StageStack => EffectParams::StageStack(StageStackParams::default()),
            EffectKind::ControlSurface => {
                EffectParams::ControlSurface(ControlSurfaceParams::default())
            }
            EffectKind::SendTap => EffectParams::SendTap(SendTapParams::default()),
            EffectKind::MasterLimiter => {
                EffectParams::MasterLimiter(MasterLimiterParams::default())
            }
        }
    }
}

/// The settings of any one effect. This is what a project stores for an
/// effect slot.
// Inline fixed-size parameters remain Copy for allocation-free realtime updates;
// boxing the convolver impulse would change that contract and parameter ownership.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum EffectParams {
    Eq(EqParams),
    Compressor(CompressorParams),
    Limiter(LimiterParams),
    Reverb(ReverbParams),
    Delay(DelayParams),
    Balance(BalanceParams),
    DcBlock(DcBlockParams),
    ChannelMute(ChannelMuteParams),
    Polarity(PolarityParams),
    StereoMatrix(StereoMatrixParams),
    SoftClipper(SoftClipperParams),
    Distortion(DistortionParams),
    FastLowpass(FastLowpassParams),
    SelectableFilter(SelectableFilterParams),
    BassShelf(BassShelfParams),
    Lofi(LofiParams),
    Chorus(ChorusParams),
    Flanger(FlangerParams),
    Phaser(PhaserParams),
    BandSplit(BandSplitParams),
    MultibandCompressor(MultibandCompressorParams),
    MultibandMaximizer(MultibandMaximizerParams),
    TransientShaper(TransientShaperParams),
    TransientSplit(TransientSplitParams),
    OneKnob(OneKnobParams),
    BassHarmonics(BassHarmonicsParams),
    Exciter(ExciterParams),
    Waveshaper(WaveshaperParams),
    Overdrive(OverdriveParams),
    GuitarRack(GuitarRackParams),
    DriveChain(DriveChainParams),
    VintageChorus(VintageChorusParams),
    HyperChorus(HyperChorusParams),
    VintagePhaser(VintagePhaserParams),
    StackedFlanger(StackedFlangerParams),
    BandDelay(BandDelayParams),
    Room(RoomParams),
    Spreader(SpreaderParams),
    StereoEnhancer(StereoEnhancerParams),
    VolumeGate(VolumeGateParams),
    TimeTransport(TimeTransportParams),
    Scratch(ScratchParams),
    PerformanceRack(PerformanceRackParams),
    Convolver(ConvolverParams),
    FrequencyShifter(FrequencyShifterParams),
    PitchShift(PitchShiftParams),
    PitchCorrect(PitchCorrectParams),
    Vocoder(VocoderParams),
    EchoBank(EchoBankParams),
    FrequencyDelay(FrequencyDelayParams),
    SevenBand(SevenBandParams),
    MorphEq(MorphEqParams),
    FilterBank(FilterBankParams),
    XyPad(XyPadParams),
    XyzPad(XyzPadParams),
    PanLfo(PanLfoParams),
    EnvelopeFollower(EnvelopeFollowerParams),
    NoteEnvelope(NoteEnvelopeParams),
    LushSpace(LushSpaceParams),
    Tuner(TunerParams),
    StageStack(StageStackParams),
    ControlSurface(ControlSurfaceParams),
    SendTap(SendTapParams),
    MasterLimiter(MasterLimiterParams),
}

/// Runs `$body` with `$params` bound to the settings inside an
/// [`EffectParams`], whichever effect they belong to.
macro_rules! each_params {
    ($value:expr, $params:ident => $body:expr) => {
        match $value {
            EffectParams::Eq($params) => $body,
            EffectParams::Compressor($params) => $body,
            EffectParams::Limiter($params) => $body,
            EffectParams::Reverb($params) => $body,
            EffectParams::Delay($params) => $body,
            EffectParams::Balance($params) => $body,
            EffectParams::DcBlock($params) => $body,
            EffectParams::ChannelMute($params) => $body,
            EffectParams::Polarity($params) => $body,
            EffectParams::StereoMatrix($params) => $body,
            EffectParams::SoftClipper($params) => $body,
            EffectParams::Distortion($params) => $body,
            EffectParams::FastLowpass($params) => $body,
            EffectParams::SelectableFilter($params) => $body,
            EffectParams::BassShelf($params) => $body,
            EffectParams::Lofi($params) => $body,
            EffectParams::Chorus($params) => $body,
            EffectParams::Flanger($params) => $body,
            EffectParams::Phaser($params) => $body,
            EffectParams::BandSplit($params) => $body,
            EffectParams::MultibandCompressor($params) => $body,
            EffectParams::MultibandMaximizer($params) => $body,
            EffectParams::TransientShaper($params) => $body,
            EffectParams::TransientSplit($params) => $body,
            EffectParams::OneKnob($params) => $body,
            EffectParams::BassHarmonics($params) => $body,
            EffectParams::Exciter($params) => $body,
            EffectParams::Waveshaper($params) => $body,
            EffectParams::Overdrive($params) => $body,
            EffectParams::GuitarRack($params) => $body,
            EffectParams::DriveChain($params) => $body,
            EffectParams::VintageChorus($params) => $body,
            EffectParams::HyperChorus($params) => $body,
            EffectParams::VintagePhaser($params) => $body,
            EffectParams::StackedFlanger($params) => $body,
            EffectParams::BandDelay($params) => $body,
            EffectParams::Room($params) => $body,
            EffectParams::Spreader($params) => $body,
            EffectParams::StereoEnhancer($params) => $body,
            EffectParams::VolumeGate($params) => $body,
            EffectParams::TimeTransport($params) => $body,
            EffectParams::Scratch($params) => $body,
            EffectParams::PerformanceRack($params) => $body,
            EffectParams::Convolver($params) => $body,
            EffectParams::FrequencyShifter($params) => $body,
            EffectParams::PitchShift($params) => $body,
            EffectParams::PitchCorrect($params) => $body,
            EffectParams::Vocoder($params) => $body,
            EffectParams::EchoBank($params) => $body,
            EffectParams::FrequencyDelay($params) => $body,
            EffectParams::SevenBand($params) => $body,
            EffectParams::MorphEq($params) => $body,
            EffectParams::FilterBank($params) => $body,
            EffectParams::XyPad($params) => $body,
            EffectParams::XyzPad($params) => $body,
            EffectParams::PanLfo($params) => $body,
            EffectParams::EnvelopeFollower($params) => $body,
            EffectParams::NoteEnvelope($params) => $body,
            EffectParams::LushSpace($params) => $body,
            EffectParams::Tuner($params) => $body,
            EffectParams::StageStack($params) => $body,
            EffectParams::ControlSurface($params) => $body,
            EffectParams::SendTap($params) => $body,
            EffectParams::MasterLimiter($params) => $body,
        }
    };
}

impl EffectParams {
    pub fn kind(&self) -> EffectKind {
        match self {
            EffectParams::Eq(_) => EffectKind::Eq,
            EffectParams::Compressor(_) => EffectKind::Compressor,
            EffectParams::Limiter(_) => EffectKind::Limiter,
            EffectParams::Reverb(_) => EffectKind::Reverb,
            EffectParams::Delay(_) => EffectKind::Delay,
            EffectParams::Balance(_) => EffectKind::Balance,
            EffectParams::DcBlock(_) => EffectKind::DcBlock,
            EffectParams::ChannelMute(_) => EffectKind::ChannelMute,
            EffectParams::Polarity(_) => EffectKind::Polarity,
            EffectParams::StereoMatrix(_) => EffectKind::StereoMatrix,
            EffectParams::SoftClipper(_) => EffectKind::SoftClipper,
            EffectParams::Distortion(_) => EffectKind::Distortion,
            EffectParams::FastLowpass(_) => EffectKind::FastLowpass,
            EffectParams::SelectableFilter(_) => EffectKind::SelectableFilter,
            EffectParams::BassShelf(_) => EffectKind::BassShelf,
            EffectParams::Lofi(_) => EffectKind::Lofi,
            EffectParams::Chorus(_) => EffectKind::Chorus,
            EffectParams::Flanger(_) => EffectKind::Flanger,
            EffectParams::Phaser(_) => EffectKind::Phaser,
            EffectParams::BandSplit(_) => EffectKind::BandSplit,
            EffectParams::MultibandCompressor(_) => EffectKind::MultibandCompressor,
            EffectParams::MultibandMaximizer(_) => EffectKind::MultibandMaximizer,
            EffectParams::TransientShaper(_) => EffectKind::TransientShaper,
            EffectParams::TransientSplit(_) => EffectKind::TransientSplit,
            EffectParams::OneKnob(_) => EffectKind::OneKnob,
            EffectParams::BassHarmonics(_) => EffectKind::BassHarmonics,
            EffectParams::Exciter(_) => EffectKind::Exciter,
            EffectParams::Waveshaper(_) => EffectKind::Waveshaper,
            EffectParams::Overdrive(_) => EffectKind::Overdrive,
            EffectParams::GuitarRack(_) => EffectKind::GuitarRack,
            EffectParams::DriveChain(_) => EffectKind::DriveChain,
            EffectParams::VintageChorus(_) => EffectKind::VintageChorus,
            EffectParams::HyperChorus(_) => EffectKind::HyperChorus,
            EffectParams::VintagePhaser(_) => EffectKind::VintagePhaser,
            EffectParams::StackedFlanger(_) => EffectKind::StackedFlanger,
            EffectParams::BandDelay(_) => EffectKind::BandDelay,
            EffectParams::Room(_) => EffectKind::Room,
            EffectParams::Spreader(_) => EffectKind::Spreader,
            EffectParams::StereoEnhancer(_) => EffectKind::StereoEnhancer,
            EffectParams::VolumeGate(_) => EffectKind::VolumeGate,
            EffectParams::TimeTransport(_) => EffectKind::TimeTransport,
            EffectParams::Scratch(_) => EffectKind::Scratch,
            EffectParams::PerformanceRack(_) => EffectKind::PerformanceRack,
            EffectParams::Convolver(_) => EffectKind::Convolver,
            EffectParams::FrequencyShifter(_) => EffectKind::FrequencyShifter,
            EffectParams::PitchShift(_) => EffectKind::PitchShift,
            EffectParams::PitchCorrect(_) => EffectKind::PitchCorrect,
            EffectParams::Vocoder(_) => EffectKind::Vocoder,
            EffectParams::EchoBank(_) => EffectKind::EchoBank,
            EffectParams::FrequencyDelay(_) => EffectKind::FrequencyDelay,
            EffectParams::SevenBand(_) => EffectKind::SevenBand,
            EffectParams::MorphEq(_) => EffectKind::MorphEq,
            EffectParams::FilterBank(_) => EffectKind::FilterBank,
            EffectParams::XyPad(_) => EffectKind::XyPad,
            EffectParams::XyzPad(_) => EffectKind::XyzPad,
            EffectParams::PanLfo(_) => EffectKind::PanLfo,
            EffectParams::EnvelopeFollower(_) => EffectKind::EnvelopeFollower,
            EffectParams::NoteEnvelope(_) => EffectKind::NoteEnvelope,
            EffectParams::LushSpace(_) => EffectKind::LushSpace,
            EffectParams::Tuner(_) => EffectKind::Tuner,
            EffectParams::StageStack(_) => EffectKind::StageStack,
            EffectParams::ControlSurface(_) => EffectKind::ControlSurface,
            EffectParams::SendTap(_) => EffectKind::SendTap,
            EffectParams::MasterLimiter(_) => EffectKind::MasterLimiter,
        }
    }

    /// A copy with every value forced into its range.
    pub fn sanitized(&self) -> Self {
        match self {
            EffectParams::Eq(params) => EffectParams::Eq(params.sanitized()),
            EffectParams::Compressor(params) => EffectParams::Compressor(params.sanitized()),
            EffectParams::Limiter(params) => EffectParams::Limiter(params.sanitized()),
            EffectParams::Reverb(params) => EffectParams::Reverb(params.sanitized()),
            EffectParams::Delay(params) => EffectParams::Delay(params.sanitized()),
            EffectParams::Balance(params) => EffectParams::Balance(params.sanitized()),
            EffectParams::DcBlock(params) => EffectParams::DcBlock(params.sanitized()),
            EffectParams::ChannelMute(params) => EffectParams::ChannelMute(params.sanitized()),
            EffectParams::Polarity(params) => EffectParams::Polarity(params.sanitized()),
            EffectParams::StereoMatrix(params) => EffectParams::StereoMatrix(params.sanitized()),
            EffectParams::SoftClipper(params) => EffectParams::SoftClipper(params.sanitized()),
            EffectParams::Distortion(params) => EffectParams::Distortion(params.sanitized()),
            EffectParams::FastLowpass(params) => EffectParams::FastLowpass(params.sanitized()),
            EffectParams::SelectableFilter(params) => {
                EffectParams::SelectableFilter(params.sanitized())
            }
            EffectParams::BassShelf(params) => EffectParams::BassShelf(params.sanitized()),
            EffectParams::Lofi(params) => EffectParams::Lofi(params.sanitized()),
            EffectParams::Chorus(params) => EffectParams::Chorus(params.sanitized()),
            EffectParams::Flanger(params) => EffectParams::Flanger(params.sanitized()),
            EffectParams::Phaser(params) => EffectParams::Phaser(params.sanitized()),
            EffectParams::BandSplit(params) => EffectParams::BandSplit(params.sanitized()),
            EffectParams::MultibandCompressor(params) => {
                EffectParams::MultibandCompressor(params.sanitized())
            }
            EffectParams::MultibandMaximizer(params) => {
                EffectParams::MultibandMaximizer(params.sanitized())
            }
            EffectParams::TransientShaper(params) => {
                EffectParams::TransientShaper(params.sanitized())
            }
            EffectParams::TransientSplit(params) => {
                EffectParams::TransientSplit(params.sanitized())
            }
            EffectParams::OneKnob(params) => EffectParams::OneKnob(params.sanitized()),
            EffectParams::BassHarmonics(params) => EffectParams::BassHarmonics(params.sanitized()),
            EffectParams::Exciter(params) => EffectParams::Exciter(params.sanitized()),
            EffectParams::Waveshaper(params) => EffectParams::Waveshaper(params.sanitized()),
            EffectParams::Overdrive(params) => EffectParams::Overdrive(params.sanitized()),
            EffectParams::GuitarRack(params) => EffectParams::GuitarRack(params.sanitized()),
            EffectParams::DriveChain(params) => EffectParams::DriveChain(params.sanitized()),
            EffectParams::VintageChorus(params) => EffectParams::VintageChorus(params.sanitized()),
            EffectParams::HyperChorus(params) => EffectParams::HyperChorus(params.sanitized()),
            EffectParams::VintagePhaser(params) => EffectParams::VintagePhaser(params.sanitized()),
            EffectParams::StackedFlanger(params) => {
                EffectParams::StackedFlanger(params.sanitized())
            }
            EffectParams::BandDelay(params) => EffectParams::BandDelay(params.sanitized()),
            EffectParams::Room(params) => EffectParams::Room(params.sanitized()),
            EffectParams::Spreader(params) => EffectParams::Spreader(params.sanitized()),
            EffectParams::StereoEnhancer(params) => {
                EffectParams::StereoEnhancer(params.sanitized())
            }
            EffectParams::VolumeGate(params) => EffectParams::VolumeGate(params.sanitized()),
            EffectParams::TimeTransport(params) => EffectParams::TimeTransport(params.sanitized()),
            EffectParams::Scratch(params) => EffectParams::Scratch(params.sanitized()),
            EffectParams::PerformanceRack(params) => {
                EffectParams::PerformanceRack(params.sanitized())
            }
            EffectParams::Convolver(params) => EffectParams::Convolver(params.sanitized()),
            EffectParams::FrequencyShifter(params) => {
                EffectParams::FrequencyShifter(params.sanitized())
            }
            EffectParams::PitchShift(params) => EffectParams::PitchShift(params.sanitized()),
            EffectParams::PitchCorrect(params) => EffectParams::PitchCorrect(params.sanitized()),
            EffectParams::Vocoder(params) => EffectParams::Vocoder(params.sanitized()),
            EffectParams::EchoBank(params) => EffectParams::EchoBank(params.sanitized()),
            EffectParams::FrequencyDelay(params) => {
                EffectParams::FrequencyDelay(params.sanitized())
            }
            EffectParams::SevenBand(params) => EffectParams::SevenBand(params.sanitized()),
            EffectParams::MorphEq(params) => EffectParams::MorphEq(params.sanitized()),
            EffectParams::FilterBank(params) => EffectParams::FilterBank(params.sanitized()),
            EffectParams::XyPad(params) => EffectParams::XyPad(params.sanitized()),
            EffectParams::XyzPad(params) => EffectParams::XyzPad(params.sanitized()),
            EffectParams::PanLfo(params) => EffectParams::PanLfo(params.sanitized()),
            EffectParams::EnvelopeFollower(params) => {
                EffectParams::EnvelopeFollower(params.sanitized())
            }
            EffectParams::NoteEnvelope(params) => EffectParams::NoteEnvelope(params.sanitized()),
            EffectParams::LushSpace(params) => EffectParams::LushSpace(params.sanitized()),
            EffectParams::Tuner(params) => EffectParams::Tuner(params.sanitized()),
            EffectParams::StageStack(params) => EffectParams::StageStack(params.sanitized()),
            EffectParams::ControlSurface(params) => {
                EffectParams::ControlSurface(params.sanitized())
            }
            EffectParams::SendTap(params) => EffectParams::SendTap(params.sanitized()),
            EffectParams::MasterLimiter(params) => {
                EffectParams::MasterLimiter(params.sanitized())
            }
        }
    }

    /// The latency a prepared effect has with these settings at
    /// `sample_rate`, which is what its
    /// [`latency_samples`](Effect::latency_samples) reports once it has
    /// been given them. A host can plan delay compensation from this
    /// without holding the effect.
    pub fn latency_samples(&self, sample_rate: f32) -> usize {
        match self {
            EffectParams::Limiter(params) => params.latency_samples(sample_rate),
            EffectParams::StereoMatrix(params) => params.latency_samples(sample_rate),
            EffectParams::Distortion(_) => DISTORTION_LATENCY_SAMPLES,
            EffectParams::Eq(_)
            | EffectParams::Compressor(_)
            | EffectParams::Reverb(_)
            | EffectParams::Delay(_)
            | EffectParams::Balance(_)
            | EffectParams::DcBlock(_)
            | EffectParams::ChannelMute(_)
            | EffectParams::Polarity(_)
            | EffectParams::SoftClipper(_)
            | EffectParams::FastLowpass(_)
            | EffectParams::SelectableFilter(_)
            | EffectParams::BassShelf(_)
            | EffectParams::Lofi(_)
            | EffectParams::Chorus(_)
            | EffectParams::Flanger(_)
            | EffectParams::Phaser(_) => 0,
            EffectParams::BandSplit(_) => 0,
            EffectParams::MultibandCompressor(_) => 0,
            EffectParams::MultibandMaximizer(_) => {
                2 * ms_to_samples(1.0, crate::balance::rate(sample_rate).max(8.0)) as usize
            }
            EffectParams::TransientShaper(_) => 0,
            EffectParams::TransientSplit(_) => 0,
            EffectParams::OneKnob(_) => {
                2 * ms_to_samples(1.0, crate::balance::rate(sample_rate).max(8.0)) as usize
            }
            EffectParams::BassHarmonics(_) => crate::multiband::HARMONICS_LATENCY_SAMPLES,
            EffectParams::Exciter(_) => crate::multiband::HARMONICS_LATENCY_SAMPLES,
            EffectParams::Waveshaper(_) => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectParams::Overdrive(_) => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectParams::GuitarRack(_) => crate::drive::DRIVE_LATENCY_SAMPLES,
            EffectParams::DriveChain(_) => crate::drive::CHAIN_LATENCY_SAMPLES,
            EffectParams::VintageChorus(_) => 0,
            EffectParams::HyperChorus(_) => 0,
            EffectParams::VintagePhaser(_) => 0,
            EffectParams::StackedFlanger(_) => 0,
            EffectParams::BandDelay(_) => 0,
            EffectParams::Room(_) => 0,
            EffectParams::Spreader(_) => 0,
            EffectParams::StereoEnhancer(_) => 0,
            EffectParams::VolumeGate(_) => 0,
            EffectParams::TimeTransport(_) => 0,
            EffectParams::Scratch(_) => 0,
            EffectParams::PerformanceRack(params) => {
                // The registry has no tempo input; processors start at 120 BPM.
                let rate = performance_sample_rate(sample_rate) as f64;
                (rate * 0.5 * params.sanitized().loop_beats as f64)
                    .round()
                    .max(1.0)
                    .min(rate.floor()) as usize
            }
            EffectParams::Convolver(_) => EffectKind::Convolver.max_latency_samples(sample_rate),
            EffectParams::FrequencyShifter(_) => {
                EffectKind::FrequencyShifter.max_latency_samples(sample_rate)
            }
            EffectParams::PitchShift(_) => EffectKind::PitchShift.max_latency_samples(sample_rate),
            EffectParams::PitchCorrect(_) => {
                EffectKind::PitchCorrect.max_latency_samples(sample_rate)
            }
            EffectParams::Vocoder(_) => EffectKind::Vocoder.max_latency_samples(sample_rate),
            EffectParams::EchoBank(_) => EffectKind::EchoBank.max_latency_samples(sample_rate),
            EffectParams::FrequencyDelay(_) => {
                EffectKind::FrequencyDelay.max_latency_samples(sample_rate)
            }
            EffectParams::SevenBand(_) => EffectKind::SevenBand.max_latency_samples(sample_rate),
            EffectParams::MorphEq(_) => EffectKind::MorphEq.max_latency_samples(sample_rate),
            EffectParams::FilterBank(_) => EffectKind::FilterBank.max_latency_samples(sample_rate),
            EffectParams::XyPad(_) => EffectKind::XyPad.max_latency_samples(sample_rate),
            EffectParams::XyzPad(_) => EffectKind::XyzPad.max_latency_samples(sample_rate),
            EffectParams::PanLfo(_) => EffectKind::PanLfo.max_latency_samples(sample_rate),
            EffectParams::EnvelopeFollower(_) => {
                EffectKind::EnvelopeFollower.max_latency_samples(sample_rate)
            }
            EffectParams::NoteEnvelope(_) => {
                EffectKind::NoteEnvelope.max_latency_samples(sample_rate)
            }
            EffectParams::LushSpace(_) => EffectKind::LushSpace.max_latency_samples(sample_rate),
            EffectParams::Tuner(_) => EffectKind::Tuner.max_latency_samples(sample_rate),
            EffectParams::StageStack(_) => EffectKind::StageStack.max_latency_samples(sample_rate),
            EffectParams::ControlSurface(_) => {
                EffectKind::ControlSurface.max_latency_samples(sample_rate)
            }
            EffectParams::SendTap(_) => EffectKind::SendTap.max_latency_samples(sample_rate),
            EffectParams::MasterLimiter(_) => master_limiter_latency(sample_rate),
        }
    }

    /// The value of the control at `index` of the effect's
    /// [descriptors](EffectKind::descriptors).
    pub fn get(&self, index: usize) -> Option<f32> {
        each_params!(self, params => params.get(index))
    }

    /// Sets the control at `index`, forcing the value into range. Returns
    /// false if there is no such control. Does not allocate, so automation
    /// can call it on the audio thread.
    pub fn set(&mut self, index: usize, value: f32) -> bool {
        each_params!(self, params => params.set(index, value))
    }
}

/// Any one of the built-in effects, behind one type, so a host can keep a
/// chain whose contents are chosen at run time.
///
/// The effect itself lives on the heap, which keeps this small enough to
/// pass through a queue. Create it, prepare it and set its parameters off
/// the audio thread; and since dropping it frees that memory, send it back
/// off the audio thread to be dropped.
pub enum AnyEffect {
    Eq(Box<ParametricEq>),
    Compressor(Box<Compressor>),
    Limiter(Box<Limiter>),
    Reverb(Box<Reverb>),
    Delay(Box<Delay>),
    Balance(Box<Balance>),
    DcBlock(Box<DcBlock>),
    ChannelMute(Box<ChannelMute>),
    Polarity(Box<Polarity>),
    StereoMatrix(Box<StereoMatrix>),
    SoftClipper(Box<SoftClipper>),
    Distortion(Box<Distortion>),
    FastLowpass(Box<FastLowpass>),
    SelectableFilter(Box<SelectableFilter>),
    BassShelf(Box<BassShelf>),
    Lofi(Box<Lofi>),
    Chorus(Box<Chorus>),
    Flanger(Box<Flanger>),
    Phaser(Box<Phaser>),
    BandSplit(Box<BandSplit>),
    MultibandCompressor(Box<MultibandCompressor>),
    MultibandMaximizer(Box<MultibandMaximizer>),
    TransientShaper(Box<TransientShaper>),
    TransientSplit(Box<TransientSplit>),
    OneKnob(Box<OneKnob>),
    BassHarmonics(Box<BassHarmonics>),
    Exciter(Box<Exciter>),
    Waveshaper(Box<Waveshaper>),
    Overdrive(Box<Overdrive>),
    GuitarRack(Box<GuitarRack>),
    DriveChain(Box<DriveChain>),
    VintageChorus(Box<VintageChorus>),
    HyperChorus(Box<HyperChorus>),
    VintagePhaser(Box<VintagePhaser>),
    StackedFlanger(Box<StackedFlanger>),
    BandDelay(Box<BandDelay>),
    Room(Box<Room>),
    Spreader(Box<Spreader>),
    StereoEnhancer(Box<StereoEnhancer>),
    VolumeGate(Box<VolumeGate>),
    TimeTransport(Box<TimeTransport>),
    Scratch(Box<Scratch>),
    PerformanceRack(Box<PerformanceRack>),
    Convolver(Box<Convolver>),
    FrequencyShifter(Box<FrequencyShifter>),
    PitchShift(Box<PitchShift>),
    PitchCorrect(Box<PitchCorrect>),
    Vocoder(Box<Vocoder>),
    EchoBank(Box<EchoBank>),
    FrequencyDelay(Box<FrequencyDelay>),
    SevenBand(Box<SevenBand>),
    MorphEq(Box<MorphEq>),
    FilterBank(Box<FilterBank>),
    XyPad(Box<XyPad>),
    XyzPad(Box<XyzPad>),
    PanLfo(Box<PanLfo>),
    EnvelopeFollower(Box<EnvelopeFollower>),
    NoteEnvelope(Box<NoteEnvelope>),
    LushSpace(Box<LushSpace>),
    Tuner(Box<Tuner>),
    StageStack(Box<StageStack>),
    ControlSurface(Box<ControlSurface>),
    SendTap(Box<SendTap>),
    MasterLimiter(Box<MasterLimiter>),
}

/// Runs `$body` with `$effect` bound to the effect inside an [`AnyEffect`].
macro_rules! each_effect {
    ($value:expr, $effect:ident => $body:expr) => {
        match $value {
            AnyEffect::Eq($effect) => $body,
            AnyEffect::Compressor($effect) => $body,
            AnyEffect::Limiter($effect) => $body,
            AnyEffect::Reverb($effect) => $body,
            AnyEffect::Delay($effect) => $body,
            AnyEffect::Balance($effect) => $body,
            AnyEffect::DcBlock($effect) => $body,
            AnyEffect::ChannelMute($effect) => $body,
            AnyEffect::Polarity($effect) => $body,
            AnyEffect::StereoMatrix($effect) => $body,
            AnyEffect::SoftClipper($effect) => $body,
            AnyEffect::Distortion($effect) => $body,
            AnyEffect::FastLowpass($effect) => $body,
            AnyEffect::SelectableFilter($effect) => $body,
            AnyEffect::BassShelf($effect) => $body,
            AnyEffect::Lofi($effect) => $body,
            AnyEffect::Chorus($effect) => $body,
            AnyEffect::Flanger($effect) => $body,
            AnyEffect::Phaser($effect) => $body,
            AnyEffect::BandSplit($effect) => $body,
            AnyEffect::MultibandCompressor($effect) => $body,
            AnyEffect::MultibandMaximizer($effect) => $body,
            AnyEffect::TransientShaper($effect) => $body,
            AnyEffect::TransientSplit($effect) => $body,
            AnyEffect::OneKnob($effect) => $body,
            AnyEffect::BassHarmonics($effect) => $body,
            AnyEffect::Exciter($effect) => $body,
            AnyEffect::Waveshaper($effect) => $body,
            AnyEffect::Overdrive($effect) => $body,
            AnyEffect::GuitarRack($effect) => $body,
            AnyEffect::DriveChain($effect) => $body,
            AnyEffect::VintageChorus($effect) => $body,
            AnyEffect::HyperChorus($effect) => $body,
            AnyEffect::VintagePhaser($effect) => $body,
            AnyEffect::StackedFlanger($effect) => $body,
            AnyEffect::BandDelay($effect) => $body,
            AnyEffect::Room($effect) => $body,
            AnyEffect::Spreader($effect) => $body,
            AnyEffect::StereoEnhancer($effect) => $body,
            AnyEffect::VolumeGate($effect) => $body,
            AnyEffect::TimeTransport($effect) => $body,
            AnyEffect::Scratch($effect) => $body,
            AnyEffect::PerformanceRack($effect) => $body,
            AnyEffect::Convolver($effect) => $body,
            AnyEffect::FrequencyShifter($effect) => $body,
            AnyEffect::PitchShift($effect) => $body,
            AnyEffect::PitchCorrect($effect) => $body,
            AnyEffect::Vocoder($effect) => $body,
            AnyEffect::EchoBank($effect) => $body,
            AnyEffect::FrequencyDelay($effect) => $body,
            AnyEffect::SevenBand($effect) => $body,
            AnyEffect::MorphEq($effect) => $body,
            AnyEffect::FilterBank($effect) => $body,
            AnyEffect::XyPad($effect) => $body,
            AnyEffect::XyzPad($effect) => $body,
            AnyEffect::PanLfo($effect) => $body,
            AnyEffect::EnvelopeFollower($effect) => $body,
            AnyEffect::NoteEnvelope($effect) => $body,
            AnyEffect::LushSpace($effect) => $body,
            AnyEffect::Tuner($effect) => $body,
            AnyEffect::StageStack($effect) => $body,
            AnyEffect::ControlSurface($effect) => $body,
            AnyEffect::SendTap($effect) => $body,
            AnyEffect::MasterLimiter($effect) => $body,
        }
    };
}

impl AnyEffect {
    /// Builds the effect `params` belongs to, with those settings. It
    /// still needs [`AnyEffect::prepare`].
    pub fn new(params: &EffectParams) -> Self {
        let mut effect = match params.kind() {
            EffectKind::Eq => AnyEffect::Eq(Box::default()),
            EffectKind::Compressor => AnyEffect::Compressor(Box::default()),
            EffectKind::Limiter => AnyEffect::Limiter(Box::default()),
            EffectKind::Reverb => AnyEffect::Reverb(Box::default()),
            EffectKind::Delay => AnyEffect::Delay(Box::default()),
            EffectKind::Balance => AnyEffect::Balance(Box::default()),
            EffectKind::DcBlock => AnyEffect::DcBlock(Box::default()),
            EffectKind::ChannelMute => AnyEffect::ChannelMute(Box::default()),
            EffectKind::Polarity => AnyEffect::Polarity(Box::default()),
            EffectKind::StereoMatrix => AnyEffect::StereoMatrix(Box::default()),
            EffectKind::SoftClipper => AnyEffect::SoftClipper(Box::default()),
            EffectKind::Distortion => AnyEffect::Distortion(Box::default()),
            EffectKind::FastLowpass => AnyEffect::FastLowpass(Box::default()),
            EffectKind::SelectableFilter => AnyEffect::SelectableFilter(Box::default()),
            EffectKind::BassShelf => AnyEffect::BassShelf(Box::default()),
            EffectKind::Lofi => AnyEffect::Lofi(Box::default()),
            EffectKind::Chorus => AnyEffect::Chorus(Box::default()),
            EffectKind::Flanger => AnyEffect::Flanger(Box::default()),
            EffectKind::Phaser => AnyEffect::Phaser(Box::default()),
            EffectKind::BandSplit => AnyEffect::BandSplit(Box::default()),
            EffectKind::MultibandCompressor => AnyEffect::MultibandCompressor(Box::default()),
            EffectKind::MultibandMaximizer => AnyEffect::MultibandMaximizer(Box::default()),
            EffectKind::TransientShaper => AnyEffect::TransientShaper(Box::default()),
            EffectKind::TransientSplit => AnyEffect::TransientSplit(Box::default()),
            EffectKind::OneKnob => AnyEffect::OneKnob(Box::default()),
            EffectKind::BassHarmonics => AnyEffect::BassHarmonics(Box::default()),
            EffectKind::Exciter => AnyEffect::Exciter(Box::default()),
            EffectKind::Waveshaper => AnyEffect::Waveshaper(Box::default()),
            EffectKind::Overdrive => AnyEffect::Overdrive(Box::default()),
            EffectKind::GuitarRack => AnyEffect::GuitarRack(Box::default()),
            EffectKind::DriveChain => AnyEffect::DriveChain(Box::default()),
            EffectKind::VintageChorus => AnyEffect::VintageChorus(Box::default()),
            EffectKind::HyperChorus => AnyEffect::HyperChorus(Box::default()),
            EffectKind::VintagePhaser => AnyEffect::VintagePhaser(Box::default()),
            EffectKind::StackedFlanger => AnyEffect::StackedFlanger(Box::default()),
            EffectKind::BandDelay => AnyEffect::BandDelay(Box::default()),
            EffectKind::Room => AnyEffect::Room(Box::default()),
            EffectKind::Spreader => AnyEffect::Spreader(Box::default()),
            EffectKind::StereoEnhancer => AnyEffect::StereoEnhancer(Box::default()),
            EffectKind::VolumeGate => AnyEffect::VolumeGate(Box::default()),
            EffectKind::TimeTransport => AnyEffect::TimeTransport(Box::default()),
            EffectKind::Scratch => AnyEffect::Scratch(Box::default()),
            EffectKind::PerformanceRack => AnyEffect::PerformanceRack(Box::default()),
            EffectKind::Convolver => AnyEffect::Convolver(Box::default()),
            EffectKind::FrequencyShifter => AnyEffect::FrequencyShifter(Box::default()),
            EffectKind::PitchShift => AnyEffect::PitchShift(Box::default()),
            EffectKind::PitchCorrect => AnyEffect::PitchCorrect(Box::default()),
            EffectKind::Vocoder => AnyEffect::Vocoder(Box::default()),
            EffectKind::EchoBank => AnyEffect::EchoBank(Box::default()),
            EffectKind::FrequencyDelay => AnyEffect::FrequencyDelay(Box::default()),
            EffectKind::SevenBand => AnyEffect::SevenBand(Box::default()),
            EffectKind::MorphEq => AnyEffect::MorphEq(Box::default()),
            EffectKind::FilterBank => AnyEffect::FilterBank(Box::default()),
            EffectKind::XyPad => AnyEffect::XyPad(Box::default()),
            EffectKind::XyzPad => AnyEffect::XyzPad(Box::default()),
            EffectKind::PanLfo => AnyEffect::PanLfo(Box::default()),
            EffectKind::EnvelopeFollower => AnyEffect::EnvelopeFollower(Box::default()),
            EffectKind::NoteEnvelope => AnyEffect::NoteEnvelope(Box::default()),
            EffectKind::LushSpace => AnyEffect::LushSpace(Box::default()),
            EffectKind::Tuner => AnyEffect::Tuner(Box::default()),
            EffectKind::StageStack => AnyEffect::StageStack(Box::default()),
            EffectKind::ControlSurface => AnyEffect::ControlSurface(Box::default()),
            EffectKind::SendTap => AnyEffect::SendTap(Box::default()),
            EffectKind::MasterLimiter => AnyEffect::MasterLimiter(Box::default()),
        };
        effect.set_params(params);
        effect
    }

    pub fn kind(&self) -> EffectKind {
        match self {
            AnyEffect::Eq(_) => EffectKind::Eq,
            AnyEffect::Compressor(_) => EffectKind::Compressor,
            AnyEffect::Limiter(_) => EffectKind::Limiter,
            AnyEffect::Reverb(_) => EffectKind::Reverb,
            AnyEffect::Delay(_) => EffectKind::Delay,
            AnyEffect::Balance(_) => EffectKind::Balance,
            AnyEffect::DcBlock(_) => EffectKind::DcBlock,
            AnyEffect::ChannelMute(_) => EffectKind::ChannelMute,
            AnyEffect::Polarity(_) => EffectKind::Polarity,
            AnyEffect::StereoMatrix(_) => EffectKind::StereoMatrix,
            AnyEffect::SoftClipper(_) => EffectKind::SoftClipper,
            AnyEffect::Distortion(_) => EffectKind::Distortion,
            AnyEffect::FastLowpass(_) => EffectKind::FastLowpass,
            AnyEffect::SelectableFilter(_) => EffectKind::SelectableFilter,
            AnyEffect::BassShelf(_) => EffectKind::BassShelf,
            AnyEffect::Lofi(_) => EffectKind::Lofi,
            AnyEffect::Chorus(_) => EffectKind::Chorus,
            AnyEffect::Flanger(_) => EffectKind::Flanger,
            AnyEffect::Phaser(_) => EffectKind::Phaser,
            AnyEffect::BandSplit(_) => EffectKind::BandSplit,
            AnyEffect::MultibandCompressor(_) => EffectKind::MultibandCompressor,
            AnyEffect::MultibandMaximizer(_) => EffectKind::MultibandMaximizer,
            AnyEffect::TransientShaper(_) => EffectKind::TransientShaper,
            AnyEffect::TransientSplit(_) => EffectKind::TransientSplit,
            AnyEffect::OneKnob(_) => EffectKind::OneKnob,
            AnyEffect::BassHarmonics(_) => EffectKind::BassHarmonics,
            AnyEffect::Exciter(_) => EffectKind::Exciter,
            AnyEffect::Waveshaper(_) => EffectKind::Waveshaper,
            AnyEffect::Overdrive(_) => EffectKind::Overdrive,
            AnyEffect::GuitarRack(_) => EffectKind::GuitarRack,
            AnyEffect::DriveChain(_) => EffectKind::DriveChain,
            AnyEffect::VintageChorus(_) => EffectKind::VintageChorus,
            AnyEffect::HyperChorus(_) => EffectKind::HyperChorus,
            AnyEffect::VintagePhaser(_) => EffectKind::VintagePhaser,
            AnyEffect::StackedFlanger(_) => EffectKind::StackedFlanger,
            AnyEffect::BandDelay(_) => EffectKind::BandDelay,
            AnyEffect::Room(_) => EffectKind::Room,
            AnyEffect::Spreader(_) => EffectKind::Spreader,
            AnyEffect::StereoEnhancer(_) => EffectKind::StereoEnhancer,
            AnyEffect::VolumeGate(_) => EffectKind::VolumeGate,
            AnyEffect::TimeTransport(_) => EffectKind::TimeTransport,
            AnyEffect::Scratch(_) => EffectKind::Scratch,
            AnyEffect::PerformanceRack(_) => EffectKind::PerformanceRack,
            AnyEffect::Convolver(_) => EffectKind::Convolver,
            AnyEffect::FrequencyShifter(_) => EffectKind::FrequencyShifter,
            AnyEffect::PitchShift(_) => EffectKind::PitchShift,
            AnyEffect::PitchCorrect(_) => EffectKind::PitchCorrect,
            AnyEffect::Vocoder(_) => EffectKind::Vocoder,
            AnyEffect::EchoBank(_) => EffectKind::EchoBank,
            AnyEffect::FrequencyDelay(_) => EffectKind::FrequencyDelay,
            AnyEffect::SevenBand(_) => EffectKind::SevenBand,
            AnyEffect::MorphEq(_) => EffectKind::MorphEq,
            AnyEffect::FilterBank(_) => EffectKind::FilterBank,
            AnyEffect::XyPad(_) => EffectKind::XyPad,
            AnyEffect::XyzPad(_) => EffectKind::XyzPad,
            AnyEffect::PanLfo(_) => EffectKind::PanLfo,
            AnyEffect::EnvelopeFollower(_) => EffectKind::EnvelopeFollower,
            AnyEffect::NoteEnvelope(_) => EffectKind::NoteEnvelope,
            AnyEffect::LushSpace(_) => EffectKind::LushSpace,
            AnyEffect::Tuner(_) => EffectKind::Tuner,
            AnyEffect::StageStack(_) => EffectKind::StageStack,
            AnyEffect::ControlSurface(_) => EffectKind::ControlSurface,
            AnyEffect::SendTap(_) => EffectKind::SendTap,
            AnyEffect::MasterLimiter(_) => EffectKind::MasterLimiter,
        }
    }

    /// See [`Effect::prepare`].
    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        each_effect!(self, effect => effect.prepare(sample_rate, max_block));
    }

    /// See [`Effect::reset`].
    pub fn reset(&mut self) {
        each_effect!(self, effect => effect.reset());
    }

    /// See [`Effect::set_params`]. Returns false, and changes nothing, if
    /// `params` are for a different effect: the host then has to build a
    /// new one.
    pub fn set_params(&mut self, params: &EffectParams) -> bool {
        match (self, params) {
            (AnyEffect::Eq(effect), EffectParams::Eq(params)) => effect.set_params(params),
            (AnyEffect::Compressor(effect), EffectParams::Compressor(params)) => {
                effect.set_params(params);
            }
            (AnyEffect::Limiter(effect), EffectParams::Limiter(params)) => {
                effect.set_params(params);
            }
            (AnyEffect::Reverb(effect), EffectParams::Reverb(params)) => effect.set_params(params),
            (AnyEffect::Delay(effect), EffectParams::Delay(params)) => effect.set_params(params),
            (AnyEffect::Balance(effect), EffectParams::Balance(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::DcBlock(effect), EffectParams::DcBlock(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::ChannelMute(effect), EffectParams::ChannelMute(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Polarity(effect), EffectParams::Polarity(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::StereoMatrix(effect), EffectParams::StereoMatrix(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::SoftClipper(effect), EffectParams::SoftClipper(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Distortion(effect), EffectParams::Distortion(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::FastLowpass(effect), EffectParams::FastLowpass(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::SelectableFilter(effect), EffectParams::SelectableFilter(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::BassShelf(effect), EffectParams::BassShelf(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Lofi(effect), EffectParams::Lofi(params)) => effect.set_params(params),
            (AnyEffect::Chorus(effect), EffectParams::Chorus(params)) => effect.set_params(params),
            (AnyEffect::Flanger(effect), EffectParams::Flanger(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Phaser(effect), EffectParams::Phaser(params)) => effect.set_params(params),
            (AnyEffect::BandSplit(effect), EffectParams::BandSplit(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::MultibandCompressor(effect), EffectParams::MultibandCompressor(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::MultibandMaximizer(effect), EffectParams::MultibandMaximizer(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::TransientShaper(effect), EffectParams::TransientShaper(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::TransientSplit(effect), EffectParams::TransientSplit(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::OneKnob(effect), EffectParams::OneKnob(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::BassHarmonics(effect), EffectParams::BassHarmonics(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Exciter(effect), EffectParams::Exciter(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Waveshaper(effect), EffectParams::Waveshaper(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Overdrive(effect), EffectParams::Overdrive(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::GuitarRack(effect), EffectParams::GuitarRack(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::DriveChain(effect), EffectParams::DriveChain(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::VintageChorus(effect), EffectParams::VintageChorus(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::HyperChorus(effect), EffectParams::HyperChorus(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::VintagePhaser(effect), EffectParams::VintagePhaser(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::StackedFlanger(effect), EffectParams::StackedFlanger(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::BandDelay(effect), EffectParams::BandDelay(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Room(effect), EffectParams::Room(params)) => effect.set_params(params),
            (AnyEffect::Spreader(effect), EffectParams::Spreader(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::StereoEnhancer(effect), EffectParams::StereoEnhancer(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::VolumeGate(effect), EffectParams::VolumeGate(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::TimeTransport(effect), EffectParams::TimeTransport(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Scratch(effect), EffectParams::Scratch(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::PerformanceRack(effect), EffectParams::PerformanceRack(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Convolver(effect), EffectParams::Convolver(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::FrequencyShifter(effect), EffectParams::FrequencyShifter(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::PitchShift(effect), EffectParams::PitchShift(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::PitchCorrect(effect), EffectParams::PitchCorrect(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Vocoder(effect), EffectParams::Vocoder(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::EchoBank(effect), EffectParams::EchoBank(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::FrequencyDelay(effect), EffectParams::FrequencyDelay(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::SevenBand(effect), EffectParams::SevenBand(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::MorphEq(effect), EffectParams::MorphEq(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::FilterBank(effect), EffectParams::FilterBank(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::XyPad(effect), EffectParams::XyPad(params)) => effect.set_params(params),
            (AnyEffect::XyzPad(effect), EffectParams::XyzPad(params)) => effect.set_params(params),
            (AnyEffect::PanLfo(effect), EffectParams::PanLfo(params)) => effect.set_params(params),
            (AnyEffect::EnvelopeFollower(effect), EffectParams::EnvelopeFollower(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::NoteEnvelope(effect), EffectParams::NoteEnvelope(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::LushSpace(effect), EffectParams::LushSpace(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::Tuner(effect), EffectParams::Tuner(params)) => effect.set_params(params),
            (AnyEffect::StageStack(effect), EffectParams::StageStack(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::ControlSurface(effect), EffectParams::ControlSurface(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::SendTap(effect), EffectParams::SendTap(params)) => {
                effect.set_params(params)
            }
            (AnyEffect::MasterLimiter(effect), EffectParams::MasterLimiter(params)) => {
                effect.set_params(params)
            }
            _ => return false,
        }
        true
    }

    /// See [`Effect::set_tempo`].
    pub fn set_tempo(&mut self, bpm: f32) {
        each_effect!(self, effect => effect.set_tempo(bpm));
    }

    pub fn set_transport(&mut self, playing: bool, position_beats: f64) {
        each_effect!(self, effect => effect.set_transport(playing, position_beats));
    }

    /// See [`Effect::process`].
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        each_effect!(self, effect => effect.process(left, right));
    }
    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        each_effect!(self, effect => effect.process_sidechain(left, right, key));
    }

    /// See [`Effect::latency_samples`].
    pub fn latency_samples(&self) -> usize {
        each_effect!(self, effect => effect.latency_samples())
    }

    /// See [`Effect::warm_up_samples`].
    pub fn warm_up_samples(&self) -> usize {
        each_effect!(self, effect => effect.warm_up_samples())
    }

    /// See [`Effect::delay_readiness_samples`].
    pub fn delay_readiness_samples(&self) -> usize {
        each_effect!(self, effect => effect.delay_readiness_samples())
    }
    pub fn latency_transition_samples_remaining(&self) -> usize {
        each_effect!(self, effect => effect.latency_transition_samples_remaining())
    }

    /// The largest latency any setting of this effect can have at
    /// `sample_rate`. A host that reserves this much never has to grow a
    /// compensation buffer while playing.
    pub fn max_latency_samples(&self, sample_rate: f32) -> usize {
        self.kind().max_latency_samples(sample_rate)
    }

    /// See [`Effect::tail_samples`].
    pub fn tail_samples(&self) -> usize {
        each_effect!(self, effect => effect.tail_samples())
    }

    /// See [`Effect::gap_samples`].
    pub fn gap_samples(&self) -> usize {
        each_effect!(self, effect => effect.gap_samples())
    }

    /// The gain reduction meter of an effect that turns its signal down:
    /// the compressor, the limiter and the multiband and mastering
    /// dynamics. Take it before the effect goes to the audio thread.
    pub fn gain_reduction(&self) -> Option<GainReductionMeter> {
        match self {
            AnyEffect::Compressor(effect) => Some(effect.meter()),
            AnyEffect::Limiter(effect) => Some(effect.meter()),
            AnyEffect::MultibandCompressor(effect) => Some(effect.meter()),
            AnyEffect::MultibandMaximizer(effect) => Some(effect.meter()),
            AnyEffect::OneKnob(effect) => Some(effect.meter()),
            AnyEffect::StageStack(effect) => Some(effect.meter()),
            AnyEffect::MasterLimiter(effect) => Some(effect.meter()),
            _ => None,
        }
    }

    /// What the effect measured over the block it processed last, for the
    /// kinds that measure anything. Reading takes nothing away from
    /// [`gain_reduction`](Self::gain_reduction)'s meter.
    pub fn observation(&self) -> Option<crate::observe::EffectObservation> {
        match self {
            AnyEffect::Limiter(effect) => effect
                .observation()
                .map(crate::observe::EffectObservation::Dynamics),
            AnyEffect::Tuner(effect) => {
                let (serial, readout) = effect.observation();
                Some(crate::observe::EffectObservation::Tuner { serial, readout })
            }
            _ => None,
        }
    }
}

/// Most separately routable outputs one effect has beside its main output.
pub const MAX_AUX_OUTPUTS: usize = 3;

/// One output of an effect that a host can route somewhere other than the
/// next place in the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxOutputInfo {
    /// Stable name for saved routing and tests.
    pub id: &'static str,
    /// What the UI calls it.
    pub label: &'static str,
    /// Routing this output takes its signal out of the effect's main
    /// output. An output that does not split is a copy, and the main
    /// output is the same whether or not it is routed.
    pub splits: bool,
}

/// The separately routable stereo frames of one block, for
/// [`EffectSlot::process_routed`].
pub struct AuxBlock<'a> {
    /// Bit `n` set: output `n` has a destination.
    pub routed: u8,
    /// One buffer per possible output, each at least as long as the block.
    /// Every buffer is overwritten: an output that is not routed, or that
    /// the effect does not have, is left silent.
    pub outputs: [&'a mut [[f32; 2]]; MAX_AUX_OUTPUTS],
}

impl EffectKind {
    /// The outputs of this effect a host can route to another track, in the
    /// order saved routing numbers them. Most effects have none.
    pub fn aux_outputs(self) -> &'static [AuxOutputInfo] {
        match self {
            EffectKind::SendTap => &[AuxOutputInfo {
                id: "send",
                label: "Send",
                splits: false,
            }],
            EffectKind::TransientShaper | EffectKind::TransientSplit => &[
                AuxOutputInfo {
                    id: "transient",
                    label: "Transient",
                    splits: true,
                },
                AuxOutputInfo {
                    id: "sustain",
                    label: "Sustain",
                    splits: true,
                },
            ],
            EffectKind::BandSplit => &[
                AuxOutputInfo { id: "low", label: "Low", splits: true },
                AuxOutputInfo { id: "mid", label: "Mid", splits: true },
                AuxOutputInfo { id: "high", label: "High", splits: true },
            ],
            _ => &[],
        }
    }
}

impl AnyEffect {
    /// Processes one block in place as [`AnyEffect::process_sidechain`]
    /// does, and fills the separately routable outputs. Does not allocate.
    pub fn process_aux(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        aux: &mut AuxBlock<'_>,
    ) {
        let frames = left.len().min(right.len());
        let [first, second, third] = &mut aux.outputs;
        let (first, second, third) = (&mut first[..frames], &mut second[..frames], &mut third[..frames]);
        match self {
            AnyEffect::SendTap(effect) => {
                effect.process(left, right);
                if aux.routed & 1 != 0 {
                    effect.send_frames(left, right, first);
                } else {
                    first.fill([0.0; 2]);
                }
                second.fill([0.0; 2]);
                third.fill([0.0; 2]);
            }
            AnyEffect::TransientShaper(effect) => {
                let routed = [aux.routed & 1 != 0, aux.routed & 2 != 0];
                effect.process_routed(left, right, routed, first, second);
                third.fill([0.0; 2]);
            }
            AnyEffect::TransientSplit(effect) => {
                let routed = [aux.routed & 1 != 0, aux.routed & 2 != 0];
                effect.process_routed(left, right, routed, first, second);
                third.fill([0.0; 2]);
            }
            AnyEffect::BandSplit(effect) => {
                effect.process_routed(left, right,
                    [aux.routed & 1 != 0, aux.routed & 2 != 0, aux.routed & 4 != 0],
                    [first, second, third]);
            }
            _ => {
                self.process_sidechain(left, right, key);
                first.fill([0.0; 2]);
                second.fill([0.0; 2]);
                third.fill([0.0; 2]);
            }
        }
    }
}

/// Time the slot takes to fade an effect in or out, and to follow its mix.
const SLOT_FADE_MS: f32 = 10.0;

/// The loudest sample a slot lets through to its effect: 60 dB over full
/// scale. Nothing a mixer produces on purpose comes near it.
const INPUT_LIMIT: f32 = 1.0e3;

/// One place in an effect chain: an effect plus the two controls every
/// slot has, an on/off switch and a mix.
///
/// Switching the slot off crossfades to the untouched input over a few
/// milliseconds instead of cutting, and then stops running the effect,
/// which costs nothing while it is off. Switched back on, the effect
/// starts with its memory cleared and runs unheard until every delayed
/// output is primed, so the fade back in finds its output already there.
/// The mix blends the untouched input
/// with the effect's output. Both use an input delayed by the effect's
/// latency, so the slot's delay is the same whether it is on, off or
/// half-mixed and compensation never has to change. When the latency
/// itself changes, as when a limiter's look-ahead is moved, that delay
/// crossfades to its new length the way the limiter does.
///
/// The slot also keeps bad input away from the effect: samples that are
/// not numbers become silence and absurdly loud ones are held to 60 dB
/// over full scale, so a misbehaving plugin upstream cannot leave an
/// effect's memory poisoned.
///
/// Like the effect inside it, a slot is built and prepared off the audio
/// thread and dropped off it.
pub struct EffectSlot {
    effect: AnyEffect,
    sample_rate: f32,
    enabled: bool,
    mix: f32,
    /// Share of the effect's output in what the slot puts out.
    wet: LinearRamp,
    dry_left: DelayLine,
    dry_right: DelayLine,
    /// Shared-delay transition, preserving every currently audible dry tap.
    dry_fade: TapCrossfade,
    dry_history: usize,
    /// Whether each prepared scratch frame uses the final dry tap.
    dry_aligned: Box<[bool]>,
    scratch_left: Box<[f32]>,
    scratch_right: Box<[f32]>,
    /// The effect has faded out and has not run since, so its memory is
    /// stale and must be cleared before it is heard again.
    dormant: bool,
    /// Samples the effect still has to run unheard after it was woken. An
    /// effect with delayed outputs puts out nothing for that long once its memory
    /// has been cleared, and fading it in at once would let its output
    /// burst in part way through the fade.
    warm_up: usize,
    /// Samples already run since the dormant effect was cleared. A delay
    /// increased during priming extends the wait without losing this history.
    warm_up_elapsed: usize,
    warming: bool,
    fresh: bool,
}

impl EffectSlot {
    /// A slot that is switched on, at full mix. It still needs
    /// [`EffectSlot::prepare`].
    pub fn new(effect: AnyEffect) -> Self {
        Self {
            effect,
            sample_rate: 48_000.0,
            enabled: true,
            mix: 1.0,
            wet: LinearRamp::new(1.0),
            dry_left: DelayLine::default(),
            dry_right: DelayLine::default(),
            dry_fade: TapCrossfade::new(0, 0),
            dry_history: 0,
            dry_aligned: Box::default(),
            scratch_left: Box::default(),
            scratch_right: Box::default(),
            dormant: false,
            warm_up: 0,
            warm_up_elapsed: 0,
            warming: false,
            fresh: true,
        }
    }

    /// Prepares the effect and the slot's own buffers. Allocates.
    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let max_block = max_block.max(1);
        self.sample_rate = sample_rate.max(1.0);
        self.effect.prepare(self.sample_rate, max_block);
        let latency = self.effect.max_latency_samples(self.sample_rate);
        self.dry_left = DelayLine::new(latency);
        self.dry_right = DelayLine::new(latency);
        self.dry_fade = TapCrossfade::new(latency, self.effect.latency_samples());
        self.scratch_left = vec![0.0; max_block].into_boxed_slice();
        self.scratch_right = vec![0.0; max_block].into_boxed_slice();
        self.dry_aligned = vec![false; max_block].into_boxed_slice();
        self.reset();
    }

    /// Clears the effect's and the slot's memory of past audio.
    pub fn reset(&mut self) {
        self.effect.reset();
        self.dry_left.clear();
        self.dry_right.clear();
        self.dry_fade.snap(self.effect.latency_samples());
        self.dry_history = 0;
        self.dormant = false;
        self.warm_up = 0;
        self.warm_up_elapsed = 0;
        self.warming = false;
        self.fresh = true;
        self.wet.snap(self.wet_target());
    }

    fn wet_target(&self) -> f32 {
        if self.enabled { self.mix } else { 0.0 }
    }

    fn retarget(&mut self) {
        let samples = if self.fresh {
            0
        } else {
            ms_to_samples(SLOT_FADE_MS, self.sample_rate)
        };
        self.wet.set_target(self.wet_target(), samples);
    }

    /// Switches the effect on or off without a click.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.retarget();
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Sets the balance between the untouched input (0) and the effect's
    /// output (1).
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = if mix.is_finite() {
            mix.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.retarget();
    }

    /// See [`AnyEffect::set_params`].
    pub fn set_params(&mut self, params: &EffectParams) -> bool {
        let accepted = self.effect.set_params(params);
        if accepted && self.warm_up > 0 {
            self.warm_up = self.warm_up.max(
                self.effect
                    .warm_up_samples()
                    .saturating_sub(self.warm_up_elapsed),
            );
        }
        accepted
    }

    /// See [`Effect::set_tempo`].
    pub fn set_tempo(&mut self, bpm: f32) {
        self.effect.set_tempo(bpm);
    }

    pub fn set_transport(&mut self, playing: bool, position_beats: f64) {
        self.effect.set_transport(playing, position_beats);
    }

    pub fn effect(&self) -> &AnyEffect {
        &self.effect
    }

    /// Takes the slot apart, for moving its effect elsewhere.
    pub fn into_effect(self) -> AnyEffect {
        self.effect
    }

    /// Samples by which the slot delays its signal. It is the effect's
    /// latency whether the slot is on or off.
    pub fn latency_samples(&self) -> usize {
        self.effect.latency_samples()
    }

    /// See [`Effect::tail_samples`]. Include wet memory until the bypass or
    /// mix fade settles at zero, as well as current and old aligned dry taps.
    pub fn tail_samples(&self) -> usize {
        self.dry_fade
            .longest_delay()
            .max(self.effect.latency_samples())
            .max(if self.wet.value() > 0.0 || !self.wet.is_settled() {
                self.effect.tail_samples()
            } else {
                0
            })
    }

    /// See [`Effect::gap_samples`]. A bypass/mix fade can still reveal wet
    /// memory. Only a settled zero-wet slot uses dry-tap accounting alone.
    pub fn gap_samples(&self) -> usize {
        self.dry_fade
            .longest_delay()
            .max(self.effect.latency_samples())
            .max(if self.wet.value() > 0.0 || !self.wet.is_settled() {
                self.effect.gap_samples()
            } else {
                0
            })
    }

    /// Processes one block in place. Blocks longer than the `max_block`
    /// given to [`EffectSlot::prepare`] are worked through in pieces.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }

    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        let frames = left.len().min(right.len());
        let piece = self.scratch_left.len().max(1);
        let mut start = 0;
        while start < frames {
            let end = (start + piece).min(frames);
            self.process_piece(
                &mut left[start..end],
                &mut right[start..end],
                key.and_then(|key| key.get(start..end)),
                None,
            );
            start = end;
        }
    }

    /// Processes one block in place as [`EffectSlot::process_sidechain`]
    /// does, and fills the effect's separately routable outputs.
    ///
    /// They follow the slot's switch and mix the way its main output does:
    /// a slot that is off sends nothing, and a mix below 1 scales what is
    /// sent, so an output that splits and the main output still add up to
    /// the signal that came in. Does not allocate.
    pub fn process_routed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        aux: &mut AuxBlock<'_>,
    ) {
        let frames = left.len().min(right.len());
        let piece = self.scratch_left.len().max(1);
        let mut start = 0;
        while start < frames {
            let end = (start + piece).min(frames);
            let [first, second, third] = &mut aux.outputs;
            let mut part = AuxBlock {
                routed: aux.routed,
                outputs: [&mut first[start..end], &mut second[start..end], &mut third[start..end]],
            };
            self.process_piece(
                &mut left[start..end],
                &mut right[start..end],
                key.and_then(|key| key.get(start..end)),
                Some(&mut part),
            );
            start = end;
        }
    }

    fn process_piece(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        mut aux: Option<&mut AuxBlock<'_>>,
    ) {
        let fresh = std::mem::replace(&mut self.fresh, false);
        let frames = left.len();
        if self.scratch_left.len() < frames {
            // Not prepared: there is nowhere to keep the dry signal.
            if let Some(aux) = &mut aux {
                for output in &mut aux.outputs {
                    output[..frames].fill([0.0; 2]);
                }
            }
            return;
        }
        for sample in left.iter_mut().chain(right.iter_mut()) {
            *sample = if sample.is_finite() {
                sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
            } else {
                0.0
            };
        }

        let latency = self.effect.latency_samples();
        let fade_samples = if fresh {
            0
        } else {
            ms_to_samples(LOOKAHEAD_FADE_MS, self.sample_rate)
        };
        let matrix = self.effect.kind() == EffectKind::StereoMatrix;
        let readiness = self.effect.delay_readiness_samples();
        if !matrix {
            self.dry_fade.retarget_joining(latency, fade_samples);
        }
        let dry_transition = self.dry_fade.remaining() as usize;
        let dry_left = &mut self.scratch_left[..frames];
        let dry_right = &mut self.scratch_right[..frames];
        for index in 0..frames {
            let ready = fresh || !matrix || self.dry_history >= readiness;
            if matrix && ready {
                self.dry_fade.retarget(latency, fade_samples);
            }
            self.dry_aligned[index] = ready && self.dry_fade.remaining() == 0;
            for (line, input, dry) in [
                (&mut self.dry_left, left[index], &mut dry_left[index]),
                (&mut self.dry_right, right[index], &mut dry_right[index]),
            ] {
                *dry = self
                    .dry_fade
                    .read(|delay| if delay == 0 { input } else { line.tap(delay) });
                // Prime even at zero shared latency for later delayed edits.
                line.push(input);
            }
            self.dry_fade.advance();
            self.dry_history = (self.dry_history + 1).min(self.dry_left.max_delay());
        }

        if self.wet.is_settled() && self.wet.value() == 0.0 {
            left.copy_from_slice(dry_left);
            right.copy_from_slice(dry_right);
            self.dormant = true;
            if let Some(aux) = &mut aux {
                for output in &mut aux.outputs {
                    output[..frames].fill([0.0; 2]);
                }
            }
            return;
        }
        if self.dormant {
            self.effect.reset();
            self.dormant = false;
            self.warm_up = self.effect.warm_up_samples().max(dry_transition);
            self.warm_up_elapsed = 0;
            self.warming = true;
        }
        if self.warm_up > 0 {
            // Reset snaps the wet taps, so keep them unheard until any
            // simultaneous dry/PDC transition has also reached its target.
            self.warm_up = self.warm_up.max(dry_transition);
        }
        match &mut aux {
            Some(aux) => self.effect.process_aux(left, right, key, aux),
            None => self.effect.process_sidechain(left, right, key),
        }
        if self.warm_up == 0 && self.wet.is_settled() && self.wet.value() == 1.0 {
            return;
        }
        for index in 0..frames {
            // The fade waits until the effect's output has arrived.
            let wet = if self.warming && (self.warm_up > 0 || !self.dry_aligned[index]) {
                self.warm_up = self.warm_up.saturating_sub(1);
                0.0
            } else {
                self.warming = false;
                self.wet.tick()
            };
            if wet == 0.0 {
                left[index] = dry_left[index];
                right[index] = dry_right[index];
            } else if wet != 1.0 {
                left[index] = dry_left[index] + (left[index] - dry_left[index]) * wet;
                right[index] = dry_right[index] + (right[index] - dry_right[index]) * wet;
            }
            // What is routed away is heard as much as the effect itself is.
            if wet != 1.0
                && let Some(aux) = &mut aux
            {
                for output in &mut aux.outputs {
                    output[index] = [output[index][0] * wet, output[index][1] * wet];
                }
            }
        }
        self.warm_up_elapsed = self.warm_up_elapsed.saturating_add(frames);
        // A fade can finish inside this piece. Waking must clear stale state
        // even if no subsequent fully dry block arrived before re-enabling.
        self.dormant = self.wet.is_settled() && self.wet.value() == 0.0;
    }
}

mod admission;

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn every_effect_kind_round_trips_and_dispatches() {
        assert_eq!(EffectKind::ALL[0], EffectKind::Eq);
        assert_eq!(EffectKind::ALL[18], EffectKind::Phaser);
        for kind in EffectKind::ALL {
            let params = kind.default_params();
            let tag = serde_json::to_value(kind).unwrap();
            let json = serde_json::to_value(params).unwrap();
            assert_eq!(json["type"], tag);
            let loaded: EffectParams = serde_json::from_value(json).unwrap();
            assert_eq!(loaded, params);
            let bare: EffectParams =
                serde_json::from_value(serde_json::json!({ "type": tag })).unwrap();
            assert_eq!(bare, params);
            assert_eq!(params.sanitized(), params);
            for (index, info) in kind.descriptors().iter().enumerate() {
                assert_eq!(params.get(index), Some(info.default));
                let mut changed = params;
                assert!(changed.set(index, info.default));
                assert_eq!(changed, params);
            }
            let mut effect = AnyEffect::new(&params);
            assert_eq!(effect.kind(), kind);
            effect.prepare(48_000.0, 64);
            assert!(effect.set_params(&params));
            let wrong =
                EffectKind::ALL[(kind as usize + 1) % EffectKind::ALL.len()].default_params();
            assert!(!effect.set_params(&wrong));
            assert_eq!(effect.latency_samples(), params.latency_samples(48_000.0));
            assert!(effect.latency_samples() <= kind.max_latency_samples(48_000.0));
            effect.set_tempo(120.0);
            effect.process_sidechain(&mut [0.0; 64], &mut [0.0; 64], None);
            let _ = (
                effect.warm_up_samples(),
                effect.delay_readiness_samples(),
                effect.latency_transition_samples_remaining(),
                effect.tail_samples(),
                effect.gap_samples(),
                effect.gain_reduction(),
            );
            effect.reset();
        }
    }

    #[test]
    fn new_effect_latency_queries_match_prepared_processors() {
        for kind in &EffectKind::ALL[19..] {
            for rate in [
                1.0,
                8_000.0,
                44_100.0,
                48_000.0,
                96_000.0,
                384_000.0,
                f32::NAN,
            ] {
                let params = kind.default_params();
                let mut effect = AnyEffect::new(&params);
                effect.prepare(rate, 1);
                assert_eq!(
                    effect.latency_samples(),
                    params.latency_samples(rate),
                    "{kind:?} at {rate}"
                );
                if *kind == EffectKind::PerformanceRack {
                    assert!(effect.latency_samples() <= kind.max_latency_samples(rate));
                } else {
                    assert_eq!(
                        effect.latency_samples(),
                        kind.max_latency_samples(rate),
                        "{kind:?} at {rate}"
                    );
                }
            }
        }
    }
}
