use super::graphic::{
    MORPH_CURVE_BANDS, MORPH_CURVE_FREQUENCIES_HZ, MORPH_CURVE_Q, MORPH_CURVES, MorphCurveParams,
    interpolated,
};
use super::stage::{Stage, input, output, peak, rate, transition_samples};
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MorphBandParams {
    /// Center in Hz, 20 to 20,000; clamped below Nyquist during design.
    pub frequency_hz: f32,
    /// Bell gain in dB, -18 to 18; default zero.
    pub gain_db: f32,
    /// Band Q, 0.1 to 10; default 1.
    pub q: f32,
}
impl Default for MorphBandParams {
    fn default() -> Self {
        Self {
            frequency_hz: 1_000.0,
            gain_db: 0.0,
            q: 1.0,
        }
    }
}
param_set!(MorphBandParams, "Morph Band", {
    float [frequency_hz] "frequencyHz" "Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [gain_db] "gainDb" "Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [q] "q" "Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MorphSnapshotParams {
    /// Four stored bell bands, with default centers 150, 600, 2500, 10000 Hz.
    pub bands: [MorphBandParams; 4],
}
impl Default for MorphSnapshotParams {
    fn default() -> Self {
        Self {
            bands: [150.0, 600.0, 2_500.0, 10_000.0].map(|frequency_hz| MorphBandParams {
                frequency_hz,
                ..MorphBandParams::default()
            }),
        }
    }
}
param_set!(MorphSnapshotParams, "Morph Snapshot", {
    float [bands[0].frequency_hz] "bands.0.frequencyHz" "Band 1 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 150.0 }
    float [bands[0].gain_db] "bands.0.gainDb" "Band 1 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [bands[0].q] "bands.0.q" "Band 1 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [bands[1].frequency_hz] "bands.1.frequencyHz" "Band 2 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 600.0 }
    float [bands[1].gain_db] "bands.1.gainDb" "Band 2 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [bands[1].q] "bands.1.q" "Band 2 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [bands[2].frequency_hz] "bands.2.frequencyHz" "Band 3 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 2_500.0 }
    float [bands[2].gain_db] "bands.2.gainDb" "Band 3 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [bands[2].q] "bands.2.q" "Band 3 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [bands[3].frequency_hz] "bands.3.frequencyHz" "Band 4 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 10_000.0 }
    float [bands[3].gain_db] "bands.3.gainDb" "Band 4 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [bands[3].q] "bands.3.q" "Band 4 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MorphEqParams {
    pub snapshot_a: MorphSnapshotParams,
    pub snapshot_b: MorphSnapshotParams,
    /// 0 selects A, 1 selects B. Hz and Q interpolate logarithmically, dB linearly.
    /// The same travel walks the stored graphic curves: the first at 0, the
    /// last in use at 1, the others evenly between.
    pub morph: f32,
    /// Four stored graphic curves of fifteen fixed bands, A to D; all flat
    /// by default.
    pub curves: [MorphCurveParams; MORPH_CURVES],
    /// How many of the curves the morph passes through, 2 to 4; default 2,
    /// which morphs from A to B.
    pub curve_count: u8,
}
impl Default for MorphEqParams {
    fn default() -> Self {
        Self {
            snapshot_a: MorphSnapshotParams::default(),
            snapshot_b: MorphSnapshotParams::default(),
            morph: 0.0,
            curves: [MorphCurveParams::default(); MORPH_CURVES],
            curve_count: 2,
        }
    }
}
param_set!(MorphEqParams, "Morph EQ", {
    float [snapshot_a.bands[0].frequency_hz] "snapshotA.bands.0.frequencyHz" "A Band 1 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 150.0 }
    float [snapshot_a.bands[0].gain_db] "snapshotA.bands.0.gainDb" "A Band 1 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_a.bands[0].q] "snapshotA.bands.0.q" "A Band 1 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_a.bands[1].frequency_hz] "snapshotA.bands.1.frequencyHz" "A Band 2 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 600.0 }
    float [snapshot_a.bands[1].gain_db] "snapshotA.bands.1.gainDb" "A Band 2 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_a.bands[1].q] "snapshotA.bands.1.q" "A Band 2 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_a.bands[2].frequency_hz] "snapshotA.bands.2.frequencyHz" "A Band 3 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 2_500.0 }
    float [snapshot_a.bands[2].gain_db] "snapshotA.bands.2.gainDb" "A Band 3 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_a.bands[2].q] "snapshotA.bands.2.q" "A Band 3 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_a.bands[3].frequency_hz] "snapshotA.bands.3.frequencyHz" "A Band 4 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 10_000.0 }
    float [snapshot_a.bands[3].gain_db] "snapshotA.bands.3.gainDb" "A Band 4 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_a.bands[3].q] "snapshotA.bands.3.q" "A Band 4 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_b.bands[0].frequency_hz] "snapshotB.bands.0.frequencyHz" "B Band 1 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 150.0 }
    float [snapshot_b.bands[0].gain_db] "snapshotB.bands.0.gainDb" "B Band 1 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_b.bands[0].q] "snapshotB.bands.0.q" "B Band 1 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_b.bands[1].frequency_hz] "snapshotB.bands.1.frequencyHz" "B Band 2 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 600.0 }
    float [snapshot_b.bands[1].gain_db] "snapshotB.bands.1.gainDb" "B Band 2 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_b.bands[1].q] "snapshotB.bands.1.q" "B Band 2 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_b.bands[2].frequency_hz] "snapshotB.bands.2.frequencyHz" "B Band 3 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 2_500.0 }
    float [snapshot_b.bands[2].gain_db] "snapshotB.bands.2.gainDb" "B Band 3 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_b.bands[2].q] "snapshotB.bands.2.q" "B Band 3 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [snapshot_b.bands[3].frequency_hz] "snapshotB.bands.3.frequencyHz" "B Band 4 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 10_000.0 }
    float [snapshot_b.bands[3].gain_db] "snapshotB.bands.3.gainDb" "B Band 4 Gain" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [snapshot_b.bands[3].q] "snapshotB.bands.3.q" "B Band 4 Q" { Ratio, Logarithmic, 0.1, 10.0, 1.0 }
    float [morph] "morph" "Morph" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [curves[0].gains_db[0]] "curves.0.gainsDb.0" "Curve A 25 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[1]] "curves.0.gainsDb.1" "Curve A 40 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[2]] "curves.0.gainsDb.2" "Curve A 63 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[3]] "curves.0.gainsDb.3" "Curve A 100 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[4]] "curves.0.gainsDb.4" "Curve A 160 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[5]] "curves.0.gainsDb.5" "Curve A 250 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[6]] "curves.0.gainsDb.6" "Curve A 400 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[7]] "curves.0.gainsDb.7" "Curve A 630 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[8]] "curves.0.gainsDb.8" "Curve A 1000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[9]] "curves.0.gainsDb.9" "Curve A 1600 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[10]] "curves.0.gainsDb.10" "Curve A 2500 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[11]] "curves.0.gainsDb.11" "Curve A 4000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[12]] "curves.0.gainsDb.12" "Curve A 6300 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[13]] "curves.0.gainsDb.13" "Curve A 10000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[0].gains_db[14]] "curves.0.gainsDb.14" "Curve A 16000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[0]] "curves.1.gainsDb.0" "Curve B 25 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[1]] "curves.1.gainsDb.1" "Curve B 40 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[2]] "curves.1.gainsDb.2" "Curve B 63 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[3]] "curves.1.gainsDb.3" "Curve B 100 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[4]] "curves.1.gainsDb.4" "Curve B 160 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[5]] "curves.1.gainsDb.5" "Curve B 250 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[6]] "curves.1.gainsDb.6" "Curve B 400 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[7]] "curves.1.gainsDb.7" "Curve B 630 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[8]] "curves.1.gainsDb.8" "Curve B 1000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[9]] "curves.1.gainsDb.9" "Curve B 1600 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[10]] "curves.1.gainsDb.10" "Curve B 2500 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[11]] "curves.1.gainsDb.11" "Curve B 4000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[12]] "curves.1.gainsDb.12" "Curve B 6300 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[13]] "curves.1.gainsDb.13" "Curve B 10000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[1].gains_db[14]] "curves.1.gainsDb.14" "Curve B 16000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[0]] "curves.2.gainsDb.0" "Curve C 25 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[1]] "curves.2.gainsDb.1" "Curve C 40 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[2]] "curves.2.gainsDb.2" "Curve C 63 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[3]] "curves.2.gainsDb.3" "Curve C 100 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[4]] "curves.2.gainsDb.4" "Curve C 160 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[5]] "curves.2.gainsDb.5" "Curve C 250 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[6]] "curves.2.gainsDb.6" "Curve C 400 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[7]] "curves.2.gainsDb.7" "Curve C 630 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[8]] "curves.2.gainsDb.8" "Curve C 1000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[9]] "curves.2.gainsDb.9" "Curve C 1600 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[10]] "curves.2.gainsDb.10" "Curve C 2500 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[11]] "curves.2.gainsDb.11" "Curve C 4000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[12]] "curves.2.gainsDb.12" "Curve C 6300 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[13]] "curves.2.gainsDb.13" "Curve C 10000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[2].gains_db[14]] "curves.2.gainsDb.14" "Curve C 16000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[0]] "curves.3.gainsDb.0" "Curve D 25 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[1]] "curves.3.gainsDb.1" "Curve D 40 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[2]] "curves.3.gainsDb.2" "Curve D 63 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[3]] "curves.3.gainsDb.3" "Curve D 100 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[4]] "curves.3.gainsDb.4" "Curve D 160 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[5]] "curves.3.gainsDb.5" "Curve D 250 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[6]] "curves.3.gainsDb.6" "Curve D 400 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[7]] "curves.3.gainsDb.7" "Curve D 630 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[8]] "curves.3.gainsDb.8" "Curve D 1000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[9]] "curves.3.gainsDb.9" "Curve D 1600 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[10]] "curves.3.gainsDb.10" "Curve D 2500 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[11]] "curves.3.gainsDb.11" "Curve D 4000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[12]] "curves.3.gainsDb.12" "Curve D 6300 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[13]] "curves.3.gainsDb.13" "Curve D 10000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    float [curves[3].gains_db[14]] "curves.3.gainsDb.14" "Curve D 16000 Hz" { Decibels, Linear, -18.0, 18.0, 0.0 }
    int [curve_count] "curveCount" "Curves" { None, 2, 4, 2 }
});

impl MorphEqParams {
    /// The four designed bands at the requested morph position, without smoothing.
    pub fn interpolated_bands(&self) -> [MorphBandParams; 4] {
        let p = self.sanitized();
        if p.morph == 0.0 {
            return p.snapshot_a.bands;
        }
        if p.morph == 1.0 {
            return p.snapshot_b.bands;
        }
        let logarithmic = |a: f32, b: f32| (a.ln() + p.morph * (b.ln() - a.ln())).exp();
        std::array::from_fn(|i| {
            let a = p.snapshot_a.bands[i];
            let b = p.snapshot_b.bands[i];
            MorphBandParams {
                frequency_hz: logarithmic(a.frequency_hz, b.frequency_hz),
                gain_db: a.gain_db + p.morph * (b.gain_db - a.gain_db),
                q: logarithmic(a.q, b.q),
            }
        })
    }

    /// The fifteen graphic band gains at the requested morph position,
    /// without smoothing. See [`MorphEqParams::morph`].
    pub fn interpolated_curve(&self) -> [f32; MORPH_CURVE_BANDS] {
        let p = self.sanitized();
        interpolated(&p.curves, p.curve_count, p.morph)
    }
}

/// A graphic equalizer of fifteen fixed bands that morphs between up to four
/// stored curves, followed by four movable bells whose shapes interpolate
/// between two stored snapshots on the same morph control. There is no
/// spectral analysis or automatic curve capture.
pub struct MorphEq {
    sample_rate: f32,
    params: MorphEqParams,
    stages: [Stage; 4],
    curve: [Stage; MORPH_CURVE_BANDS],
    fresh: bool,
}
impl Default for MorphEq {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: MorphEqParams::default(),
            stages: [Stage::default(); 4],
            curve: [Stage::default(); MORPH_CURVE_BANDS],
            fresh: true,
        }
    }
}
impl MorphEq {
    fn apply(&mut self) {
        let samples = transition_samples(self.sample_rate, self.fresh);
        for (stage, band) in self.stages.iter_mut().zip(self.params.interpolated_bands()) {
            stage.set(
                peak(band.frequency_hz, band.gain_db, band.q, self.sample_rate),
                samples,
            );
        }
        let gains = self.params.interpolated_curve();
        for (band, stage) in self.curve.iter_mut().enumerate() {
            stage.set(
                peak(
                    MORPH_CURVE_FREQUENCIES_HZ[band],
                    gains[band],
                    MORPH_CURVE_Q,
                    self.sample_rate,
                ),
                samples,
            );
        }
    }
}
impl Effect for MorphEq {
    type Params = MorphEqParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.fresh = true;
        self.apply();
        for stage in self.stages.iter_mut().chain(&mut self.curve) {
            stage.reset();
        }
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            self.fresh = false;
            let mut frame = [input(*l), input(*r)];
            for stage in self.curve.iter_mut().chain(&mut self.stages) {
                frame = stage.tick(frame);
            }
            *l = output(frame[0]);
            *r = output(frame[1]);
        }
    }
    fn tail_samples(&self) -> usize {
        self.stages
            .iter()
            .chain(&self.curve)
            .map(Stage::tail_samples)
            .sum()
    }
}
