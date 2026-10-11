use super::*;
use crate::ParamSet;
use crate::asset::{CurvePoint, CurveRef};

fn run<E: Effect>(effect: &mut E, input: &[f32], block: usize) -> Vec<f32> {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    for (l, r) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        effect.process(l, r);
    }
    l
}

#[test]
fn drawn_time_reverses_a_bounded_record_and_volume_follows_an_independent_line() {
    let mut gate = VolumeGate::new();
    gate.prepare(1000.0, 512);
    gate.set_tempo(60.0);
    let p = VolumeGateParams {
        loop_beats: 0.125,
        time_curve: CurveRef::new(&[CurvePoint::new(0.0, 1.0), CurvePoint::new(1.0, 0.0)]).unwrap(),
        volume_curve: CurveRef::new(&[CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)])
            .unwrap(),
        ..Default::default()
    };
    gate.set_params(&p);
    let input: Vec<_> = (0..5000).map(|n| n as f32 * 0.0001).collect();
    let output = run(&mut gate, &input, 7);
    // Each complete cycle reads the previous immutable 125-frame record.
    // Ignore the explicitly documented two-frame discontinuity crossfade.
    for (n, &sample) in output.iter().enumerate().take(5000).skip(125) {
        let phase = n % 125;
        if phase < 3 {
            continue;
        }
        let oldest = n / 125 * 125 - 125;
        let position = (1.0 - phase as f32 / 125.0) * 124.0;
        let expected = (oldest as f32 + position) * 0.0001 * (phase as f32 / 125.0);
        assert!((sample - expected).abs() < 1e-6, "frame {n}");
    }
    assert_eq!(gate.history.ring.data.len(), 2000);
    assert!(std::mem::size_of::<VolumeGateParams>() < 128);
}

#[test]
fn host_position_sets_exact_phase_and_seeks_preserve_history() {
    let mut gate = VolumeGate::new();
    gate.prepare(48_000.0, 512);
    gate.set_tempo(120.0);
    gate.set_params(&VolumeGateParams {
        loop_beats: 1.0,
        ..Default::default()
    });
    run(&mut gate, &vec![0.25; 4000], 137);
    let filled = gate.history.ring.filled;
    gate.set_transport(true, 7.25);
    assert_eq!(gate.phase, 6000);
    assert_eq!(gate.history.ring.filled, filled);
    run(&mut gate, &[0.25; 137], 137);
    gate.set_transport(true, 7.25 + 137.0 / 24000.0);
    assert_eq!(gate.phase, 6137);
    gate.set_transport(true, f64::NAN);
    assert_eq!(gate.phase, 0);
}

#[test]
fn mapped_gate_trigger_restarts_curves_and_preserves_a_held_capture_across_host_blocks() {
    let p = VolumeGateParams {
        loop_beats: 0.125,
        time_curve: CurveRef::new(&[CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]).unwrap(),
        volume_curve: CurveRef::new(&[CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)])
            .unwrap(),
        ..Default::default()
    };
    let render = |block| {
        let mut gate = VolumeGate::new();
        gate.prepare(1000.0, 512);
        gate.set_tempo(60.0);
        gate.set_params(&p);
        let history: Vec<_> = (0..503).map(|n| n as f32 * 0.001).collect();
        run(&mut gate, &history, block);
        let filled = gate.history.ring.filled;
        let write = gate.history.ring.write;
        let mut mapped = p.sanitized();
        let trigger = VolumeGateParams::descriptors()
            .iter()
            .position(|d| d.id == "trigger")
            .unwrap();
        assert_eq!(trigger, 21, "old descriptor indices must not move");
        assert!(mapped.set(trigger, 1.0));
        gate.set_params(&mapped);
        let mut output = vec![0.9; 5000];
        let mut right = output.clone();
        let mut elapsed = 0;
        for (left, right) in output.chunks_mut(block).zip(right.chunks_mut(block)) {
            gate.set_transport(true, (503 + elapsed) as f64 / 1000.0);
            gate.process(left, right);
            elapsed += left.len();
        }
        assert_eq!(gate.history.ring.filled, filled);
        assert_eq!(
            gate.history.ring.write, write,
            "held capture must never be overwritten"
        );
        assert_eq!(gate.tail_samples(), usize::MAX);
        for (n, got) in output.iter().enumerate() {
            let phase = n % 125;
            if phase < 3 {
                continue;
            }
            let normalized = phase as f32 / 125.0;
            let expected = (378.0 + normalized * 124.0) * 0.001 * normalized;
            assert!((*got - expected).abs() < 1e-6, "frame {n}");
        }
        gate.set_params(&p);
        run(&mut gate, &[0.7; 3], 1);
        assert_eq!(gate.history.ring.filled, filled + 3);
        gate.set_transport(true, 5.506);
        assert_eq!(gate.phase, 6, "release restores the song clock");
        output
    };
    let expected = render(137);
    for block in [1, 7, 64, 512] {
        assert_eq!(expected, render(block));
    }
    assert_eq!(
        serde_json::to_value(VolumeGateParams::default()).unwrap()["trigger"],
        false
    );
}

#[test]
fn a_gate_trigger_without_a_drawn_time_curve_repeats_the_original_input_at_unity() {
    let mut gate = VolumeGate::new();
    gate.prepare(1000.0, 512);
    gate.set_tempo(60.0);
    let p = VolumeGateParams {
        loop_beats: 0.125,
        ..Default::default()
    };
    gate.set_params(&p);
    let history: Vec<_> = (0..503).map(|n| n as f32 * 0.001).collect();
    run(&mut gate, &history, 7);
    gate.set_params(&VolumeGateParams { trigger: true, ..p });
    let output = run(&mut gate, &vec![0.9; 5000], 137);
    for (n, got) in output.iter().enumerate() {
        if n % 125 < 3 {
            continue;
        }
        assert!((*got - history[378 + n % 125]).abs() < 1e-6);
    }
}

#[test]
fn transient_capture_starts_at_the_onset_and_reloops_at_exact_tempo_length() {
    let mut input = vec![0.0; 800];
    for (n, x) in input.iter_mut().enumerate().take(135).skip(10) {
        *x = if n == 10 {
            0.8
        } else {
            0.02 * (n - 10) as f32 / 125.0
        };
    }
    input[600] = 0.9;
    let p = TimeTransportParams {
        auto_trigger: true,
        threshold_db: -12.0,
        refractory_ms: 500.0,
        loop_beats: 0.125,
        ..Default::default()
    };
    let render = |block| {
        let mut effect = TimeTransport::new();
        effect.prepare(1000.0, 512);
        effect.set_tempo(60.0);
        effect.set_params(&p);
        run(&mut effect, &input, block)
    };
    let output = render(137);
    for n in 139..260 {
        assert!(
            (output[n] - input[10 + (n - 135) % 125]).abs() < 1e-6,
            "frame {n}"
        );
    }
    assert!(output[603] == input[603]);
    for block in [1, 7, 64, 512] {
        assert_eq!(output, render(block));
    }
}

#[test]
fn both_xy_axes_change_each_of_the_twelve_independent_treatments() {
    let input: Vec<_> = (0..8000)
        .map(|n| {
            0.2 * (std::f32::consts::TAU * 431.0 * n as f32 / 4000.0).sin()
                + 0.1 * (n % 79) as f32 / 79.0
        })
        .collect();
    for model in PerformanceModel::ALL {
        let render = |x, y, block| {
            let mut e = PerformanceRack::new();
            e.prepare(4000.0, 512);
            e.set_tempo(120.0);
            e.set_params(&PerformanceRackParams {
                model,
                xy_enabled: true,
                x,
                y,
                loop_beats: 0.5,
                ..Default::default()
            });
            run(&mut e, &input, block)
        };
        let a = render(0.9, 0.2, 137);
        let b = render(0.2, 0.2, 137);
        let c = render(0.9, 0.8, 137);
        for changed in [&b, &c] {
            assert!(
                a[3000..]
                    .iter()
                    .zip(&changed[3000..])
                    .any(|(a, b)| (a - b).abs() > 1e-4),
                "{model:?}"
            );
        }
        assert_eq!(a, render(0.9, 0.2, 7));
    }
}

#[test]
fn maximum_replay_storage_is_fixed_even_for_huge_host_blocks() {
    let mut gate = VolumeGate::new();
    gate.prepare(384_000.0, usize::MAX);
    assert_eq!(
        gate.history.ring.data.len() * std::mem::size_of::<Frame>(),
        6_144_000
    );
    let mut transport = TimeTransport::new();
    transport.prepare(384_000.0, usize::MAX);
    assert_eq!(
        (transport.history.ring.data.len() + transport.replay.data.len())
            * std::mem::size_of::<Frame>(),
        12_288_000
    );
}

#[test]
fn loaded_scratch_interpolates_a_stereo_source_and_follows_reverse_curves() {
    let audio = windfall_core::AudioBuffer::from_interleaved(
        1000,
        2,
        (0..126)
            .flat_map(|n| [n as f32 * 0.001, -n as f32 * 0.002])
            .collect(),
    );
    let reference = crate::asset::publish_audio(&audio).unwrap();
    let mut effect = Scratch::new();
    effect.prepare(1000.0, 512);
    effect.set_tempo(60.0);
    let p = ScratchParams {
        source_id: 42,
        runtime_source: reference,
        position: 0.5,
        loop_beats: 0.125,
        ..Default::default()
    };
    effect.set_params(&p);
    let mut left = [0.0; 250];
    let mut right = left;
    effect.process(&mut left, &mut right);
    assert!((left[100] - 0.0625).abs() < 1e-7);
    assert!((right[100] + 0.125).abs() < 1e-7);
    effect.reset();
    effect.set_params(&ScratchParams {
        curve_enabled: true,
        position_curve: CurveRef::new(&[CurvePoint::new(0.0, 1.0), CurvePoint::new(1.0, 0.0)])
            .unwrap(),
        ..p
    });
    left.fill(0.0);
    right.fill(0.0);
    effect.process(&mut left, &mut right);
    for (n, &sample) in left.iter().enumerate().take(125).skip(4) {
        assert!((sample - (0.125 - n as f32 * 0.001)).abs() < 1e-6);
    }
    let encoded = serde_json::to_value(p).unwrap();
    assert!(encoded.get("runtimeSource").is_none());
    assert_eq!(encoded["sourceId"], 42);
}

#[test]
fn runtime_source_identity_cannot_leak_between_projects_that_reuse_an_id() {
    let a = windfall_core::AudioBuffer::from_interleaved(1000, 1, vec![0.25; 128]);
    let b = windfall_core::AudioBuffer::from_interleaved(1000, 1, vec![-0.5; 128]);
    let first = crate::asset::publish_audio(&a).unwrap();
    let other = crate::asset::publish_audio(&b).unwrap();
    assert_ne!(first, other);
    assert_eq!(first, crate::asset::publish_audio(&a).unwrap());
    assert_eq!(first.audio().unwrap().samples()[0], 0.25);
    assert_eq!(other.audio().unwrap().samples()[0], -0.5);
    let mut missing = Scratch::new();
    missing.prepare(1000.0, 512);
    missing.set_params(&ScratchParams {
        source_id: 1,
        ..Default::default()
    });
    assert_eq!(run(&mut missing, &vec![0.9; 100], 7), vec![0.0; 100]);
}

#[test]
fn deliberate_source_discontinuities_follow_a_two_millisecond_crossfade() {
    let mut fade = JumpFade::default();
    fade.prepare(48000.0);
    assert_eq!(fade.tick([-0.5, 0.5]), [-0.5, 0.5]);
    fade.jump();
    for n in 1..=96 {
        let got = fade.tick([0.5, -0.5]);
        let expected = -0.5 + n as f32 / 96.0;
        assert!((got[0] - expected).abs() < 1e-6);
        assert!((got[1] + expected).abs() < 1e-6);
    }
    assert_eq!(fade.tick([0.5, -0.5]), [0.5, -0.5]);
}
