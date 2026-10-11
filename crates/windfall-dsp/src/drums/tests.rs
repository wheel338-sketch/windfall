use super::*;
use crate::param::ParamSet;

const RATE: f32 = 48_000.0;

fn render<I: Instrument>(instrument: &mut I, samples: usize, partitions: &[usize]) -> Vec<f32> {
    let mut left = vec![0.0; samples];
    let mut right = vec![0.0; samples];
    let mut at = 0;
    let mut block = 0;
    while at < samples {
        let end = (at + partitions[block % partitions.len()]).min(samples);
        instrument.process(&mut left[at..end], &mut right[at..end]);
        at = end;
        block += 1;
    }
    assert_eq!(left, right);
    left
}

fn power(signal: &[f32]) -> f64 {
    signal.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / signal.len() as f64
}

// Measure actual audio zero crossings, with interpolation to remove sample
// quantization. The click is disabled so the crossings belong to the body sine.
fn upward_crossings(signal: &[f32]) -> Vec<f64> {
    signal
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            if pair[0] < 0.0 && pair[1] >= 0.0 {
                Some(index as f64 + f64::from(-pair[0] / (pair[1] - pair[0])))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn kick_pitch_drops_from_short_early_period_to_long_late_period() {
    for rate in [44_100.0, RATE, 96_000.0] {
        let mut kick = Kick::default();
        kick.prepare(rate, 256);
        kick.set_params(&KickParams {
            click: 0.0,
            ..KickParams::default()
        });
        kick.note_on(36, 1.0);
        let signal = render(&mut kick, (rate * 0.3) as usize, &[127]);
        let crossings = upward_crossings(&signal);
        let early = crossings[1] - crossings[0];
        let late_periods: Vec<f64> = crossings
            .windows(2)
            .filter(|c| c[0] > f64::from(rate) * 0.12)
            .map(|c| c[1] - c[0])
            .collect();
        let late = late_periods.iter().sum::<f64>() / late_periods.len() as f64;
        assert!(
            early < late * 0.6,
            "early {early}, late {late}, rate {rate}"
        );
        assert!((f64::from(rate) / late - 52.0).abs() < 1.0);
    }
}

fn high_frequency_power(signal: &[f32]) -> f64 {
    // A measured 4 kHz high-pass rejects the kick body, retaining wire/hat energy.
    let mut filter = crate::blocks::svf::Svf::default();
    let coeffs = crate::blocks::svf::SvfCoeffs::new(4000.0, 0.707, RATE);
    signal
        .iter()
        .map(|x| f64::from(filter.tick(&coeffs, *x).high).powi(2))
        .sum::<f64>()
        / signal.len() as f64
}

#[test]
fn snare_and_hat_have_more_high_frequency_energy_than_kick() {
    let sounds = [
        DrumMode::Kick,
        DrumMode::Snare,
        DrumMode::Hat,
        DrumMode::Tom,
    ]
    .map(|mode| {
        let mut drum = DrumVoice::default();
        drum.prepare(RATE, 512);
        drum.set_params(&DrumVoiceParams {
            mode,
            ..DrumVoiceParams::default()
        });
        drum.note_on(36, 1.0);
        render(&mut drum, 12_000, &[512])
    });
    let high = sounds.each_ref().map(|s| high_frequency_power(s));
    assert!(high[1] > high[0] * 10.0, "snare {high:?}");
    assert!(high[2] > high[0] * 10.0, "hat {high:?}");
    for a in 0..4 {
        for b in a + 1..4 {
            let difference: Vec<f32> = sounds[a]
                .iter()
                .zip(&sounds[b])
                .map(|(a, b)| a - b)
                .collect();
            assert!(power(&difference) > 0.001, "modes {a} and {b}");
        }
    }
}

#[test]
fn rack_pads_and_an_individual_pad_edit_are_audible() {
    let mut rack = DrumRack::default();
    rack.prepare(RATE, 512);
    rack.note_on(36, 1.0);
    let first = render(&mut rack, 6000, &[512]);
    rack.reset();
    rack.note_on(37, 1.0);
    let second = render(&mut rack, 6000, &[512]);
    let difference: Vec<f32> = first.iter().zip(&second).map(|(a, b)| a - b).collect();
    assert!(power(&difference) > 0.01);

    let mut params = DrumRackParams::default();
    params.pads[0].pitch_hz = 650.0;
    params.pads[0].noise_mix = 0.7;
    rack.reset();
    rack.set_params(&params);
    rack.note_on(36, 1.0);
    let edited = render(&mut rack, 6000, &[512]);
    let difference: Vec<f32> = first.iter().zip(&edited).map(|(a, b)| a - b).collect();
    assert!(power(&difference) > 0.01);
    rack.reset();
    rack.note_on(37, 1.0);
    assert_eq!(second, render(&mut rack, 6000, &[512]));
    rack.reset();
    rack.note_on(35, 1.0);
    rack.note_on(52, 1.0);
    assert_eq!(rack.active_voices(), 0);
    assert!(render(&mut rack, 128, &[128]).iter().all(|x| *x == 0.0));
}

fn decay_and_release<I: Instrument + Default>(key: u8) {
    let mut drum = I::default();
    drum.prepare(RATE, 256);
    drum.note_on(key, 1.0);
    let signal = render(&mut drum, 240_000, &[777]);
    assert!(power(&signal[..1000]) > 0.0001);
    assert_eq!(drum.active_voices(), 0);
    assert!(signal[200_000..].iter().all(|x| *x == 0.0));
    drum.note_on(key, 1.0);
    render(&mut drum, 1000, &[256]);
    drum.note_off(key);
    let release = render(&mut drum, 2000, &[137]);
    assert!(release[1500..].iter().all(|x| *x == 0.0));
    assert_eq!(drum.active_voices(), 0);
    drum.note_on(key, 1.0);
    render(&mut drum, 1000, &[256]);
    drum.note_off(key);
    drum.all_notes_off();
    let choke = render(&mut drum, 256, &[1]);
    assert!(choke[193..].iter().all(|x| *x == 0.0));
    drum.note_on(key, 1.0);
    drum.reset();
    assert!(render(&mut drum, 64, &[64]).iter().all(|x| *x == 0.0));
}

#[test]
fn drums_end_in_exact_silence_after_decay_release_choke_and_reset() {
    decay_and_release::<Membrane>(60);
    decay_and_release::<DrumRack>(36);
    decay_and_release::<Kick>(36);
    decay_and_release::<DrumVoice>(36);
    for mode in [DrumMode::Snare, DrumMode::Hat, DrumMode::Tom] {
        let mut drum = DrumVoice::default();
        drum.prepare(RATE, 128);
        drum.set_params(&DrumVoiceParams {
            mode,
            ..DrumVoiceParams::default()
        });
        drum.note_on(36, 1.0);
        let audio = render(&mut drum, 48000, &[128]);
        assert_eq!(drum.active_voices(), 0);
        assert!(audio[40000..].iter().all(|x| *x == 0.0));
    }
}

fn extremes<I: Instrument + Default>(key: u8, cap: usize) {
    for rate in [f32::NAN, f32::INFINITY, 0.0, 8000.0, 384000.0] {
        let mut drum = I::default();
        drum.prepare(rate, 64);
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -f32::MAX,
            f32::MAX,
        ] {
            let mut params = I::Params::default();
            for index in 0..I::Params::descriptors().len() {
                assert!(params.set(index, value));
            }
            drum.set_params(&params);
            for note in 0..160 {
                drum.note_on((note % 128) as u8, f32::MAX);
            }
            drum.note_on(key, 1.0);
            assert!(drum.active_voices() <= cap);
            let signal = render(&mut drum, 4000, &[64]);
            assert!(
                signal
                    .iter()
                    .all(|x| x.is_finite() && x.abs() < cap as f32 * 4.0)
            );
            drum.note_on(key, f32::NAN);
            drum.note_on(255, 1.0);
            drum.all_notes_off();
            render(&mut drum, 2000, &[127]);
            drum.reset();
        }
    }
}

#[test]
fn extreme_parameters_and_note_storms_are_finite_and_bounded() {
    extremes::<Membrane>(60, Membrane::MAX_VOICES);
    extremes::<DrumRack>(36, DrumRack::MAX_VOICES);
    extremes::<Kick>(36, Kick::MAX_VOICES);
    extremes::<DrumVoice>(36, DrumVoice::MAX_VOICES);
}

fn partition_invariance<I: Instrument + Default>(key: u8) {
    fn sequence<I: Instrument + Default>(key: u8, blocks: &[usize]) -> Vec<f32> {
        let mut drum = I::default();
        drum.prepare(RATE, 512);
        drum.note_on(key, 0.9);
        let mut audio = render(&mut drum, 1379, blocks);
        let mut params = I::Params::default();
        for (index, info) in I::Params::descriptors().iter().enumerate() {
            params.set(index, info.max);
        }
        drum.set_params(&params);
        audio.extend(render(&mut drum, 1711, blocks));
        drum.note_on(key, 0.4);
        drum.note_on(key + 1, 0.7);
        audio.extend(render(&mut drum, 1403, blocks));
        drum.note_off(key);
        audio.extend(render(&mut drum, 2337, blocks));
        drum.all_notes_off();
        audio.extend(render(&mut drum, 511, blocks));
        drum.reset();
        drum.note_on(key, 0.9);
        audio.extend(render(&mut drum, 1379, blocks));
        audio
    }
    let full = sequence::<I>(key, &[16384]);
    assert_eq!(full, sequence::<I>(key, &[1]));
    assert_eq!(full, sequence::<I>(key, &[3, 127, 1, 512, 29, 64]));
}

#[test]
fn audio_and_automation_are_bit_identical_across_partitions() {
    partition_invariance::<Membrane>(60);
    partition_invariance::<DrumRack>(36);
    partition_invariance::<Kick>(36);
    partition_invariance::<DrumVoice>(36);
}

fn descriptors<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned>() {
    let defaults = P::default();
    assert_eq!(serde_json::from_str::<P>("{}").unwrap(), defaults);
    assert_eq!(
        defaults,
        serde_json::from_str(&serde_json::to_string(&defaults).unwrap()).unwrap()
    );
    let json = serde_json::to_value(defaults).unwrap();
    for (index, info) in P::descriptors().iter().enumerate() {
        assert_eq!(defaults.get(index), Some(info.default), "{}", info.id);
        assert_eq!(P::index_of(info.id), Some(index));
        let pointer = format!("/{}", info.id.replace('.', "/"));
        let value = json.pointer(&pointer).unwrap();
        if info.kind == crate::param::ParamKind::Choice {
            assert_eq!(
                value.as_str(),
                Some(info.choices[info.default as usize].value)
            );
        } else {
            assert!((value.as_f64().unwrap() - f64::from(info.default)).abs() < 0.001);
        }
        let mut invalid = defaults;
        assert!(invalid.set(index, f32::NAN));
        assert!(invalid.sanitized().get(index).unwrap().is_finite());
    }
    let mut params = defaults;
    assert!(!params.set(P::descriptors().len(), 0.0));
    assert_eq!(params.get(usize::MAX), None);
}

#[test]
fn descriptors_defaults_and_camel_case_persistence_agree() {
    descriptors::<MembraneParams>();
    descriptors::<KickParams>();
    descriptors::<DrumVoiceParams>();
    descriptors::<DrumPadParams>();
    descriptors::<DrumRackParams>();
    let bad = MembraneParams {
        pitch_hz: f32::NAN,
        decay_ms: f32::INFINITY,
        tone: -f32::MAX,
        snap: f32::MAX,
        pitch_drop_semitones: f32::NAN,
        pitch_decay_ms: f32::NEG_INFINITY,
        level: f32::NAN,
    }
    .sanitized();
    assert_eq!(bad.pitch_hz, MembraneParams::default().pitch_hz);
    assert_eq!(bad.tone, 0.0);
    assert_eq!(bad.snap, 1.0);
    let mut rack = DrumRackParams::default();
    rack.pads[15].pitch_hz = f32::NAN;
    assert_eq!(rack.sanitized(), DrumRackParams::default());
}

#[test]
fn membrane_strike_damping_and_body_change_the_passive_modal_response() {
    let sound = |strike_position, damping, body| {
        let mut drum = Membrane::default();
        drum.prepare(RATE, 257);
        drum.set_params(&MembraneParams { strike_position, damping, body, snap: 0.0, pitch_drop_semitones: 0.0, decay_ms: 2000.0, ..Default::default() });
        drum.note_on(60, 1.0);
        render(&mut drum, 48_000, &[7, 257, 31])
    };
    let center = sound(0.0, 0.0, 0.0);
    let rim = sound(0.75, 0.0, 0.0);
    let damped = sound(0.0, 1.0, 0.0);
    let body = sound(0.0, 0.0, 1.0);
    for other in [&rim, &damped, &body] {
        let delta: Vec<_> = center.iter().zip(other.iter()).map(|(a,b)| a-b).collect();
        assert!(power(&delta) > 1e-6);
        assert!(other.iter().all(|x| x.is_finite() && x.abs() < 2.0));
    }
    assert!(high_frequency_power(&damped[12_000..]) < high_frequency_power(&center[12_000..]));
}

#[test]
fn membrane_has_measured_circular_mode_frequencies_and_transposes() {
    fn amplitude(signal: &[f32], frequency: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &x) in signal.iter().enumerate() {
            let angle = std::f64::consts::TAU * frequency * i as f64 / 48_000.0;
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / signal.len() as f64).cos();
            re += f64::from(x) * window * angle.cos();
            im += f64::from(x) * window * angle.sin();
        }
        (re*re + im*im).sqrt()
    }
    for (key, fundamental) in [(60, 160.0), (72, 320.0)] {
        let mut drum = Membrane::default();
        drum.prepare(RATE, 256);
        drum.set_params(&MembraneParams { strike_position: 0.0, damping: 0.0, body: 0.0, tone: 1.0, snap: 0.0, pitch_drop_semitones: 0.0, decay_ms: 4000.0, ..Default::default() });
        drum.note_on(key, 1.0);
        let signal = render(&mut drum, 24_000, &[127]);
        for ratio in [1.0, 1.59334, 2.13555, 2.29542] {
            let f = fundamental * ratio;
            assert!(amplitude(&signal, f) > amplitude(&signal, f + 20.0) * 5.0);
        }
    }
}
