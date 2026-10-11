use super::*;
use crate::effect::Effect;
use crate::param::ParamSet;

#[test]
fn formula_arithmetic_and_supported_functions() {
    let mut source = FormulaSource::default();
    source.prepare(1000.0, 1);
    source.set_params(&FormulaSourceParams {
        a: 0.4,
        b: 0.25,
        ..Default::default()
    });
    let mut output = [0.0];
    source.set_expression("a + b * peak").unwrap();
    source.process_control(&[-0.8], &[0.1], &mut output);
    assert!((output[0] - 0.6).abs() < 1e-6);
    source
        .set_expression("min(max(abs(-a), b), 0.7) + sin(peak) * 0.1")
        .unwrap();
    source.process_control(&[0.5], &[0.0], &mut output);
    assert!((output[0] - (0.4 + 0.5_f32.sin() * 0.1)).abs() < 1e-6);
    source.set_expression("(a + b) * 1e-1").unwrap();
    source.process_control(&[0.0], &[0.0], &mut output);
    assert!((output[0] - 0.065).abs() < 1e-6);
}

#[test]
fn formula_rejects_unbounded_and_malformed_input_atomically() {
    let mut source = FormulaSource::default();
    source.set_expression("0.25").unwrap();
    for expression in [
        "", "loop(a)", "a / b", "min(a)", "sin(a,b)", "1e999", "a b", "é",
    ] {
        assert!(source.set_expression(expression).is_err(), "{expression}");
        let mut output = [0.0];
        source.process_control(&[0.0], &[0.0], &mut output);
        assert_eq!(output, [0.25]);
    }
    assert_eq!(
        source.set_expression(&"a".repeat(257)),
        Err(FormulaError::TooLong)
    );
    let operations = std::iter::repeat_n("a", 33).collect::<Vec<_>>().join("+");
    assert_eq!(
        source.set_expression(&operations),
        Err(FormulaError::TooComplex)
    );
    let nesting = format!("{}a{}", "(".repeat(16), ")".repeat(16));
    assert_eq!(
        source.set_expression(&nesting),
        Err(FormulaError::TooComplex)
    );
    source
        .set_expression("1000000 * 1000000 * 1000000")
        .unwrap();
    let mut output = [0.0];
    source.process_control(&[f32::NAN], &[f32::INFINITY], &mut output);
    assert_eq!(output, [1.0]);
}

#[test]
fn xy_pan_moves_and_gain_is_independent() {
    let mut pad = XyPad::default();
    pad.prepare(1000.0, 8);
    let mut left = [1.0; 8];
    let mut right = [1.0; 8];
    pad.process(&mut left, &mut right);
    assert_eq!(left, right);
    assert_eq!(left, [1.0; 8]);
    pad.set_params(&XyPadParams { x: 1.0, y: 0.5 });
    left.fill(1.0);
    right.fill(1.0);
    let mut axes = [[0.0; 2]; 8];
    pad.process_with_control(&mut left, &mut right, &mut axes);
    assert!(left[0] > left[3]);
    assert_eq!(left[7], 0.0);
    assert_eq!(right[7], 1.0);
    assert_eq!(axes[7], [1.0, 0.5]);
    pad.set_params(&XyPadParams { x: 0.0, y: 0.25 });
    left.fill(1.0);
    right.fill(1.0);
    pad.process(&mut left, &mut right);
    assert_eq!(left[7], 0.5);
    assert_eq!(right[7], 0.0);
}

#[test]
fn xyz_third_axis_changes_resonance() {
    fn power(z: f32) -> f32 {
        let mut pad = XyzPad::default();
        pad.prepare(48_000.0, 4096);
        pad.set_params(&XyzPadParams {
            x: 0.5,
            y: 50.0_f32.ln() / 1000.0_f32.ln(),
            z,
        });
        let mut left = std::array::from_fn::<_, 4096, _>(|n| {
            0.1 * (std::f32::consts::TAU * 1000.0 * n as f32 / 48_000.0).sin()
        });
        let mut right = left;
        pad.process(&mut left, &mut right);
        left[2048..].iter().map(|v| v * v).sum()
    }
    assert!(power(1.0) > 10.0 * power(0.0));
}

#[test]
fn follower_attack_and_ducking_use_the_stereo_peak() {
    let mut follower = EnvelopeFollower::default();
    follower.prepare(1000.0, 10);
    follower.set_params(&EnvelopeFollowerParams {
        duck: 1.0,
        ..Default::default()
    });
    let mut left = [1.0; 10];
    let mut right = [0.1; 10];
    let mut control = [0.0; 10];
    follower.process_with_control(&mut left, &mut right, &mut control);
    assert!(control.windows(2).all(|pair| pair[1] > pair[0]));
    assert!((control[9] - 0.63212).abs() < 0.002);
    assert!((left[9] - (1.0 - control[9])).abs() < 1e-6);
    assert!((right[9] - 0.1 * left[9]).abs() < 1e-6);
    let mut release = [0.0; 100];
    follower.process_control(&[0.0; 100], &[0.0; 100], &mut release);
    assert!((release[99] - control[9] / std::f32::consts::E).abs() < 0.002);
}

#[test]
fn note_envelope_attacks_sustains_releases_and_retriggers() {
    let mut envelope = NoteEnvelope::default();
    envelope.prepare(1000.0, 32);
    let mut params = NoteEnvelopeParams {
        trigger: true,
        attack_ms: 10.0,
        decay_ms: 10.0,
        sustain: 0.5,
        release_ms: 20.0,
        ..Default::default()
    };
    envelope.set_params(&params);
    let mut left = [1.0; 32];
    let mut right = left;
    let mut control = [0.0; 32];
    envelope.process_with_control(&mut left, &mut right, &mut control);
    assert!(control[..9].windows(2).all(|pair| pair[1] > pair[0]));
    assert!(control[9] > 0.99);
    assert!((control[31] - 0.5).abs() < 1e-5);
    assert_eq!(left, control);
    envelope.set_params(&params); // A held gate does not restart attack.
    envelope.process_control(&mut control);
    assert!((control[0] - 0.5).abs() < 1e-5);
    envelope.trigger();
    envelope.process_control(&mut control);
    assert!(control[0] > 0.5);
    params.trigger = false;
    envelope.set_params(&params);
    envelope.process_control(&mut control);
    assert!(control[0] < 0.5);
    assert_eq!(control[31], 0.0);
    params.trigger = true;
    envelope.set_params(&params);
    envelope.reset();
    envelope.process_control(&mut control);
    assert!(control[0] > 0.0 && control[0] < 1.0);
}

#[test]
fn pan_motion_modulates_audio_and_volume() {
    let mut motion = PanLfo::default();
    motion.prepare(1000.0, 1000);
    motion.set_params(&PanLfoParams {
        volume_depth: 1.0,
        ..Default::default()
    });
    let mut left = [1.0; 1000];
    let mut right = left;
    let mut output = [[0.0; 2]; 1000];
    motion.process_with_control(&mut left, &mut right, &mut output);
    assert!((output[250][0] - 1.0).abs() < 1e-5);
    assert!(left[250] < 1e-5 && right[250] < 1e-5);
    assert!((output[750][0] + 1.0).abs() < 1e-5);
    assert!((left[750] - 1.0).abs() < 1e-5 && right[750] < 1e-5);
}

#[test]
fn keyboard_last_note_drives_pitch_and_velocity_offsets() {
    let mut source = KeyboardSource::default();
    source.prepare(1000.0, 8);
    source.set_note(72, 0.5);
    let mut output = [KeyboardControl::default(); 8];
    source.process_control(&mut output);
    assert_eq!(
        output[0],
        KeyboardControl {
            pitch_semitones: 12.0,
            gain_offset: -0.5
        }
    );
    source.set_note(72, 0.0);
    source.process_control(&mut output);
    assert_eq!(
        output[7],
        KeyboardControl {
            pitch_semitones: 12.0,
            gain_offset: -1.0
        }
    );
    source.reset();
    source.process_control(&mut output);
    assert_eq!(
        output[0],
        KeyboardControl {
            pitch_semitones: 0.0,
            gain_offset: -1.0
        }
    );
}

fn finite_effect<E: Effect + Default>() {
    for sample_rate in [f32::NAN, 1.0, 48_000.0, 384_000.0] {
        let mut effect = E::default();
        effect.prepare(sample_rate, 256);
        let mut params = E::Params::default();
        for (i, info) in E::Params::descriptors().iter().enumerate() {
            params.set(i, f32::NAN);
            assert!(params.get(i).unwrap().is_finite());
            params.set(i, info.max);
        }
        effect.set_params(&params);
        let mut left = std::array::from_fn::<_, 256, _>(|i| {
            [f32::NAN, f32::INFINITY, -f32::MAX, 0.0, 1.0][i % 5]
        });
        let mut right = left;
        effect.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|v| v.is_finite()));
        effect.reset();
        effect.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|v| v.is_finite()));
    }
}

#[test]
fn every_processor_contains_nonfinite_audio_and_parameters() {
    finite_effect::<XyPad>();
    finite_effect::<XyzPad>();
    finite_effect::<PanLfo>();
    finite_effect::<EnvelopeFollower>();
    finite_effect::<NoteEnvelope>();
    let mut formula = FormulaSource::default();
    formula.prepare(f32::NAN, 4);
    formula.set_params(&FormulaSourceParams {
        a: f32::NAN,
        b: f32::INFINITY,
        base: -f32::INFINITY,
        amount: f32::MAX,
        ..Default::default()
    });
    let mut output = [0.0; 4];
    formula.process_control(&[f32::NAN; 4], &[f32::INFINITY; 4], &mut output);
    assert!(
        output
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    );
    let mut keyboard = KeyboardSource::default();
    keyboard.set_params(&KeyboardSourceParams {
        root_key: 255,
        pitch_amount: f32::NAN,
        transpose: f32::INFINITY,
        velocity_amount: f32::NAN,
    });
    keyboard.set_note(255, f32::NAN);
    let mut controls = [KeyboardControl::default(); 4];
    keyboard.process_control(&mut controls);
    assert!(
        controls
            .iter()
            .all(|v| v.pitch_semitones.is_finite() && v.gain_offset.is_finite())
    );
}

fn invariant<E: Effect + Default>() {
    let mut whole = E::default();
    let mut split = E::default();
    for effect in [&mut whole, &mut split] {
        effect.prepare(48_000.0, 1024);
        effect.set_params(&E::Params::default());
        effect.process(&mut [0.0; 1], &mut [0.0; 1]);
        let mut target = E::Params::default();
        for (i, info) in E::Params::descriptors().iter().enumerate() {
            target.set(i, info.max);
        }
        effect.set_params(&target);
    }
    let input = std::array::from_fn::<_, 1024, _>(|i| (i as f32 * 0.13).sin() * 0.1);
    let (mut wl, mut wr, mut sl, mut sr) = (input, input, input, input);
    whole.process(&mut wl, &mut wr);
    for (l, r) in sl.chunks_mut(37).zip(sr.chunks_mut(37)) {
        split.process(l, r);
    }
    assert_eq!(wl, sl);
    assert_eq!(wr, sr);
}

#[test]
fn audio_and_parameter_glides_are_block_invariant() {
    invariant::<XyPad>();
    invariant::<XyzPad>();
    invariant::<PanLfo>();
    invariant::<EnvelopeFollower>();
    invariant::<NoteEnvelope>();
}

fn persistence<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned>() {
    let default = P::default();
    assert_eq!(default, default.sanitized());
    for (i, descriptor) in P::descriptors().iter().enumerate() {
        assert_eq!(default.get(i), Some(descriptor.default));
        assert_eq!(P::index_of(descriptor.id), Some(i));
    }
    assert_eq!(default.get(P::descriptors().len()), None);
    let json = serde_json::to_string(&default).unwrap();
    assert_eq!(serde_json::from_str::<P>(&json).unwrap(), default);
    assert_eq!(serde_json::from_str::<P>("{}").unwrap(), default);
    let value = serde_json::to_value(default).unwrap();
    for descriptor in P::descriptors() {
        assert!(value.get(descriptor.id).is_some(), "{}", descriptor.id);
    }
}

#[test]
fn parameter_defaults_descriptors_and_camel_case_persist() {
    persistence::<FormulaSourceParams>();
    persistence::<XyPadParams>();
    persistence::<XyzPadParams>();
    persistence::<PanLfoParams>();
    persistence::<EnvelopeFollowerParams>();
    persistence::<NoteEnvelopeParams>();
    persistence::<KeyboardSourceParams>();
}
