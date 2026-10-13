use super::*;
use ts_rs::TS;

fn render(params: SpeechVoiceParams, key: u8, velocity: f32, count: usize) -> Vec<f32> {
    let mut voice = SpeechVoice::default();
    voice.set_params(&params);
    voice.note_on(key, velocity);
    let mut left = vec![0.0; count];
    let mut right = vec![0.0; count];
    voice.process(&mut left, &mut right);
    assert_eq!(left, right);
    assert!(
        left.iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
    );
    left
}

fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|sample| sample * sample).sum::<f32>() / signal.len() as f32).sqrt()
}

#[test]
fn speech_codes_have_distinct_audible_colors() {
    let codes = [
        Phoneme::Ah,
        Phoneme::Ee,
        Phoneme::Oo,
        Phoneme::Eh,
        Phoneme::Oh,
        Phoneme::NoiseBurst,
        Phoneme::Nasal,
        Phoneme::Silence,
    ];
    let signals: Vec<_> = codes
        .iter()
        .map(|code| {
            render(
                SpeechVoiceParams {
                    phrase: PhonemeBuffer::from_slice(&[*code]),
                    rate: 0.5,
                    ..Default::default()
                },
                48,
                1.0,
                12_000,
            )
        })
        .collect();
    for (index, signal) in signals.iter().enumerate() {
        if codes[index] == Phoneme::Silence {
            assert!(signal.iter().all(|sample| *sample == 0.0));
        } else {
            assert!(rms(signal) > 0.005, "inaudible {:?}", codes[index]);
        }
    }
    // Compare coarse spectral energy distributions, rather than sample inequality.
    let spectra: Vec<Vec<f64>> = signals
        .iter()
        .map(|signal| {
            (1..=36)
                .map(|bin| {
                    let frequency = bin as f64 * 100.0;
                    let (mut real, mut imaginary) = (0.0, 0.0);
                    for (index, sample) in signal[1000..9000].iter().enumerate() {
                        let phase = std::f64::consts::TAU * frequency * index as f64 / 48_000.0;
                        real += *sample as f64 * phase.cos();
                        imaginary += *sample as f64 * phase.sin();
                    }
                    real * real + imaginary * imaginary
                })
                .collect()
        })
        .collect();
    for first in 0..7 {
        for second in first + 1..7 {
            let a = &spectra[first];
            let b = &spectra[second];
            let sum_a = a.iter().sum::<f64>();
            let sum_b = b.iter().sum::<f64>();
            let distance: f64 = a
                .iter()
                .zip(b)
                .map(|(a, b)| (a / sum_a - b / sum_b).abs())
                .sum();
            assert!(
                distance > 0.08,
                "similar colors {:?}/{:?}: {distance}",
                codes[first],
                codes[second]
            );
        }
    }
}

#[test]
fn speech_sequence_and_control_changes_are_block_independent() {
    fn run(split: bool) -> Vec<f32> {
        let mut voice = SpeechVoice::default();
        let mut params = SpeechVoiceParams {
            phrase: PhonemeBuffer::from_slice(&[
                Phoneme::Ah,
                Phoneme::NoiseBurst,
                Phoneme::Silence,
                Phoneme::Ee,
                Phoneme::Nasal,
                Phoneme::Oo,
            ]),
            rate: 30.0,
            release_ms: 50.0,
            ..Default::default()
        };
        voice.set_params(&params);
        voice.note_on(53, 0.7);
        let mut output = vec![0.0; 20_000];
        let mut right = vec![0.0; 20_000];
        for (start, end) in [(0, 3000), (3000, 7000), (7000, 20_000)] {
            if start == 3000 {
                params.pitch = 7.0;
                params.rate = 18.0;
                voice.set_params(&params);
            }
            if start == 7000 {
                voice.note_off(53);
            }
            let mut position = start;
            while position < end {
                let count = if split {
                    [1, 7, 113, 64][position % 4]
                } else {
                    end - position
                };
                let next = (position + count).min(end);
                voice.process(&mut output[position..next], &mut right[position..next]);
                position = next;
            }
        }
        assert_eq!(output, right);
        assert_eq!(voice.active_voices(), 0);
        assert!(output[9500..].iter().all(|sample| *sample == 0.0));
        output
    }
    assert_eq!(run(false), run(true));
}

#[test]
fn speech_params_describe_and_sanitize_fixed_storage() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<SpeechVoiceParams>();
    assert_copy::<PhonemeBuffer>();
    assert_eq!(SpeechVoiceParams::NAME, "Speech Voice");
    let default = SpeechVoiceParams::default();
    let json = serde_json::to_value(default).unwrap();
    assert!(json.get("releaseMs").is_some());
    assert_eq!(
        serde_json::from_value::<SpeechVoiceParams>(json).unwrap(),
        default
    );
    assert!(!SpeechVoiceParams::decl(&ts_rs::Config::default()).is_empty());
    for (index, info) in SpeechVoiceParams::descriptors().iter().enumerate() {
        assert_eq!(default.get(index), Some(info.default));
        let mut changed = default;
        assert!(changed.set(index, info.max + 100.0));
        assert_eq!(changed.get(index), Some(info.max));
        assert!(changed.set(index, info.min - 100.0));
        assert_eq!(changed.get(index), Some(info.min));
    }
    let invalid = SpeechVoiceParams {
        rate: f32::NAN,
        pitch: f32::INFINITY,
        release_ms: f32::NEG_INFINITY,
        level: f32::NAN,
        phrase: PhonemeBuffer {
            length: 255,
            ..Default::default()
        },
    };
    let clean = invalid.sanitized();
    assert_eq!(clean.rate, default.rate);
    assert_eq!(clean.pitch, default.pitch);
    assert_eq!(clean.release_ms, default.release_ms);
    assert_eq!(clean.level, default.level);
    assert_eq!(clean.phrase.length, 32);
    assert_eq!(PhonemeBuffer::from_slice(&[Phoneme::Ah; 40]).length, 32);
}

#[test]
fn speech_release_retrigger_rate_pitch_and_velocity() {
    let params = SpeechVoiceParams {
        phrase: PhonemeBuffer::from_slice(&[Phoneme::Ah]),
        rate: 0.5,
        release_ms: 20.0,
        ..Default::default()
    };
    let mut voice = SpeechVoice::default();
    voice.set_params(&params);
    voice.note_on(60, 1.0);
    let mut left = [0.0; 1200];
    let mut right = [0.0; 1200];
    voice.process(&mut left, &mut right);
    let first = left;
    voice.note_off(61);
    assert_eq!(voice.active_voices(), 1);
    voice.note_on(60, 1.0);
    voice.process(&mut left, &mut right);
    assert_eq!(first, left);
    voice.note_off(60);
    voice.process(&mut left, &mut right);
    assert_eq!(voice.active_voices(), 0);
    assert!(left[960..].iter().all(|sample| *sample == 0.0));
    voice.note_on(60, 1.0);
    voice.process(&mut left, &mut right);
    voice.all_notes_off();
    voice.process(&mut left, &mut right);
    assert!(left[240..].iter().all(|sample| *sample == 0.0));
    let low = render(params, 48, 1.0, 8000);
    let high = render(
        SpeechVoiceParams {
            pitch: 12.0,
            ..params
        },
        48,
        1.0,
        8000,
    );
    assert_eq!(high, render(params, 60, 1.0, 8000));
    assert_ne!(low, high);
    let quiet = render(params, 48, 0.25, 8000);
    assert!(rms(&quiet) < rms(&low) * 0.35);
    voice.set_params(&SpeechVoiceParams {
        rate: 30.0,
        release_ms: 5.0,
        ..params
    });
    voice.note_on(60, 1.0);
    for _ in 0..2 {
        voice.process(&mut left, &mut right);
    }
    assert_eq!(voice.active_voices(), 0);
    voice.set_params(&SpeechVoiceParams {
        phrase: PhonemeBuffer::from_slice(&[]),
        ..params
    });
    voice.note_on(60, 1.0);
    assert_eq!(voice.active_voices(), 0);
}

#[test]
fn speech_extremes_stay_finite_and_reset_is_silent() {
    for sample_rate in [f32::NAN, -1.0, 8_000.0, 44_100.0, 192_000.0, f32::MAX] {
        let mut voice = SpeechVoice::default();
        voice.prepare(sample_rate, 64);
        let params = SpeechVoiceParams {
            phrase: PhonemeBuffer {
                codes: [Phoneme::Nasal; 32],
                length: 255,
            },
            rate: f32::NAN,
            pitch: f32::MAX,
            release_ms: f32::INFINITY,
            level: f32::MAX,
        };
        voice.set_params(&params);
        let mut left = [0.0; 256];
        let mut right = [0.0; 256];
        for key in [0, 127, 255] {
            voice.note_on(key, f32::MAX);
            for _ in 0..80 {
                voice.process(&mut left, &mut right);
                assert!(
                    left.iter()
                        .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
                );
            }
        }
        voice.reset();
        voice.note_on(60, f32::NAN);
        voice.process(&mut left, &mut right);
        assert_eq!(left, [0.0; 256]);
        assert_eq!(right, left);
        voice.process(&mut [], &mut []);
    }
}

#[test]
fn speech_callbacks_have_zero_alloc_realloc_free() {
    use std::fs;
    use std::process::Command;
    // Compile a probe against the actual sources and actual Instrument trait.
    // A separate process avoids conflicting with the library's global allocator.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let temp = std::env::temp_dir().join(format!("windfall-speech-probe-{}", std::process::id()));
    fs::create_dir_all(&temp).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp.clone());
    let instrument = fs::read_to_string(source.join("instrument.rs")).unwrap();
    let start = instrument.find("pub trait Instrument: Send {").unwrap();
    let end = instrument[start..]
        .find("\n}\n")
        .or_else(|| instrument[start..].find("\n}\r\n"))
        .unwrap();
    let trait_source = &instrument[start..start + end + 2];
    let path = |relative: &str| format!("{:?}", source.join(relative));
    let root = format!(
        "#![allow(dead_code)]\nextern crate serde; extern crate ts_rs; extern crate windfall_core;\n\
         #[path = {}] mod param;\nmod blocks {{ #[path = {}] pub mod math; }}\n\
           #[path = {}] mod note_expression;\n\
           #[path = {}] mod note_pan;\n\
         pub use note_expression::{{NoteExpression, NoteInstanceId}};\n\
         mod instrument {{ use crate::param::ParamSet; use crate::{{NoteExpression, NoteInstanceId}};\n{}\n}}\n\
         #[path = {}] mod speech;\n#[path = {}] mod allocator_probe;\n\
         fn main() {{ allocator_probe::run(); }}\n",
        path("param.rs"),
        path("blocks/math.rs"),
        path("note_expression.rs"),
        path("note_pan.rs"),
        trait_source,
        path("speech/mod.rs"),
        path("speech/allocator_probe.rs")
    );
    fs::write(temp.join("probe.rs"), root).unwrap();
    let executable = temp.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    command
        .arg("--edition=2024")
        .arg(temp.join("probe.rs"))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("-o")
        .arg(&executable);
    for name in ["serde", "ts_rs", "windfall_core"] {
        let prefix = format!("lib{name}-");
        let dependency = fs::read_dir(&deps)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with(&prefix)
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "rlib")
            })
            .max_by_key(|entry| entry.metadata().unwrap().modified().unwrap())
            .unwrap();
        command
            .arg("--extern")
            .arg(format!("{name}={}", dependency.path().display()));
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "allocator probe compile: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(executable).output().unwrap();
    assert!(
        result.status.success(),
        "allocator probe: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}
