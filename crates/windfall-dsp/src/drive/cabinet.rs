//! Speaker cabinets as impulse responses, and the convolution that plays
//! them.
//!
//! # Where the impulse responses come from
//!
//! They are Windfall's own. Nothing here is a recording, a measurement of a
//! product or data taken from anywhere else. Each response is computed, when
//! a processor is prepared, from the [`Recipe`] of its cabinet in this file:
//! a unit impulse goes through a speaker resonance (second-order highpass),
//! a body peak, a presence peak and a fourth-order rolloff; two reflections
//! off the baffle are added; and a short tail of seeded noise, shaped the
//! same way, stands in for the box. The result is cut to 12 ms at most
//! (1,024 taps at most), faded out over its last quarter and scaled to unit
//! energy. The recipes are round numbers in the range guitar and bass
//! speakers occupy; none is fitted to an existing cabinet.
//!
//! Twelve milliseconds is short for the lowest resonances: the bass
//! cabinet's 48 Hz corner is only roughly kept by a response that long.
//!
//! Because the response is a function of this file and the sample rate, it
//! is the same on every machine and in every render.

use crate::blocks::biquad::{Biquad, BiquadCoeffs};
use crate::blocks::noise::Rng;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest response, in taps.
pub const MAX_CABINET_TAPS: usize = 1_024;
const RESPONSE_SECONDS: f32 = 0.012;

/// One of Windfall's cabinets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Cabinet {
    /// One twelve-inch speaker in an open-backed box: light lows, forward mids.
    #[default]
    Combo,
    /// Two twelve-inch speakers, open back: wider, brighter.
    Twin,
    /// Four twelve-inch speakers in a closed box: tight lows, dark top.
    Stack,
    /// One fifteen-inch speaker in a ported box, for bass.
    Bass,
}

impl Cabinet {
    pub const ALL: [Self; 4] = [Self::Combo, Self::Twin, Self::Stack, Self::Bass];

    fn recipe(self) -> Recipe {
        match self {
            Self::Combo => Recipe {
                resonance_hz: 110.0,
                resonance_q: 1.1,
                body_hz: 420.0,
                body_db: -2.5,
                presence_hz: 2_600.0,
                presence_db: 5.0,
                rolloff_hz: 4_800.0,
                rolloff_q: 0.9,
                reflections: [(0.42, -0.22), (0.95, 0.12)],
                box_level: 0.05,
                box_decay_ms: 3.0,
                seed: 11,
            },
            Self::Twin => Recipe {
                resonance_hz: 95.0,
                resonance_q: 0.9,
                body_hz: 350.0,
                body_db: -1.5,
                presence_hz: 3_200.0,
                presence_db: 6.0,
                rolloff_hz: 5_600.0,
                rolloff_q: 0.8,
                reflections: [(0.31, 0.2), (0.74, -0.16)],
                box_level: 0.07,
                box_decay_ms: 4.0,
                seed: 23,
            },
            Self::Stack => Recipe {
                resonance_hz: 82.0,
                resonance_q: 1.5,
                body_hz: 500.0,
                body_db: -4.0,
                presence_hz: 2_200.0,
                presence_db: 4.0,
                rolloff_hz: 4_000.0,
                rolloff_q: 1.1,
                reflections: [(0.58, -0.3), (1.4, 0.18)],
                box_level: 0.09,
                box_decay_ms: 5.5,
                seed: 37,
            },
            Self::Bass => Recipe {
                resonance_hz: 48.0,
                resonance_q: 1.3,
                body_hz: 250.0,
                body_db: -3.0,
                presence_hz: 1_100.0,
                presence_db: 3.0,
                rolloff_hz: 2_400.0,
                rolloff_q: 0.8,
                reflections: [(0.8, -0.2), (1.9, 0.1)],
                box_level: 0.06,
                box_decay_ms: 7.0,
                seed: 53,
            },
        }
    }
}

/// Everything a cabinet's response is computed from.
struct Recipe {
    /// The speaker's low resonance: corner and Q of a highpass.
    resonance_hz: f32,
    resonance_q: f32,
    /// A dip or bump in the low mids.
    body_hz: f32,
    body_db: f32,
    /// The cone's presence peak.
    presence_hz: f32,
    presence_db: f32,
    /// Where the cone stops following: two lowpass sections.
    rolloff_hz: f32,
    rolloff_q: f32,
    /// Baffle reflections as (delay in ms, gain).
    reflections: [(f32, f32); 2],
    /// Level and decay of the box's noise tail.
    box_level: f32,
    box_decay_ms: f32,
    seed: u32,
}

/// Taps a response has at `sample_rate`.
pub fn cabinet_taps(sample_rate: f32) -> usize {
    let rate = crate::balance::rate(sample_rate);
    ((RESPONSE_SECONDS * rate).round() as usize).clamp(16, MAX_CABINET_TAPS)
}

/// The impulse response of `cabinet` at `sample_rate`: [`cabinet_taps`]
/// values with unit energy. Allocates; the same arguments always give the
/// same response.
pub fn cabinet_response(cabinet: Cabinet, sample_rate: f32) -> Vec<f32> {
    let rate = crate::balance::rate(sample_rate);
    let taps = cabinet_taps(rate);
    let recipe = cabinet.recipe();
    let sections = [
        BiquadCoeffs::high_pass(recipe.resonance_hz, recipe.resonance_q, rate),
        BiquadCoeffs::peak(recipe.body_hz, 1.0, recipe.body_db, rate),
        BiquadCoeffs::peak(recipe.presence_hz, 1.4, recipe.presence_db, rate),
        BiquadCoeffs::low_pass(recipe.rolloff_hz, recipe.rolloff_q, rate),
        BiquadCoeffs::low_pass(recipe.rolloff_hz, 0.6, rate),
    ];
    let mut filters = [Biquad::default(); 5];
    let mut rng = Rng::new(recipe.seed);
    let decay = (-1.0 / (f64::from(recipe.box_decay_ms) * 0.001 * f64::from(rate))).exp();
    let mut box_gain = f64::from(recipe.box_level);
    let mut direct = vec![0.0_f64; taps];
    for (n, tap) in direct.iter_mut().enumerate() {
        // The cone's click, plus the box answering it with decaying noise.
        let mut value = if n == 0 { 1.0 } else { 0.0 };
        if n > 0 {
            value += f64::from(rng.bipolar()) * box_gain;
            box_gain *= decay;
        }
        for (filter, coeffs) in filters.iter_mut().zip(&sections) {
            value = filter.tick(coeffs, value);
        }
        *tap = value;
    }
    let mut response = direct.clone();
    for (delay_ms, gain) in recipe.reflections {
        let delay = (delay_ms * 0.001 * rate).round() as usize;
        if delay == 0 {
            continue;
        }
        for (n, tap) in response.iter_mut().enumerate().skip(delay) {
            *tap += f64::from(gain) * direct[n - delay];
        }
    }
    let fade_from = taps - taps / 4;
    for (n, tap) in response.iter_mut().enumerate().skip(fade_from) {
        let left = (taps - n) as f64 / (taps - fade_from) as f64;
        *tap *= left * left;
    }
    let energy: f64 = response.iter().map(|tap| tap * tap).sum();
    let scale = if energy > 1.0e-30 {
        1.0 / energy.sqrt()
    } else {
        1.0
    };
    response
        .into_iter()
        .map(|tap| {
            let tap = (tap * scale) as f32;
            if tap.is_finite() { tap } else { 0.0 }
        })
        .collect()
}

/// Every cabinet's response at one sample rate, stored newest tap last so
/// the convolution is a dot product of two slices.
pub(crate) struct CabinetBank {
    reversed: [Box<[f32]>; 4],
    taps: usize,
}

impl CabinetBank {
    /// A bank with no responses. Channels pass their input through.
    pub(crate) fn empty() -> Self {
        Self {
            reversed: std::array::from_fn(|_| Box::default()),
            taps: 0,
        }
    }

    /// Computes all four responses. Allocates: for `prepare`.
    pub(crate) fn new(sample_rate: f32) -> Self {
        let taps = cabinet_taps(sample_rate);
        Self {
            reversed: Cabinet::ALL.map(|cabinet| {
                let mut response = cabinet_response(cabinet, sample_rate);
                response.reverse();
                response.into_boxed_slice()
            }),
            taps,
        }
    }

    pub(crate) fn taps(&self) -> usize {
        self.taps
    }
}

/// One channel's memory of the samples a response reaches back over.
pub(crate) struct CabinetChannel {
    /// Twice the response length, each sample written twice, so the last
    /// `taps` samples are always one contiguous slice.
    ring: Box<[f32]>,
    at: usize,
}

impl CabinetChannel {
    pub(crate) fn new(taps: usize) -> Self {
        Self {
            ring: vec![0.0; taps * 2].into_boxed_slice(),
            at: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.at = 0;
    }

    /// Takes the next input sample without computing an output.
    #[inline]
    pub(crate) fn push(&mut self, input: f32) {
        let taps = self.ring.len() / 2;
        if taps == 0 {
            return;
        }
        self.ring[self.at] = input;
        self.ring[self.at + taps] = input;
        self.at = (self.at + 1) % taps;
    }

    /// The response of `cabinet` to everything pushed so far.
    #[inline]
    pub(crate) fn read(&self, bank: &CabinetBank, cabinet: Cabinet) -> f32 {
        let taps = self.ring.len() / 2;
        let response = &bank.reversed[cabinet as usize];
        if taps == 0 || response.len() != taps {
            return 0.0;
        }
        // After a push, `at` is the oldest sample and the newest sits at
        // `at + taps - 1`.
        let history = &self.ring[self.at..self.at + taps];
        let mut sum = 0.0_f32;
        for (tap, sample) in response.iter().zip(history) {
            sum += tap * sample;
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convolve(cabinet: Cabinet, rate: f32, input: &[f32]) -> Vec<f32> {
        let bank = CabinetBank::new(rate);
        let mut channel = CabinetChannel::new(bank.taps());
        input
            .iter()
            .map(|sample| {
                channel.push(*sample);
                channel.read(&bank, cabinet)
            })
            .collect()
    }

    /// Gain of a response at `hz`, from the response itself.
    fn gain_db(response: &[f32], hz: f64, rate: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, tap) in response.iter().enumerate() {
            let phase = std::f64::consts::TAU * hz * n as f64 / rate;
            re += f64::from(*tap) * phase.cos();
            im -= f64::from(*tap) * phase.sin();
        }
        10.0 * (re * re + im * im).max(1.0e-30).log10()
    }

    #[test]
    fn responses_are_bounded_deterministic_and_have_unit_energy() {
        for rate in [8_000.0, 44_100.0, 48_000.0, 96_000.0, 384_000.0] {
            let taps = cabinet_taps(rate);
            assert!((16..=MAX_CABINET_TAPS).contains(&taps));
            for cabinet in Cabinet::ALL {
                let response = cabinet_response(cabinet, rate);
                assert_eq!(response.len(), taps);
                assert!(response.iter().all(|tap| tap.is_finite()));
                let energy: f64 = response.iter().map(|tap| f64::from(*tap).powi(2)).sum();
                assert!((energy - 1.0).abs() < 1.0e-4, "{cabinet:?} at {rate}: {energy}");
                assert_eq!(response, cabinet_response(cabinet, rate));
                // Faded out: the last tap is far below the largest.
                let peak = response.iter().fold(0.0_f32, |peak, tap| peak.max(tap.abs()));
                assert!(response[taps - 1].abs() < peak * 1.0e-2);
            }
        }
        assert_eq!(cabinet_taps(48_000.0), 576);
        assert_eq!(cabinet_taps(384_000.0), MAX_CABINET_TAPS);
        assert_eq!(cabinet_taps(f32::NAN), 576);
    }

    #[test]
    fn each_cabinet_is_a_speaker_and_no_two_are_alike() {
        let rate = 48_000.0;
        let responses: Vec<_> = Cabinet::ALL
            .iter()
            .map(|cabinet| cabinet_response(*cabinet, rate))
            .collect();
        for (cabinet, response) in Cabinet::ALL.iter().zip(&responses) {
            let mids = gain_db(response, 1_000.0, 48_000.0);
            // Treble is far down, and so is everything under the resonance.
            assert!(gain_db(response, 12_000.0, 48_000.0) < mids - 24.0, "{cabinet:?} treble");
            assert!(gain_db(response, 20.0, 48_000.0) < mids - 3.0, "{cabinet:?} sub bass");
        }
        // The bass cabinet reaches lower and stops sooner than the combo.
        let (combo, bass) = (&responses[0], &responses[3]);
        assert!(
            gain_db(bass, 60.0, 48_000.0) - gain_db(bass, 1_000.0, 48_000.0)
                > gain_db(combo, 60.0, 48_000.0) - gain_db(combo, 1_000.0, 48_000.0) + 3.0
        );
        assert!(
            gain_db(bass, 4_000.0, 48_000.0) - gain_db(bass, 1_000.0, 48_000.0)
                < gain_db(combo, 4_000.0, 48_000.0) - gain_db(combo, 1_000.0, 48_000.0) - 3.0
        );
        for a in 0..responses.len() {
            for b in 0..a {
                let distance: f32 = responses[a]
                    .iter()
                    .zip(&responses[b])
                    .map(|(x, y)| (x - y).powi(2))
                    .sum();
                assert!(distance > 0.05, "cabinets {a} and {b} are alike: {distance}");
            }
        }
    }

    #[test]
    fn the_convolution_is_the_response_itself() {
        let rate = 48_000.0;
        for cabinet in Cabinet::ALL {
            let response = cabinet_response(cabinet, rate);
            let mut impulse = vec![0.0; response.len() + 40];
            impulse[7] = 1.0;
            let output = convolve(cabinet, rate, &impulse);
            assert!(output[..7].iter().all(|x| *x == 0.0));
            for (n, tap) in response.iter().enumerate() {
                assert_eq!(output[n + 7], *tap, "{cabinet:?} tap {n}");
            }
            assert!(output[response.len() + 7..].iter().all(|x| *x == 0.0));
        }
        // Against the sum written out directly, on noise.
        let mut rng = Rng::new(5);
        let input: Vec<f32> = (0..2_000).map(|_| rng.bipolar()).collect();
        let response = cabinet_response(Cabinet::Stack, rate);
        let output = convolve(Cabinet::Stack, rate, &input);
        for n in [0, 1, 575, 576, 577, 1_999] {
            let direct: f64 = response
                .iter()
                .enumerate()
                .filter(|(k, _)| *k <= n)
                .map(|(k, tap)| f64::from(*tap) * f64::from(input[n - k]))
                .sum();
            assert!((f64::from(output[n]) - direct).abs() < 1.0e-4, "sample {n}");
        }
    }

    #[test]
    fn an_unprepared_channel_is_silent_and_does_not_panic() {
        let bank = CabinetBank::empty();
        let mut channel = CabinetChannel::new(bank.taps());
        channel.push(1.0);
        assert_eq!(channel.read(&bank, Cabinet::Twin), 0.0);
        channel.reset();
        // A channel sized for another rate than the bank reads as silence.
        let bank = CabinetBank::new(48_000.0);
        let mut channel = CabinetChannel::new(32);
        channel.push(1.0);
        assert_eq!(channel.read(&bank, Cabinet::Combo), 0.0);
    }
}
