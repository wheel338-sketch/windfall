//! Authored independent dry/room fixtures. Written, NOT executed by worker.
use std::f32::consts::TAU;
use std::time::Duration;
use windfall_analysis::{
    AnalysisError, CancelToken, Work,
    dereverb::{DereverbMode, DereverbSettings, DereverbStream},
};
const RATE: u32 = 8_000;
fn work() -> Work {
    Work::new(
        CancelToken::default(),
        100_000_000_000,
        Duration::from_secs(600),
    )
    .unwrap()
}
fn dry(kind: usize) -> Vec<f32> {
    let mut signal = vec![0.0; RATE as usize * 4];
    for onset in [0.2, 0.9, 1.6, 2.3] {
        let start = (onset * RATE as f32) as usize;
        if kind == 0 {
            signal[start] = 0.8;
            continue;
        }
        let length = if kind == 1 {
            (RATE / 4) as usize
        } else {
            (RATE / 6) as usize
        };
        let mut phase = 0.0;
        for i in 0..length {
            let t = i as f32 / RATE as f32;
            phase += TAU * 180.0 * (1.0 + 0.008 * (TAU * 5.5 * t).sin()) / RATE as f32;
            let voice =
                (phase.sin() + 0.35 * (2.0 * phase).sin() + 0.15 * (3.0 * phase).sin()) * 0.22;
            let edge = (i as f32 / 32.0).min(1.0) * ((length - i) as f32 / 32.0).min(1.0);
            signal[start + i] = if kind == 1 {
                voice * edge
            } else {
                ((TAU * (95.0 * t + 30.0 * t * t)).sin() * (-t / 0.008).exp() * 0.5 + voice * 0.4)
                    * edge
            };
        }
    }
    signal
}
fn room(dry: &[f32], rt: f32) -> Vec<f32> {
    let mut impulse = vec![0.0; (RATE as f32 * rt) as usize];
    impulse[0] = 1.0;
    let mut seed: u32 = 0x916392;
    for (i, x) in impulse.iter_mut().enumerate().skip((RATE / 40) as usize) {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
        *x = noise * 0.09 * (-6.907755 * i as f32 / RATE as f32 / rt).exp();
    }
    let mut wet = vec![0.0; dry.len()];
    for (i, &x) in dry.iter().enumerate() {
        if x != 0.0 {
            for (j, &h) in impulse.iter().take(wet.len() - i).enumerate() {
                wet[i + j] += x * h;
            }
        }
    }
    wet
}
fn process(
    input: &[f32],
    channels: u16,
    mode: DereverbMode,
    amount: f32,
    block: usize,
) -> Vec<f32> {
    let mut processor =
        DereverbStream::new(RATE, channels, DereverbSettings { mode, amount }).unwrap();
    let mut output = Vec::new();
    let mut budget = work();
    let mut emit = |s: &[f32], _: &mut Work| {
        output.extend_from_slice(s);
        Ok(())
    };
    for chunk in input.chunks(block * channels as usize) {
        processor.push(chunk, &mut budget, &mut emit).unwrap();
        assert!(processor.retained_frames() <= RATE as usize * 3);
    }
    processor.finish(&mut budget, &mut emit).unwrap();
    assert_eq!(output.len(), input.len());
    output
}
fn energy(input: &[f32]) -> f64 {
    input.iter().map(|x| (*x as f64).powi(2)).sum()
}
#[test]
fn authored_impulses_vibrato_and_music_reduce_late_tail_with_bounded_direct_damage() {
    for kind in 0..3 {
        for mode in [DereverbMode::Vocal, DereverbMode::General] {
            let reference = dry(kind);
            let wet = room(&reference, 0.65);
            let output = process(&wet, 1, mode, 1.0, 257);
            let tail = (2.3 * RATE as f32) as usize
                + if kind == 0 {
                    (RATE / 10) as usize
                } else {
                    (RATE * 3 / 10) as usize
                };
            assert!(
                energy(&output[tail..]) < energy(&wet[tail..]) * 0.9,
                "late-tail reduction kind={kind} mode={mode:?}"
            );
            let start = (2.3 * RATE as f32) as usize;
            let end = start + (RATE / 50) as usize;
            let baseline: Vec<_> = wet[start..end]
                .iter()
                .zip(&reference[start..end])
                .map(|(a, b)| a - b)
                .collect();
            let distortion: Vec<_> = output[start..end]
                .iter()
                .zip(&reference[start..end])
                .map(|(a, b)| a - b)
                .collect();
            assert!(
                energy(&distortion)
                    <= energy(&baseline) + energy(&reference[start..end]) * 0.01 + 1e-8,
                "direct-sound distortion"
            );
        }
    }
}
#[test]
fn already_dry_is_preserved_silence_and_zero_amount_are_exact() {
    for kind in 0..3 {
        let input = dry(kind);
        for mode in [DereverbMode::Vocal, DereverbMode::General] {
            let output = process(&input, 1, mode, 1.0, 97);
            let error: Vec<_> = input.iter().zip(&output).map(|(a, b)| a - b).collect();
            assert!(energy(&error) / energy(&input).max(1e-20) < 1e-6);
            assert_eq!(process(&input, 1, mode, 0.0, 97), input);
            assert_eq!(process(&vec![0.0; 321], 1, mode, 1.0, 7), vec![0.0; 321]);
        }
    }
}
#[test]
fn stereo_image_partition_invariance_and_distinct_modes() {
    let wet = room(&dry(1), 0.8);
    let stereo: Vec<_> = wet.iter().flat_map(|x| [*x, -*x * 0.5]).collect();
    let vocal = process(&stereo, 2, DereverbMode::Vocal, 1.0, 31);
    assert_eq!(vocal, process(&stereo, 2, DereverbMode::Vocal, 1.0, 4096));
    for frame in vocal.as_chunks::<2>().0 {
        assert!((frame[1] + frame[0] * 0.5).abs() < 1e-5);
    }
    let general = process(&stereo, 2, DereverbMode::General, 1.0, 211);
    assert!(vocal.iter().zip(general).any(|(a, b)| (a - b).abs() > 1e-4));
}

#[test]
fn partial_wet_is_linear_and_long_stream_retention_is_bounded() {
    let input = room(&dry(2), 0.7);
    for mode in [DereverbMode::General, DereverbMode::Vocal] {
        let wet = process(&input, 1, mode, 1.0, 509);
        let half = process(&input, 1, mode, 0.5, 509);
        for ((dry, wet), half) in input.iter().zip(&wet).zip(half) {
            assert!((half - (dry + wet) * 0.5).abs() < 2e-6);
        }
    }
    // Five minutes without retaining the file or its output in the harness.
    let mut stream = DereverbStream::new(
        RATE,
        2,
        DereverbSettings {
            mode: DereverbMode::General,
            amount: 1.0,
        },
    )
    .unwrap();
    let mut emitted = 0usize;
    let mut budget = work();
    let block = vec![0.0; RATE as usize * 2];
    let mut emit = |samples: &[f32], _: &mut Work| {
        emitted += samples.len() / 2;
        Ok(())
    };
    for _ in 0..300 {
        stream.push(&block, &mut budget, &mut emit).unwrap();
        assert!(stream.retained_frames() <= RATE as usize * 3);
    }
    stream.finish(&mut budget, &mut emit).unwrap();
    assert_eq!(emitted, RATE as usize * 300);
    assert_eq!(stream.retained_frames(), 0);
}
#[test]
fn cancellation_bad_shapes_and_stream_lifecycle_fail_without_finishing() {
    let token = CancelToken::default();
    let mut budget = Work::new(token.clone(), 100_000, Duration::from_secs(30)).unwrap();
    token.cancel();
    let mut stream = DereverbStream::new(
        RATE,
        2,
        DereverbSettings {
            mode: DereverbMode::Vocal,
            amount: 1.0,
        },
    )
    .unwrap();
    assert!(matches!(
        stream.push(&[0.0, 0.0], &mut budget, &mut |_, _| Ok(())),
        Err(AnalysisError::Cancelled)
    ));
    assert!(
        DereverbStream::new(
            RATE,
            3,
            DereverbSettings {
                mode: DereverbMode::General,
                amount: 1.0
            }
        )
        .is_err()
    );
    assert!(
        stream
            .push(&[0.0], &mut work(), &mut |_, _| Ok(()))
            .is_err()
    );
    stream.finish(&mut work(), &mut |_, _| Ok(())).unwrap();
    assert!(
        stream
            .push(&[0.0, 0.0], &mut work(), &mut |_, _| Ok(()))
            .is_err()
    );
}

#[test]
fn cancel_before_final_partial_segment_never_emits_dry_bypass() {
    let token = CancelToken::default();
    let mut budget = Work::new(token.clone(), 1_000_000, Duration::from_secs(30)).unwrap();
    let mut stream = DereverbStream::new(
        RATE,
        1,
        DereverbSettings {
            mode: DereverbMode::General,
            amount: 1.0,
        },
    )
    .unwrap();
    let mut emitted = 0;
    let mut emit = |samples: &[f32], _: &mut Work| {
        emitted += samples.len();
        Ok(())
    };
    stream.push(&[0.0; 137], &mut budget, &mut emit).unwrap();
    token.cancel();
    assert!(matches!(
        stream.finish(&mut budget, &mut emit),
        Err(AnalysisError::Cancelled)
    ));
    assert_eq!(emitted, 0);
}
