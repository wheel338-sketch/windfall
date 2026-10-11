//! A rack of eleven guitar pedals that can all be on at once, ending in a
//! speaker cabinet played as an impulse response.
//!
//! The signal runs through the pedals in a fixed order, the order a
//! pedalboard is usually wired in:
//!
//! gate, compressor, wah, treble booster, boost, then the three drive
//! pedals (overdrive, fuzz, octave) above the host rate, then mid scoop,
//! spring and cabinet.
//!
//! Each pedal has a switch and two knobs, `amount` and `tone`. Switching a
//! pedal crossfades it in or out over 5 ms. Every pedal's memory keeps
//! running while it is off, so switching one on never starts from a click.

use super::cabinet::{Cabinet, CabinetBank, CabinetChannel};
use super::oversample::{DRIVE_LATENCY_SAMPLES, Kernels, Oversampler, Oversampling};
use crate::balance::{Controls, audio, rate};
use crate::blocks::CONTROL_PERIOD;
use crate::blocks::dc::DcBlocker;
use crate::blocks::delay_line::DelayLine;
use crate::blocks::envelope::Ballistics;
use crate::blocks::lfo::{Lfo, LfoShape};
use crate::blocks::math::{db_to_gain, ms_to_samples};
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::{OnePoleFilter, Svf, SvfCoeffs};
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The eleven pedals, in the order their controls are numbered. This is
/// not the order the signal passes them in; see the module comment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GuitarModel {
    #[default]
    CleanBoost,
    TubeSoftClip,
    HardFuzz,
    OctaveUp,
    TrebleBooster,
    MidScoop,
    Wah,
    CompressorSquash,
    Gate,
    SpringSlap,
    Cabinet,
}
impl GuitarModel {
    pub const ALL: [Self; 11] = [
        Self::CleanBoost,
        Self::TubeSoftClip,
        Self::HardFuzz,
        Self::OctaveUp,
        Self::TrebleBooster,
        Self::MidScoop,
        Self::Wah,
        Self::CompressorSquash,
        Self::Gate,
        Self::SpringSlap,
        Self::Cabinet,
    ];

    /// The order the signal passes the pedals in.
    pub const SIGNAL_ORDER: [Self; 11] = [
        Self::Gate,
        Self::CompressorSquash,
        Self::Wah,
        Self::TrebleBooster,
        Self::CleanBoost,
        Self::TubeSoftClip,
        Self::HardFuzz,
        Self::OctaveUp,
        Self::MidScoop,
        Self::SpringSlap,
        Self::Cabinet,
    ];
}

/// One pedal's switch and knobs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PedalParams {
    /// Whether the pedal is in the signal; default off.
    pub on: bool,
    /// How much the pedal does, 0..1. See [`GuitarRackParams::pedals`].
    pub amount: f32,
    /// The pedal's character, 0..1. See [`GuitarRackParams::pedals`].
    pub tone: f32,
}
impl Default for PedalParams {
    fn default() -> Self {
        Self {
            on: false,
            amount: 0.5,
            tone: 0.5,
        }
    }
}

/// Default (amount, tone) of each pedal, indexed like [`GuitarModel::ALL`].
const PEDAL_DEFAULTS: [(f32, f32); 11] = [
    (0.5, 0.5),
    (0.5, 0.5),
    (0.5, 0.5),
    (1.0, 0.0),
    (0.5, 0.5),
    (0.5, 0.5),
    (0.5, 0.5),
    (0.5, 0.5),
    (0.5, 0.5),
    (0.5, 0.5),
    (1.0, 0.8),
];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GuitarRackParams {
    /// A pedal that is on whatever its own switch says. This is the one
    /// pedal a rack saved before the pedals had switches was set to. Clean
    /// boost, the default, forces nothing.
    pub model: GuitarModel,
    /// Input gain in dB, 0..36; default 6.
    pub drive_db: f32,
    /// Output trim in dB, -24..12; default -6.
    pub output_db: f32,
    /// The pedals, indexed like [`GuitarModel::ALL`]. What the knobs do:
    ///
    /// | Pedal | amount | tone |
    /// | --- | --- | --- |
    /// | Clean boost | gain, 0..18 dB | tilt, dark to bright around 1 kHz |
    /// | Tube soft clip | gain into the clipper, 0..30 dB | asymmetry of its two ceilings |
    /// | Hard fuzz | gain into the clip, 6..32 dB | clip ceiling, 0.35..0.95 |
    /// | Octave up | share of the rectified signal | full-wave (0) to half-wave (1) |
    /// | Treble booster | boost above the corner, up to 5 times | corner, 600..4800 Hz |
    /// | Mid scoop | depth of the cut | centre, 300..1500 Hz |
    /// | Wah | resonance, Q 1.5..5.5 | sweep rate, 0.2..6 Hz |
    /// | Compressor | lower threshold and more make-up | release, 30..300 ms |
    /// | Gate | threshold, -54..-18 dB | release, 5..200 ms |
    /// | Spring slap | level of the slap | damping corner, 1..8 kHz |
    /// | Cabinet | share of the cabinet's sound | microphone, dark (2 kHz) to open (16 kHz) |
    pub pedals: [PedalParams; 11],
    /// Which of Windfall's cabinets the cabinet pedal plays.
    pub cabinet: Cabinet,
    /// Rate the three drive pedals run at; default four times the host rate.
    pub oversampling: Oversampling,
}
impl Default for GuitarRackParams {
    fn default() -> Self {
        Self {
            model: GuitarModel::CleanBoost,
            drive_db: 6.0,
            output_db: -6.0,
            pedals: PEDAL_DEFAULTS.map(|(amount, tone)| PedalParams {
                on: false,
                amount,
                tone,
            }),
            cabinet: Cabinet::Combo,
            oversampling: Oversampling::X4,
        }
    }
}
param_set!(GuitarRackParams, "Guitar Rack", {
    choice [model] "model" "Pedal" { GuitarModel, CleanBoost, [
        CleanBoost "cleanBoost" "Clean boost",
        TubeSoftClip "tubeSoftClip" "Tube soft clip",
        HardFuzz "hardFuzz" "Hard fuzz",
        OctaveUp "octaveUp" "Octave up",
        TrebleBooster "trebleBooster" "Treble booster",
        MidScoop "midScoop" "Mid scoop",
        Wah "wah" "Wah",
        CompressorSquash "compressorSquash" "Compressor squash",
        Gate "gate" "Gate",
        SpringSlap "springSlap" "Spring slap",
        Cabinet "cabinet" "Cabinet"
    ] }
    float [drive_db] "driveDb" "Drive" { Decibels, Linear, 0.0, 36.0, 6.0 }
    float [output_db] "outputDb" "Output" { Decibels, Linear, -24.0, 12.0, -6.0 }
    toggle [pedals[0].on] "pedals.0.on" "Boost on" { false }
    float [pedals[0].amount] "pedals.0.amount" "Boost amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[0].tone] "pedals.0.tone" "Boost tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[1].on] "pedals.1.on" "Overdrive on" { false }
    float [pedals[1].amount] "pedals.1.amount" "Overdrive amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[1].tone] "pedals.1.tone" "Overdrive tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[2].on] "pedals.2.on" "Fuzz on" { false }
    float [pedals[2].amount] "pedals.2.amount" "Fuzz amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[2].tone] "pedals.2.tone" "Fuzz tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[3].on] "pedals.3.on" "Octave on" { false }
    float [pedals[3].amount] "pedals.3.amount" "Octave amount" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [pedals[3].tone] "pedals.3.tone" "Octave tone" { Fraction, Linear, 0.0, 1.0, 0.0 }
    toggle [pedals[4].on] "pedals.4.on" "Treble on" { false }
    float [pedals[4].amount] "pedals.4.amount" "Treble amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[4].tone] "pedals.4.tone" "Treble tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[5].on] "pedals.5.on" "Scoop on" { false }
    float [pedals[5].amount] "pedals.5.amount" "Scoop amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[5].tone] "pedals.5.tone" "Scoop tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[6].on] "pedals.6.on" "Wah on" { false }
    float [pedals[6].amount] "pedals.6.amount" "Wah amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[6].tone] "pedals.6.tone" "Wah tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[7].on] "pedals.7.on" "Compressor on" { false }
    float [pedals[7].amount] "pedals.7.amount" "Compressor amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[7].tone] "pedals.7.tone" "Compressor tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[8].on] "pedals.8.on" "Gate on" { false }
    float [pedals[8].amount] "pedals.8.amount" "Gate amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[8].tone] "pedals.8.tone" "Gate tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[9].on] "pedals.9.on" "Spring on" { false }
    float [pedals[9].amount] "pedals.9.amount" "Spring amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [pedals[9].tone] "pedals.9.tone" "Spring tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    toggle [pedals[10].on] "pedals.10.on" "Cabinet on" { false }
    float [pedals[10].amount] "pedals.10.amount" "Cabinet amount" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [pedals[10].tone] "pedals.10.tone" "Cabinet tone" { Fraction, Linear, 0.0, 1.0, 0.8 }
    choice [cabinet] "cabinet" "Cabinet model" { Cabinet, Combo, [
        Combo "combo" "1x12 combo", Twin "twin" "2x12 open",
        Stack "stack" "4x12 closed", Bass "bass" "1x15 bass"
    ] }
    choice [oversampling] "oversampling" "Quality" { Oversampling, X4, [
        Off "off" "Off", X2 "x2" "2x", X4 "x4" "4x", X8 "x8" "8x"
    ] }
});

impl GuitarRackParams {
    /// Whether a pedal is in the signal: its own switch, or being the
    /// pedal `model` names. Clean boost is never forced on.
    pub fn pedal_on(&self, pedal: GuitarModel) -> bool {
        self.pedals[pedal as usize].on
            || (self.model == pedal && pedal != GuitarModel::CleanBoost)
    }
}

/// Smooth cubic soft knee with different positive and negative limits.
/// `asymmetry` of 0 is symmetric; 0.5 gives limits of 0.85 and 1.15.
#[inline]
fn tube(input: f32, asymmetry: f32) -> f32 {
    let ceiling = if input >= 0.0 {
        1.0 - 0.3 * asymmetry
    } else {
        1.0 + 0.3 * asymmetry
    };
    let x = (input / ceiling).clamp(-1.0, 1.0);
    ceiling * (x - x * x * x / 3.0)
}

// Where the smoothed values of a pedal sit in the controls.
const WEIGHT: usize = 2;
const AMOUNT: usize = 13;
const TONE: usize = 24;
const VALUES: usize = 35;

/// What the knobs come to, worked out every [`CONTROL_PERIOD`] samples.
#[derive(Clone, Copy)]
struct Coeffs {
    gate_open: f32,
    gate_close: f32,
    gate_release_ms: f32,
    comp_threshold: f32,
    comp_makeup: f32,
    comp_release_ms: f32,
    wah: SvfCoeffs,
    wah_rate: f32,
    treble: f32,
    treble_gain: f32,
    tilt: f32,
    boost_gain: f32,
    boost_tilt: f32,
    overdrive_gain: f32,
    overdrive_asymmetry: f32,
    fuzz_gain: f32,
    fuzz_ceiling: f32,
    octave_amount: f32,
    octave_half: f32,
    scoop: SvfCoeffs,
    scoop_depth: f32,
    spring_damping: f32,
    spring_wet: f32,
    cabinet_wet: f32,
    cabinet_tone: f32,
}

impl Coeffs {
    fn new(values: &[f32; VALUES], sample_rate: f32, sweep: f32) -> Self {
        let amount = |pedal: GuitarModel| values[AMOUNT + pedal as usize].clamp(0.0, 1.0);
        let tone = |pedal: GuitarModel| values[TONE + pedal as usize].clamp(0.0, 1.0);
        use GuitarModel::*;
        let gate_open = db_to_gain(-54.0 + 36.0 * amount(Gate));
        let squash = amount(CompressorSquash);
        Self {
            gate_open,
            gate_close: 0.55 * gate_open,
            gate_release_ms: 5.0 + 195.0 * tone(Gate),
            comp_threshold: 0.6 * (1.0 - squash) * (1.0 - squash) + 0.03,
            comp_makeup: 1.0 + squash,
            comp_release_ms: 30.0 + 270.0 * tone(CompressorSquash),
            wah: SvfCoeffs::new(
                450.0 * 4.0_f32.powf(0.5 * (sweep + 1.0)),
                1.5 + 4.0 * amount(Wah),
                sample_rate,
            ),
            wah_rate: 0.2 * 30.0_f32.powf(tone(Wah)),
            treble: OnePoleFilter::coefficient(
                600.0 * 8.0_f32.powf(tone(TrebleBooster)),
                sample_rate,
            ),
            treble_gain: 5.0 * amount(TrebleBooster),
            tilt: OnePoleFilter::coefficient(1_000.0, sample_rate),
            boost_gain: db_to_gain(18.0 * amount(CleanBoost)),
            boost_tilt: 2.0 * tone(CleanBoost) - 1.0,
            overdrive_gain: db_to_gain(30.0 * amount(TubeSoftClip)),
            overdrive_asymmetry: tone(TubeSoftClip),
            fuzz_gain: 2.0 * 20.0_f32.powf(amount(HardFuzz)),
            fuzz_ceiling: 0.35 + 0.6 * tone(HardFuzz),
            octave_amount: amount(OctaveUp),
            octave_half: tone(OctaveUp),
            scoop: SvfCoeffs::new(300.0 * 5.0_f32.powf(tone(MidScoop)), 0.7, sample_rate),
            scoop_depth: 1.7 * amount(MidScoop),
            spring_damping: OnePoleFilter::coefficient(
                1_000.0 * 8.0_f32.powf(tone(SpringSlap)),
                sample_rate,
            ),
            spring_wet: 0.8 * amount(SpringSlap),
            cabinet_wet: amount(Cabinet),
            cabinet_tone: OnePoleFilter::coefficient(
                2_000.0 * 8.0_f32.powf(tone(Cabinet)),
                sample_rate,
            ),
        }
    }
}

/// Which cabinet plays, and the one it is fading in from.
#[derive(Clone, Copy)]
struct CabinetChoice {
    now: Cabinet,
    before: Cabinet,
    arrived: f32,
}

struct Channel {
    gate_detector: Ballistics,
    gate_gain: Ballistics,
    gate_open: bool,
    compressor: Ballistics,
    wah: Svf,
    treble: OnePoleFilter,
    tilt: OnePoleFilter,
    section: Oversampler,
    section_dc: DcBlocker,
    scoop: Svf,
    spring: DelayLine,
    spring_damping: OnePoleFilter,
    dispersion: f32,
    cabinet: CabinetChannel,
    cabinet_tone: OnePoleFilter,
}
impl Channel {
    fn new(sample_rate: f32, delay: usize, taps: usize) -> Self {
        let mut gate_detector = Ballistics::new(0.0);
        gate_detector.set_times(0.5, 35.0, sample_rate);
        Self {
            gate_detector,
            gate_gain: Ballistics::new(0.0),
            gate_open: false,
            compressor: Ballistics::new(0.0),
            wah: Svf::default(),
            treble: OnePoleFilter::default(),
            tilt: OnePoleFilter::default(),
            section: Oversampler::default(),
            section_dc: DcBlocker::new(20.0, sample_rate),
            scoop: Svf::default(),
            spring: DelayLine::new(delay),
            spring_damping: OnePoleFilter::default(),
            dispersion: 0.0,
            cabinet: CabinetChannel::new(taps),
            cabinet_tone: OnePoleFilter::default(),
        }
    }
    fn reset(&mut self) {
        self.gate_detector.snap(0.0);
        self.gate_gain.snap(0.0);
        self.gate_open = false;
        self.compressor.snap(0.0);
        self.wah.reset();
        self.treble.reset();
        self.tilt.reset();
        self.section.reset();
        self.section_dc.reset();
        self.scoop.reset();
        self.spring.clear();
        self.spring_damping.reset();
        self.dispersion = 0.0;
        self.cabinet.reset();
        self.cabinet_tone.reset();
    }
    /// Takes the times that follow the knobs, and drops memory that has
    /// decayed below hearing. Called at the control rate.
    fn retime(&mut self, coeffs: &Coeffs, sample_rate: f32) {
        self.gate_gain.set_times(2.0, coeffs.gate_release_ms, sample_rate);
        self.compressor.set_times(2.0, coeffs.comp_release_ms, sample_rate);
        self.wah.flush();
        self.treble.flush();
        self.tilt.flush();
        self.scoop.flush();
        self.spring_damping.flush();
        self.cabinet_tone.flush();
    }
    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        input: f32,
        values: &[f32; VALUES],
        c: &Coeffs,
        kernels: Option<&Kernels>,
        bank: &CabinetBank,
        cabinet: CabinetChoice,
        slap: [usize; 2],
    ) -> f32 {
        use GuitarModel::*;
        let weight = |pedal: GuitarModel| values[WEIGHT + pedal as usize];
        let mut x = input;

        let level = self.gate_detector.tick(x.abs());
        if level > c.gate_open {
            self.gate_open = true;
        }
        if level < c.gate_close {
            self.gate_open = false;
        }
        let gate = self.gate_gain.tick(if self.gate_open { 1.0 } else { 0.0 });
        x += (gate * x - x) * weight(Gate);

        let envelope = self.compressor.tick(x.abs());
        let squash = if envelope > c.comp_threshold {
            (c.comp_threshold / envelope).powf(0.8)
        } else {
            1.0
        };
        x += (c.comp_makeup * squash * x - x) * weight(CompressorSquash);

        let band = self.wah.tick(&c.wah, x).band * c.wah.k;
        x += (2.0 * band - x) * weight(Wah);

        let high = self.treble.high_pass(c.treble, x);
        x += c.treble_gain * high * weight(TrebleBooster);

        let tilted = x + c.boost_tilt * self.tilt.high_pass(c.tilt, x);
        x += (c.boost_gain * tilted - x) * weight(CleanBoost);

        // The three drive pedals are plain curves, so they share one trip
        // above the host rate.
        let (overdrive, fuzz, octave) = (weight(TubeSoftClip), weight(HardFuzz), weight(OctaveUp));
        let c = *c;
        let shaped = self.section.tick(kernels, audio(x), |sample| {
            let mut y = sample as f32;
            y += (tube(y * c.overdrive_gain, c.overdrive_asymmetry) - y) * overdrive;
            y += ((y * c.fuzz_gain).clamp(-c.fuzz_ceiling, c.fuzz_ceiling) - y) * fuzz;
            // Full-wave is the magnitude; adding the signal back makes it
            // half-wave at twice the level.
            let rectified = y.abs() + c.octave_half * y;
            y += (1.6 * rectified - y) * c.octave_amount * octave;
            f64::from(y)
        });
        // Only the lopsided curves make DC, so only they are blocked.
        let blocked = self.section_dc.tick(shaped);
        x = shaped + (blocked - shaped) * overdrive.max(octave);

        let mids = self.scoop.tick(&c.scoop, x).band * c.scoop.k;
        x -= c.scoop_depth * mids * weight(MidScoop);

        let first = self.spring.tap(slap[0]);
        let second = self.spring.tap(slap[1]);
        let damped = self
            .spring_damping
            .low_pass(c.spring_damping, 0.65 * first + 0.35 * second);
        // One-sample allpass dispersion inside a damped, bounded feedback
        // loop: a short metallic slap rather than a physical spring model.
        let dispersed = self.dispersion - 0.55 * damped;
        self.dispersion = audio(damped + 0.55 * dispersed);
        self.spring.push(audio(x + 0.35 * dispersed));
        x += c.spring_wet * dispersed * weight(SpringSlap);

        // The cabinet always hears the signal, and only works when heard.
        self.cabinet.push(audio(x));
        let heard = weight(Cabinet);
        if heard > 0.0 {
            let mut wet = self.cabinet.read(bank, cabinet.now);
            if cabinet.arrived < 1.0 {
                let before = self.cabinet.read(bank, cabinet.before);
                wet = before + (wet - before) * cabinet.arrived;
            }
            let toned = self.cabinet_tone.low_pass(c.cabinet_tone, wet);
            x += (toned - x) * c.cabinet_wet * heard;
        }
        audio(x * values[1])
    }
}

pub struct GuitarRack {
    controls: Controls<VALUES>,
    channels: [Channel; 2],
    coeffs: Coeffs,
    /// Samples until the coefficients are worked out again.
    countdown: usize,
    sample_rate: f32,
    lfo: Lfo,
    sweep: f32,
    slap: [usize; 2],
    kernels: Option<&'static Kernels>,
    quality: Oversampling,
    bank: CabinetBank,
    cabinet: Cabinet,
    cabinet_before: Cabinet,
    cabinet_arrival: LinearRamp,
    fade_samples: u32,
    /// Nothing has been processed since `prepare` or `reset`.
    fresh: bool,
}

fn slap_taps(sample_rate: f32) -> [usize; 2] {
    [
        ms_to_samples(37.0, sample_rate) as usize,
        ms_to_samples(53.0, sample_rate) as usize,
    ]
}

impl Default for GuitarRack {
    fn default() -> Self {
        let values = Self::values(&GuitarRackParams::default());
        let slap = slap_taps(48_000.0);
        Self {
            controls: Controls::new(values),
            channels: std::array::from_fn(|_| Channel::new(48_000.0, slap[1], 0)),
            coeffs: Coeffs::new(&values, 48_000.0, 0.0),
            countdown: 0,
            sample_rate: 48_000.0,
            lfo: Lfo::new(1),
            sweep: 0.0,
            slap,
            kernels: None,
            quality: Oversampling::default(),
            bank: CabinetBank::empty(),
            cabinet: Cabinet::default(),
            cabinet_before: Cabinet::default(),
            cabinet_arrival: LinearRamp::new(1.0),
            fade_samples: 240,
            fresh: true,
        }
    }
}
impl GuitarRack {
    fn values(p: &GuitarRackParams) -> [f32; VALUES] {
        let mut values = [0.0; VALUES];
        values[0] = db_to_gain(p.drive_db);
        values[1] = db_to_gain(p.output_db);
        for pedal in GuitarModel::ALL {
            let index = pedal as usize;
            values[WEIGHT + index] = if p.pedal_on(pedal) { 1.0 } else { 0.0 };
            values[AMOUNT + index] = p.pedals[index].amount;
            values[TONE + index] = p.pedals[index].tone;
        }
        values
    }
}
impl Effect for GuitarRack {
    type Params = GuitarRackParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.slap = slap_taps(self.sample_rate);
        self.kernels = Some(Kernels::shared());
        self.bank = CabinetBank::new(self.sample_rate);
        self.channels = std::array::from_fn(|_| {
            Channel::new(self.sample_rate, self.slap[1], self.bank.taps())
        });
        self.controls.prepare(self.sample_rate);
        self.fade_samples = ms_to_samples(5.0, self.sample_rate);
        self.reset();
        for channel in &mut self.channels {
            channel.section.set_quality(self.quality, true);
        }
    }
    fn reset(&mut self) {
        self.controls.reset();
        self.lfo.reset();
        self.sweep = 0.0;
        self.countdown = 0;
        for c in &mut self.channels {
            c.reset();
        }
        self.cabinet_before = self.cabinet;
        self.cabinet_arrival.snap(1.0);
        self.fresh = true;
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set(Self::values(&p));
        self.quality = p.oversampling;
        for channel in &mut self.channels {
            channel.section.set_quality(self.quality, self.fresh);
        }
        if p.cabinet != self.cabinet {
            if self.fresh {
                self.cabinet_before = p.cabinet;
                self.cabinet_arrival.snap(1.0);
            } else {
                self.cabinet_before = self.cabinet;
                self.cabinet_arrival.snap(0.0);
                self.cabinet_arrival.set_target(1.0, self.fade_samples);
            }
            self.cabinet = p.cabinet;
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        for (l, r) in left.iter_mut().zip(right) {
            let values = self.controls.tick();
            if self.countdown == 0 {
                self.coeffs = Coeffs::new(&values, self.sample_rate, self.sweep);
                for channel in &mut self.channels {
                    channel.retime(&self.coeffs, self.sample_rate);
                }
                self.countdown = CONTROL_PERIOD;
            }
            self.countdown -= 1;
            self.sweep = self
                .lfo
                .tick(LfoShape::Sine, self.coeffs.wah_rate / self.sample_rate);
            let cabinet = CabinetChoice {
                now: self.cabinet,
                before: self.cabinet_before,
                arrived: self.cabinet_arrival.tick(),
            };
            for (channel, sample) in self.channels.iter_mut().zip([l, r]) {
                *sample = channel.tick(
                    audio(audio(*sample) * values[0]),
                    &values,
                    &self.coeffs,
                    self.kernels,
                    &self.bank,
                    cabinet,
                    self.slap,
                );
            }
        }
    }
    fn latency_samples(&self) -> usize {
        DRIVE_LATENCY_SAMPLES
    }
    // All histories run continuously, including while a pedal is off.
    fn tail_samples(&self) -> usize {
        (self.sample_rate * 2.0).ceil() as usize + DRIVE_LATENCY_SAMPLES + self.bank.taps()
    }
    fn gap_samples(&self) -> usize {
        self.slap[1] + DRIVE_LATENCY_SAMPLES + self.bank.taps()
    }
}
