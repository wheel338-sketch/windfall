//! Independent synthetic numerical references. No downloaded preset/PCM files.
use std::f64::consts::TAU;
use windfall_codec::{DrumSynthPreset, MAX_DRUMSYNTH_FRAMES};
fn render(text: &str, rate: u32, seed: u64) -> Vec<f32> {
    DrumSynthPreset::parse(format!("[General]\nVersion=DrumSynth v2.0\n{text}").as_bytes())
        .unwrap()
        .render(rate, MAX_DRUMSYNTH_FRAMES, seed)
        .unwrap()
        .audio
        .samples()
        .to_vec()
}
fn pcm(x: f64) -> f32 {
    (x.clamp(-32700.0, 32700.0).trunc() / 32768.0) as f32
}

#[test]
fn cancellable_drumsynth_matches_full_pcm_and_never_returns_a_cancelled_prefix() {
    let source = b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nF1=440\nF2=440\nEnvelope=0,100 44100,0\n";
    let preset = DrumSynthPreset::parse(source).unwrap();
    let full = preset.render(48000, MAX_DRUMSYNTH_FRAMES, 7).unwrap();
    let cancellable = preset
        .render_cancellable(48000, MAX_DRUMSYNTH_FRAMES, 7, &mut || true)
        .unwrap();
    assert_eq!(full.audio.samples(), cancellable.audio.samples());
    let mut blocks = 0;
    let error = preset
        .render_cancellable(48000, MAX_DRUMSYNTH_FRAMES, 7, &mut || {
            blocks += 1;
            blocks < 4
        })
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(blocks, 4);
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() <= 1.0 / 32768.0, "{a} versus {b}");
}

#[test]
fn linear_and_drooping_pitch_follow_independent_integrated_phase() {
    for droop in [0.0, 20.0, 65.0] {
        let audio = render(
            &format!(
                "[Tone]\nOn=1\nPhase=0\nF1=900\nF2=100\nDroop={droop}\nEnvelope=0,100 12000,0\n"
            ),
            44100,
            7,
        );
        let k = -4.0 * 10.0_f64.powf((droop - 20.0) / 30.0) / 12000.0;
        let asymptote = 900.0 + (100.0 - 900.0) / (1.0 - (12000.0 * k).exp());
        for t in [0, 1, 100, 2000, 9000, 11999] {
            let n = (t + 1) as f64;
            let integrated = if droop == 0.0 {
                n * 900.0 - 800.0 * (t * (t + 1) / 2) as f64 / 12000.0
            } else {
                n * asymptote + (900.0 - asymptote) * (1.0 - (n * k).exp()) / (1.0 - k.exp())
            };
            close(
                audio[t],
                pcm(16384.0 * (1.0 - t as f64 / 12000.0) * (TAU * integrated / 44100.0).sin()),
            );
        }
    }
}

#[test]
fn droop_uses_reference_authored_time_before_global_length_scaling() {
    for (rate, stretch) in [(22050, 50), (48000, 200), (96000, 100)] {
        let audio = render(
            &format!(
                "Stretch={stretch}\n[Tone]\nOn=1\nPhase=0\nF1=400\nF2=100\nDroop=20\nEnvelope=0,100 4410,0\n"
            ),
            rate,
            7,
        );
        let end = 4410.0 * stretch as f64 / 100.0 * rate as f64 / 44100.0;
        let k = -4.0_f64 / 4410.0;
        let asymptote = 400.0 - 300.0 / (1.0 - (k * 4410.0).exp());
        for t in [0, 1, 100, (end / 2.0) as usize] {
            let n = (t + 1) as f64;
            let integrated =
                n * asymptote + (400.0 - asymptote) * (1.0 - (n * k).exp()) / (1.0 - k.exp());
            close(
                audio[t],
                pcm(16384.0 * (1.0 - t as f64 / end) * (TAU * integrated / rate as f64).sin()),
            );
        }
    }
}

#[test]
fn every_envelope_point_and_coincident_terminal_points_are_consumed() {
    let audio = render(
        "[Tone]\nOn=1\nPhase=0\nF1=300\nF2=300\nEnvelope=0,0 100,100 200,20 300,80 400,0 400,0\n",
        44100,
        7,
    );
    for (t, level) in [
        (0, 0.0),
        (50, 0.5),
        (100, 1.0),
        (150, 0.6),
        (200, 0.2),
        (250, 0.5),
        (300, 0.8),
        (350, 0.4),
        (401, 0.0),
    ] {
        close(
            audio[t],
            pcm(16384.0 * level * (TAU * 300.0 * (t + 1) as f64 / 44100.0).sin()),
        );
    }
    assert_eq!(audio.len(), 2400);
    assert!(audio[1200..].iter().all(|x| *x == 0.0));
}

#[test]
fn all_overtone_waves_add_fm_and_ring_match_closed_form() {
    for form in 0..5 {
        for method in 0..3 {
            let audio = render(
                &format!(
                    "[Overtones]\nOn=1\nMethod={method}\nWave1={form}\nWave2=0\nF1=400\nF2=127\nParam=50\nEnvelope1=0,100 1200,0\nEnvelope2=0,100 2400,0\n[Tone]\nPhase=0\n"
                ),
                44100,
                7,
            );
            for t in [0, 7, 35, 99, 650, 1199] {
                let p1 = TAU * 400.0 * (t + 1) as f64 / 44100.0;
                let p2 = TAU * 127.0 * (t + 1) as f64 / 44100.0;
                let e1 = 1.0 - t as f64 / 1200.0;
                let e2 = 1.0 - t as f64 / 2400.0;
                let shape = |p: f64| match form {
                    0 => p.sin(),
                    1 => (2.0 * (p * 0.5).sin()).abs() - 1.0,
                    2 => {
                        let x = 4.0 * p.rem_euclid(TAU) / TAU - 1.0;
                        if x > 1.0 { 2.0 - x } else { x }
                    }
                    3 => 2.0 * p.rem_euclid(TAU) / TAU - 1.0,
                    _ => {
                        if p.sin() > 0.0 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                };
                let value = match method {
                    0 => 0.5 * e1 * shape(p1) + 0.5 * e2 * p2.sin(),
                    1 => e1 * shape(p1 + e2 * p2.sin()),
                    _ => e1 * shape(p1) * (0.875 + 0.125 * e2 * p2.sin()),
                };
                close(audio[t], pcm(16384.0 * value));
            }
        }
    }
}

#[test]
fn overtone_tracking_uses_the_tone_sweep_ratio() {
    let audio = render(
        "[Tone]\nOn=1\nLevel=0\nPhase=0\nF1=300\nF2=100\nEnvelope=0,100 12000,0\n[Overtones]\nOn=1\nTrack1=1\nF1=900\nMethod=0\nParam=0\nEnvelope1=0,100 12000,0\n",
        44100,
        7,
    );
    for t in [0, 9, 300, 6000, 11999] {
        let phase = 3.0 * TAU / 44100.0
            * ((t + 1) as f64 * 300.0 - 200.0 * (t * (t + 1) / 2) as f64 / 12000.0);
        close(
            audio[t],
            pcm(16384.0 * (1.0 - t as f64 / 12000.0) * phase.sin()),
        );
    }
}

#[test]
fn tracked_overtone_phase_continues_across_blocks_after_the_tone_stops() {
    let audio = render(
        "[Tone]\nOn=1\nLevel=0\nPhase=0\nF1=300\nF2=300\nEnvelope=0,100 1200,0\n[Overtones]\nOn=1\nTrack1=1\nF1=900\nMethod=0\nParam=0\nEnvelope1=0,100 4800,0\n",
        44100,
        7,
    );
    for t in [1200, 2399, 2400, 3599, 3600, 4799] {
        close(
            audio[t],
            pcm(16384.0
                * (1.0 - t as f64 / 4800.0)
                * (TAU * 900.0 * (t + 1) as f64 / 44100.0).sin()),
        );
    }
}

#[test]
fn fixed_sequence_covers_coloured_noise_and_both_bands() {
    for slope in [-100, 0, 100] {
        let text = format!(
            "[Noise]\nOn=1\nLevel=40\nSlope={slope}\nFixedSeq=1\nEnvelope=0,100 12000,0\n[NoiseBand]\nOn=1\nLevel=40\nF=1400\ndF=40\nEnvelope=0,100 12000,0\n[NoiseBand2]\nOn=1\nLevel=40\nF=400\ndF=10\nEnvelope=0,100 12000,0\n"
        );
        assert_eq!(render(&text, 44100, 7), render(&text, 44100, 999));
        assert_ne!(
            render(&text, 44100, 7),
            render(&text.replace("FixedSeq=1", "FixedSeq=0"), 44100, 999)
        );
    }
    let noise = |slope| {
        render(
            &format!(
                "[Noise]\nOn=1\nLevel=10\nSlope={slope}\nFixedSeq=1\nEnvelope=0,100 12000,0\n"
            ),
            44100,
            7,
        )
    };
    let roughness = |s: Vec<f32>| {
        let energy = s.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        s.windows(2)
            .map(|w| f64::from(w[1] - w[0]).powi(2))
            .sum::<f64>()
            / energy
    };
    assert!(roughness(noise(-100)) < 0.2);
    assert!(roughness(noise(100)) > 3.0);
}

fn power(audio: &[f32], f: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (t, x) in audio.iter().enumerate() {
        let window = 0.5 - 0.5 * (TAU * t as f64 / (audio.len() - 1) as f64).cos();
        let phase = TAU * f * t as f64 / 44100.0;
        re += f64::from(*x) * window * phase.cos();
        im += f64::from(*x) * window * phase.sin();
    }
    re * re + im * im
}
#[test]
fn band_centres_and_width_have_measured_spectral_effects() {
    for section in ["NoiseBand", "NoiseBand2"] {
        for centre in [500, 2000] {
            let text =
                format!("[{section}]\nOn=1\nLevel=60\nF={centre}\ndF=0\nEnvelope=0,100 44100,0\n");
            let narrow = render(&text, 44100, 7);
            let wide = render(&text.replace("dF=0", "dF=100"), 44100, 7);
            let frequencies: Vec<f64> = (-10..=10)
                .map(|i| centre as f64 + i as f64 * 30.0)
                .collect();
            let n: Vec<f64> = frequencies
                .iter()
                .map(|f| power(&narrow[..16384], *f))
                .collect();
            let w: Vec<f64> = frequencies
                .iter()
                .map(|f| power(&wide[..16384], *f))
                .collect();
            assert!(
                n[10]
                    > n.iter()
                        .enumerate()
                        .filter(|(i, _)| *i != 10)
                        .map(|(_, x)| x)
                        .sum::<f64>()
                        * 100.0
            );
            let spread = |powers: Vec<f64>| {
                powers
                    .iter()
                    .enumerate()
                    .map(|(i, p)| p * (i as f64 - 10.0).powi(2))
                    .sum::<f64>()
                    / powers.iter().sum::<f64>()
            };
            assert!(spread(w) > spread(n) + 1.0);
        }
    }
}

#[test]
fn distortion_clips_quantizes_and_averages_documented_rate_groups() {
    let source = "[Tone]\nOn=1\nLevel=128\nF1=900\nF2=900\nPhase=0\nEnvelope=0,100 12000,0\n[Distortion]\nOn=1\nClipping=12\nBits=4\nRate=0\n";
    let audio = render(source, 44100, 7);
    assert!(audio.iter().all(|x| x.abs() <= 0.5));
    assert!(audio.contains(&0.5));
    let gain = 256.0 * 10.0_f64.powf(0.6);
    for t in [0, 1, 4, 11, 300, 11000] {
        let raw =
            16384.0 * (1.0 - t as f64 / 12000.0) * (TAU * 900.0 * (t + 1) as f64 / 44100.0).sin();
        let expected = ((raw / 256.0).trunc() * gain)
            .clamp(-16384.0, 16384.0)
            .trunc()
            / 32768.0;
        close(audio[t], expected as f32);
    }
    let quantized = render(&source.replace("Clipping=12", "Clipping=0"), 44100, 7);
    assert!(quantized.iter().all(|x| (x * 128.0).fract() == 0.0));
    for (rate, step) in [(1, 2), (2, 3), (3, 4), (4, 8), (5, 10), (6, 20)] {
        let samples = render(&source.replace("Rate=0", &format!("Rate={rate}")), 44100, 7);
        assert!(samples.chunks(step).all(|g| g.iter().all(|x| *x == g[0])));
    }
}

#[test]
fn filter_highpass_and_resonance_follow_independent_recurrence() {
    for highpass in [0, 1] {
        for resonance in [0.0, 70.0] {
            let audio = render(
                &format!(
                    "Filter=1\nHighPass={highpass}\nResonance={resonance}\nFilterEnv=0,40 12000,40\n[Tone]\nOn=1\nF1=300\nF2=300\nPhase=0\nEnvelope=0,100 12000,0\n"
                ),
                44100,
                7,
            );
            let fb = 1.001 - 10.0_f64.powf(-0.6);
            let r = (0.0101_f64 * resonance).sqrt();
            let (mut input_state, mut output_state) = (0.0, 0.0);
            for (t, actual) in audio.iter().take(1000).enumerate() {
                let input = 16384.0
                    * (1.0 - t as f64 / 12000.0)
                    * (TAU * 300.0 * (t + 1) as f64 / 44100.0).sin();
                let x = input + r * (1.0 + 1.0 / fb) * (input_state - output_state);
                input_state = fb * (input_state - x) + x;
                output_state = fb * (output_state - input_state) + input_state;
                close(*actual, pcm(output_state - highpass as f64 * input));
            }
        }
    }
    let plain = render("[Tone]\nOn=1\nEnvelope=0,100 12000,0\n", 44100, 7);
    let overtone_only = render(
        "[Tone]\nOn=1\nEnvelope=0,100 12000,0\n[Overtones]\nFilter=1\n",
        44100,
        7,
    );
    assert_eq!(plain, overtone_only);
}

#[test]
fn stretch_rate_and_tuning_have_independent_physical_units() {
    for rate in [22050, 44100, 48000, 96000] {
        for stretch in [20, 100, 200] {
            let samples = render(
                &format!(
                    "Stretch={stretch}\nTuning=12\n[Tone]\nOn=1\nF1=440\nF2=440\nPhase=0\nEnvelope=0,100 4410,0\n"
                ),
                rate,
                7,
            );
            let end = 4410.0 * stretch as f64 / 100.0 * rate as f64 / 44100.0;
            assert_eq!(samples.len(), 2400 + 1200 * (end / 1200.0) as usize);
            for t in [0, 1, 100] {
                close(
                    samples[t],
                    pcm(16384.0
                        * (1.0 - t as f64 / end)
                        * (TAU * 880.0 * (t + 1) as f64 / rate as f64).sin()),
                );
            }
        }
    }
}

#[test]
fn cymbal_and_all_sections_are_finite_and_hostile_parameters_are_refused() {
    let text = "Filter=1\nResonance=30\nFilterEnv=0,40 12000,80\n[Tone]\nOn=1\nEnvelope=0,100 12000,0\n[Noise]\nOn=1\nLevel=30\nSlope=-50\nEnvelope=0,100 9000,0\n[Overtones]\nOn=1\nMethod=3\nF1=300\nF2=811\nEnvelope1=0,100 8000,0\nEnvelope2=0,90 10000,0\n[NoiseBand]\nOn=1\nEnvelope=0,100 4000,0\n[NoiseBand2]\nOn=1\nEnvelope=0,100 5000,0\n[Distortion]\nOn=1\nBits=2\nRate=6\n";
    let audio = render(text, 48000, 7);
    assert!(audio.iter().all(|x| x.is_finite() && x.abs() < 1.0));
    assert!(audio.iter().any(|x| x.abs() > 0.1));
    for extra in [
        "[Noise]\nOn=1\nSlope=NaN\n",
        "[Overtones]\nOn=1\nMethod=3\nF1=0\n",
        "[General]\nFilter=2\n",
        "[Distortion]\nBits=99\n",
        "[NoiseBand]\nOn=1\nF=20000\n",
    ] {
        let bytes = format!("[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\n{extra}");
        assert!(
            DrumSynthPreset::parse(bytes.as_bytes())
                .unwrap()
                .render(8000, MAX_DRUMSYNTH_FRAMES, 7)
                .is_err(),
            "{extra}"
        );
    }
}

#[test]
fn integer_reference_controls_refuse_fractional_values_instead_of_changing_the_model() {
    for text in [
        "[Tone]\nLevel=128.5\n",
        "[Noise]\nSlope=-10.5\n",
        "[NoiseBand]\ndF=10.5\n",
        "[Overtones]\nParam=50.5\n",
        "[Distortion]\nClipping=3.5\n",
    ] {
        let bytes = format!("[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\n{text}");
        assert!(
            DrumSynthPreset::parse(bytes.as_bytes())
                .unwrap()
                .render(44100, MAX_DRUMSYNTH_FRAMES, 7)
                .is_err(),
            "{text}"
        );
    }
}

#[test]
fn legacy_byte_metadata_does_not_reject_or_change_an_ascii_synthesis_recipe() {
    let recipe=b"[General]\nVersion=DrumSynth v2.0\nComment=legacy \xe9\n[Tone]\nOn=1\nEnvelope=0,100 1200,0\n";
    let reference =
        b"[General]\nVersion=DrumSynth v2.0\nComment=plain\n[Tone]\nOn=1\nEnvelope=0,100 1200,0\n";
    let audio = |bytes: &[u8]| {
        DrumSynthPreset::parse(bytes)
            .unwrap()
            .render(44100, MAX_DRUMSYNTH_FRAMES, 7)
            .unwrap()
            .audio
    };
    assert_eq!(audio(recipe).samples(), audio(reference).samples());
    let invalid = b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nF1=\xff\n";
    assert!(
        DrumSynthPreset::parse(invalid)
            .unwrap()
            .render(44100, MAX_DRUMSYNTH_FRAMES, 7)
            .is_err()
    );
}
