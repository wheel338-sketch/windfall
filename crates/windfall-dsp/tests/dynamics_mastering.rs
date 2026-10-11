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

fn left_of(interleaved: &[f32]) -> Vec<f32> {
    interleaved.chunks_exact(2).map(|frame| frame[0]).collect()
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
            let tones = (TAU * 100.0 * t).sin() + (TAU * 1_000.0 * t).sin() + (TAU * 6_000.0 * t).sin();
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
    assert!(after.sample_peak_db <= -1.0 + 0.01, "{}", after.sample_peak_db);

    // One gain for the whole band: every tone moves by the same amount.
    let from = RATE as usize;
    let balance = |signal: &[f32], hz: f64| {
        db(amplitude_at(signal, hz, RATE) / amplitude_at(signal, 1_000.0, RATE))
    };
    let (dry, wet) = (&mix[from..mix.len() - latency], &output[from + latency..]);
    for hz in [100.0, 6_000.0] {
        let shift = balance(wet, hz) - balance(dry, hz);
        assert!(shift.abs() < 1.0, "{hz} Hz moved by {shift} dB against 1 kHz");
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
        let (mut left, mut right) = (mix.clone(), mix.iter().map(|s| -0.7 * s).collect::<Vec<_>>());
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
        assert!(AnyEffect::new(&kind.default_params()).gain_reduction().is_none());
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
