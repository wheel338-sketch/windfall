use super::*;
use crate::instrument_asset::{SpectralBank, SpectralFrame, SpectralPartial};
use crate::{Instrument, ParamSet};
fn sound(morph: f32, parts: &[usize]) -> Vec<f32> {
    let mut synth = SnapshotMorph::default();
    synth.prepare(48000.0, 257);
    synth.set_params(&SnapshotMorphParams {
        morph,
        ..Default::default()
    });
    synth.note_on(69, 1.0);
    let mut l = vec![0.0; 12000];
    let mut r = vec![0.0; 12000];
    let mut at = 0;
    let mut block = 0;
    while at < l.len() {
        let end = (at + parts[block % parts.len()]).min(l.len());
        synth.process(&mut l[at..end], &mut r[at..end]);
        at = end;
        block += 1;
    }
    assert_eq!(l, r);
    l
}
#[test]
fn prepared_morph_is_exact_linear_interpolation_of_measured_pcm() {
    let a = sound(0.0, &[257]);
    let b = sound(1.0, &[31]);
    for morph in [0.25, 0.5, 0.75] {
        let mixed = sound(morph, &[1, 7, 257]);
        for ((a, b), mixed) in a.iter().zip(&b).zip(mixed) {
            assert!((mixed - (a + (b - a) * morph)).abs() < 1e-5);
        }
    }
    assert_eq!(sound(0.37, &[12000]), sound(0.37, &[1, 3, 127]));
}
#[test]
fn prepared_partial_amplitude_phase_and_pan_match_independent_sine_reference() {
    let bank = SpectralBank {
        frames: vec![
            SpectralFrame {
                partials: vec![SpectralPartial {
                    ratio: 1.0,
                    level: 0.5,
                    phase: 0.25,
                    pan: -1.0
                }]
            };
            2
        ],
        frame_seconds: 0.1,
        root_hz: 440.0,
        preset_number: None,
    };
    let mut synth = SnapshotMorph::default();
    synth.set_bank(&bank);
    synth.prepare(48000.0, 64);
    synth.set_params(&SnapshotMorphParams {
        level: 1.0,
        attack_ms: 0.1,
        ..Default::default()
    });
    synth.note_on(69, 1.0);
    let mut l = [0.0; 4096];
    let mut r = [0.0; 4096];
    synth.process(&mut l, &mut r);
    let filter = 1.0 / (1.0 + (440.0f64 / 20000.0).powi(4));
    for (i, x) in l.iter().enumerate().skip(8) {
        let reference =
            (std::f64::consts::TAU * (0.25 + 440.0 * i as f64 / 48000.0)).sin() * 0.5 * filter;
        assert!((f64::from(*x) - reference).abs() < 0.0005);
    }
    assert!(r.iter().all(|x| *x == 0.0));
    let mut json = serde_json::to_value(SnapshotMorphParams::default()).unwrap();
    assert!(json.get("frames").is_none());
    json["morph"] = serde_json::json!(0.5);
    let params: SnapshotMorphParams = serde_json::from_value(json).unwrap();
    assert_eq!(params.get(0), Some(0.5));
}

fn harmonic_sound(params: HarmonicFilterParams) -> Vec<f32> {
    let mut synth = HarmonicFilter::default();
    synth.prepare(48000.0, 257);
    synth.set_params(&params);
    synth.note_on(57, 1.0);
    let mut l = vec![0.0; 24000];
    let mut r = vec![0.0; 24000];
    synth.process(&mut l, &mut r);
    l
}
fn band(signal: &[f32], hz: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, x) in signal.iter().enumerate() {
        let angle = std::f64::consts::TAU * hz * i as f64 / 48000.0;
        let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / signal.len() as f64).cos();
        re += f64::from(*x) * hann * angle.cos();
        im += f64::from(*x) * hann * angle.sin();
    }
    re * re + im * im
}
#[test]
fn subtractive_controls_shape_an_additive_harmonic_spectrum() {
    let saw = harmonic_sound(HarmonicFilterParams {
        cutoff_hz: 20000.0,
        ..Default::default()
    });
    let square = harmonic_sound(HarmonicFilterParams {
        shape: HarmonicShape::Square,
        cutoff_hz: 20000.0,
        ..Default::default()
    });
    let triangle = harmonic_sound(HarmonicFilterParams {
        shape: HarmonicShape::Triangle,
        cutoff_hz: 20000.0,
        ..Default::default()
    });
    let dark = harmonic_sound(HarmonicFilterParams {
        cutoff_hz: 300.0,
        ..Default::default()
    });
    assert!(band(&saw, 440.0) > band(&square, 440.0) * 1000.0);
    let square_ratio = band(&square, 660.0) / band(&square, 220.0);
    let triangle_ratio = band(&triangle, 660.0) / band(&triangle, 220.0);
    assert!((square_ratio - 1.0 / 9.0).abs() < 0.001);
    assert!((triangle_ratio - 1.0 / 81.0).abs() < 0.001);
    assert!(band(&dark, 2200.0) < band(&saw, 2200.0) * 0.001);
    let resonant = harmonic_sound(HarmonicFilterParams {
        cutoff_hz: 660.0,
        resonance: 1.0,
        ..Default::default()
    });
    let plain = harmonic_sound(HarmonicFilterParams {
        cutoff_hz: 660.0,
        ..Default::default()
    });
    assert!(band(&resonant, 660.0) > band(&plain, 660.0) * 10.0);
}

#[test]
fn metallic_partials_shimmer_decay_and_reset_are_measured() {
    let sound = |params: MetalVoiceParams| {
        let mut synth = MetalVoice::default();
        synth.prepare(48000.0, 257);
        synth.set_params(&params);
        synth.note_on(57, 1.0);
        let mut l = vec![0.0; 48000];
        let mut r = vec![0.0; 48000];
        synth.process(&mut l, &mut r);
        assert!(l.iter().all(|x| x.is_finite()));
        l
    };
    let params = MetalVoiceParams {
        stretch: 0.0,
        shimmer_depth: 0.0,
        decay_seconds: 12.0,
        ..Default::default()
    };
    let base = sound(params);
    assert!(band(&base[..24000], 220.0 * 2.76) > band(&base[..24000], 440.0) * 100.0);
    let shimmer = sound(MetalVoiceParams {
        shimmer_depth: 1.0,
        shimmer_hz: 8.0,
        ..params
    });
    assert!(
        base.iter()
            .zip(shimmer)
            .map(|(a, b)| (*a - b).abs())
            .sum::<f32>()
            > 10.0
    );
    let short = sound(MetalVoiceParams {
        decay_seconds: 0.05,
        ..params
    });
    assert!(short[12000..].iter().all(|x| *x == 0.0));
    assert_eq!(base, sound(params));
}

#[test]
fn numbered_patches_cover_full_u32_domain_and_reproduce_measured_pcm() {
    let sound = |number| {
        let bank = SpectralBank::numbered(number);
        assert_eq!(bank.check(), Ok(()));
        assert_eq!(bank.preset_number, Some(number));
        let mut synth = NumberVoice::default();
        synth.set_bank(&bank);
        synth.prepare(48000.0, 257);
        synth.note_on(60, 1.0);
        let mut l = vec![0.0; 12000];
        let mut r = vec![0.0; 12000];
        synth.process(&mut l, &mut r);
        assert!(l.iter().all(|x| x.is_finite()));
        l
    };
    let base = sound(0);
    assert_eq!(base, sound(0));
    for number in [1, 16777216, 2147483648, u32::MAX] {
        let other = sound(number);
        assert_eq!(other, sound(number));
        assert!(
            base.iter()
                .zip(other)
                .map(|(a, b)| (*a - b).abs())
                .sum::<f32>()
                > 10.0
        );
    }
}

#[test]
fn resynthesis_static_frames_reconstruct_phase_and_scan_changes_measured_audio() {
    let bank = SpectralBank {
        frames: (0..3)
            .map(|i| SpectralFrame {
                partials: vec![SpectralPartial {
                    ratio: 1.0,
                    level: 0.5,
                    phase: i as f32 * 0.125,
                    pan: 0.0,
                }],
            })
            .collect(),
        frame_seconds: 0.05,
        root_hz: 440.0,
        preset_number: None,
    };
    let sound = |position, scan| {
        let mut synth = SpectralResynth::default();
        synth.set_bank(&bank);
        synth.prepare(48000.0, 257);
        synth.set_params(&SpectralResynthParams {
            position,
            scan,
            level: 1.0,
            attack_ms: 0.1,
            ..Default::default()
        });
        synth.note_on(69, 1.0);
        let mut l = vec![0.0; 12000];
        let mut r = vec![0.0; 12000];
        synth.process(&mut l, &mut r);
        l
    };
    for (position, phase) in [(0.0, 0.0), (0.5, 0.125), (1.0, 0.25)] {
        let pcm = sound(position, false);
        for (i, x) in pcm.iter().enumerate().skip(10) {
            let reference =
                (std::f64::consts::TAU * (phase + 440.0 * i as f64 / 48000.0)).sin() * 0.25;
            assert!((f64::from(*x) - reference).abs() < 0.0005);
        }
    }
    let frozen = sound(0.0, false);
    let scan = sound(0.0, true);
    assert!(
        frozen
            .iter()
            .zip(scan)
            .map(|(a, b)| (*a - b).abs())
            .sum::<f32>()
            > 100.0
    );
}

#[test]
fn image_scan_interpolates_brightness_pitch_and_stereo_with_partition_identity() {
    let bank = SpectralBank {
        frames: vec![
            SpectralFrame {
                partials: vec![SpectralPartial {
                    ratio: 1.0,
                    level: 0.5,
                    phase: 0.0,
                    pan: -1.0,
                }],
            },
            SpectralFrame {
                partials: vec![SpectralPartial {
                    ratio: 2.0,
                    level: 0.25,
                    phase: 0.0,
                    pan: 1.0,
                }],
            },
        ],
        frame_seconds: 0.05,
        root_hz: 440.0,
        preset_number: None,
    };
    let sound = |position, scan, width, block| {
        let mut synth = ColorScan::default();
        synth.set_bank(&bank);
        synth.prepare(48000.0, 257);
        synth.set_params(&ColorScanParams {
            position,
            scan,
            stereo_width: width,
            attack_ms: 0.1,
            level: 1.0,
            ..Default::default()
        });
        synth.note_on(69, 1.0);
        let mut l = vec![0.0; 12000];
        let mut r = vec![0.0; 12000];
        for (l, r) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
            synth.process(l, r);
        }
        (l, r)
    };
    let (left, right) = sound(0.0, false, 1.0, 257);
    assert!(right.iter().all(|x| *x == 0.0));
    assert!(band(&left, 440.0) > band(&left, 880.0) * 100.0);
    let (left, right) = sound(1.0, false, 1.0, 31);
    assert!(left.iter().all(|x| *x == 0.0));
    assert!(band(&right, 880.0) > band(&right, 440.0) * 100.0);
    let (left, right) = sound(0.5, false, 1.0, 17);
    assert_eq!(left, right);
    assert!(band(&left, 660.0) > band(&left, 440.0) * 100.0);
    let (left, right) = sound(0.0, false, 0.0, 257);
    assert_eq!(left, right);
    assert_eq!(sound(0.0, true, 1.0, 12000), sound(0.0, true, 1.0, 1));
}

#[test]
fn prepared_voice_stealing_preserves_endpoint_and_stop_reaches_exact_silence() {
    let mut synth = SnapshotMorph::default();
    synth.prepare(48000.0, 257);
    synth.set_params(&SnapshotMorphParams {
        polyphony: 1,
        attack_ms: 0.1,
        ..Default::default()
    });
    synth.note_on(69, 1.0);
    let (mut l, mut r) = ([0.0; 257], [0.0; 257]);
    synth.process(&mut l, &mut r);
    let before = l[256];
    synth.note_on(57, 1.0);
    let (mut first, mut right) = ([0.0], [0.0]);
    synth.process(&mut first, &mut right);
    assert!((first[0] - before).abs() < 0.002);
    assert_eq!(synth.active_voices(), 1);
    synth.all_notes_off();
    synth.process(&mut l, &mut r);
    synth.process(&mut l, &mut r);
    assert_eq!(synth.active_voices(), 0);
    assert!(l.iter().chain(&r).all(|x| *x == 0.0));
}

#[test]
fn snapshot_phase_interpolation_takes_the_short_path_across_cycle_zero() {
    let bank = SpectralBank {
        frames: [0.99, 0.01]
            .into_iter()
            .map(|phase| SpectralFrame {
                partials: vec![SpectralPartial {
                    ratio: 1.0,
                    level: 1.0,
                    phase,
                    pan: 0.0,
                }],
            })
            .collect(),
        frame_seconds: 0.05,
        root_hz: 440.0,
        preset_number: None,
    };
    let mut synth = SnapshotMorph::default();
    synth.set_bank(&bank);
    synth.prepare(48000.0, 257);
    synth.set_params(&SnapshotMorphParams {
        morph: 0.5,
        level: 1.0,
        attack_ms: 0.1,
        ..Default::default()
    });
    synth.note_on(69, 1.0);
    let (mut l, mut r) = ([0.0; 4096], [0.0; 4096]);
    synth.process(&mut l, &mut r);
    for (i, x) in l.iter().enumerate().skip(10) {
        let reference = (std::f64::consts::TAU * 440.0 * i as f64 / 48000.0).sin() * 0.5;
        assert!((f64::from(*x) - reference).abs() < 0.0005);
    }
}
