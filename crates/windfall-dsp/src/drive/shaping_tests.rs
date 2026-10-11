//! Measurements of the waveshaper and the overdrive: transfer curves and
//! harmonics against references worked out here, aliasing with and without
//! oversampling, headroom, edits and what a saved project holds.

use super::*;
use crate::asset::{CurvePoint, ShapeRef};
use crate::effect::{Effect, EffectParams};
use crate::param::ParamSet;

const RATE: f64 = 48_000.0;
const LATENCY: usize = DRIVE_LATENCY_SAMPLES;

fn sine(hz: f64, level: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| (level * (std::f64::consts::TAU * hz * n as f64 / RATE).sin()) as f32)
        .collect()
}

fn render<E: Effect + Default>(params: &E::Params, input: &[f32]) -> Vec<f32> {
    let mut effect = E::default();
    effect.prepare(RATE as f32, 512);
    effect.set_params(params);
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        effect.process(l, r);
    }
    assert_eq!(left, right, "equal channels come out equal");
    assert!(left.iter().all(|x| x.is_finite()));
    left
}

/// Amplitude of the component at `hz` over the last 19,200 samples, which
/// is a whole number of cycles of every multiple of 1 kHz at 48 kHz.
fn level(signal: &[f32], hz: f64) -> f64 {
    let window = &signal[signal.len() - 19_200..];
    let (mut re, mut im) = (0.0, 0.0);
    for (n, sample) in window.iter().enumerate() {
        let phase = std::f64::consts::TAU * hz * n as f64 / RATE;
        re += f64::from(*sample) * phase.cos();
        im += f64::from(*sample) * phase.sin();
    }
    (re * re + im * im).sqrt() * 2.0 / window.len() as f64
}

/// The same reading for a function of a unit sine, worked out directly:
/// what a memoryless curve does to a sine with no filter in the way.
fn reference_harmonic(curve: impl Fn(f64) -> f64, harmonic: usize) -> f64 {
    let steps = 1 << 16;
    let (mut re, mut im) = (0.0, 0.0);
    for n in 0..steps {
        let phase = std::f64::consts::TAU * n as f64 / steps as f64;
        let value = curve(phase.sin());
        re += value * (harmonic as f64 * phase).cos();
        im += value * (harmonic as f64 * phase).sin();
    }
    (re * re + im * im).sqrt() * 2.0 / steps as f64
}

/// Energy at four frequencies no harmonic of 5 kHz lands on. At 48 kHz the
/// 7th, 9th, 11th and 13th harmonics fold onto them.
fn alias_energy(signal: &[f32]) -> f64 {
    [3_000.0, 7_000.0, 13_000.0, 17_000.0]
        .into_iter()
        .map(|hz| level(signal, hz).powi(2))
        .sum()
}

fn shape(points: &[(f32, f32)]) -> ShapeRef {
    let points: Vec<CurvePoint> = points.iter().map(|(x, y)| CurvePoint::new(*x, *y)).collect();
    ShapeRef::new(&points).unwrap()
}

fn host_rate(shape: ShapeRef, bipolar: bool) -> WaveshaperParams {
    WaveshaperParams {
        oversampling: Oversampling::Off,
        bipolar,
        shape,
        ..Default::default()
    }
}

#[test]
fn a_drawn_shape_is_the_transfer_curve_in_both_readings() {
    // Bipolar: the left half of the drawing is negative input.
    let bent = host_rate(shape(&[(0.0, 0.25), (0.5, 0.5), (1.0, 1.0)]), true);
    let expected = |x: f32| if x < 0.0 { 0.5 * x } else { x };
    // Unipolar: drawn for the magnitude and mirrored.
    let folded = host_rate(shape(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.5)]), false);
    let fold = |x: f32| {
        let magnitude = x.abs();
        let value = if magnitude < 0.5 {
            2.0 * magnitude
        } else {
            1.5 - magnitude
        };
        value.copysign(x)
    };
    for n in -100..=100 {
        let x = n as f32 / 100.0;
        assert!((bent.transfer(x) - expected(x)).abs() < 1.0e-3, "bipolar at {x}");
        assert!((folded.transfer(x) - fold(x)).abs() < 2.0e-3, "unipolar at {x}");
        assert!((folded.transfer(-x) + folded.transfer(x)).abs() < 1.0e-6);
    }
    // Past the ends the curve holds its last value.
    assert!((bent.transfer(4.0) - 1.0).abs() < 1.0e-6);
    assert!((bent.transfer(-4.0) + 0.5).abs() < 1.0e-6);
    // The processor puts out exactly that curve, late.
    let ramp: Vec<f32> = (0..2_000).map(|n| n as f32 / 1_000.0 - 1.0).collect();
    for params in [bent, folded] {
        let output = render::<Waveshaper>(&params, &ramp);
        for (n, input) in ramp[..ramp.len() - LATENCY].iter().enumerate() {
            assert!(
                (output[n + LATENCY] - params.transfer(*input)).abs() < 1.0e-6,
                "sample {n}"
            );
        }
    }
    // A drawn shape replaces the knots; without one the knots are back.
    let mut knots = bent;
    knots.curve = IDENTITY_CURVE.map(|x| -x);
    assert!((knots.transfer(0.5) - 0.5).abs() < 1.0e-3);
    knots.shape = ShapeRef::NONE;
    assert!((knots.transfer(0.5) + 0.5).abs() < 1.0e-6);
}

#[test]
fn harmonics_of_a_drawn_clip_match_the_curve_and_follow_its_symmetry() {
    let input = sine(1_000.0, 1.0, 24_000);
    // A hard clip at half scale, drawn for the magnitude: odd harmonics.
    let clip = WaveshaperParams {
        oversampling: Oversampling::X4,
        bipolar: false,
        shape: shape(&[(0.0, 0.0), (0.5, 0.5), (1.0, 0.5)]),
        ..Default::default()
    };
    let output = render::<Waveshaper>(&clip, &input);
    let reference = |x: f64| x.clamp(-0.5, 0.5);
    for harmonic in [1, 3, 5, 7] {
        let measured = level(&output, 1_000.0 * harmonic as f64);
        let expected = reference_harmonic(reference, harmonic);
        assert!(
            (measured - expected).abs() < 0.02 * expected + 2.0e-4,
            "harmonic {harmonic}: {measured} against {expected}"
        );
    }
    for even in [2_000.0, 4_000.0, 6_000.0] {
        assert!(level(&output, even) < 1.0e-4, "{even} Hz in a symmetric clip");
    }
    // A curve that treats the two polarities differently adds even ones.
    let lopsided = WaveshaperParams {
        bipolar: true,
        shape: shape(&[(0.0, 0.25), (0.5, 0.5), (1.0, 1.0)]),
        ..clip
    };
    let output = render::<Waveshaper>(&lopsided, &input);
    let expected = reference_harmonic(|x| if x < 0.0 { 0.5 * x } else { x }, 2);
    assert!(expected > 0.05);
    assert!((level(&output, 2_000.0) - expected).abs() < 0.02 * expected + 2.0e-4);
}

#[test]
fn oversampling_lowers_the_aliasing_of_a_drawn_clip_and_of_the_overdrive() {
    let input = sine(5_000.0, 1.0, 24_000);
    let clip = shape(&[(0.0, 0.0), (0.2, 0.2), (1.0, 0.2)]);
    let shaped = |oversampling| {
        alias_energy(&render::<Waveshaper>(
            &WaveshaperParams {
                oversampling,
                bipolar: false,
                shape: clip,
                ..Default::default()
            },
            &input,
        ))
    };
    let naive = shaped(Oversampling::Off);
    assert!(naive > 1.0e-8, "the host rate aliases: {naive}");
    assert!(shaped(Oversampling::X2) < naive);
    assert!(shaped(Oversampling::X4) < naive / 16.0);
    assert!(shaped(Oversampling::X8) < naive / 16.0);

    // A gentle drive: the soft clipper's harmonics then fall off fast
    // enough for four times the rate to hold nearly all of them.
    let driven = |oversampling| {
        alias_energy(&render::<Overdrive>(
            &OverdriveParams {
                drive_db: 6.0,
                emphasis: 0.0,
                tone_hz: 18_000.0,
                oversampling,
                ..Default::default()
            },
            &input,
        ))
    };
    let naive = driven(Oversampling::Off);
    assert!(naive > 1.0e-10, "the host rate aliases: {naive}");
    assert!(driven(Oversampling::X4) < naive / 16.0);
    assert!(driven(Oversampling::X8) < naive / 16.0);
}

#[test]
fn a_bounded_curve_keeps_its_headroom_at_every_quality() {
    // 36 dB into a curve that never leaves -1..1. The decimation filter
    // may ring past the ceiling, by a bounded amount.
    let input = sine(997.0, 1.0, 24_000);
    for oversampling in Oversampling::ALL {
        let output = render::<Waveshaper>(
            &WaveshaperParams {
                input_db: 36.0,
                oversampling,
                ..Default::default()
            },
            &input,
        );
        let peak = output.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
        assert!((0.9..=1.25).contains(&peak), "{oversampling:?} peaks at {peak}");
        let driven = render::<Overdrive>(
            &OverdriveParams {
                drive_db: 36.0,
                hot: true,
                output_db: 0.0,
                oversampling,
                ..Default::default()
            },
            &input,
        );
        let peak = driven.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
        // The clipper's own ceiling is 1.175 at this asymmetry.
        assert!(peak < 1.6, "{oversampling:?} overdrive peaks at {peak}");
    }
}

#[test]
fn mix_blends_with_the_delayed_input_and_a_shape_edit_crossfades() {
    let input = sine(440.0, 0.4, 4_096);
    let dry = render::<Waveshaper>(
        &WaveshaperParams {
            mix: 0.0,
            input_db: 30.0,
            ..Default::default()
        },
        &input,
    );
    assert!(dry[..LATENCY].iter().all(|x| *x == 0.0));
    assert_eq!(&dry[LATENCY..], &input[..input.len() - LATENCY]);

    // A flat curve at one level, replaced by a flat curve at another: the
    // output walks from one to the other in 5 ms without a step.
    let low = host_rate(shape(&[(0.0, 0.6), (1.0, 0.6)]), true);
    let high = host_rate(shape(&[(0.0, 0.9), (1.0, 0.9)]), true);
    let mut effect = Waveshaper::default();
    effect.prepare(RATE as f32, 512);
    effect.set_params(&low);
    let mut left = [0.5_f32; 512];
    let mut right = left;
    effect.process(&mut left, &mut right);
    assert!((left[511] - 0.2).abs() < 1.0e-6);
    effect.set_params(&high);
    let mut left = [0.5_f32; 512];
    let mut right = left;
    effect.process(&mut left, &mut right);
    assert!(left[0] < 0.21, "the edit does not jump: {}", left[0]);
    assert!(left.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!(left.windows(2).all(|pair| pair[1] - pair[0] < 0.004));
    assert!((left[511] - 0.8).abs() < 1.0e-6);
    // After a reset the new curve is in force from the first sample out.
    effect.set_params(&low);
    effect.reset();
    let mut left = [0.5_f32; 64];
    let mut right = left;
    effect.process(&mut left, &mut right);
    assert!((left[LATENCY + 1] - 0.2).abs() < 1.0e-6);
}

#[test]
fn old_waveshaper_settings_load_and_a_shape_is_saved_as_its_points() {
    let knots: Vec<f32> = IDENTITY_CURVE.iter().map(|x| x * 0.5).collect();
    let old = serde_json::json!({ "inputDb": 6.0, "outputDb": -3.0, "curve": knots });
    let loaded: WaveshaperParams = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.input_db, 6.0);
    assert_eq!(loaded.curve[32], 0.5);
    assert_eq!(loaded.oversampling, Oversampling::X4);
    assert_eq!(loaded.mix, 1.0);
    assert!(loaded.bipolar);
    assert!(loaded.shape.is_none());

    let drawn = WaveshaperParams {
        shape: shape(&[(0.0, 0.0), (0.25, 0.75), (1.0, 1.0)]),
        oversampling: Oversampling::X8,
        ..loaded
    };
    let json = serde_json::to_value(EffectParams::Waveshaper(drawn)).unwrap();
    assert_eq!(json["type"], "waveshaper");
    assert_eq!(json["oversampling"], "x8");
    assert_eq!(json["shape"].as_array().unwrap().len(), 3);
    assert_eq!(json["shape"][1]["x"], 0.25);
    assert_eq!(json["shape"][1]["y"], 0.75);
    let back: EffectParams = serde_json::from_value(json).unwrap();
    assert_eq!(back, EffectParams::Waveshaper(drawn));
    assert_eq!(back.sanitized(), back);

    // The controls automation knows keep their numbers.
    assert_eq!(WaveshaperParams::index_of("inputDb"), Some(0));
    assert_eq!(WaveshaperParams::index_of("outputDb"), Some(1));
    assert_eq!(WaveshaperParams::index_of("curve.0"), Some(2));
    assert_eq!(WaveshaperParams::index_of("curve.32"), Some(34));
    assert_eq!(WaveshaperParams::index_of("oversampling"), Some(35));
    assert_eq!(WaveshaperParams::index_of("mix"), Some(36));
    assert_eq!(WaveshaperParams::index_of("bipolar"), Some(37));
    assert_eq!(WaveshaperParams::descriptors().len(), 38);
    // Setting a control leaves the drawn shape alone.
    let mut edited = drawn;
    assert!(edited.set(36, 0.5));
    assert_eq!(edited.shape, drawn.shape);
}

/// The overdrive's clipper, written out again from its description.
fn clip_reference(input: f64, drive_db: f64, asymmetry: f64, output_db: f64) -> f64 {
    let x = input * 10.0_f64.powf(drive_db / 20.0);
    let ceiling = if x >= 0.0 {
        1.0 - 0.45 * asymmetry
    } else {
        1.0 + 0.25 * asymmetry
    };
    x / (1.0 + x.abs() / ceiling) * 10.0_f64.powf(output_db / 20.0)
}

#[test]
fn the_overdrive_curve_matches_its_description_and_hot_adds_twenty_decibels() {
    for asymmetry in [0.0_f32, 0.7, 1.0] {
        let params = OverdriveParams {
            drive_db: 12.0,
            asymmetry,
            output_db: -3.0,
            ..Default::default()
        };
        for n in -20..=20 {
            let x = n as f32 / 10.0;
            let expected = clip_reference(f64::from(x), 12.0, f64::from(asymmetry), -3.0);
            assert!(
                (f64::from(params.transfer(x)) - expected).abs() < 1.0e-5,
                "asymmetry {asymmetry} at {x}"
            );
        }
        let hot = OverdriveParams { hot: true, ..params };
        let expected = clip_reference(0.05, 32.0, f64::from(asymmetry), -3.0);
        assert!((f64::from(hot.transfer(0.05)) - expected).abs() < 1.0e-5);
    }
    // Silence stays silence, and the curve never passes its ceilings.
    let params = OverdriveParams {
        asymmetry: 1.0,
        output_db: 0.0,
        ..Default::default()
    };
    assert_eq!(params.transfer(0.0), 0.0);
    assert!(params.transfer(1.0e3) < 0.55 && params.transfer(1.0e3) > 0.54);
    assert!(params.transfer(-1.0e3) > -1.25 && params.transfer(-1.0e3) < -1.24);
}

#[test]
fn overdrive_asymmetry_adds_even_harmonics_and_matches_the_curve() {
    let input = sine(1_000.0, 0.5, 24_000);
    let settings = |asymmetry| OverdriveParams {
        drive_db: 18.0,
        asymmetry,
        emphasis: 0.0,
        tone_hz: 18_000.0,
        output_db: 0.0,
        ..Default::default()
    };
    let even = render::<Overdrive>(&settings(0.0), &input);
    assert!(level(&even, 2_000.0) < 1.0e-4, "a symmetric clip has no second harmonic");
    let third = reference_harmonic(|x| clip_reference(0.5 * x, 18.0, 0.0, 0.0), 3);
    // The 18 kHz tone filter takes a little off 3 kHz.
    assert!((level(&even, 3_000.0) / third - 1.0).abs() < 0.05);

    let lopsided = render::<Overdrive>(&settings(1.0), &input);
    let second = reference_harmonic(|x| clip_reference(0.5 * x, 18.0, 1.0, 0.0), 2);
    assert!(second > 0.02);
    assert!((level(&lopsided, 2_000.0) / second - 1.0).abs() < 0.05);
    // The DC the lopsided curve makes is taken out again.
    let settled = &lopsided[lopsided.len() - 19_200..];
    let mean = settled.iter().map(|x| f64::from(*x)).sum::<f64>() / settled.len() as f64;
    assert!(mean.abs() < 2.0e-3, "mean {mean}");
}

#[test]
fn the_pre_band_sets_where_the_emphasis_starts() {
    // Quiet enough to stay on the straight part of the curve.
    let input = sine(1_000.0, 1.0e-3, 24_000);
    let gain = |pre_band_hz| {
        let output = render::<Overdrive>(
            &OverdriveParams {
                drive_db: 0.0,
                asymmetry: 0.0,
                emphasis: 1.0,
                tone_hz: 18_000.0,
                output_db: 0.0,
                pre_band_hz,
                ..Default::default()
            },
            &input,
        );
        level(&output, 1_000.0) / 1.0e-3
    };
    // Far below 1 kHz the whole emphasis is in: one plus three.
    assert!((gain(200.0) - 3.95).abs() < 0.15, "{}", gain(200.0));
    // Two octaves above it, most of the emphasis is out.
    assert!((gain(4_000.0) - 1.37).abs() < 0.1, "{}", gain(4_000.0));
}

#[test]
fn old_overdrive_settings_load_with_the_new_controls_at_their_defaults() {
    let old = serde_json::json!({
        "type": "overdrive", "driveDb": 12.0, "asymmetry": 0.2,
        "emphasis": 0.1, "toneHz": 3000.0, "outputDb": -3.0
    });
    let EffectParams::Overdrive(loaded) = serde_json::from_value(old).unwrap() else {
        panic!("wrong kind");
    };
    assert_eq!(loaded.drive_db, 12.0);
    assert_eq!(loaded.pre_band_hz, 900.0);
    assert!(!loaded.hot);
    assert_eq!(loaded.oversampling, Oversampling::X4);
    let names: Vec<&str> = OverdriveParams::descriptors().iter().map(|info| info.id).collect();
    assert_eq!(
        names,
        [
            "driveDb",
            "asymmetry",
            "emphasis",
            "toneHz",
            "outputDb",
            "preBandHz",
            "hot",
            "oversampling"
        ]
    );
    for (processor, latency) in [
        (Overdrive::default().latency_samples(), LATENCY),
        (Waveshaper::default().latency_samples(), LATENCY),
    ] {
        assert_eq!(processor, latency);
    }
}
