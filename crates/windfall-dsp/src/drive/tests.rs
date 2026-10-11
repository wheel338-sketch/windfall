use super::*;
use crate::effect::Effect;
use crate::param::{ParamKind, ParamSet};

const RATE: f32 = 48_000.0;

fn sine(length: usize, amplitude: f32) -> Vec<f32> {
    (0..length)
        .map(|n| amplitude * (std::f32::consts::TAU * 440.0 * n as f32 / RATE).sin())
        .collect()
}

fn render<E: Effect + Default>(params: E::Params, input: &[f32]) -> Vec<f32> {
    let mut effect = E::default();
    effect.prepare(RATE, input.len());
    effect.set_params(&params);
    let mut left = input.to_vec();
    let mut right: Vec<_> = input.iter().map(|x| -0.7 * x).collect();
    effect.process(&mut left, &mut right);
    assert!(left.iter().chain(&right).all(|x| x.is_finite()));
    left
}

fn difference(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max)
}

/// `input` as a processor with the drive latency and no other change
/// would put it out.
fn late(input: &[f32]) -> Vec<f32> {
    let mut output = vec![0.0; DRIVE_LATENCY_SAMPLES];
    output.extend_from_slice(&input[..input.len() - DRIVE_LATENCY_SAMPLES]);
    output
}

#[test]
fn identity_waveshape_is_unity() {
    let input = sine(4_096, 0.35);
    // At the host rate the identity curve is the input itself, late.
    let host_rate = WaveshaperParams {
        oversampling: Oversampling::Off,
        ..Default::default()
    };
    assert_eq!(render::<Waveshaper>(host_rate, &input), late(&input));
    let mut tiny = [0.0; 40];
    tiny[0] = -1.0e-8;
    tiny[2] = 1.0e-8;
    let output = render::<Waveshaper>(host_rate, &tiny);
    assert_eq!(output[DRIVE_LATENCY_SAMPLES], -1.0e-8);
    assert_eq!(output[DRIVE_LATENCY_SAMPLES + 1], 0.0);
    assert_eq!(output[DRIVE_LATENCY_SAMPLES + 2], 1.0e-8);
    // Oversampled, it is the input through two flat filters.
    let output = render::<Waveshaper>(WaveshaperParams::default(), &input);
    assert!(difference(&output[512..], &late(&input)[512..]) < 2.0e-3);
}

#[test]
fn bent_curve_changes_amplitude_and_interpolates_knots() {
    let params = WaveshaperParams {
        curve: IDENTITY_CURVE.map(|x| x * 0.5),
        oversampling: Oversampling::Off,
        ..Default::default()
    };
    let input = sine(4_096, 0.35);
    let output = render::<Waveshaper>(params, &input);
    let input = late(&input);
    for (a, b) in output.iter().zip(&input) {
        assert!((a - b * 0.5).abs() < 2.0e-7);
    }
    assert!(difference(&input, &output) > 0.17);
    let mut p = WaveshaperParams::default();
    p.curve[17] = 0.5;
    assert!((p.transfer(0.03125) - 0.25).abs() < 1.0e-6);
    assert_eq!(p.transfer(2.0), 1.0);
    assert_eq!(p.transfer(-2.0), -1.0);
}

#[test]
fn overdrive_has_unequal_positive_and_negative_peaks() {
    let p = OverdriveParams {
        asymmetry: 1.0,
        ..Default::default()
    };
    assert!((p.transfer(0.8).abs() - p.transfer(-0.8).abs()).abs() > 0.2);
    let output = render::<Overdrive>(p, &sine(16_384, 0.8));
    let steady = &output[8_192..];
    let positive = steady.iter().copied().fold(0.0_f32, f32::max);
    let negative = -steady.iter().copied().fold(0.0_f32, f32::min);
    assert!(
        (positive - negative).abs() > 0.02,
        "peaks {positive}, {negative}"
    );
}

#[test]
fn all_eleven_guitar_models_have_distinct_sine_responses() {
    let input = sine(8_192, 0.65);
    let responses: Vec<_> = GuitarModel::ALL
        .iter()
        .map(|&model| {
            render::<GuitarRack>(
                GuitarRackParams {
                    model,
                    ..Default::default()
                },
                &input,
            )
        })
        .collect();
    for i in 0..responses.len() {
        for j in 0..i {
            assert!(
                difference(&responses[i], &responses[j]) > 0.01,
                "equal responses: {:?}, {:?}",
                GuitarModel::ALL[i],
                GuitarModel::ALL[j]
            );
        }
    }
}

#[test]
fn octave_rectification_moves_energy_to_the_second_harmonic() {
    let output = render::<GuitarRack>(
        GuitarRackParams {
            model: GuitarModel::OctaveUp,
            ..Default::default()
        },
        &sine(24_000, 0.5),
    );
    fn amplitude(input: &[f32], start: usize, hz: f32) -> f32 {
        let mut re = 0.0;
        let mut im = 0.0;
        for (n, x) in input.iter().enumerate().skip(start) {
            let phase = std::f32::consts::TAU * hz * n as f32 / RATE;
            re += x * phase.cos();
            im += x * phase.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / (input.len() - start) as f32
    }
    let fundamental = amplitude(&output, 12_000, 440.0);
    let octave = amplitude(&output, 12_000, 880.0);
    assert!(octave > 0.15 && octave > fundamental * 50.0);
}

#[test]
fn spring_has_a_delayed_tail_and_cabinet_attenuates_treble() {
    let mut impulse = vec![0.0; 8_192];
    impulse[0] = 0.5;
    let slap = render::<GuitarRack>(
        GuitarRackParams {
            model: GuitarModel::SpringSlap,
            oversampling: Oversampling::Off,
            ..Default::default()
        },
        &impulse,
    );
    assert!(slap[..DRIVE_LATENCY_SAMPLES].iter().all(|x| *x == 0.0));
    assert!(
        slap[DRIVE_LATENCY_SAMPLES + 1..DRIVE_LATENCY_SAMPLES + 1_776]
            .iter()
            .all(|x| *x == 0.0)
    );
    assert!(
        slap[DRIVE_LATENCY_SAMPLES + 1_776..]
            .iter()
            .any(|x| x.abs() > 0.001)
    );
    let high: Vec<_> = (0..8_192)
        .map(|n| 0.3 * (std::f32::consts::TAU * 12_000.0 * n as f32 / RATE).sin())
        .collect();
    let cab = render::<GuitarRack>(
        GuitarRackParams {
            model: GuitarModel::Cabinet,
            ..Default::default()
        },
        &high,
    );
    let power = |xs: &[f32]| xs[2_048..].iter().map(|x| x * x).sum::<f32>();
    assert!(power(&cab) < power(&high) * 0.01);
}

#[test]
fn gate_closes_on_quiet_audio_and_compressor_squashes_loud_audio() {
    let quiet = sine(8_192, 0.001);
    let gated = render::<GuitarRack>(
        GuitarRackParams {
            model: GuitarModel::Gate,
            ..Default::default()
        },
        &quiet,
    );
    assert!(gated.iter().all(|x| *x == 0.0));
    let loud = sine(8_192, 0.9);
    let compressed = render::<GuitarRack>(
        GuitarRackParams {
            model: GuitarModel::CompressorSquash,
            ..Default::default()
        },
        &loud,
    );
    assert!(compressed[4_096..].iter().all(|x| x.abs() < 0.4));
}

#[test]
fn drive_chain_order_changes_the_result_and_bypass_is_unity() {
    let input = sine(4_096, 0.65);
    assert_eq!(
        render::<DriveChain>(
            DriveChainParams {
                oversampling: Oversampling::Off,
                ..Default::default()
            },
            &input
        ),
        {
            let mut delayed = vec![0.0; CHAIN_LATENCY_SAMPLES];
            delayed.extend_from_slice(&input[..input.len() - CHAIN_LATENCY_SAMPLES]);
            delayed
        }
    );
    let mut first = DriveChainParams {
        oversampling: Oversampling::Off,
        stages: [
            DriveStageParams {
                model: DriveStage::Overdrive,
                gain_db: 12.0,
                ..Default::default()
            },
            DriveStageParams {
                model: DriveStage::Waveshape,
                gain_db: 3.0,
                ..Default::default()
            },
            DriveStageParams::default(),
        ],
        ..Default::default()
    };
    let a = render::<DriveChain>(first, &input);
    first.stages.swap(0, 1);
    let b = render::<DriveChain>(first, &input);
    assert!(difference(&a, &b) > 0.1);
    for (x, y) in input.iter().zip(&b[CHAIN_LATENCY_SAMPLES..]) {
        assert!((first.transfer(*x) - y).abs() < 1.0e-6);
    }
}

fn partition_check<E: Effect + Default>(initial: E::Params, target: E::Params) {
    fn run<E: Effect + Default>(
        initial: E::Params,
        target: E::Params,
        pattern: &[usize],
    ) -> (Vec<f32>, Vec<f32>) {
        let mut effect = E::default();
        effect.prepare(RATE, 4_096);
        effect.set_params(&initial);
        let mut l = sine(4_096, 0.6);
        let mut r: Vec<_> = l.iter().map(|x| -0.4 * x).collect();
        let mut at = 0;
        let mut block = 0;
        for end in [701, 809, 4_096] {
            while at < end {
                let next = (at + pattern[block % pattern.len()]).min(end);
                effect.process(&mut l[at..next], &mut r[at..next]);
                at = next;
                block += 1;
            }
            if end == 701 {
                effect.set_params(&target);
            }
            if end == 809 {
                effect.set_params(&initial);
            }
        }
        (l, r)
    }
    let whole = run::<E>(initial, target, &[4_096]);
    assert_eq!(whole, run::<E>(initial, target, &[1]));
    assert_eq!(whole, run::<E>(initial, target, &[37, 2, 127, 5, 256]));
}

#[test]
fn partition_invariance_including_automation_and_model_retargeting() {
    partition_check::<Waveshaper>(
        WaveshaperParams::default(),
        WaveshaperParams {
            input_db: 18.0,
            output_db: -9.0,
            curve: IDENTITY_CURVE.map(|x| x * x.abs()),
            oversampling: Oversampling::X2,
            mix: 0.7,
            bipolar: false,
            shape: crate::asset::ShapeRef::new(&[
                crate::asset::CurvePoint::new(0.0, 0.0),
                crate::asset::CurvePoint::new(0.4, 0.9),
                crate::asset::CurvePoint::new(1.0, 0.6),
            ])
            .unwrap(),
        },
    );
    partition_check::<Overdrive>(
        OverdriveParams::default(),
        OverdriveParams {
            drive_db: 36.0,
            tone_hz: 500.0,
            asymmetry: 1.0,
            emphasis: 1.0,
            output_db: -12.0,
            pre_band_hz: 2_500.0,
            hot: true,
            oversampling: Oversampling::X8,
        },
    );
    for model in GuitarModel::ALL {
        partition_check::<GuitarRack>(
            GuitarRackParams::default(),
            GuitarRackParams {
                model,
                drive_db: 24.0,
                output_db: -12.0,
                ..Default::default()
            },
        );
    }
    partition_check::<DriveChain>(
        DriveChainParams::default(),
        DriveChainParams {
            stages: [
                DriveStageParams {
                    model: DriveStage::Fuzz,
                    gain_db: 12.0,
                    ..Default::default()
                },
                DriveStageParams {
                    model: DriveStage::Overdrive,
                    gain_db: -3.0,
                    ..Default::default()
                },
                DriveStageParams {
                    model: DriveStage::Waveshape,
                    gain_db: 6.0,
                    ..Default::default()
                },
            ],
            output_db: -6.0,
            ..Default::default()
        },
    );
}

#[test]
fn drive_changes_ramp_and_reset_restores_the_current_target() {
    let mut effect = GuitarRack::default();
    effect.prepare(RATE, 512);
    effect.set_params(&GuitarRackParams {
        drive_db: 0.0,
        output_db: 0.0,
        ..Default::default()
    });
    let mut l = [0.01];
    let mut r = l;
    effect.process(&mut l, &mut r);
    assert_eq!(l[0], 0.01);
    effect.set_params(&GuitarRackParams {
        drive_db: 20.0,
        output_db: 0.0,
        ..Default::default()
    });
    let mut l = [0.01; 240];
    let mut r = l;
    effect.process(&mut l, &mut r);
    assert!(l[0] > 0.01 && l[0] < 0.011);
    assert!((l[239] - 0.1).abs() < 1.0e-6);
    assert!(l.windows(2).all(|xs| xs[1] >= xs[0]));
    effect.reset();
    let mut l = [0.01];
    let mut r = l;
    effect.process(&mut l, &mut r);
    assert!((l[0] - 0.1).abs() < 1.0e-6);
}

fn metadata<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned + ts_rs::TS>() {
    let defaults = P::default();
    assert_eq!(defaults, serde_json::from_str::<P>("{}").unwrap());
    let encoded = serde_json::to_string(&defaults).unwrap();
    assert_eq!(defaults, serde_json::from_str::<P>(&encoded).unwrap());
    assert!(!P::decl(&ts_rs::Config::default()).is_empty());
    assert_eq!(defaults, defaults.sanitized());
    for (index, descriptor) in P::descriptors().iter().enumerate() {
        assert_eq!(defaults.get(index), Some(descriptor.default));
        assert_eq!(P::index_of(descriptor.id), Some(index));
        let mut changed = defaults;
        assert!(changed.set(index, descriptor.max));
        assert_eq!(changed.get(index), Some(descriptor.max));
        assert_eq!(changed, changed.sanitized());
        if descriptor.kind == ParamKind::Float {
            assert!(changed.set(index, f32::NAN));
            assert_eq!(changed.get(index), Some(descriptor.default));
        }
    }
    assert_eq!(defaults.get(P::descriptors().len()), None);
    assert!(!P::default().set(P::descriptors().len(), 1.0));
}

#[test]
fn parameters_have_stable_descriptors_serde_defaults_and_typescript_types() {
    metadata::<WaveshaperParams>();
    metadata::<OverdriveParams>();
    metadata::<GuitarRackParams>();
    metadata::<DriveChainParams>();
    let guitar = serde_json::to_value(GuitarRackParams {
        model: GuitarModel::TubeSoftClip,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(guitar["model"], "tubeSoftClip");
    assert!(guitar.get("driveDb").is_some());
    let chain: DriveChainParams =
        serde_json::from_str(r#"{"stages":[{"model":"fuzz"},{},{}]}"#).unwrap();
    assert_eq!(chain.stages[0].gain_db, 0.0);
}

fn finite<E: Effect + Default>(params: E::Params) {
    for rate in [1.0, 8_000.0, RATE, 384_000.0, f32::NAN, f32::INFINITY] {
        let mut effect = E::default();
        effect.prepare(rate, 512);
        effect.set_params(&params);
        let damaged = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            -f32::MAX,
            1.0e-39,
            -0.9,
            0.9,
        ];
        let mut l = [0.0; 512];
        let mut r = l;
        for n in 0..512 {
            l[n] = damaged[n % damaged.len()];
            r[n] = -l[n];
        }
        effect.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        effect.reset();
        l.fill(0.0);
        r.fill(0.0);
        effect.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    }
}

#[test]
fn damaged_parameters_extreme_audio_and_sample_rates_never_produce_nan() {
    finite::<Waveshaper>(WaveshaperParams {
        input_db: f32::MAX,
        output_db: f32::INFINITY,
        curve: [f32::NAN; 33],
        mix: f32::NAN,
        ..Default::default()
    });
    finite::<Overdrive>(OverdriveParams {
        drive_db: f32::MAX,
        emphasis: f32::INFINITY,
        asymmetry: f32::NAN,
        tone_hz: f32::MAX,
        output_db: f32::MAX,
        pre_band_hz: f32::NAN,
        hot: true,
        ..Default::default()
    });
    for model in GuitarModel::ALL {
        finite::<GuitarRack>(GuitarRackParams {
            model,
            drive_db: f32::MAX,
            output_db: f32::MAX,
            ..Default::default()
        });
    }
    for model in [
        DriveStage::Bypass,
        DriveStage::Overdrive,
        DriveStage::Waveshape,
        DriveStage::Fuzz,
    ] {
        finite::<DriveChain>(DriveChainParams {
            stages: [DriveStageParams {
                model,
                gain_db: f32::MAX,
                ..Default::default()
            }; 3],
            output_db: f32::MAX,
            ..Default::default()
        });
    }
}
