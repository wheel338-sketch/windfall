use super::{audio, output, rate};
use crate::balance::Controls;
use crate::blocks::biquad::{Biquad, BiquadCoeffs};
use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::smoothing_coefficient;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const GRAIN_SPAN: usize = 2048;
/// Fixed grain-centre/dry-path delay; wet grains span 2..2050 samples.
pub const PITCH_SHIFT_LATENCY: usize = 2 + GRAIN_SPAN / 2;
const TRACK_SIZE: usize = 256;
const TRACK_HOP: usize = 32;
const TRACK_LAGS: usize = 152;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PitchShiftMode {
    #[default]
    Mix,
    Voice,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PitchShiftParams {
    /// -24..24 semitones; ratio = 2^(semitones/12).
    pub semitones: f32,
    pub formant_semitones: f32,
    pub mode: PitchShiftMode,
    pub mix: f32,
}
impl Default for PitchShiftParams {
    fn default() -> Self {
        Self {
            semitones: 0.0,
            formant_semitones: 0.0,
            mode: PitchShiftMode::Mix,
            mix: 1.0,
        }
    }
}
param_set!(PitchShiftParams, "Pitch Shift", {
    float [semitones] "semitones" "Shift" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [formant_semitones] "formantSemitones" "Formants" { Semitones, Linear, -12.0, 12.0, 0.0 }
    choice [mode] "mode" "Mode" { PitchShiftMode, Mix, [Mix "mix" "Full mix", Voice "voice" "Voice"] }
});

/// Two cubic fractional read heads with complementary raised-cosine windows.
/// Their linked stereo phase advances by 1-ratio samples per input sample.
struct Grains {
    lines: [DelayLine; 2],
    phase: f64,
    extra_delay: usize,
}
impl Default for Grains {
    fn default() -> Self {
        Self {
            lines: std::array::from_fn(|_| DelayLine::new(GRAIN_SPAN + 2)),
            phase: 0.5,
            extra_delay: 0,
        }
    }
}
impl Grains {
    fn prepare(&mut self, extra_delay: usize) {
        self.extra_delay = extra_delay;
        self.lines = std::array::from_fn(|_| DelayLine::new(extra_delay + GRAIN_SPAN + 2));
        self.reset();
    }
    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.phase = 0.5;
    }
    fn tick(&mut self, input: [f32; 2], ratio: f32) -> ([f32; 2], [f32; 2]) {
        self.tick_span(input, ratio, GRAIN_SPAN as f32)
    }
    fn tick_span(&mut self, input: [f32; 2], ratio: f32, span: f32) -> ([f32; 2], [f32; 2]) {
        let p = self.phase as f32;
        let q = (p + 0.5).fract();
        let weight = 0.5 - 0.5 * (std::f32::consts::TAU * p).cos();
        // Keep the centre age fixed when a tracked voice chooses a shorter,
        // pitch-synchronous window; storage and reported latency stay bounded.
        let start = (self.extra_delay + 2) as f32 + (GRAIN_SPAN as f32 - span) * 0.5;
        let a = start + p * span;
        let b = start + q * span;
        let mut wet = [0.0; 2];
        let mut dry = [0.0; 2];
        // Collapse onto the fixed delay at unison, including after automation.
        let shifted = ((ratio - 1.0).abs() * 10_000.0).min(1.0);
        for ch in 0..2 {
            dry[ch] = self.lines[ch].tap(self.extra_delay + PITCH_SHIFT_LATENCY);
            let grain =
                self.lines[ch].tap_cubic(a) * weight + self.lines[ch].tap_cubic(b) * (1.0 - weight);
            wet[ch] = dry[ch] + (grain - dry[ch]) * shifted;
            self.lines[ch].push(input[ch]);
        }
        self.phase = (self.phase + (1.0 - f64::from(ratio)) / f64::from(span)).rem_euclid(1.0);
        (wet, dry)
    }
    fn latency(&self) -> usize {
        self.extra_delay + PITCH_SHIFT_LATENCY
    }
    fn tail(&self) -> usize {
        self.extra_delay + GRAIN_SPAN + 4
    }
}

pub struct PitchShift {
    grains: Grains,
    source_grains: Grains,
    formants: super::formants::Formants,
    mode: PitchShiftMode,
    sample_rate: f32,
    controls: Controls<3>,
}
impl Default for PitchShift {
    fn default() -> Self {
        Self {
            grains: Grains::default(),
            source_grains: Grains::default(),
            formants: super::formants::Formants::default(),
            mode: PitchShiftMode::Mix,
            sample_rate: 48000.0,
            controls: Controls::new([0.0, 1.0, 0.0]),
        }
    }
}
impl Effect for PitchShift {
    type Params = PitchShiftParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.grains.prepare(0);
        self.source_grains.prepare(0);
        self.formants.reset();
        self.controls.prepare(rate(sample_rate));
    }
    fn reset(&mut self) {
        self.grains.reset();
        self.source_grains.reset();
        self.formants.reset();
        self.controls.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.mode = p.mode;
        self.controls.set([p.semitones, p.mix, p.formant_semitones]);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let [semitones, mix, formant] = self.controls.tick();
            let input = [audio(*l), audio(*r)];
            let ratio = (semitones / 12.0).exp2();
            let (plain, dry) = self.grains.tick(input, ratio);
            let excitation = self
                .formants
                .excitation(input, self.mode == PitchShiftMode::Voice);
            let (shifted, _) = self.source_grains.tick(excitation, ratio);
            let filtered = self.formants.synthesize(shifted, formant);
            // Mix mode is the legacy transparent grain path at zero formant;
            // Voice mode preserves the estimated envelope during pitch shifts.
            let share = if self.mode == PitchShiftMode::Voice {
                (semitones.abs() + formant.abs()).min(1.0)
            } else {
                formant.abs().min(1.0)
            };
            let wet =
                std::array::from_fn::<_, 2, _>(|ch| plain[ch] + (filtered[ch] - plain[ch]) * share);
            *l = output(dry[0] + (wet[0] - dry[0]) * mix);
            *r = output(dry[1] + (wet[1] - dry[1]) * mix);
        }
    }
    fn latency_samples(&self) -> usize {
        self.grains.latency()
    }
    fn tail_samples(&self) -> usize {
        self.grains.tail() + (self.sample_rate * 2.0) as usize
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
    fn warm_up_samples(&self) -> usize {
        self.grains.tail()
    }
    fn delay_readiness_samples(&self) -> usize {
        self.grains.tail()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PitchScale {
    #[default]
    Chromatic,
    Major,
    Minor,
    Pentatonic,
}
impl PitchScale {
    fn mask(self) -> u16 {
        match self {
            Self::Chromatic => 0x0fff,
            Self::Major => 0x0ab5,
            Self::Minor => 0x05ad,
            Self::Pentatonic => 0x0295,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PitchCorrectParams {
    /// Pitch class, C=0..B=11.
    pub root: u8,
    pub scale: PitchScale,
    /// Time constant in ms, 0..500; zero snaps on each voiced estimate.
    pub speed_ms: f32,
    /// Correction share, 0..1.
    pub amount: f32,
    /// A4 tuning, 400..480 Hz.
    pub tuning_hz: f32,
    pub mix: f32,
    #[serde(default, skip_serializing_if = "is_zero_channel")]
    #[ts(as = "Option<u32>", optional)]
    pub harmony_channel: u32,
    pub harmony_mix: f32,
}
fn is_zero_channel(channel: &u32) -> bool {
    *channel == 0
}
impl Default for PitchCorrectParams {
    fn default() -> Self {
        Self {
            root: 0,
            scale: PitchScale::Chromatic,
            speed_ms: 35.0,
            amount: 1.0,
            tuning_hz: 440.0,
            mix: 1.0,
            harmony_channel: 0,
            harmony_mix: 0.0,
        }
    }
}
param_set!(PitchCorrectParams, "Pitch Correct", {
    int [root] "root" "Root" { None, 0, 11, 0 }
    choice [scale] "scale" "Scale" { PitchScale, Chromatic, [Chromatic "chromatic" "Chromatic", Major "major" "Major", Minor "minor" "Minor", Pentatonic "pentatonic" "Pentatonic"] }
    float [speed_ms] "speedMs" "Speed" { Milliseconds, Linear, 0.0, 500.0, 35.0 }
    float [amount] "amount" "Amount" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [tuning_hz] "tuningHz" "Tuning" { Hertz, Linear, 400.0, 480.0, 440.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [harmony_mix] "harmonyMix" "Harmony level" { Fraction, Linear, 0.0, 1.0, 0.0 }
});

/// Monophonic shared-YIN detection at approximately 6 kHz. A bounded
/// 256-sample ring, 32-sample hop and <=151 lags cost no worker handoff/locks.
pub struct PitchCorrect {
    params: PitchCorrectParams,
    controls: Controls<5>,
    harmonies: [Grains; 4],
    harmony_notes: [u8; 4],
    harmony_levels: [f32; 4],
    grains: Grains,
    sample_rate: f32,
    decimation: usize,
    decimation_cursor: usize,
    tracker_rate: f32,
    lowpass: [[Biquad; 2]; 2],
    lowpass_coeffs: BiquadCoeffs,
    tracker: [[f32; TRACK_SIZE]; 2],
    tracker_cursor: usize,
    tracked: usize,
    hop: usize,
    frequency: f32,
    correction: f32,
}
impl Default for PitchCorrect {
    fn default() -> Self {
        let mut effect = Self {
            params: PitchCorrectParams::default(),
            controls: Controls::new([35.0, 1.0, 440.0, 1.0, 0.0]),
            harmonies: std::array::from_fn(|_| Grains::default()),
            harmony_notes: [255; 4],
            harmony_levels: [0.0; 4],
            grains: Grains::default(),
            sample_rate: 48_000.0,
            decimation: 8,
            decimation_cursor: 0,
            tracker_rate: 6_000.0,
            lowpass: [[Biquad::default(); 2]; 2],
            lowpass_coeffs: BiquadCoeffs::IDENTITY,
            tracker: [[0.0; TRACK_SIZE]; 2],
            tracker_cursor: 0,
            tracked: 0,
            hop: 0,
            frequency: 0.0,
            correction: 0.0,
        };
        effect.prepare(48_000.0, 1);
        effect
    }
}
impl PitchCorrect {
    /// Four lowest distinct held MIDI keys. 255 means released/absent.
    pub fn set_harmony_notes(&mut self, notes: [u8; 4]) {
        self.harmony_notes = notes.map(|key| if key < 128 { key } else { 255 });
    }
    fn estimate(&mut self) {
        let centered_energy = |samples: &[f32; TRACK_SIZE]| {
            let mean = samples.iter().sum::<f32>() / TRACK_SIZE as f32;
            samples.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
        };
        let channel =
            usize::from(centered_energy(&self.tracker[1]) > centered_energy(&self.tracker[0]));
        let mut signal = [0.0; TRACK_SIZE];
        let mean = self.tracker[channel].iter().sum::<f32>() / TRACK_SIZE as f32;
        for (i, sample) in signal.iter_mut().enumerate() {
            *sample = self.tracker[channel][(self.tracker_cursor + i) % TRACK_SIZE] - mean;
        }
        if signal.iter().map(|v| v * v).sum::<f32>() / (TRACK_SIZE as f32) < 1e-7 {
            self.frequency = 0.0;
            return;
        }
        let max_lag = (self.tracker_rate / 60.0)
            .ceil()
            .min((TRACK_LAGS - 2) as f32) as usize;
        let comparison = TRACK_SIZE - TRACK_LAGS;
        let mut raw = [0.0_f64; TRACK_LAGS];
        let mut cmnd = [1.0_f64; TRACK_LAGS];
        let mut sum = 0.0;
        for lag in 1..=max_lag + 1 {
            raw[lag] = (0..comparison)
                .map(|i| {
                    let d = f64::from(signal[i]) - f64::from(signal[i + lag]);
                    d * d
                })
                .sum();
            sum += raw[lag];
            cmnd[lag] = if sum > 0.0 {
                raw[lag] * lag as f64 / sum
            } else {
                1.0
            };
        }
        self.frequency = windfall_core::yin::first_trough(&cmnd, max_lag, 0.15)
            .and_then(|(lag, _)| {
                let offset =
                    windfall_core::yin::parabolic_minimum(raw[lag - 1], raw[lag], raw[lag + 1]).0;
                let hz = f64::from(self.tracker_rate) / (lag as f64 + offset);
                (60.0..=1000.0).contains(&hz).then_some(hz as f32)
            })
            .unwrap_or(0.0);
    }
    fn target(&self, tuning: f32) -> f32 {
        if self.frequency <= 0.0 {
            return 0.0;
        }
        let note = 69.0 + 12.0 * (self.frequency / tuning).log2();
        let centre = note.round() as i32;
        let mut nearest = centre;
        let mut distance = f32::INFINITY;
        for candidate in centre - 12..=centre + 12 {
            let class = (candidate - i32::from(self.params.root)).rem_euclid(12) as u32;
            if self.params.scale.mask() & (1_u16 << class) != 0 {
                let d = (candidate as f32 - note).abs();
                if d < distance {
                    nearest = candidate;
                    distance = d;
                }
            }
        }
        (nearest as f32 - note).clamp(-12.0, 12.0)
    }
}
impl Effect for PitchCorrect {
    type Params = PitchCorrectParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.decimation = (self.sample_rate / 6000.0).round().max(1.0) as usize;
        self.tracker_rate = self.sample_rate / self.decimation as f32;
        self.lowpass_coeffs =
            BiquadCoeffs::low_pass(self.tracker_rate * 0.4, 0.707, self.sample_rate);
        self.grains.prepare(TRACK_SIZE * self.decimation);
        for harmony in &mut self.harmonies {
            harmony.prepare(TRACK_SIZE * self.decimation);
        }
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.grains.reset();
        self.controls.reset();
        self.lowpass.fill([Biquad::default(); 2]);
        self.tracker.fill([0.0; TRACK_SIZE]);
        for harmony in &mut self.harmonies {
            harmony.reset();
        }
        self.harmony_notes.fill(255);
        self.harmony_levels.fill(0.0);
        self.tracker_cursor = 0;
        self.decimation_cursor = 0;
        self.tracked = 0;
        self.hop = 0;
        self.frequency = 0.0;
        self.correction = 0.0;
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        let p = self.params;
        self.controls
            .set([p.speed_ms, p.amount, p.tuning_hz, p.mix, p.harmony_mix]);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let input = [audio(*l), audio(*r)];
            let filtered = std::array::from_fn::<_, 2, _>(|ch| {
                let mut value = f64::from(input[ch]);
                for section in &mut self.lowpass[ch] {
                    value = section.tick(&self.lowpass_coeffs, value);
                    section.flush();
                }
                output(value as f32)
            });
            self.decimation_cursor += 1;
            if self.decimation_cursor == self.decimation {
                self.decimation_cursor = 0;
                for (ch, value) in filtered.iter().enumerate() {
                    self.tracker[ch][self.tracker_cursor] = *value;
                }
                self.tracker_cursor = (self.tracker_cursor + 1) % TRACK_SIZE;
                self.tracked = (self.tracked + 1).min(TRACK_SIZE);
                self.hop += 1;
                if self.hop == TRACK_HOP {
                    self.hop = 0;
                    if self.tracked == TRACK_SIZE {
                        self.estimate();
                    }
                }
            }
            let [speed, amount, tuning, mix, harmony_mix] = self.controls.tick();
            let target = self.target(tuning) * amount;
            self.correction +=
                (target - self.correction) * smoothing_coefficient(speed, self.sample_rate);
            // Both read heads must differ by whole source periods. Otherwise
            // their windows cancel/beat and quantize a harmony onto grain-rate
            // sidebands instead of the requested MIDI pitch.
            let span = if self.frequency > 0.0 {
                let pair = 2.0 * self.sample_rate / self.frequency;
                ((GRAIN_SPAN as f32 / pair).floor().max(1.0) * pair).min(GRAIN_SPAN as f32)
            } else {
                GRAIN_SPAN as f32
            };
            let (mut wet, dry) = self.grains.tick_span(input, (self.correction / 12.0).exp2(), span);
            let source_note = if self.frequency > 0.0 {
                69.0 + 12.0 * (self.frequency / tuning).log2()
            } else {
                69.0
            };
            for voice in 0..4 {
                let active = self.harmony_notes[voice] < 128 && self.frequency > 0.0;
                let target_level = if active { 1.0 } else { 0.0 };
                self.harmony_levels[voice] += (target_level - self.harmony_levels[voice])
                    * smoothing_coefficient(5.0, self.sample_rate);
                let shift = if active {
                    (f32::from(self.harmony_notes[voice]) - source_note).clamp(-24.0, 24.0)
                } else {
                    0.0
                };
                let (harmony, _) = self.harmonies[voice].tick_span(input, (shift / 12.0).exp2(), span);
                for ch in 0..2 {
                    wet[ch] += harmony[ch] * self.harmony_levels[voice] * harmony_mix * 0.25;
                }
            }
            *l = output(dry[0] + (wet[0] - dry[0]) * mix);
            *r = output(dry[1] + (wet[1] - dry[1]) * mix);
        }
    }
    fn latency_samples(&self) -> usize {
        self.grains.latency()
    }
    fn tail_samples(&self) -> usize {
        self.grains.tail()
    }
    fn gap_samples(&self) -> usize {
        self.grains.tail()
    }
    fn warm_up_samples(&self) -> usize {
        self.grains.tail()
    }
    fn delay_readiness_samples(&self) -> usize {
        self.grains.tail()
    }
}
