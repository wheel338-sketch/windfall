//! A polyphonic subtractive synthesizer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::pan_gains;

use crate::blocks::CONTROL_PERIOD;
use crate::blocks::adsr::Adsr;
use crate::blocks::halfband::HalfbandDecimator;
use crate::blocks::lfo::{Lfo, LfoShape};
use crate::blocks::math::{key_to_hz, ms_to_samples, smoothing_coefficient};
use crate::blocks::noise::{PinkNoise, Rng};
use crate::blocks::oscillator::{MAX_INCREMENT, Waveform, pulse, saw, sine, triangle};
use crate::blocks::shaper::soft_clip;
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::{Svf, SvfCoeffs, cutoff_gain};
use crate::instrument::Instrument;
use crate::{NoteExpression, NoteInstanceId};
use crate::param::{ParamSet, param_set};

/// Most notes that can sound at full level at once.
pub const MAX_POLYPHONY: usize = 32;

/// Most stacked copies of each oscillator.
pub const MAX_UNISON: usize = 7;

const OSCILLATORS: usize = 3;
const LFOS: usize = 2;

/// Extra voices for notes that are fading out after being stolen, so that
/// taking a voice never has to cut one off.
const SPARE_VOICES: usize = 8;
const VOICES: usize = MAX_POLYPHONY + SPARE_VOICES;

/// Samples at the doubled internal rate in one control period.
const OVERSAMPLED: usize = 2 * CONTROL_PERIOD;

/// Length of the fade that ends a stolen or stopped voice.
const CHOKE_MS: f32 = 4.0;

/// Shortest attack and release of the amplitude envelope. Anything faster
/// is a click.
const MIN_AMP_SEGMENT_MS: f32 = 0.5;

/// Time the continuous controls take to cover 63% of a change.
const CONTROL_SMOOTHING_MS: f32 = 10.0;

/// Keys remembered in mono mode, for falling back to a key that is still
/// held when a later one is let go.
const HELD_KEYS: usize = 16;

/// Q of the filter with resonance at 1.
const MAX_RESONANCE_Q: f32 = 20.0;

/// Octaves the cutoff drops for the softest note with filter velocity at
/// 1.
const VELOCITY_OCTAVES: f32 = 4.0;

/// Input gain of the drive stage with drive just above 0, and how many
/// times larger it is with drive at 1.
const DRIVE_FLOOR: f32 = 0.02;
const DRIVE_RANGE: f32 = 250.0;

/// Output samples of silence after which the down-sampling filter is known
/// to be empty.
const QUIET_SAMPLES: usize = 64;

/// Settings of one oscillator of the [`SubtractiveSynth`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct OscillatorParams {
    /// The waveform. Default saw.
    pub waveform: Waveform,
    /// Volume, 0 to 1. An oscillator at 0 is off and costs nothing.
    /// Default 0.
    pub level: f32,
    /// Tuning in semitones. -36 to 36, default 0.
    pub coarse: i32,
    /// Fine tuning in cents. -100 to 100, default 0.
    pub fine_cents: f32,
    /// Share of each cycle the pulse waveform spends high. 0.5 is a square;
    /// values toward either end sound thinner. 0.05 to 0.95, default 0.5.
    pub pulse_width: f32,
    /// Position from left (-1) to right (1). Default 0.
    pub pan: f32,
}

impl Default for OscillatorParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            level: 0.0,
            coarse: 0,
            fine_cents: 0.0,
            pulse_width: 0.5,
            pan: 0.0,
        }
    }
}

/// Which part of the spectrum the synth's filter lets through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilterMode {
    /// Keeps what is below the cutoff. The classic synth filter.
    #[default]
    LowPass,
    /// Keeps a band around the cutoff.
    BandPass,
    /// Keeps what is above the cutoff.
    HighPass,
}

/// How steeply the synth's filter falls away past the cutoff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilterSlope {
    /// 12 dB per octave: open and bright.
    #[default]
    Db12,
    /// 24 dB per octave: darker and more focused.
    Db24,
}

/// Settings of the synth's filter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct FilterParams {
    /// Default low-pass.
    pub mode: FilterMode,
    /// Default 12 dB per octave.
    pub slope: FilterSlope,
    /// Cutoff in Hz, before key tracking, envelope, velocity and LFOs move
    /// it. 20 to 20000, default 20000 (fully open).
    pub cutoff_hz: f32,
    /// Emphasis at the cutoff, 0 to 1. At 0 there is none; toward 1 the
    /// filter rings and whistles. Default 0.1.
    pub resonance: f32,
    /// How much the cutoff follows the key played, around middle C. At 1 it
    /// moves an octave per octave, so every note is equally bright. 0 to
    /// 1, default 0.
    pub key_tracking: f32,
    /// How far the filter envelope moves the cutoff at its peak, in
    /// octaves. Negative values close the filter instead. -8 to 8, default
    /// 0.
    pub envelope_octaves: f32,
    /// How much playing softly closes the filter. At 1 the softest note is
    /// four octaves darker than the hardest. 0 to 1, default 0.
    pub velocity: f32,
    /// Overdrive before the filter. 0 is clean; higher values are louder,
    /// fuller and grittier. 0 to 1, default 0.
    pub drive: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            mode: FilterMode::LowPass,
            slope: FilterSlope::Db12,
            cutoff_hz: 20_000.0,
            resonance: 0.1,
            key_tracking: 0.0,
            envelope_octaves: 0.0,
            velocity: 0.0,
            drive: 0.0,
        }
    }
}

/// An attack, decay, sustain, release envelope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct EnvelopeParams {
    /// Time to rise to full level after a key goes down, in ms. 0 to
    /// 10000.
    pub attack_ms: f32,
    /// Time to fall from full level to the sustain level, in ms. 1 to
    /// 10000.
    pub decay_ms: f32,
    /// Level held while the key stays down, 0 to 1.
    pub sustain: f32,
    /// Time to fall to silence after the key is let go, in ms. 1 to 10000.
    pub release_ms: f32,
}

impl Default for EnvelopeParams {
    fn default() -> Self {
        Self {
            attack_ms: 2.0,
            decay_ms: 200.0,
            sustain: 0.8,
            release_ms: 150.0,
        }
    }
}

/// Settings of one LFO of the synth. The LFOs run freely and are shared by
/// all voices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LfoParams {
    /// Default sine.
    pub shape: LfoShape,
    /// Speed in Hz. 0.01 to 30, default 5.
    pub rate_hz: f32,
    /// How far it bends the pitch at its extremes, in semitones: vibrato.
    /// -24 to 24, default 0.
    pub pitch_semitones: f32,
    /// How far it moves the filter cutoff at its extremes, in octaves.
    /// -5 to 5, default 0.
    pub cutoff_octaves: f32,
    /// How much it dips the volume: tremolo. 0 to 1, default 0.
    pub amp: f32,
    /// How far it moves the pulse width of every oscillator. -0.45 to
    /// 0.45, default 0.
    pub pulse_width: f32,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            rate_hz: 5.0,
            pitch_semitones: 0.0,
            cutoff_octaves: 0.0,
            amp: 0.0,
            pulse_width: 0.0,
        }
    }
}

/// How the synth handles more than one note at a time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VoiceMode {
    /// Every note gets its own voice.
    #[default]
    Poly,
    /// One note at a time. Each new note restarts the envelopes, and glide
    /// always applies.
    Mono,
    /// One note at a time. A note played while another is still held
    /// takes over without restarting the envelopes, and only such
    /// overlapping notes glide.
    Legato,
}

/// Settings of the [`SubtractiveSynth`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SynthParams {
    /// The three oscillators. By default the first is a saw at full level
    /// and the other two are off.
    pub oscillators: [OscillatorParams; OSCILLATORS],
    /// Copies of each oscillator stacked per note. 1 to 7, default 1.
    pub unison_voices: u8,
    /// How far apart the outermost copies are tuned, in cents each way. 0
    /// to 100, default 20.
    pub unison_detune_cents: f32,
    /// How far the copies are spread across the stereo field, 0 to 1.
    /// Default 0.7.
    pub unison_spread: f32,
    pub filter: FilterParams,
    /// Shapes the volume of each note. Default: attack 2 ms, decay 200 ms,
    /// sustain 0.8, release 150 ms.
    pub amp_envelope: EnvelopeParams,
    /// Moves the filter cutoff over each note by
    /// `filter.envelope_octaves`. Default: attack 2 ms, decay 300 ms,
    /// sustain 0.3, release 200 ms.
    pub filter_envelope: EnvelopeParams,
    pub lfos: [LfoParams; LFOS],
    /// Default poly.
    pub voice_mode: VoiceMode,
    /// Time the pitch takes to slide from the previous note, in ms. 0 to
    /// 2000, default 0 (no slide).
    pub glide_ms: f32,
    /// Most notes sounding at once in poly mode. One more takes over the
    /// oldest. 1 to 32, default 16.
    pub polyphony: u8,
    /// How much playing softly lowers the volume. At 0 every note is at
    /// full level; at 1 volume follows velocity directly. Default 0.7.
    pub amp_velocity: f32,
    /// Output level as a linear gain. 0 to 2, default 0.25, which leaves
    /// room for chords.
    pub gain: f32,
    /// Output position from left (-1) to right (1). Default 0.
    pub pan: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        let mut oscillators = [OscillatorParams::default(); OSCILLATORS];
        oscillators[0].level = 1.0;
        Self {
            oscillators,
            unison_voices: 1,
            unison_detune_cents: 20.0,
            unison_spread: 0.7,
            filter: FilterParams::default(),
            amp_envelope: EnvelopeParams::default(),
            filter_envelope: EnvelopeParams {
                attack_ms: 2.0,
                decay_ms: 300.0,
                sustain: 0.3,
                release_ms: 200.0,
            },
            lfos: [LfoParams::default(); LFOS],
            voice_mode: VoiceMode::Poly,
            glide_ms: 0.0,
            polyphony: 16,
            amp_velocity: 0.7,
            gain: 0.25,
            pan: 0.0,
        }
    }
}

param_set!(SynthParams, "Subtractive synth", {
    choice [oscillators[0].waveform] "oscillators.0.waveform" "Osc 1 waveform" {
        Waveform, Saw,
        [
            Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw",
            Square "square" "Square", Pulse "pulse" "Pulse",
            WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"
        ]
    }
    float [oscillators[0].level] "oscillators.0.level" "Osc 1 level"
        { Fraction, Linear, 0.0, 1.0, 1.0 }
    int [oscillators[0].coarse] "oscillators.0.coarse" "Osc 1 coarse" { Semitones, -36, 36, 0 }
    float [oscillators[0].fine_cents] "oscillators.0.fineCents" "Osc 1 fine"
        { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[0].pulse_width] "oscillators.0.pulseWidth" "Osc 1 pulse width"
        { Fraction, Linear, 0.05, 0.95, 0.5 }
    float [oscillators[0].pan] "oscillators.0.pan" "Osc 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }

    choice [oscillators[1].waveform] "oscillators.1.waveform" "Osc 2 waveform" {
        Waveform, Saw,
        [
            Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw",
            Square "square" "Square", Pulse "pulse" "Pulse",
            WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"
        ]
    }
    float [oscillators[1].level] "oscillators.1.level" "Osc 2 level"
        { Fraction, Linear, 0.0, 1.0, 0.0 }
    int [oscillators[1].coarse] "oscillators.1.coarse" "Osc 2 coarse" { Semitones, -36, 36, 0 }
    float [oscillators[1].fine_cents] "oscillators.1.fineCents" "Osc 2 fine"
        { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[1].pulse_width] "oscillators.1.pulseWidth" "Osc 2 pulse width"
        { Fraction, Linear, 0.05, 0.95, 0.5 }
    float [oscillators[1].pan] "oscillators.1.pan" "Osc 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }

    choice [oscillators[2].waveform] "oscillators.2.waveform" "Osc 3 waveform" {
        Waveform, Saw,
        [
            Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw",
            Square "square" "Square", Pulse "pulse" "Pulse",
            WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"
        ]
    }
    float [oscillators[2].level] "oscillators.2.level" "Osc 3 level"
        { Fraction, Linear, 0.0, 1.0, 0.0 }
    int [oscillators[2].coarse] "oscillators.2.coarse" "Osc 3 coarse" { Semitones, -36, 36, 0 }
    float [oscillators[2].fine_cents] "oscillators.2.fineCents" "Osc 3 fine"
        { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[2].pulse_width] "oscillators.2.pulseWidth" "Osc 3 pulse width"
        { Fraction, Linear, 0.05, 0.95, 0.5 }
    float [oscillators[2].pan] "oscillators.2.pan" "Osc 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }

    int [unison_voices] "unisonVoices" "Unison voices" { None, 1, 7, 1 }
    float [unison_detune_cents] "unisonDetuneCents" "Unison detune"
        { Cents, Linear, 0.0, 100.0, 20.0 }
    float [unison_spread] "unisonSpread" "Unison spread" { Fraction, Linear, 0.0, 1.0, 0.7 }

    choice [filter.mode] "filter.mode" "Filter mode" {
        FilterMode, LowPass,
        [LowPass "lowPass" "Low-pass", BandPass "bandPass" "Band-pass", HighPass "highPass" "High-pass"]
    }
    choice [filter.slope] "filter.slope" "Filter slope" {
        FilterSlope, Db12, [Db12 "db12" "12 dB/oct", Db24 "db24" "24 dB/oct"]
    }
    float [filter.cutoff_hz] "filter.cutoffHz" "Cutoff"
        { Hertz, Logarithmic, 20.0, 20_000.0, 20_000.0 }
    float [filter.resonance] "filter.resonance" "Resonance" { Fraction, Linear, 0.0, 1.0, 0.1 }
    float [filter.key_tracking] "filter.keyTracking" "Key tracking"
        { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [filter.envelope_octaves] "filter.envelopeOctaves" "Filter envelope amount"
        { Octaves, Linear, -8.0, 8.0, 0.0 }
    float [filter.velocity] "filter.velocity" "Filter velocity" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [filter.drive] "filter.drive" "Drive" { Fraction, Linear, 0.0, 1.0, 0.0 }

    float [amp_envelope.attack_ms] "ampEnvelope.attackMs" "Amp attack"
        { Milliseconds, Linear, 0.0, 10_000.0, 2.0 }
    float [amp_envelope.decay_ms] "ampEnvelope.decayMs" "Amp decay"
        { Milliseconds, Logarithmic, 1.0, 10_000.0, 200.0 }
    float [amp_envelope.sustain] "ampEnvelope.sustain" "Amp sustain"
        { Fraction, Linear, 0.0, 1.0, 0.8 }
    float [amp_envelope.release_ms] "ampEnvelope.releaseMs" "Amp release"
        { Milliseconds, Logarithmic, 1.0, 10_000.0, 150.0 }

    float [filter_envelope.attack_ms] "filterEnvelope.attackMs" "Filter attack"
        { Milliseconds, Linear, 0.0, 10_000.0, 2.0 }
    float [filter_envelope.decay_ms] "filterEnvelope.decayMs" "Filter decay"
        { Milliseconds, Logarithmic, 1.0, 10_000.0, 300.0 }
    float [filter_envelope.sustain] "filterEnvelope.sustain" "Filter sustain"
        { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [filter_envelope.release_ms] "filterEnvelope.releaseMs" "Filter release"
        { Milliseconds, Logarithmic, 1.0, 10_000.0, 200.0 }

    choice [lfos[0].shape] "lfos.0.shape" "LFO 1 shape" {
        LfoShape, Sine,
        [
            Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw",
            Square "square" "Square", Random "random" "Random"
        ]
    }
    float [lfos[0].rate_hz] "lfos.0.rateHz" "LFO 1 rate" { Hertz, Logarithmic, 0.01, 30.0, 5.0 }
    float [lfos[0].pitch_semitones] "lfos.0.pitchSemitones" "LFO 1 to pitch"
        { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [lfos[0].cutoff_octaves] "lfos.0.cutoffOctaves" "LFO 1 to cutoff"
        { Octaves, Linear, -5.0, 5.0, 0.0 }
    float [lfos[0].amp] "lfos.0.amp" "LFO 1 to volume" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [lfos[0].pulse_width] "lfos.0.pulseWidth" "LFO 1 to pulse width"
        { Fraction, Linear, -0.45, 0.45, 0.0 }

    choice [lfos[1].shape] "lfos.1.shape" "LFO 2 shape" {
        LfoShape, Sine,
        [
            Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw",
            Square "square" "Square", Random "random" "Random"
        ]
    }
    float [lfos[1].rate_hz] "lfos.1.rateHz" "LFO 2 rate" { Hertz, Logarithmic, 0.01, 30.0, 5.0 }
    float [lfos[1].pitch_semitones] "lfos.1.pitchSemitones" "LFO 2 to pitch"
        { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [lfos[1].cutoff_octaves] "lfos.1.cutoffOctaves" "LFO 2 to cutoff"
        { Octaves, Linear, -5.0, 5.0, 0.0 }
    float [lfos[1].amp] "lfos.1.amp" "LFO 2 to volume" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [lfos[1].pulse_width] "lfos.1.pulseWidth" "LFO 2 to pulse width"
        { Fraction, Linear, -0.45, 0.45, 0.0 }

    choice [voice_mode] "voiceMode" "Voice mode" {
        VoiceMode, Poly, [Poly "poly" "Poly", Mono "mono" "Mono", Legato "legato" "Legato"]
    }
    float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
    int [polyphony] "polyphony" "Polyphony" { None, 1, 32, 16 }
    float [amp_velocity] "ampVelocity" "Volume velocity" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [gain] "gain" "Volume" { Gain, Linear, 0.0, 2.0, 0.25 }
    float [pan] "pan" "Pan" { Pan, Linear, -1.0, 1.0, 0.0 }
});

/// What every voice needs that follows from the parameters alone. Worked
/// out once per control period while a control is moving, not per voice.
#[derive(Clone, Copy)]
struct Patch {
    waveforms: [Waveform; OSCILLATORS],
    /// Tuning of each oscillator in semitones.
    semitones: [f32; OSCILLATORS],
    unison: usize,
    /// Frequency ratio of each unison copy.
    detune: [f32; MAX_UNISON],
    /// Left and right gain of each unison copy of each oscillator: its
    /// level, its share of the stack and its place in the stereo field.
    gains: [[[f32; 2]; MAX_UNISON]; OSCILLATORS],
    /// Left and right gain of each oscillator when it plays noise, which
    /// is not stacked.
    noise_gains: [[f32; 2]; OSCILLATORS],
    audible: [bool; OSCILLATORS],
    /// True when every oscillator sits in the middle of the stereo field,
    /// which makes the left and right channels of a voice identical.
    centred: bool,
    /// Gain into and out of the drive stage. `None` when drive is off.
    drive: Option<(f32, f32)>,
    /// Gain into the filter, lowered as resonance rises so a ringing
    /// filter does not get much louder.
    filter_gain: f32,
    /// Damping (1 / Q) of each filter stage.
    damping: f32,
    two_stages: bool,
    /// Share of the low, band and high output in the filter's result.
    blend: [f32; 3],
}

impl Patch {
    fn new(params: &SynthParams) -> Self {
        let unison = usize::from(params.unison_voices).clamp(1, MAX_UNISON);
        // Copies sit evenly from -1 to 1: the lowest-tuned on the left,
        // the highest on the right.
        let position = |copy: usize| {
            if unison > 1 {
                2.0 * copy as f32 / (unison - 1) as f32 - 1.0
            } else {
                0.0
            }
        };
        let mut detune = [1.0; MAX_UNISON];
        for (copy, ratio) in detune.iter_mut().enumerate().take(unison) {
            *ratio = (params.unison_detune_cents * position(copy) / 1200.0).exp2();
        }
        let share = 1.0 / (unison as f32).sqrt();
        let mut gains = [[[0.0; 2]; MAX_UNISON]; OSCILLATORS];
        let mut noise_gains = [[0.0; 2]; OSCILLATORS];
        for (index, oscillator) in params.oscillators.iter().enumerate() {
            for (copy, gain) in gains[index].iter_mut().enumerate().take(unison) {
                let pan = oscillator.pan + params.unison_spread * position(copy);
                let (left, right) = pan_gains(pan);
                *gain = [
                    oscillator.level * share * left,
                    oscillator.level * share * right,
                ];
            }
            let (left, right) = pan_gains(oscillator.pan);
            noise_gains[index] = [oscillator.level * left, oscillator.level * right];
        }

        let filter = &params.filter;
        let q = std::f32::consts::FRAC_1_SQRT_2
            * (MAX_RESONANCE_Q / std::f32::consts::FRAC_1_SQRT_2).powf(filter.resonance);
        let two_stages = filter.slope == FilterSlope::Db24;
        // Two stages in a row multiply their peaks, so each gets the
        // square root of the Q to keep the total where it was.
        let stage_q = if two_stages { q.sqrt() } else { q };
        let drive = (filter.drive > 0.0).then(|| {
            let gain_in = DRIVE_FLOOR * DRIVE_RANGE.powf(filter.drive);
            // Unity gain as drive leaves 0, rising less than the input
            // gain does, so more drive is louder but mostly dirtier.
            let gain_out = (DRIVE_FLOOR / gain_in).powf(0.7) / DRIVE_FLOOR;
            (gain_in, gain_out)
        });

        Self {
            waveforms: params.oscillators.map(|oscillator| oscillator.waveform),
            semitones: params
                .oscillators
                .map(|oscillator| oscillator.coarse as f32 + oscillator.fine_cents * 0.01),
            unison,
            detune,
            gains,
            noise_gains,
            audible: params.oscillators.map(|oscillator| oscillator.level > 0.0),
            centred: gains
                .iter()
                .flatten()
                .chain(&noise_gains)
                .all(|[left, right]| left == right),
            drive,
            filter_gain: 1.0 / (1.0 + filter.resonance),
            damping: 1.0 / stage_q,
            two_stages,
            blend: match filter.mode {
                FilterMode::LowPass => [1.0, 0.0, 0.0],
                FilterMode::BandPass => [0.0, 1.0, 0.0],
                FilterMode::HighPass => [0.0, 0.0, 1.0],
            },
        }
    }
}

/// What the LFOs add to every voice during one control period.
#[derive(Clone, Copy)]
struct Modulation {
    pitch_semitones: f32,
    cutoff_octaves: f32,
    level: f32,
    pulse_width: f32,
}

impl Modulation {
    const NONE: Self = Self {
        pitch_semitones: 0.0,
        cutoff_octaves: 0.0,
        level: 1.0,
        pulse_width: 0.0,
    };
}

/// One oscillator of one voice.
#[derive(Clone, Copy)]
struct OscillatorState {
    phases: [f32; MAX_UNISON],
    /// Phase step per oversampled sample before unison detune, and how
    /// much it changes each sample on its way to the next control value.
    increment: f32,
    increment_step: f32,
    width: f32,
    width_step: f32,
    pink: PinkNoise,
}

impl OscillatorState {
    const SILENT: Self = Self {
        phases: [0.0; MAX_UNISON],
        increment: 0.0,
        increment_step: 0.0,
        width: 0.5,
        width_step: 0.0,
        pink: PinkNoise::new(),
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    /// The key is down.
    Held,
    /// The key was let go and the release is playing.
    Released,
    /// Being faded out quickly to make room or to stop.
    Choked,
}

#[derive(Clone, Copy)]
struct HeldNote {
    id: NoteInstanceId,
    key: u8,
    velocity: f32,
    pan: f32,
    expression: NoteExpression,
    pitch: f32,
    /// Linear gain from the expression map of the channel. 1 is unity.
    gain: f32,
}

impl HeldNote {
    const EMPTY: Self = Self { id: NoteInstanceId(0), key: 0, velocity: 0.0, pan: 0.0, expression: NoteExpression::NEUTRAL, pitch: 0.0, gain: 1.0 };
}

struct Voice {
    stage: Stage,
    instance: NoteInstanceId,
    key: u8,
    velocity: f32,
    expression: NoteExpression,
    note_pan: f32,
    /// Linear gain from the expression map of the channel. 1 is unity.
    gain: f32,
    /// Counts up with every note, so the oldest voice can be found.
    order: u64,
    /// Pitch in keys. It advances once per control period, which is how a
    /// glide moves.
    pitch: LinearRamp,
    oscillators: [OscillatorState; OSCILLATORS],
    amp: Adsr,
    filter_envelope: Adsr,
    /// Two stages, each for left and right.
    filters: [[Svf; 2]; 2],
    /// The filter's integrator gain and its change per sample.
    cutoff: f32,
    cutoff_step: f32,
    /// Velocity and tremolo, and their change per sample.
    level: f32,
    level_step: f32,
    choke: LinearRamp,
    seed: u32,
    rng: Rng,
}

/// The phase step and pulse width of one oscillator for each sample of a
/// stretch being rendered.
struct Glide {
    /// Which of the voice's oscillators this is.
    oscillator: usize,
    increments: [f32; OVERSAMPLED],
    widths: [f32; OVERSAMPLED],
}

impl Glide {
    /// Advances the oscillator's gliding values by `frames` samples and
    /// records each step.
    ///
    /// The values are stepped one sample at a time, and every unison copy
    /// reads the same recorded steps. Adding up the steps any other way
    /// would round differently depending on where a block happened to
    /// end, and the output would depend on the host's block size.
    fn advance(oscillator: usize, state: &mut OscillatorState, frames: usize) -> Self {
        let mut glide = Self {
            oscillator,
            increments: [0.0; OVERSAMPLED],
            widths: [0.0; OVERSAMPLED],
        };
        let steps = glide.increments.iter_mut().zip(&mut glide.widths);
        for (increment, width) in steps.take(frames) {
            *increment = state.increment;
            *width = state.width;
            state.increment += state.increment_step;
            state.width += state.width_step;
        }
        glide
    }
}

/// Adds one pitched oscillator, with all its unison copies, to a voice's
/// buffers.
#[inline]
fn render_pitched(
    (state, patch, glide): (&mut OscillatorState, &Patch, &Glide),
    left: &mut [f32],
    right: &mut [f32],
    sample: impl Fn(f32, f32, f32) -> f32,
) {
    let gains = &patch.gains[glide.oscillator];
    let copies = state.phases.iter_mut().zip(&patch.detune).zip(gains);
    for ((phase_slot, ratio), [gain_left, gain_right]) in copies.take(patch.unison) {
        let mut phase = *phase_slot;
        let steps = glide.increments.iter().zip(&glide.widths);
        for ((left, right), (increment, width)) in left.iter_mut().zip(right.iter_mut()).zip(steps)
        {
            let increment = increment * ratio;
            let value = sample(phase, increment, *width);
            *left += value * gain_left;
            *right += value * gain_right;
            phase += increment;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        *phase_slot = phase;
    }
}

impl Voice {
    fn new(index: usize) -> Self {
        Self::silent(index as u32 * 7_919 + 1)
    }

    /// A voice with nothing sounding and nothing remembered.
    fn silent(seed: u32) -> Self {
        Self {
            stage: Stage::Idle,
            instance: NoteInstanceId(0),
            key: 60,
            velocity: 0.0,
            expression: NoteExpression::default(),
            note_pan: 0.0,
            gain: 1.0,
            order: 0,
            pitch: LinearRamp::new(60.0),
            oscillators: [OscillatorState::SILENT; OSCILLATORS],
            amp: Adsr::default(),
            filter_envelope: Adsr::default(),
            filters: [[Svf::default(); 2]; 2],
            cutoff: 0.0,
            cutoff_step: 0.0,
            level: 0.0,
            level_step: 0.0,
            choke: LinearRamp::new(1.0),
            seed,
            rng: Rng::new(seed),
        }
    }

    fn is_sounding(&self) -> bool {
        matches!(self.stage, Stage::Held | Stage::Released)
    }

    /// How loud the voice is right now, for choosing which fading voice to
    /// take over.
    fn loudness(&self) -> f32 {
        self.amp.value() * self.choke.value()
    }

    fn configure_envelopes(&mut self, params: &SynthParams, sample_rate: f32) {
        let amp = &params.amp_envelope;
        self.amp.configure(
            amp.attack_ms.max(MIN_AMP_SEGMENT_MS),
            amp.decay_ms,
            amp.sustain,
            (amp.release_ms * self.expression.release_multiplier()).max(MIN_AMP_SEGMENT_MS),
            sample_rate * 2.0,
        );
        // The filter envelope is only read once per control period, so it
        // runs at that rate.
        let filter = &params.filter_envelope;
        self.filter_envelope.configure(
            filter.attack_ms,
            filter.decay_ms,
            filter.sustain,
            filter.release_ms * self.expression.release_multiplier(),
            sample_rate / CONTROL_PERIOD as f32,
        );
    }

    /// Works out where pitch, pulse width, cutoff and level should be one
    /// control period from now and sets them gliding there. With `snap`
    /// they jump instead, which is what a note that has just started
    /// needs.
    fn control(
        &mut self,
        params: &SynthParams,
        patch: &Patch,
        modulation: &Modulation,
        sample_rate: f32,
        snap: bool,
    ) {
        let rate = sample_rate * 2.0;
        let (pitch, envelope) = if snap {
            (self.pitch.value(), self.filter_envelope.value())
        } else {
            (self.pitch.tick(), self.filter_envelope.tick())
        };
        let steps = 1.0 / OVERSAMPLED as f32;

        // The widest unison detune must still fit under the limit the
        // waveform corrections are valid for.
        let highest = MAX_INCREMENT / patch.detune[patch.unison - 1].max(1.0);
        for (index, state) in self.oscillators.iter_mut().enumerate() {
            let key = pitch + self.expression.fine_pitch_cents / 100.0 + patch.semitones[index] + modulation.pitch_semitones;
            let increment = (key_to_hz(key) / rate).min(highest);
            let width =
                (params.oscillators[index].pulse_width + modulation.pulse_width).clamp(0.02, 0.98);
            if snap {
                state.increment = increment;
                state.increment_step = 0.0;
                state.width = width;
                state.width_step = 0.0;
            } else {
                state.increment_step = (increment - state.increment) * steps;
                state.width_step = (width - state.width) * steps;
            }
        }

        let filter = &params.filter;
        let octaves = filter.key_tracking * (pitch - 60.0) / 12.0
            + filter.envelope_octaves * envelope
            + modulation.cutoff_octaves
            + self.expression.cutoff_octaves()
            + filter.velocity * VELOCITY_OCTAVES * (self.velocity - 1.0);
        let cutoff_hz = (filter.cutoff_hz * octaves.exp2()).clamp(16.0, 0.45 * rate.min(48_000.0));
        let cutoff = cutoff_gain(cutoff_hz, rate);
        let level = (1.0 - params.amp_velocity * (1.0 - self.velocity)) * modulation.level * self.gain;
        if snap {
            self.cutoff = cutoff;
            self.cutoff_step = 0.0;
            self.level = level;
            self.level_step = 0.0;
        } else {
            // A cutoff that has all but arrived is put on its target, so
            // that rounding cannot keep it creeping forever.
            if (cutoff - self.cutoff).abs() <= 1.0e-6 * cutoff {
                self.cutoff = cutoff;
            }
            self.cutoff_step = (cutoff - self.cutoff) * steps;
            self.level_step = (level - self.level) * steps;
        }

        for filter in self.filters.iter_mut().flatten() {
            filter.flush();
        }
    }

    /// Renders `left.len()` oversampled samples of the voice and adds them
    /// to the two buses.
    fn render(&mut self, patch: &Patch, bus_left: &mut [f32], bus_right: &mut [f32]) {
        let frames = bus_left.len();
        let mut left_buffer = [0.0_f32; OVERSAMPLED];
        let mut right_buffer = [0.0_f32; OVERSAMPLED];
        let (left, right) = (&mut left_buffer[..frames], &mut right_buffer[..frames]);

        for (index, state) in self.oscillators.iter_mut().enumerate() {
            // The glide moves on even when the oscillator is silent, so
            // that it is in the right place if its level comes up.
            let glide = Glide::advance(index, state, frames);
            if !patch.audible[index] {
                continue;
            }
            // One call per waveform, so that each gets its own copy of the
            // inner loop with the waveform compiled in.
            let oscillator = (&mut *state, patch, &glide);
            match patch.waveforms[index] {
                Waveform::Sine => {
                    render_pitched(oscillator, left, right, |phase, _, _| sine(phase))
                }
                Waveform::Triangle => {
                    render_pitched(oscillator, left, right, |phase, step, _| {
                        triangle(phase, step)
                    });
                }
                Waveform::Saw => {
                    render_pitched(oscillator, left, right, |phase, step, _| saw(phase, step))
                }
                Waveform::Square => {
                    render_pitched(oscillator, left, right, |phase, step, _| {
                        pulse(phase, step, 0.5)
                    });
                }
                Waveform::Pulse => render_pitched(oscillator, left, right, pulse),
                Waveform::WhiteNoise | Waveform::PinkNoise => {
                    let pink = patch.waveforms[index] == Waveform::PinkNoise;
                    let [gain_left, gain_right] = patch.noise_gains[index];
                    for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                        let value = if pink {
                            state.pink.tick(&mut self.rng)
                        } else {
                            self.rng.bipolar()
                        };
                        *left += value * gain_left;
                        *right += value * gain_right;
                    }
                }
            }
        }

        // With everything panned to the middle the two channels are the
        // same signal, so the right one is not worked out separately but
        // copied from the left at the end.
        let centred = patch.centred;
        if let Some((gain_in, gain_out)) = patch.drive {
            let driven = if centred {
                &mut right[..0]
            } else {
                &mut *right
            };
            for sample in left.iter_mut().chain(driven.iter_mut()) {
                *sample = soft_clip(*sample * gain_in) * gain_out;
            }
        }

        let [low, band, high] = patch.blend;
        // The band output peaks at Q. Scaling by the damping brings it
        // back to unity.
        let damping = (patch.damping / self.expression.resonance_multiplier()).clamp(0.05, 20.0);
        let band = band * damping;
        let filter_gain = patch.filter_gain;
        let [first, second] = &mut self.filters;
        // A cutoff that is standing still needs its coefficients only once.
        let moving = self.cutoff_step != 0.0;
        let mut coeffs = SvfCoeffs::from_gain(self.cutoff, damping);
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            if moving {
                self.cutoff += self.cutoff_step;
                coeffs = SvfCoeffs::from_gain(self.cutoff, damping);
            }
            let blend = |filter: &mut Svf, input: f32| {
                let out = filter.tick(&coeffs, input);
                out.low * low + out.band * band + out.high * high
            };
            *left = blend(&mut first[0], *left * filter_gain);
            if patch.two_stages {
                *left = blend(&mut second[0], *left);
            }
            if centred {
                *right = *left;
            } else {
                *right = blend(&mut first[1], *right * filter_gain);
                if patch.two_stages {
                    *right = blend(&mut second[1], *right);
                }
            }
        }
        if centred {
            // Keep the right filters in step, so that panning something
            // away from the middle later starts from the right place.
            first[1] = first[0];
            second[1] = second[0];
        }

        for index in 0..frames {
            self.level += self.level_step;
            let gain = self.amp.tick() * self.level * self.choke.tick();
            let (pan_left, pan_right) = pan_gains(self.note_pan);
            bus_left[index] += left[index] * gain * (pan_left * std::f32::consts::SQRT_2).min(1.0);
            bus_right[index] += right[index] * gain * (pan_right * std::f32::consts::SQRT_2).min(1.0);
        }

        let faded = self.stage == Stage::Choked && self.choke.is_settled();
        if faded || self.amp.is_idle() {
            self.stage = Stage::Idle;
        }
    }
}

/// A polyphonic subtractive synthesizer: three oscillators into a
/// resonant filter into an amplifier, each shaped per note.
///
/// # Signal path
///
/// Every voice mixes three oscillators (each stacked up to seven times for
/// unison), optionally overdrives the mix, filters it, and shapes its
/// volume with an envelope. A second envelope and two LFOs move the
/// filter; the LFOs can also move pitch, volume and pulse width.
///
/// The oscillators use four-point PolyBLEP corrections, and the whole voice
/// runs at twice the sample rate and is brought down through a half-band
/// filter. Together these put aliasing far below audibility even for the
/// highest notes, keep the filter in tune right up to 20 kHz, and catch
/// most of what the drive stage would otherwise fold back. The filter is a
/// trapezoidal state-variable filter, which stays stable at any cutoff and
/// resonance, however fast they move.
///
/// # Notes
///
/// A new note beyond the polyphony limit takes over from the oldest
/// released note, or the oldest held note if none is released. The note
/// that loses its voice is faded out over 4 ms in a spare voice, so
/// stealing never clicks. [`all_notes_off`](Instrument::all_notes_off)
/// uses the same fade.
///
/// With one unison voice every note starts its oscillators at the same
/// phase, so repeated notes sound identical. With more, each copy starts at
/// a random phase, as free-running oscillators would.
///
/// [`process`](Instrument::process) replaces the contents of its buffers.
/// The down-sampling filter delays the output by a fixed 12 samples,
/// reported by [`latency_samples`](Instrument::latency_samples).
pub struct SubtractiveSynth {
    sample_rate: f32,
    params: SynthParams,
    /// The parameters on their way to `params`.
    current: SynthParams,
    settling: bool,
    patch: Patch,
    modulation: Modulation,
    voices: [Voice; VOICES],
    lfos: [Lfo; LFOS],
    decimators: [HalfbandDecimator; 2],
    gain_left: LinearRamp,
    gain_right: LinearRamp,
    /// Instances held in mono mode with their complete voice controls.
    held: [HeldNote; HELD_KEYS],
    held_len: usize,
    /// The key of the last note started, which is where a glide begins.
    last_key: Option<u8>,
    notes_started: u64,
    legacy_instances: u64,
    until_control: usize,
    /// Output samples since a voice last sounded.
    quiet: usize,
    amount: f32,
    fresh: bool,
}

impl Default for SubtractiveSynth {
    fn default() -> Self {
        let params = SynthParams::default();
        let mut synth = Self {
            sample_rate: 48_000.0,
            params,
            current: params,
            settling: false,
            patch: Patch::new(&params),
            modulation: Modulation::NONE,
            voices: std::array::from_fn(Voice::new),
            lfos: [Lfo::new(1), Lfo::new(2)],
            decimators: [HalfbandDecimator::default(); 2],
            gain_left: LinearRamp::new(0.0),
            gain_right: LinearRamp::new(0.0),
            held: [HeldNote::EMPTY; HELD_KEYS],
            held_len: 0,
            last_key: None,
            notes_started: 0,
            legacy_instances: 0,
            until_control: 0,
            quiet: QUIET_SAMPLES + 1,
            amount: 1.0,
            fresh: true,
        };
        synth.reset();
        synth
    }
}

impl SubtractiveSynth {
    /// Samples by which the output lags the notes, at every setting and
    /// sample rate.
    pub const LATENCY_SAMPLES: usize = HalfbandDecimator::LATENCY;

    fn output_gains(&self) -> (f32, f32) {
        let (left, right) = pan_gains(self.current.pan);
        (self.current.gain * left, self.current.gain * right)
    }

    /// Control periods a glide lasts.
    fn glide_steps(&self) -> u32 {
        (self.params.glide_ms * 0.001 * self.sample_rate / CONTROL_PERIOD as f32).round() as u32
    }

    fn control(&mut self) {
        if self.settling {
            self.settling = self.current.approach(&self.params, self.amount);
            self.patch = Patch::new(&self.current);
        }

        let mut modulation = Modulation::NONE;
        let per_period = CONTROL_PERIOD as f32 / self.sample_rate;
        for (lfo, params) in self.lfos.iter_mut().zip(&self.current.lfos) {
            let value = lfo.tick(params.shape, params.rate_hz * per_period);
            modulation.pitch_semitones += params.pitch_semitones * value;
            modulation.cutoff_octaves += params.cutoff_octaves * value;
            modulation.level *= 1.0 - params.amp * (0.5 - 0.5 * value);
            modulation.pulse_width += params.pulse_width * value;
        }
        self.modulation = modulation;

        let (left, right) = self.output_gains();
        self.gain_left.set_target(left, CONTROL_PERIOD as u32);
        self.gain_right.set_target(right, CONTROL_PERIOD as u32);

        for voice in &mut self.voices {
            if voice.stage != Stage::Idle {
                voice.control(
                    &self.current,
                    &self.patch,
                    &self.modulation,
                    self.sample_rate,
                    false,
                );
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len();
        let mut bus_left = [0.0_f32; OVERSAMPLED];
        let mut bus_right = [0.0_f32; OVERSAMPLED];
        let mut sounding = false;
        for voice in &mut self.voices {
            if voice.stage != Stage::Idle {
                voice.render(
                    &self.patch,
                    &mut bus_left[..2 * frames],
                    &mut bus_right[..2 * frames],
                );
                sounding = true;
            }
        }
        self.quiet = if sounding {
            0
        } else {
            self.quiet.saturating_add(frames)
        };
        if self.quiet > QUIET_SAMPLES {
            // The filters below hold nothing but zeros by now. The gains
            // still advance, so that the output does not depend on when
            // this shortcut was taken.
            for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                self.gain_left.tick();
                self.gain_right.tick();
                *left = 0.0;
                *right = 0.0;
            }
            return;
        }
        for index in 0..frames {
            let down_left = self.decimators[0].tick(bus_left[2 * index], bus_left[2 * index + 1]);
            let down_right =
                self.decimators[1].tick(bus_right[2 * index], bus_right[2 * index + 1]);
            left[index] = down_left * self.gain_left.tick();
            right[index] = down_right * self.gain_right.tick();
        }
    }

    /// Starts a quick fade that ends with the voice free.
    fn choke(&mut self, index: usize) {
        let samples = ms_to_samples(CHOKE_MS, self.sample_rate * 2.0);
        let voice = &mut self.voices[index];
        voice.stage = Stage::Choked;
        voice.choke.set_target(0.0, samples);
    }

    /// The voice a new note should take: a free one, or failing that the
    /// quietest of the ones fading out.
    fn free_voice(&self) -> usize {
        if let Some(index) = self
            .voices
            .iter()
            .position(|voice| voice.stage == Stage::Idle)
        {
            return index;
        }
        let mut quietest = 0;
        let mut level = f32::INFINITY;
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.stage == Stage::Choked && voice.loudness() < level {
                quietest = index;
                level = voice.loudness();
            }
        }
        quietest
    }

    /// The sounding voice that started first, preferring ones whose key
    /// has been let go.
    fn oldest_sounding(&self) -> Option<usize> {
        let oldest = |stage: Stage| {
            self.voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.stage == stage)
                .min_by_key(|(_, voice)| voice.order)
                .map(|(index, _)| index)
        };
        oldest(Stage::Released).or_else(|| oldest(Stage::Held))
    }

    /// The sounding voice that started last. In the mono modes it is the
    /// only one.
    fn newest_sounding(&self) -> Option<usize> {
        self.voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.is_sounding())
            .max_by_key(|(_, voice)| voice.order)
            .map(|(index, _)| index)
    }

    /// Starts a note in the voice at `index`. `glide_from` is the key to
    /// slide in from, if any.
    fn start_voice(&mut self, index: usize, note: HeldNote, glide_from: Option<u8>) {
        let HeldNote { id, key, velocity, pan, expression, pitch, gain } = note;
        let glide_steps = self.glide_steps();
        let voice = &mut self.voices[index];
        if voice.stage == Stage::Idle {
            *voice = Voice::silent(voice.seed);
            // Each note draws its own random numbers, so repeated notes do
            // not repeat their noise or their unison phases.
            voice.rng = Rng::new(voice.seed.wrapping_add(self.notes_started as u32 * 31));
            if self.patch.unison > 1 {
                for state in &mut voice.oscillators {
                    for phase in &mut state.phases {
                        *phase = voice.rng.unipolar();
                    }
                }
            }
        } else {
            // Taking over a voice that is still fading: carry on from the
            // level it has reached, so there is no jump.
            voice.amp.scale_level(voice.choke.value());
        }
        voice.choke.snap(1.0);
        voice.stage = Stage::Held;
        voice.key = key;
        voice.instance = id;
        voice.velocity = velocity;
        voice.expression = expression;
        voice.note_pan = pan;
        voice.gain = gain;
        voice.order = self.notes_started;
        self.notes_started = self.notes_started.wrapping_add(1);
        match glide_from {
            Some(from) if glide_steps > 0 => {
                voice.pitch.snap(f32::from(from));
                voice.pitch.set_target(pitch - expression.fine_pitch_cents / 100.0, glide_steps);
            }
            _ => voice.pitch.snap(pitch - expression.fine_pitch_cents / 100.0),
        }
        voice.configure_envelopes(&self.params, self.sample_rate);
        voice.amp.gate_on();
        voice.filter_envelope.gate_on();
        voice.control(
            &self.current,
            &self.patch,
            &self.modulation,
            self.sample_rate,
            true,
        );
        self.last_key = Some(key);
        self.quiet = 0;
    }

    fn start_poly(&mut self, note: HeldNote) {
        let limit = usize::from(self.params.polyphony).clamp(1, MAX_POLYPHONY);
        while self
            .voices
            .iter()
            .filter(|voice| voice.is_sounding())
            .count()
            >= limit
        {
            match self.oldest_sounding() {
                Some(index) => self.choke(index),
                None => break,
            }
        }
        let index = self.free_voice();
        self.start_voice(index, note, self.last_key);
    }

    fn start_mono(&mut self, note: HeldNote) {
        let HeldNote { id, key, velocity, pan, expression, pitch, gain } = note;
        self.forget_instance(id);
        if self.held_len == HELD_KEYS {
            self.held.copy_within(1.., 0);
            self.held_len -= 1;
        }
        self.held[self.held_len] = note;
        self.held_len += 1;

        let legato = self.params.voice_mode == VoiceMode::Legato;
        let Some(index) = self.newest_sounding() else {
            let index = self.free_voice();
            // Legato only slides between notes that overlap.
            let glide_from = if legato { None } else { self.last_key };
            self.start_voice(index, note, glide_from);
            return;
        };
        let glide_steps = self.glide_steps();
        let voice = &mut self.voices[index];
        let overlapping = voice.stage == Stage::Held;
        if glide_steps > 0 && (!legato || overlapping) {
            voice.pitch.set_target(pitch - expression.fine_pitch_cents / 100.0, glide_steps);
        } else {
            voice.pitch.snap(pitch - expression.fine_pitch_cents / 100.0);
        }
        voice.key = key;
        voice.instance = id;
        voice.stage = Stage::Held;
        voice.expression = expression;
        voice.note_pan = pan;
        voice.velocity = velocity;
        voice.gain = gain;
        voice.configure_envelopes(&self.params, self.sample_rate);
        voice.order = self.notes_started;
        self.notes_started = self.notes_started.wrapping_add(1);
        if !(legato && overlapping) {
            voice.amp.gate_on();
            voice.filter_envelope.gate_on();
        }
        self.last_key = Some(key);
    }

    /// Removes just this occurrence, preserving other notes on the same key.
    fn forget_instance(&mut self, id: NoteInstanceId) {
        if let Some(position) = self.held[..self.held_len]
            .iter()
            .position(|held| held.id == id)
        {
            self.held.copy_within(position + 1..self.held_len, position);
            self.held_len -= 1;
        }
    }

    fn release_mono(&mut self, id: NoteInstanceId) {
        self.forget_instance(id);
        let glide_steps = self.glide_steps();
        let previous = self.held_len.checked_sub(1).map(|last| self.held[last]);
        for voice in &mut self.voices {
            if voice.stage != Stage::Held || voice.instance != id {
                continue;
            }
            match previous {
                // Another key is still down: go back to it.
                Some(previous) => {
                    voice.instance = previous.id;
                    voice.key = previous.key;
                    voice.velocity = previous.velocity;
                    voice.expression = previous.expression;
                    voice.note_pan = previous.pan;
                    voice.gain = previous.gain;
                    voice.configure_envelopes(&self.params, self.sample_rate);
                    if glide_steps > 0 {
                        voice.pitch.set_target(previous.pitch - previous.expression.fine_pitch_cents / 100.0, glide_steps);
                    } else {
                        voice.pitch.snap(previous.pitch - previous.expression.fine_pitch_cents / 100.0);
                    }
                    self.last_key = Some(previous.key);
                }
                None => {
                    voice.stage = Stage::Released;
                    voice.amp.gate_off();
                    voice.filter_envelope.gate_off();
                }
            }
        }
    }
}

impl Instrument for SubtractiveSynth {
    type Params = SynthParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = sample_rate.max(1.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.amount = smoothing_coefficient(
            CONTROL_SMOOTHING_MS,
            self.sample_rate / CONTROL_PERIOD as f32,
        );
        self.current = self.params;
        self.settling = false;
        self.patch = Patch::new(&self.current);
        self.modulation = Modulation::NONE;
        for (index, voice) in self.voices.iter_mut().enumerate() {
            *voice = Voice::new(index);
        }
        for lfo in &mut self.lfos {
            lfo.reset();
        }
        for decimator in &mut self.decimators {
            decimator.reset();
        }
        let (left, right) = self.output_gains();
        self.gain_left.snap(left);
        self.gain_right.snap(right);
        self.held_len = 0;
        self.last_key = None;
        self.notes_started = 0;
        self.legacy_instances = 0;
        self.until_control = 0;
        self.quiet = QUIET_SAMPLES + 1;
    }

    fn set_params(&mut self, params: &SynthParams) {
        let params = params.sanitized();
        let previous = std::mem::replace(&mut self.params, params);
        if self.fresh {
            self.current = params;
            self.settling = false;
            self.patch = Patch::new(&self.current);
            let (left, right) = self.output_gains();
            self.gain_left.snap(left);
            self.gain_right.snap(right);
        } else {
            self.settling = true;
        }
        if params.voice_mode != previous.voice_mode {
            // Voices from the old mode would not follow the new one's
            // rules.
            self.all_notes_off();
        }
        if params.amp_envelope != previous.amp_envelope
            || params.filter_envelope != previous.filter_envelope
        {
            for voice in &mut self.voices {
                if voice.stage != Stage::Idle {
                    voice.configure_envelopes(&params, self.sample_rate);
                }
            }
        }
    }

    fn note_on(&mut self, key: u8, velocity: f32) {
        self.note_on_expression(key, velocity, 0.0, NoteExpression::default());
    }

    fn note_off(&mut self, key: u8) {
        let key = key.min(127);
        if self.params.voice_mode != VoiceMode::Poly {
            while let Some(note) = self.held[..self.held_len].iter().find(|note| note.key == key).copied() {
                self.release_mono(note.id);
            }
            return;
        }
        for voice in &mut self.voices {
            if voice.stage == Stage::Held && voice.key == key {
                voice.stage = Stage::Released;
                voice.amp.gate_off();
                voice.filter_envelope.gate_off();
            }
        }
    }

    fn note_on_expression(&mut self, key: u8, velocity: f32, pan: f32, expression: NoteExpression) {
        if !velocity.is_finite() || velocity <= 0.0 { self.note_off(key); return; }
        // Legacy key calls use a separate namespace from engine-owned ids.
        self.legacy_instances = self.legacy_instances.wrapping_add(1) & (u64::MAX >> 1);
        if self.legacy_instances == 0 { self.legacy_instances = 1; }
        self.note_on_instance(NoteInstanceId(self.legacy_instances | (1 << 63)), key, velocity, pan, expression);
    }

    fn supports_note_instances(&self) -> bool { true }

    fn note_on_instance(&mut self, id: NoteInstanceId, key: u8, velocity: f32, pan: f32, expression: NoteExpression) {
        if id.0 == 0 || !velocity.is_finite() || velocity <= 0.0 { return; }
        let note = HeldNote {
            id,
            key: key.min(127),
            velocity: velocity.min(1.0),
            pan: if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 },
            expression: expression.clamped(),
            pitch: f32::from(key.min(127)) + expression.clamped().fine_pitch_cents / 100.0,
            gain: 1.0,
        };
        match self.params.voice_mode {
            VoiceMode::Poly => self.start_poly(note),
            VoiceMode::Mono | VoiceMode::Legato => self.start_mono(note),
        }
    }

    fn note_off_instance(&mut self, id: NoteInstanceId, _key: u8) {
        if self.params.voice_mode != VoiceMode::Poly { self.release_mono(id); return; }
        for voice in &mut self.voices {
            if voice.stage == Stage::Held && voice.instance == id {
                voice.stage = Stage::Released;
                voice.amp.gate_off();
                voice.filter_envelope.gate_off();
            }
        }
    }

    fn set_note_expression(&mut self, id: NoteInstanceId, pan: f32, expression: NoteExpression) {
        let expression = expression.clamped();
        let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
        for held in &mut self.held[..self.held_len] {
            if held.id == id {
                held.pitch += (expression.fine_pitch_cents - held.expression.fine_pitch_cents) / 100.0;
                held.expression = expression;
                held.pan = pan;
            }
        }
        for voice in &mut self.voices {
            if voice.stage != Stage::Idle && voice.instance == id {
                let release_changed = voice.expression.release != expression.release;
                let controls_changed = voice.expression.fine_pitch_cents != expression.fine_pitch_cents
                    || voice.expression.modulation_x != expression.modulation_x
                    || voice.expression.modulation_y != expression.modulation_y;
                voice.expression = expression;
                voice.note_pan = pan;
                if release_changed { voice.configure_envelopes(&self.params, self.sample_rate); }
                // Expression updates can arrive every output frame. Read the
                // current control/envelope values without advancing their clocks.
                if controls_changed { voice.control(&self.current, &self.patch, &self.modulation, self.sample_rate, true); }
            }
        }
    }

    fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
        if !gain.is_finite() { return; }
        let gain = gain.clamp(0.0, 16.0);
        for held in &mut self.held[..self.held_len] {
            if held.id == id { held.gain = gain; }
        }
        for voice in &mut self.voices {
            if voice.stage != Stage::Idle && voice.instance == id && voice.gain != gain {
                voice.gain = gain;
                voice.control(&self.current, &self.patch, &self.modulation, self.sample_rate, true);
            }
        }
    }

    fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        if !pitch.is_finite() { return; }
        for held in &mut self.held[..self.held_len] {
            if held.id == id { held.pitch = pitch.clamp(-12.0, 139.0); }
        }
        for voice in &mut self.voices {
            if voice.stage != Stage::Idle && voice.instance == id {
                voice.pitch.snap(pitch.clamp(-12.0, 139.0) - voice.expression.fine_pitch_cents / 100.0);
                voice.control(&self.current, &self.patch, &self.modulation, self.sample_rate, true);
            }
        }
    }

    fn all_notes_off(&mut self) {
        for index in 0..VOICES {
            if self.voices[index].stage != Stage::Idle {
                self.choke(index);
            }
        }
        self.held_len = 0;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        let frames = left.len().min(right.len());
        let mut start = 0;
        while start < frames {
            if self.until_control == 0 {
                self.control();
                self.until_control = CONTROL_PERIOD;
            }
            let end = start + (frames - start).min(self.until_control);
            self.render(&mut left[start..end], &mut right[start..end]);
            self.until_control -= end - start;
            start = end;
        }
    }

    fn active_voices(&self) -> usize {
        self.voices
            .iter()
            .filter(|voice| voice.stage != Stage::Idle)
            .count()
    }

    fn latency_samples(&self) -> usize {
        Self::LATENCY_SAMPLES
    }

    fn tail_samples(&self) -> usize {
        QUIET_SAMPLES
    }
}
