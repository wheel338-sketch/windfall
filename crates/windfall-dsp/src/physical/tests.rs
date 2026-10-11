use std::f64::consts::TAU;

use serde::{de::DeserializeOwned, Serialize};

use super::*;
use crate::blocks::math::key_to_hz;

fn render<I: Instrument>(instrument: &mut I, samples: usize, partitions: &[usize]) -> Vec<f32> {
    let mut left = vec![0.0; samples];
    let mut right = vec![0.0; samples];
    let mut offset = 0;
    let mut partition = 0;
    while offset < samples {
        let end = (offset + partitions[partition % partitions.len()]).min(samples);
        instrument.process(&mut left[offset..end], &mut right[offset..end]);
        offset = end;
        partition += 1;
    }
    assert_eq!(left, right);
    left
}

fn energy(samples: &[f32]) -> f64 {
    samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / samples.len() as f64
}

fn spectral_power(samples: &[f32], frequency: f64, sample_rate: f64) -> f64 {
    let coefficient = 2.0 * (TAU * frequency / sample_rate).cos();
    let mut previous = 0.0;
    let mut older = 0.0;
    for (i, &sample) in samples.iter().enumerate() {
        let hann = 0.5 - 0.5 * (TAU * i as f64 / (samples.len() - 1) as f64).cos();
        let current = f64::from(sample) * hann + coefficient * previous - older;
        older = previous;
        previous = current;
    }
    (previous * previous + older * older - coefficient * previous * older).max(0.0)
}

fn assert_pitch<I: Instrument + Default>(params: I::Params) {
    for sample_rate in [44_100.0, 48_000.0] {
        for key in [28, 45, 69, 84] {
            let mut instrument = I::default();
            instrument.prepare(sample_rate, 257);
            instrument.set_params(&params);
            instrument.note_on(key, 0.9);
            let samples = render(&mut instrument, (sample_rate * 1.2) as usize, &[257]);
            let window = &samples[(sample_rate * 0.08) as usize..];
            assert!(energy(window) > 1.0e-12, "pitch test must contain sound");
            let frequency = f64::from(key_to_hz(key as f32));
            let best = (-40..=40)
                .max_by(|a, b| {
                    let power = |cents: i32| {
                        spectral_power(
                            window,
                            frequency * 2.0_f64.powf(f64::from(cents) / 1200.0),
                            f64::from(sample_rate),
                        )
                    };
                    power(*a).total_cmp(&power(*b))
                })
                .unwrap();
            assert!(
                best.abs() <= 20,
                "key {key}, rate {sample_rate}: {best} cents"
            );
        }
    }
}

#[test]
fn physical_pitch_tracks_midi_after_attack() {
    assert_pitch::<Pluck>(PluckParams::default());
    assert_pitch::<FingerBass>(FingerBassParams::default());
    assert_pitch::<AcousticString>(AcousticStringParams::default());
}

#[test]
fn physical_pitch_survives_brightness_and_stiffness_extremes() {
    for brightness in [0.0, 1.0] {
        assert_pitch::<Pluck>(PluckParams {
            brightness,
            decay_seconds: 8.0,
            ..Default::default()
        });
        for stiffness in [0.0, 1.0] {
            assert_pitch::<AcousticString>(AcousticStringParams {
                brightness,
                stiffness,
                sympathetic: 0.0,
                decay_seconds: 12.0,
                ..Default::default()
            });
        }
    }
}

fn assert_decay<I: Instrument + Default>(mut params: I::Params) {
    params.set(0, 0.25);
    let mut instrument = I::default();
    instrument.prepare(48_000.0, 256);
    instrument.set_params(&params);
    instrument.note_on(40, 1.0);
    let samples = render(&mut instrument, 96_000, &[256]);
    let attack = energy(&samples[480..4800]);
    let tail = energy(&samples[72_000..]);
    assert!(attack > 1.0e-6);
    assert!(tail < attack * 1.0e-6, "attack {attack}, tail {tail}");
    assert_eq!(instrument.active_voices(), 0);
    assert!(samples[90_000..].iter().all(|&x| x == 0.0));
}

#[test]
fn physical_decay_reaches_exact_silence() {
    assert_decay::<Pluck>(PluckParams::default());
    assert_decay::<FingerBass>(FingerBassParams::default());
    assert_decay::<AcousticString>(AcousticStringParams::default());
}

fn assert_extremes<I: Instrument + Default>() {
    let descriptors = I::Params::descriptors();
    let mut instrument = I::default();
    instrument.prepare(8000.0, 257);
    let mut left = [0.0; 257];
    let mut right = [0.0; 257];
    for mask in 0..(1 << descriptors.len()) {
        instrument.reset();
        let mut params = I::Params::default();
        for (index, info) in descriptors.iter().enumerate() {
            params.set(
                index,
                if mask & (1 << index) == 0 {
                    info.min
                } else {
                    info.max
                },
            );
        }
        instrument.set_params(&params);
        for key in [0, 12, 28, 45, 69, 96, 110, 127] {
            instrument.note_on(key, 1.0);
        }
        for _ in 0..32 {
            instrument.process(&mut left, &mut right);
            assert!(left
                .iter()
                .chain(&right)
                .all(|x| x.is_finite() && x.abs() < 8.0));
            assert!(instrument.active_voices() <= MAX_POLYPHONY);
        }
    }
    // Rapid automation while the longest-decay, maximum-brightness strings ring.
    instrument.prepare(48_000.0, 257);
    for key in [0, 12, 28, 45, 69, 96, 110, 127] {
        instrument.note_on(key, 1.0);
    }
    for step in 0..400 {
        let mut params = I::Params::default();
        for (i, info) in descriptors.iter().enumerate() {
            params.set(i, if step % 2 == 0 { info.max } else { info.min });
        }
        instrument.set_params(&params);
        instrument.process(&mut left, &mut right);
        assert!(left
            .iter()
            .chain(&right)
            .all(|x| x.is_finite() && x.abs() < 8.0));
    }
}

#[test]
fn physical_feedback_is_finite_at_every_control_corner_and_during_automation() {
    assert_extremes::<Pluck>();
    assert_extremes::<FingerBass>();
    assert_extremes::<AcousticString>();
}

#[test]
fn physical_finger_bass_body_changes_harmonic_balance() {
    let render_body = |body| {
        let mut bass = FingerBass::default();
        bass.prepare(48_000.0, 256);
        bass.set_params(&FingerBassParams {
            body,
            damping: 0.0,
            tone: 0.5,
            ..Default::default()
        });
        bass.note_on(33, 1.0);
        render(&mut bass, 24_000, &[256])
    };
    let bare = render_body(0.0);
    let body = render_body(1.0);
    let frequency = f64::from(key_to_hz(33.0));
    let ratio = |samples: &[f32]| {
        spectral_power(&samples[1000..], frequency * 2.0, 48_000.0)
            / spectral_power(&samples[1000..], frequency, 48_000.0)
    };
    let change_db = 10.0 * (ratio(&body) / ratio(&bare)).log10();
    assert!(
        change_db.abs() > 1.5,
        "body harmonic-balance change: {change_db} dB"
    );
}

#[test]
fn physical_stiffness_changes_upper_partial_spectrum_with_tuned_fundamental() {
    let render_stiffness = |stiffness| {
        let mut string = AcousticString::default();
        string.prepare(48_000.0, 256);
        string.set_params(&AcousticStringParams {
            stiffness,
            brightness: 1.0,
            sympathetic: 0.0,
            ..Default::default()
        });
        string.note_on(69, 1.0);
        render(&mut string, 24_000, &[256])
    };
    let flexible = render_stiffness(0.0);
    let stiff = render_stiffness(1.0);
    let ratio = |samples: &[f32]| {
        spectral_power(&samples[1000..], 440.0 * 7.0, 48_000.0)
            / spectral_power(&samples[1000..], 440.0, 48_000.0)
    };
    let change_db = 10.0 * (ratio(&stiff) / ratio(&flexible)).log10();
    assert!(
        change_db.abs() > 3.0,
        "stiffness spectral change: {change_db} dB"
    );
}

#[test]
fn physical_second_string_changes_the_render() {
    let mut string = AcousticString::default();
    string.prepare(48_000.0, 256);
    string.set_params(&AcousticStringParams {
        sympathetic: 0.0,
        ..Default::default()
    });
    string.note_on(60, 1.0);
    let single = render(&mut string, 48_000, &[256]);
    string.reset();
    string.set_params(&AcousticStringParams {
        sympathetic: 1.0,
        ..Default::default()
    });
    string.note_on(60, 1.0);
    let dual = render(&mut string, 48_000, &[256]);
    let difference: Vec<_> = single.iter().zip(&dual).map(|(a, b)| a - b).collect();
    assert!(energy(&difference) > energy(&single) * 0.005);
}

fn event_render<I: Instrument + Default>(partitions: &[usize]) -> Vec<f32> {
    let mut instrument = I::default();
    instrument.prepare(48_000.0, 513);
    instrument.note_on(40, 0.8);
    let mut samples = render(&mut instrument, 1031, partitions);
    let mut params = I::Params::default();
    for (i, info) in I::Params::descriptors().iter().enumerate() {
        params.set(i, info.max);
    }
    instrument.set_params(&params);
    instrument.note_on(64, 0.6);
    samples.extend(render(&mut instrument, 5003, partitions));
    instrument.note_off(40);
    samples.extend(render(&mut instrument, 4079, partitions));
    instrument.all_notes_off();
    samples.extend(render(&mut instrument, 1001, partitions));
    assert_eq!(instrument.active_voices(), 0);
    samples
}

#[test]
fn physical_partition_invariance_includes_automation_and_note_events() {
    macro_rules! check {
        ($ty:ty) => {
            let whole = event_render::<$ty>(&[20_000]);
            assert_eq!(whole, event_render::<$ty>(&[1]));
            assert_eq!(whole, event_render::<$ty>(&[3, 257, 1, 513, 19]));
        };
    }
    check!(Pluck);
    check!(FingerBass);
    check!(AcousticString);
}

fn assert_lifecycle<I: Instrument + Default>() {
    let mut instrument = I::default();
    instrument.note_on(60, 1.0);
    assert_eq!(instrument.active_voices(), 0);
    instrument.prepare(f32::NAN, 256);
    for key in 20..40 {
        instrument.note_on(key, 1.0);
    }
    assert_eq!(instrument.active_voices(), MAX_POLYPHONY);
    instrument.reset();
    instrument.note_on(40, 1.0);
    let first = render(&mut instrument, 2048, &[256]);
    instrument.reset();
    instrument.note_on(40, 1.0);
    assert_eq!(first, render(&mut instrument, 2048, &[256]));
    instrument.note_on(40, 0.0);
    render(&mut instrument, 4000, &[256]);
    assert_eq!(instrument.active_voices(), 0);
    instrument.prepare(192_000.0, 256);
    instrument.note_on(0, 1.0);
    assert!(render(&mut instrument, 8000, &[256])
        .iter()
        .all(|x| x.is_finite()));
    instrument.reset();
    assert!(render(&mut instrument, 64, &[1]).iter().all(|&x| x == 0.0));
}

#[test]
fn physical_polyphony_reset_repeatability_and_unprepared_silence() {
    assert_lifecycle::<Pluck>();
    assert_lifecycle::<FingerBass>();
    assert_lifecycle::<AcousticString>();
}

fn assert_params<P: ParamSet + Serialize + DeserializeOwned>() {
    let defaults = P::default();
    assert_eq!(serde_json::from_str::<P>("{}").unwrap(), defaults);
    let json = serde_json::to_value(defaults).unwrap();
    assert_eq!(serde_json::from_value::<P>(json.clone()).unwrap(), defaults);
    for (i, info) in P::descriptors().iter().enumerate() {
        assert_eq!(defaults.get(i), Some(info.default));
        assert_eq!(json[info.id].as_f64().unwrap() as f32, info.default);
        assert_eq!(P::index_of(info.id), Some(i));
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut params = defaults;
            assert!(params.set(i, invalid));
            assert_eq!(params.get(i), Some(info.default));
        }
        let mut params = defaults;
        params.set(i, info.max * 2.0 + 100.0);
        assert_eq!(params.get(i), Some(info.max));
        params.set(i, -100.0);
        assert_eq!(params.get(i), Some(info.min));
    }
    let mut params = defaults;
    assert!(!params.set(P::descriptors().len(), 1.0));
    assert_eq!(params.get(P::descriptors().len()), None);
}

#[test]
fn physical_parameter_descriptors_serialization_and_sanitization() {
    assert_params::<PluckParams>();
    assert_params::<FingerBassParams>();
    assert_params::<AcousticStringParams>();
    assert_eq!(
        PluckParams {
            brightness: f32::NAN,
            ..Default::default()
        }
        .sanitized(),
        PluckParams::default()
    );
    assert_eq!(
        FingerBassParams {
            damping: f32::INFINITY,
            ..Default::default()
        }
        .sanitized(),
        FingerBassParams::default()
    );
    assert_eq!(
        AcousticStringParams {
            stiffness: f32::NEG_INFINITY,
            ..Default::default()
        }
        .sanitized(),
        AcousticStringParams::default()
    );
    assert_eq!(PluckParams::NAME, "Pluck");
    assert_eq!(FingerBassParams::NAME, "Finger Bass");
    assert_eq!(AcousticStringParams::NAME, "Acoustic String");
}

#[test]
fn finger_bass_is_monophonic_and_old_key_release_cannot_release_new_string() {
    let mut bass = FingerBass::default();
    bass.prepare(48_000.0, 257);
    bass.note_on(28, 1.0);
    let first = render(&mut bass, 2400, &[17, 257]);
    for key in 29..60 {
        bass.note_on(key, 1.0);
        assert_eq!(bass.active_voices(), 1);
    }
    bass.note_off(28);
    let second = render(&mut bass, 9600, &[31]);
    assert_eq!(bass.active_voices(), 1);
    assert!(energy(&second[480..]) > 1e-9);
    assert!((second[0] - first[first.len() - 1]).abs() < 1e-5);
    bass.all_notes_off();
    render(&mut bass, 480, &[7]);
    assert_eq!(bass.active_voices(), 0);
}
