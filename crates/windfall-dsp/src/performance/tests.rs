use super::*;
use crate::ParamSet;

fn run<E: Effect>(effect: &mut E, input: &[f32]) -> Vec<f32> {
    let mut left = input.to_vec();
    let mut right = left.clone();
    effect.process(&mut left, &mut right);
    assert_eq!(left, right);
    left
}

#[test]
fn gate_release_after_loop_shrink_returns_to_absolute_phase() {
    let mut gate = VolumeGate::new();
    gate.prepare(1000.0, 1);
    gate.set_tempo(60.0);
    let p = VolumeGateParams {
        loop_beats: 1.0,
        ..Default::default()
    };
    gate.set_params(&p);
    gate.set_transport(true, 0.3);
    gate.set_params(&VolumeGateParams { trigger: true, ..p });
    run(&mut gate, &[1.0]);
    assert_eq!(gate.phase_offset, 700);
    gate.set_params(&VolumeGateParams {
        loop_beats: 0.125,
        ..p
    });
    run(&mut gate, &[1.0]);
    // Absolute sample 301 resumes modulo the new 125-frame loop, then advances.
    assert_eq!(gate.phase, 52);
    assert_eq!(gate.phase_offset, 0);
    assert!(!gate.trigger_down);
}

#[test]
fn zero_volume_step_silences_its_sine_slice() {
    let mut gate = VolumeGate::new();
    gate.prepare(1600.0, 1600);
    gate.set_tempo(60.0);
    let mut p = VolumeGateParams {
        loop_beats: 1.0,
        ..Default::default()
    };
    p.steps[5] = 0.0;
    gate.set_params(&p);
    let sine: Vec<_> = (0..3200)
        .map(|n| (std::f32::consts::TAU * 37.0 * n as f32 / 1600.0).sin())
        .collect();
    let out = run(&mut gate, &sine);
    assert!(
        out[500..600]
            .iter()
            .chain(&out[2100..2200])
            .all(|&x| x == 0.0)
    );
    assert_eq!(out[0..500], sine[0..500]);
    assert_eq!(out[600..1600], sine[600..1600]);
}

#[test]
fn reverse_reads_captured_audio_backward_and_keeps_it_immutable() {
    let mut effect = TimeTransport::new();
    effect.prepare(16.0, 16);
    effect.set_tempo(60.0);
    let p = TimeTransportParams {
        loop_beats: 1.0,
        mode: TransportMode::Reverse,
        ..Default::default()
    };
    effect.set_params(&p);
    run(&mut effect, &(0..16).map(|n| n as f32).collect::<Vec<_>>());
    effect.set_params(&TimeTransportParams { trigger: true, ..p });
    let out = run(&mut effect, &[99.0; 48]);
    for (i, &sample) in out.iter().enumerate() {
        assert_eq!(sample, (15 - i % 16) as f32);
    }
}

#[test]
fn transport_hold_repeat_release_and_reset_have_real_trigger_behavior() {
    for mode in [TransportMode::Hold, TransportMode::Repeat] {
        let mut effect = TimeTransport::new();
        effect.prepare(16.0, 16);
        effect.set_tempo(60.0);
        let p = TimeTransportParams {
            mode,
            loop_beats: 1.0,
            ..Default::default()
        };
        effect.set_params(&p);
        run(&mut effect, &(0..16).map(|n| n as f32).collect::<Vec<_>>());
        effect.set_params(&TimeTransportParams { trigger: true, ..p });
        let out = run(&mut effect, &[33.0; 32]);
        for (n, sample) in out.iter().enumerate() {
            assert_eq!(
                *sample,
                if mode == TransportMode::Hold {
                    15.0
                } else {
                    (n % 16) as f32
                }
            );
        }
        effect.set_params(&p);
        assert_eq!(run(&mut effect, &[42.0]), [42.0]);
        effect.set_params(&TimeTransportParams { trigger: true, ..p });
        effect.reset();
        assert_eq!(run(&mut effect, &[42.0; 8]), [0.0; 8]);
    }
}

#[test]
fn scratch_position_selects_the_sample_and_interpolates() {
    for (position, expected) in [(0.0, 0.0), (0.5, 7.5), (1.0, 15.0)] {
        let mut effect = Scratch::new();
        effect.prepare(16.0, 16);
        effect.set_tempo(60.0);
        let p = ScratchParams {
            position,
            loop_beats: 1.0,
            ..Default::default()
        };
        effect.set_params(&p);
        run(&mut effect, &(0..16).map(|n| n as f32).collect::<Vec<_>>());
        effect.set_params(&ScratchParams { freeze: true, ..p });
        assert_eq!(run(&mut effect, &[99.0; 64]), [expected; 64]);
    }
}

#[test]
fn at_least_three_rack_models_differ_on_identical_audio() {
    let input: Vec<_> = (0..512).map(|n| (n % 71) as f32 / 71.0 - 0.5).collect();
    let output: Vec<_> = [
        PerformanceModel::Reverse,
        PerformanceModel::TelephoneFilter,
        PerformanceModel::Distortion,
    ]
    .into_iter()
    .map(|model| {
        let mut effect = PerformanceRack::new();
        effect.prepare(128.0, 512);
        effect.set_tempo(60.0);
        effect.set_params(&PerformanceRackParams {
            model,
            loop_beats: 1.0,
            ..Default::default()
        });
        assert_eq!(effect.latency_samples(), 128);
        run(&mut effect, &input)
    })
    .collect();
    for a in 0..output.len() {
        for b in a + 1..output.len() {
            let difference: f32 = output[a][128..]
                .iter()
                .zip(&output[b][128..])
                .map(|(x, y)| (x - y).abs())
                .sum();
            assert!(difference > 1.0, "models {a} and {b} unexpectedly match");
        }
    }
}

#[test]
fn rack_dry_path_is_exactly_latency_aligned_through_ring_wraps() {
    let mut rack = PerformanceRack::new();
    rack.prepare(100.0, 1000);
    rack.set_tempo(120.0);
    rack.set_params(&PerformanceRackParams {
        mix: 0.0,
        loop_beats: 1.0,
        ..Default::default()
    });
    let input: Vec<_> = (0..1000).map(|n| n as f32).collect();
    let out = run(&mut rack, &input);
    assert_eq!(rack.latency_samples(), 50);
    assert_eq!(&out[..50], &[0.0; 50]);
    assert_eq!(&out[50..], &input[..950]);
}

#[test]
fn volume_gate_is_partition_invariant_including_random_retriggers_and_edits() {
    fn render(partition: usize) -> Vec<f32> {
        let mut gate = VolumeGate::new();
        gate.prepare(4800.0, 4000);
        gate.set_tempo(137.0);
        let mut p = VolumeGateParams {
            retrigger_chance: 0.73,
            feedback: 0.5,
            loop_beats: 0.5,
            ..Default::default()
        };
        for (i, gain) in p.steps.iter_mut().enumerate() {
            *gain = i as f32 / 15.0;
        }
        gate.set_params(&p);
        let mut out: Vec<_> = (0..12_000).map(|n| (n as f32 * 0.037).sin()).collect();
        for (segment, region) in out.chunks_mut(4000).enumerate() {
            if segment == 1 {
                p.steps[0] = 0.0;
                p.mix = 0.6;
                gate.set_params(&p);
                gate.set_tempo(91.0);
            }
            for chunk in region.chunks_mut(partition) {
                let mut right = chunk.to_vec();
                gate.process(chunk, &mut right);
                assert_eq!(chunk, right);
            }
        }
        out
    }
    let whole = render(4000);
    for n in [1, 7, 127, 513] {
        assert_eq!(whole, render(n), "partition {n}");
    }
}

#[test]
fn chance_one_replays_history_and_chance_zero_is_live() {
    let mut live = VolumeGate::new();
    let mut replay = VolumeGate::new();
    live.prepare(160.0, 320);
    replay.prepare(160.0, 320);
    live.set_tempo(60.0);
    replay.set_tempo(60.0);
    let p = VolumeGateParams {
        loop_beats: 1.0,
        ..Default::default()
    };
    live.set_params(&p);
    replay.set_params(&VolumeGateParams {
        retrigger_chance: 1.0,
        ..p
    });
    let input: Vec<_> = (0..320).map(|n| n as f32).collect();
    assert_eq!(run(&mut live, &input), input);
    let out = run(&mut replay, &input);
    assert_eq!(&out[160..170], &input[150..160]);
}

#[test]
fn tempo_sync_memory_bound_and_reset_are_consistent() {
    let mut gate = VolumeGate::new();
    gate.prepare(48000.0, 99999);
    gate.set_params(&VolumeGateParams {
        loop_beats: 1.0,
        ..Default::default()
    });
    assert_eq!(gate.history.ring.data.len(), 96000);
    assert_eq!(gate.history.length, 24000);
    gate.set_tempo(60.0);
    assert_eq!(gate.history.length, 48000);
    gate.set_params(&VolumeGateParams {
        loop_beats: 16.0,
        ..Default::default()
    });
    assert_eq!(gate.history.length, 96000);
    let pointer = gate.history.ring.data.as_ptr();
    let capacity = gate.history.ring.data.capacity();
    gate.reset();
    gate.set_tempo(f32::NAN);
    let mut l = [1.0; 17];
    let mut r = [1.0; 17];
    gate.process(&mut l, &mut r);
    assert_eq!(pointer, gate.history.ring.data.as_ptr());
    assert_eq!(capacity, gate.history.ring.data.capacity());
    let mut rack = PerformanceRack::new();
    rack.prepare(48000.0, 1);
    rack.set_params(&PerformanceRackParams {
        loop_beats: 16.0,
        ..Default::default()
    });
    assert_eq!(rack.history.length, 48000);
    assert_eq!(rack.history.ring.data.len(), 96000);
    rack.set_params(&PerformanceRackParams {
        loop_beats: 0.5,
        ..Default::default()
    });
    rack.set_tempo(60.0);
    assert_eq!(rack.latency_samples(), 24000);
}

fn hostile<E: Effect>(effect: &mut E) {
    for n in 0..30 {
        let mut l = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            -f32::MAX,
            0.0,
            1.0,
            -1.0,
        ];
        let mut r = l;
        effect.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()), "frame group {n}");
    }
}

#[test]
fn every_processor_and_every_model_remains_finite_on_hostile_audio_and_params() {
    let mut gate = VolumeGate::new();
    hostile(&mut gate);
    gate.prepare(f32::NAN, 8);
    gate.set_params(&VolumeGateParams {
        steps: [f32::NAN; 16],
        feedback: f32::INFINITY,
        mix: f32::NAN,
        retrigger_chance: 1.0,
        ..Default::default()
    });
    hostile(&mut gate);
    let mut transport = TimeTransport::new();
    hostile(&mut transport);
    transport.prepare(16.0, 8);
    for mode in [
        TransportMode::Hold,
        TransportMode::Reverse,
        TransportMode::Repeat,
    ] {
        transport.set_params(&TimeTransportParams {
            mode,
            trigger: false,
            rate: f32::NAN,
            ..Default::default()
        });
        hostile(&mut transport);
        transport.set_params(&TimeTransportParams {
            mode,
            trigger: true,
            rate: 4.0,
            ..Default::default()
        });
        hostile(&mut transport);
    }
    let mut scratch = Scratch::new();
    hostile(&mut scratch);
    scratch.prepare(16.0, 8);
    for position in [f32::NAN, -5.0, 5.0] {
        scratch.set_params(&ScratchParams {
            position,
            ..Default::default()
        });
        hostile(&mut scratch);
    }
    let mut rack = PerformanceRack::new();
    hostile(&mut rack);
    rack.prepare(16.0, 8);
    for model in PerformanceModel::ALL {
        rack.set_params(&PerformanceRackParams {
            model,
            feedback: 0.95,
            pitch_semitones: f32::INFINITY,
            ..Default::default()
        });
        hostile(&mut rack);
    }
}

fn param_contract<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned + ts_rs::TS>() {
    let defaults = P::default();
    let json = serde_json::to_value(defaults).unwrap();
    assert_eq!(defaults, serde_json::from_value(json.clone()).unwrap());
    assert_eq!(defaults, serde_json::from_str::<P>("{}").unwrap());
    assert!(!P::decl(&ts_rs::Config::default()).is_empty());
    for (index, info) in P::descriptors().iter().enumerate() {
        assert_eq!(defaults.get(index), Some(info.default));
        assert_eq!(P::index_of(info.id), Some(index));
        let mut value = &json;
        for key in info.id.split('.') {
            value = if let Ok(n) = key.parse::<usize>() {
                &value[n]
            } else {
                &value[key]
            };
        }
        assert!(!value.is_null(), "missing {}", info.id);
        let mut p = defaults;
        assert!(p.set(index, f32::NAN));
        assert!(p.sanitized().get(index).unwrap().is_finite());
        assert!(p.set(index, info.max));
        let clean = p.sanitized();
        assert_eq!(clean.get(index), Some(info.max));
        let mut moving = defaults;
        moving.approach(&clean, 1.0);
        assert_eq!(moving, clean);
    }
    let mut p = defaults;
    assert!(!p.set(P::descriptors().len(), 0.0));
    assert_eq!(p.get(P::descriptors().len()), None);
}

#[test]
fn params_have_stable_descriptors_serialization_sanitization_and_typescript() {
    param_contract::<VolumeGateParams>();
    param_contract::<TimeTransportParams>();
    param_contract::<ScratchParams>();
    param_contract::<PerformanceRackParams>();
    assert_eq!(VolumeGateParams::NAME, "Volume Gate");
    assert_eq!(TimeTransportParams::NAME, "Time Transport");
    assert_eq!(ScratchParams::NAME, "Scratch");
    assert_eq!(PerformanceRackParams::NAME, "Performance Rack");
}

#[test]
fn ring_linear_reads_wrap_within_the_slice() {
    let mut ring = Ring::default();
    ring.prepare(4.0);
    for n in 0..24 {
        ring.push([n as f32, -(n as f32)]);
    }
    assert_eq!(ring.read(ring.start(4), 4, 0.0), [20.0, -20.0]);
    assert_eq!(ring.read(ring.start(4), 4, -1.0), [23.0, -23.0]);
    assert_eq!(ring.read(ring.start(4), 4, 3.5), [21.5, -21.5]);
}

#[test]
fn transport_scratch_and_all_rack_models_are_partition_invariant() {
    fn compare<E: Effect>(mut a: E, mut b: E, params: E::Params) {
        a.prepare(3200.0, 4096);
        b.prepare(3200.0, 4096);
        a.set_tempo(127.0);
        b.set_tempo(127.0);
        a.set_params(&params);
        b.set_params(&params);
        let input: Vec<_> = (0..4096).map(|n| (n as f32 * 0.073).sin()).collect();
        let mut l = input.clone();
        let mut r = input.clone();
        a.process(&mut l, &mut r);
        let mut small_l = input.clone();
        let mut small_r = input.clone();
        for (l, r) in small_l.chunks_mut(37).zip(small_r.chunks_mut(37)) {
            b.process(l, r);
        }
        assert_eq!(l, small_l);
        assert_eq!(r, small_r);
        a.set_tempo(91.0);
        b.set_tempo(91.0);
        a.set_params(&params);
        b.set_params(&params);
        let mut l = input.clone();
        let mut r = input.clone();
        a.process(&mut l, &mut r);
        let mut small_l = input.clone();
        let mut small_r = input;
        for (l, r) in small_l.chunks_mut(13).zip(small_r.chunks_mut(13)) {
            b.process(l, r);
        }
        assert_eq!(l, small_l);
        assert_eq!(r, small_r);
    }
    compare(
        TimeTransport::new(),
        TimeTransport::new(),
        TimeTransportParams::default(),
    );
    compare(
        Scratch::new(),
        Scratch::new(),
        ScratchParams {
            position: 0.37,
            ..Default::default()
        },
    );
    for model in PerformanceModel::ALL {
        compare(
            PerformanceRack::new(),
            PerformanceRack::new(),
            PerformanceRackParams {
                model,
                feedback: 0.3,
                loop_beats: 0.5,
                ..Default::default()
            },
        );
    }
}
