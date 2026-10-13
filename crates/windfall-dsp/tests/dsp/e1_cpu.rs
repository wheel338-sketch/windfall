//! Non-ignored E1 CPU gate plus deterministic workload/allocation regressions.
//! Integrator: run in release, serially, with --nocapture on the target machine.
//! Debug numbers are diagnostic only and have a separate, wider development budget.
use std::time::{Duration, Instant};

use windfall_dsp::{AnyEffect, EffectKind, EffectParams, ParamKind};

const KINDS: [EffectKind; 9] = [
    EffectKind::SevenBand,
    EffectKind::Spreader,
    EffectKind::StereoEnhancer,
    EffectKind::PanLfo,
    EffectKind::MorphEq,
    EffectKind::FilterBank,
    EffectKind::SelectableFilter,
    EffectKind::BandSplit,
    EffectKind::Eq,
];

/// Every filter/band is working. Extreme is a reproducible range-end workload,
/// not a bypass, a flat curve or an empty/silent input.
fn workload(kind: EffectKind, extreme: bool) -> EffectParams {
    let mut params = kind.default_params();
    for (index, info) in kind.descriptors().iter().enumerate() {
        let value = match info.kind {
            ParamKind::Float => {
                if extreme {
                    if index % 2 == 0 { info.max } else { info.min }
                } else {
                    info.min + 0.65 * (info.max - info.min)
                }
            }
            ParamKind::Integer => info.max,
            ParamKind::Toggle => 1.0,
            ParamKind::Choice => {
                // Filter-bank Dry/Bypass skip the IIR. Choose eight real filters.
                if info.id.starts_with("slots.") && info.id.ends_with(".mode") {
                    1.0 + (index % 3) as f32
                } else if info.id == "mode" {
                    // An active resonant bell, whose nonzero gain exercises the SVF.
                    5.0
                } else {
                    info.max
                }
            }
        };
        assert!(params.set(index, value));
    }
    // Keep gain/mix/width and all graphic gains active at both workloads.
    for (index, info) in kind.descriptors().iter().enumerate() {
        let value = if info.id == "mix" || info.id == "gain" || info.id == "amount" {
            Some(1.0)
        } else if info.id == "width" {
            Some(if extreme { 2.0 } else { 1.4 })
        } else if info.id.contains("gainDb") || info.id.contains("gainsDb") {
            Some(if index % 2 == 0 {
                if extreme { 18.0 } else { 3.0 }
            } else if extreme {
                -18.0
            } else {
                -3.0
            })
        } else if info.id == "sync" {
            Some(0.0) // exercise the maximum free-running LFO rate
        } else {
            None
        };
        if let Some(value) = value {
            assert!(params.set(index, value));
        }
    }
    if let EffectParams::Eq(eq) = &mut params {
        // Keep a real passband: blindly alternating frequency range ends puts
        // both cuts at 20 Hz. All seven bands, including the four-stage cuts,
        // remain active while frequency, gain and Q are automated below.
        eq.low_cut.frequency_hz = if extreme { 20.0 } else { 40.0 };
        eq.high_cut.frequency_hz = if extreme { 20_000.0 } else { 18_000.0 };
        for (band, hz, gain, max_q) in [
            (&mut eq.low_shelf, 100.0, 1.0, 2.0),
            (&mut eq.peak1, 400.0, -1.0, 18.0),
            (&mut eq.peak2, 1_000.0, 1.0, 18.0),
            (&mut eq.peak3, 6_300.0, -1.0, 18.0),
            (&mut eq.high_shelf, 16_000.0, 1.0, 2.0),
        ] {
            band.frequency_hz = if extreme { hz } else { hz * 0.8 };
            band.gain_db = gain * if extreme { 24.0 } else { 3.0 };
            band.q = if extreme { max_q } else { 1.0 };
        }
        // Measure the filters, without an unrelated +24 dB output multiplier.
        eq.output_gain_db = 0.0;
    }
    params.sanitized()
}

fn input(frame: usize, rate: f32) -> [f32; 2] {
    let t = frame as f64 / f64::from(rate);
    let x = 0.1 * (std::f64::consts::TAU * 63.0 * t).sin()
        + 0.07 * (std::f64::consts::TAU * 1_000.0 * t).sin()
        + 0.05 * (std::f64::consts::TAU * 6_300.0 * t).sin();
    [
        x as f32,
        (0.6 * x + 0.03 * (std::f64::consts::TAU * 160.0 * t).sin()) as f32,
    ]
}

fn render(kind: EffectKind, extreme: bool, block: usize) -> Vec<[f32; 2]> {
    let rate = 48_000.0;
    let params = workload(kind, extreme);
    let mut effect = AnyEffect::new(&params);
    effect.prepare(rate, block);
    effect.set_params(&params);
    effect.set_tempo(137.0);
    let frames = 4_096;
    let (mut left, mut right) = (vec![0.0; block], vec![0.0; block]);
    let mut result = Vec::with_capacity(frames);
    for from in (0..frames).step_by(block) {
        let count = block.min(frames - from);
        for i in 0..count {
            [left[i], right[i]] = input(from + i, rate);
        }
        assert_eq!(
            crate::realtime::allocator_calls(|| {
                effect.process(&mut left[..count], &mut right[..count]);
            }),
            0,
            "{kind:?}"
        );
        result.extend((0..count).map(|i| [left[i], right[i]]));
    }
    assert_eq!(
        crate::realtime::allocator_calls(|| {
            effect.set_params(&workload(kind, !extreme));
            effect.set_tempo(75.0);
            effect.reset();
            effect.process(&mut left, &mut right);
        }),
        0,
        "{kind:?} edit/reset"
    );
    result
}

#[test]
fn e1_cpu_workloads_are_active_finite_partition_invariant_and_allocate_nothing() {
    for kind in KINDS {
        for extreme in [false, true] {
            let single = render(kind, extreme, 1);
            let blocks = render(kind, extreme, 257);
            assert!(single.iter().flatten().all(|sample| sample.is_finite()));
            assert!(
                single.iter().flatten().any(|sample| sample.abs() > 1e-6),
                "silent workload {kind:?} extreme={extreme}"
            );
            let difference = single
                .iter()
                .flatten()
                .zip(blocks.iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                difference < 2e-5,
                "{kind:?} extreme={extreme}: {difference}"
            );
        }
    }
}

#[test]
fn e1_parametric_eq_workloads_keep_all_seven_bands_active_during_automation() {
    let extreme = workload(EffectKind::Eq, true);
    let active = workload(EffectKind::Eq, false);
    for params in [&active, &extreme] {
        let EffectParams::Eq(eq) = params else {
            panic!("Eq must instantiate the seven-band parametric processor");
        };
        assert!(eq.low_cut.enabled && eq.high_cut.enabled);
        assert_eq!(eq.low_cut.slope, windfall_dsp::CutSlope::Db48);
        assert_eq!(eq.high_cut.slope, windfall_dsp::CutSlope::Db48);
        assert!(eq.low_cut.frequency_hz < eq.high_cut.frequency_hz);
        for band in [eq.low_shelf, eq.peak1, eq.peak2, eq.peak3, eq.high_shelf] {
            assert!(band.enabled && band.gain_db != 0.0);
        }
    }
    assert_ne!(
        active, extreme,
        "the timed edit must change real parameters"
    );
    let EffectParams::Eq(eq) = extreme else {
        unreachable!()
    };
    assert_eq!((eq.low_cut.q, eq.high_cut.q), (8.0, 8.0));
    assert_eq!((eq.low_shelf.q, eq.high_shelf.q), (2.0, 2.0));
    assert_eq!((eq.peak1.q, eq.peak2.q, eq.peak3.q), (18.0, 18.0, 18.0));
    for band in [eq.low_shelf, eq.peak1, eq.peak2, eq.peak3, eq.high_shelf] {
        assert_eq!(band.gain_db.abs(), 24.0);
    }
}

/// Minimum of three performance-core trials plus one unrestricted diagnostic. Timed work includes copying source
/// buffers, one real parameter edit per block, and the full processor call.
/// Construction, preparation, input synthesis and printing are outside timing.
fn cpu_share(kind: EffectKind, extreme: bool, rate: f32, block: usize, edits: bool) -> f64 {
    let params = workload(kind, extreme);
    let alternate = workload(kind, !extreme);
    let mut effect = AnyEffect::new(&params);
    effect.prepare(rate, block);
    effect.set_params(&params);
    effect.set_tempo(137.0);
    let sources: Vec<_> = (0..block).map(|n| input(n, rate)).collect();
    let source_left: Vec<_> = sources.iter().map(|frame| frame[0]).collect();
    let source_right: Vec<_> = sources.iter().map(|frame| frame[1]).collect();
    let (mut left, mut right) = (vec![0.0; block], vec![0.0; block]);
    let blocks = (rate as usize / 4 / block).max(1);
    // Settle warm-up before recording, retaining active nonzero input.
    for _ in 0..blocks {
        left.copy_from_slice(&source_left);
        right.copy_from_slice(&source_right);
        effect.process(&mut left, &mut right);
    }
    let mut trials = [Duration::ZERO; 4];
    for (index, trial) in trials.iter_mut().enumerate() {
        let _core = (index > 0).then(super::deadline_cpu::PerformanceCore::enter);
        let start = Instant::now();
        for index in 0..blocks {
            left.copy_from_slice(&source_left);
            right.copy_from_slice(&source_right);
            // Changes include costly coefficient/control updates, not just static DSP.
            // Three quarters of blocks use the labelled workload.
            if edits {
                effect.set_params(if index % 4 == 0 { &alternate } else { &params });
            }
            effect.process(&mut left, &mut right);
            std::hint::black_box((&left, &right));
        }
        *trial = Duration::from_secs_f64(
            start.elapsed().as_secs_f64() / (blocks * block) as f64 * f64::from(rate),
        );
    }
    assert!(left.iter().chain(&right).all(|value| value.is_finite()));
    let unrestricted = trials[0];
    super::deadline_cpu::report(
        "E1 CPU share (seconds per audio second)",
        &mut trials[1..],
        unrestricted,
    )
    .as_secs_f64()
}

#[test]
fn e1_active_and_extreme_cpu_measurements_meet_the_declared_target_budget() {
    let profile = if cfg!(debug_assertions) {
        "debug-diagnostic"
    } else {
        "release-acceptance"
    };
    println!(
        "E1_CPU profile={profile} arch={} os={} median_trials=5; includes block edits",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    for kind in KINDS {
        // Fraction of one logical core's audio duration, including parameter writes.
        // Seven fixed bands have the stricter low-CPU gate. Eq has the same
        // declared 35% release budget as the other movable/filter-bank EQs.
        let release_budget = if kind == EffectKind::SevenBand {
            0.10
        } else {
            0.35
        };
        let budget = if cfg!(debug_assertions) {
            release_budget * 40.0
        } else {
            release_budget
        };
        for rate in [48_000.0, 96_000.0] {
            for block in [64, 256] {
                for extreme in [false, true] {
                    for edits in [false, true] {
                        let share = cpu_share(kind, extreme, rate, block, edits);
                        println!(
                            "E1_CPU kind={kind:?} extreme={extreme} edits={edits} rate={rate} block={block} cpu_pct={:.4} budget_pct={:.2} ns_per_frame={:.2}",
                            share * 100.0,
                            budget * 100.0,
                            share / f64::from(rate) * 1e9
                        );
                        assert!(
                            share <= budget,
                            "{kind:?} extreme={extreme} edits={edits} rate={rate} block={block}: {share:.5} > {budget:.5}; inspect target load/profile before changing budgets"
                        );
                    }
                }
            }
        }
    }
}
