//! Independent signal measurements; deterministic original synthetic material.
use windfall_dsp::spectral::{PitchCorrect, PitchCorrectParams, PitchShift, PitchShiftMode, PitchShiftParams};
use windfall_dsp::Effect;

const RATES: [u32; 6] = [8_000, 44_100, 48_000, 96_000, 192_000, 384_000];

fn input(rate: u32, frequency: f64, seconds: usize) -> Vec<f32> {
    (0..rate as usize * seconds).map(|n|
        (0.3 * (std::f64::consts::TAU * frequency * n as f64 / rate as f64).sin()) as f32
    ).collect()
}

fn render<E: Effect + Default>(rate: u32, params: E::Params, source: &[f32]) -> Vec<f32> {
    let mut effect = E::default();
    effect.prepare(rate as f32, 257);
    effect.set_params(&params);
    let mut left = source.to_vec();
    let mut right: Vec<_> = source.iter().map(|s| -*s).collect();
    for (l, r) in left.chunks_mut(257).zip(right.chunks_mut(257)) { effect.process(l, r); }
    assert!(left.iter().chain(&right).all(|s| s.is_finite()));
    left
}

fn crossing_pitch(signal: &[f32], rate: u32) -> f64 {
    let crossings: Vec<_> = signal.windows(2).enumerate().filter_map(|(n, pair)| {
        (pair[0] <= 0.0 && pair[1] > 0.0).then(||
            n as f64 - pair[0] as f64 / (pair[1] as f64 - pair[0] as f64))
    }).collect();
    assert!(crossings.len() > 20);
    (crossings.len() - 1) as f64 * rate as f64 / (crossings.last().unwrap() - crossings[0])
}

fn power(signal: &[f32], rate: u32, hz: f64) -> f64 {
    let stride = (rate as usize / 8_000).max(1);
    let count = signal.len().div_ceil(stride);
    let mut real = 0.0;
    let mut imag = 0.0;
    for (n, value) in signal.iter().step_by(stride).enumerate() {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / (count - 1) as f64).cos();
        let phase = std::f64::consts::TAU * hz * (n * stride) as f64 / rate as f64;
        real += *value as f64 * window * phase.cos();
        imag += *value as f64 * window * phase.sin();
    }
    (real * real + imag * imag) / (count * count) as f64
}

#[test]
fn nonaligned_pitch_and_independent_formants_are_measured_in_both_modes_at_all_rates() {
    for rate in RATES {
        for mode in [PitchShiftMode::Mix, PitchShiftMode::Voice] {
            for hz in [83.7, 137.3, 233.1, 431.7, 719.9] {
                let source = input(rate, hz, 2);
                for semitones in [-7.0_f32, 5.0, 12.0] {
                    let output = render::<PitchShift>(rate, PitchShiftParams { semitones, mode, ..Default::default() }, &source);
                    let expected = hz * 2.0_f64.powf(semitones as f64 / 12.0);
                    let measured = crossing_pitch(&output[rate as usize..], rate);
                    let cents = 1200.0 * (measured / expected).log2();
                    assert!(cents.abs() < 25.0, "rate={rate} mode={mode:?} source={hz} shift={semitones} cents={cents}");
                }
            }
            // Original harmonic vocal-like envelope, deliberately not period aligned.
            let fundamental = 137.3;
            let source: Vec<f32> = (0..rate as usize).map(|n| (1..=20).map(|h| {
                let hz = fundamental * h as f64;
                let gain = (-((hz - 1500.0) / 450.0).powi(2)).exp();
                0.08 * gain * (std::f64::consts::TAU * hz * n as f64 / rate as f64).sin()
            }).sum::<f64>() as f32).collect();
            let centroid = |audio: &[f32]| {
                let (mut sum, mut weighted) = (0.0, 0.0);
                for h in 1..=24 {
                    let hz = fundamental * h as f64;
                    let energy = power(&audio[rate as usize / 2..], rate, hz);
                    sum += energy; weighted += energy * hz;
                }
                assert!(sum > 1e-8);
                weighted / sum
            };
            let plain = render::<PitchShift>(rate, PitchShiftParams { mode, ..Default::default() }, &source);
            let raised = render::<PitchShift>(rate, PitchShiftParams { mode, formant_semitones: 5.0, ..Default::default() }, &source);
            assert!(centroid(&raised) > centroid(&plain) * 1.1, "formants rate={rate} mode={mode:?}");
        }
    }
}

#[test]
fn correction_and_all_four_harmonies_cover_sixty_to_one_thousand_hz_at_all_rates() {
    for rate in RATES {
        for hz in [60.0, 61.7, 83.7, 137.3, 233.1, 431.7, 719.9, 997.3, 1000.0] {
            let source = input(rate, hz, 2);
            let params = PitchCorrectParams { speed_ms: 0.0, ..Default::default() };
            let base = render::<PitchCorrect>(rate, params, &source);
            let key = (69.0 + 12.0 * (hz / 440.0).log2()).round() as u8;
            let expected = 440.0 * 2.0_f64.powf((key as f64 - 69.0) / 12.0);
            let actual = crossing_pitch(&base[rate as usize..], rate);
            let cents = 1200.0 * (actual / expected).log2();
            assert!(cents.abs() < 25.0, "correction rate={rate} source={hz} cents={cents}");
            let notes = [key, key + 4, key + 7, key + 12];
            let mut effect = PitchCorrect::default();
            effect.prepare(rate as f32, 257);
            effect.set_params(&PitchCorrectParams { harmony_mix: 1.0, ..params });
            effect.set_harmony_notes(notes);
            let mut left = source.clone();
            let mut right: Vec<_> = source.iter().map(|s| -*s).collect();
            for (l, r) in left.chunks_mut(257).zip(right.chunks_mut(257)) { effect.process(l, r); }
            let isolated: Vec<_> = left.iter().zip(&base).skip(rate as usize).map(|(a, b)| a - b).collect();
            assert!(isolated.iter().all(|s| s.is_finite()));
            for note in notes {
                let target = 440.0 * 2.0_f64.powf((note as f64 - 69.0) / 12.0);
                let (cents, energy) = (-20..=20).map(|step| {
                    let cents = step as f64 * 7.5;
                    (cents, power(&isolated, rate, target * 2.0_f64.powf(cents / 1200.0)))
                }).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                assert!(energy > 1e-5, "missing harmony rate={rate} source={hz} note={note} power={energy}");
                assert!(cents.abs() <= 22.5, "harmony rate={rate} source={hz} note={note} cents={cents}");
            }
        }
    }
}

#[test]
fn expanded_pitch_windows_preserve_public_latency_and_callback_heap_contracts() {
    use windfall_dsp::{AnyEffect, EffectKind, EffectParams};
    for rate in RATES {
        for params in [EffectParams::PitchShift(PitchShiftParams::default()), EffectParams::PitchCorrect(PitchCorrectParams { harmony_mix: 1.0, ..Default::default() })] {
            let mut effect = AnyEffect::new(&params);
            effect.prepare(rate as f32, 257);
            effect.set_params(&params);
            assert_eq!(effect.latency_samples(), params.kind().max_latency_samples(rate as f32));
            let mut left = [0.1; 257]; let mut right = [-0.1; 257];
            assert_eq!(super::realtime::allocator_calls(|| {
                for _ in 0..64 { effect.process(&mut left, &mut right); }
                effect.reset(); effect.set_params(&params);
            }), 0);
            if params.kind() == EffectKind::PitchShift {
                let mut impulse = vec![0.0; effect.latency_samples() + 4];
                impulse[0] = 1.0;
                let output = render::<PitchShift>(rate, PitchShiftParams::default(), &impulse);
                assert_eq!(output.iter().position(|s| *s != 0.0), Some(effect.latency_samples()));
            }
        }
    }
}
