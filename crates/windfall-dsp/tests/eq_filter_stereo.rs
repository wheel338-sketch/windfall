//! Measured acceptance of the equalizer, filter and stereo effects.
//!
//! Every response is measured from audio that went through the registered
//! processor and compared with a reference written here: the analog
//! prototype of each section, evaluated at the frequency the bilinear
//! transform maps a digital frequency to. Nothing in a reference calls the
//! crate's coefficient code.

use std::f64::consts::{PI, TAU};

use windfall_dsp::eqbank::{SEVEN_BAND_FREQUENCIES_HZ, SevenBandParams};
use windfall_dsp::{AnyEffect, EffectParams, EffectSlot};

const RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];

/// A complex number, real part first.
type Complex = (f64, f64);

fn multiply(a: Complex, b: Complex) -> Complex {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

fn divide(a: Complex, b: Complex) -> Complex {
    let size = b.0 * b.0 + b.1 * b.1;
    (
        (a.0 * b.0 + a.1 * b.1) / size,
        (a.1 * b.0 - a.0 * b.1) / size,
    )
}

fn magnitude_db(value: Complex) -> f64 {
    20.0 * value.0.hypot(value.1).log10()
}

fn phase(value: Complex) -> f64 {
    value.1.atan2(value.0)
}

/// The smallest angle between two phases.
fn phase_gap(a: f64, b: f64) -> f64 {
    let gap = (a - b).rem_euclid(TAU);
    gap.min(TAU - gap)
}

/// The second-order analog prototypes, with the corner at 1 rad/s.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Lowpass,
    Highpass,
    /// Unity gain at the center.
    Bandpass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

/// `shape` at analog frequency `w` (relative to its corner), with quality
/// `q` and a gain of `db` for the shapes that have one.
fn prototype(shape: Shape, w: f64, q: f64, db: f64) -> Complex {
    let a = 10.0_f64.powf(db / 40.0);
    // s = jw, so s^2 = -w^2.
    let poly = |c2: f64, c1: f64, c0: f64| -> Complex { (c0 - c2 * w * w, c1 * w) };
    let plain = poly(1.0, 1.0 / q, 1.0);
    match shape {
        Shape::Lowpass => divide((1.0, 0.0), plain),
        Shape::Highpass => divide(poly(1.0, 0.0, 0.0), plain),
        Shape::Bandpass => divide(poly(0.0, 1.0 / q, 0.0), plain),
        Shape::Notch => divide(poly(1.0, 0.0, 1.0), plain),
        Shape::Peak => divide(poly(1.0, a / q, 1.0), poly(1.0, 1.0 / (a * q), 1.0)),
        Shape::LowShelf => multiply(
            (a, 0.0),
            divide(poly(1.0, a.sqrt() / q, a), poly(a, a.sqrt() / q, 1.0)),
        ),
        Shape::HighShelf => multiply(
            (a, 0.0),
            divide(poly(a, a.sqrt() / q, 1.0), poly(1.0, a.sqrt() / q, a)),
        ),
    }
}

/// The response at `hz` of a digital section made from `shape` by the
/// bilinear transform with its corner at `corner_hz`.
fn section(shape: Shape, corner_hz: f64, q: f64, db: f64, hz: f64, rate: f64) -> Complex {
    // The processors hold a corner below 0.49 of the sample rate.
    let corner = corner_hz.clamp(1.0, 0.49 * rate);
    let warped = (PI * hz / rate).tan() / (PI * corner / rate).tan();
    prototype(shape, warped, q, db)
}

/// A prepared processor of whatever kind `params` belong to.
fn effect(params: &EffectParams, rate: f32, max_block: usize) -> AnyEffect {
    let mut effect = AnyEffect::new(params);
    effect.prepare(rate, max_block);
    assert!(effect.set_params(params));
    effect
}

/// What a processor does to a steady sine at `hz`, measured on the left
/// channel over the last half of one second: gain and phase as one complex
/// number. `hz` must be a multiple of 2 so the window holds whole cycles.
fn measured(params: &EffectParams, rate: f32, hz: f64) -> Complex {
    let frames = rate as usize;
    let window = rate as usize / 2;
    let angle = |n: usize| TAU * hz * n as f64 / f64::from(rate);
    let mut left: Vec<f32> = (0..frames).map(|n| (0.1 * angle(n).sin()) as f32).collect();
    let mut right: Vec<f32> = left.iter().map(|sample| sample * -0.5).collect();
    let mut effect = effect(params, rate, 512);
    for (left, right) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        effect.process(left, right);
    }
    let (mut in_phase, mut quadrature) = (0.0_f64, 0.0_f64);
    for n in frames - window..frames {
        in_phase += f64::from(left[n]) * angle(n).sin();
        quadrature += f64::from(left[n]) * angle(n).cos();
    }
    let scale = 2.0 / (0.1 * window as f64);
    (in_phase * scale, quadrature * scale)
}

/// A deterministic signal with energy across the band, for exact
/// comparisons of two renderings.
fn busy(frames: usize, seed: u32) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..frames)
        .map(|n| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = state as f32 / u32::MAX as f32 - 0.5;
            0.3 * noise + 0.2 * (n as f32 * 0.021).sin()
        })
        .collect()
}

/// Renders `frames` frames through a slot, changing from `start` to `end`
/// settings half way, in blocks of the given lengths.
fn rendered(
    start: &EffectParams,
    end: &EffectParams,
    frames: usize,
    blocks: &[usize],
) -> (Vec<f32>, Vec<f32>) {
    let mut slot = EffectSlot::new(AnyEffect::new(start));
    slot.prepare(48_000.0, 512);
    slot.set_tempo(120.0);
    assert!(slot.set_params(start));
    let mut left = busy(frames, 7);
    let mut right = busy(frames, 99);
    let half = frames / 2;
    let mut cursor = 0;
    let mut block = 0;
    while cursor < frames {
        if cursor == half {
            assert!(slot.set_params(end));
        }
        let limit = if cursor < half { half } else { frames };
        let next = (cursor + blocks[block % blocks.len()]).min(limit);
        slot.process(&mut left[cursor..next], &mut right[cursor..next]);
        cursor = next;
        block += 1;
    }
    (left, right)
}

/// The same audio comes out however the host cuts it into blocks, through
/// a change of settings too.
fn assert_block_invariant(start: &EffectParams, end: &EffectParams) {
    let whole = rendered(start, end, 9_600, &[512]);
    let partitions: [&[usize]; 3] = [&[1], &[7, 64, 127], &[256, 33, 3, 480]];
    for blocks in partitions {
        let pieces = rendered(start, end, 9_600, blocks);
        assert_eq!(whole.0, pieces.0, "left differs for blocks {blocks:?}");
        assert_eq!(whole.1, pieces.1, "right differs for blocks {blocks:?}");
    }
    assert!(whole.0.iter().chain(&whole.1).all(|sample| sample.is_finite()));
}

/// Switching the slot off and on never jumps by more than `largest_step`
/// from one sample to the next, and a slot that is off passes its input
/// exactly. The input is a 220 Hz sine at half scale, whose own largest
/// step is 0.0144.
fn assert_bypass_is_continuous(params: &EffectParams, largest_step: f32) {
    let mut slot = EffectSlot::new(AnyEffect::new(params));
    slot.prepare(48_000.0, 256);
    slot.set_tempo(120.0);
    assert!(slot.set_params(params));
    assert_eq!(slot.latency_samples(), 0);
    let input: Vec<f32> = (0..14_400)
        .map(|n| 0.5 * (TAU * 220.0 * n as f64 / 48_000.0).sin() as f32)
        .collect();
    let mut left = input.clone();
    let mut right = input.clone();
    for (index, (left, right)) in left.chunks_mut(240).zip(right.chunks_mut(240)).enumerate() {
        match index {
            20 => slot.set_enabled(false),
            40 => slot.set_enabled(true),
            _ => {}
        }
        slot.process(left, right);
    }
    for side in [&left, &right] {
        // The first frames are the processor settling on a sine that
        // starts from silence, which is not a switch.
        let step = side[480..]
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(step <= largest_step, "a step of {step} is a click");
        // 4800..9600 is off. The fade takes 10 ms, 480 frames.
        assert_eq!(&side[5_400..9_600], &input[5_400..9_600]);
    }
}

fn seven(gains_db: [f32; 7]) -> EffectParams {
    EffectParams::SevenBand(SevenBandParams { gains_db })
}

/// Seven bells in series at the fixed centers, each with a Q of 1.4.
fn seven_reference(gains_db: [f32; 7], hz: f64, rate: f64) -> Complex {
    let mut total = (1.0, 0.0);
    for (center, gain) in SEVEN_BAND_FREQUENCIES_HZ.iter().zip(gains_db) {
        total = multiply(
            total,
            section(Shape::Peak, f64::from(*center), 1.4, f64::from(gain), hz, rate),
        );
    }
    total
}

#[test]
fn seven_band_matches_seven_fixed_bells_in_level_and_phase() {
    let curves = [
        [6.0, -4.0, 3.0, -9.0, 12.0, -6.0, 5.0],
        [-18.0, 18.0, -18.0, 18.0, -18.0, 18.0, -18.0],
        [0.0, 0.0, 0.0, 7.5, 0.0, 0.0, 0.0],
    ];
    for rate in RATES {
        for gains in curves {
            for hz in [
                30.0, 64.0, 100.0, 160.0, 250.0, 400.0, 640.0, 1_000.0, 1_600.0, 2_500.0,
                4_000.0, 6_300.0, 10_000.0, 16_000.0,
            ] {
                let measured = measured(&seven(gains), rate, hz);
                let expected = seven_reference(gains, hz, f64::from(rate));
                assert!(
                    (magnitude_db(measured) - magnitude_db(expected)).abs() < 0.05,
                    "level at {hz} Hz, {rate} Hz, {gains:?}: {} against {}",
                    magnitude_db(measured),
                    magnitude_db(expected)
                );
                assert!(
                    phase_gap(phase(measured), phase(expected)) < 0.01,
                    "phase at {hz} Hz, {rate} Hz, {gains:?}"
                );
            }
        }
    }
}

#[test]
fn seven_band_centers_are_fixed_and_each_fader_moves_only_its_band() {
    for (index, center) in SEVEN_BAND_FREQUENCIES_HZ.iter().enumerate() {
        let mut gains = [0.0; 7];
        gains[index] = 12.0;
        let at_center = magnitude_db(measured(&seven(gains), 48_000.0, f64::from(*center)));
        assert!((at_center - 12.0).abs() < 0.05, "band {index}: {at_center}");
        // Two bands away the bell has all but gone.
        for other in SEVEN_BAND_FREQUENCIES_HZ
            .iter()
            .enumerate()
            .filter(|(at, _)| at.abs_diff(index) >= 2)
            .map(|(_, hz)| f64::from(*hz))
        {
            let leak = magnitude_db(measured(&seven(gains), 48_000.0, other));
            assert!(leak.abs() < 0.8, "band {index} moved {other} Hz by {leak}");
        }
    }
}

#[test]
fn seven_band_at_rest_costs_nothing_and_changes_nothing() {
    // The low CPU cost at rest: a band at 0 dB runs no filter at all, so
    // the flat equalizer returns its input bit for bit.
    let input = busy(4_096, 3);
    let mut left = input.clone();
    let mut right = input.clone();
    let mut effect = effect(&seven([0.0; 7]), 48_000.0, 512);
    for (left, right) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        effect.process(left, right);
    }
    assert_eq!(left, input);
    assert_eq!(right, input);
    assert_eq!(effect.latency_samples(), 0);
    assert_eq!(effect.tail_samples(), 0);
}

#[test]
fn seven_band_is_block_invariant_and_bypasses_without_a_click() {
    assert_block_invariant(
        &seven([3.0, -5.0, 8.0, -3.0, 2.0, 4.0, -6.0]),
        &seven([-6.0, 9.0, -4.0, 2.0, -7.0, 5.0, 3.0]),
    );
    // +12 dB at 160 Hz lifts the 220 Hz test tone by about 2.6 times.
    assert_bypass_is_continuous(&seven([0.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 0.06);
}

/// Wall-clock comparison, so it is not part of the ordinary run:
/// `cargo test -p windfall-dsp --release --test eq_filter_stereo -- --ignored`.
#[test]
#[ignore = "timing: run in release on a quiet machine"]
fn seven_band_costs_less_than_the_parametric_equalizer_with_every_band_working() {
    use std::hint::black_box;
    use std::time::Instant;
    use windfall_dsp::EffectKind;

    let time = |params: &EffectParams| {
        let mut effect = effect(params, 48_000.0, 512);
        let mut left = busy(512, 5);
        let mut right = busy(512, 6);
        let start = Instant::now();
        for _ in 0..4_000 {
            effect.process(black_box(&mut left), black_box(&mut right));
            // Keep the level steady: the filters work in place.
            left[0] = 0.5;
            right[0] = -0.5;
        }
        start.elapsed()
    };
    let mut parametric = EffectKind::Eq.default_params();
    for (index, info) in EffectKind::Eq.descriptors().iter().enumerate() {
        if info.id.ends_with("enabled") {
            parametric.set(index, 1.0);
        } else if info.id.ends_with("gainDb") {
            parametric.set(index, 6.0);
        }
    }
    let graphic = time(&seven([6.0; 7]));
    let full = time(&parametric);
    assert!(
        graphic <= full,
        "seven fixed bands took {graphic:?}, the parametric equalizer {full:?}"
    );
}
