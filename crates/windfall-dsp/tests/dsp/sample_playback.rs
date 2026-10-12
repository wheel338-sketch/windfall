use std::sync::Arc;
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{
    MAX_ASSET_SLICES, MAX_CURVE_POINTS, MAX_GRAINS_PER_VOICE, MAX_POLYPHONY, PlaybackAsset,
    PlaybackCurve, PlaybackPoint, SliceRegion,
};
use windfall_dsp::{AnyInstrument, InstrumentKind, NoteExpression, NoteInstanceId};
#[test]
fn populated_sample_instrument_callbacks_never_allocate_or_free() {
    let source = AudioBuffer::from_interleaved(48000, 2, [0.2, -0.3].repeat(96000));
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

/// Offline render at the declared storage limits: 16 simultaneous voices,
/// 128 overlapping slices, 128 key curves of 256 points, and Cloud's 32 grain
/// slots per voice (250 ms at 128 Hz). Source preparation is outside the watch.
fn maximum_load_render(kind: InstrumentKind) {
    const RATE: u32 = 48_000;
    const BLOCK: usize = 128;
    const BLOCKS: usize = 750; // Two seconds; long enough to fill all grain slots.
    assert_eq!(MAX_POLYPHONY, 16);
    assert_eq!(MAX_GRAINS_PER_VOICE, 32);
    let source = AudioBuffer::from_interleaved(
        RATE,
        2,
        (0..RATE * 3)
            .flat_map(|frame| {
                let value = (frame as f32 * 0.013).sin() * 0.5;
                [value, -value * 0.7]
            })
            .collect(),
    );
    let curve = PlaybackCurve {
        points: (0..MAX_CURVE_POINTS)
            .map(|point| PlaybackPoint {
                time: point as f32 / (MAX_CURVE_POINTS - 1) as f32,
                value: if point % 2 == 0 { 0.2 } else { 0.8 },
            })
            .collect(),
        ..Default::default()
    };
    let asset = Arc::new(
        PlaybackAsset::new(
            [Some(source.clone()), Some(source)],
            (0..MAX_ASSET_SLICES)
                .map(|slice| SliceRegion {
                    deck: if kind == InstrumentKind::SliceDeck {
                        (slice % 2) as u8
                    } else {
                        0
                    },
                    gain: 2.0,
                    reverse: slice % 2 != 0,
                    ..Default::default()
                })
                .collect(),
            curve.clone(),
            (0..128).map(|key| (key, curve.clone())).collect(),
        )
        .unwrap(),
    );
    let mut params = kind.default_params();
    for (index, info) in kind.descriptors().iter().enumerate() {
        // Keep the append-only legacy marker/pad/curve masks untouched.
        let value = match info.id {
            "baseKey" => Some(0.0),
            "rootKey" => Some(127.0),
            "grainSizeMs" | "density" | "durationMs" | "level" => Some(info.max),
            _ => None,
        };
        if let Some(value) = value {
            params.set(index, value);
        }
    }
    let mut instrument = AnyInstrument::new(&params);
    instrument.prepare(RATE as f32, BLOCK);
    assert!(instrument.set_playback_asset(asset));
    let mut first = vec![0.0f32; BLOCKS * BLOCK * 2];
    let mut repeated = vec![0.0f32; first.len()];
    let mut left = [0.0; BLOCK];
    let mut right = [0.0; BLOCK];
    let calls = super::realtime::allocator_calls(|| {
        for output in [&mut first, &mut repeated] {
            instrument.reset();
            instrument.set_params(&params);
            // Seventeen identified notes exercise deterministic stealing at 16.
            // High keys also reach the end of the slice and per-key curve tables.
            for voice in 0..=MAX_POLYPHONY {
                let key = 112 + (voice % MAX_POLYPHONY) as u8;
                instrument.note_on_instance(
                    NoteInstanceId(voice as u64 + 1),
                    key,
                    1.0,
                    0.0,
                    NoteExpression::default(),
                );
            }
            assert_eq!(instrument.active_voices(), MAX_POLYPHONY);
            for block in output.chunks_exact_mut(BLOCK * 2) {
                instrument.process(&mut left, &mut right);
                for (frame, pair) in block.chunks_exact_mut(2).enumerate() {
                    pair[0] = left[frame];
                    pair[1] = right[frame];
                }
            }
            if let AnyInstrument::GrainCloud(cloud) = &instrument {
                // 128 events/sec for each of sixteen continuously held voices.
                assert_eq!(cloud.grains_spawned(), (256 * MAX_POLYPHONY) as u64);
                assert_eq!(cloud.grains_dropped(), 0);
                assert_eq!(cloud.voices_stolen(), 1);
            }
            instrument.all_notes_off();
            for _ in 0..400 {
                instrument.process(&mut left, &mut right);
            }
            assert_eq!(instrument.active_voices(), 0);
        }
    });
    assert_eq!(calls, 0, "{kind:?} maximum-load callback allocation/free");
    assert_eq!(first, repeated, "{kind:?} reset/seed determinism");
    // |source| <= 0.5, level <= 1, pan <= 1, output scale = 0.25.
    // Slice gain <= 2; Cloud's 32 Hann-windowed grains divide by sqrt(32).
    let voice_bound = MAX_POLYPHONY as f32 * 0.5 * 0.25;
    let bound = voice_bound
        * match kind {
            InstrumentKind::SliceMap | InstrumentKind::SliceDeck => 2.0,
            InstrumentKind::GrainCloud => (MAX_GRAINS_PER_VOICE as f32).sqrt(),
            _ => 1.0,
        };
    assert!(
        first.iter().all(|v| v.is_finite() && v.abs() <= bound + 1e-4),
        "{kind:?} bounded PCM"
    );
    assert!(
        first.iter().any(|v| v.abs() > 1e-4),
        "{kind:?} must render populated voices"
    );
    assert!(
        left.iter().chain(&right).all(|v| *v == 0.0),
        "{kind:?} released tail"
    );
}

#[test]
fn slice_map_maximum_load_offline_48khz_128_frames_is_bounded_deterministic_and_allocation_free() {
    maximum_load_render(InstrumentKind::SliceMap);
}

#[test]
fn slice_deck_maximum_load_offline_48khz_128_frames_is_bounded_deterministic_and_allocation_free() {
    maximum_load_render(InstrumentKind::SliceDeck);
}

#[test]
fn grain_cloud_maximum_load_offline_48khz_128_frames_is_bounded_deterministic_and_allocation_free() {
    maximum_load_render(InstrumentKind::GrainCloud);
}

#[test]
fn wave_ride_maximum_load_offline_48khz_128_frames_is_bounded_deterministic_and_allocation_free() {
    maximum_load_render(InstrumentKind::WaveRide);
}
