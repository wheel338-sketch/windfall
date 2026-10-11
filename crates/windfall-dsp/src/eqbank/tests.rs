use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::blocks::biquad::{Biquad, BiquadCoeffs};
use crate::effect::Effect;
use crate::param::{ParamKind, ParamSet};

fn signal(count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| ((i * 73 % 991) as f32 / 991.0 - 0.5) * 0.4)
        .collect()
}

fn render<E: Effect + Default>(params: &E::Params, rate: f32, input: &[f32]) -> Vec<f32> {
    let mut effect = E::default();
    effect.prepare(rate, input.len());
    effect.set_params(params);
    let mut left = input.to_vec();
    let mut right = input.iter().map(|v| -*v).collect::<Vec<_>>();
    effect.process(&mut left, &mut right);
    for (l, r) in left.iter().zip(right) {
        assert!(
            (l + r).abs() < 1.0e-6,
            "stereo histories must be independent"
        );
    }
    assert_eq!(effect.latency_samples(), 0);
    left
}

fn measured_db<E: Effect + Default>(params: &E::Params, rate: f32, frequency: f32) -> f64 {
    let frames = rate as usize;
    let sine: Vec<f32> = (0..frames)
        .map(|n| (0.1 * (TAU * f64::from(frequency) * n as f64 / f64::from(rate)).sin()) as f32)
        .collect();
    let output = render::<E>(params, rate, &sine);
    let input_power: f64 = sine[frames / 2..]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum();
    let output_power: f64 = output[frames / 2..]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum();
    10.0 * (output_power / input_power).log10()
}

#[test]
fn neutral_settings_are_unity_at_every_rate() {
    let input = signal(4_097);
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        assert_eq!(
            render::<SevenBand>(&SevenBandParams::default(), rate, &input),
            input
        );
        for morph in [0.0, 0.31, 1.0] {
            let params = MorphEqParams {
                morph,
                ..Default::default()
            };
            assert_eq!(render::<MorphEq>(&params, rate, &input), input);
        }
        for routing in [FilterRouting::Serial, FilterRouting::Parallel] {
            let params = FilterBankParams {
                routing,
                ..Default::default()
            };
            assert_eq!(render::<FilterBank>(&params, rate, &input), input);
        }
    }
}

#[test]
fn seven_band_measured_boost_and_cut_at_all_centers_and_rates() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for (index, center) in SEVEN_BAND_FREQUENCIES_HZ.iter().enumerate() {
            for gain in [-9.0, 9.0] {
                let mut params = SevenBandParams::default();
                params.gains_db[index] = gain;
                let measured = measured_db::<SevenBand>(&params, rate, *center);
                assert!(
                    (measured - f64::from(gain)).abs() < 0.025,
                    "rate={rate} center={center} gain={gain} measured={measured}"
                );
                let off_center = if *center < 1_000.0 { 12_000.0 } else { 30.0 };
                assert!(measured_db::<SevenBand>(&params, rate, off_center).abs() < 0.15);
            }
        }
    }
}

#[test]
fn morph_eq_measured_boost_and_cut_at_all_rates() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for gain in [-12.0, 12.0] {
            let mut params = MorphEqParams::default();
            params.snapshot_a.bands[1] = MorphBandParams {
                frequency_hz: 1_000.0,
                gain_db: gain,
                q: 2.0,
            };
            let measured = measured_db::<MorphEq>(&params, rate, 1_000.0);
            assert!((measured - f64::from(gain)).abs() < 0.025);
            assert!(measured_db::<MorphEq>(&params, rate, 15_000.0).abs() < 0.1);
        }
    }
}

fn morph_settings() -> MorphEqParams {
    let mut params = MorphEqParams::default();
    for i in 0..4 {
        params.snapshot_a.bands[i].gain_db = [7.0, -5.0, 4.0, -3.0][i];
        params.snapshot_a.bands[i].q = 0.6 + i as f32;
        params.snapshot_b.bands[i].frequency_hz *= 1.7;
        params.snapshot_b.bands[i].gain_db = [-4.0, 9.0, -6.0, 2.0][i];
        params.snapshot_b.bands[i].q = 2.0 + i as f32;
    }
    params
}

#[test]
fn morph_endpoints_match_independent_snapshot_filters() {
    let input = signal(8_003);
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for endpoint in [0.0, 1.0] {
            let mut params = morph_settings();
            params.morph = endpoint;
            let snapshot = if endpoint == 0.0 {
                params.snapshot_a
            } else {
                params.snapshot_b
            };
            assert_eq!(params.interpolated_bands(), snapshot.bands);
            let coeffs = snapshot
                .bands
                .map(|b| BiquadCoeffs::peak(b.frequency_hz, b.q, b.gain_db, rate));
            let mut filters = [Biquad::default(); 4];
            let actual = render::<MorphEq>(&params, rate, &input);
            for (n, (x, actual)) in input.iter().zip(actual).enumerate() {
                let mut reference = f64::from(*x);
                for (filter, coeffs) in filters.iter_mut().zip(&coeffs) {
                    reference = filter.tick(coeffs, reference);
                    if (n + 1) % 64 == 0 {
                        filter.flush();
                    }
                }
                assert!((f64::from(actual) - reference).abs() < 2.0e-7);
            }
        }
    }
}

#[test]
fn morph_midpoint_interpolates_shapes_and_preserves_stored_snapshots() {
    let mut params = morph_settings();
    params.morph = 0.5;
    let saved = params;
    for (i, band) in params.interpolated_bands().iter().enumerate() {
        let a = params.snapshot_a.bands[i];
        let b = params.snapshot_b.bands[i];
        assert!(
            (band.frequency_hz / (a.frequency_hz * b.frequency_hz).sqrt() - 1.0).abs() < 2.0e-6
        );
        assert!((band.q - (a.q * b.q).sqrt()).abs() < 2.0e-6);
        assert_eq!(band.gain_db, (a.gain_db + b.gain_db) * 0.5);
    }
    let a = render::<MorphEq>(&params, 48_000.0, &signal(1_024));
    params.morph = 0.0;
    let b = render::<MorphEq>(&params, 48_000.0, &signal(1_024));
    assert!(a.iter().zip(b).any(|(a, b)| (a - b).abs() > 0.01));
    assert_eq!(params.snapshot_a, saved.snapshot_a);
    assert_eq!(params.snapshot_b, saved.snapshot_b);
}

fn filter_settings(routing: FilterRouting) -> FilterBankParams {
    let mut params = FilterBankParams {
        routing,
        ..Default::default()
    };
    params.slots[0] = FilterSlotParams {
        mode: FilterSlotMode::Lowpass,
        frequency_hz: 800.0,
        q: 0.8,
    };
    params.slots[1] = FilterSlotParams {
        mode: FilterSlotMode::Highpass,
        frequency_hz: 2_000.0,
        q: 1.2,
    };
    params.slots[2] = FilterSlotParams {
        mode: FilterSlotMode::Bandpass,
        frequency_hz: 3_200.0,
        q: 2.0,
    };
    params
}

#[test]
fn filter_bank_serial_and_parallel_match_their_audio_graphs() {
    let input = signal(8_003);
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let coeffs = [
            BiquadCoeffs::low_pass(800.0, 0.8, rate),
            BiquadCoeffs::high_pass(2_000.0, 1.2, rate),
            BiquadCoeffs::band_pass(3_200.0, 2.0, rate),
        ];
        let mut results = Vec::new();
        for routing in [FilterRouting::Serial, FilterRouting::Parallel] {
            let actual = render::<FilterBank>(&filter_settings(routing), rate, &input);
            let mut filters = [Biquad::default(); 3];
            for (n, (x, actual)) in input.iter().zip(&actual).enumerate() {
                let mut serial = f64::from(*x);
                let mut parallel = 0.0;
                for (filter, coeffs) in filters.iter_mut().zip(&coeffs) {
                    if routing == FilterRouting::Serial {
                        serial = filter.tick(coeffs, serial);
                    } else {
                        parallel += filter.tick(coeffs, f64::from(*x)) / 3.0;
                    }
                    if (n + 1) % 64 == 0 {
                        filter.flush();
                    }
                }
                let expected = if routing == FilterRouting::Serial {
                    serial
                } else {
                    parallel
                };
                assert!((f64::from(*actual) - expected).abs() < 2.0e-7);
            }
            results.push(actual);
        }
        assert!(
            results[0]
                .iter()
                .zip(&results[1])
                .any(|(s, p)| (s - p).abs() > 0.01)
        );
    }
}

#[test]
fn every_filter_slot_processes_each_mode_at_all_rates() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for routing in [FilterRouting::Serial, FilterRouting::Parallel] {
            for mode in [
                FilterSlotMode::Lowpass,
                FilterSlotMode::Bandpass,
                FilterSlotMode::Highpass,
            ] {
                // Check all eight addresses, including the final slot.
                for index in 0..8 {
                    let mut params = FilterBankParams {
                        routing,
                        ..Default::default()
                    };
                    params.slots[index].mode = mode;
                    let center = measured_db::<FilterBank>(&params, rate, 1_000.0);
                    let expected = if mode == FilterSlotMode::Bandpass {
                        0.0
                    } else {
                        -3.0103
                    };
                    assert!((center - expected).abs() < 0.025);
                    let stop = if mode == FilterSlotMode::Highpass {
                        100.0
                    } else {
                        10_000.0
                    };
                    assert!(measured_db::<FilterBank>(&params, rate, stop) < -15.0);
                }
            }
        }
        let mut boosted = FilterBankParams::default();
        boosted.slots[0] = FilterSlotParams {
            mode: FilterSlotMode::Lowpass,
            frequency_hz: 1_000.0,
            q: 2.0,
        };
        assert!((measured_db::<FilterBank>(&boosted, rate, 1_000.0) - 6.0206).abs() < 0.025);
    }
}

fn split_matches<E: Effect + Default>(start: E::Params, end: E::Params) {
    let mut a = E::default();
    let mut b = E::default();
    a.prepare(48_000.0, 8_003);
    b.prepare(48_000.0, 8_003);
    a.set_params(&start);
    b.set_params(&start);
    // Warm both histories, then move controls while processing irregular blocks.
    let mut warm_l = signal(333);
    let mut warm_r = signal(333);
    let mut other_l = warm_l.clone();
    let mut other_r = warm_r.clone();
    a.process(&mut warm_l, &mut warm_r);
    b.process(&mut other_l, &mut other_r);
    a.set_params(&end);
    b.set_params(&end);
    let mut left = signal(8_003);
    let mut right: Vec<_> = left.iter().map(|x| x * -0.3).collect();
    let mut split_l = left.clone();
    let mut split_r = right.clone();
    a.process(&mut left, &mut right);
    let lengths = [1, 17, 3, 251, 2, 64, 513, 7];
    let mut cursor = 0;
    let mut block = 0;
    while cursor < split_l.len() {
        let end = (cursor + lengths[block % lengths.len()]).min(split_l.len());
        b.process(&mut split_l[cursor..end], &mut split_r[cursor..end]);
        cursor = end;
        block += 1;
    }
    assert_eq!(left, split_l);
    assert_eq!(right, split_r);
    a.reset();
    let mut reset_l = signal(8_003);
    let mut reset_r = reset_l.clone();
    a.process(&mut reset_l, &mut reset_r);
    assert_eq!(reset_l, render::<E>(&end, 48_000.0, &signal(8_003)));
}

#[test]
fn irregular_blocks_match_during_gain_morph_mode_and_routing_transitions() {
    split_matches::<SevenBand>(
        SevenBandParams {
            gains_db: [3.0, -5.0, 8.0, -3.0, 2.0, 4.0, -6.0],
        },
        SevenBandParams {
            gains_db: [-6.0, 9.0, -4.0, 2.0, -7.0, 5.0, 3.0],
        },
    );
    let a = morph_settings();
    let b = MorphEqParams { morph: 1.0, ..a };
    split_matches::<MorphEq>(a, b);
    let mut b = filter_settings(FilterRouting::Parallel);
    b.slots[0].mode = FilterSlotMode::Bandpass;
    b.slots[1].mode = FilterSlotMode::Bypass;
    b.slots[7].mode = FilterSlotMode::Highpass;
    split_matches::<FilterBank>(filter_settings(FilterRouting::Serial), b);
    split_matches::<FilterBank>(b, FilterBankParams::default());
    split_matches::<SevenBand>(
        SevenBandParams {
            gains_db: [10.0; 7],
        },
        SevenBandParams::default(),
    );
}

fn malformed_audio<E: Effect + Default>(mut params: E::Params) {
    let mut effect = E::default();
    for rate in [f32::NAN, f32::INFINITY, -1.0, 1.0, 44_100.0, 96_000.0] {
        effect.prepare(rate, 257);
        for value in [
            f32::MAX,
            -f32::MAX,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            for index in 0..E::Params::descriptors().len() {
                params.set(index, value);
            }
            effect.set_params(&params);
            let mut left = [0.0; 257];
            let mut right = [0.0; 257];
            for (i, sample) in left.iter_mut().enumerate() {
                *sample = [
                    f32::NAN,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::MAX,
                    -f32::MAX,
                    0.1,
                ][i % 6];
            }
            right.copy_from_slice(&left);
            effect.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|v| v.is_finite()));
            left.fill(0.1);
            right.fill(0.1);
            effect.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|v| v.is_finite()));
            effect.set_tempo(value);
            effect.reset();
        }
    }
}

#[test]
fn malformed_parameters_and_audio_remain_finite() {
    malformed_audio::<SevenBand>(SevenBandParams::default());
    malformed_audio::<MorphEq>(morph_settings());
    malformed_audio::<FilterBank>(filter_settings(FilterRouting::Parallel));
    // Direct struct writes exercise sanitization separately from ParamSet::set.
    let seven = SevenBandParams {
        gains_db: [f32::NAN; 7],
    };
    assert_eq!(seven.sanitized(), SevenBandParams::default());
    let mut morph = morph_settings();
    morph.morph = f32::INFINITY;
    morph.snapshot_a.bands[0].frequency_hz = f32::NAN;
    morph.snapshot_b.bands[3].q = f32::NEG_INFINITY;
    assert!(
        morph
            .sanitized()
            .interpolated_bands()
            .iter()
            .all(|b| b.frequency_hz.is_finite() && b.q.is_finite())
    );
}

#[test]
fn neutral_returns_to_unity_after_a_live_transition() {
    fn check<E: Effect + Default>(active: E::Params) {
        let mut effect = E::default();
        effect.prepare(48_000.0, 1_001);
        effect.set_params(&active);
        let mut left = signal(1_001);
        let mut right = left.clone();
        effect.process(&mut left, &mut right);
        effect.set_params(&E::Params::default());
        left = signal(1_001);
        right = left.clone();
        effect.process(&mut left, &mut right);
        assert_eq!(&left[240..], &signal(1_001)[240..]);
        assert_eq!(&right[240..], &signal(1_001)[240..]);
        assert_eq!(effect.tail_samples(), 0);
    }
    check::<SevenBand>(SevenBandParams {
        gains_db: [12.0; 7],
    });
    check::<MorphEq>(morph_settings());
    check::<FilterBank>(filter_settings(FilterRouting::Parallel));
}

#[test]
fn active_filters_report_tails_without_latency() {
    fn check<E: Effect + Default>(params: E::Params) {
        let mut effect = E::default();
        effect.prepare(48_000.0, 64);
        effect.set_params(&params);
        assert!(effect.tail_samples() > 0);
        assert_eq!(effect.latency_samples(), 0);
    }
    check::<SevenBand>(SevenBandParams { gains_db: [6.0; 7] });
    check::<MorphEq>(morph_settings());
    check::<FilterBank>(filter_settings(FilterRouting::Serial));
}

fn metadata<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned + ts_rs::TS>(
    count: usize,
) {
    let params = P::default();
    assert_eq!(P::descriptors().len(), count);
    let mut json = serde_json::to_value(params).unwrap();
    // Default pattern storage is intentionally absent in legacy-compatible JSON.
    if P::NAME == FilterBankParams::NAME {
        json["patterns"] =
            serde_json::to_value([super::FilterPatternParams::default(); 8]).unwrap();
    }
    for (index, descriptor) in P::descriptors().iter().enumerate() {
        assert_eq!(params.get(index), Some(descriptor.default));
        assert_eq!(P::index_of(descriptor.id), Some(index));
        let mut item = &json;
        for part in descriptor.id.split('.') {
            item = if let Ok(index) = part.parse::<usize>() {
                &item[index]
            } else {
                &item[part]
            };
        }
        assert!(!item.is_null(), "missing serde path {}", descriptor.id);
        if descriptor.kind == ParamKind::Choice {
            assert_eq!(
                item.as_str(),
                Some(descriptor.choices[descriptor.default as usize].value)
            );
        } else {
            assert!((item.as_f64().unwrap() - f64::from(descriptor.default)).abs() < 1.0e-5);
        }
        let mut changed = params;
        assert!(changed.set(index, descriptor.max));
        assert_eq!(changed.get(index), Some(descriptor.max));
        changed.set(index, descriptor.min);
        assert_eq!(changed.get(index), Some(descriptor.min));
        assert_eq!(changed.sanitized(), changed);
    }
    let roundtrip: P = serde_json::from_value(json).unwrap();
    assert_eq!(roundtrip, params);
    assert!(!P::decl(&ts_rs::Config::default()).is_empty());
    assert_eq!(params.get(count), None);
    let mut params = params;
    assert!(!params.set(count, 0.0));
}

#[test]
fn parameter_metadata_serde_and_ts_agree() {
    metadata::<SevenBandParams>(7);
    metadata::<MorphBandParams>(3);
    metadata::<MorphSnapshotParams>(12);
    metadata::<MorphEqParams>(86);
    metadata::<FilterSlotParams>(3);
    metadata::<FilterBankParams>(177);
}

fn run(command: &mut Command) {
    let output = command.output().expect("launch allocator probe tool");
    assert!(
        output.status.success(),
        "allocator probe failed:\n{}\n{}",
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
        .unwrap_or_else(|| panic!("missing {name} dependency in {}", directory.display()))
}

#[test]
fn callback_allocator_probe() {
    // The library test root already owns a global allocator. Compile the real
    // production library and a separate test executable so our probe can own
    // its allocator without changing another family's test infrastructure.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let scratch =
        std::env::temp_dir().join(format!("windfall-eqbank-probe-{}", std::process::id()));
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
        .arg(manifest.join("src/eqbank/allocator_probe.rs"))
        .args(["--test", "--edition=2024", "-Copt-level=1", "--extern"])
        .arg(format!("windfall_dsp={}", library.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("-o")
        .arg(&executable));
    run(Command::new(executable).arg("--nocapture"));
}
