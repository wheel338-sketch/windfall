//! Properties every effect must have, written once and applied to all of
//! them.

use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::{
    Compressor, CompressorParams, CutSlope, Delay, DelayMode, DelayParams, DetectorMode, Effect,
    EqParams, Limiter, LimiterParams, NoteDivision, ParamSet, ParametricEq, Reverb, ReverbParams,
};

use crate::support::{
    RATE, RATES, assert_finite, has_subnormals, impulse, noise, peak, prepared, random_params, rms,
    run, silence, sine, square, steepest,
};

/// Inputs chosen to break things: nothing at all, a constant, the loudest
/// legal signal, single spikes, noise far over full scale, and values so
/// small that careless arithmetic slows to a crawl on them.
fn hostile_inputs(frames: usize, seed: u32) -> Vec<(&'static str, Vec<f32>, Vec<f32>)> {
    let mut rng = Rng::new(seed);
    let spikes: Vec<f32> = (0..frames)
        .map(|_| {
            if rng.unipolar() < 0.01 {
                rng.bipolar().signum()
            } else {
                0.0
            }
        })
        .collect();
    let tiny: Vec<f32> = (0..frames)
        .map(|n| if n % 2 == 0 { 1.0e-39 } else { -3.0e-42 })
        .collect();
    let burst: Vec<f32> = (0..frames)
        .map(|n| if n < frames / 4 { rng.bipolar() } else { 0.0 })
        .collect();
    vec![
        ("silence", silence(frames), silence(frames)),
        ("DC", vec![1.0; frames], vec![-1.0; frames]),
        ("square", square(64, 1.0, frames), square(50, 1.0, frames)),
        (
            "impulses",
            spikes.clone(),
            impulse(frames, frames / 3, -1.0),
        ),
        (
            "noise at +24 dB",
            noise(seed, 15.85, frames),
            noise(seed + 1, 15.85, frames),
        ),
        ("subnormal input", tiny.clone(), tiny),
        ("burst then silence", burst, spikes),
    ]
}

/// No setting and no input makes an effect put out anything but finite
/// numbers, including while its parameters are thrown around.
fn stays_finite<E: Effect + Default>(seed: u32) {
    let mut rng = Rng::new(seed);
    let name = E::Params::NAME;
    for round in 0..24 {
        let rate = RATES[round % RATES.len()];
        let params: E::Params = random_params(&mut rng);
        let mut effect: E = prepared(&params, rate);
        effect.set_tempo(20.0 + 400.0 * rng.unipolar());
        for (what, mut left, mut right) in hostile_inputs(3_000, seed + round as u32) {
            let block = [1, 7, 64, 512][round % 4];
            for (index, (left, right)) in left
                .chunks_mut(block)
                .zip(right.chunks_mut(block))
                .enumerate()
            {
                if rng.unipolar() < 0.02 || index == 3 {
                    effect.set_params(&random_params(&mut rng));
                }
                effect.process(left, right);
            }
            let context = format!("{name} round {round}, {what}, {params:?}");
            assert_finite(&left, &context);
            assert_finite(&right, &context);
            assert!(
                peak(&left).max(peak(&right)) < 1.0e9,
                "{context}: output exploded"
            );
            assert!(effect.tail_samples() >= effect.latency_samples());
        }
    }
}

#[test]
fn every_effect_stays_finite_for_any_settings_and_input() {
    stays_finite::<ParametricEq>(100);
    stays_finite::<Compressor>(200);
    stays_finite::<Limiter>(300);
    stays_finite::<Reverb>(400);
    stays_finite::<Delay>(500);
}

/// Sample rates far from the usual ones, and blocks of no length at all,
/// are handled like any other.
fn runs_at_any_rate<E: Effect + Default>(seed: u32) {
    let mut rng = Rng::new(seed);
    for rate in [8_000.0, 11_025.0, 22_050.0, 88_200.0, 176_400.0, 192_000.0] {
        for _ in 0..3 {
            let params: E::Params = random_params(&mut rng);
            let mut effect: E = prepared(&params, rate);
            effect.process(&mut [], &mut []);
            let (mut left, mut right) = (noise(seed, 1.0, 6_000), noise(seed + 1, 1.0, 6_000));
            run(&mut effect, &mut left, &mut right, 480);
            effect.set_params(&random_params(&mut rng));
            run(&mut effect, &mut left, &mut right, 31);
            let context = format!("{} at {rate}: {params:?}", E::Params::NAME);
            assert_finite(&left, &context);
            assert_finite(&right, &context);
            assert!(
                peak(&left).max(peak(&right)) < 1.0e9,
                "{context}: output exploded"
            );
        }
    }
}

#[test]
fn every_effect_runs_at_unusual_sample_rates() {
    runs_at_any_rate::<ParametricEq>(11);
    runs_at_any_rate::<Compressor>(22);
    runs_at_any_rate::<Limiter>(33);
    runs_at_any_rate::<Reverb>(44);
    runs_at_any_rate::<Delay>(55);
}

/// A lively stereo test signal.
fn programme(frames: usize, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let tone = sine(330.0, 0.4, frames, rate);
    let hiss = noise(7, 0.5, frames);
    let left = tone.iter().zip(&hiss).map(|(a, b)| a + b).collect();
    let right = tone
        .iter()
        .zip(noise(8, 0.5, frames))
        .map(|(a, b)| a - b)
        .collect();
    (left, right)
}

/// Processes the programme with `first` in effect, then switches to
/// `second` part of the way through, in blocks of `block`.
fn two_part_run<E: Effect + Default>(
    first: &E::Params,
    second: &E::Params,
    block: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = programme(9_000, RATE);
    let mut effect: E = prepared(first, RATE);
    let (left_a, left_b) = left.split_at_mut(4_000);
    let (right_a, right_b) = right.split_at_mut(4_000);
    run(&mut effect, left_a, right_a, block);
    effect.set_params(second);
    run(&mut effect, left_b, right_b, block);
    (left, right)
}

/// The output is the same, bit for bit, whatever block size the host uses,
/// including across a parameter change.
fn block_size_does_not_matter<E: Effect + Default>(first: &E::Params, second: &E::Params) {
    let reference = two_part_run::<E>(first, second, 9_000);
    assert!(
        rms(&reference.0) > 1.0e-4,
        "{}: nothing came out",
        E::Params::NAME
    );
    for block in [1, 7, 64, 512] {
        let other = two_part_run::<E>(first, second, block);
        assert!(
            other == reference,
            "{} differs with blocks of {block}",
            E::Params::NAME
        );
    }
}

/// `reset` leaves no trace of what came before: a used and reset effect
/// gives exactly the output of a new one.
fn reset_forgets_everything<E: Effect + Default>(other: &E::Params, params: &E::Params) {
    let (mut left, mut right) = programme(6_000, RATE);
    let mut fresh: E = prepared(params, RATE);
    run(&mut fresh, &mut left, &mut right, 128);

    let mut used: E = prepared(other, RATE);
    let (mut junk_left, mut junk_right) = (noise(1, 2.0, 5_000), noise(2, 2.0, 5_000));
    run(&mut used, &mut junk_left, &mut junk_right, 100);
    used.set_params(params);
    run(&mut used, &mut junk_left, &mut junk_right, 33);
    used.reset();
    let (mut again_left, mut again_right) = programme(6_000, RATE);
    run(&mut used, &mut again_left, &mut again_right, 128);
    assert!(
        again_left == left && again_right == right,
        "{}: reset left something behind",
        E::Params::NAME
    );

    // Two new instances agree with each other too, run after run.
    let mut twin: E = prepared(params, RATE);
    let (mut twin_left, mut twin_right) = programme(6_000, RATE);
    run(&mut twin, &mut twin_left, &mut twin_right, 128);
    assert!(twin_left == left && twin_right == right);
}

/// Changing settings under a steady tone never makes the output jump. The
/// bound: it never moves faster than a sine at three times the tone's
/// frequency and the output's own peak level would.
fn changes_do_not_click<E: Effect + Default>(first: &E::Params, second: &E::Params, level: f32) {
    let frequency = 220.0;
    for rate in RATES {
        let frames = (rate * 1.5) as usize;
        let mut left = sine(frequency, level, frames, rate);
        let mut right = left.clone();
        let mut effect: E = prepared(first, rate);
        // Switch back and forth several times, at awkward moments.
        let mut settings = [second, first].into_iter().cycle();
        for (index, (left, right)) in left.chunks_mut(96).zip(right.chunks_mut(96)).enumerate() {
            if index > 40 && index % 97 == 0 {
                effect.set_params(settings.next().unwrap());
            }
            effect.process(left, right);
        }
        for (side, output) in [("left", &left), ("right", &right)] {
            let settled = &output[(rate * 0.2) as usize..];
            let fastest = 3.0 * std::f32::consts::TAU * frequency / rate * peak(settled);
            let bound = fastest + 0.002;
            assert!(
                steepest(settled) <= bound,
                "{} at {rate}: {side} output jumps by {} (limit {bound})",
                E::Params::NAME,
                steepest(settled)
            );
        }
    }
}

/// Identical channels in give identical channels out, and swapping the
/// inputs swaps the outputs.
fn treats_both_channels_alike<E: Effect + Default>(params: &E::Params) {
    let (left, right) = programme(8_000, RATE);
    let mut effect: E = prepared(params, RATE);
    let (mut same_left, mut same_right) = (left.clone(), left.clone());
    run(&mut effect, &mut same_left, &mut same_right, 256);
    assert!(
        same_left == same_right,
        "{}: mono in, stereo out",
        E::Params::NAME
    );

    let mut straight: E = prepared(params, RATE);
    let (mut out_left, mut out_right) = (left.clone(), right.clone());
    run(&mut straight, &mut out_left, &mut out_right, 256);
    let mut swapped: E = prepared(params, RATE);
    let (mut swap_left, mut swap_right) = (right, left);
    run(&mut swapped, &mut swap_left, &mut swap_right, 256);
    assert!(
        swap_left == out_right && swap_right == out_left,
        "{}: channels are not treated alike",
        E::Params::NAME
    );
}

/// After the input stops the output dies away within the reported tail,
/// then reaches exact silence, and never passes through subnormal numbers
/// on the way.
fn tail_ends_in_exact_silence<E: Effect + Default>(params: &E::Params, silence_seconds: f32) {
    for rate in [44_100.0, 96_000.0] {
        let mut effect: E = prepared(params, rate);
        let (mut left, mut right) = (noise(3, 0.5, 20_000), noise(4, 0.5, 20_000));
        run(&mut effect, &mut left, &mut right, 256);
        let loud = rms(&left).max(rms(&right)).max(1.0e-3);

        let tail = effect.tail_samples();
        let (mut left, mut right) = (silence(tail), silence(tail));
        run(&mut effect, &mut left, &mut right, 256);
        assert!(!has_subnormals(&left) && !has_subnormals(&right));

        let (mut left, mut right) = (silence(4_096), silence(4_096));
        run(&mut effect, &mut left, &mut right, 256);
        let left_over = rms(&left).max(rms(&right));
        assert!(
            left_over < loud * 2.0e-3,
            "{} at {rate}: still at {left_over} after its tail of {tail} samples",
            E::Params::NAME
        );

        let frames = (silence_seconds * rate) as usize;
        let (mut left, mut right) = (silence(frames), silence(frames));
        run(&mut effect, &mut left, &mut right, 256);
        assert!(
            !has_subnormals(&left) && !has_subnormals(&right),
            "{} at {rate}: subnormal numbers in the tail",
            E::Params::NAME
        );
        let (mut left, mut right) = (silence(2_048), silence(2_048));
        run(&mut effect, &mut left, &mut right, 256);
        assert!(
            left.iter().chain(&right).all(|sample| *sample == 0.0),
            "{} at {rate}: the tail never reaches exact silence",
            E::Params::NAME
        );
    }
}

fn busy_eq() -> EqParams {
    let mut params = EqParams::default();
    params.low_cut.enabled = true;
    params.low_cut.frequency_hz = 80.0;
    params.low_cut.slope = CutSlope::Db24;
    params.low_shelf.gain_db = 4.0;
    params.peak1.gain_db = -9.0;
    params.peak1.q = 4.0;
    params.peak2.gain_db = 6.0;
    params.peak3.gain_db = 3.0;
    params.peak3.frequency_hz = 5_000.0;
    params.high_shelf.gain_db = -5.0;
    params.high_cut.enabled = true;
    params.high_cut.frequency_hz = 12_000.0;
    params.output_gain_db = -3.0;
    params
}

fn other_eq() -> EqParams {
    let mut params = EqParams::default();
    params.low_cut.enabled = true;
    params.low_cut.frequency_hz = 300.0;
    params.low_cut.slope = CutSlope::Db48;
    params.peak1.gain_db = 12.0;
    params.peak1.frequency_hz = 2_500.0;
    params.peak1.q = 9.0;
    params.peak2.enabled = false;
    params.high_shelf.gain_db = 9.0;
    params.high_shelf.frequency_hz = 3_000.0;
    params.output_gain_db = 2.0;
    params
}

fn busy_compressor() -> CompressorParams {
    CompressorParams {
        sidechain: false,
        threshold_db: -30.0,
        ratio: 6.0,
        attack_ms: 3.0,
        release_ms: 60.0,
        knee_db: 9.0,
        makeup_db: 4.0,
        auto_makeup: true,
        detector: DetectorMode::Rms,
        mix: 0.8,
    }
}

fn busy_limiter() -> LimiterParams {
    LimiterParams {
        ceiling_db: -9.0,
        input_gain_db: 9.0,
        release_ms: 40.0,
        lookahead_ms: 3.0,
        ..LimiterParams::default()
    }
}

fn other_limiter() -> LimiterParams {
    LimiterParams {
        ceiling_db: -3.0,
        input_gain_db: 0.0,
        release_ms: 300.0,
        lookahead_ms: 11.0,
        ..LimiterParams::default()
    }
}

fn busy_reverb() -> ReverbParams {
    ReverbParams {
        size: 0.8,
        decay_s: 0.6,
        pre_delay_ms: 25.0,
        damping: 0.7,
        diffusion: 0.6,
        early_level: 0.8,
        modulation: 0.6,
        low_cut_hz: 200.0,
        high_cut_hz: 6_000.0,
        width: 0.7,
        mix: 0.6,
    }
}

fn short_reverb() -> ReverbParams {
    ReverbParams {
        size: 0.2,
        decay_s: 0.25,
        mix: 1.0,
        ..ReverbParams::default()
    }
}

fn busy_delay() -> DelayParams {
    DelayParams {
        sync: false,
        time_ms: 23.0,
        division: NoteDivision::Sixteenth,
        feedback: 0.6,
        mode: DelayMode::PingPong,
        stereo_offset_ms: 4.0,
        low_cut_hz: 150.0,
        high_cut_hz: 5_000.0,
        saturation: 0.5,
        mix: 0.5,
        ..DelayParams::default()
    }
}

fn plain_delay() -> DelayParams {
    DelayParams {
        sync: false,
        time_ms: 31.0,
        feedback: 0.5,
        mix: 0.5,
        ..DelayParams::default()
    }
}

#[test]
fn block_size_never_changes_the_output() {
    block_size_does_not_matter::<ParametricEq>(&busy_eq(), &other_eq());
    block_size_does_not_matter::<Compressor>(&busy_compressor(), &CompressorParams::default());
    block_size_does_not_matter::<Limiter>(&busy_limiter(), &other_limiter());
    block_size_does_not_matter::<Reverb>(&busy_reverb(), &ReverbParams::default());
    block_size_does_not_matter::<Delay>(&busy_delay(), &plain_delay());
}

#[test]
fn reset_returns_to_the_initial_state() {
    reset_forgets_everything::<ParametricEq>(&other_eq(), &busy_eq());
    reset_forgets_everything::<Compressor>(&CompressorParams::default(), &busy_compressor());
    reset_forgets_everything::<Limiter>(&other_limiter(), &busy_limiter());
    reset_forgets_everything::<Reverb>(&ReverbParams::default(), &busy_reverb());
    reset_forgets_everything::<Delay>(&plain_delay(), &busy_delay());
}

#[test]
fn parameter_changes_never_click() {
    changes_do_not_click::<ParametricEq>(&busy_eq(), &other_eq(), 0.25);
    changes_do_not_click::<ParametricEq>(&EqParams::default(), &other_eq(), 0.25);
    changes_do_not_click::<Compressor>(&busy_compressor(), &CompressorParams::default(), 0.8);
    // Loud enough that the limiter is working the whole time.
    changes_do_not_click::<Limiter>(&busy_limiter(), &other_limiter(), 0.9);
    changes_do_not_click::<Reverb>(&busy_reverb(), &ReverbParams::default(), 0.5);
    changes_do_not_click::<Delay>(&busy_delay(), &plain_delay(), 0.5);
}

#[test]
fn both_channels_are_treated_alike() {
    treats_both_channels_alike::<ParametricEq>(&busy_eq());
    treats_both_channels_alike::<Compressor>(&busy_compressor());
    treats_both_channels_alike::<Limiter>(&busy_limiter());
    treats_both_channels_alike::<Delay>(&plain_delay());
}

#[test]
fn tails_end_in_exact_silence() {
    let mut ringing = EqParams::default();
    ringing.peak1.frequency_hz = 300.0;
    ringing.peak1.q = 18.0;
    ringing.peak1.gain_db = 12.0;
    ringing.low_cut.enabled = true;
    ringing.low_cut.q = 4.0;
    tail_ends_in_exact_silence::<ParametricEq>(&ringing, 3.0);
    tail_ends_in_exact_silence::<Compressor>(&busy_compressor(), 3.0);
    tail_ends_in_exact_silence::<Limiter>(&busy_limiter(), 1.0);
    tail_ends_in_exact_silence::<Reverb>(&short_reverb(), 2.5);
    tail_ends_in_exact_silence::<Delay>(&plain_delay(), 3.0);
}

#[test]
fn defaults_are_transparent_or_sane() {
    let (left, right) = programme(8_000, RATE);
    let ran = |effect: &mut dyn FnMut(&mut [f32], &mut [f32])| {
        let (mut out_left, mut out_right) = (left.clone(), right.clone());
        effect(&mut out_left, &mut out_right);
        (out_left, out_right)
    };

    // The equaliser's defaults change nothing at all.
    let mut eq: ParametricEq = prepared(&EqParams::default(), RATE);
    let (out_left, out_right) = ran(&mut |l, r| run(&mut eq, l, r, 256));
    assert!(out_left == left && out_right == right);

    // The limiter's defaults leave a signal under its ceiling alone, apart
    // from its latency.
    let mut limiter: Limiter = prepared(&LimiterParams::default(), RATE);
    let latency = limiter.latency_samples();
    assert_eq!(latency, 240);
    let quiet: Vec<f32> = left.iter().map(|sample| sample * 0.5).collect();
    let (mut out_left, mut out_right) = (quiet.clone(), quiet.clone());
    run(&mut limiter, &mut out_left, &mut out_right, 256);
    for (index, sample) in quiet[..quiet.len() - latency].iter().enumerate() {
        assert!((out_left[index + latency] - sample).abs() < 1.0e-6);
    }

    // The others change the level only moderately.
    let input_level = rms(&left);
    let mut compressor: Compressor = prepared(&CompressorParams::default(), RATE);
    let (out_left, _) = ran(&mut |l, r| run(&mut compressor, l, r, 256));
    let change = 20.0 * (rms(&out_left) / input_level).log10();
    assert!((-12.0..=0.0).contains(&change), "compressor: {change} dB");

    let mut reverb: Reverb = prepared(&ReverbParams::default(), RATE);
    let (out_left, _) = ran(&mut |l, r| run(&mut reverb, l, r, 256));
    let change = 20.0 * (rms(&out_left) / input_level).log10();
    assert!((-6.0..=3.0).contains(&change), "reverb: {change} dB");

    let mut delay: Delay = prepared(&DelayParams::default(), RATE);
    let (out_left, _) = ran(&mut |l, r| run(&mut delay, l, r, 256));
    let change = 20.0 * (rms(&out_left) / input_level).log10();
    assert!((-6.0..=3.0).contains(&change), "delay: {change} dB");
}

#[test]
fn only_the_limiter_adds_latency() {
    let spike = impulse(2_000, 100, 0.5);
    for rate in RATES {
        let mut eq: ParametricEq = prepared(&busy_eq(), rate);
        assert_eq!(eq.latency_samples(), 0);
        let mut compressor: Compressor = prepared(&CompressorParams::default(), rate);
        assert_eq!(compressor.latency_samples(), 0);
        let mut reverb: Reverb = prepared(&busy_reverb(), rate);
        assert_eq!(reverb.latency_samples(), 0);
        let mut delay: Delay = prepared(&busy_delay(), rate);
        assert_eq!(delay.latency_samples(), 0);

        // With no latency, the first thing out lines up with the first
        // thing in.
        let first_sound = |output: &[f32]| output.iter().position(|sample| sample.abs() > 1.0e-4);
        let (mut left, mut right) = (spike.clone(), spike.clone());
        run(&mut eq, &mut left, &mut right, 64);
        assert_eq!(first_sound(&left), Some(100));
        let (mut left, mut right) = (spike.clone(), spike.clone());
        run(&mut compressor, &mut left, &mut right, 64);
        assert_eq!(first_sound(&left), Some(100));
        let (mut left, mut right) = (spike.clone(), spike.clone());
        run(&mut reverb, &mut left, &mut right, 64);
        assert_eq!(first_sound(&left), Some(100));
        let (mut left, mut right) = (spike.clone(), spike.clone());
        run(&mut delay, &mut left, &mut right, 64);
        assert_eq!(first_sound(&left), Some(100));
    }
}
