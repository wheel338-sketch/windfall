//! The limiter: the ceiling holds for anything, the latency is exact, and
//! steady material is left clean.

use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::{Effect, Limiter, LimiterParams, ParamSet};

use crate::support::{
    RATE, RATES, db, impulse, noise, peak, prepared, random_params, rms, run, run_mono, sine,
    steepest, tone_level,
};

fn ceiling_gain(params: &LimiterParams) -> f32 {
    10.0_f32.powf(params.sanitized().ceiling_db / 20.0)
}

/// A signal built to catch a limiter out. `kind` picks the trick.
fn adversarial(kind: u32, frames: usize, rng: &mut Rng) -> Vec<f32> {
    (0..frames)
        .map(|n| match kind % 8 {
            // Full-scale steps.
            0 => {
                if (n / 113) % 2 == 0 {
                    4.0
                } else {
                    -4.0
                }
            }
            // Lone spikes far over the ceiling, out of near silence.
            1 => {
                if n % 499 == 250 {
                    300.0
                } else {
                    1.0e-3
                }
            }
            // Dense noise at +24 dB.
            2 => rng.bipolar() * 15.85,
            // Bursts that start while the gain is still recovering.
            3 => (n as f32 * 1.3).sin() * if (n / 700) % 3 == 0 { 10.0 } else { 0.3 },
            // A swell that keeps growing.
            4 => (n as f32 * 0.01).sin() * n as f32 * 0.002,
            // Rare, random, enormous samples.
            5 => {
                if rng.unipolar() < 0.004 {
                    rng.bipolar() * 1.0e3
                } else {
                    rng.bipolar() * 0.7
                }
            }
            // A spike exactly one sample after another.
            6 => match n % 1_000 {
                500 => 20.0,
                501 => -50.0,
                _ => 0.0,
            },
            // A constant over the ceiling.
            _ => 3.0,
        })
        .collect()
}

#[test]
fn no_output_sample_ever_exceeds_the_ceiling() {
    let mut rng = Rng::new(77);
    for round in 0..60_u32 {
        let rate = RATES[round as usize % RATES.len()];
        let mut params: LimiterParams = random_params(&mut rng);
        let mut limiter: Limiter = prepared(&params, rate);
        let block = [1, 5, 64, 300][round as usize % 4];
        for part in 0..4 {
            let mut left = adversarial(round + part, 3_000, &mut rng);
            let mut right = adversarial(round + part + 3, 3_000, &mut rng);
            for (left, right) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
                // Every so often the settings change mid-stream. The
                // ceiling in force is always the one set last.
                if rng.unipolar() < 0.02 {
                    params = random_params(&mut rng);
                    limiter.set_params(&params);
                }
                limiter.process(left, right);
                let ceiling = ceiling_gain(&params);
                let loudest = peak(left).max(peak(right));
                assert!(
                    loudest <= ceiling,
                    "round {round}: {loudest} over a ceiling of {ceiling} with {params:?}"
                );
            }
        }
    }
}

#[test]
fn an_impulse_arrives_exactly_the_latency_late() {
    for rate in RATES {
        for lookahead_ms in [0.1, 1.0, 5.0, 12.5, 20.0] {
            let params = LimiterParams {
                lookahead_ms,
                ceiling_db: 0.0,
                ..LimiterParams::default()
            };
            let mut limiter: Limiter = prepared(&params, rate);
            let latency = limiter.latency_samples();
            let expected = ((lookahead_ms * rate / 1_000.0).round() as usize).max(1);
            assert_eq!(latency, expected, "{lookahead_ms} ms at {rate}");
            assert_eq!(limiter.tail_samples(), latency);

            // Under the ceiling, so the limiter is only a delay.
            let output = run_mono(&mut limiter, &impulse(4_000, 37, 0.5));
            let position = output.iter().position(|sample| *sample != 0.0);
            assert_eq!(position, Some(37 + latency));
            assert!((output[37 + latency] - 0.5).abs() < 1.0e-6);
            assert_eq!(output.iter().filter(|sample| **sample != 0.0).count(), 1);
        }
    }
    assert_eq!(Limiter::max_latency_samples(48_000.0), 960);
}

#[test]
fn a_peak_over_the_ceiling_arrives_on_time_and_at_the_ceiling() {
    for rate in RATES {
        let params = LimiterParams {
            ceiling_db: -6.0,
            ..LimiterParams::default()
        };
        let mut limiter: Limiter = prepared(&params, rate);
        let latency = limiter.latency_samples();
        let output = run_mono(&mut limiter, &impulse(8_000, 1_000, 4.0));
        let loudest = output
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap();
        assert_eq!(loudest.0, 1_000 + latency);
        let ceiling = ceiling_gain(&params);
        assert!(
            *loudest.1 <= ceiling && *loudest.1 > ceiling * 0.999,
            "{}",
            loudest.1
        );
    }
}

#[test]
fn signals_under_the_ceiling_are_only_delayed() {
    let params = LimiterParams::default();
    let mut limiter: Limiter = prepared(&params, RATE);
    let latency = limiter.latency_samples();
    let input = noise(9, 0.45, 20_000);
    let output = run_mono(&mut limiter, &input);
    for index in 0..input.len() - latency {
        assert!((output[index + latency] - input[index]).abs() < 1.0e-6);
    }
    assert_eq!(limiter.meter().take_db(), 0.0);
}

#[test]
fn input_gain_drives_the_signal_into_the_ceiling() {
    // Quiet material just gets louder.
    let params = LimiterParams {
        input_gain_db: 12.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let input = sine(440.0, 0.05, 24_000, RATE);
    let output = run_mono(&mut limiter, &input);
    assert!((db(rms(&output[4_000..]) / rms(&input[4_000..])) - 12.0).abs() < 0.01);

    // Loud material stops at the ceiling.
    let mut limiter: Limiter = prepared(&params, RATE);
    let input = sine(440.0, 0.9, 24_000, RATE);
    let output = run_mono(&mut limiter, &input);
    let ceiling = ceiling_gain(&params);
    let loudest = peak(&output[4_000..]);
    assert!(loudest <= ceiling && loudest > ceiling * 0.99, "{loudest}");
}

#[test]
fn a_steady_tone_over_the_ceiling_is_turned_down_cleanly() {
    // 12 dB over the ceiling. The gain must sit still, not wobble between
    // one peak and the next, or the tone would come out distorted.
    for (frequency, worst_harmonic_db) in [(1_000.0, -80.0), (250.0, -80.0), (60.0, -40.0)] {
        let params = LimiterParams {
            ceiling_db: -12.0,
            release_ms: 100.0,
            ..LimiterParams::default()
        };
        let mut limiter: Limiter = prepared(&params, RATE);
        let input = sine(frequency, 1.0, 96_000, RATE);
        let output = run_mono(&mut limiter, &input);
        let steady = &output[32_768..32_768 + 32_768];
        let ceiling = ceiling_gain(&params);
        let fundamental = tone_level(steady, f64::from(frequency), RATE);
        assert!(
            fundamental > f64::from(ceiling) * 0.9 && fundamental <= f64::from(ceiling) * 1.001,
            "{frequency} Hz comes out at {fundamental}"
        );
        for harmonic in 2..=5 {
            let level = tone_level(steady, f64::from(frequency) * f64::from(harmonic), RATE);
            let relative = db(level / fundamental);
            assert!(
                relative < worst_harmonic_db,
                "{frequency} Hz: harmonic {harmonic} at {relative} dB"
            );
        }
    }
}

#[test]
fn steady_noise_does_not_pump() {
    // Dense noise pushed 9 dB into the ceiling. The short-term level of
    // the output should stay put instead of breathing.
    let params = LimiterParams {
        ceiling_db: -1.0,
        input_gain_db: 9.0,
        release_ms: 100.0,
        lookahead_ms: 5.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let output = run_mono(&mut limiter, &noise(31, 0.7, 144_000));
    let levels: Vec<f64> = output[48_000..]
        .chunks(4_800)
        .map(|window| db(rms(window)))
        .collect();
    let highest = levels.iter().cloned().fold(f64::MIN, f64::max);
    let lowest = levels.iter().cloned().fold(f64::MAX, f64::min);
    assert!(
        highest - lowest < 1.5,
        "level moves by {} dB",
        highest - lowest
    );
}

#[test]
fn gain_recovers_at_the_release_rate_after_a_peak() {
    for release_ms in [20.0_f32, 100.0, 400.0] {
        let params = LimiterParams {
            ceiling_db: 0.0,
            release_ms,
            lookahead_ms: 2.0,
            ..LimiterParams::default()
        };
        let mut limiter: Limiter = prepared(&params, RATE);
        let latency = limiter.latency_samples();
        // A quiet steady signal with one sample 20 dB over the ceiling.
        let mut input = vec![0.25_f32; 96_000 * 2];
        input[10_000] = 10.0;
        let output = run_mono(&mut limiter, &input);
        let gain = |index: usize| f64::from(output[index + latency] / 0.25);
        // At the peak the gain is the tenth that the peak needed.
        assert!((f64::from(output[10_000 + latency]) - 1.0).abs() < 1.0e-3);
        // Well before the peak comes into view it is unity.
        assert!((gain(9_000) - 1.0).abs() < 1.0e-6);
        // Afterwards it climbs back: 63% of the way after the hold (one
        // look-ahead) plus one release time, give or take the smoothing.
        let after = 10_000 + latency + (release_ms * 48.0) as usize;
        let recovered = (gain(after) - 0.1) / 0.9;
        assert!(
            (0.5..=0.75).contains(&recovered),
            "release {release_ms} ms: {recovered} of the way back"
        );
        // Almost four seconds on it is all the way back, whatever the
        // release.
        assert!((gain(190_000) - 1.0).abs() < 1.0e-3);
    }
}

#[test]
fn gain_moves_smoothly_around_a_peak() {
    // A slow sine with one huge spike on top. The limiter has to duck for
    // the spike; the sine around it shows how smoothly it does so.
    let params = LimiterParams {
        ceiling_db: 0.0,
        lookahead_ms: 5.0,
        release_ms: 50.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let mut input = sine(100.0, 0.5, 48_000, RATE);
    input[20_000] += 30.0;
    let output = run_mono(&mut limiter, &input);
    let latency = limiter.latency_samples();
    let spike = 20_000 + latency;
    // Around the spike but not at it, the output never jumps: the gain
    // glides down over the look-ahead instead of stepping.
    let natural = 0.5 * std::f32::consts::TAU * 100.0 / RATE;
    assert!(steepest(&output[spike - 2_000..spike - 1]) < 3.0 * natural);
    assert!(steepest(&output[spike + 2..spike + 4_000]) < 3.0 * natural);
    // And it started to come down before the spike arrived, not after.
    let before = output[spike - 60].abs() / input[20_000 - 60].abs().max(1.0e-6);
    assert!(before < 0.5, "gain {before} shortly before the spike");
}

#[test]
fn the_meter_reports_the_deepest_reduction() {
    let params = LimiterParams {
        ceiling_db: -6.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let meter = limiter.meter();
    run_mono(&mut limiter, &sine(500.0, 0.25, 4_800, RATE));
    assert_eq!(meter.take_db(), 0.0);
    // Full scale against a ceiling of -6 dB needs 6 dB of reduction.
    run_mono(&mut limiter, &sine(500.0, 1.0, 48_000, RATE));
    assert!((meter.take_db() - 6.02).abs() < 0.05);
}

#[test]
fn changing_the_look_ahead_changes_the_latency_without_a_click() {
    let short = LimiterParams {
        lookahead_ms: 2.0,
        ..LimiterParams::default()
    };
    let long = LimiterParams {
        lookahead_ms: 15.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&short, RATE);
    assert_eq!(limiter.latency_samples(), 96);
    let mut left = sine(150.0, 0.4, 48_000, RATE);
    let mut right = left.clone();
    for (index, (left, right)) in left.chunks_mut(480).zip(right.chunks_mut(480)).enumerate() {
        if index % 20 == 10 {
            limiter.set_params(if index % 40 == 10 { &long } else { &short });
        }
        limiter.process(left, right);
    }
    // The last change in the loop went to the long setting.
    assert_eq!(limiter.latency_samples(), 720);
    limiter.set_params(&short);
    assert_eq!(limiter.latency_samples(), 96);
    let natural = 0.4 * std::f32::consts::TAU * 150.0 / RATE;
    assert!(
        steepest(&left[1_000..]) < 3.0 * natural,
        "{}",
        steepest(&left[1_000..])
    );

    let mut tail = (vec![0.0_f32; 256], vec![0.0_f32; 256]);
    run(&mut limiter, &mut tail.0, &mut tail.1, 64);
}
