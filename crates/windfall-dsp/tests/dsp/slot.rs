//! The effect slot: bypass, mix and the host-facing wrapper types.

use windfall_dsp::{
    AnyEffect, AnyInstrument, CompressorParams, DelayParams, EffectKind, EffectParams, EffectSlot,
    EqParams, InstrumentKind, LimiterParams, ReverbParams,
};

use crate::support::{RATE, db, noise, peak, rms, silence, sine, steepest};

fn slot(params: EffectParams) -> EffectSlot {
    let mut slot = EffectSlot::new(AnyEffect::new(&params));
    slot.prepare(RATE, 256);
    slot
}

fn run(slot: &mut EffectSlot, signal: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (signal.to_vec(), signal.to_vec());
    for (left, right) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
        slot.process(left, right);
    }
    (left, right)
}

fn loud_eq() -> EffectParams {
    let mut params = EqParams::default();
    params.peak2.gain_db = 12.0;
    params.peak2.frequency_hz = 440.0;
    EffectParams::Eq(params)
}

fn audible_params(kind: EffectKind) -> EffectParams {
    let mut params = kind.default_params();
    // A note-controlled gate is intentionally silent until a note triggers it.
    if let EffectParams::NoteEnvelope(envelope) = &mut params {
        envelope.trigger = true;
    }
    params
}

#[test]
fn hosting_types_can_be_sent_to_the_audio_thread_and_are_small() {
    fn sendable<T: Send>() {}
    sendable::<AnyEffect>();
    sendable::<EffectSlot>();
    sendable::<AnyInstrument>();
    sendable::<EffectParams>();
    sendable::<windfall_dsp::InstrumentParams>();
    sendable::<windfall_dsp::GainReductionMeter>();
    // The processors live on the heap, so passing one through a queue
    // moves a pointer and a tag.
    assert!(std::mem::size_of::<AnyEffect>() <= 2 * std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<AnyInstrument>() <= 2 * std::mem::size_of::<usize>());
}

#[test]
fn any_effect_reports_what_it_holds_and_refuses_other_settings() {
    for kind in EffectKind::ALL {
        let mut effect = AnyEffect::new(&kind.default_params());
        effect.prepare(RATE, 256);
        assert_eq!(effect.kind(), kind);
        assert!(effect.set_params(&kind.default_params()));
        for other in EffectKind::ALL {
            if other != kind {
                assert!(!effect.set_params(&other.default_params()));
            }
        }
        let has_meter = matches!(kind, EffectKind::Compressor | EffectKind::Limiter);
        assert_eq!(effect.gain_reduction().is_some(), has_meter);
        let has_latency = matches!(
            kind,
            EffectKind::Limiter
                | EffectKind::Distortion
                | EffectKind::MultibandMaximizer
                | EffectKind::OneKnob
                | EffectKind::BassHarmonics
                | EffectKind::Exciter
                | EffectKind::Waveshaper
                | EffectKind::Overdrive
                | EffectKind::GuitarRack
                | EffectKind::DriveChain
                | EffectKind::PerformanceRack
                | EffectKind::Convolver
                | EffectKind::FrequencyShifter
                | EffectKind::PitchShift
                | EffectKind::PitchCorrect
        );
        let can_delay = has_latency || kind == EffectKind::StereoMatrix;
        assert_eq!(effect.latency_samples() > 0, has_latency, "{}", kind.name());
        assert_eq!(
            effect.latency_samples(),
            kind.default_params().latency_samples(RATE),
            "{}",
            kind.name()
        );
        assert_eq!(
            effect.max_latency_samples(RATE) > 0,
            can_delay,
            "{}",
            kind.name()
        );
        assert!(effect.max_latency_samples(RATE) >= effect.latency_samples());

        if kind == EffectKind::NoteEnvelope {
            let mut closed_left = noise(1, 0.3, 256);
            let mut closed_right = closed_left.clone();
            effect.process(&mut closed_left, &mut closed_right);
            assert!(
                closed_left
                    .iter()
                    .chain(&closed_right)
                    .all(|sample| *sample == 0.0)
            );
        }
        assert!(effect.set_params(&audible_params(kind)));
        let (mut left, mut right) = (noise(1, 0.3, 96_000), noise(2, 0.3, 96_000));
        effect.set_tempo(140.0);
        for (l, r) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
            effect.process(l, r);
        }
        assert!(
            peak(&left) > 0.0 && left.iter().chain(&right).all(|sample| sample.is_finite()),
            "{}",
            kind.name()
        );
        effect.reset();
        assert!(effect.tail_samples() >= effect.latency_samples());
    }
}

#[test]
fn any_instrument_plays_through_the_same_calls() {
    let kind = InstrumentKind::SubtractiveSynth;
    let mut instrument = AnyInstrument::new(&kind.default_params());
    instrument.prepare(RATE, 256);
    assert_eq!(instrument.kind(), kind);
    assert!(instrument.set_params(&kind.default_params()));
    instrument.set_tempo(120.0);
    instrument.note_on(60, 1.0);
    instrument.note_on(67, 1.0);
    assert_eq!(instrument.active_voices(), 2);
    let (mut left, mut right) = (vec![9.0; 4_800], vec![9.0; 4_800]);
    instrument.process(&mut left, &mut right);
    // The buffers are overwritten, not added to.
    assert!(peak(&left) < 2.0 && rms(&left) > 0.01);
    instrument.note_off(60);
    instrument.all_notes_off();
    instrument.process(&mut left, &mut right);
    assert_eq!(instrument.active_voices(), 0);
    instrument.reset();
    assert_eq!(instrument.latency_samples(), 12);
    assert!(instrument.tail_samples() > 0);
}

#[test]
fn processors_that_were_never_prepared_do_not_panic() {
    // Using one before `prepare` is a host bug and the audio is not
    // meaningful, but it must not bring the audio thread down.
    for kind in EffectKind::ALL {
        let mut effect = AnyEffect::new(&kind.default_params());
        let (mut left, mut right) = (noise(1, 0.5, 3_000), noise(2, 0.5, 3_000));
        effect.process(&mut left, &mut right);
        effect.reset();
        effect.process(&mut left, &mut right);

        // A slot that was never prepared has nowhere to keep the dry
        // signal, so it leaves the audio alone.
        let mut slot = EffectSlot::new(AnyEffect::new(&kind.default_params()));
        let input = noise(3, 0.5, 1_000);
        let (mut left, mut right) = (input.clone(), input.clone());
        slot.process(&mut left, &mut right);
        assert!(left == input && right == input);
    }
    let mut instrument = AnyInstrument::new(&InstrumentKind::SubtractiveSynth.default_params());
    instrument.note_on(60, 1.0);
    let (mut left, mut right) = (silence(3_000), silence(3_000));
    instrument.process(&mut left, &mut right);
    assert!(peak(&left) > 0.0);
}

#[test]
fn a_slot_that_is_switched_off_passes_its_input_untouched() {
    let input = noise(3, 0.5, 8_000);
    let mut slot = slot(loud_eq());
    slot.set_enabled(false);
    assert!(!slot.is_enabled());
    let (left, right) = run(&mut slot, &input);
    assert!(left == input && right == input);
    assert_eq!(slot.tail_samples(), 0);
}

#[test]
fn switching_a_slot_on_and_off_crossfades() {
    let input = sine(440.0, 0.2, 48_000, RATE);
    let mut slot = slot(loud_eq());
    let (mut left, mut right) = (input.clone(), input.clone());
    for (index, (left, right)) in left.chunks_mut(100).zip(right.chunks_mut(100)).enumerate() {
        if index % 60 == 30 {
            slot.set_enabled(index % 120 != 30);
        }
        slot.process(left, right);
    }
    // On, the tone is 12 dB up. Off, it is as it came in. In between it
    // never jumps.
    let natural = std::f32::consts::TAU * 440.0 / RATE * peak(&left);
    assert!(
        steepest(&left) < 1.5 * natural,
        "{} against {natural}",
        steepest(&left)
    );
    assert!((db(rms(&left[..2_000]) / rms(&input[..2_000])) - 12.0).abs() < 0.2);
    assert!(left[4_000..5_900] == input[4_000..5_900]);
    assert!((db(rms(&left[10_000..11_000]) / rms(&input[10_000..11_000])) - 12.0).abs() < 0.2);
}

#[test]
fn an_effect_starts_clean_when_its_slot_is_switched_back_on() {
    let params = ReverbParams {
        mix: 1.0,
        decay_s: 5.0,
        ..ReverbParams::default()
    };
    let mut slot = slot(EffectParams::Reverb(params));
    run(&mut slot, &noise(4, 0.8, 24_000));
    slot.set_enabled(false);
    run(&mut slot, &silence(4_800));
    // The long tail that was ringing when the slot was switched off does
    // not come back with it.
    slot.set_enabled(true);
    let (left, _) = run(&mut slot, &silence(24_000));
    assert_eq!(peak(&left), 0.0);
}

#[test]
fn mix_blends_the_input_with_the_effect() {
    let input = sine(440.0, 0.2, 9_600, RATE);
    let mut slot = slot(loud_eq());
    slot.set_mix(0.5);
    let (left, _) = run(&mut slot, &input);
    // Half the input plus half of it boosted by 12 dB. A bell does not
    // shift the phase at its centre, so the two add in step.
    let expected = db(0.5 + 0.5 * 10.0_f64.powf(12.0 / 20.0));
    assert!((db(rms(&left[4_800..]) / rms(&input[4_800..])) - expected).abs() < 0.1);

    slot.set_mix(f32::NAN);
    let (left, _) = run(&mut slot, &input);
    assert!((db(rms(&left[4_800..]) / rms(&input[4_800..])) - 12.0).abs() < 0.2);
}

#[test]
fn a_slot_keeps_the_same_delay_on_off_and_in_between() {
    let params = LimiterParams {
        lookahead_ms: 5.0,
        ceiling_db: 0.0,
        ..LimiterParams::default()
    };
    let input = noise(6, 0.4, 6_000);
    for (enabled, mix) in [(true, 1.0), (false, 1.0), (true, 0.3)] {
        let mut slot = slot(EffectParams::Limiter(params));
        slot.set_enabled(enabled);
        slot.set_mix(mix);
        assert_eq!(slot.latency_samples(), 240);
        let (left, _) = run(&mut slot, &input);
        assert!(left[..240].iter().all(|sample| *sample == 0.0));
        for index in 0..input.len() - 240 {
            assert!(
                (left[index + 240] - input[index]).abs() < 1.0e-6,
                "enabled {enabled}, mix {mix}: sample {index}"
            );
        }
    }
}

#[test]
fn a_slot_shields_its_effect_from_bad_input() {
    for kind in EffectKind::ALL {
        let mut slot = slot(audible_params(kind));
        let mut left = noise(7, 0.5, 2_048);
        let mut right = noise(8, 0.5, 2_048);
        left[10] = f32::NAN;
        left[500] = f32::INFINITY;
        right[20] = f32::NEG_INFINITY;
        right[900] = 1.0e30;
        slot.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        // And the effect still works afterwards.
        let (left, _) = run(&mut slot, &noise(9, 0.5, 96_000));
        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(
            rms(&left[90_000..]) > 0.01 && peak(&left[90_000..]) < 4.0,
            "{}",
            kind.name()
        );
    }
}

#[test]
fn a_slot_passes_parameters_and_tempo_through() {
    let mut slot = slot(EffectParams::Delay(DelayParams {
        mix: 1.0,
        ..DelayParams::default()
    }));
    assert!(!slot.set_params(&EffectParams::Compressor(CompressorParams::default())));
    assert_eq!(slot.effect().kind(), EffectKind::Delay);
    slot.set_tempo(240.0);
    // An eighth note at 240 bpm is 125 ms.
    let mut input = silence(8_000);
    input[0] = 1.0;
    let (left, _) = run(&mut slot, &input);
    assert_eq!(left.iter().position(|sample| *sample != 0.0), Some(6_000));
    assert_eq!(slot.into_effect().kind(), EffectKind::Delay);
}

#[test]
fn switching_a_limiter_back_on_does_not_open_with_a_hole() {
    // A steady tone through a limiter that has nothing to do, so that on
    // or off the slot puts out the same tone, 240 samples late.
    let params = EffectParams::Limiter(LimiterParams {
        ceiling_db: 0.0,
        input_gain_db: 0.0,
        ..LimiterParams::default()
    });
    let mut slot = slot(params);
    let signal = sine(220.0, 0.5, 48_000, RATE);
    let mut output = Vec::new();
    for (index, piece) in signal.chunks(4_800).enumerate() {
        // Off after 0.3 s, long enough for the effect to stop running, and
        // on again after 0.6 s.
        match index {
            3 => slot.set_enabled(false),
            6 => slot.set_enabled(true),
            _ => {}
        }
        output.extend(run(&mut slot, piece).0);
    }
    // Were the limiter faded in while its look-ahead was still empty, its
    // output would burst in half way through the fade.
    for index in 240..signal.len() {
        let expected = signal[index - 240];
        assert!(
            (output[index] - expected).abs() < 1.0e-5,
            "sample {index}: {} for {expected}",
            output[index]
        );
    }
    assert!(steepest(&output) <= steepest(&signal) * 1.001);
}

#[test]
fn the_dry_signal_follows_a_change_of_latency_without_a_click() {
    // Around a limiter with nothing to do, a slot puts out its input late
    // by the look-ahead, whether it is on, off or half mixed. When the
    // look-ahead moves, the limiter crossfades to its new delay, and the
    // dry signal has to do just the same.
    let idle = |lookahead_ms: f32| {
        EffectParams::Limiter(LimiterParams {
            ceiling_db: 0.0,
            input_gain_db: 0.0,
            lookahead_ms,
            ..LimiterParams::default()
        })
    };
    let signal = sine(173.0, 0.5, 28_800, RATE);
    for (enabled, mix) in [(true, 1.0), (true, 0.5), (true, 0.0), (false, 1.0)] {
        let mut slot = slot(idle(5.0));
        slot.set_enabled(enabled);
        slot.set_mix(mix);
        let mut output = Vec::new();
        for (index, piece) in signal.chunks(9_600).enumerate() {
            match index {
                1 => assert!(slot.set_params(&idle(15.0))),
                2 => assert!(slot.set_params(&idle(1.0))),
                _ => {}
            }
            output.extend(run(&mut slot, piece).0);
        }
        let what = format!("enabled {enabled}, mix {mix}");
        // A crossfade of 240 samples between two delays of a signal that
        // spans one full scale adds at most a 240th of it to the slope.
        let crossfade = 1.0 / 240.0;
        assert!(steepest(&output) <= steepest(&signal) + crossfade, "{what}");
        // Each stretch ends on its own latency: 240, 720 and 48 samples.
        for (end, latency) in [(9_600, 240), (19_200, 720), (28_800, 48)] {
            let index = end - 1;
            let expected = signal[index - latency];
            assert!((output[index] - expected).abs() < 1.0e-5, "{what}");
        }
    }
}

#[test]
fn a_meter_that_saw_no_reduction_still_takes_a_later_one() {
    // A limiter with nothing to do, then the same limiter with work.
    let mut effect = AnyEffect::new(&EffectKind::Limiter.default_params());
    effect.prepare(RATE, 256);
    let meter = effect.gain_reduction().expect("a limiter has a meter");
    let quiet = sine(220.0, 0.1, 4_800, RATE);
    let loud = sine(220.0, 2.0, 4_800, RATE);
    for signal in [quiet, loud] {
        let (mut left, mut right) = (signal.clone(), signal);
        for (left, right) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
            effect.process(left, right);
        }
    }
    assert!(meter.take_db() > 5.0);
    assert_eq!(meter.take_db(), 0.0);
}

#[test]
fn settings_alone_say_what_latency_a_prepared_processor_has() {
    for rate in [22_050.0, 44_100.0, 48_000.0, 96_000.0] {
        for kind in EffectKind::ALL {
            let mut effect = AnyEffect::new(&kind.default_params());
            effect.prepare(rate, 256);
            assert_eq!(
                kind.default_params().latency_samples(rate),
                effect.latency_samples(),
                "{kind:?} at {rate}"
            );
            assert_eq!(
                kind.max_latency_samples(rate),
                effect.max_latency_samples(rate)
            );
        }
        for lookahead_ms in [0.0, 0.1, 0.37, 5.0, 12.5, 20.0, 90.0, f32::NAN] {
            let params = EffectParams::Limiter(LimiterParams {
                lookahead_ms,
                ..LimiterParams::default()
            });
            let mut effect = AnyEffect::new(&params);
            effect.prepare(rate, 256);
            let latency = params.latency_samples(rate);
            assert_eq!(latency, effect.latency_samples(), "{lookahead_ms} ms");
            assert!(latency <= EffectKind::Limiter.max_latency_samples(rate));
        }
        for kind in InstrumentKind::ALL {
            let mut instrument = AnyInstrument::new(&kind.default_params());
            instrument.prepare(rate, 256);
            assert_eq!(
                kind.default_params().latency_samples(rate),
                instrument.latency_samples()
            );
        }
    }
}
