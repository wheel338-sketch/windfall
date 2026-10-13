//! From FL Studio's own effects to Windfall's.
//!
//! The name tables say which effect stands for which. The names
//! in their source column are the internal names FL Studio writes into a project
//! file, which is the only way to tell what a slot holds. They are used
//! here to recognise the plugins and nowhere else: Windfall's effects have
//! their own names.
//!
//! | FL Studio writes | Windfall effect | Settings |
//! |---|---|---|
//! | Fruity Parametric EQ 2, Fruity Parametric EQ | Parametric EQ | bands by kind, as far as Windfall's seven fixed bands go |
//! | Fruity Compressor | Compressor | threshold, ratio, gain, attack, release, knee |
//! | Fruity Limiter | Limiter | public native limiter/compressor prefix |
//! | Fruity Reeverb 2, Fruity Reeverb | Reverb | size, decay, diffusion, cuts, mix; damping by a rule of thumb |
//! | Fruity Delay 2, Fruity Delay | Delay | time, feedback, ping-pong, mix |
//! | Fruity Delay 3 | Delay | public native fields and normalized automation |
//!
//! The additional names in `DEFAULT_EFFECTS` select registered effects with
//! native state translators where the public sources define layouts; missing
//! layouts receive named source-evidence reports and sanitized defaults. The full mapping is documented in `docs/integration/seams/flp-effects.md`.
//!
//! Decoded states are rows of 32-bit numbers. Their order comes from DawVert
//! `data_main/datadef/fl_studio.ddef`, their ranges from DawVert
//! `data_main/dataset/fl_studio.dset`, and what they mean in real units
//! from DawVert `plugins/plugconv/universal__n_flstudio.py` and
//! `data_main/plugts/flstudio_univ.pltr`. A state that is shorter than the
//! table expects, as older versions wrote, leaves the settings it lacks at
//! Windfall's defaults.

use windfall_dsp::{
    CompressorParams, DelayMode, DelayParams, EffectKind, EffectParams, EqBand, EqCutBand,
    EqParams, LimiterParams, NoteDivision, ParamSet, ReverbParams,
};

use crate::plugin::Numbers;

/// A setting of a Windfall effect or instrument that follows one
/// parameter of the FL Studio plugin it stands for, so that automation of
/// the one can be carried over to the other.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ParamLink {
    /// The number FL Studio knows the parameter by.
    pub(crate) fl_param: u16,
    /// What an automation value of 0 and of 1 is in FL Studio's numbers.
    pub(crate) fl_range: (f64, f64),
    /// The id of the setting in Windfall's descriptors.
    pub(crate) id: &'static str,
    /// From FL Studio's number to the setting's own unit.
    pub(crate) convert: fn(f64) -> f64,
    /// A disclosed linear range fit when public sources give a knob range
    /// but no acoustic transfer function. Never used for existing formulas.
    pub(crate) destination_range: Option<(f64, f64)>,
}

impl ParamLink {
    /// PyFLP ControllerEvent stores normalized f32; DawVert's importer uses
    /// floatbyteint2float for plugin event lanes. Retain compatibility with
    /// older native-unit fixtures/records whose small words are subnormal as
    /// floats. Only a normal finite 0..1 float is treated as normalized.
    pub(crate) fn recorded(self, raw: i32) -> f64 {
        let normalized = f32::from_bits(raw as u32);
        if (raw == 0 || normalized.is_normal()) && (0.0..=1.0).contains(&normalized) {
            self.value(
                self.fl_range.0 + f64::from(normalized) * (self.fl_range.1 - self.fl_range.0),
            )
        } else {
            self.value(f64::from(raw))
        }
    }
    pub(crate) fn value(self, raw: f64) -> f64 {
        if let Some((min, max)) = self.destination_range {
            let n = ((raw - self.fl_range.0) / (self.fl_range.1 - self.fl_range.0)).clamp(0.0, 1.0);
            min + n * (max - min)
        } else {
            (self.convert)(raw)
        }
    }
}

/// An FL Studio effect as a Windfall effect.
#[derive(Debug, Clone)]
pub(crate) struct Translated {
    pub(crate) params: EffectParams,
    /// What did not carry over, in words that end a sentence about the
    /// effect. Empty when every setting has an exact place.
    pub(crate) notes: Vec<&'static str>,
    pub(crate) links: Vec<ParamLink>,
}

/// Translates the effect FL Studio calls `internal_name`, or `None` when
/// Windfall has nothing that stands for it.
pub(crate) fn translate(internal_name: &str, state: &[u8], tempo_bpm: f64) -> Option<Translated> {
    if let Some(translated) = super::native::effect(internal_name, state, tempo_bpm) {
        return Some(translated);
    }
    let numbers = Numbers(state);
    let name = internal_name.trim().to_ascii_lowercase();
    Some(match name.as_str() {
        "fruity parametric eq 2" => equaliser(numbers, true),
        "fruity parametric eq" => equaliser(numbers, false),
        "fruity compressor" => compressor(numbers),
        "fruity balance" => balance(numbers),
        "fruity limiter" => untranslated(
            EffectParams::Limiter(LimiterParams::default()),
            "its settings are not understood, so it starts from Windfall's defaults",
        ),
        "fruity reeverb 2" => reverb_2(numbers),
        "fruity reeverb" => reverb_1(numbers),
        "fruity delay 2" => delay_2(numbers, tempo_bpm),
        "fruity delay" => delay_1(numbers, tempo_bpm),
        "fruity delay 3" => untranslated(
            EffectParams::Delay(DelayParams::default()),
            "its settings are not understood, so it starts from Windfall's defaults",
        ),
        _ => {
            let (_, kind) = DEFAULT_EFFECTS
                .iter()
                .find(|(source, _)| source.eq_ignore_ascii_case(&name))?;
            untranslated(kind.default_params(), SETTINGS_NOT_DECODED)
        }
    })
}

const SETTINGS_NOT_DECODED: &str =
    "its settings were not decoded, so it starts from Windfall's defaults";

/// Import keys only: these names do not define Windfall processor names.
/// Keep mappings limited to kinds registered in `windfall_dsp::EffectKind`.
pub(super) const DEFAULT_EFFECTS: &[(&str, EffectKind)] = &[
    ("Fruity Center", EffectKind::DcBlock),
    ("Fruity Mute 2", EffectKind::ChannelMute),
    ("Fruity Phase Inverter", EffectKind::Polarity),
    ("Fruity Stereo Shaper", EffectKind::StereoMatrix),
    ("Fruity Soft Clipper", EffectKind::SoftClipper),
    ("Fruity Fast Dist", EffectKind::Distortion),
    ("Fruity Fast LP", EffectKind::FastLowpass),
    ("Fruity Free Filter", EffectKind::SelectableFilter),
    ("Fruity Bass Boost", EffectKind::BassShelf),
    ("Fruity Squeeze", EffectKind::Lofi),
    ("Fruity Chorus", EffectKind::Chorus),
    ("Fruity Flanger", EffectKind::Flanger),
    ("Fruity Phaser", EffectKind::Phaser),
    ("Frequency Splitter", EffectKind::BandSplit),
    (
        "Fruity Multiband Compressor",
        EffectKind::MultibandCompressor,
    ),
    ("Maximus", EffectKind::MultibandMaximizer),
    ("Transient Processor", EffectKind::TransientShaper),
    ("Transmitter", EffectKind::TransientSplit),
    ("Soundgoodizer", EffectKind::OneKnob),
    ("Low Lifter", EffectKind::BassHarmonics),
    ("Fruity Waveshaper", EffectKind::Waveshaper),
    ("Fruity Blood Overdrive", EffectKind::Overdrive),
    ("Hardcore", EffectKind::GuitarRack),
    ("Distructor", EffectKind::DriveChain),
    ("Vintage Chorus", EffectKind::VintageChorus),
    ("Hyper Chorus", EffectKind::HyperChorus),
    ("Vintage Phaser", EffectKind::VintagePhaser),
    ("Fruity Flangus", EffectKind::StackedFlanger),
    ("Multiband Delay", EffectKind::BandDelay),
    ("Spreader", EffectKind::Spreader),
    ("Fruity Stereo Enhancer", EffectKind::StereoEnhancer),
    ("Gross Beat", EffectKind::VolumeGate),
    ("Transporter", EffectKind::TimeTransport),
    ("Fruity Scratcher", EffectKind::Scratch),
    ("Effector", EffectKind::PerformanceRack),
    ("Fruity Convolver", EffectKind::Convolver),
    ("Frequency Shifter", EffectKind::FrequencyShifter),
    ("Pitch Shifter", EffectKind::PitchShift),
    ("Pitcher", EffectKind::PitchCorrect),
    ("Fruity Vocoder", EffectKind::Vocoder),
    ("Vocodex", EffectKind::Vocoder),
    ("Fruity Delay Bank", EffectKind::EchoBank),
    ("Fruity 7 Band EQ", EffectKind::SevenBand),
    ("EQUO", EffectKind::MorphEq),
    ("Fruity Love Philter", EffectKind::FilterBank),
    ("LuxeVerb", EffectKind::LushSpace),
    ("Emphasis", EffectKind::MasterLimiter),
    ("Emphasizer", EffectKind::StageStack),
    ("Tuner", EffectKind::Tuner),
    ("Control Surface", EffectKind::ControlSurface),
    ("Fruity PanOMatic", EffectKind::PanLfo),
    ("Fruity Peak Controller", EffectKind::EnvelopeFollower),
    ("Fruity X-Y Controller", EffectKind::XyPad),
    ("Fruity X-Y-Z Controller", EffectKind::XyzPad),
    ("Fruity Send", EffectKind::SendTap),
];

fn untranslated(params: EffectParams, note: &'static str) -> Translated {
    Translated {
        params: params.sanitized(),
        notes: vec![note],
        links: Vec::new(),
    }
}

/// PyFLP FruityBalanceEvent; DawVert fl_studio.ddef/dset and
/// flstudio_univ.pltr explicitly divide pan by 128 and volume by 256.
fn balance(numbers: Numbers<'_>) -> Translated {
    let mut params = windfall_dsp::BalanceParams::default();
    if let Some(pan) = numbers.get(0) {
        params.pan = pan as f32 / 128.0;
    }
    if let Some(gain) = numbers.get(1) {
        params.gain = gain as f32 / 256.0;
    }
    Translated {
        params: EffectParams::Balance(params.sanitized()),
        notes: vec![
            "pan and gain translated from the public two-word state; gain uses DawVert's /256 approximation (PyFLP describes logarithmic gain)",
        ],
        links: vec![
            ParamLink {
                fl_param: 0,
                fl_range: (-128.0, 128.0),
                id: "pan",
                convert: |v| v / 128.0,
                destination_range: None,
            },
            ParamLink {
                fl_param: 1,
                fl_range: (0.0, 320.0),
                id: "gain",
                convert: |v| v / 256.0,
                destination_range: None,
            },
        ],
    }
}

/// A frequency of FL Studio's newer equaliser: 20 Hz to 20 kHz over 0 to
/// 65536, in equal ratios.
fn eq_2_hz(raw: f64) -> f64 {
    20.0 * 1000_f64.powf((raw / 65_536.0).clamp(0.0, 1.0))
}

/// A frequency of FL Studio's older equaliser: 10 Hz to 16 kHz.
fn eq_1_hz(raw: f64) -> f64 {
    10.0 * 1600_f64.powf((raw / 65_536.0).clamp(0.0, 1.0).powf(0.6))
}

fn hundredths(raw: f64) -> f64 {
    raw / 100.0
}

/// The kinds of band FL Studio's equalisers have.
const LOW_PASS: i32 = 1;
const HIGH_PASS: i32 = 3;
const LOW_SHELF: i32 = 5;
const PEAK: i32 = 6;
const HIGH_SHELF: i32 = 7;

/// Both of FL Studio's seven-band equalisers. Each band can be any kind;
/// Windfall's has one low cut, one low shelf, three bells, one high shelf
/// and one high cut. Bands go to the place of their kind, in order, and
/// what does not fit is left out.
fn equaliser(numbers: Numbers<'_>, newer: bool) -> Translated {
    // The newer one has a version number in front, and a row of slopes
    // after the kinds.
    let first = usize::from(newer);
    let field = |row: usize, band: usize| numbers.get(first + row * 7 + band);
    let hz = if newer { eq_2_hz } else { eq_1_hz };
    let mut params = EqParams::default();
    let off = EqBand {
        enabled: false,
        ..EqBand::default()
    };
    params.low_shelf = EqBand {
        enabled: false,
        ..params.low_shelf
    };
    params.high_shelf = EqBand {
        enabled: false,
        ..params.high_shelf
    };
    params.peak1 = EqBand {
        frequency_hz: params.peak1.frequency_hz,
        ..off
    };
    params.peak2 = EqBand {
        frequency_hz: params.peak2.frequency_hz,
        ..off
    };
    params.peak3 = EqBand {
        frequency_hz: params.peak3.frequency_hz,
        ..off
    };

    let mut links = Vec::new();
    let mut peaks = 0_usize;
    let mut left_out = false;
    let mut cuts = false;
    for band in 0..7 {
        let kind = field(3, band).unwrap_or(0);
        if kind == 0 {
            continue;
        }
        let frequency_hz = field(1, band).map_or(1_000.0, |raw| hz(f64::from(raw))) as f32;
        let gain_db = field(0, band).map_or(0.0, |raw| f64::from(raw) / 100.0) as f32;
        let width = field(2, band).map_or(0.5, |raw| f64::from(raw) / 65_536.0);
        let mut link = |id_gain: &'static str, id_frequency: &'static str| {
            if newer {
                links.push(ParamLink {
                    fl_param: band as u16,
                    fl_range: (-1_800.0, 1_800.0),
                    id: id_gain,
                    convert: hundredths,
                    destination_range: None,
                });
                links.push(ParamLink {
                    fl_param: 7 + band as u16,
                    fl_range: (0.0, 65_536.0),
                    id: id_frequency,
                    convert: eq_2_hz,
                    destination_range: None,
                });
            }
        };
        let bell = EqBand {
            enabled: true,
            frequency_hz,
            gain_db,
            // DawVert's reading of the width knob for bells.
            q: (1.0 / ((width + 0.01) * 3.0)) as f32,
        };
        let shelf = EqBand {
            q: ((1.0 - width) * 1.2) as f32,
            ..bell
        };
        let cut = EqCutBand {
            enabled: true,
            frequency_hz,
            q: 2_f64.powf((1.0 - 2.0 * width) * 5.0) as f32,
            ..EqCutBand::default()
        };
        match kind {
            LOW_SHELF if !params.low_shelf.enabled => {
                params.low_shelf = shelf;
                link("lowShelf.gainDb", "lowShelf.frequencyHz");
            }
            HIGH_SHELF if !params.high_shelf.enabled => {
                params.high_shelf = shelf;
                link("highShelf.gainDb", "highShelf.frequencyHz");
            }
            PEAK if peaks < 3 => {
                let (place, gain, frequency) = match peaks {
                    0 => (&mut params.peak1, "peak1.gainDb", "peak1.frequencyHz"),
                    1 => (&mut params.peak2, "peak2.gainDb", "peak2.frequencyHz"),
                    _ => (&mut params.peak3, "peak3.gainDb", "peak3.frequencyHz"),
                };
                *place = bell;
                peaks += 1;
                link(gain, frequency);
            }
            HIGH_PASS if !params.low_cut.enabled => {
                params.low_cut = cut;
                cuts = true;
            }
            LOW_PASS if !params.high_cut.enabled => {
                params.high_cut = cut;
                cuts = true;
            }
            _ => left_out = true,
        }
    }
    // The newer one has its level after the slopes, the older one right
    // after the kinds, where it is a plain volume and is left alone.
    if newer && let Some(level) = numbers.get(first + 5 * 7) {
        params.output_gain_db = (f64::from(level) / 100.0) as f32;
    }

    let mut notes = vec!["the width of each band was converted by a rule of thumb"];
    if left_out {
        notes.push("bands of a kind or number Windfall's equaliser has no place for were left out");
    }
    if cuts {
        notes.push("the slopes of its cut bands were not carried over");
    }
    Translated {
        params: EffectParams::Eq(params.sanitized()),
        notes,
        links,
    }
}

fn tenths(raw: f64) -> f64 {
    raw / 10.0
}

fn compressor(numbers: Numbers<'_>) -> Translated {
    let mut params = CompressorParams::default();
    let mut notes = Vec::new();
    let value = |index: usize| numbers.get(index).map(f64::from);
    if let Some(threshold) = value(1) {
        params.threshold_db = (threshold / 10.0) as f32;
    }
    if let Some(ratio) = value(2) {
        params.ratio = (ratio / 10.0).max(1.0) as f32;
    }
    if let Some(gain) = value(3) {
        params.makeup_db = (gain / 10.0) as f32;
        if gain > 240.0 {
            notes.push("its gain is above Windfall's 24 dB and was brought down to it");
        }
    }
    if let Some(attack) = value(4) {
        // Ten-thousandths of a second.
        params.attack_ms = (attack / 10.0) as f32;
        if attack > 2_500.0 {
            notes.push("its attack is longer than Windfall's 250 ms and was brought down to it");
        }
    }
    if let Some(release) = value(5) {
        params.release_ms = release as f32;
        if release > 2_500.0 {
            notes.push("its release is longer than Windfall's 2.5 s and was brought down to it");
        }
    }
    if let Some(kind) = numbers.get(6) {
        // The low two bits pick one of four knees.
        params.knee_db = [0.0, 6.0, 7.0, 15.0][(kind & 3) as usize];
        if kind >> 2 != 0 {
            notes.push("its second response curve has no equivalent");
        }
    }
    Translated {
        params: EffectParams::Compressor(params.sanitized()),
        notes,
        links: vec![
            ParamLink {
                fl_param: 0,
                fl_range: (-600.0, 0.0),
                id: "thresholdDb",
                convert: tenths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 1,
                fl_range: (0.0, 300.0),
                id: "ratio",
                convert: tenths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 2,
                fl_range: (0.0, 300.0),
                id: "makeupDb",
                convert: tenths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 3,
                fl_range: (0.0, 4_000.0),
                id: "attackMs",
                convert: tenths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 4,
                fl_range: (0.0, 4_000.0),
                id: "releaseMs",
                convert: |raw| raw,
                destination_range: None,
            },
        ],
    }
}

/// How much of the output is the effect, from a dry level and a wet level.
fn mix_of(dry: f64, wet: f64) -> f32 {
    let (dry, wet) = (dry.max(0.0), wet.max(0.0));
    if dry + wet <= 0.0 {
        0.0
    } else {
        (wet / (dry + wet)) as f32
    }
}

/// A damping frequency as Windfall's amount of damping: none at the top
/// of the range, all of it at the bottom. A rule of thumb.
fn damping_of(hz: f64) -> f32 {
    (1.0 - (hz - 500.0) / (22_050.0 - 500.0)).clamp(0.0, 1.0) as f32
}

fn times_hundred(raw: f64) -> f64 {
    raw * 100.0
}

fn over_128(raw: f64) -> f64 {
    raw / 128.0
}

fn reverb_2(numbers: Numbers<'_>) -> Translated {
    let mut params = ReverbParams::default();
    let value = |index: usize| numbers.get(index).map(f64::from);
    if let Some(low_cut) = value(1) {
        params.low_cut_hz = low_cut as f32;
    }
    if let Some(high_cut) = value(2) {
        params.high_cut_hz = (high_cut * 100.0) as f32;
    }
    if let Some(size) = value(4) {
        params.size = (size / 100.0) as f32;
    }
    if let Some(diffusion) = value(5) {
        params.diffusion = (diffusion / 100.0) as f32;
    }
    if let Some(decay) = value(6) {
        params.decay_s = (decay / 10.0) as f32;
    }
    if let Some(damping) = value(7) {
        params.damping = damping_of(damping * 100.0);
    }
    if let Some(early) = value(12) {
        params.early_level = (early / 128.0) as f32;
    }
    if let (Some(dry), Some(wet)) = (value(11), value(13)) {
        params.mix = mix_of(dry / 128.0, wet / 128.0);
    }
    Translated {
        params: EffectParams::Reverb(params.sanitized()),
        notes: vec![
            "it is another reverb, so it will not sound the same: size, decay, diffusion, cuts and mix were carried over, and its pre-delay, bass and modulation were not",
        ],
        links: vec![
            ParamLink {
                fl_param: 0,
                fl_range: (0.0, 3_000.0),
                id: "lowCutHz",
                convert: |raw| raw,
                destination_range: None,
            },
            ParamLink {
                fl_param: 1,
                fl_range: (0.0, 221.0),
                id: "highCutHz",
                convert: times_hundred,
                destination_range: None,
            },
            ParamLink {
                fl_param: 3,
                fl_range: (0.0, 100.0),
                id: "size",
                convert: hundredths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 4,
                fl_range: (0.0, 100.0),
                id: "diffusion",
                convert: hundredths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 5,
                fl_range: (0.0, 200.0),
                id: "decayS",
                convert: tenths,
                destination_range: None,
            },
            ParamLink {
                fl_param: 11,
                fl_range: (0.0, 160.0),
                id: "earlyLevel",
                convert: over_128,
                destination_range: None,
            },
        ],
    }
}

fn reverb_1(numbers: Numbers<'_>) -> Translated {
    let mut params = ReverbParams::default();
    // Every knob of the older reverb runs from 0 to 65536.
    let knob = |index: usize| numbers.get(index).map(|raw| f64::from(raw) / 65_536.0);
    let between = |low: f64, high: f64, knob: f64| low + (high - low) * knob.clamp(0.0, 1.0);
    if let Some(low_cut) = knob(1) {
        params.low_cut_hz = between(20.0, 3_000.0, low_cut) as f32;
    }
    if let Some(high_cut) = knob(2) {
        params.high_cut_hz = between(500.0, 22_050.0, high_cut) as f32;
    }
    if let Some(size) = knob(4) {
        params.size = size as f32;
    }
    if let Some(diffusion) = knob(5) {
        params.diffusion = diffusion as f32;
    }
    if let Some(decay) = knob(7) {
        params.decay_s = between(0.1, 20.0, decay) as f32;
    }
    if let Some(damping) = knob(8) {
        params.damping = damping_of(between(500.0, 22_050.0, damping));
    }
    if let (Some(dry), Some(wet)) = (knob(9), knob(10)) {
        params.mix = mix_of(dry, wet);
    }
    Translated {
        params: EffectParams::Reverb(params.sanitized()),
        notes: vec![
            "it is another reverb, so it will not sound the same: size, decay, diffusion, cuts and mix were carried over, and its pre-delay and colour were not",
        ],
        links: Vec::new(),
    }
}

/// Sets a delay's time from a length in sixteenth-note steps: as a note
/// length that follows the tempo when Windfall has that length, and as
/// milliseconds at the project's tempo otherwise. Returns false for the
/// second.
fn set_delay_steps(params: &mut DelayParams, steps: f64, tempo_bpm: f64) -> bool {
    const DIVISIONS: [NoteDivision; 14] = [
        NoteDivision::Whole,
        NoteDivision::HalfDotted,
        NoteDivision::Half,
        NoteDivision::HalfTriplet,
        NoteDivision::QuarterDotted,
        NoteDivision::Quarter,
        NoteDivision::QuarterTriplet,
        NoteDivision::EighthDotted,
        NoteDivision::Eighth,
        NoteDivision::EighthTriplet,
        NoteDivision::SixteenthDotted,
        NoteDivision::Sixteenth,
        NoteDivision::SixteenthTriplet,
        NoteDivision::ThirtySecond,
    ];
    let beats = steps / 4.0;
    let division = DIVISIONS
        .into_iter()
        .find(|division| (f64::from(division.beats()) - beats).abs() < 1e-3);
    match division {
        Some(division) => {
            params.sync = true;
            params.division = division;
            true
        }
        None => {
            params.sync = false;
            params.time_ms = (beats * 60_000.0 / tempo_bpm.max(1.0)) as f32;
            false
        }
    }
}

const FREE_TIME: &str = "its time is not a note length Windfall has, so it was set in milliseconds at the project's tempo and no longer follows the tempo";
const INVERTED: &str = "its inverted feedback has no equivalent";

fn squared_over_128(raw: f64) -> f64 {
    (raw / 128.0).powi(2)
}

fn delay_2(numbers: Numbers<'_>, tempo_bpm: f64) -> Translated {
    let mut params = DelayParams::default();
    let mut notes = vec!["its input pan, stereo offset and feedback cut were not carried over"];
    let value = |index: usize| numbers.get(index).map(f64::from);
    if let (Some(input), Some(dry)) = (value(1), value(2)) {
        params.mix = mix_of(dry / 128.0, input / 160.0);
    }
    if let Some(feedback) = value(3) {
        params.feedback = squared_over_128(feedback) as f32;
    }
    // 48 to a step: ticks at 192 to the quarter note.
    if let Some(time) = value(4)
        && !set_delay_steps(&mut params, time / 48.0, tempo_bpm)
    {
        notes.push(FREE_TIME);
    }
    match numbers.get(6) {
        Some(2) => params.mode = DelayMode::PingPong,
        Some(1) => notes.push(INVERTED),
        _ => {}
    }
    Translated {
        params: EffectParams::Delay(params.sanitized()),
        notes,
        links: vec![ParamLink {
            fl_param: 3,
            fl_range: (0.0, 127.0),
            id: "feedback",
            convert: squared_over_128,
            destination_range: None,
        }],
    }
}

fn delay_1(numbers: Numbers<'_>, tempo_bpm: f64) -> Translated {
    let mut params = DelayParams::default();
    let mut notes = vec!["its cutoff and its own tempo were not carried over"];
    let knob = |index: usize| numbers.get(index).map(|raw| f64::from(raw) / 1_024.0);
    if let Some(input) = knob(1) {
        // The dry signal always passes at full level.
        params.mix = mix_of(1.0, input);
    }
    if let Some(feedback) = knob(2) {
        params.feedback = feedback.powi(2) as f32;
    }
    if let Some(steps) = numbers.get(5)
        && !set_delay_steps(&mut params, f64::from(steps), tempo_bpm)
    {
        notes.push(FREE_TIME);
    }
    match numbers.get(6) {
        Some(2) => params.mode = DelayMode::PingPong,
        // The older delay's second mode swaps the sides of every echo,
        // which is what the inverted mode here does.
        Some(1) => params.mode = DelayMode::Inverted,
        _ => {}
    }
    Translated {
        params: EffectParams::Delay(params.sanitized()),
        notes,
        links: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{ConvertOptions, PluginPlace, convert};
    use crate::model::{FlpProject, Insert, Plugin, Slot};
    use crate::report::{Outcome, ReportSection};

    fn state(values: &[i32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn eq(translated: &Translated) -> EqParams {
        match translated.params {
            EffectParams::Eq(params) => params,
            other => panic!("not an equaliser: {other:?}"),
        }
    }

    /// The state of the newer equaliser from each band's gain, frequency,
    /// width and kind, and the level.
    fn eq_2_state(bands: [(i32, i32, i32, i32); 7], level: i32) -> Vec<u8> {
        let mut values = vec![2];
        values.extend(bands.iter().map(|band| band.0));
        values.extend(bands.iter().map(|band| band.1));
        values.extend(bands.iter().map(|band| band.2));
        values.extend(bands.iter().map(|band| band.3));
        values.extend([0; 7]);
        values.push(level);
        state(&values)
    }

    #[test]
    fn every_name_in_the_table_gives_the_effect_the_table_says() {
        let table = [
            ("Fruity Parametric EQ 2", EffectKind::Eq),
            ("Fruity Parametric EQ", EffectKind::Eq),
            ("Fruity Compressor", EffectKind::Compressor),
            ("Fruity Limiter", EffectKind::Limiter),
            ("Fruity Reeverb 2", EffectKind::Reverb),
            ("Fruity Reeverb", EffectKind::Reverb),
            ("Fruity Delay 2", EffectKind::Delay),
            ("Fruity Delay", EffectKind::Delay),
            ("Fruity Delay 3", EffectKind::Delay),
            ("Fruity Balance", EffectKind::Balance),
            ("Fruity Center", EffectKind::DcBlock),
            ("Fruity Mute 2", EffectKind::ChannelMute),
            ("Fruity Phase Inverter", EffectKind::Polarity),
            ("Fruity Stereo Shaper", EffectKind::StereoMatrix),
            ("Fruity Soft Clipper", EffectKind::SoftClipper),
            ("Fruity Fast Dist", EffectKind::Distortion),
            ("Fruity Fast LP", EffectKind::FastLowpass),
            ("Fruity Free Filter", EffectKind::SelectableFilter),
            ("Fruity Bass Boost", EffectKind::BassShelf),
            ("Fruity Squeeze", EffectKind::Lofi),
            ("Fruity Chorus", EffectKind::Chorus),
            ("Fruity Flanger", EffectKind::Flanger),
            ("Fruity Phaser", EffectKind::Phaser),
            ("Frequency Splitter", EffectKind::BandSplit),
            (
                "Fruity Multiband Compressor",
                EffectKind::MultibandCompressor,
            ),
            ("Maximus", EffectKind::MultibandMaximizer),
            ("Transient Processor", EffectKind::TransientShaper),
            ("Transmitter", EffectKind::TransientSplit),
            ("Soundgoodizer", EffectKind::OneKnob),
            ("Low Lifter", EffectKind::BassHarmonics),
            ("Fruity Waveshaper", EffectKind::Waveshaper),
            ("Fruity Blood Overdrive", EffectKind::Overdrive),
            ("Hardcore", EffectKind::GuitarRack),
            ("Distructor", EffectKind::DriveChain),
            ("Vintage Chorus", EffectKind::VintageChorus),
            ("Hyper Chorus", EffectKind::HyperChorus),
            ("Vintage Phaser", EffectKind::VintagePhaser),
            ("Fruity Flangus", EffectKind::StackedFlanger),
            ("Multiband Delay", EffectKind::BandDelay),
            ("Spreader", EffectKind::Spreader),
            ("Fruity Stereo Enhancer", EffectKind::StereoEnhancer),
            ("Gross Beat", EffectKind::VolumeGate),
            ("Transporter", EffectKind::TimeTransport),
            ("Fruity Scratcher", EffectKind::Scratch),
            ("Effector", EffectKind::PerformanceRack),
            ("Fruity Convolver", EffectKind::Convolver),
            ("Frequency Shifter", EffectKind::FrequencyShifter),
            ("Pitch Shifter", EffectKind::PitchShift),
            ("Pitcher", EffectKind::PitchCorrect),
            ("Fruity Vocoder", EffectKind::Vocoder),
            ("Vocodex", EffectKind::Vocoder),
            ("Fruity Delay Bank", EffectKind::EchoBank),
            ("Fruity 7 Band EQ", EffectKind::SevenBand),
            ("EQUO", EffectKind::MorphEq),
            ("Fruity Love Philter", EffectKind::FilterBank),
            ("LuxeVerb", EffectKind::LushSpace),
            ("Emphasis", EffectKind::MasterLimiter),
            ("Emphasizer", EffectKind::StageStack),
            ("Tuner", EffectKind::Tuner),
            ("Control Surface", EffectKind::ControlSurface),
            ("Fruity PanOMatic", EffectKind::PanLfo),
            ("Fruity Peak Controller", EffectKind::EnvelopeFollower),
            ("Fruity X-Y Controller", EffectKind::XyPad),
            ("Fruity X-Y-Z Controller", EffectKind::XyzPad),
            ("Fruity Send", EffectKind::SendTap),
        ];
        for (name, kind) in table {
            // An empty state is what a damaged file gives: defaults.
            let translated = translate(name, &[], 120.0).expect(name);
            assert_eq!(translated.params.kind(), kind, "{name}");
            assert_eq!(translated.params, translated.params.sanitized(), "{name}");
        }
        assert_eq!(
            translate("  fruity COMPRESSOR ", &[], 120.0).map(|t| t.params.kind()),
            Some(EffectKind::Compressor)
        );
        assert!(translate("Some Other Effect", &[], 120.0).is_none());
        assert!(translate("", &[], 120.0).is_none());
    }

    fn project_with_effect(name: &str, state: Vec<u8>) -> FlpProject {
        let mut project = FlpProject::default();
        project.mixer.inserts.push(Insert {
            slots: vec![Slot {
                index: 3,
                plugin: Plugin {
                    internal_name: name.to_owned(),
                    state,
                    ..Plugin::default()
                },
                ..Slot::default()
            }],
            ..Insert::default()
        });
        project
    }

    #[test]
    fn undecoded_effects_use_defaults_without_parameter_links_and_report_the_loss() {
        let states = [
            Vec::new(),
            // Sixteen bytes are now the supported Fruity Send record, not
            // an undecoded fixture. Keep this hostile state outside that
            // layout; supported send mapping has separate conversion tests.
            state(&[i32::MIN, i32::MAX, -1, 42, 17]),
            vec![255; 3],
        ];
        for &(name, kind) in DEFAULT_EFFECTS {
            for state in &states {
                let translated = translate(name, state, 120.0).expect(name);
                assert_eq!(
                    translated.params,
                    kind.default_params().sanitized(),
                    "{name}"
                );
                assert_eq!(translated.notes, [SETTINGS_NOT_DECODED], "{name}");
                assert!(translated.links.is_empty(), "{name}");
            }
            let normalized = format!("  {}  ", name.to_ascii_uppercase());
            assert_eq!(
                translate(&normalized, &[], 120.0).unwrap().params.kind(),
                kind,
                "{name}"
            );

            let project = project_with_effect(name, states[1].clone());
            let imported = convert(&project, &ConvertOptions::default());
            let effects = &imported.project.mixer.tracks[0].effects;
            assert_eq!(effects.len(), 1, "{name}");
            assert_eq!(
                effects[0].params,
                kind.default_params().sanitized(),
                "{name}"
            );
            assert_eq!(
                imported.plugins.len(),
                1,
                "{name}: the undecoded original state is retained"
            );
            let report = imported.report.category(ReportSection::Effects);
            assert_eq!(report.approximated, 1, "{name}");
            assert!(
                report.lines.iter().any(|line| {
                    line.outcome == Outcome::Approximated
                        && line.text.contains(name)
                        && line.text.contains(SETTINGS_NOT_DECODED)
                }),
                "{name}"
            );
        }
    }

    #[test]
    fn unsupported_names_stay_out_of_the_chain_and_keep_their_state() {
        for name in [
            "Some Other Effect",
            "Fruity Filter",
            "Fruity Formula Controller",
            "Fruity LSD",
            "Patcher",
            "Fruity HTML NoteBook",
            "Fruity NoteBook",
            "Fruity NoteBook 2",
            "Razer Chroma",
            "VFX Color Mapper",
            "VFX Envelope",
            "VFX Level Scaler",
            "VFX Keyboard Splitter",
            "VFX Key Mapper",
            "VFX Sequencer",
            "VFX Script",
        ] {
            let state = vec![0, 255, 1, 128, 42];
            assert!(translate(name, &state, 120.0).is_none(), "{name}");
            let project = project_with_effect(name, state.clone());
            let imported = convert(&project, &ConvertOptions::default());
            assert!(
                imported.project.mixer.tracks[0].effects.is_empty(),
                "{name}"
            );
            assert_eq!(imported.plugins.len(), 1, "{name}");
            let kept = &imported.plugins[0];
            assert_eq!(kept.internal_name, name);
            assert_eq!(kept.state, state, "{name}");
            assert_eq!(
                kept.place,
                PluginPlace::Effect {
                    track: imported.project.mixer.tracks[0].id,
                    slot: 3,
                },
                "{name}"
            );
            let report = imported.report.category(ReportSection::Effects);
            assert_eq!(report.placeholders, 1, "{name}");
            assert!(
                report.lines.iter().any(|line| {
                    line.outcome == Outcome::Placeholder
                        && line.text.contains("its settings were kept")
                }),
                "{name}"
            );
        }
    }

    #[test]
    fn every_linked_setting_exists_in_the_effect_it_is_for() {
        for name in [
            "Fruity Parametric EQ 2",
            "Fruity Compressor",
            "Fruity Reeverb 2",
            "Fruity Delay 2",
        ] {
            let bands = [(0, 30_000, 20_000, PEAK); 7];
            let state = if name.contains("EQ") {
                eq_2_state(bands, 0)
            } else {
                Vec::new()
            };
            let translated = translate(name, &state, 120.0).expect(name);
            let descriptors = translated.params.kind().descriptors();
            assert!(!translated.links.is_empty(), "{name}");
            for link in &translated.links {
                assert!(
                    descriptors.iter().any(|info| info.id == link.id),
                    "{name} has no setting {}",
                    link.id
                );
            }
        }
    }

    #[test]
    fn equaliser_bands_go_to_the_place_of_their_kind() {
        let off = (0, 0, 0, 0);
        let translated = translate(
            "Fruity Parametric EQ 2",
            &eq_2_state(
                [
                    (0, 0, 32_768, HIGH_PASS),
                    (300, 21_845, 32_768, LOW_SHELF),
                    (-600, 32_768, 0, PEAK),
                    off,
                    (1_200, 43_691, 65_536, PEAK),
                    (-150, 54_613, 32_768, HIGH_SHELF),
                    (0, 65_536, 32_768, LOW_PASS),
                ],
                250,
            ),
            120.0,
        )
        .expect("an equaliser");
        let params = eq(&translated);
        assert!(params.low_cut.enabled);
        assert!((params.low_cut.frequency_hz - 20.0).abs() < 1e-3);
        assert!(params.low_shelf.enabled);
        assert_eq!(params.low_shelf.gain_db, 3.0);
        assert!((params.low_shelf.frequency_hz - 200.0).abs() < 0.1);
        assert!(params.peak1.enabled && params.peak2.enabled && !params.peak3.enabled);
        assert_eq!(params.peak1.gain_db, -6.0);
        assert!((params.peak1.frequency_hz - 632.46).abs() < 0.1);
        assert_eq!(params.peak2.gain_db, 12.0);
        // A narrow width is a high Q and a wide one a low Q, inside the
        // range of Windfall's bells.
        assert!(params.peak1.q > params.peak2.q);
        assert!(params.peak1.q <= 18.0 && params.peak2.q >= 0.1);
        assert!(params.high_shelf.enabled);
        assert_eq!(params.high_shelf.gain_db, -1.5);
        assert!(params.high_cut.enabled);
        assert!((params.high_cut.frequency_hz - 20_000.0).abs() < 1.0);
        assert_eq!(params.output_gain_db, 2.5);
        assert!(
            !translated
                .notes
                .iter()
                .any(|note| note.contains("left out"))
        );
    }

    #[test]
    fn bands_the_equaliser_has_no_place_for_are_left_out_and_said() {
        const NOTCH: i32 = 4;
        let peak = (100, 30_000, 20_000, PEAK);
        let translated = translate(
            "Fruity Parametric EQ 2",
            &eq_2_state(
                [
                    peak,
                    peak,
                    peak,
                    peak,
                    (0, 100, 100, NOTCH),
                    (0, 0, 0, 0),
                    (0, 0, 0, 0),
                ],
                0,
            ),
            120.0,
        )
        .expect("an equaliser");
        let params = eq(&translated);
        assert!(params.peak1.enabled && params.peak2.enabled && params.peak3.enabled);
        assert!(!params.low_shelf.enabled && !params.high_shelf.enabled);
        assert!(!params.low_cut.enabled && !params.high_cut.enabled);
        assert!(
            translated
                .notes
                .iter()
                .any(|note| note.contains("left out"))
        );
        // Three bells, two links each.
        assert_eq!(translated.links.len(), 6);
    }

    #[test]
    fn an_equaliser_with_every_band_off_changes_nothing() {
        let translated = translate(
            "Fruity Parametric EQ 2",
            &eq_2_state([(0, 0, 0, 0); 7], 0),
            120.0,
        )
        .expect("an equaliser");
        let params = eq(&translated);
        let bands = [
            params.low_shelf,
            params.peak1,
            params.peak2,
            params.peak3,
            params.high_shelf,
        ];
        assert!(bands.iter().all(|band| !band.enabled));
        assert!(!params.low_cut.enabled && !params.high_cut.enabled);
        assert_eq!(params.output_gain_db, 0.0);
    }

    #[test]
    fn the_older_equaliser_has_no_version_in_front_and_its_own_scale() {
        let mut values = Vec::new();
        values.extend([500, 0, 0, 0, 0, 0, 0]);
        values.extend([65_536, 0, 0, 0, 0, 0, 0]);
        values.extend([32_768, 0, 0, 0, 0, 0, 0]);
        values.extend([PEAK, 0, 0, 0, 0, 0, 0]);
        values.push(256);
        let translated =
            translate("Fruity Parametric EQ", &state(&values), 120.0).expect("an equaliser");
        let params = eq(&translated);
        assert_eq!(params.peak1.gain_db, 5.0);
        assert!((params.peak1.frequency_hz - 16_000.0).abs() < 1.0);
        assert_eq!(params.output_gain_db, 0.0);
        assert!(translated.links.is_empty());
    }

    #[test]
    fn a_compressor_carries_its_five_knobs_and_its_knee() {
        let translated = translate(
            "Fruity Compressor",
            &state(&[1, -240, 40, 60, 100, 250, 1]),
            120.0,
        )
        .expect("a compressor");
        let EffectParams::Compressor(params) = translated.params else {
            panic!("not a compressor");
        };
        assert_eq!(params.threshold_db, -24.0);
        assert_eq!(params.ratio, 4.0);
        assert_eq!(params.makeup_db, 6.0);
        assert_eq!(params.attack_ms, 10.0);
        assert_eq!(params.release_ms, 250.0);
        assert_eq!(params.knee_db, 6.0);
        assert!(translated.notes.is_empty());
    }

    #[test]
    fn a_compressor_setting_past_windfalls_range_is_brought_in_and_said() {
        let translated = translate(
            "Fruity Compressor",
            &state(&[1, -700, 0, 300, 4_000, 4_000, 7]),
            120.0,
        )
        .expect("a compressor");
        let EffectParams::Compressor(params) = translated.params else {
            panic!("not a compressor");
        };
        assert_eq!(params.threshold_db, -60.0);
        assert_eq!(params.ratio, 1.0);
        assert_eq!(params.makeup_db, 24.0);
        assert_eq!(params.attack_ms, 250.0);
        assert_eq!(params.release_ms, 2_500.0);
        assert_eq!(params.knee_db, 15.0);
        assert_eq!(translated.notes.len(), 4);
    }

    #[test]
    fn the_newer_reverb_carries_size_decay_cuts_and_mix() {
        // The values of a reverb in a project saved by FL Studio 11, which
        // has no modulation fields yet.
        let translated = translate(
            "Fruity Reeverb 2",
            &state(&[
                10_000, 485, 40, 0, 69, 100, 18, 40, 100, 500, -9, 128, 64, 41,
            ]),
            120.0,
        )
        .expect("a reverb");
        let EffectParams::Reverb(params) = translated.params else {
            panic!("not a reverb");
        };
        assert_eq!(params.low_cut_hz, 485.0);
        assert_eq!(params.high_cut_hz, 4_000.0);
        assert_eq!(params.size, 0.69);
        assert_eq!(params.diffusion, 1.0);
        assert_eq!(params.decay_s, 1.8);
        assert_eq!(params.early_level, 0.5);
        assert!((params.mix - 41.0 / 169.0).abs() < 1e-6);
        assert!(params.damping > 0.8);
    }

    #[test]
    fn the_older_reverb_spreads_its_knobs_over_their_ranges() {
        let half = 32_768;
        let translated = translate(
            "Fruity Reeverb",
            &state(&[1, 0, 65_536, 0, half, half, 0, half, 65_536, 65_536, 65_536]),
            120.0,
        )
        .expect("a reverb");
        let EffectParams::Reverb(params) = translated.params else {
            panic!("not a reverb");
        };
        assert_eq!(params.low_cut_hz, 20.0);
        assert_eq!(params.high_cut_hz, 20_000.0);
        assert_eq!(params.size, 0.5);
        assert!((params.decay_s - 10.05).abs() < 1e-3);
        assert_eq!(params.damping, 0.0);
        assert_eq!(params.mix, 0.5);
    }

    #[test]
    fn a_delay_on_a_note_length_follows_the_tempo() {
        // Three steps is a dotted eighth: 144 at 48 to a step.
        let translated = translate(
            "Fruity Delay 2",
            &state(&[0, 160, 128, 64, 144, 0, 2, 0]),
            120.0,
        )
        .expect("a delay");
        let EffectParams::Delay(params) = translated.params else {
            panic!("not a delay");
        };
        assert!(params.sync);
        assert_eq!(params.division, NoteDivision::EighthDotted);
        assert_eq!(params.feedback, 0.25);
        assert_eq!(params.mode, DelayMode::PingPong);
        assert_eq!(params.mix, 0.5);
        assert!(!translated.notes.contains(&FREE_TIME));
    }

    #[test]
    fn a_delay_between_note_lengths_is_set_in_milliseconds_and_said() {
        // Five steps at 100 bpm: 1.25 beats of 600 ms.
        let translated = translate(
            "Fruity Delay 2",
            &state(&[0, 160, 128, 64, 240, 0, 1, 0]),
            100.0,
        )
        .expect("a delay");
        let EffectParams::Delay(params) = translated.params else {
            panic!("not a delay");
        };
        assert!(!params.sync);
        assert_eq!(params.time_ms, 750.0);
        assert_eq!(params.mode, DelayMode::Stereo);
        assert!(translated.notes.contains(&FREE_TIME));
        assert!(translated.notes.contains(&INVERTED));
    }

    #[test]
    fn the_older_delay_keeps_its_inverted_stereo_mode() {
        let translated = translate("Fruity Delay", &state(&[1, 1_024, 512, 0, 0, 4, 1]), 120.0)
            .expect("a delay");
        let EffectParams::Delay(params) = translated.params else {
            panic!("not a delay");
        };
        assert_eq!(params.mode, DelayMode::Inverted);
        assert!(!translated.notes.contains(&INVERTED));
    }

    #[test]
    fn the_older_delay_counts_its_time_in_steps() {
        let translated = translate("Fruity Delay", &state(&[1, 1_024, 512, 0, 0, 4, 2]), 120.0)
            .expect("a delay");
        let EffectParams::Delay(params) = translated.params else {
            panic!("not a delay");
        };
        assert!(params.sync);
        assert_eq!(params.division, NoteDivision::Quarter);
        assert_eq!(params.feedback, 0.25);
        assert_eq!(params.mode, DelayMode::PingPong);
        assert_eq!(params.mix, 0.5);
    }

    #[test]
    fn automation_values_convert_through_the_same_rule_as_the_setting() {
        let translated = translate("Fruity Compressor", &[], 120.0).expect("a compressor");
        let threshold = translated.links[0];
        assert_eq!(threshold.id, "thresholdDb");
        let at = |value: f64| {
            threshold
                .value(threshold.fl_range.0 + value * (threshold.fl_range.1 - threshold.fl_range.0))
        };
        assert_eq!(at(0.0), -60.0);
        assert_eq!(at(0.5), -30.0);
        assert_eq!(at(1.0), 0.0);
    }
}
