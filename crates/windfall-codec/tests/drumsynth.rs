use windfall_codec::*;
const PRESET:&[u8]=b"[General]\nVersion=DrumSynth v2.0\nComment=Original fixture\n[Tone]\nOn=1\nLevel=128\nF1=440\nF2=440\nPhase=0\nEnvelope=0,100 4410,0\n[Noise]\nOn=0\n[Own Extension]\nText=retained\n";
#[test]
fn bounded_public_text_reader_retains_unknown_fields_and_renders_reference_tone() {
    let preset = DrumSynthPreset::parse(PRESET).unwrap();
    assert_eq!(preset.get("own extension", "text"), Some("retained"));
    let render = preset.render(44100, 7000, 7).unwrap();
    assert_eq!(render.audio.frames(), 6000);
    // Independent closed-form reference for a constant oscillator and linear envelope.
    for frame in [0, 1, 100, 2205, 4409] {
        let expected = 0.5
            * (1.0 - frame as f64 / 4410.0)
            * (std::f64::consts::TAU * 440.0 * (frame + 1) as f64 / 44100.0).sin();
        assert_eq!(
            render.audio.samples()[frame],
            (expected * 32768.0).trunc() as f32 / 32768.0
        );
    }
    assert!(
        render
            .warnings
            .iter()
            .any(|w| w.contains("own extension.text"))
    );
}
#[test]
fn hostile_inputs_fail_without_wrong_audio() {
    for extra in [
        "[Overtones]\nOn=1\nMethod=4\n",
        "[Distortion]\nOn=1\nRate=7\n",
    ] {
        let bytes = [PRESET, extra.as_bytes()].concat();
        assert!(
            DrumSynthPreset::parse(&bytes)
                .unwrap()
                .render(44100, 7000, 1)
                .is_err()
        );
    }
    assert!(DrumSynthPreset::parse(&vec![b'x'; MAX_DRUMSYNTH_PRESET_BYTES + 1]).is_err());
    let duplicate = [PRESET, b"[Tone]\nOn=0\n"].concat();
    assert!(DrumSynthPreset::parse(&duplicate).is_err());
    let nan = String::from_utf8(PRESET.to_vec())
        .unwrap()
        .replace("F1=440", "F1=NaN");
    assert!(
        DrumSynthPreset::parse(nan.as_bytes())
            .unwrap()
            .render(44100, 7000, 1)
            .is_err()
    );
    let envelope = String::from_utf8(PRESET.to_vec())
        .unwrap()
        .replace("0,100 4410,0", "0,100 0,0");
    assert!(
        DrumSynthPreset::parse(envelope.as_bytes())
            .unwrap()
            .render(44100, 7000, 1)
            .is_err()
    );
    assert!(
        DrumSynthPreset::parse(PRESET)
            .unwrap()
            .render(44100, 10, 1)
            .is_err()
    );
}
#[test]
fn seeded_flat_noise_is_finite_deterministic_and_bounded() {
    let bytes=b"[General]\nVersion=DrumSynth v1.0\n[Noise]\nOn=1\nLevel=128\nSlope=0\nEnvelope=0,100 4410,0\n";
    let preset = DrumSynthPreset::parse(bytes).unwrap();
    let a = preset.render(48000, 8000, 7).unwrap();
    let b = preset.render(48000, 8000, 7).unwrap();
    let c = preset.render(48000, 8000, 8).unwrap();
    assert_eq!(a.audio.samples(), b.audio.samples());
    assert_ne!(a.audio.samples(), c.audio.samples());
    assert!(
        a.audio
            .samples()
            .iter()
            .all(|x| x.is_finite() && x.abs() <= 0.5)
    );
}

#[test]
fn registered_drumsynth_sources_probe_and_decode_with_exact_limits_and_case_insensitive_hints() {
    assert!(is_audio_extension("DS"));
    let info = probe_bytes(PRESET, Some("DS")).unwrap();
    assert_eq!(
        (info.sample_rate, info.channels, info.frames),
        (44100, 1, Some(6000))
    );
    assert_eq!(info.codec, "drumsynth");
    let options = DecodeOptions {
        max_decoded_bytes: 6000 * 4,
    };
    let ordinary = decode_bytes_with(PRESET, Some("ds"), &options).unwrap();
    let strict = decode_bytes_strict_with(PRESET, Some("DS"), &options).unwrap();
    assert_eq!(ordinary.samples(), strict.samples());
    for frame in [0, 1, 100, 2205, 4409] {
        let expected = 0.5
            * (1.0 - frame as f64 / 4410.0)
            * (std::f64::consts::TAU * 440.0 * (frame + 1) as f64 / 44100.0).sin();
        assert_eq!(
            ordinary.samples()[frame],
            (expected * 32768.0).trunc() as f32 / 32768.0
        );
    }
    for budget in [0, 3, 6000 * 4 - 1] {
        assert!(matches!(
            decode_bytes_with(
                PRESET,
                Some("ds"),
                &DecodeOptions {
                    max_decoded_bytes: budget
                }
            ),
            Err(CodecError::TooLarge { .. })
        ));
    }
    assert!(
        decode_bytes_with(
            b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nF1=NaN\n",
            Some("ds"),
            &options
        )
        .is_err()
    );
    assert!(decode_bytes(&vec![b'x'; MAX_DRUMSYNTH_PRESET_BYTES + 1], Some("ds")).is_err());
}
