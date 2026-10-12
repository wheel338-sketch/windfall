use super::cabinet::{Cabinet, CabinetBank, CabinetChannel};
use super::oversample::{DRIVE_LATENCY_SAMPLES, Kernels, Oversampler, Oversampling};
use crate::balance::{Controls, audio, rate};
use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::db_to_gain;
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::{Svf, SvfCoeffs};
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DriveStage {
    #[default]
    Bypass,
    Overdrive,
    Waveshape,
    Fuzz,
    Lowpass,
    Highpass,
    Chorus,
    Cabinet,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DriveStageParams {
    /// Stage algorithm; default bypass (which ignores stage gain).
    pub model: DriveStage,
    /// Gain ahead of the nonlinearity in dB, -24..36; default 0.
    pub gain_db: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub depth: f32,
    pub rate_hz: f32,
    pub cabinet: Cabinet,
}
impl Default for DriveStageParams {
    fn default() -> Self {
        Self {
            model: DriveStage::Bypass,
            gain_db: 0.0,
            cutoff_hz: 4_000.0,
            resonance: 0.707,
            depth: 0.5,
            rate_hz: 0.7,
            cabinet: Cabinet::Combo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DriveChainParams {
    /// Signal flows through indices 0, 1, 2, then through output trim.
    pub stages: [DriveStageParams; 3],
    /// Output trim in dB, -24..12; default 0.
    pub output_db: f32,
    pub oversampling: Oversampling,
}
impl Default for DriveChainParams {
    fn default() -> Self {
        Self {
            stages: [DriveStageParams::default(); 3],
            output_db: 0.0,
            oversampling: Oversampling::X4,
        }
    }
}
param_set!(DriveChainParams, "Drive Chain", {
    choice [stages[0].model] "stages.0.model" "Stage 1" { DriveStage, Bypass, [Bypass "bypass" "Bypass", Overdrive "overdrive" "Overdrive", Waveshape "waveshape" "Waveshape", Fuzz "fuzz" "Fuzz", Lowpass "lowpass" "Lowpass", Highpass "highpass" "Highpass", Chorus "chorus" "Chorus", Cabinet "cabinet" "Cabinet"] }
    float [stages[0].gain_db] "stages.0.gainDb" "Stage 1 gain" { Decibels, Linear, -24.0, 36.0, 0.0 }
    choice [stages[1].model] "stages.1.model" "Stage 2" { DriveStage, Bypass, [Bypass "bypass" "Bypass", Overdrive "overdrive" "Overdrive", Waveshape "waveshape" "Waveshape", Fuzz "fuzz" "Fuzz", Lowpass "lowpass" "Lowpass", Highpass "highpass" "Highpass", Chorus "chorus" "Chorus", Cabinet "cabinet" "Cabinet"] }
    float [stages[1].gain_db] "stages.1.gainDb" "Stage 2 gain" { Decibels, Linear, -24.0, 36.0, 0.0 }
    choice [stages[2].model] "stages.2.model" "Stage 3" { DriveStage, Bypass, [Bypass "bypass" "Bypass", Overdrive "overdrive" "Overdrive", Waveshape "waveshape" "Waveshape", Fuzz "fuzz" "Fuzz", Lowpass "lowpass" "Lowpass", Highpass "highpass" "Highpass", Chorus "chorus" "Chorus", Cabinet "cabinet" "Cabinet"] }
    float [stages[2].gain_db] "stages.2.gainDb" "Stage 3 gain" { Decibels, Linear, -24.0, 36.0, 0.0 }
    float [output_db] "outputDb" "Output" { Decibels, Linear, -24.0, 12.0, 0.0 }
    float [stages[0].cutoff_hz] "stages.0.cutoffHz" "Stage 1 cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 4000.0 }
    float [stages[0].resonance] "stages.0.resonance" "Stage 1 resonance" { Ratio, Linear, 0.5, 4.0, 0.707 }
    float [stages[0].depth] "stages.0.depth" "Stage 1 depth" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [stages[0].rate_hz] "stages.0.rateHz" "Stage 1 rate" { Hertz, Logarithmic, 0.1, 10.0, 0.7 }
    choice [stages[0].cabinet] "stages.0.cabinet" "Stage 1 cabinet" { Cabinet, Combo, [Combo "combo" "Combo", Twin "twin" "Twin", Stack "stack" "Stack", Bass "bass" "Bass"] }
    float [stages[1].cutoff_hz] "stages.1.cutoffHz" "Stage 2 cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 4000.0 }
    float [stages[1].resonance] "stages.1.resonance" "Stage 2 resonance" { Ratio, Linear, 0.5, 4.0, 0.707 }
    float [stages[1].depth] "stages.1.depth" "Stage 2 depth" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [stages[1].rate_hz] "stages.1.rateHz" "Stage 2 rate" { Hertz, Logarithmic, 0.1, 10.0, 0.7 }
    choice [stages[1].cabinet] "stages.1.cabinet" "Stage 2 cabinet" { Cabinet, Combo, [Combo "combo" "Combo", Twin "twin" "Twin", Stack "stack" "Stack", Bass "bass" "Bass"] }
    float [stages[2].cutoff_hz] "stages.2.cutoffHz" "Stage 3 cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 4000.0 }
    float [stages[2].resonance] "stages.2.resonance" "Stage 3 resonance" { Ratio, Linear, 0.5, 4.0, 0.707 }
    float [stages[2].depth] "stages.2.depth" "Stage 3 depth" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [stages[2].rate_hz] "stages.2.rateHz" "Stage 3 rate" { Hertz, Logarithmic, 0.1, 10.0, 0.7 }
    choice [stages[2].cabinet] "stages.2.cabinet" "Stage 3 cabinet" { Cabinet, Combo, [Combo "combo" "Combo", Twin "twin" "Twin", Stack "stack" "Stack", Bass "bass" "Bass"] }
    choice [oversampling] "oversampling" "Quality" { Oversampling, X4, [Off "off" "Off", X2 "x2" "2x", X4 "x4" "4x", X8 "x8" "8x"] }
});

#[inline]
fn transfers(input: f32, gain: f32) -> [f32; 4] {
    let x = input * gain;
    let bounded = x.clamp(-1.0, 1.0);
    [
        input,
        super::asymmetric(x, 0.8),
        1.5 * bounded - 0.5 * bounded.powi(3),
        (4.0 * x).clamp(-0.7, 0.7),
    ]
}
impl DriveChainParams {
    pub fn transfer(&self, input: f32) -> f32 {
        let p = self.sanitized();
        let mut x = audio(input);
        for stage in p.stages {
            x = if (stage.model as usize) < 4 {
                transfers(x, db_to_gain(stage.gain_db))[stage.model as usize]
            } else {
                x
            };
        }
        audio(x * db_to_gain(p.output_db))
    }
}

pub const CHAIN_LATENCY_SAMPLES: usize = 3 * DRIVE_LATENCY_SAMPLES;
const WIDTH: usize = 13;
struct StageChannel {
    filter_cache: Option<([f32; 3], SvfCoeffs)>,
    shaper: Oversampler,
    filter: Svf,
    chorus: DelayLine,
    cabinet: CabinetChannel,
    selected: Cabinet,
    before: Cabinet,
    arrival: LinearRamp,
    fresh: bool,
}
impl StageChannel {
    fn new(rate: f32, taps: usize) -> Self {
        Self {
            filter_cache: None,
            shaper: Oversampler::default(),
            filter: Svf::default(),
            chorus: DelayLine::new((rate * 0.04).ceil() as usize),
            cabinet: CabinetChannel::new(taps),
            selected: Cabinet::Combo,
            before: Cabinet::Combo,
            arrival: LinearRamp::new(1.0),
            fresh: true,
        }
    }
    fn reset(&mut self) {
        self.shaper.reset();
        self.filter.reset();
        self.chorus.clear();
        self.cabinet.reset();
        self.fresh = true;
        self.arrival.snap(1.0);
    }
    fn tick(
        &mut self,
        input: f32,
        v: &[f32],
        rate: f32,
        phase: f64,
        kernels: Option<&Kernels>,
        cabinet: (&CabinetBank, Cabinet),
    ) -> f32 {
        let (bank, cabinet) = cabinet;
        // Stateful stages pass through the same identity resampler as bypass;
        // each module always contributes 32 frames, regardless of selection.
        let shaped = self.shaper.tick(kernels, input, |sample| {
            let y = transfers(sample as f32, v[0]);
            f64::from(
                y.iter().zip(&v[1..5]).map(|(y, w)| y * w).sum::<f32>()
                    + sample as f32 * v[5..9].iter().sum::<f32>(),
            )
        });
        let key = [v[9], v[10], rate];
        let coeffs = match self.filter_cache {
            Some((before, coeffs)) if before == key => coeffs,
            _ => {
                let coeffs = SvfCoeffs::new(v[9], v[10], rate);
                self.filter_cache = Some((key, coeffs));
                coeffs
            }
        };
        let filtered = self.filter.tick(&coeffs, shaped);
        self.filter.flush();
        let delay = (rate * (0.012 + 0.008 * v[11] * phase.sin() as f32))
            .max(1.0)
            .min(self.chorus.max_delay().max(1) as f32);
        let chorused = 0.7 * shaped + 0.7 * self.chorus.tap_linear(delay);
        self.chorus.push(shaped);
        self.cabinet.push(shaped);
        if cabinet != self.selected {
            self.before = self.selected;
            self.selected = cabinet;
            if !self.fresh {
                self.arrival.snap(0.0);
                self.arrival
                    .set_target(1.0, (rate * 0.005).round().max(1.0) as u32);
            }
        }
        self.fresh = false;
        let arrived = self.arrival.tick();
        let cab = if v[8] > 0.0 {
            let now = self.cabinet.read(bank, self.selected);
            if arrived < 1.0 {
                let before = self.cabinet.read(bank, self.before);
                before + (now - before) * arrived
            } else {
                now
            }
        } else {
            shaped
        };
        audio(
            shaped
                + (filtered.low - shaped) * v[5]
                + (filtered.high - shaped) * v[6]
                + (chorused - shaped) * v[7]
                + (cab - shaped) * v[8],
        )
    }
}
pub struct DriveChain {
    controls: Controls<40>,
    channels: [[StageChannel; 2]; 3],
    phases: [f64; 3],
    bank: CabinetBank,
    cabinets: [Cabinet; 3],
    rate: f32,
    kernels: Option<&'static Kernels>,
    quality: Oversampling,
    fresh: bool,
}
impl Default for DriveChain {
    fn default() -> Self {
        Self {
            controls: Controls::new(Self::values(&DriveChainParams::default())),
            channels: std::array::from_fn(|_| {
                std::array::from_fn(|_| StageChannel::new(48_000.0, 0))
            }),
            phases: [0.0; 3],
            bank: CabinetBank::empty(),
            cabinets: [Cabinet::Combo; 3],
            rate: 48_000.0,
            kernels: None,
            quality: Oversampling::default(),
            fresh: true,
        }
    }
}
impl DriveChain {
    fn values(p: &DriveChainParams) -> [f32; 40] {
        let mut values = [0.0; 40];
        for (i, stage) in p.stages.iter().enumerate() {
            values[WIDTH * i] = db_to_gain(stage.gain_db);
            values[WIDTH * i + 1 + stage.model as usize] = 1.0;
            values[WIDTH * i + 9] = stage.cutoff_hz;
            values[WIDTH * i + 10] = stage.resonance;
            values[WIDTH * i + 11] = stage.depth;
            values[WIDTH * i + 12] = stage.rate_hz;
        }
        values[39] = db_to_gain(p.output_db);
        values
    }
}
impl Effect for DriveChain {
    type Params = DriveChainParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.kernels = Some(Kernels::shared());
        self.bank = CabinetBank::new(self.rate);
        self.channels = std::array::from_fn(|_| {
            std::array::from_fn(|_| StageChannel::new(self.rate, self.bank.taps()))
        });
        self.controls.prepare(self.rate);
        self.reset();
        for c in self.channels.iter_mut().flatten() {
            c.shaper.set_quality(self.quality, true);
        }
    }
    fn reset(&mut self) {
        self.controls.reset();
        self.phases = [0.0; 3];
        for c in self.channels.iter_mut().flatten() {
            c.reset();
        }
        self.fresh = true;
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set(Self::values(&p));
        self.quality = p.oversampling;
        for c in self.channels.iter_mut().flatten() {
            c.shaper.set_quality(self.quality, self.fresh);
        }
        self.cabinets = p.stages.map(|stage| stage.cabinet);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        for (l, r) in left.iter_mut().zip(right) {
            let values = self.controls.tick();
            let mut pair = [audio(*l), audio(*r)];
            for stage in 0..3 {
                let v = &values[stage * WIDTH..(stage + 1) * WIDTH];
                for (c, sample) in pair.iter_mut().enumerate() {
                    *sample = self.channels[stage][c].tick(
                        *sample,
                        v,
                        self.rate,
                        self.phases[stage] + c as f64 * 1.1,
                        self.kernels,
                        (&self.bank, self.cabinets[stage]),
                    );
                }
                self.phases[stage] = (self.phases[stage]
                    + std::f64::consts::TAU * v[12] as f64 / self.rate as f64)
                    .rem_euclid(std::f64::consts::TAU);
            }
            *l = audio(pair[0] * values[39]);
            *r = audio(pair[1] * values[39]);
        }
    }
    fn latency_samples(&self) -> usize {
        CHAIN_LATENCY_SAMPLES
    }
    fn tail_samples(&self) -> usize {
        (self.rate * 2.0) as usize + CHAIN_LATENCY_SAMPLES + self.bank.taps()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(params: DriveChainParams, input: &[f32], block: usize) -> Vec<f32> {
        let mut effect = DriveChain::default();
        effect.prepare(48_000.0, 512);
        effect.set_params(&params);
        let mut left = input.to_vec();
        let mut right = input.to_vec();
        for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
            effect.process(l, r);
        }
        left
    }
    #[test]
    fn filters_chorus_and_cabinet_are_real_modules_and_reordering_changes_audio() {
        let input: Vec<_> = (0..16_000)
            .map(|n| 0.4 * (std::f32::consts::TAU * 3719.0 * n as f32 / 48_000.0).sin())
            .collect();
        let mut p = DriveChainParams {
            stages: [
                DriveStageParams {
                    model: DriveStage::Overdrive,
                    gain_db: 15.0,
                    ..Default::default()
                },
                DriveStageParams {
                    model: DriveStage::Chorus,
                    depth: 0.9,
                    rate_hz: 3.0,
                    ..Default::default()
                },
                DriveStageParams {
                    model: DriveStage::Cabinet,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let reference = run(p, &input, 137);
        for (index, model) in [
            (0, DriveStage::Fuzz),
            (1, DriveStage::Lowpass),
            (1, DriveStage::Highpass),
            (2, DriveStage::Bypass),
        ] {
            let mut changed = p;
            changed.stages[index].model = model;
            let output = run(changed, &input, 137);
            assert!(
                reference[4096..]
                    .iter()
                    .zip(&output[4096..])
                    .any(|(a, b)| (a - b).abs() > 1e-3),
                "{model:?}"
            );
        }
        p.stages.swap(0, 2);
        let swapped = run(p, &input, 137);
        assert!(
            reference[4096..]
                .iter()
                .zip(&swapped[4096..])
                .any(|(a, b)| (a - b).abs() > 1e-3)
        );
        for block in [1, 7, 64, 512] {
            assert_eq!(swapped, run(p, &input, block));
        }
    }
    #[test]
    fn legacy_indices_are_unchanged_and_static_chain_matches_its_reference() {
        let p: DriveChainParams = serde_json::from_str(r#"{"stages":[{"model":"fuzz","gainDb":0},{"model":"overdrive","gainDb":0},{"model":"bypass","gainDb":0}],"outputDb":0,"oversampling":"off"}"#).unwrap();
        assert_eq!(DriveChainParams::descriptors()[6].id, "outputDb");
        let input: Vec<_> = (0..1024).map(|n| (n as f32 / 511.5 - 1.0) * 0.8).collect();
        let output = run(p, &input, 7);
        for (n, y) in output.iter().enumerate().skip(CHAIN_LATENCY_SAMPLES) {
            let fuzz = (4.0 * input[n - CHAIN_LATENCY_SAMPLES]).clamp(-0.7, 0.7);
            let ceiling = if fuzz >= 0.0 {
                1.0 - 0.45 * 0.8
            } else {
                1.0 + 0.25 * 0.8
            };
            let expected = fuzz / (1.0 + fuzz.abs() / ceiling);
            assert!((y - expected).abs() < 1e-6);
        }
    }
}
