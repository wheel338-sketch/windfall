//! E5 public live engine acceptance. Written, not executed by the worker.
use crate::realtime::allocator_calls;
use crate::support::{Rig, left, level, peak, sine};
use windfall_dsp::spectral::{
    ConvolverParams, FrequencyShifterParams, PitchCorrectParams, PitchShiftParams, VocoderParams,
};
use windfall_project::{ConvolverImpulse, EffectParams, Send};
const RATE: u32 = 48000;
fn spectral_rig(params: EffectParams) -> Rig {
    let mut rig = Rig::new();
    let track = rig.track();
    let source = rig.channel_on(sine(RATE, 440.0, 3.0), track);
    rig.note(source, 0, 3840);
    rig.effect(track, params);
    rig
}
#[test]
fn spectral_live_processing_and_parameter_changes_never_allocate_or_free() {
    for params in [
        EffectParams::Convolver(ConvolverParams::default()),
        EffectParams::FrequencyShifter(FrequencyShifterParams::default()),
        EffectParams::PitchShift(PitchShiftParams::default()),
        EffectParams::PitchCorrect(PitchCorrectParams::default()),
        EffectParams::Vocoder(VocoderParams::default()),
    ] {
        let mut rig = spectral_rig(params);
        let effect = rig.project.mixer.tracks[1].effects[0].id;
        if matches!(params, EffectParams::Convolver(_)) {
            rig.effect_mut(effect).impulse = Some(ConvolverImpulse {
                name: "Own IR".into(),
                sample_rate: RATE,
                group_delay: 0,
                eq_gains: None,
                frames: vec![[0.01, 0.02]; 8192],
            });
        }
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let mut output = [0.0; 512];
        for _ in 0..80 {
            assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
        }
        let changed = params.kind().descriptors()[0].max;
        let mut next = params;
        assert!(next.set(0, changed));
        rig.effect_mut(effect).params = next;
        controller.set_project(&rig.project, &rig.pool);
        for _ in 0..32 {
            assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
        }
        controller.stop();
        assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    }
}
#[test]
fn spectral_processors_are_live_partition_invariant() {
    for params in [
        EffectParams::FrequencyShifter(FrequencyShifterParams {
            shift_hz: 317.0,
            mix: 1.0,
        }),
        EffectParams::PitchShift(PitchShiftParams {
            semitones: 7.0,
            formant_semitones: 3.0,
            ..Default::default()
        }),
        EffectParams::PitchCorrect(PitchCorrectParams::default()),
    ] {
        let rig = spectral_rig(params);
        assert_eq!(rig.play(RATE, 48000, 1), rig.play(RATE, 48000, 191));
    }
}
#[test]
fn classic_and_synth_vocoder_use_a_real_mixer_key() {
    for synth in [false, true] {
        let mut rig = Rig::new();
        let carrier_track = rig.track();
        let key_track = rig.track();
        let carrier = rig.channel_on(sine(RATE, 880.0, 2.0), carrier_track);
        rig.note(carrier, 0, 3840);
        let key = rig.channel_on(sine(RATE, 880.0, 2.0), key_track);
        rig.note(key, 0, 3840);
        rig.track_mut(key_track).output = None;
        rig.track_mut(key_track).sidechains.push(Send {
            target: carrier_track,
            gain: 1.0,
        });
        rig.effect(
            carrier_track,
            EffectParams::Vocoder(VocoderParams {
                band_count: if synth { 32 } else { 16 },
                synth_carrier: synth,
                ..Default::default()
            }),
        );
        let sounding = rig.play(RATE, 48000, 127);
        assert!(peak(&sounding[16000..]) > 0.001);
        assert_eq!(sounding, rig.play(RATE, 48000, 1));
        rig.track_mut(key_track).sidechains.clear();
        let silent = rig.play(RATE, 48000, 127);
        assert!(silent[16000..].iter().all(|v| *v == 0.0));
    }
}
#[test]
fn midi_harmony_source_changes_audio_and_short_notes_are_partition_invariant() {
    let mut rig = Rig::new();
    let track = rig.track();
    let source = rig.channel_on(sine(RATE, 440.0, 3.0), track);
    rig.note(source, 0, 3840);
    let midi = rig.channel(level(RATE, 0.0, 3.0));
    rig.note(midi, 480, 960).key = 72;
    rig.note(midi, 1440, 40).key = 76;
    let effect = rig.effect(
        track,
        EffectParams::PitchCorrect(PitchCorrectParams {
            speed_ms: 0.0,
            harmony_channel: midi.0,
            harmony_mix: 1.0,
            ..Default::default()
        }),
    );
    let harmony = rig.play(RATE, 72000, 1);
    assert_eq!(harmony, rig.play(RATE, 72000, 251));
    rig.effect_mut(effect).params = EffectParams::PitchCorrect(PitchCorrectParams {
        speed_ms: 0.0,
        ..Default::default()
    });
    let plain = rig.play(RATE, 72000, 251);
    let difference: f32 = harmony
        .iter()
        .zip(&plain)
        .map(|(a, b)| (a - b).powi(2))
        .sum();
    assert!(difference > 1.0);
}
#[test]
fn replacing_a_prepared_ir_changes_the_live_owner_without_callback_destruction() {
    let mut rig = spectral_rig(EffectParams::Convolver(ConvolverParams::default()));
    let effect = rig.project.mixer.tracks[1].effects[0].id;
    rig.effect_mut(effect).impulse = Some(ConvolverImpulse {
        name: "Silence".into(),
        sample_rate: RATE,
        group_delay: 0,
        eq_gains: None,
        frames: vec![[0.0; 2]; 4096],
    });
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut output = [0.0; 512];
    for _ in 0..40 {
        processor.process(&mut output);
    }
    assert!(output.iter().all(|v| *v == 0.0));
    rig.effect_mut(effect).impulse = Some(ConvolverImpulse {
        name: "Stereo identity".into(),
        sample_rate: RATE,
        group_delay: 0,
        eq_gains: None,
        frames: vec![[1.0, 0.5]],
    });
    controller.set_project(&rig.project, &rig.pool);
    for _ in 0..80 {
        assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    }
    assert!(peak(&left(&output)) > 0.1);
}

#[test]
fn native_convolver_matches_direct_fir_and_offline_compensation() {
    let mut input = vec![0.0; 12000];
    input[2048] = 0.7;
    input[4117] = -0.4;
    let mut rig = Rig::new();
    let source = rig.channel(windfall_core::AudioBuffer::from_interleaved(RATE, 1, input));
    rig.note(source, 0, 3840);
    let options = windfall_engine::RenderOptions {
        tail_secs: 0.1,
        block_frames: 73,
        ..Default::default()
    };
    let plain = windfall_engine::render(&rig.project, &rig.pool, &options, &mut |_| true);
    let effect = rig.effect(
        windfall_project::TrackId::MASTER,
        EffectParams::Convolver(ConvolverParams::default()),
    );
    let taps = [(0, 0.8), (128, -0.15), (8191, 0.1)];
    let mut frames = vec![[0.0; 2]; 8192];
    for (tap, gain) in taps {
        frames[tap] = [gain, gain * 0.5];
    }
    rig.effect_mut(effect).impulse = Some(ConvolverImpulse {
        name: "Direct FIR fixture".into(),
        sample_rate: RATE,
        group_delay: 0,
        eq_gains: None,
        frames,
    });
    let convolved = windfall_engine::render(&rig.project, &rig.pool, &options, &mut |_| true);
    let one = windfall_engine::render(
        &rig.project,
        &rig.pool,
        &windfall_engine::RenderOptions {
            block_frames: 1,
            ..options.clone()
        },
        &mut |_| true,
    );
    assert_eq!(convolved.samples(), one.samples());
    for n in 0..convolved.frames().min(20000) {
        let expected: f32 = taps
            .iter()
            .filter(|(tap, _)| n >= *tap)
            .map(|(tap, gain)| gain * plain.samples()[(n - tap) * 2])
            .sum();
        assert!(
            (convolved.samples()[n * 2] - expected).abs() < 1e-5,
            "frame {n}"
        );
        assert!((convolved.samples()[n * 2 + 1] - expected * 0.5).abs() < 1e-5);
    }
}

#[test]
fn linear_phase_eq_group_delay_is_in_public_mixer_compensation() {
    let mut rig = spectral_rig(EffectParams::Convolver(ConvolverParams::default()));
    let effect = rig.project.mixer.tracks[1].effects[0].id;
    let mut frames = vec![[0.0; 2]; 1025];
    frames[512] = [1.0; 2];
    rig.effect_mut(effect).impulse = Some(ConvolverImpulse {
        name: "Linear identity".into(),
        sample_rate: RATE,
        frames,
        group_delay: 512,
        eq_gains: Some([0.0; 3]),
    });
    let (mut processor, controller) = rig.processor(RATE);
    let mut output = [0.0; 512];
    processor.process(&mut output);
    assert_eq!(controller.latency_frames(), 640);
}
