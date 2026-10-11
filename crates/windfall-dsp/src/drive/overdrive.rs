//! A guitar-style overdrive: treble emphasis, an asymmetric soft clipper
//! run above the host rate, a tone filter and a DC blocker.

use super::oversample::{DRIVE_LATENCY_SAMPLES, Kernels, Oversampler, Oversampling};
use crate::balance::{Controls, audio, rate};
use crate::blocks::dc::DcBlocker;
use crate::blocks::math::db_to_gain;
use crate::blocks::svf::{OnePoleFilter, Svf, SvfCoeffs};
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Gain the "hot" switch adds ahead of the clipper: 20 dB.
const HOT_GAIN: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct OverdriveParams {
    /// Input drive in dB, 0..36; default 18.
    pub drive_db: f32,
    /// Unequal clip ceilings, 0..1; default 0.7.
    pub asymmetry: f32,
    /// Treble emphasis before clipping, 0..1; default 0.5.
    pub emphasis: f32,
    /// Post lowpass cutoff in Hz, 200..18000; default 6000.
    pub tone_hz: f32,
    /// Output trim in dB, -24..12; default -6.
    pub output_db: f32,
    /// Corner of the treble emphasis in Hz, 200..4000; default 900.
    pub pre_band_hz: f32,
    /// Another 20 dB into the clipper; default off.
    pub hot: bool,
    /// Rate the clipper runs at; default four times the host rate.
    pub oversampling: Oversampling,
}
impl Default for OverdriveParams {
    fn default() -> Self {
        Self {
            drive_db: 18.0,
            asymmetry: 0.7,
            emphasis: 0.5,
            tone_hz: 6_000.0,
            output_db: -6.0,
            pre_band_hz: 900.0,
            hot: false,
            oversampling: Oversampling::X4,
        }
    }
}
param_set!(OverdriveParams, "Overdrive", {
    float [drive_db] "driveDb" "Drive" { Decibels, Linear, 0.0, 36.0, 18.0 }
    float [asymmetry] "asymmetry" "Asymmetry" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [emphasis] "emphasis" "Pre-emphasis" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [tone_hz] "toneHz" "Tone" { Hertz, Logarithmic, 200.0, 18_000.0, 6_000.0 }
    float [output_db] "outputDb" "Output" { Decibels, Linear, -24.0, 12.0, -6.0 }
    float [pre_band_hz] "preBandHz" "Pre band" { Hertz, Logarithmic, 200.0, 4_000.0, 900.0 }
    toggle [hot] "hot" "Hot" { false }
    choice [oversampling] "oversampling" "Quality" { Oversampling, X4, [
        Off "off" "Off", X2 "x2" "2x", X4 "x4" "4x", X8 "x8" "8x"
    ] }
});
impl OverdriveParams {
    /// Settled static clip curve; excludes the emphasis, tone and DC filters.
    pub fn transfer(&self, input: f32) -> f32 {
        let p = self.sanitized();
        let hot = if p.hot { HOT_GAIN } else { 1.0 };
        audio(
            super::asymmetric(audio(input) * db_to_gain(p.drive_db) * hot, p.asymmetry)
                * db_to_gain(p.output_db),
        )
    }
}

struct Channel {
    pre: OnePoleFilter,
    clip: Oversampler,
    post: Svf,
    dc: DcBlocker,
}
impl Channel {
    fn new(sample_rate: f32) -> Self {
        Self {
            pre: OnePoleFilter::default(),
            clip: Oversampler::default(),
            post: Svf::default(),
            dc: DcBlocker::new(5.0, sample_rate),
        }
    }
    fn reset(&mut self) {
        self.pre.reset();
        self.clip.reset();
        self.post.reset();
        self.dc.reset();
    }
    fn tick(
        &mut self,
        input: f32,
        values: &[f32; 7],
        pre: f32,
        post: &SvfCoeffs,
        kernels: Option<&Kernels>,
    ) -> f32 {
        let high = self.pre.high_pass(pre, input);
        let asymmetry = values[1];
        let driven = (input + 3.0 * values[2] * high) * values[0] * values[6];
        let clipped = self.clip.tick(kernels, audio(driven), |x| {
            f64::from(super::asymmetric(x as f32, asymmetry))
        });
        let filtered = self.post.tick(post, clipped).low;
        self.pre.flush();
        self.post.flush();
        audio(self.dc.tick(filtered) * values[4])
    }
}

pub struct Overdrive {
    controls: Controls<7>,
    channels: [Channel; 2],
    sample_rate: f32,
    kernels: Option<&'static Kernels>,
    quality: Oversampling,
    /// Nothing has been processed since `prepare` or `reset`.
    fresh: bool,
}
impl Default for Overdrive {
    fn default() -> Self {
        Self {
            controls: Controls::new(Self::values(&OverdriveParams::default())),
            channels: std::array::from_fn(|_| Channel::new(48_000.0)),
            sample_rate: 48_000.0,
            kernels: None,
            quality: Oversampling::default(),
            fresh: true,
        }
    }
}
impl Overdrive {
    fn values(p: &OverdriveParams) -> [f32; 7] {
        [
            db_to_gain(p.drive_db),
            p.asymmetry,
            p.emphasis,
            p.tone_hz,
            db_to_gain(p.output_db),
            p.pre_band_hz,
            if p.hot { HOT_GAIN } else { 1.0 },
        ]
    }
    fn apply_quality(&mut self) {
        for channel in &mut self.channels {
            channel.clip.set_quality(self.quality, self.fresh);
        }
    }
}
impl Effect for Overdrive {
    type Params = OverdriveParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.kernels = Some(Kernels::shared());
        self.channels = std::array::from_fn(|_| Channel::new(self.sample_rate));
        self.controls.prepare(self.sample_rate);
        self.fresh = true;
        self.apply_quality();
    }
    fn reset(&mut self) {
        self.controls.reset();
        for c in &mut self.channels {
            c.reset();
        }
        self.fresh = true;
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set(Self::values(&p));
        self.quality = p.oversampling;
        self.apply_quality();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        for (l, r) in left.iter_mut().zip(right) {
            let values = self.controls.tick();
            let post = SvfCoeffs::new(values[3], std::f32::consts::FRAC_1_SQRT_2, self.sample_rate);
            let pre = OnePoleFilter::coefficient(values[5], self.sample_rate);
            *l = self.channels[0].tick(audio(*l), &values, pre, &post, self.kernels);
            *r = self.channels[1].tick(audio(*r), &values, pre, &post, self.kernels);
        }
    }
    fn latency_samples(&self) -> usize {
        DRIVE_LATENCY_SAMPLES
    }
    fn tail_samples(&self) -> usize {
        (self.sample_rate * 2.0).ceil() as usize + DRIVE_LATENCY_SAMPLES
    }
}
