use super::*;
use crate::effect::Effect;
use crate::param::ParamSet;
use std::f32::consts::TAU;

const RATE: f32 = 48_000.0;

fn tone(hz: f32, amplitude: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| amplitude * (TAU * hz * i as f32 / RATE).sin())
        .collect()
}
fn power(samples: &[f32]) -> f64 {
    samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / samples.len() as f64
}
fn amplitude_at(samples: &[f32], hz: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &x) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
        re += f64::from(x) * phase.cos();
        im += f64::from(x) * phase.sin();
    }
    2.0 * re.hypot(im) / samples.len() as f64
}

#[test]
fn unity_crossover_recombines_with_flat_magnitude() {
    for sample_rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        for bands in [BandCount::Two, BandCount::Three] {
            for frequency in [20.0, 80.0, 200.0, 600.0, 2_500.0, 7_000.0, 16_000.0] {
                let mut effect = BandSplit::default();
                effect.prepare(sample_rate, 1);
                effect.set_params(&BandSplitParams {
                    bands,
                    ..Default::default()
                });
                let mut input_power = 0.0_f64;
                let mut output_power = 0.0_f64;
                for n in 0..(sample_rate as usize * 2) {
                    let sample = (std::f64::consts::TAU * frequency * n as f64
                        / f64::from(sample_rate))
                    .sin() as f32
                        * 0.2;
                    let bands = effect.split_frame([sample, -sample]);
                    let output = bands.iter().map(|b| b[0]).sum::<f32>();
                    if n >= sample_rate as usize {
                        input_power += f64::from(sample).powi(2);
                        output_power += f64::from(output).powi(2);
                    }
                }
                let error_db = 10.0 * (output_power / input_power).log10();
                assert!(
                    error_db.abs() < 0.02,
                    "{sample_rate} {frequency} {bands:?}: {error_db} dB"
                );
            }
        }
    }
}

#[test]
fn low_band_detector_compresses_loud_bass_but_leaves_quiet_bass_and_highs() {
    let params = MultibandCompressorParams {
        low: BandDynamicsParams {
            threshold_db: -24.0,
            ratio: 8.0,
            attack_ms: 0.5,
            ..Default::default()
        },
        mid: BandDynamicsParams {
            threshold_db: 0.0,
            ratio: 1.0,
            ..Default::default()
        },
        high: BandDynamicsParams {
            threshold_db: 0.0,
            ratio: 1.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let render = |amp: f32| {
        let mut fx = MultibandCompressor::default();
        fx.prepare(RATE, 96_000);
        fx.set_params(&params);
        let mut left = tone(60.0, amp, 96_000);
        for (i, x) in left.iter_mut().enumerate() {
            *x += 0.08 * (TAU * 6_000.0 * i as f32 / RATE).sin();
        }
        let mut right = left.clone();
        fx.process(&mut left, &mut right);
        (
            amplitude_at(&left[48_000..], 60.0) / f64::from(amp),
            amplitude_at(&left[48_000..], 6_000.0),
        )
    };
    let loud = render(0.7);
    let quiet = render(0.02);
    assert!(loud.0 < quiet.0 * 0.4, "loud {loud:?}, quiet {quiet:?}");
    assert!(quiet.0 > 0.98);
    assert!(
        (loud.1 - quiet.1).abs() < 0.001,
        "independent high band: {loud:?} {quiet:?}"
    );
}

#[test]
fn bass_harmonics_add_energy_above_the_fundamental() {
    let render = |amount| {
        let mut fx = BassHarmonics::default();
        fx.prepare(RATE, 96_000);
        fx.set_params(&BassHarmonicsParams {
            amount,
            drive: 6.0,
            ..Default::default()
        });
        let mut l = tone(60.0, 0.5, 96_000);
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        l
    };
    let dry = render(0.0);
    let wet = render(1.0);
    let dry_harmonic = amplitude_at(&dry[48_000..], 120.0);
    let wet_harmonic = amplitude_at(&wet[48_000..], 120.0);
    assert!(
        wet_harmonic > 0.03 && wet_harmonic > dry_harmonic * 100.0,
        "{dry_harmonic} -> {wet_harmonic}"
    );
}

#[test]
fn exciter_generates_high_harmonics_without_exciting_bass() {
    let render = |frequency, mix| {
        let mut fx = Exciter::default();
        fx.prepare(RATE, 96_000);
        fx.set_params(&ExciterParams {
            frequency_hz: 1_000.0,
            drive: 5.0,
            mix,
            ..Default::default()
        });
        let mut l = tone(frequency, 0.5, 96_000);
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        l
    };
    let high = render(2_000.0, 1.0);
    let dry = render(2_000.0, 0.0);
    assert!(amplitude_at(&high[48_000..], 6_000.0) > 0.005);
    assert!(amplitude_at(&dry[48_000..], 6_000.0) < 0.0001);
    let bass_wet = render(60.0, 1.0);
    let bass_dry = render(60.0, 0.0);
    assert!(
        bass_wet[48_000..]
            .iter()
            .zip(&bass_dry[48_000..])
            .all(|(a, b)| (a - b).abs() < 1.0e-5)
    );
}

#[test]
fn transient_attack_boost_raises_first_milliseconds_relative_to_sustain() {
    let mut fx = TransientShaper::default();
    fx.prepare(RATE, 24_000);
    fx.set_params(&TransientShaperParams {
        attack: 0.8,
        ..Default::default()
    });
    let input = tone(2_000.0, 0.1, 24_000);
    let mut l = input.clone();
    let mut r = input.clone();
    fx.process(&mut l, &mut r);
    let first_gain = (power(&l[0..240]) / power(&input[0..240])).sqrt();
    let sustain_gain = (power(&l[12_000..]) / power(&input[12_000..])).sqrt();
    assert!(
        first_gain > sustain_gain * 2.0,
        "{first_gain}, {sustain_gain}"
    );
    assert!((sustain_gain - 1.0).abs() < 0.05);
}

#[test]
fn transient_split_outputs_are_complementary_and_independently_editable() {
    let mut fx = TransientSplit::default();
    fx.prepare(RATE, 1);
    let mut first = 0.0;
    let mut later = 0.0;
    for i in 0..24_000 {
        let input = [0.1, -0.2];
        let [attack, sustain] = fx.split_frame(input);
        for c in 0..2 {
            assert!((attack[c] + sustain[c] - input[c]).abs() < 1.0e-7);
        }
        if i < 240 {
            first += attack[0].abs();
        }
        if (12_000..12_240).contains(&i) {
            later += attack[0].abs();
        }
    }
    assert!(first > 10.0 * later);
    fx.reset();
    fx.set_params(&TransientSplitParams {
        sustain_gain_db: -60.0,
        ..Default::default()
    });
    let out = fx.split_frame([0.1, -0.1]);
    assert!(out[0][0].abs() > out[1][0].abs() * 100.0);
}

fn partition_check<E: Effect + Default>(initial: E::Params, edited: E::Params) {
    let mut whole = E::default();
    let mut pieces = E::default();
    whole.prepare(RATE, 4_096);
    pieces.prepare(RATE, 4_096);
    whole.set_params(&initial);
    pieces.set_params(&initial);
    let input: Vec<f32> = (0..8_192)
        .map(|i| {
            0.45 * (TAU * 61.0 * i as f32 / RATE).sin()
                + 0.3 * (TAU * 3_720.0 * i as f32 / RATE).sin()
        })
        .collect();
    let mut a = input.clone();
    let mut ar: Vec<f32> = input.iter().map(|x| -0.6 * x).collect();
    let mut b = a.clone();
    let mut br = ar.clone();
    let key: Vec<[f32; 2]> = input.iter().map(|&x| [x * 1.2, x * 0.5]).collect();
    for half in 0..2 {
        if half == 1 {
            whole.set_params(&edited);
            pieces.set_params(&edited);
        }
        let start = half * 4_096;
        let end = start + 4_096;
        whole.process_sidechain(
            &mut a[start..end],
            &mut ar[start..end],
            Some(&key[start..end]),
        );
        // Empty calls do not count as frames or consume fresh state.
        pieces.process(&mut [], &mut []);
        let mut offset = start;
        let sizes = [1, 31, 7, 512, 3, 79];
        let mut i = 0;
        while offset < end {
            let next = (offset + sizes[i % sizes.len()]).min(end);
            pieces.process_sidechain(
                &mut b[offset..next],
                &mut br[offset..next],
                Some(&key[offset..next]),
            );
            offset = next;
            i += 1;
        }
    }
    assert_eq!(a, b, "left partition mismatch: {}", E::Params::NAME);
    assert_eq!(ar, br, "right partition mismatch: {}", E::Params::NAME);
    whole.reset();
    pieces.reset();
    let mut al = [0.2; 512];
    let mut bl = al;
    let mut ar = [-0.1; 512];
    let mut br = ar;
    whole.process(&mut al, &mut ar);
    pieces.process(&mut bl, &mut br);
    assert_eq!(al, bl);
    assert_eq!(ar, br);
}

#[test]
fn all_processors_are_partition_invariant_during_control_changes_and_reset() {
    partition_check::<BandSplit>(
        BandSplitParams::default(),
        BandSplitParams {
            bands: BandCount::Two,
            low_gain_db: -8.0,
            high_gain_db: 7.0,
            low_crossover_hz: 900.0,
            high_crossover_hz: 4_500.0,
            ..Default::default()
        },
    );
    partition_check::<MultibandCompressor>(
        MultibandCompressorParams::default(),
        MultibandCompressorParams {
            sidechain: true,
            low: BandDynamicsParams {
                threshold_db: -32.0,
                ..Default::default()
            },
            low_crossover_hz: 800.0,
            ..Default::default()
        },
    );
    partition_check::<MultibandMaximizer>(
        MultibandMaximizerParams::default(),
        MultibandMaximizerParams {
            sidechain: true,
            input_gain_db: 12.0,
            low_ceiling_db: -12.0,
            ceiling_db: -6.0,
            ..Default::default()
        },
    );
    partition_check::<OneKnob>(OneKnobParams::default(), OneKnobParams { amount: 0.8 });
    partition_check::<BassHarmonics>(
        BassHarmonicsParams::default(),
        BassHarmonicsParams {
            amount: 0.9,
            cutoff_hz: 450.0,
            drive: 8.0,
        },
    );
    partition_check::<Exciter>(
        ExciterParams::default(),
        ExciterParams {
            mix: 0.8,
            color: 0.5,
            frequency_hz: 1_000.0,
            drive: 5.0,
        },
    );
    partition_check::<TransientShaper>(
        TransientShaperParams::default(),
        TransientShaperParams {
            attack: 0.9,
            sustain: -0.5,
            sensitivity: 2.0,
            listen: TransientListen::Attack,
        },
    );
    partition_check::<TransientSplit>(
        TransientSplitParams::default(),
        TransientSplitParams {
            transient_gain_db: 8.0,
            sustain_gain_db: -12.0,
            sensitivity: 0.5,
        },
    );
}

fn finite_check<E: Effect + Default>() {
    let mut fx = E::default();
    fx.prepare(f32::NAN, 512);
    let mut params = E::Params::default();
    for i in 0..E::Params::descriptors().len() {
        params.set(i, f32::NAN);
    }
    fx.set_params(&params);
    fx.set_tempo(f32::INFINITY);
    let mut l = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        -f32::MAX,
        0.0,
        1.0,
    ];
    let mut r = l;
    fx.process_sidechain(&mut l, &mut r, Some(&[[f32::NAN, f32::INFINITY]]));
    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    for (i, info) in E::Params::descriptors().iter().enumerate() {
        params.set(i, info.max);
    }
    fx.set_params(&params);
    for _ in 0..50 {
        let mut l = [100.0; 512];
        let mut r = [-100.0; 512];
        fx.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    }
}
#[test]
fn every_processor_produces_finite_output_with_damaged_audio_and_parameters() {
    finite_check::<BandSplit>();
    finite_check::<MultibandCompressor>();
    finite_check::<MultibandMaximizer>();
    finite_check::<OneKnob>();
    finite_check::<BassHarmonics>();
    finite_check::<Exciter>();
    finite_check::<TransientShaper>();
    finite_check::<TransientSplit>();
}

#[test]
fn maximizer_respects_ceiling_and_reports_both_lookahead_stages() {
    let mut fx = MultibandMaximizer::default();
    fx.prepare(RATE, 8_192);
    fx.set_params(&MultibandMaximizerParams {
        ceiling_db: -6.0,
        input_gain_db: 24.0,
        ..Default::default()
    });
    assert_eq!(fx.latency_samples(), 96);
    let mut l: Vec<f32> = (0..8_192)
        .map(|i| {
            if i % 193 == 0 {
                10.0
            } else {
                (i as f32 * 2.73).sin()
            }
        })
        .collect();
    let mut r: Vec<f32> = l.iter().map(|x| x * -0.75).collect();
    fx.process(&mut l, &mut r);
    let ceiling = crate::blocks::math::db_to_gain_exp(-6.0);
    assert!(l.iter().chain(&r).all(|x| x.abs() <= ceiling + 1.0e-5));
    assert!(l[..96].iter().all(|&x| x == 0.0));
}

#[test]
fn external_key_is_detector_only_and_missing_key_is_silent() {
    let params = MultibandCompressorParams {
        sidechain: true,
        ..Default::default()
    };
    let render = |key: Option<&[[f32; 2]]>| {
        let mut fx = MultibandCompressor::default();
        fx.prepare(RATE, 48_000);
        fx.set_params(&params);
        let mut l = tone(60.0, 0.3, 48_000);
        let mut r = l.clone();
        fx.process_sidechain(&mut l, &mut r, key);
        l
    };
    let key: Vec<[f32; 2]> = tone(60.0, 0.9, 48_000).iter().map(|&x| [x, x]).collect();
    let unkeyed = render(None);
    let keyed = render(Some(&key));
    assert!(power(&keyed[24_000..]) < power(&unkeyed[24_000..]) * 0.3);
    let mut fx = MultibandCompressor::default();
    fx.prepare(RATE, 48_000);
    fx.set_params(&params);
    let mut l = vec![0.0; 48_000];
    let mut r = l.clone();
    fx.process_sidechain(&mut l, &mut r, Some(&key));
    assert!(l.iter().chain(&r).all(|&x| x == 0.0));
}

fn schema_check<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned>() {
    let defaults = P::default();
    let legacy: P = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults, legacy);
    let json = serde_json::to_value(defaults).unwrap();
    for (i, info) in P::descriptors().iter().enumerate() {
        assert_eq!(defaults.get(i).unwrap(), info.default);
        let mut field = &json;
        for part in info.id.split('.') {
            field = &field[part];
        }
        assert!(!field.is_null(), "{}: {}", P::NAME, info.id);
        let mut edited = defaults;
        assert!(edited.set(i, info.max));
        assert!(edited.get(i).unwrap() <= info.max);
        assert_eq!(edited, edited.sanitized());
    }
    assert_eq!(defaults.get(P::descriptors().len()), None);
}
#[test]
fn defaults_descriptors_and_legacy_serde_defaults_match() {
    schema_check::<BandSplitParams>();
    schema_check::<BandDynamicsParams>();
    schema_check::<MultibandCompressorParams>();
    schema_check::<MultibandMaximizerParams>();
    schema_check::<OneKnobParams>();
    schema_check::<BassHarmonicsParams>();
    schema_check::<ExciterParams>();
    schema_check::<TransientShaperParams>();
    schema_check::<TransientSplitParams>();
}
