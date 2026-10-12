use crate::realtime::allocator_calls;
use std::sync::Arc;
use windfall_core::AudioBuffer;
use windfall_dsp::sample_bank::{LoadedZone, SampleBank, SampleZone};
use windfall_dsp::{AnyInstrument, InstrumentKind, NoteExpression, NoteInstanceId};

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
    for kind in [InstrumentKind::MultiSampler, InstrumentKind::SamplePlayer,
        InstrumentKind::SoundFont, InstrumentKind::PadBank, InstrumentKind::PianoKeys, InstrumentKind::TextVoice] {
        let bank = || Arc::new(SampleBank::new(vec![LoadedZone {
            mapping: SampleZone { choke_group: 1, ..Default::default() },
            audio: AudioBuffer::from_interleaved(48000, 1, vec![0.25; 16384]),
        }]).unwrap());
        let params = kind.default_params();
        let mut old = AnyInstrument::new(&params);
        old.prepare(48000.0, 64);
        old.install_sample_bank(bank());
        let mut incoming = AnyInstrument::new(&params);
        incoming.prepare(48000.0, 64);
        incoming.install_sample_bank(bank());
        let mut left = [0.0; 64]; let mut right = [0.0; 64];
        old.note_on_instance(NoteInstanceId(1), 60, 1.0, 0.0, NoteExpression::default());
        old.process(&mut left, &mut right);
        let mut retired = None;
        let calls = allocator_calls(|| {
            assert!(incoming.inherit_sample_bank_state(&mut old));
            incoming.process(&mut left, &mut right);
            incoming.note_on_instance(NoteInstanceId(2), 60, 0.5, 0.0, NoteExpression::default());
            incoming.process(&mut left, &mut right);
            incoming.all_notes_off();
            for _ in 0..10 { incoming.process(&mut left, &mut right); }
            for _ in 0..257 {
                if let Some(bank) = incoming.take_unused_sample_bank() { retired = Some(bank); }
            }
        });
        assert_eq!(calls, 0, "{kind:?} replacement/tail/sweep callback allocator calls");
        assert!(retired.is_some());
        drop(retired);
    }
}
