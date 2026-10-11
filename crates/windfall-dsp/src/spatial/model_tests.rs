//! Independent measurements for the E3 circuits. Written, not executed.
use super::*;
use crate::{AnyEffect, Effect, EffectKind, EffectParams, ParamSet};

const RATE: f32 = 16_000.0;

fn run<E: Effect + Default>(params: E::Params, input: &[f32]) -> Vec<f32> {
    let mut fx = E::default();
    fx.prepare(RATE, 127);
    fx.set_params(&params);
    let mut left = input.to_vec();
    let mut right = left.clone();
    fx.process(&mut left, &mut right);
    left
}

fn sine(hz: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|n| 0.2 * (std::f32::consts::TAU * hz * n as f32 / RATE).sin())
        .collect()
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|x| f64::from(*x).powi(2)).sum()
}

#[test]
fn bucket_clock_has_128_samples_in_transit_and_a_bounded_work_limit() {
    use super::bucket::{Bucket, ticks_for};
    let mut bucket = Bucket::default();
    let output: Vec<_> = (0..300)
        .map(|n| bucket.tick(if n == 0 { 1.0 } else { 0.0 }, 1.0))
        .collect();
    // A unit clock hands the first input out exactly 128 frames later.
    for (n, x) in output.iter().enumerate() {
        assert_eq!(*x, if n == 128 { 1.0 } else { 0.0 });
    }
    assert_eq!(ticks_for(128.0), 1.0);
    assert_eq!(ticks_for(64.0), 2.0);
    assert_eq!(ticks_for(0.0), 16.0);
    bucket.clear();
    assert_eq!(bucket.tick(0.0, 1.0), 0.0);
}

#[test]
fn bucket_sweeps_have_opposed_triangles_and_the_both_mode_stays_mono() {
    for mode in [BucketMode::One, BucketMode::Two, BucketMode::Both] {
        let sweep = mode.sweep().unwrap();
        assert_eq!(sweep.delay_ms(0.0, false), sweep.min_ms);
        assert_eq!(sweep.delay_ms(0.5, false), sweep.max_ms);
        assert_eq!(sweep.delay_ms(1.0, false), sweep.min_ms);
        for phase in [0.0, 0.125, 0.5, 0.75] {
            let left = sweep.delay_ms(phase, false);
            let right = sweep.delay_ms(phase, true);
            if sweep.stereo {
                assert!((left + right - sweep.min_ms - sweep.max_ms).abs() < 1e-5);
            } else {
                assert_eq!(left, right);
            }
        }
        let mut fx = VintageChorus::default();
        fx.prepare(RATE, 64);
        fx.set_params(&VintageChorusParams {
            mode,
            mix: 1.0,
            ..Default::default()
        });
        let mut left = sine(500.0, 32_000);
        let mut right = left.clone();
        fx.process(&mut left, &mut right);
        assert!(energy(&left) > 1.0);
        if sweep.stereo {
            assert_ne!(left, right);
        } else {
            assert_eq!(left, right);
        }
    }
}

#[test]
fn bucket_modes_ignore_free_controls_and_have_distinct_audio() {
    let input = sine(900.0, 32_000);
    let mut results = Vec::new();
    for mode in [BucketMode::One, BucketMode::Two, BucketMode::Both] {
        let params = VintageChorusParams {
            mode,
            mix: 1.0,
            ..Default::default()
        };
        let baseline = run::<VintageChorus>(params, &input);
        let changed = run::<VintageChorus>(
            VintageChorusParams {
                delay_ms: 25.0,
                depth_ms: 12.0,
                rate_hz: 5.0,
                spread: 0.0,
                sync: true,
                cycle_beats: 0.125,
                ..params
            },
            &input,
        );
        assert_eq!(
            baseline, changed,
            "fixed model modes ignore the free circuit"
        );
        results.push(baseline);
    }
    for pair in results.windows(2) {
        let difference: Vec<_> = pair[0].iter().zip(&pair[1]).map(|(a, b)| a - b).collect();
        assert!(energy(&difference) / energy(&pair[0]) > 0.01);
    }
}

#[test]
fn swarm_count_measures_the_average_of_independently_delayed_voices() {
    let hz = 400.0;
    let input = sine(hz, 8_000);
    for voices in [2, 4, 8, 16] {
        let params = HyperChorusParams {
            delay_ms: 10.0,
            depth_ms: 0.0,
            rate_hz: 0.0,
            spread: 0.0,
            tone_hz: 5000.0,
            mix: 1.0,
            voices,
            ..Default::default()
        };
        let actual = run::<HyperChorus>(params, &input);
        // Independent continuous-delay reference followed by the documented
        // one-pole. Cubic interpolation error at 400 Hz is bounded separately.
        let pole = (-std::f64::consts::TAU * 5000.0 / f64::from(RATE)).exp();
        let mut filtered = 0.0;
        for (n, output) in actual.iter().enumerate() {
            let mut wet = 0.0;
            for i in 0..voices {
                let delay = 160.0 * (1.0 + 0.04 * f64::from(i));
                wet += 0.2
                    * (std::f64::consts::TAU * f64::from(hz) * (n as f64 - delay)
                        / f64::from(RATE))
                    .sin()
                    / f64::from(voices);
            }
            filtered = (1.0 - pole) * wet + pole * filtered;
            if n > 1000 {
                assert!(
                    (f64::from(*output) - filtered).abs() < 2e-4,
                    "{voices} voices, frame {n}"
                );
            }
        }
    }
}

#[test]
fn four_equal_allpasses_produce_two_measured_pedal_notches() {
    let corner = 700.0_f32;
    let params = VintagePhaserParams {
        min_hz: corner,
        max_hz: corner,
        rate_hz: 0.0,
        feedback: 0.0,
        stages: PhaserStages::Four,
        stagger: 0.0,
        mix: 0.5,
        ..Default::default()
    };
    // Bilinear warp of the analog four-stage cancellation frequencies.
    let g = (std::f64::consts::PI * f64::from(corner) / f64::from(RATE)).tan();
    for angle in [std::f64::consts::PI / 8.0, 3.0 * std::f64::consts::PI / 8.0] {
        let hz = (f64::from(RATE) / std::f64::consts::PI * (g * angle.tan()).atan()) as f32;
        let input = sine(hz, 16_000);
        let actual = run::<VintagePhaser>(params, &input);
        assert!(
            energy(&actual[4000..]) / energy(&input[4000..]) < 1e-5,
            "notch {hz} Hz"
        );
    }
    let input = sine(corner, 16_000);
    let actual = run::<VintagePhaser>(params, &input);
    assert!((energy(&actual[4000..]) / energy(&input[4000..]) - 1.0).abs() < 1e-3);
}

#[test]
fn stacked_combs_match_an_independent_difference_equation_for_every_order() {
    let mut impulse = vec![0.0; 8_000];
    impulse[0] = 1.0;
    for order in 1..=4 {
        for feedback in [-0.75, 0.0, 0.75] {
            let params = StackedFlangerParams {
                order,
                delay_ms: 4.0,
                depth_ms: 0.0,
                rate1_hz: 0.0,
                rate2_hz: 0.0,
                feedback,
                mix: 1.0,
                ..Default::default()
            };
            let actual = run::<StackedFlanger>(params, &impulse);
            let mut reference: Vec<f64> = impulse.iter().map(|x| f64::from(*x)).collect();
            for stage in 0..order {
                let delay = 64.0 * (1.0 - 0.13 * f64::from(stage));
                let whole = delay.floor() as usize;
                let fraction = delay - whole as f64;
                let pole = (-std::f64::consts::TAU * 7200.0 / f64::from(RATE)).exp();
                let mut history = vec![0.0; reference.len()];
                let mut tone = 0.0;
                for n in 0..reference.len() {
                    let near = if n >= whole { history[n - whole] } else { 0.0 };
                    let far = if n > whole {
                        history[n - whole - 1]
                    } else {
                        0.0
                    };
                    let echo = (1.0 - fraction) * near + fraction * far;
                    tone = (1.0 - pole) * echo + pole * tone;
                    history[n] = reference[n] + f64::from(feedback) * tone;
                    reference[n] = 0.5 * (reference[n] + echo);
                }
            }
            for (n, (a, b)) in actual.iter().zip(reference).enumerate() {
                assert!(
                    (f64::from(*a) - b).abs() < 3e-5,
                    "order {order}, feedback {feedback}, frame {n}"
                );
            }
        }
    }
}

fn cases() -> Vec<EffectParams> {
    vec![
        EffectParams::VintageChorus(VintageChorusParams {
            mode: BucketMode::One,
            mix: 1.0,
            ..Default::default()
        }),
        EffectParams::VintageChorus(VintageChorusParams {
            mode: BucketMode::Two,
            mix: 1.0,
            ..Default::default()
        }),
        EffectParams::VintageChorus(VintageChorusParams {
            mode: BucketMode::Both,
            mix: 1.0,
            ..Default::default()
        }),
        EffectParams::HyperChorus(HyperChorusParams {
            voices: 16,
            mix: 1.0,
            ..Default::default()
        }),
        EffectParams::VintagePhaser(VintagePhaserParams {
            stages: PhaserStages::Four,
            stagger: 0.0,
            feedback: -0.8,
            ..Default::default()
        }),
        EffectParams::VintagePhaser(VintagePhaserParams {
            stages: PhaserStages::Twelve,
            feedback: 0.8,
            ..Default::default()
        }),
        EffectParams::StackedFlanger(StackedFlangerParams {
            order: 4,
            feedback: 0.75,
            ..Default::default()
        }),
        EffectParams::StackedFlanger(StackedFlangerParams {
            order: 4,
            feedback: -0.75,
            ..Default::default()
        }),
    ]
}

#[test]
fn model_extrema_partition_edits_empty_blocks_and_tails_are_stable() {
    for params in cases() {
        let build = || {
            let mut fx = AnyEffect::new(&params);
            fx.prepare(RATE, 127);
            fx.set_params(&params);
            fx
        };
        let mut whole = build();
        let mut split = build();
        let input = sine(250.0, 16_001);
        let (mut left, mut right) = (input.clone(), input.clone());
        let (mut sl, mut sr) = (input.clone(), input);
        for (start, end) in [(0, 3217), (3217, 16_001)] {
            if start > 0 {
                let mut changed = params;
                for (i, info) in params.kind().descriptors().iter().enumerate() {
                    changed.set(i, info.max);
                }
                whole.set_params(&changed);
                split.set_params(&changed);
                whole.set_tempo(173.0);
                split.set_tempo(173.0);
            }
            whole.process(&mut left[start..end], &mut right[start..end]);
            let mut at = start;
            while at < end {
                split.process(&mut [], &mut []);
                let next = (at + 127).min(end);
                split.process(&mut sl[at..next], &mut sr[at..next]);
                at = next;
            }
        }
        assert_eq!(left, sl, "{:?}", params.kind());
        assert_eq!(right, sr);
        assert!(
            left.iter()
                .chain(&right)
                .all(|x| x.is_finite() && x.abs() < 100.0)
        );
        let len = whole.tail_samples() + 1;
        let mut left = vec![0.0; len];
        let mut right = left.clone();
        whole.process(&mut left, &mut right);
        assert_eq!(left[len - 1], 0.0);
        assert_eq!(right[len - 1], 0.0);
        assert_eq!(whole.latency_samples(), 0);
    }
}

#[test]
fn tempo_sync_matches_free_rate_and_depth_zero_removes_detuning() {
    for kind in [
        EffectKind::VintageChorus,
        EffectKind::HyperChorus,
        EffectKind::VintagePhaser,
        EffectKind::StackedFlanger,
    ] {
        let mut synced = kind.default_params();
        synced.set(
            kind.descriptors()
                .iter()
                .position(|p| p.id == "sync")
                .unwrap(),
            1.0,
        );
        synced.set(
            kind.descriptors()
                .iter()
                .position(|p| p.id == "cycleBeats")
                .unwrap(),
            4.0,
        );
        let mut free = kind.default_params();
        let rate = if kind == EffectKind::StackedFlanger {
            "rate1Hz"
        } else {
            "rateHz"
        };
        free.set(
            kind.descriptors()
                .iter()
                .position(|p| p.id == rate)
                .unwrap(),
            0.5,
        );
        if kind == EffectKind::StackedFlanger {
            free.set(
                kind.descriptors()
                    .iter()
                    .position(|p| p.id == "rate2Hz")
                    .unwrap(),
                0.5 * 1.382,
            );
        }
        let process = |params: EffectParams, bpm: f32| {
            let mut fx = AnyEffect::new(&params);
            fx.prepare(RATE, 64);
            fx.set_params(&params);
            fx.set_tempo(bpm);
            let mut left = sine(800.0, 24_000);
            let mut right = left.clone();
            fx.process(&mut left, &mut right);
            (left, right)
        };
        assert_eq!(process(synced, 120.0), process(free, 73.0), "{kind:?}");
        assert_ne!(process(synced, 120.0), process(synced, 240.0));
        if let Some(depth) = kind.descriptors().iter().position(|p| p.id == "depthMs") {
            synced.set(depth, 0.0);
            assert_eq!(process(synced, 120.0), process(synced, 240.0));
        }
    }
}

#[test]
fn legacy_model_settings_keep_defaults_and_descriptor_prefixes() {
    let old: VintageChorusParams = serde_json::from_str("{}").unwrap();
    assert_eq!(old.mode, BucketMode::Free);
    assert!(!old.sync);
    let old: HyperChorusParams = serde_json::from_str("{}").unwrap();
    assert_eq!(old.voices, 8);
    let old: VintagePhaserParams = serde_json::from_str("{}").unwrap();
    assert_eq!((old.stages, old.stagger), (PhaserStages::Twelve, 1.0));
    let old: StackedFlangerParams = serde_json::from_str("{}").unwrap();
    assert_eq!(old.order, 2);
    assert_eq!(VintageChorusParams::index_of("mode"), Some(6));
    assert_eq!(HyperChorusParams::index_of("voices"), Some(6));
    assert_eq!(VintagePhaserParams::index_of("stages"), Some(7));
    assert_eq!(VintagePhaserParams::index_of("stagger"), Some(8));
    assert_eq!(StackedFlangerParams::index_of("order"), Some(8));
}

#[test]
fn legacy_room_first_reflection_matches_its_fractional_delay_and_signed_level() {
    let mut impulse = vec![0.0; 800];
    impulse[0] = 1.0;
    let params = RoomParams {
        size: 0.0,
        pre_delay_ms: 0.0,
        early_level: 1.0,
        width: 1.0,
        mix: 1.0,
        ..Default::default()
    };
    let actual = run::<Room>(params, &impulse);
    // First tap: 3.7 ms * 0.6 * 16 kHz = 35.52 frames, level +0.45.
    assert!(actual[..35].iter().all(|x| *x == 0.0));
    assert!((actual[35] - 0.45 * 0.48).abs() < 1e-5);
    assert!((actual[36] - 0.45 * 0.52).abs() < 1e-5);
}
