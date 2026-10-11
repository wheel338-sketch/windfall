use super::{audio, output, rate};
use crate::balance::Controls;
use crate::blocks::biquad::{Biquad, BiquadCoeffs};
use crate::blocks::math::smoothing_coefficient;
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const MAX_BANDS: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct VocoderParams {
    /// 16 = classic resolution, 32 = detailed resolution. Intermediate counts
    /// rebuild a real bank with that many filters, not a preset label.
    pub band_count: u8,
    /// Envelope-to-carrier band offset, -1..1 maps to +/-4 bands.
    pub color: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub gain: f32,
    pub mix: f32,
    /// Main carrier + key modulator, or main modulator + key carrier.
    pub key_is_carrier: bool,
    /// An explicitly selected built-in harmonic carrier, driven by carrier_hz.
    pub synth_carrier: bool,
    pub carrier_hz: f32,
}
impl Default for VocoderParams {
    fn default() -> Self {
        Self {
            band_count: 16,
            color: 0.0,
            attack_ms: 5.0,
            release_ms: 80.0,
            gain: 2.0,
            mix: 1.0,
            key_is_carrier: false,
            synth_carrier: false,
            carrier_hz: 110.0,
        }
    }
}
param_set!(VocoderParams, "Vocoder", {
    int [band_count] "bandCount" "Bands" { None, 16, 32, 16 }
    float [color] "color" "Color" { None, Linear, -1.0, 1.0, 0.0 }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Logarithmic, 0.1, 100.0, 5.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 5.0, 1000.0, 80.0 }
    float [gain] "gain" "Gain" { Gain, Linear, 0.0, 8.0, 2.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [key_is_carrier] "keyIsCarrier" "Key is carrier" { false }
    toggle [synth_carrier] "synthCarrier" "Synth carrier" { false }
    float [carrier_hz] "carrierHz" "Carrier pitch" { Hertz, Logarithmic, 40.0, 1000.0, 110.0 }
});

#[derive(Clone, Copy)]
struct Band {
    coeffs: BiquadCoeffs,
    carrier: [Biquad; 2],
    modulator: [Biquad; 2],
    envelope: f32,
}
impl Default for Band {
    fn default() -> Self {
        Self {
            coeffs: BiquadCoeffs::IDENTITY,
            carrier: [Biquad::default(); 2],
            modulator: [Biquad::default(); 2],
            envelope: 0.0,
        }
    }
}
struct Bank {
    bands: [Band; MAX_BANDS],
    count: usize,
}
impl Default for Bank {
    fn default() -> Self {
        Self {
            bands: [Band::default(); MAX_BANDS],
            count: 16,
        }
    }
}
impl Bank {
    fn configure(&mut self, count: usize, sample_rate: f32) {
        self.count = count;
        let high = 12_000.0_f32.min(sample_rate * 0.45);
        let spacing = (high / 80.0).powf(1.0 / (count - 1) as f32);
        let q = spacing.sqrt() / (spacing - 1.0);
        for (i, band) in self.bands.iter_mut().enumerate() {
            *band = Band::default();
            if i < count {
                band.coeffs =
                    BiquadCoeffs::band_pass(80.0 * spacing.powi(i as i32), q, sample_rate);
            }
        }
    }
    fn tick(
        &mut self,
        carrier: [f32; 2],
        key: [f32; 2],
        attack: f32,
        release: f32,
        color: f32,
    ) -> [f32; 2] {
        let mut carriers = [[0.0; 2]; MAX_BANDS];
        let mut envelopes = [0.0; MAX_BANDS];
        for (i, band) in self.bands.iter_mut().take(self.count).enumerate() {
            let mut detector = 0.0_f32;
            for ch in 0..2 {
                carriers[i][ch] =
                    band.carrier[ch].tick(&band.coeffs, f64::from(carrier[ch])) as f32;
                let modulated = band.modulator[ch].tick(&band.coeffs, f64::from(key[ch])) as f32;
                // Stereo linked rectification doesn't cancel an antiphase key.
                detector = detector.max(modulated.abs());
                band.carrier[ch].flush();
                band.modulator[ch].flush();
            }
            let coefficient = if detector > band.envelope {
                attack
            } else {
                release
            };
            band.envelope += (detector - band.envelope) * coefficient;
            if band.envelope < 1.0e-8 {
                band.envelope = 0.0;
            }
            envelopes[i] = band.envelope;
        }
        let mut result = [0.0; 2];
        for (i, carrier_band) in carriers.iter().take(self.count).enumerate() {
            let source = (i as f32 - color * 4.0).clamp(0.0, (self.count - 1) as f32);
            let a = source as usize;
            let b = (a + 1).min(self.count - 1);
            let envelope = envelopes[a] + (envelopes[b] - envelopes[a]) * source.fract();
            for ch in 0..2 {
                result[ch] += carrier_band[ch] * envelope;
            }
        }
        result.map(output)
    }
}

/// The main input is always the carrier. Optional sidechain is the modulator;
/// Missing key is silent, while a short key is zero past its supplied frames.
pub struct Vocoder {
    params: VocoderParams,
    banks: [Bank; 2],
    active: usize,
    bank_mix: LinearRamp,
    controls: Controls<6>,
    synth_phase: f64,
    sample_rate: f32,
    fresh: bool,
}
impl Default for Vocoder {
    fn default() -> Self {
        let mut effect = Self {
            params: VocoderParams::default(),
            banks: [Bank::default(), Bank::default()],
            active: 0,
            bank_mix: LinearRamp::new(0.0),
            controls: Controls::new([0.0, 5.0, 80.0, 2.0, 1.0, 110.0]),
            synth_phase: 0.0,
            sample_rate: 48_000.0,
            fresh: true,
        };
        effect.prepare(48_000.0, 1);
        effect
    }
}
impl Vocoder {
    fn start_pending_bank(&mut self) {
        if self.bank_mix.is_settled()
            && self.banks[self.active].count != usize::from(self.params.band_count)
        {
            self.active = 1 - self.active;
            self.banks[self.active]
                .configure(usize::from(self.params.band_count), self.sample_rate);
            self.bank_mix.set_target(
                self.active as f32,
                (self.sample_rate * 0.005).round() as u32,
            );
        }
    }
}
impl Effect for Vocoder {
    type Params = VocoderParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        for bank in &mut self.banks {
            bank.configure(usize::from(self.params.band_count), self.sample_rate);
        }
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        for bank in &mut self.banks {
            bank.configure(usize::from(self.params.band_count), self.sample_rate);
        }
        self.bank_mix.snap(self.active as f32);
        self.controls.reset();
        self.fresh = true;
        self.synth_phase = 0.0;
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set([
            p.color,
            p.attack_ms,
            p.release_ms,
            p.gain,
            p.mix,
            p.carrier_hz,
        ]);
        if p.band_count != self.params.band_count && self.fresh {
            for bank in &mut self.banks {
                bank.configure(usize::from(p.band_count), self.sample_rate);
            }
        }
        self.params = p;
        self.start_pending_bank();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        for (i, (l, r)) in left.iter_mut().zip(right).enumerate() {
            self.fresh = false;
            self.start_pending_bank();
            let main = [audio(*l), audio(*r)];
            let key = key
                .and_then(|frames| frames.get(i))
                .copied()
                .unwrap_or([0.0; 2])
                .map(audio);
            let [color, attack, release, gain, mix, carrier_hz] = self.controls.tick();
            let (mut carrier, modulator) = if self.params.key_is_carrier {
                (key, main)
            } else {
                (main, key)
            };
            if self.params.synth_carrier {
                // Bounded additive saw, bandlimited by omitting harmonics above
                // Nyquist. This is a selected synth, never an implicit modulator.
                let mut value = 0.0;
                let harmonics = (self.sample_rate * 0.45 / carrier_hz).floor().min(64.0) as usize;
                for harmonic in 1..=harmonics {
                    value += (self.synth_phase * harmonic as f64).sin() as f32 / harmonic as f32;
                }
                carrier = [value * 0.4; 2];
            }
            self.synth_phase = (self.synth_phase
                + std::f64::consts::TAU * f64::from(carrier_hz / self.sample_rate))
            .rem_euclid(std::f64::consts::TAU);
            let attack = smoothing_coefficient(attack, self.sample_rate);
            let release = smoothing_coefficient(release, self.sample_rate);
            let a = self.banks[0].tick(carrier, modulator, attack, release, color);
            let b = self.banks[1].tick(carrier, modulator, attack, release, color);
            let blend = self.bank_mix.tick();
            *l = output(main[0] * (1.0 - mix) + (a[0] + (b[0] - a[0]) * blend) * gain * mix);
            *r = output(main[1] * (1.0 - mix) + (a[1] + (b[1] - a[1]) * blend) * gain * mix);
        }
    }
    // Causal filters/envelopes add phase/attack response, no transport buffer.
    fn latency_samples(&self) -> usize {
        0
    }
    fn tail_samples(&self) -> usize {
        // Include both a previous slow release and the current target ramp.
        (self.sample_rate * 32.0).ceil() as usize
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
}
