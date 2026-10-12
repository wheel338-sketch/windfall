//! Non-ignored admission budgets. Inputs and workloads are deterministic;
//! elapsed times must be recorded on the integration machine in release mode.
use std::time::Instant;
use windfall_dsp::drive::{
    Cabinet, DriveChainParams, DriveStage, DriveStageParams, GuitarRackParams, OverdriveParams,
    Oversampling, PedalParams, WaveshaperParams,
};
use windfall_dsp::performance::{PerformanceModel, PerformanceRackParams, TimeTransportParams};
use windfall_dsp::spectral::{PitchCorrectParams, PitchShiftMode, PitchShiftParams};
use windfall_dsp::{AnyEffect, EffectKind, EffectParams};

const RATE: f32 = 384_000.0;
// Debug checks catch pathological stalls; only release numbers admit a CPU
// budget. The factor is explicit and cannot be reported as a realtime pass.
const DEBUG_FACTOR: f64 = if cfg!(debug_assertions) { 64.0 } else { 1.0 };

fn material(n: usize) -> f32 {
    let t = n as f64 / RATE as f64;
    let voice = (1..=12)
        .map(|h| {
            let hz = 137.3 * h as f64;
            let envelope = (-((hz - 1300.0) / 600.0).powi(2)).exp();
            0.025 * envelope * (std::f64::consts::TAU * hz * t).sin()
        })
        .sum::<f64>();
    let pick = (-((n % 19_200) as f64) / 1200.0).exp();
    (voice
        + 0.18 * pick * (std::f64::consts::TAU * 719.9 * t).sin()
        + 0.05 * (std::f64::consts::TAU * 233.1 * t).sin()) as f32
}

fn measure(params: EffectParams, fraction: f64) {
    let kind = params.kind();
    for block in [1_usize, 64, 256, 2048] {
        let mut effect = AnyEffect::new(&params);
        effect.prepare(RATE, block);
        effect.set_params(&params);
        if let AnyEffect::PitchCorrect(pitch) = &mut effect {
            pitch.set_harmony_notes([48, 52, 55, 60]);
        }
        let frames = 4096.max(block * 8);
        let mut left = [0.0; 2048];
        let mut right = [0.0; 2048];
        // Warm detector/LPC and capture rings outside the timed interval.
        let warm_frames = match kind {
            EffectKind::PerformanceRack => (RATE * 1.1) as usize,
            EffectKind::PitchShift | EffectKind::PitchCorrect | EffectKind::TimeTransport => {
                (RATE * 0.1) as usize
            }
            _ => 4096,
        };
        let source: Vec<_> = (0..warm_frames + frames)
            .map(|n| {
                if kind == EffectKind::PitchCorrect {
                    let phase = std::f64::consts::TAU * 137.3 * n as f64 / RATE as f64;
                    (0.25 * phase.sin() + 0.05 * (2.0 * phase).sin() + 0.025 * (3.0 * phase).sin())
                        as f32
                } else {
                    material(n)
                }
            })
            .collect();
        for chunk in source[..warm_frames].chunks(block) {
            left[..chunk.len()].copy_from_slice(chunk);
            for (r, l) in right[..chunk.len()].iter_mut().zip(chunk) {
                *r = -*l;
            }
            effect.process(&mut left[..chunk.len()], &mut right[..chunk.len()]);
        }
        let mut elapsed = 0.0_f64;
        let mut maximum = 0.0_f64;
        for (index, chunk) in source[warm_frames..].chunks(block).enumerate() {
            left[..chunk.len()].copy_from_slice(chunk);
            for (r, l) in right[..chunk.len()].iter_mut().zip(chunk) {
                *r = -*l;
            }
            let mut edited = params;
            if let EffectParams::TimeTransport(p) = &mut edited {
                p.trigger = index % 2 == 0;
            }
            let start = Instant::now();
            effect.set_params(&edited);
            effect.process(&mut left[..chunk.len()], &mut right[..chunk.len()]);
            let cost = start.elapsed().as_secs_f64();
            elapsed += cost;
            maximum = maximum.max(cost);
            assert!(
                left[..chunk.len()]
                    .iter()
                    .chain(&right[..chunk.len()])
                    .all(|s| s.is_finite())
            );
        }
        let realtime_share = elapsed * RATE as f64 / frames as f64;
        let mut reset = f64::INFINITY;
        let mut maximum_reset = 0.0_f64;
        for _ in 0..8 {
            let start = Instant::now();
            effect.reset();
            let cost = start.elapsed().as_secs_f64();
            reset = reset.min(cost);
            maximum_reset = maximum_reset.max(cost);
        }
        println!(
            "P2 {kind:?} params={params:?} block={block} rate={RATE} CPU-share={realtime_share:.6} max-callback-us={:.3} reset-best-us={:.3} reset-max-us={:.3} debug-factor={DEBUG_FACTOR}",
            maximum * 1e6,
            reset * 1e6,
            maximum_reset * 1e6
        );
        // The one-frame diagnostic includes timer and dispatch overhead. Real
        // device admission is based on blocks >=64, not a fabricated 1-frame
        // device promise. We still exercise and report every one-frame call.
        if block >= 64 {
            assert!(
                realtime_share <= fraction * DEBUG_FACTOR,
                "{kind:?} block={block}: CPU {realtime_share} exceeds declared share {fraction} (debug factor {DEBUG_FACTOR})"
            );
            assert!(
                maximum <= block as f64 / RATE as f64 * DEBUG_FACTOR + 0.005,
                "{kind:?} block={block}: callback {maximum}s exceeds one deadline plus 5ms scheduling allowance"
            );
        }
        assert!(
            reset <= (64.0 / RATE as f64) * DEBUG_FACTOR,
            "{kind:?}: reset {reset}s exceeds a 64-frame deadline"
        );
        assert!(
            maximum_reset <= (64.0 / RATE as f64) * DEBUG_FACTOR + 0.005,
            "{kind:?}: maximum reset {maximum_reset}s exceeds the deadline plus scheduling allowance"
        );
    }
}

#[test]
fn maximum_quality_drive_guitar_and_modular_graphs_have_declared_e4_cpu_budgets() {
    // Keep every original assertion/budget, but complete independent workloads
    // after an earlier one fails so integration receives all measurements.
    let mut failures = Vec::new();
    let mut measure = |params: EffectParams, fraction| {
        if let Err(error) = std::panic::catch_unwind(|| measure(params, fraction)) {
            let message = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            failures.push(format!("{:?}: {message}", params.kind()));
        }
    };
    let shape = windfall_dsp::asset::ShapeRef::new(&[
        windfall_dsp::asset::CurvePoint::new(0.0, 0.0),
        windfall_dsp::asset::CurvePoint::new(0.4, 0.0),
        windfall_dsp::asset::CurvePoint::new(0.6, 1.0),
        windfall_dsp::asset::CurvePoint::new(1.0, 1.0),
    ])
    .unwrap();
    measure(
        EffectParams::Overdrive(OverdriveParams {
            oversampling: Oversampling::X8,
            hot: true,
            ..Default::default()
        }),
        0.20,
    );
    measure(
        EffectParams::Waveshaper(WaveshaperParams {
            shape,
            oversampling: Oversampling::X8,
            ..Default::default()
        }),
        0.20,
    );
    for cabinet in Cabinet::ALL {
        measure(
            EffectParams::GuitarRack(GuitarRackParams {
                pedals: [PedalParams {
                    on: true,
                    amount: 0.75,
                    tone: 0.75,
                }; 11],
                cabinet,
                oversampling: Oversampling::X8,
                ..Default::default()
            }),
            0.75,
        );
    }
    for models in [
        [DriveStage::Fuzz, DriveStage::Chorus, DriveStage::Cabinet],
        [
            DriveStage::Lowpass,
            DriveStage::Highpass,
            DriveStage::Overdrive,
        ],
        [DriveStage::Cabinet; 3],
    ] {
        measure(
            EffectParams::DriveChain(DriveChainParams {
                stages: models.map(|model| DriveStageParams {
                    model,
                    gain_db: 36.0,
                    depth: 1.0,
                    resonance: 4.0,
                    rate_hz: 10.0,
                    ..Default::default()
                }),
                oversampling: Oversampling::X8,
                ..Default::default()
            }),
            0.75,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn all_twelve_xy_models_and_transient_capture_reset_have_declared_callback_budgets() {
    measure(
        EffectParams::TimeTransport(TimeTransportParams {
            auto_trigger: true,
            loop_beats: 0.125,
            ..Default::default()
        }),
        0.25,
    );
    for model in PerformanceModel::ALL {
        measure(
            EffectParams::PerformanceRack(PerformanceRackParams {
                model,
                xy_enabled: true,
                x: 1.0,
                y: 1.0,
                feedback: 0.95,
                loop_beats: 16.0,
                ..Default::default()
            }),
            0.25,
        );
    }
}

#[test]
fn representative_vocal_mix_and_four_harmony_workloads_have_declared_spectral_cpu_budgets() {
    for mode in [PitchShiftMode::Mix, PitchShiftMode::Voice] {
        measure(
            EffectParams::PitchShift(PitchShiftParams {
                mode,
                semitones: 7.0,
                formant_semitones: 5.0,
                ..Default::default()
            }),
            0.50,
        );
    }
    // Set all four real MIDI voices, rather than measuring released grains.
    let params = EffectParams::PitchCorrect(PitchCorrectParams {
        harmony_mix: 1.0,
        speed_ms: 0.0,
        ..Default::default()
    });
    measure(params, 0.50);
    assert_eq!(params.kind(), EffectKind::PitchCorrect);
}
