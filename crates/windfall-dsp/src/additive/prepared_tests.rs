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
