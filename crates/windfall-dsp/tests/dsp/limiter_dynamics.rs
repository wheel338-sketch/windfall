//! The limiter's compress mode, its gate, and what it reports per block.
//!
//! The expected levels here are worked out from the settings by hand, in
//! this file, not by calling the limiter's own curve.

use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::{
    AnyEffect, Effect, EffectObservation, EffectParams, GATE_FLOOR_DB, GATE_OFF_DB, Limiter,
    LimiterMode, LimiterParams, ParamSet,
};

use crate::support::{RATE, db, noise, peak, prepared, random_params, run_mono, sine, square};

/// A compressor that reacts at once and barely lets go, so a steady input
/// sits on the static curve.
fn firm(threshold_db: f32, ratio: f32, knee_db: f32) -> LimiterParams {
    LimiterParams {
        ceiling_db: 0.0,
        mode: LimiterMode::Compress,
        threshold_db,
        ratio,
        knee_db,
        attack_ms: 0.05,
        release_ms: 1000.0,
        lookahead_ms: 1.0,
        ..LimiterParams::default()
    }
}

fn level_db(level: f32) -> f64 {
    db(f64::from(level))
}

fn gain(level_db: f64) -> f32 {
    10.0_f64.powf(level_db / 20.0) as f32
}

#[test]
fn the_new_settings_are_appended_so_stored_automation_keeps_its_targets() {
    let ids: Vec<&str> = LimiterParams::descriptors()
        .iter()
        .map(|info| info.id)
        .collect();
    assert_eq!(
        ids,
        [
            "ceilingDb",
            "inputGainDb",
            "releaseMs",
            "lookaheadMs",
            "mode",
            "thresholdDb",
            "ratio",
            "kneeDb",
            "attackMs",
            "gateThresholdDb",
            "gateReleaseMs",
        ]
    );
    // A project saved before these settings existed loads as a plain limiter.
    let legacy: LimiterParams = serde_json::from_str(
        r#"{"ceilingDb":-3.0,"inputGainDb":6.0,"releaseMs":50.0,"lookaheadMs":2.0}"#,
    )
    .unwrap();
    assert_eq!(legacy.mode, LimiterMode::Limit);
    assert_eq!(legacy.gate_threshold_db, GATE_OFF_DB);
    assert!(!legacy.gated());
    assert_eq!(legacy.ceiling_db, -3.0);
    let saved = serde_json::to_string(&firm(-20.0, 4.0, 0.0)).unwrap();
    assert!(saved.contains(r#""mode":"compress""#), "{saved}");
    assert_eq!(
        serde_json::from_str::<LimiterParams>(&saved).unwrap(),
        firm(-20.0, 4.0, 0.0)
    );
}

#[test]
fn limit_mode_without_a_gate_ignores_the_compress_and_gate_settings() {
    // The two new stages multiply by exactly one, so no bit of the plain
    // limiter's output depends on their settings.
    let plain = LimiterParams {
        ceiling_db: -3.0,
        input_gain_db: 9.0,
        ..LimiterParams::default()
    };
    let other = LimiterParams {
        threshold_db: -50.0,
        ratio: 100.0,
        knee_db: 0.0,
        attack_ms: 0.05,
        gate_release_ms: 5.0,
        ..plain
    };
    let input = noise(5, 0.9, 24_000);
    let mut a: Limiter = prepared(&plain, RATE);
    let mut b: Limiter = prepared(&other, RATE);
    let (out_a, out_b) = (run_mono(&mut a, &input), run_mono(&mut b, &input));
    assert!(
        out_a
            .iter()
            .zip(&out_b)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert_eq!(other.static_gain_db(0.0), 0.0);
}

#[test]
fn compress_mode_puts_steady_levels_on_the_ratio_line() {
    // A square wave has one absolute level, so the peak detector reads it
    // exactly and the output can be compared with the textbook line:
    // threshold + (input - threshold) / ratio.
    for (threshold, ratio) in [(-20.0_f64, 2.0_f64), (-20.0, 4.0), (-30.0, 10.0)] {
        for input_db in [-40.0_f64, -24.0, -12.0, -6.0] {
            let params = firm(threshold as f32, ratio as f32, 0.0);
            let mut limiter: Limiter = prepared(&params, RATE);
            let input = square(96, gain(input_db), 48_000);
            let output = run_mono(&mut limiter, &input);
            let measured = level_db(peak(&output[24_000..]));
            let expected = if input_db > threshold {
                threshold + (input_db - threshold) / ratio
            } else {
                input_db
            };
            assert!(
                (measured - expected).abs() < 0.05,
                "threshold {threshold}, ratio {ratio}, input {input_db}: {measured} vs {expected}"
            );
            assert!(
                (f64::from(params.static_gain_db(input_db as f32)) - (expected - input_db)).abs()
                    < 1.0e-4
            );
        }
    }
}

#[test]
fn the_top_ratio_holds_the_level_at_the_threshold_and_a_knee_eases_in() {
    let hard = firm(-18.0, 100.0, 0.0);
    let mut limiter: Limiter = prepared(&hard, RATE);
    let output = run_mono(&mut limiter, &square(96, gain(-3.0), 48_000));
    assert!((level_db(peak(&output[24_000..])) + 18.0).abs() < 0.05);

    // At the threshold a 12 dB knee has built up a quarter of the slope
    // over half its width: slope * 6 * 6 / (2 * 12) = 1.5 dB * 0.75.
    let soft = firm(-18.0, 4.0, 12.0);
    let mut limiter: Limiter = prepared(&soft, RATE);
    let output = run_mono(&mut limiter, &square(96, gain(-18.0), 48_000));
    let reduction = -18.0 - level_db(peak(&output[24_000..]));
    assert!((reduction - 1.125).abs() < 0.05, "{reduction}");
}

#[test]
fn input_gain_moves_the_signal_against_the_compress_threshold() {
    // The threshold is measured after the input gain: -30 dB pushed up by
    // 18 dB is 8 dB over a -20 dB threshold, and comes out 2 dB over it.
    let params = LimiterParams {
        input_gain_db: 18.0,
        ..firm(-20.0, 4.0, 0.0)
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let output = run_mono(&mut limiter, &square(96, gain(-30.0), 48_000));
    assert!((level_db(peak(&output[24_000..])) + 18.0).abs() < 0.05);
}

#[test]
fn compression_arrives_at_the_attack_time_and_leaves_at_the_release_time() {
    let params = LimiterParams {
        attack_ms: 20.0,
        release_ms: 200.0,
        ..firm(-20.0, 100.0, 0.0)
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let latency = limiter.latency_samples();
    // Quiet, then 20 dB over the threshold, then quiet again.
    let (quiet, loud) = (gain(-40.0), gain(0.0));
    let mut input = square(96, quiet, 144_000);
    for (index, sample) in input.iter_mut().enumerate().take(96_000).skip(24_000) {
        *sample = if (index / 48) % 2 == 0 { loud } else { -loud };
    }
    let output = run_mono(&mut limiter, &input);
    let reduction_at =
        |index: usize, level: f32| level_db(level) - level_db(output[index + latency].abs());
    // One attack time in, 63% of the 20 dB has arrived.
    let attack = reduction_at(24_000 + 960, loud) / 20.0;
    assert!((0.58..=0.68).contains(&attack), "attack reached {attack}");
    // Fully there well before the loud part ends.
    assert!((reduction_at(90_000, loud) - 20.0).abs() < 0.1);
    // One release time after it ends, 63% has gone again.
    let release = 1.0 - reduction_at(96_000 + 9_600, quiet) / 20.0;
    assert!(
        (0.58..=0.68).contains(&release),
        "release reached {release}"
    );
}

#[test]
fn the_gate_silences_what_is_under_its_threshold_and_passes_what_is_over() {
    let gated = LimiterParams {
        ceiling_db: 0.0,
        gate_threshold_db: -40.0,
        gate_release_ms: 20.0,
        lookahead_ms: 1.0,
        ..LimiterParams::default()
    };
    assert!(gated.gated());
    let open = LimiterParams {
        gate_threshold_db: GATE_OFF_DB,
        ..gated
    };

    // Over the threshold: the same as with no gate, once it has opened.
    let loud = sine(440.0, gain(-20.0), 48_000, RATE);
    let mut limiter: Limiter = prepared(&gated, RATE);
    let mut plain: Limiter = prepared(&open, RATE);
    let (with, without) = (run_mono(&mut limiter, &loud), run_mono(&mut plain, &loud));
    for (with, without) in with[9_600..].iter().zip(&without[9_600..]) {
        assert!((with - without).abs() < 1.0e-6, "{with} vs {without}");
    }

    // Under it: gone, where the ungated limiter passes it.
    let faint = sine(440.0, gain(-50.0), 48_000, RATE);
    let mut limiter: Limiter = prepared(&gated, RATE);
    let mut plain: Limiter = prepared(&open, RATE);
    let (with, without) = (run_mono(&mut limiter, &faint), run_mono(&mut plain, &faint));
    assert!(level_db(peak(&without[24_000..])) > -50.5);
    assert!(
        level_db(peak(&with[24_000..])) < -120.0,
        "{}",
        level_db(peak(&with[24_000..]))
    );
}

#[test]
fn the_gate_stays_open_through_the_zero_crossings_of_a_low_note() {
    // 30 Hz is under the threshold for most of each cycle if the gate went
    // by single samples. It goes by the held peak, so the note is whole.
    let gated = LimiterParams {
        ceiling_db: 0.0,
        gate_threshold_db: -30.0,
        gate_release_ms: 5.0,
        lookahead_ms: 1.0,
        ..LimiterParams::default()
    };
    let open = LimiterParams {
        gate_threshold_db: GATE_OFF_DB,
        ..gated
    };
    let low = sine(30.0, gain(-20.0), 96_000, RATE);
    let mut limiter: Limiter = prepared(&gated, RATE);
    let mut plain: Limiter = prepared(&open, RATE);
    let (with, without) = (run_mono(&mut limiter, &low), run_mono(&mut plain, &low));
    for (with, without) in with[9_600..].iter().zip(&without[9_600..]) {
        assert!((with - without).abs() < 1.0e-5, "{with} vs {without}");
    }
}

#[test]
fn the_gate_shuts_at_its_release_time() {
    let release_ms = 100.0_f32;
    let params = LimiterParams {
        ceiling_db: 0.0,
        gate_threshold_db: -30.0,
        gate_release_ms: release_ms,
        lookahead_ms: 1.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    let latency = limiter.latency_samples();
    // Loud for half a second, then a steady level under the threshold. The
    // gate's reading takes 25 ms * ln(level ratio) to fall to the
    // threshold, and one release later the gate is 63% shut.
    let (loud, faint) = (gain(-10.0), gain(-50.0));
    let mut input = square(96, loud, 24_000);
    input.extend(square(96, faint, 48_000));
    let output = run_mono(&mut limiter, &input);
    let detector = 0.025 * f64::from(loud / gain(-30.0)).ln() * f64::from(RATE);
    let at = 24_000 + detector as usize + (release_ms * 48.0) as usize;
    let opening = f64::from(output[at + latency].abs() / faint);
    assert!((0.30..=0.44).contains(&opening), "{opening}");
    // Just before the loud part ended the gate was wide open.
    assert!((f64::from(output[23_000 + latency].abs() / loud) - 1.0).abs() < 1.0e-4);
    // And long after, it is shut.
    assert!(output[70_000].abs() < faint * 1.0e-3);
}

#[test]
fn the_ceiling_holds_whatever_the_mode_and_the_gate_do() {
    let mut rng = Rng::new(404);
    for round in 0..40 {
        let mut params: LimiterParams = random_params(&mut rng);
        params.mode = if round % 2 == 0 {
            LimiterMode::Compress
        } else {
            LimiterMode::Limit
        };
        params.gate_threshold_db = [-90.0, -60.0, -20.0, 0.0][round % 4];
        let ceiling = 10.0_f32.powf(params.sanitized().ceiling_db / 20.0);
        let mut limiter: Limiter = prepared(&params, RATE);
        let output = run_mono(&mut limiter, &noise(round as u32, 20.0, 12_000));
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            peak(&output) <= ceiling * 1.000_01,
            "round {round}: {} over {ceiling}",
            peak(&output)
        );
    }
}

#[test]
fn each_block_reports_its_real_levels_without_taking_the_meter_reading() {
    let params = LimiterParams {
        ceiling_db: -6.0,
        input_gain_db: 6.0,
        lookahead_ms: 1.0,
        ..LimiterParams::default()
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    assert_eq!(limiter.observation(), None);
    let meter = limiter.meter();

    let mut deepest = 0.0_f32;
    let mut serial = 0;
    for block in 0..40 {
        let mut left = sine(997.0, 0.9, 256, RATE);
        let mut right = sine(997.0, 0.45, 256, RATE);
        let loudest = peak(&left);
        limiter.process(&mut left, &mut right);
        let seen = limiter.observation().expect("a processed block");
        // Reading twice gives the same thing: nothing is consumed.
        assert_eq!(limiter.observation(), Some(seen));
        assert_eq!(seen.serial, serial + 1);
        serial = seen.serial;
        assert_eq!(seen.frames, 256);
        // The input peak is the louder channel after 6 dB of input gain.
        let expected = f64::from(loudest) * 10.0_f64.powf(6.0 / 20.0);
        assert!(
            (f64::from(seen.input_peak) - expected).abs() < 1.0e-4,
            "block {block}: {} vs {expected}",
            seen.input_peak
        );
        assert_eq!(seen.output_peak, peak(&left).max(peak(&right)));
        assert!(seen.output_peak <= 10.0_f32.powf(-6.0 / 20.0) * 1.000_01);
        assert_eq!(seen.gate_db, 0.0);
        deepest = deepest.max(seen.reduction_db);
    }
    // 0.9 driven 6 dB up is 1.8, against a ceiling of 0.5: about 11 dB.
    assert!((deepest - 11.1).abs() < 0.3, "{deepest}");
    // The meter still holds the deepest reading of all those blocks.
    assert!((meter.take_db() - deepest).abs() < 1.0e-5);
    assert_eq!(meter.take_db(), 0.0);

    limiter.reset();
    assert_eq!(limiter.observation(), None);
}

#[test]
fn a_shut_gate_and_compression_are_reported_apart() {
    let params = LimiterParams {
        gate_threshold_db: -30.0,
        gate_release_ms: 5.0,
        ..firm(-20.0, 4.0, 0.0)
    };
    let mut limiter: Limiter = prepared(&params, RATE);
    // Loud: compressed, gate open.
    let mut seen = None;
    for _ in 0..40 {
        let mut left = square(96, gain(-8.0), 256);
        let mut right = left.clone();
        limiter.process(&mut left, &mut right);
        seen = limiter.observation();
    }
    let loud = seen.unwrap();
    assert!(loud.gate_db < 0.01, "{}", loud.gate_db);
    assert!(
        (loud.reduction_db - 9.0).abs() < 0.1,
        "{}",
        loud.reduction_db
    );
    // Faint: gate shut. The compression lets go at the one-second release,
    // so 9 dB has fallen to 9 * e^-8, a few thousandths, after 8 seconds.
    for _ in 0..1_500 {
        let mut left = square(96, gain(-60.0), 256);
        let mut right = left.clone();
        limiter.process(&mut left, &mut right);
        seen = limiter.observation();
    }
    let faint = seen.unwrap();
    assert_eq!(faint.gate_db, GATE_FLOOR_DB);
    assert!(faint.reduction_db < 0.05, "{}", faint.reduction_db);
    assert_eq!(faint.output_peak, 0.0);
}

#[test]
fn only_measuring_effects_report_and_the_tuner_reports_the_calibrated_note() {
    let mut eq = AnyEffect::new(&EffectParams::Eq(Default::default()));
    eq.prepare(RATE, 256);
    let (mut left, mut right) = (vec![0.1; 256], vec![0.1; 256]);
    eq.process(&mut left, &mut right);
    assert_eq!(eq.observation(), None);

    // A4 at 440 Hz against a 432 Hz reference is 1200 * log2(440 / 432)
    // cents sharp of A4.
    let params = windfall_dsp::tuner::TunerParams {
        reference_hz: 432.0,
    };
    let mut tuner = AnyEffect::new(&EffectParams::Tuner(params));
    tuner.prepare(RATE, 256);
    tuner.set_params(&EffectParams::Tuner(params));
    let tone = sine(440.0, 0.5, 48_000, RATE);
    let mut last = 0;
    for chunk in tone.chunks(256) {
        let (mut left, mut right) = (chunk.to_vec(), chunk.to_vec());
        tuner.process(&mut left, &mut right);
        // Listening changes no sample.
        assert_eq!(left, chunk);
        let Some(EffectObservation::Tuner { serial, .. }) = tuner.observation() else {
            panic!("the tuner always reports");
        };
        assert_eq!(serial, last + 1);
        last = serial;
    }
    let Some(EffectObservation::Tuner { readout, .. }) = tuner.observation() else {
        unreachable!()
    };
    let expected = 1200.0 * (440.0_f64 / 432.0).log2();
    assert_eq!(readout.midi_note, Some(69));
    assert!((f64::from(readout.frequency_hz) - 440.0).abs() < 0.3);
    assert!(
        (f64::from(readout.cents_error) - expected).abs() < 1.5,
        "{} vs {expected}",
        readout.cents_error
    );
    assert!(readout.confidence > 0.9);
}
