use std::sync::Arc;
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{PlaybackAsset, PlaybackCurve, SliceRegion};
use windfall_dsp::{AnyInstrument, InstrumentKind, NoteExpression, NoteInstanceId};
#[test]
fn populated_sample_instrument_callbacks_never_allocate_or_free() {
    let source = AudioBuffer::from_interleaved(48000, 2, vec![0.2, -0.3].repeat(96000));
    let asset = Arc::new(
        PlaybackAsset::new(
            [Some(source), None],
            vec![SliceRegion::default()],
            PlaybackCurve::default(),
            vec![],
        )
        .unwrap(),
    );
    for kind in [
        InstrumentKind::SliceMap,
        InstrumentKind::SliceDeck,
        InstrumentKind::GrainCloud,
        InstrumentKind::WaveRide,
    ] {
        let mut params = kind.default_params();
        let mut i = AnyInstrument::new(&params);
        i.prepare(48000.0, 512);
        assert!(i.set_playback_asset(asset.clone()));
        let mut l = [0.0; 512];
        let mut r = [0.0; 512];
        let calls = super::realtime::allocator_calls(|| {
            i.reset();
            i.set_params(&params);
            i.set_tempo(90.0);
            for n in 1..=32 {
                i.note_on_instance(NoteInstanceId(n), 60, 1.0, 0.0, NoteExpression::default());
            }
            for _ in 0..20 {
                i.process(&mut l, &mut r);
            }
            // Retained marker/per-slice/curve indices must remain safe even
            // when an explicit edit writes the original default value.
            for (index, info) in kind.descriptors().iter().enumerate() {
                params.set(index, info.default);
                i.set_params(&params);
                i.note_on_instance(NoteInstanceId(32), 60, 1.0, 0.0, NoteExpression::default());
                i.process(&mut l, &mut r);
            }
            params.set(1, kind.descriptors()[1].max);
            i.set_params(&params);
            i.set_note_pitch(NoteInstanceId(32), 72.0);
            i.set_note_gain(NoteInstanceId(32), 0.5);
            i.note_off_instance(NoteInstanceId(32), 60);
            i.all_notes_off();
            i.process(&mut l, &mut r);
            i.reset();
        });
        assert_eq!(calls, 0, "{kind:?}");
    }
}
