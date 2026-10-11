use super::*;
use crate::effect::Effect;
use crate::param::ParamSet;

const SR: f32 = 48_000.0;
fn sine(hz: f32, count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin())
        .collect()
}
fn crossing_rate(signal: &[f32]) -> f32 {
    let crossings = signal
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    crossings as f32 * SR / signal.len() as f32
}
fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|v| v * v).sum::<f32>() / signal.len() as f32).sqrt()
}
fn render<E: Effect + Default>(
    params: E::Params,
    input: &[f32],
    chunks: &[usize],
) -> (Vec<f32>, Vec<f32>) {
    let mut effect = E::default();
    effect.prepare(SR, 4096);
    effect.set_params(&params);
    let mut l = input.to_vec();
    let mut r: Vec<f32> = input.iter().map(|v| -0.25 * v).collect();
    let mut at = 0;
    let mut index = 0;
    while at < l.len() {
        let end = (at + chunks[index % chunks.len()]).min(l.len());
        effect.process(&mut l[at..end], &mut r[at..end]);
        at = end;
        index += 1;
    }
    (l, r)
}

fn render_ir(
    frames: &[[f32; 2]],
    params: ConvolverParams,
    input: &[f32],
    chunks: &[usize],
) -> (Vec<f32>, Vec<f32>) {
    let mut effect = Convolver::with_impulse(std::sync::Arc::new(
        PreparedImpulse::prepare(frames).unwrap(),
    ));
    effect.prepare(SR, 4096);
    effect.set_params(&params);
    let mut l = input.to_vec();
    let mut r: Vec<f32> = input.iter().map(|v| -0.25 * v).collect();
    let mut at = 0;
    let mut index = 0;
    while at < l.len() {
        let end = (at + chunks[index % chunks.len()]).min(l.len());
        effect.process(&mut l[at..end], &mut r[at..end]);
        at = end;
        index += 1;
    }
    (l, r)
}
#[test]
fn empty_and_explicit_identity_convolvers_preserve_an_impulse_at_reported_latency() {
    let mut input = vec![0.0; 1024];
    input[0] = 1.0;
    for (l, r) in [
        render::<Convolver>(ConvolverParams::default(), &input, &[1, 7, 125]),
        render_ir(
            &[[1.0; 2]],
            ConvolverParams::default(),
            &input,
            &[1, 7, 125],
        ),
    ] {
        for i in 0..l.len() {
            let expected = if i == CONVOLUTION_PARTITION { 1.0 } else { 0.0 };
            assert!((l[i] - expected).abs() < 2e-6);
            assert!((r[i] + 0.25 * expected).abs() < 2e-6);
        }
    }
}
#[test]
fn convolution_matches_a_direct_stereo_fir_beyond_legacy_capacity() {
    let taps = [
        (0, 0.8),
        (127, -0.2),
        (128, 0.15),
        (511, 0.1),
        (1025, -0.1),
        (8191, 0.05),
    ];
    let mut frames = vec![[0.0; 2]; 8192];
    for (i, value) in taps {
        frames[i] = [value, value * 0.5];
    }
    let input: Vec<f32> = (0..10000)
        .map(|i| (((i * 37 + 11) % 251) as f32 - 125.0) / 250.0)
        .collect();
    let (l, r) = render_ir(
        &frames,
        ConvolverParams::default(),
        &input,
        &[71, 1, 320, 17],
    );
    for n in 0..l.len() {
        let expected: f32 = taps
            .iter()
            .filter(|(tap, _)| n >= CONVOLUTION_PARTITION + tap)
            .map(|(tap, gain)| gain * input[n - CONVOLUTION_PARTITION - tap])
            .sum();
        assert!(
            (l[n] - expected).abs() < 5e-6,
            "{n}: {} != {expected}",
            l[n]
        );
        assert!((r[n] + 0.125 * expected).abs() < 5e-6);
    }
}
#[test]
fn convolver_is_partition_invariant() {
    let frames: Vec<[f32; 2]> = (0..2048)
        .map(|i| [((i * 13 % 31) as f32 - 15.0) * 0.001; 2])
        .collect();
    let params = ConvolverParams {
        mix: 0.7,
        ..Default::default()
    };
    let input = sine(719.0, 9501);
    assert_eq!(
        render_ir(&frames, params, &input, &[input.len()]),
        render_ir(&frames, params, &input, &[1, 73, 129, 7, 2048])
    );
}
#[test]
fn impulse_admission_refuses_empty_oversized_and_damaged_data() {
    assert!(PreparedImpulse::prepare(&[]).is_err());
    assert!(PreparedImpulse::prepare(&vec![[0.0; 2]; MAX_IMPULSE_SAMPLES + 1]).is_err());
    assert!(PreparedImpulse::prepare(&[[f32::NAN, 0.0]]).is_err());
}

#[test]
fn frequency_shift_changes_zero_crossing_rate_in_both_directions() {
    let input = sine(1000.0, 48000);
    for shift_hz in [-300.0, 300.0] {
        let (l, r) = render::<FrequencyShifter>(
            FrequencyShifterParams { shift_hz, mix: 1.0 },
            &input,
            &[91, 1, 17],
        );
        let observed = crossing_rate(&l[8000..]);
        assert!(
            (observed - (1000.0 + shift_hz)).abs() < 3.0,
            "shift {shift_hz}: {observed}"
        );
        assert!((crossing_rate(&r[8000..]) - observed).abs() < 3.0);
    }
}

#[test]
fn frequency_shifter_is_partition_invariant() {
    let input = sine(1387.0, 8109);
    let params = FrequencyShifterParams {
        shift_hz: -137.25,
        mix: 0.81,
    };
    assert_eq!(
        render::<FrequencyShifter>(params, &input, &[input.len()]),
        render::<FrequencyShifter>(params, &input, &[1, 127, 18, 509])
    );
}

#[test]
fn pitch_shift_changes_period_by_semitones() {
    // 375 Hz has exactly 16 cycles in a grain span: overlap stays coherent.
    let input = sine(375.0, 72000);
    for (semitones, expected) in [(-12.0, 187.5), (12.0, 750.0)] {
        let (l, _) = render::<PitchShift>(
            PitchShiftParams {
                semitones,
                mix: 1.0,
            },
            &input,
            &[1, 71, 127],
        );
        let observed = crossing_rate(&l[12000..]);
        assert!((observed - expected).abs() < 3.0, "{semitones}: {observed}");
        assert!(rms(&l[12000..]) > 0.1);
    }
}

#[test]
fn unison_pitch_shift_is_the_exact_fixed_delay() {
    let input = sine(739.0, 6000);
    let (l, _) = render::<PitchShift>(PitchShiftParams::default(), &input, &[1, 131]);
    for n in 0..l.len() {
        assert_eq!(
            l[n],
            if n < PITCH_SHIFT_LATENCY {
                0.0
            } else {
                input[n - PITCH_SHIFT_LATENCY]
            }
        );
    }
}

#[test]
fn pitch_correction_moves_a_voiced_note_toward_a_scale_note() {
    let input = sine(455.0, 144000);
    // 455 Hz is closest to A# in chromatic tuning, but to A in C major.
    for (scale, expected) in [
        (PitchScale::Chromatic, 466.16376),
        (PitchScale::Major, 440.0),
    ] {
        let params = PitchCorrectParams {
            scale,
            speed_ms: 0.0,
            ..PitchCorrectParams::default()
        };
        let (l, _) = render::<PitchCorrect>(params, &input, &[173, 1, 211]);
        let observed = crossing_rate(&l[48000..]);
        assert!(
            (observed - expected).abs() < 5.0,
            "{scale:?} corrected frequency: {observed}"
        );
        assert!(rms(&l[48000..]) > 0.05);
    }
}

#[test]
fn vocoder_requires_a_nonzero_modulator_envelope_and_absent_key_is_silent() {
    let input = sine(900.0, 24000);
    for band_count in [16, 23, 32] {
        let params = VocoderParams {
            band_count,
            ..VocoderParams::default()
        };
        let mut effect = Vocoder::default();
        effect.prepare(SR, 512);
        effect.set_params(&params);
        let silent_key = vec![[0.0; 2]; input.len()];
        let mut l = input.clone();
        let mut r = input.clone();
        effect.process_sidechain(&mut l, &mut r, Some(&silent_key));
        assert!(l.iter().chain(&r).all(|v| *v == 0.0));
        effect.reset();
        let key: Vec<[f32; 2]> = input.iter().map(|v| [*v, -*v]).collect();
        l.copy_from_slice(&input);
        r.copy_from_slice(&input);
        effect.process_sidechain(&mut l, &mut r, Some(&key));
        assert!(rms(&l[4000..]) > 0.01, "silent {band_count}-band vocoder");
        effect.reset();
        l.copy_from_slice(&input);
        r.copy_from_slice(&input);
        effect.process_sidechain(&mut l, &mut r, None);
        assert!(l.iter().chain(&r).all(|v| *v == 0.0));
    }
}

#[test]
fn vocoder_short_key_is_zero_filled_instead_of_self_modulated() {
    let mut effect = Vocoder::default();
    let mut l = sine(900.0, 1000);
    let mut r = l.clone();
    effect.process_sidechain(&mut l, &mut r, Some(&[]));
    assert!(l.iter().chain(&r).all(|v| *v == 0.0));
}

fn healthy<E: Effect + Default>(params: E::Params) {
    let mut effect = E::default();
    effect.prepare(f32::NAN, 17);
    effect.set_params(&params);
    let mut l = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        -f32::MAX,
        1.0e-40,
        0.0,
        1.0,
    ];
    let mut r = l;
    for i in 0..400 {
        if i % 23 == 0 {
            effect.set_params(&params);
            effect.set_tempo(f32::NAN);
        }
        effect.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    }
    effect.reset();
    l.fill(0.0);
    r.fill(0.0);
    effect.process(&mut l, &mut r);
    assert!(l.iter().chain(&r).all(|v| *v == 0.0));
}
#[test]
fn damaged_parameters_and_audio_never_produce_nan() {
    let convolution = ConvolverParams {
        mix: f32::NAN,
        gain: f32::INFINITY,
        ..ConvolverParams::default()
    };
    healthy::<Convolver>(convolution);
    healthy::<FrequencyShifter>(FrequencyShifterParams {
        shift_hz: f32::NAN,
        mix: f32::INFINITY,
    });
    healthy::<PitchShift>(PitchShiftParams {
        semitones: f32::INFINITY,
        mix: f32::NAN,
    });
    healthy::<PitchCorrect>(PitchCorrectParams {
        root: u8::MAX,
        speed_ms: f32::NAN,
        amount: f32::INFINITY,
        tuning_hz: f32::NEG_INFINITY,
        mix: f32::NAN,
        ..PitchCorrectParams::default()
    });
    healthy::<Vocoder>(VocoderParams {
        band_count: u8::MAX,
        color: f32::NAN,
        attack_ms: f32::INFINITY,
        release_ms: f32::NAN,
        gain: f32::INFINITY,
        mix: f32::NAN,
        ..Default::default()
    });
}

fn roundtrip<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned>() {
    let original = P::default();
    let json = serde_json::to_string(&original).unwrap();
    assert_eq!(serde_json::from_str::<P>(&json).unwrap(), original);
    assert_eq!(serde_json::from_str::<P>("{}").unwrap(), original);
    for (i, info) in P::descriptors().iter().enumerate() {
        assert_eq!(original.get(i), Some(info.default));
        let mut p = original;
        assert!(p.set(i, f32::NAN));
        assert!(p.get(i).unwrap().is_finite());
        assert!(info.id.chars().all(|c| c != '_'));
    }
}
#[test]
fn params_roundtrip_and_defaults_match_descriptors() {
    roundtrip::<ConvolverParams>();
    roundtrip::<FrequencyShifterParams>();
    roundtrip::<PitchShiftParams>();
    roundtrip::<PitchCorrectParams>();
    roundtrip::<VocoderParams>();
    assert!(
        serde_json::to_value(PitchCorrectParams::default())
            .unwrap()
            .get("speedMs")
            .is_some()
    );
}

#[test]
fn remaining_processors_are_also_partition_invariant() {
    let input = sine(455.0, 19001);
    let one = &[input.len()];
    let many = &[1, 7, 65, 513];
    let shift = PitchShiftParams {
        semitones: 7.0,
        mix: 0.91,
    };
    assert_eq!(
        render::<PitchShift>(shift, &input, one),
        render::<PitchShift>(shift, &input, many)
    );
    let correct = PitchCorrectParams::default();
    assert_eq!(
        render::<PitchCorrect>(correct, &input, one),
        render::<PitchCorrect>(correct, &input, many)
    );
    let vocoder = VocoderParams {
        band_count: 32,
        color: 0.3,
        ..VocoderParams::default()
    };
    assert_eq!(
        render::<Vocoder>(vocoder, &input, one),
        render::<Vocoder>(vocoder, &input, many)
    );
}
