//! Dynamics, transient and mastering processors, measured through the
//! crate's public API against figures worked out by hand.

use std::f64::consts::TAU;

use windfall_dsp::blocks::math::db_to_gain;
use windfall_dsp::mastering::{
    LoudnessMeter, MASTERING_PEAK_TOLERANCE_DB, MASTERING_TOLERANCE_LU, MasterLimiter,
    MasterLimiterParams, MasteringError, MasteringSettings, master, master_limiter_latency,
    measure,
};
use windfall_dsp::multiband::{
    BandDynamicsParams, MultibandCompressorParams, OneKnob, OneKnobParams, TransientListen,
    TransientShaper, TransientShaperParams,
};
use windfall_dsp::{AnyEffect, Effect, EffectKind, EffectParams};

const RATE: u32 = 48_000;

fn sine(hz: f64, amplitude: f64, phase: f64, frames: usize, rate: u32) -> Vec<f32> {
    (0..frames)
        .map(|n| (amplitude * (TAU * hz * n as f64 / f64::from(rate) + phase).sin()) as f32)
        .collect()
}

/// The same signal on both channels, interleaved.
fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|sample| [*sample, *sample]).collect()
}

/// Amplitude of the component at `hz`, by projection onto a sine and cosine.
fn amplitude_at(samples: &[f32], hz: f64, rate: u32) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, sample) in samples.iter().enumerate() {
        let phase = TAU * hz * n as f64 / f64::from(rate);
        re += f64::from(*sample) * phase.cos();
        im += f64::from(*sample) * phase.sin();
    }
    2.0 * re.hypot(im) / samples.len() as f64
}

/// Eases the first and last 10 ms in and out, so that the cut ends of a
/// tone do not ring between the samples and read as a peak of their own.
fn faded(mut samples: Vec<f32>) -> Vec<f32> {
    let length = samples.len();
    for index in 0..480.min(length / 2) {
        let gain = index as f32 / 480.0;
        samples[index] *= gain;
        samples[length - 1 - index] *= gain;
    }
    samples
}

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// Three tones an octave band or more apart whose level swells twice a
/// second: something with a low, a middle and a top, and loud and quiet
/// passages. `level` is the amplitude of each tone at the top of a swell.
fn programme(level: f64, seconds: usize) -> Vec<f32> {
    let frames = seconds * RATE as usize;
    (0..frames)
        .map(|n| {
            let t = n as f64 / f64::from(RATE);
            let swell = 0.6 + 0.4 * (TAU * 2.0 * t).sin();
            let tones =
                (TAU * 100.0 * t).sin() + (TAU * 1_000.0 * t).sin() + (TAU * 6_000.0 * t).sin();
            (level * swell * tones) as f32
        })
        .collect()
}

fn run<E: Effect>(effect: &mut E, signal: &[f32], block: usize) -> Vec<f32> {
    let mut left = signal.to_vec();
    let mut right = signal.to_vec();
    for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        effect.process(l, r);
    }
    left
}

fn master_limiter(params: MasterLimiterParams) -> MasterLimiter {
    let mut limiter = MasterLimiter::default();
    limiter.prepare(RATE as f32, 512);
    limiter.set_params(&params);
    limiter
}

#[test]
fn calibration_tones_read_their_level_in_lufs_at_two_sample_rates() {
    // EBU Tech 3341, cases 1 and 2: a stereo 1 kHz sine at -23 dBFS reads
    // -23.0 LUFS, and at -33 dBFS reads -33.0 LUFS, within 0.1 LU.
    for rate in [48_000_u32, 44_100] {
        for level in [-23.0_f32, -33.0] {
            let amplitude = f64::from(db_to_gain(level));
            let tone = stereo(&sine(1_000.0, amplitude, 0.0, 20 * rate as usize, rate));
            let metrics = measure(&tone, rate);
            assert!(
                (metrics.integrated_lufs - level).abs() < 0.1,
                "{level} dBFS at {rate}: {}",
                metrics.integrated_lufs
            );
            assert!((metrics.momentary_max_lufs - level).abs() < 0.1);
            assert!((metrics.short_term_max_lufs - level).abs() < 0.1);
            assert!((metrics.sample_peak_db - level).abs() < 0.05);
            assert!(metrics.loudness_range_lu < 0.1);
            assert!(metrics.is_audible());
        }
    }
}

#[test]
fn quiet_passages_are_gated_out_of_the_programme_loudness() {
    // After EBU Tech 3341 case 3: -36, then -23, then -36 dBFS. The quiet
    // ends are more than 10 LU under the rest and do not count.
    let tone = |level: f32, seconds: usize| {
        sine(
            1_000.0,
            f64::from(db_to_gain(level)),
            0.0,
            seconds * RATE as usize,
            RATE,
        )
    };
    let mut mono = tone(-36.0, 2);
    mono.extend(tone(-23.0, 20));
    mono.extend(tone(-36.0, 2));
    let metrics = measure(&stereo(&mono), RATE);
    assert!(
        (metrics.integrated_lufs + 23.0).abs() < 0.1,
        "{}",
        metrics.integrated_lufs
    );
    // Without the gate the average would be about a dB lower.
    let silence = measure(&vec![0.0; RATE as usize * 4], RATE);
    assert!(!silence.is_audible());
    assert_eq!(silence.peak_to_loudness_lu(), 0.0);
}

#[test]
fn loudness_range_is_the_spread_between_quiet_and_loud_passages() {
    // After EBU Tech 3342 case 1: -20 then -30 dBFS gives 10 LU, within 1.
    let tone = |level: f32| {
        sine(
            1_000.0,
            f64::from(db_to_gain(level)),
            0.0,
            12 * RATE as usize,
            RATE,
        )
    };
    let mut mono = tone(-20.0);
    mono.extend(tone(-30.0));
    let metrics = measure(&stereo(&mono), RATE);
    assert!(
        (metrics.loudness_range_lu - 10.0).abs() < 1.0,
        "{}",
        metrics.loudness_range_lu
    );
}

#[test]
fn true_peak_is_read_between_the_samples_and_feeding_in_pieces_changes_nothing() {
    // EBU Tech 3341 case 15: a sine at a quarter of the sample rate,
    // started at 45 degrees, has its samples 3 dB under its peak.
    let tone = stereo(&faded(sine(12_000.0, 0.5, TAU / 8.0, RATE as usize, RATE)));
    let metrics = measure(&tone, RATE);
    assert!(
        (metrics.sample_peak_db + 9.03).abs() < 0.05,
        "{}",
        metrics.sample_peak_db
    );
    assert!(
        metrics.true_peak_dbtp > -6.4 && metrics.true_peak_dbtp < -5.8,
        "{}",
        metrics.true_peak_dbtp
    );

    let mut pieces = LoudnessMeter::new(RATE);
    let mut at = 0;
    for size in [2_usize, 14, 4_098, 62, 1_000].iter().cycle() {
        if at >= tone.len() {
            break;
        }
        let end = (at + size).min(tone.len());
        pieces.feed(&tone[at..end]);
        at = end;
    }
    assert_eq!(pieces.frames(), u64::from(RATE));
    assert_eq!(pieces.metrics(), metrics);
}

#[test]
fn an_idle_mastering_limiter_is_an_exact_delay_of_its_fixed_latency() {
    let latency = master_limiter_latency(RATE as f32);
    // 1.5 ms of look-ahead and six samples of intersample detector.
    assert_eq!(latency, 72 + 6);
    let mut limiter = master_limiter(MasterLimiterParams {
        leveling: 0.0,
        ..MasterLimiterParams::default()
    });
    assert_eq!(limiter.latency_samples(), latency);
    let input = sine(440.0, 0.5, 0.0, 9_600, RATE);
    let output = run(&mut limiter, &input, 128);
    assert!(output[..latency].iter().all(|sample| *sample == 0.0));
    assert_eq!(&output[latency..], &input[..input.len() - latency]);
    // No setting moves the latency, the intersample detector included.
    limiter.set_params(&MasterLimiterParams {
        true_peak: false,
        gain_db: 24.0,
        release_ms: 10.0,
        ..MasterLimiterParams::default()
    });
    assert_eq!(limiter.latency_samples(), latency);
    assert!(limiter.tail_samples() >= latency);
}

/// The settled reduction of the level and peak stages for a full-scale
/// 1 kHz tone, and the amplitude that comes out.
fn settled_stages(leveling: f32) -> (f32, f32, f64) {
    let mut limiter = master_limiter(MasterLimiterParams {
        gain_db: 12.0,
        leveling,
        ceiling_db: -1.0,
        ..MasterLimiterParams::default()
    });
    let [level, peak] = limiter.stage_meters();
    let total = limiter.meter();
    let tone = sine(1_000.0, 1.0, 0.0, 5 * RATE as usize, RATE);
    run(&mut limiter, &tone[..4 * RATE as usize], 256);
    // What the meters held from the start is not the settled reading.
    let _ = (level.take_db(), peak.take_db(), total.take_db());
    let output = run(&mut limiter, &tone[4 * RATE as usize..], 256);
    let (level, peak) = (level.take_db(), peak.take_db());
    assert!((total.take_db() - (level + peak)).abs() < 0.1);
    (level, peak, amplitude_at(&output, 1_000.0, RATE))
}

#[test]
fn the_slow_stage_takes_its_share_and_the_peak_stage_only_the_rest() {
    // Driven by 12 dB the tone averages 8.99 dB RMS, 24.99 dB over the
    // level stage's -16 dB threshold. At half leveling the stage takes 0.3
    // of that, 7.5 dB. The peak is then at 4.5 dB, 5.5 dB over the ceiling.
    let (level, peak, amplitude) = settled_stages(0.5);
    assert!((level - 7.5).abs() < 0.3, "level stage {level}");
    assert!((peak - 5.5).abs() < 0.3, "peak stage {peak}");
    let ceiling = f64::from(db_to_gain(-1.0));
    assert!((db(amplitude / ceiling)).abs() < 0.1, "{amplitude}");

    // With no leveling the peak stage does all 13 dB of it.
    let (level, peak, amplitude) = settled_stages(0.0);
    assert_eq!(level, 0.0);
    assert!((peak - 13.0).abs() < 0.3, "peak stage {peak}");
    assert!((db(amplitude / ceiling)).abs() < 0.1, "{amplitude}");

    // At full leveling the slow stage takes 0.6 of 24.99 dB, which leaves
    // the peak 2 dB under the ceiling: the peak stage has nothing to do.
    let (level, peak, amplitude) = settled_stages(1.0);
    assert!((level - 15.0).abs() < 0.3, "level stage {level}");
    assert!(peak < 0.1, "peak stage {peak}");
    assert!((db(amplitude) + 3.0).abs() < 0.3, "{amplitude}");
}

#[test]
fn intersample_peaks_are_held_to_the_ceiling_only_with_true_peak_on() {
    // Samples at 0.707 of a waveform that peaks at 1.0, against -6 dB.
    let tone = sine(12_000.0, 1.0, TAU / 8.0, 2 * RATE as usize, RATE);
    let ceiling = f64::from(db_to_gain(-6.0));
    let held = |true_peak: bool| {
        let mut limiter = master_limiter(MasterLimiterParams {
            leveling: 0.0,
            ceiling_db: -6.0,
            true_peak,
            ..MasterLimiterParams::default()
        });
        let output = run(&mut limiter, &tone, 512);
        let settled = output[RATE as usize..].to_vec();
        // The samples never exceed the ceiling either way.
        assert!(settled.iter().all(|s| f64::from(s.abs()) <= ceiling + 1e-6));
        (
            amplitude_at(&settled, 12_000.0, RATE),
            measure(&stereo(&faded(settled)), RATE).true_peak_dbtp,
        )
    };
    let (amplitude, true_peak) = held(true);
    assert!(
        amplitude < ceiling * 1.03 && amplitude > ceiling * 0.95,
        "{amplitude} against {ceiling}"
    );
    assert!(true_peak < -6.0 + 0.3, "{true_peak} dBTP");

    // Watching the samples alone leaves the waveform 3 dB over.
    let (amplitude, true_peak) = held(false);
    assert!(amplitude > ceiling * 1.38, "{amplitude} against {ceiling}");
    assert!(true_peak > -6.0 + 2.5, "{true_peak} dBTP");
}

#[test]
fn the_mastering_limiter_raises_loudness_and_keeps_the_tonal_balance() {
    let mix = programme(0.08, 8);
    let mut limiter = master_limiter(MasterLimiterParams {
        gain_db: 12.0,
        ..MasterLimiterParams::default()
    });
    let latency = limiter.latency_samples();
    let output = run(&mut limiter, &mix, 256);
    let before = measure(&stereo(&mix), RATE);
    let after = measure(&stereo(&output), RATE);
    let raised = after.integrated_lufs - before.integrated_lufs;
    assert!(raised > 6.0 && raised < 12.1, "raised by {raised} LU");
    assert!(
        after.sample_peak_db <= -1.0 + 0.01,
        "{}",
        after.sample_peak_db
    );

    // One gain for the whole band: every tone moves by the same amount.
    let from = RATE as usize;
    let balance = |signal: &[f32], hz: f64| {
        db(amplitude_at(signal, hz, RATE) / amplitude_at(signal, 1_000.0, RATE))
    };
    let (dry, wet) = (&mix[from..mix.len() - latency], &output[from + latency..]);
    for hz in [100.0, 6_000.0] {
        let shift = balance(wet, hz) - balance(dry, hz);
        assert!(
            shift.abs() < 1.0,
            "{hz} Hz moved by {shift} dB against 1 kHz"
        );
    }
}

#[test]
fn the_mastering_limiter_does_not_depend_on_how_the_audio_is_divided() {
    let mix = programme(0.4, 2);
    let settings = [
        MasterLimiterParams::default(),
        MasterLimiterParams {
            gain_db: 14.0,
            leveling: 0.9,
            saturation: 0.7,
            tone: 0.6,
            ceiling_db: -3.0,
            release_ms: 40.0,
            true_peak: false,
        },
    ];
    let render = |sizes: &[usize]| {
        let mut limiter = master_limiter(settings[0]);
        let (mut left, mut right) = (
            mix.clone(),
            mix.iter().map(|s| -0.7 * s).collect::<Vec<_>>(),
        );
        let mut at = 0;
        let mut index = 0;
        while at < left.len() {
            if at >= left.len() / 2 {
                limiter.set_params(&settings[1]);
            }
            let end = (at + sizes[index % sizes.len()]).min(left.len());
            limiter.process(&mut left[at..end], &mut right[at..end]);
            // An empty block changes nothing.
            limiter.process(&mut [], &mut []);
            at = end;
            index += 1;
        }
        (left, right)
    };
    // The change of settings lands on the same frame in both: 48 000 is a
    // multiple of every size in the first list, and the second is all ones.
    let whole = render(&[48_000]);
    assert_eq!(whole, render(&[1]));
    assert!(whole.0.iter().chain(&whole.1).all(|s| s.is_finite()));
}

#[test]
fn a_master_lands_on_its_loudness_target_under_its_true_peak_ceiling() {
    let mix = stereo(&programme(0.05, 8));
    let settings = MasteringSettings {
        target_lufs: -14.0,
        ceiling_dbtp: -1.0,
        dynamics: 0.5,
    };
    let (output, report) = master(&mix, RATE, &settings).unwrap();
    assert_eq!(output.len(), mix.len());
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert_eq!(report.before, measure(&mix, RATE));
    // The report is what the written audio measures, not an estimate.
    assert_eq!(report.after, measure(&output, RATE));
    assert!(report.target_reached, "{report:?}");
    assert!(
        (report.after.integrated_lufs + 14.0).abs() <= MASTERING_TOLERANCE_LU,
        "{report:?}"
    );
    assert!(
        report.after.true_peak_dbtp <= -1.0 + MASTERING_PEAK_TOLERANCE_DB,
        "{report:?}"
    );
    assert!(report.after.integrated_lufs > report.before.integrated_lufs + 6.0);
    assert!(report.passes >= 2 && report.passes <= 7, "{report:?}");
    assert_eq!(report.settings, settings);

    // The same mix and settings give the same master, sample for sample.
    let (again, second) = master(&mix, RATE, &settings).unwrap();
    assert_eq!(again, output);
    assert_eq!(second, report);
}

#[test]
fn a_master_with_no_dynamics_and_no_limiting_is_the_mix_times_one_gain() {
    let mix = stereo(&programme(0.2, 6));
    let before = measure(&mix, RATE);
    let settings = MasteringSettings {
        target_lufs: (before.integrated_lufs - 6.0).max(-30.0),
        ceiling_dbtp: -1.0,
        dynamics: 0.0,
    };
    let (output, report) = master(&mix, RATE, &settings).unwrap();
    assert!(report.target_reached, "{report:?}");
    assert_eq!(report.compression_db, 0.0);
    assert_eq!(report.limiting_db, 0.0);
    // One trial that lands, and the pass that is kept.
    assert_eq!(report.passes, 2);
    assert!((report.gain_db - (settings.target_lufs - before.integrated_lufs)).abs() < 1e-3);
    // Lined up with the mix and as long: the latency is taken off.
    let gain = db_to_gain(report.gain_db);
    assert_eq!(output.len(), mix.len());
    for (index, (out, dry)) in output.iter().zip(&mix).enumerate() {
        assert!(
            (out - dry * gain).abs() < 1e-5,
            "sample {index}: {out} for {}",
            dry * gain
        );
    }
}

#[test]
fn a_master_refuses_silence_and_reports_a_target_it_cannot_reach() {
    assert_eq!(
        master(&vec![0.0; 96_000], RATE, &MasteringSettings::default()).unwrap_err(),
        MasteringError::Silent
    );
    assert_eq!(
        MasteringSettings {
            target_lufs: f32::NAN,
            ceiling_dbtp: 6.0,
            dynamics: -3.0,
        }
        .sanitized(),
        MasteringSettings {
            target_lufs: -14.0,
            ceiling_dbtp: 0.0,
            dynamics: 0.0,
        }
    );
    // A stereo sine is as loud in LUFS as its peak is high in dBTP, so it
    // cannot be at -6 LUFS under a ceiling of -9 dBTP. The report says so,
    // and the ceiling is the one that is kept.
    let tone = sine(1_000.0, 0.9, 0.0, 4 * RATE as usize, RATE);
    let settings = MasteringSettings {
        target_lufs: -6.0,
        ceiling_dbtp: -9.0,
        dynamics: 0.0,
    };
    let (output, report) = master(&stereo(&tone), RATE, &settings).unwrap();
    assert!(!report.target_reached, "{report:?}");
    assert!(report.after.integrated_lufs < -6.0 - MASTERING_TOLERANCE_LU);
    assert!(report.after.true_peak_dbtp <= -9.0 + 0.3, "{report:?}");
    assert!(report.after.sample_peak_db <= -9.0 + 0.01, "{report:?}");
    assert_eq!(report.passes, 7);
    assert_eq!(output.len(), tone.len() * 2);
}

#[test]
fn the_transient_shaper_splits_into_an_attack_and_a_sustain_that_add_up() {
    // Half a second of silence, then a tone that starts at once and holds.
    let mut input = vec![0.0_f32; 24_000];
    input.extend(sine(2_000.0, 0.3, 0.0, 48_000, RATE));
    let shaped = |listen: TransientListen| {
        let mut shaper = TransientShaper::default();
        shaper.prepare(RATE as f32, 512);
        shaper.set_params(&TransientShaperParams {
            attack: 0.5,
            sustain: -0.25,
            listen,
            ..TransientShaperParams::default()
        });
        run(&mut shaper, &input, 64)
    };
    let both = shaped(TransientListen::Both);
    let attack = shaped(TransientListen::Attack);
    let sustain = shaped(TransientListen::Sustain);
    for index in 0..input.len() {
        assert!(
            (attack[index] + sustain[index] - both[index]).abs() < 1e-6,
            "sample {index}"
        );
    }
    // The attack output is the first milliseconds and little after them.
    let (onset, held) = (24_000..24_240, 60_000..60_240);
    assert!(rms(&attack[onset.clone()]) > 5.0 * rms(&attack[held.clone()]));
    assert!(rms(&attack[onset.clone()]) > 0.05);
    // The sustain output is the held tone, turned down by a quarter of 18 dB.
    let body = db(rms(&sustain[held.clone()]) / rms(&input[held.clone()]));
    assert!((body + 4.5).abs() < 0.5, "{body} dB");
    assert!(rms(&sustain[held.clone()]) > 8.0 * rms(&attack[held]));

    // The host seam gives the same two parts in one pass.
    let mut shaper = TransientShaper::default();
    shaper.prepare(RATE as f32, 512);
    shaper.set_params(&TransientShaperParams {
        attack: 0.5,
        sustain: -0.25,
        ..TransientShaperParams::default()
    });
    for (index, sample) in input.iter().enumerate() {
        let [a, s] = shaper.split_frame([*sample, *sample]);
        assert!((a[0] - attack[index]).abs() < 1e-6, "attack {index}");
        assert!((s[0] - sustain[index]).abs() < 1e-6, "sustain {index}");
        assert_eq!(a[0], a[1]);
    }
}

#[test]
fn the_one_knob_enhancer_gets_louder_all_the_way_up() {
    // Quiet enough that no band reaches a threshold: what is heard is the
    // recipe's drive and make-up, 10 to 12 dB a band at the top.
    let mix = programme(0.004, 6);
    let loudness = |amount: f32| {
        let mut enhancer = OneKnob::default();
        enhancer.prepare(RATE as f32, 512);
        enhancer.set_params(&OneKnobParams { amount });
        let output = run(&mut enhancer, &mix, 256);
        measure(&stereo(&output[RATE as usize..]), RATE).integrated_lufs
    };
    let (off, half, full) = (loudness(0.0), loudness(0.5), loudness(1.0));
    assert!(half > off + 3.0 && full > half + 3.0, "{off} {half} {full}");
    assert!(full - off > 8.0 && full - off < 13.0, "{off} {full}");
}

#[test]
fn dynamics_kinds_report_their_gain_reduction_through_the_registry() {
    let metered = [
        EffectKind::MultibandCompressor,
        EffectKind::MultibandMaximizer,
        EffectKind::OneKnob,
        EffectKind::StageStack,
        EffectKind::MasterLimiter,
    ];
    for kind in metered {
        let effect = AnyEffect::new(&kind.default_params());
        assert!(effect.gain_reduction().is_some(), "{kind:?}");
    }
    for kind in [EffectKind::TransientShaper, EffectKind::BassHarmonics] {
        assert!(
            AnyEffect::new(&kind.default_params())
                .gain_reduction()
                .is_none()
        );
    }

    // A 60 Hz tone 20.9 dB over the low band's threshold at eight to one
    // is turned down by seven eighths of that, 18.3 dB, and the other two
    // bands by nothing.
    let params = EffectParams::MultibandCompressor(MultibandCompressorParams {
        low: BandDynamicsParams {
            threshold_db: -24.0,
            ratio: 8.0,
            attack_ms: 1.0,
            ..BandDynamicsParams::default()
        },
        ..MultibandCompressorParams::default()
    });
    let mut effect = AnyEffect::new(&params);
    effect.prepare(RATE as f32, 512);
    assert!(effect.set_params(&params));
    let meter = effect.gain_reduction().unwrap();
    let mut left = sine(60.0, 0.7, 0.0, 2 * RATE as usize, RATE);
    let mut right = left.clone();
    for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        effect.process(l, r);
    }
    let reading = meter.take_db();
    assert!(reading > 16.5 && reading < 19.5, "{reading} dB");
    assert_eq!(meter.take_db(), 0.0);
    let settled = amplitude_at(&left[RATE as usize..], 60.0, RATE);
    assert!((db(settled / 0.7) + 18.3).abs() < 1.5, "{settled}");
}

#[test]
fn shaper_saved_aux_routes_remove_only_the_routed_parts_and_ignore_listen() {
    let input = programme(0.2, 1);
    let params = TransientShaperParams {
        attack: 0.4,
        sustain: -0.3,
        ..Default::default()
    };
    let mut reference = TransientShaper::default();
    reference.prepare(RATE as f32, 512);
    reference.set_params(&params);
    let expected: Vec<_> = input
        .iter()
        .map(|x| reference.split_frame([*x, -*x]))
        .collect();
    for mask in 0..4 {
        for listen in [
            TransientListen::Both,
            TransientListen::Attack,
            TransientListen::Sustain,
        ] {
            let settings =
                EffectParams::TransientShaper(TransientShaperParams { listen, ..params });
            let render = |block| {
                let mut effect = AnyEffect::new(&settings);
                effect.prepare(RATE as f32, 512);
                effect.set_params(&settings);
                let mut left = input.clone();
                let mut right: Vec<_> = input.iter().map(|x| -*x).collect();
                let mut attack = vec![[99.0; 2]; input.len()];
                let mut sustain = attack.clone();
                let mut unused = attack.clone();
                for ((((l, r), a), s), u) in left
                    .chunks_mut(block)
                    .zip(right.chunks_mut(block))
                    .zip(attack.chunks_mut(block))
                    .zip(sustain.chunks_mut(block))
                    .zip(unused.chunks_mut(block))
                {
                    effect.process_aux(
                        l,
                        r,
                        None,
                        &mut windfall_dsp::AuxBlock {
                            routed: mask,
                            outputs: [a, s, u],
                        },
                    );
                }
                assert!(unused.iter().all(|frame| *frame == [0.0; 2]));
                (left, right, attack, sustain)
            };
            let output = render(257);
            assert_eq!(output, render(1));
            for (i, parts) in expected.iter().enumerate() {
                let routed = [mask & 1 != 0, mask & 2 != 0];
                let audible = [
                    listen != TransientListen::Sustain,
                    listen != TransientListen::Attack,
                ];
                for c in 0..2 {
                    assert_eq!(output.2[i][c], if routed[0] { parts[0][c] } else { 0.0 });
                    assert_eq!(output.3[i][c], if routed[1] { parts[1][c] } else { 0.0 });
                    let main = (if !routed[0] && audible[0] {
                        parts[0][c]
                    } else {
                        0.0
                    }) + (if !routed[1] && audible[1] {
                        parts[1][c]
                    } else {
                        0.0
                    });
                    let actual = if c == 0 { output.0[i] } else { output.1[i] };
                    assert!(
                        (actual - main).abs() < 1e-6,
                        "frame {i}, mask {mask}, {listen:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn unity_band_sum_matches_the_independent_cascaded_allpass_phase_reference() {
    // LR4 low+high equals the second-order Butterworth allpass. Derive
    // its bilinear coefficients here rather than using product filters.
    let allpass = |edge: f64, hz: f64, rate: f64| {
        let k = (std::f64::consts::PI * edge / rate).tan();
        let norm = 1.0 + std::f64::consts::SQRT_2 * k + k * k;
        let a1 = 2.0 * (k * k - 1.0) / norm;
        let a2 = (1.0 - std::f64::consts::SQRT_2 * k + k * k) / norm;
        let w = TAU * hz / rate;
        let (c1, s1, c2, s2) = (w.cos(), -w.sin(), (2.0 * w).cos(), -(2.0 * w).sin());
        let (nr, ni) = (a2 + a1 * c1 + c2, a1 * s1 + s2);
        let (dr, di) = (1.0 + a1 * c1 + a2 * c2, a1 * s1 + a2 * s2);
        let d = dr * dr + di * di;
        ((nr * dr + ni * di) / d, (ni * dr - nr * di) / d)
    };
    for rate in [44_100, 48_000, 96_000] {
        for hz in [20.0, 200.0, 1_000.0, 2_500.0, 12_000.0] {
            let unity = BandDynamicsParams {
                ratio: 1.0,
                makeup_db: 0.0,
                ..Default::default()
            };
            let mut compressor = windfall_dsp::multiband::MultibandCompressor::default();
            compressor.prepare(rate as f32, 512);
            compressor.set_params(&MultibandCompressorParams {
                low: unity,
                mid: unity,
                high: unity,
                ..Default::default()
            });
            let input = sine(hz, 0.1, 0.0, 3 * rate as usize, rate);
            let output = run(&mut compressor, &input, 137);
            let (mut re, mut im) = (0.0, 0.0);
            for (n, x) in output.iter().enumerate().skip(2 * rate as usize) {
                let w = TAU * hz * n as f64 / f64::from(rate);
                re += f64::from(*x) * w.sin();
                im += f64::from(*x) * w.cos();
            }
            re *= 20.0 / f64::from(rate);
            im *= 20.0 / f64::from(rate);
            let (a, b) = allpass(200.0, hz, f64::from(rate));
            let (c, d) = allpass(2_500.0, hz, f64::from(rate));
            assert!(
                (re - (a * c - b * d)).hypot(im - (a * d + b * c)) < 2e-4,
                "phase {rate} Hz {hz}: {re} {im}"
            );
            assert!((re.hypot(im) - 1.0).abs() < 2e-4);
        }
    }
}

#[test]
fn maximizer_band_ceilings_protect_each_band_before_the_master_sum() {
    use windfall_dsp::multiband::{MultibandMaximizer, MultibandMaximizerParams};
    let unity = BandDynamicsParams {
        ratio: 1.0,
        makeup_db: 0.0,
        ..Default::default()
    };
    for (hz, ceiling) in [(40.0, -18.0), (800.0, -12.0), (12_000.0, -6.0)] {
        let mut effect = MultibandMaximizer::default();
        effect.prepare(RATE as f32, 512);
        effect.set_params(&MultibandMaximizerParams {
            low: unity,
            mid: unity,
            high: unity,
            low_ceiling_db: if hz < 200.0 { ceiling } else { 0.0 },
            mid_ceiling_db: if hz > 200.0 && hz < 2_500.0 {
                ceiling
            } else {
                0.0
            },
            high_ceiling_db: if hz > 2_500.0 { ceiling } else { 0.0 },
            ceiling_db: 0.0,
            input_gain_db: 0.0,
            ..Default::default()
        });
        let input = sine(hz, 0.8, 0.0, 3 * RATE as usize, RATE);
        let output = run(&mut effect, &input, 131);
        let fundamental = amplitude_at(&output[2 * RATE as usize..], hz, RATE);
        assert!(
            (db(fundamental) - f64::from(ceiling)).abs() < 1.5,
            "{hz}: {} dB",
            db(fundamental)
        );
        assert_eq!(effect.latency_samples(), 96);
        assert!(effect.tail_samples() >= effect.latency_samples());
    }
}

#[test]
fn one_knob_is_constrained_but_still_has_a_real_ceiling_latency_and_finite_tail() {
    use windfall_dsp::ParamSet;
    assert_eq!(OneKnobParams::descriptors().len(), 1);
    assert_eq!(OneKnobParams::descriptors()[0].id, "amount");
    for amount in [0.0, 0.5, 1.0] {
        let mut effect = OneKnob::default();
        effect.prepare(RATE as f32, 512);
        effect.set_params(&OneKnobParams { amount });
        let mut input = vec![0.0_f32; RATE as usize];
        input[0] = 0.01;
        let output = run(&mut effect, &input, 79);
        assert_eq!(effect.latency_samples(), 96);
        assert!(output[..96].iter().all(|x| *x == 0.0));
        assert!(output[96].abs() > 1e-6);
        assert!(output[40_000..].iter().all(|x| x.abs() < 1e-8));
        effect.reset();
        let hot = programme(2.0, 1);
        let limited = run(&mut effect, &hot, 257);
        let ceiling = db_to_gain(-0.3 * amount);
        assert!(limited.iter().all(|x| x.abs() <= ceiling + 1e-6));
    }
}

#[test]
fn bass_octave_survives_fundamental_removal_and_amount_is_additive_after_aligned_dry() {
    use windfall_dsp::multiband::{BassHarmonics, BassHarmonicsParams, HARMONICS_LATENCY_SAMPLES};
    let input = sine(60.0, 0.3, 0.0, 3 * RATE as usize, RATE);
    let render = |amount| {
        let mut effect = BassHarmonics::default();
        effect.prepare(RATE as f32, 512);
        effect.set_params(&BassHarmonicsParams {
            amount,
            drive: 3.0,
            cutoff_hz: 180.0,
        });
        assert_eq!(effect.latency_samples(), HARMONICS_LATENCY_SAMPLES);
        let mut left = input.clone();
        let mut right: Vec<_> = input.iter().map(|x| -*x).collect();
        for (l, r) in left.chunks_mut(131).zip(right.chunks_mut(131)) {
            effect.process(l, r);
        }
        (left, right)
    };
    let (dry, dry_r) = render(0.0);
    let latency = HARMONICS_LATENCY_SAMPLES;
    assert!(dry[..latency].iter().all(|x| *x == 0.0));
    // The dry path is the recombined LR4 bands, then the 13-frame delay;
    // unity magnitude does not mean unchanged phase. Independently evaluate
    // the two second-order Butterworth allpasses in direct form.
    let mut reference: Vec<f64> = input.iter().map(|&x| f64::from(x)).collect();
    for edge in [180.0, 2_500.0] {
        let g = (std::f64::consts::PI * edge / f64::from(RATE)).tan();
        let k = 1.0 / f64::from(std::f32::consts::FRAC_1_SQRT_2);
        let norm = 1.0 / (1.0 + k * g + g * g);
        let a1 = 2.0 * (g * g - 1.0) * norm;
        let a2 = (1.0 - k * g + g * g) * norm;
        let mut state = [0.0; 4];
        for sample in &mut reference {
            let x = *sample;
            let y = a2 * x + a1 * state[0] + state[1] - a1 * state[2] - a2 * state[3];
            state = [x, state[0], y, state[2]];
            *sample = y;
        }
    }
    for n in latency..dry.len() {
        assert!((f64::from(dry[n]) - reference[n - latency]).abs() < 2e-7, "dry sample {n}");
    }
    let (full, full_r) = render(1.0);
    let (quarter, _) = render(0.25);
    let wet: Vec<_> = full.iter().zip(&dry).map(|(a, b)| a - b).collect();
    for n in RATE as usize..input.len() {
        assert!((quarter[n] - dry[n] - 0.25 * wet[n]).abs() < 2e-7);
        // Squaring is even; inverse fundamentals receive the same new octave.
        assert!((full_r[n] - dry_r[n] - wet[n]).abs() < 2e-7);
    }
    let octave = amplitude_at(&wet[RATE as usize..], 120.0, RATE);
    assert!(octave > 0.01, "small-speaker octave {octave}");
    assert!(amplitude_at(&dry[RATE as usize..], 120.0, RATE) < 1e-6);
    let dc = wet[RATE as usize..]
        .iter()
        .map(|x| f64::from(*x))
        .sum::<f64>()
        / f64::from(2 * RATE);
    assert!(dc.abs() < 1e-5, "generated path DC {dc}");
}

/// Independent 16x reconstruction with a 32-tap Hann-windowed sinc, not
/// the product's 4x/12-tap Lanczos detector or LoudnessMeter.
fn reference_true_peak(samples: &[f32]) -> f64 {
    let mut maximum = samples
        .iter()
        .map(|x| f64::from(x.abs()))
        .fold(0.0, f64::max);
    let sinc = |x: f64| {
        if x.abs() < 1e-12 {
            1.0
        } else {
            (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
        }
    };
    for n in 0..samples.len() {
        for phase in 1..16 {
            let time = n as f64 + f64::from(phase) / 16.0;
            let (mut sum, mut norm) = (0.0, 0.0);
            for offset in -15..=16 {
                let at = n as isize + offset;
                let delta = time - at as f64;
                let window = 0.5 + 0.5 * (std::f64::consts::PI * delta / 16.0).cos();
                let weight = sinc(delta) * window;
                norm += weight;
                if at >= 0 && at < samples.len() as isize {
                    sum += f64::from(samples[at as usize]) * weight;
                }
            }
            maximum = maximum.max((sum / norm).abs());
        }
    }
    maximum
}

#[test]
fn mastering_limiter_true_peak_is_checked_by_a_different_dense_reconstructor() {
    for rate in [44_100, 48_000, 96_000] {
        let input = faded(sine(f64::from(rate) / 4.0, 1.4, TAU / 8.0, 4_096, rate));
        assert!(reference_true_peak(&input) > 1.3);
        let mut effect = MasterLimiter::default();
        effect.prepare(rate as f32, 512);
        effect.set_params(&MasterLimiterParams {
            ceiling_db: -1.0,
            leveling: 0.0,
            saturation: 0.0,
            true_peak: true,
            ..Default::default()
        });
        let mut padded = input.clone();
        padded.resize(input.len() + effect.latency_samples() + 32, 0.0);
        let output = run(&mut effect, &padded, 53);
        let peak = db(reference_true_peak(&output));
        // A moving gain is allowed 0.3 dB reconstruction tolerance, as
        // documented; stationary signals are covered by the analytic test.
        assert!(peak <= -0.7, "{rate}: independent true peak {peak} dBTP");
        assert!(output.iter().all(|x| x.abs() <= db_to_gain(-1.0) + 1e-6));
    }
}

#[test]
fn slow_leveling_keeps_shared_stereo_gain_and_tonal_balance() {
    let mut effect = master_limiter(MasterLimiterParams {
        gain_db: 6.0,
        leveling: 1.0,
        saturation: 0.0,
        ceiling_db: 0.0,
        ..Default::default()
    });
    let mut left: Vec<_> = (0..4 * RATE)
        .map(|n| {
            let t = f64::from(n) / f64::from(RATE);
            (0.2 * (TAU * 60.0 * t).sin()
                + 0.15 * (TAU * 1_000.0 * t).sin()
                + 0.1 * (TAU * 8_000.0 * t).sin()) as f32
        })
        .collect();
    let input = left.clone();
    let mut right: Vec<_> = left.iter().map(|x| -*x * 0.25).collect();
    for (l, r) in left.chunks_mut(137).zip(right.chunks_mut(137)) {
        effect.process(l, r);
    }
    for (l, r) in left.iter().zip(&right) {
        assert!((r + 0.25 * l).abs() < 1e-7);
    }
    let mut gains = Vec::new();
    for hz in [60.0, 1_000.0, 8_000.0] {
        gains.push(
            amplitude_at(&left[3 * RATE as usize..], hz, RATE)
                / amplitude_at(&input[3 * RATE as usize..], hz, RATE),
        );
    }
    let smallest = gains.iter().copied().fold(f64::INFINITY, f64::min);
    let largest = gains.iter().copied().fold(0.0, f64::max);
    assert!(db(largest / smallest) < 0.15, "tonal balance {gains:?}");
    assert!(effect.stage_meters()[0].take_db() > 1.0);
}

#[test]
fn simplified_two_stages_match_independent_saturation_and_linked_release_reference() {
    use windfall_dsp::mastering::{StageStack, StageStackParams};
    let params = StageStackParams {
        drive_db: 9.0,
        tone: 0.4,
        ceiling_db: -6.0,
        mix: 0.75,
    };
    let input = programme(0.6, 1);
    let mut effect = StageStack::default();
    effect.prepare(RATE as f32, 512);
    effect.set_params(&params);
    let mut left = input.clone();
    let mut right: Vec<_> = input.iter().map(|x| *x * 0.2).collect();
    effect.process(&mut left, &mut right);
    let drive = 10.0_f64.powf(f64::from(params.drive_db) / 20.0);
    let ceiling = 10.0_f64.powf(f64::from(params.ceiling_db) / 20.0);
    let release = 1.0 - (-1000.0 / (3.0 * f64::from(RATE))).exp();
    let mut gain = 1.0_f64;
    for (n, dry) in input.iter().enumerate() {
        let shape = |x: f64| {
            let t = (x * drive * 0.5).tanh();
            2.0 * t + f64::from(params.tone) * 0.5 * t * t * (1.0 - t * t)
        };
        let wet = [shape(f64::from(*dry)), shape(f64::from(*dry) * 0.2)];
        let peak = wet[0].abs().max(wet[1].abs());
        let needed = (ceiling / peak.max(1e-30)).min(1.0);
        gain = if needed < gain {
            needed
        } else {
            gain + (needed - gain) * release
        };
        for c in 0..2 {
            let d = f64::from(*dry) * if c == 0 { 1.0 } else { 0.2 };
            let expected = d * 0.25 + (wet[c] * gain).clamp(-ceiling, ceiling) * 0.75;
            let actual = if c == 0 { left[n] } else { right[n] };
            assert!(
                (f64::from(actual) - expected).abs() < 3e-6,
                "stage reference {n} channel {c}"
            );
        }
    }
    assert_eq!(effect.latency_samples(), 0);
    assert_eq!(effect.tail_samples(), 0);
}

#[test]
fn local_mastering_passes_are_invariant_to_analysis_and_render_block_sizes() {
    use windfall_dsp::mastering::Mastering;
    let input = stereo(&programme(0.03, 2));
    let settings = MasteringSettings {
        target_lufs: -18.0,
        ceiling_dbtp: -1.0,
        dynamics: 0.5,
    };
    let render = |frames: usize| {
        let mut chain = Mastering::new(RATE, settings);
        for block in input.chunks(frames * 2) {
            chain.analyze(block);
        }
        chain.begin().unwrap();
        let mut block = Vec::new();
        loop {
            let keep = chain.is_render_pass();
            let mut result = Vec::new();
            for samples in input.chunks(frames * 2) {
                chain.process(samples, &mut block);
                if keep {
                    result.extend_from_slice(&block);
                }
            }
            let report = chain.finish_pass(&mut block);
            if keep {
                result.extend_from_slice(&block);
            }
            if let Some(report) = report {
                return (result, report);
            }
        }
    };
    let expected = render(8_192);
    assert_eq!(expected.0.len(), input.len());
    assert_eq!(expected, render(137));
    assert_eq!(expected, render(1));
}

#[test]
fn an_unreachable_loudness_target_keeps_peak_safety_ahead_of_loudness_error() {
    let input = stereo(&programme(0.5, 3));
    let settings = MasteringSettings {
        target_lufs: -6.0,
        ceiling_dbtp: -9.0,
        dynamics: 0.0,
    };
    let (output, report) = master(&input, RATE, &settings).unwrap();
    assert!(!report.target_reached, "{report:?}");
    assert!(
        report.after.true_peak_dbtp <= settings.ceiling_dbtp + MASTERING_PEAK_TOLERANCE_DB,
        "{report:?}"
    );
    assert_eq!(report.after, measure(&output, RATE));
}
