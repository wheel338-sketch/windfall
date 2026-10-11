use super::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn hall(params: LushSpaceParams, rate: f32) -> LushSpace {
    let mut effect = LushSpace::default();
    effect.prepare(rate, 257);
    effect.set_params(&params);
    effect
}

fn impulse(params: LushSpaceParams, rate: f32, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let mut effect = hall(params, rate);
    let mut left = vec![0.0; (rate * seconds) as usize];
    let mut right = left.clone();
    left[0] = 1.0;
    right[0] = 1.0;
    effect.process(&mut left, &mut right);
    (left, right)
}

fn energy(signal: &[f32]) -> f64 {
    signal.iter().map(|x| f64::from(*x).powi(2)).sum()
}

#[test]
fn parameter_metadata_serde_and_ts_agree() {
    let params = LushSpaceParams::default();
    assert_eq!(LushSpaceParams::NAME, "Lush Space");
    assert_eq!(LushSpaceParams::descriptors().len(), 11);
    let json = serde_json::to_value(params).unwrap();
    for (index, info) in LushSpaceParams::descriptors().iter().enumerate() {
        assert_eq!(params.get(index), Some(info.default));
        assert_eq!(LushSpaceParams::index_of(info.id), Some(index));
        assert!((json[info.id].as_f64().unwrap() - f64::from(info.default)).abs() < 1e-6);
        let mut changed = params;
        changed.set(index, info.max);
        assert_eq!(changed.get(index), Some(info.max));
        changed.set(index, info.min);
        assert_eq!(changed.get(index), Some(info.min));
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            changed.set(index, value);
            assert_eq!(changed.get(index), Some(info.default));
        }
    }
    assert_eq!(
        serde_json::from_value::<LushSpaceParams>(json).unwrap(),
        params
    );
    assert!(LushSpaceParams::decl(&ts_rs::Config::default()).contains("modulationDepthMs"));
    assert_eq!(params.get(11), None);
    assert!(!LushSpaceParams::default().set(11, 0.0));
}

#[test]
fn impulse_timing_separates_early_reflections_and_long_tank() {
    let params = LushSpaceParams {
        pre_delay_ms: 10.0,
        early_level: 1.0,
        modulation_depth_ms: 0.0,
        width: 1.0,
        mix: 1.0,
        ..Default::default()
    };
    for rate in [1_000.0, 48_000.0] {
        let (left, right) = impulse(params, rate, 0.3);
        let first_l = frames(10.0, rate) + frames(17.0, rate);
        let first_r = frames(10.0, rate) + frames(23.0, rate);
        assert!(left[..first_l].iter().all(|x| *x == 0.0));
        assert!(right[..first_r].iter().all(|x| *x == 0.0));
        assert!((left[first_l] - 0.55).abs() < 1e-6);
        assert!((right[first_r] - 0.55).abs() < 1e-6);

        let (left, right) = impulse(
            LushSpaceParams {
                early_level: 0.0,
                ..params
            },
            rate,
            0.3,
        );
        let late_l = frames(10.0, rate) + frames(151.0, rate);
        let late_r = frames(10.0, rate) + frames(173.0, rate);
        assert!(left[..late_l].iter().all(|x| *x == 0.0));
        assert!(right[..late_r].iter().all(|x| *x == 0.0));
        assert!(left[late_l].abs() > 0.01);
        assert!(right[late_r].abs() > 0.01);
    }
}

#[test]
fn modulation_rate_and_depth_change_the_tail() {
    let base = LushSpaceParams {
        decay_s: 8.0,
        early_level: 0.0,
        pre_delay_ms: 0.0,
        damping: 0.1,
        modulation_rate_hz: 0.7,
        modulation_depth_ms: 1.5,
        mix: 1.0,
        ..Default::default()
    };
    let moving = impulse(base, 12_000.0, 3.0);
    for other in [
        LushSpaceParams {
            modulation_depth_ms: 0.0,
            ..base
        },
        LushSpaceParams {
            modulation_rate_hz: 0.0,
            ..base
        },
    ] {
        let stationary = impulse(other, 12_000.0, 3.0);
        let difference: f64 = moving.0[12_000..]
            .iter()
            .zip(&stationary.0[12_000..])
            .map(|(a, b)| f64::from(a - b).powi(2))
            .sum();
        assert!(difference > 1e-4, "tail difference {difference}");
    }
}

#[test]
fn width_zero_preserves_the_mid_tail() {
    let base = LushSpaceParams {
        early_level: 0.0,
        mix: 1.0,
        ..Default::default()
    };
    let (wide_l, wide_r) = impulse(base, 8_000.0, 2.0);
    let (mono_l, mono_r) = impulse(LushSpaceParams { width: 0.0, ..base }, 8_000.0, 2.0);
    assert_eq!(mono_l, mono_r);
    assert!(energy(&mono_l) > 1e-4);
    for ((left, right), mono) in wide_l.iter().zip(wide_r).zip(mono_l) {
        assert!((0.5 * (left + right) - mono).abs() < 1e-6);
    }
}

#[test]
fn decay_zero_is_short_finite_and_long_decay_survives_later() {
    let base = LushSpaceParams {
        early_level: 0.0,
        pre_delay_ms: 0.0,
        modulation_depth_ms: 0.0,
        damping: 0.0,
        mix: 1.0,
        ..Default::default()
    };
    let short = LushSpaceParams {
        decay_s: 0.0,
        ..base
    };
    let short_hall = hall(short, 8_000.0);
    assert!(short_hall.tail_samples() <= 8_400);
    let (short_l, short_r) = impulse(short, 8_000.0, 3.0);
    assert!(
        short_l[short_hall.tail_samples() + 1..]
            .iter()
            .chain(&short_r[short_hall.tail_samples() + 1..])
            .all(|x| *x == 0.0)
    );
    let (long, _) = impulse(
        LushSpaceParams {
            decay_s: 8.0,
            ..base
        },
        8_000.0,
        9.0,
    );
    assert!(energy(&long[32_000..40_000]) > 1e-5);
    assert!(energy(&long[56_000..64_000]) < energy(&long[8_000..16_000]) * 0.01);
}

#[test]
fn damping_reduces_high_frequency_tail_energy() {
    let base = LushSpaceParams {
        decay_s: 6.0,
        early_level: 0.0,
        modulation_depth_ms: 0.0,
        mix: 1.0,
        ..Default::default()
    };
    let (bright, _) = impulse(
        LushSpaceParams {
            damping: 0.0,
            ..base
        },
        24_000.0,
        2.0,
    );
    let (dark, _) = impulse(
        LushSpaceParams {
            damping: 1.0,
            ..base
        },
        24_000.0,
        2.0,
    );
    let high_energy = |samples: &[f32]| {
        samples[12_000..]
            .windows(2)
            .map(|pair| f64::from(pair[1] - pair[0]).powi(2))
            .sum::<f64>()
    };
    assert!(high_energy(&dark) < high_energy(&bright) * 0.1);
}

#[test]
fn irregular_blocks_and_automation_match_one_block() {
    let mut whole = hall(
        LushSpaceParams {
            mix: 1.0,
            ..Default::default()
        },
        12_000.0,
    );
    let mut split = hall(
        LushSpaceParams {
            mix: 1.0,
            ..Default::default()
        },
        12_000.0,
    );
    let mut left: Vec<f32> = (0..24_001)
        .map(|i| ((i * 17 % 31) as f32 - 15.0) * 0.002)
        .collect();
    let mut right: Vec<f32> = left.iter().map(|x| -0.3 * x).collect();
    let (mut sl, mut sr) = (left.clone(), right.clone());
    for (start, end, params) in [
        (
            0,
            7_321,
            LushSpaceParams {
                mix: 1.0,
                ..Default::default()
            },
        ),
        (
            7_321,
            24_001,
            LushSpaceParams {
                decay_s: 8.0,
                modulation_rate_hz: 1.3,
                modulation_depth_ms: 2.0,
                pre_delay_ms: 87.0,
                width: 0.2,
                mix: 0.7,
                pitch_amount: 1.0,
                pitch_semitones: -12.0,
                size: 0.8,
                ..Default::default()
            },
        ),
    ] {
        whole.set_params(&params);
        split.set_params(&params);
        whole.process(&mut left[start..end], &mut right[start..end]);
        let mut at = start;
        let mut iteration = 0;
        while at < end {
            split.process(&mut [], &mut []);
            let next = (at + [1, 13, 257, 7, 511, 31][iteration % 6]).min(end);
            split.process(&mut sl[at..next], &mut sr[at..next]);
            at = next;
            iteration += 1;
        }
    }
    assert_eq!(left, sl);
    assert_eq!(right, sr);
}

#[test]
fn size_moves_early_arrivals_without_changing_their_signed_gain() {
    for size in [0.0, 0.5, 1.0] {
        let params = LushSpaceParams {
            size,
            pre_delay_ms: 10.0,
            early_level: 1.0,
            modulation_depth_ms: 0.0,
            mix: 1.0,
            ..Default::default()
        };
        let (left, right) = impulse(params, 16_000.0, 0.3);
        let scale = 0.4 + 1.2 * size;
        for (signal, first_ms) in [(&left, 17.0_f32), (&right, 23.0)] {
            let first = (160.0 + (first_ms * scale * 16.0).round()) as usize;
            assert!(signal[..first].iter().all(|x| *x == 0.0));
            assert!((signal[first] - 0.55).abs() < 1e-6);
        }
    }
}

#[test]
fn zero_pitch_amount_preserves_plain_audio_and_legacy_defaults() {
    let plain = LushSpaceParams {
        mix: 1.0,
        pitch_amount: 0.0,
        ..Default::default()
    };
    assert_eq!(
        impulse(plain, 8000.0, 3.0),
        impulse(
            LushSpaceParams {
                pitch_semitones: -12.0,
                ..plain
            },
            8000.0,
            3.0
        )
    );
    let old:LushSpaceParams=serde_json::from_str(r#"{"decayS":3.5,"preDelayMs":20.0,"damping":0.45,"earlyLevel":0.35,"modulationRateHz":0.27,"modulationDepthMs":0.7,"width":1.0,"mix":0.3}"#).unwrap();
    assert_eq!(old, LushSpaceParams::default());
    assert_eq!(LushSpaceParams::index_of("mix"), Some(7));
    assert_eq!(LushSpaceParams::index_of("pitchSemitones"), Some(8));
    assert_eq!(LushSpaceParams::index_of("pitchAmount"), Some(9));
    assert_eq!(LushSpaceParams::index_of("size"), Some(10));
}

#[test]
fn invalid_audio_params_and_rates_stay_finite_and_reset_repeats() {
    for rate in [f32::NAN, f32::INFINITY, -1.0, 1.0, 384_000.0] {
        let mut effect = hall(LushSpaceParams::default(), rate);
        let params = LushSpaceParams {
            decay_s: f32::NAN,
            pre_delay_ms: f32::INFINITY,
            damping: f32::NEG_INFINITY,
            early_level: f32::MAX,
            modulation_rate_hz: f32::NAN,
            modulation_depth_ms: f32::INFINITY,
            width: f32::NAN,
            mix: f32::INFINITY,
            ..Default::default()
        };
        effect.set_params(&params);
        effect.set_tempo(f32::NAN);
        let mut left = [f32::NAN; 257];
        let mut right = [f32::INFINITY; 257];
        effect.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|x| x.is_finite()));
        effect.reset();
        left.fill(0.0);
        right.fill(0.0);
        effect.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|x| *x == 0.0));
    }
    let params = LushSpaceParams {
        mix: 1.0,
        ..Default::default()
    };
    let mut effect = hall(params, 8_000.0);
    let mut outputs = Vec::new();
    for _ in 0..2 {
        effect.reset();
        let mut left = vec![0.0; 16_000];
        let mut right = left.clone();
        left[0] = 1.0;
        effect.process(&mut left, &mut right);
        outputs.push((left, right));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn maximum_tail_expires_even_after_decay_automation() {
    let mut effect = hall(
        LushSpaceParams {
            decay_s: 8.0,
            mix: 1.0,
            ..Default::default()
        },
        1_000.0,
    );
    let lifetime = effect.tail_samples();
    let mut left = vec![0.0; lifetime + 1];
    let mut right = left.clone();
    left[0] = 1.0;
    effect.process(&mut left[..500], &mut right[..500]);
    effect.set_params(&LushSpaceParams {
        decay_s: 0.0,
        mix: 1.0,
        ..Default::default()
    });
    assert_eq!(effect.tail_samples(), lifetime);
    effect.process(&mut left[500..], &mut right[500..]);
    let mut silence_l = [0.0; 1_000];
    let mut silence_r = silence_l;
    effect.process(&mut silence_l, &mut silence_r);
    assert_eq!(silence_l, [0.0; 1_000]);
    assert_eq!(silence_r, [0.0; 1_000]);
}

fn run(command: &mut Command) {
    let output = command.output().expect("launch lush allocator probe tool");
    assert!(
        output.status.success(),
        "lush allocator probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dependency(directory: &Path, name: &str) -> PathBuf {
    let prefix = format!("lib{name}-");
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
                && path.extension().is_some_and(|ext| ext == "rlib")
        })
        .max_by_key(|path| path.metadata().unwrap().modified().unwrap())
        .unwrap_or_else(|| panic!("missing {name} dependency"))
}

#[test]
fn callback_allocator_probe() {
    // A separate executable avoids competing with the library test allocator.
    // It links the actual production library, with no duplicate DSP source.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let scratch = std::env::temp_dir().join(format!("windfall-lush-probe-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(scratch.clone());
    let library = scratch.join("libwindfall_dsp.rlib");
    let mut compile = Command::new("rustc");
    compile
        .arg(manifest.join("src/lib.rs"))
        .args([
            "--crate-name",
            "windfall_dsp",
            "--crate-type",
            "rlib",
            "--edition=2024",
            "-Copt-level=1",
            "-L",
        ])
        .arg(format!("dependency={}", deps.display()))
        .arg("-o")
        .arg(&library);
    for name in ["serde", "ts_rs", "windfall_core"] {
        compile
            .arg("--extern")
            .arg(format!("{name}={}", dependency(&deps, name).display()));
    }
    run(&mut compile);
    let executable = scratch.join(if cfg!(windows) {
        "allocator_probe.exe"
    } else {
        "allocator_probe"
    });
    run(Command::new("rustc")
        .arg(manifest.join("src/lush/allocator_probe.rs"))
        .args(["--test", "--edition=2024", "-Copt-level=1", "--extern"])
        .arg(format!("windfall_dsp={}", library.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("-o")
        .arg(&executable));
    run(Command::new(executable).arg("--nocapture"));
}
