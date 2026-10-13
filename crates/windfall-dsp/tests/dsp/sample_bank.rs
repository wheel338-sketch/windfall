use crate::realtime::allocator_calls;
use std::sync::Arc;
use windfall_core::AudioBuffer;
use windfall_dsp::sample_bank::{LoadedZone, SampleBank, SampleZone};
use windfall_dsp::{AnyInstrument, InstrumentKind, NoteExpression, NoteInstanceId};

#[test]
fn sf2_real_stereo_24_bit_modulated_voice_controls_chokes_reset_and_handover_are_allocator_free() {
    let bytes = super::sf2_fixture::bytes();
    let budget = Arc::new(windfall_dsp::sample_bank::BankBudget::default());
    let bank = Arc::new(
        windfall_dsp::sample_bank::soundfont::parse_soundfont_preset_budgeted(
            &bytes, 0, 0, &budget,
        )
        .unwrap(),
    );
    assert_eq!(bank.zones()[0].audio.channels(), 2);
    let mut params = windfall_dsp::sample_bank::SoundFontParams::default();
    let mut old = AnyInstrument::new(&windfall_dsp::InstrumentParams::SoundFont(params));
    let mut new = AnyInstrument::new(&windfall_dsp::InstrumentParams::SoundFont(params));
    old.prepare(48000.0, 64);
    new.prepare(48000.0, 64);
    old.install_sample_bank(bank.clone());
    new.install_sample_bank(bank);
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    let calls = allocator_calls(|| {
        for i in 0..320 {
            params.mod_wheel = i % 128;
            params.pressure = (i * 3) % 128;
            params.pitch_wheel = (i * 41) % 16384;
            params.controllers[0] = windfall_dsp::MidiOutController {
                number: 20,
                value: i % 128,
            };
            params.controllers[1] = windfall_dsp::MidiOutController {
                number: 21,
                value: (i * 7) % 128,
            };
            old.set_params(&windfall_dsp::InstrumentParams::SoundFont(params));
            old.soundfont_midi(windfall_dsp::sample_bank::SoundFontMidi::PitchWheel(
                (i * 41) as u16,
            ));
            old.soundfont_midi(windfall_dsp::sample_bank::SoundFontMidi::PolyPressure {
                key: 60,
                value: (i % 128) as u8,
            });
            old.note_on_instance(
                NoteInstanceId(i as u64),
                60,
                0.75,
                0.0,
                NoteExpression::default(),
            );
            old.process(&mut left, &mut right);
            old.note_off_instance(NoteInstanceId(i as u64), 60);
        }
        assert!(new.inherit_sample_bank_state(&mut old));
        new.process(&mut left, &mut right);
        new.all_notes_off();
        new.process(&mut left, &mut right);
        new.reset();
    });
    assert_eq!(calls, 0);
    assert_eq!(new.active_voices(), 0);
    assert!(left.iter().all(|v| v.is_finite()));
}

#[test]
fn asset_instrument_callbacks_never_allocate_or_free_and_sources_retire_off_audio() {
    for kind in [
        InstrumentKind::MultiSampler,
        InstrumentKind::SamplePlayer,
        InstrumentKind::SoundFont,
        InstrumentKind::PadBank,
        InstrumentKind::PianoKeys,
        InstrumentKind::TextVoice,
    ] {
        let bank = Arc::new(
            SampleBank::new(vec![LoadedZone {
                mapping: SampleZone::default(),
                audio: AudioBuffer::from_interleaved(48000, 1, vec![0.25; 8192]),
            }])
            .unwrap(),
        );
        let weak = Arc::downgrade(&bank);
        let mut params = kind.default_params();
        let mut unit = AnyInstrument::new(&params);
        unit.prepare(48000.0, 64);
        assert!(unit.install_sample_bank(bank.clone()));
        drop(bank);
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        let calls = allocator_calls(|| {
            for id in 0..320 {
                unit.note_on_instance(NoteInstanceId(id), 60, 1.0, 0.0, NoteExpression::default());
                params.set(0, 0.25);
                unit.set_params(&params);
                unit.set_note_pitch(NoteInstanceId(id), 60.5);
                unit.set_note_gain(NoteInstanceId(id), 0.5);
                unit.process(&mut left, &mut right);
                unit.note_off_instance(NoteInstanceId(id), 60);
            }
            unit.all_notes_off();
            for _ in 0..20 {
                unit.process(&mut left, &mut right);
            }
            unit.reset();
        });
        assert_eq!(calls, 0, "{kind:?} callback allocator calls");
        assert_eq!(unit.active_voices(), 0);
        assert!(weak.upgrade().is_some(), "voice shutdown keeps owner alive");
        drop(unit);
        assert!(
            weak.upgrade().is_none(),
            "off-audio owner destruction releases asset"
        );
    }
}

#[test]
fn replacement_chokes_and_bank_retirement_transfer_never_allocate_or_free_on_audio() {
    for kind in [
        InstrumentKind::MultiSampler,
        InstrumentKind::SamplePlayer,
        InstrumentKind::SoundFont,
        InstrumentKind::PadBank,
        InstrumentKind::PianoKeys,
        InstrumentKind::TextVoice,
    ] {
        let bank = || {
            Arc::new(
                SampleBank::new(vec![LoadedZone {
                    mapping: SampleZone {
                        choke_group: 1,
                        ..Default::default()
                    },
                    audio: AudioBuffer::from_interleaved(48000, 1, vec![0.25; 16384]),
                }])
                .unwrap(),
            )
        };
        let params = kind.default_params();
        let mut old = AnyInstrument::new(&params);
        old.prepare(48000.0, 64);
        old.install_sample_bank(bank());
        let mut incoming = AnyInstrument::new(&params);
        incoming.prepare(48000.0, 64);
        incoming.install_sample_bank(bank());
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        old.note_on_instance(NoteInstanceId(1), 60, 1.0, 0.0, NoteExpression::default());
        old.process(&mut left, &mut right);
        let mut retired = None;
        let calls = allocator_calls(|| {
            assert!(incoming.inherit_sample_bank_state(&mut old));
            incoming.process(&mut left, &mut right);
            incoming.note_on_instance(NoteInstanceId(2), 60, 0.5, 0.0, NoteExpression::default());
            incoming.process(&mut left, &mut right);
            incoming.all_notes_off();
            for _ in 0..10 {
                incoming.process(&mut left, &mut right);
            }
            for _ in 0..257 {
                if let Some(bank) = incoming.take_unused_sample_bank() {
                    retired = Some(bank);
                }
            }
        });
        assert_eq!(
            calls, 0,
            "{kind:?} replacement/tail/sweep callback allocator calls"
        );
        assert!(retired.is_some());
        drop(retired);
    }
}

#[test]
fn multisampler_modulation_effect_changes_and_reset_are_allocator_free() {
    let mut unit = AnyInstrument::new(&InstrumentKind::MultiSampler.default_params());
    unit.prepare(48000.0, 64);
    unit.install_sample_bank(Arc::new(
        SampleBank::new(vec![LoadedZone {
            audio: AudioBuffer::from_interleaved(48000, 1, vec![0.25; 16384]),
            mapping: SampleZone {
                modulation: windfall_dsp::sample_bank::ZoneModulation {
                    lfos: [windfall_dsp::sample_bank::ZoneLfo {
                        pitch_cents: 100.0,
                        filter_cents: 1200.0,
                        amplitude: 0.5,
                        ..Default::default()
                    }; 2],
                    ..Default::default()
                },
                ..Default::default()
            },
        }])
        .unwrap(),
    ));
    let mut params = windfall_dsp::sample_bank::MultiSamplerParams::default();
    params.chorus.mix = 0.5;
    params.delay.mix = 0.3;
    params.reverb.mix = 0.2;
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    let calls = allocator_calls(|| {
        unit.set_params(&windfall_dsp::InstrumentParams::MultiSampler(params));
        unit.set_tempo(90.0);
        unit.note_on_instance(NoteInstanceId(1), 60, 1.0, 0.0, NoteExpression::default());
        for _ in 0..64 {
            unit.process(&mut left, &mut right);
        }
        unit.note_off_instance(NoteInstanceId(1), 60);
        unit.reset();
    });
    assert_eq!(calls, 0);
    assert_eq!(unit.active_voices(), 0);
}
