use super::*;
use windfall_core::AudioBuffer;
const RATE: f32 = 8000.0;
const SCALE: f32 = std::f32::consts::FRAC_1_SQRT_2 * 0.25;
fn source() -> AudioBuffer {
    AudioBuffer::from_interleaved(
        RATE as u32,
        1,
        (0..16000).map(|i| i as f32 / 15999.0).collect(),
    )
}
fn asset(slices: Vec<SliceRegion>, curve: PlaybackCurve) -> Arc<PlaybackAsset> {
    Arc::new(PlaybackAsset::new([Some(source()), None], slices, curve, vec![]).unwrap())
}
fn render<I: Instrument>(instrument: &mut I, count: usize, block: usize) -> Vec<f32> {
    let mut left = vec![0.0; count];
    let mut right = vec![0.0; count];
    for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        instrument.process(l, r);
    }
    assert!(left.iter().chain(&right).all(|v| v.is_finite()));
    left
}
#[test]
fn immutable_sources_exceed_4096_frames_and_slice_boundaries_never_bleed() {
    let slices = vec![
        SliceRegion {
            start: 0.25,
            end: 0.375,
            ..Default::default()
        },
        SliceRegion {
            start: 0.75,
            end: 1.0,
            ..Default::default()
        },
    ];
    let mut i = SliceMap::default();
    i.prepare(RATE, 64);
    i.set_asset(asset(slices, PlaybackCurve::default()));
    i.set_params(&SliceMapParams {
        level: 1.0,
        ..Default::default()
    });
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 2100, 7);
    for (frame, &sample) in pcm.iter().enumerate().take(1992).skip(8) {
        let expected = (4000 + frame) as f32 / 15999.0 * SCALE;
        assert!((sample - expected).abs() < 1e-6);
    }
    assert!(pcm[2000..].iter().all(|v| *v == 0.0));
    i.note_on(59, 1.0);
    i.note_on(62, 1.0);
    assert_eq!(i.active_voices(), 0);
}
#[test]
fn reverse_pitch_gain_overlaps_reorder_and_second_deck_are_independent() {
    let slices = vec![
        SliceRegion {
            start: 0.25,
            end: 0.5,
            pitch: 12.0,
            gain: 0.5,
            reverse: true,
            ..Default::default()
        },
        SliceRegion {
            start: 0.375,
            end: 0.625,
            ..Default::default()
        },
        SliceRegion {
            deck: 1,
            ..Default::default()
        },
    ];
    let other = AudioBuffer::from_interleaved(RATE as u32, 2, [0.2, -0.4].repeat(16000));
    let a = Arc::new(
        PlaybackAsset::new(
            [Some(source()), Some(other)],
            slices,
            PlaybackCurve::default(),
            vec![],
        )
        .unwrap(),
    );
    let mut i = SliceDeck::default();
    i.prepare(RATE, 64);
    i.set_asset(a);
    i.set_params(&SliceDeckParams {
        level: 1.0,
        ..Default::default()
    });
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 2100, 31);
    for (frame, &sample) in pcm.iter().enumerate().take(1980).skip(8) {
        assert!((sample - (7999 - 2 * frame) as f32 / 15999.0 * SCALE * 0.5).abs() < 1e-6);
    }
    assert!(pcm[2000..].iter().all(|v| *v == 0.0));
    i.reset();
    i.note_on(60, 1.0);
    i.note_on(61, 1.0);
    assert_eq!(i.active_voices(), 2);
    i.reset();
    i.note_on(62, 1.0);
    let mut l = [0.0; 32];
    let mut r = [0.0; 32];
    i.process(&mut l, &mut r);
    assert!((l[16] - 0.2 * SCALE).abs() < 1e-6);
    assert!((r[16] + 0.4 * SCALE).abs() < 1e-6);
}
#[test]
fn seeded_grain_schedules_repeat_and_respect_density_capacity() {
    let mut i = GrainCloud::default();
    i.prepare(RATE, 64);
    i.set_asset(asset(
        vec![SliceRegion::default()],
        PlaybackCurve::default(),
    ));
    let mut p = GrainCloudParams {
        seed: 12345,
        density: 64.0,
        grain_size_ms: 40.0,
        ..Default::default()
    };
    i.set_params(&p);
    i.note_on(60, 1.0);
    let first = render(&mut i, 8000, 127);
    assert!((63..=65).contains(&i.grains_spawned()));
    assert_eq!(i.grains_dropped(), 0);
    i.reset();
    i.note_on(60, 1.0);
    assert_eq!(first, render(&mut i, 8000, 1));
    i.reset();
    p.seed += 1;
    i.set_params(&p);
    i.note_on(60, 1.0);
    assert_ne!(first, render(&mut i, 8000, 31));
    i.reset();
    p.density = 128.0;
    p.grain_size_ms = 250.0;
    i.set_params(&p);
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 16000, 17);
    assert!(i.grains_spawned() <= 257);
    assert!(pcm.iter().all(|v| v.abs() <= 4.0));
    for n in 1..=32 {
        i.note_on_instance(NoteInstanceId(n), 60, 1.0, 0.0, NoteExpression::default());
    }
    assert_eq!(i.active_voices(), MAX_POLYPHONY);
    assert!(i.voices_stolen() > 0);
    i.all_notes_off();
    let pcm = render(&mut i, 64, 1);
    assert!(pcm[32..].iter().all(|v| *v == 0.0));
}
#[test]
fn drawn_position_and_integrated_speed_curves_match_independent_reference() {
    let curve = PlaybackCurve {
        points: vec![
            PlaybackPoint {
                time: 0.0,
                value: 0.2,
            },
            PlaybackPoint {
                time: 0.5,
                value: 0.8,
            },
            PlaybackPoint {
                time: 1.0,
                value: 0.3,
            },
        ],
        ..Default::default()
    };
    let mut i = WaveRide::default();
    i.prepare(RATE, 64);
    i.set_asset(asset(vec![SliceRegion::default()], curve));
    i.set_params(&WaveRideParams {
        duration_ms: 1000.0,
        level: 1.0,
        ..Default::default()
    });
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 8000, 43);
    for frame in [100, 1000, 2000, 4000, 5000, 7000] {
        let t = frame as f64 / 8000.0;
        let expected = if t <= 0.5 {
            0.2 + 1.2 * t
        } else {
            0.8 - (t - 0.5)
        };
        assert!((pcm[frame] - expected as f32 * SCALE).abs() < 1e-5);
    }
    let speed = PlaybackCurve {
        mode: PlaybackCurveMode::Speed,
        origin: 0.4,
        points: vec![
            PlaybackPoint {
                time: 0.0,
                value: 1.0,
            },
            PlaybackPoint {
                time: 1.0,
                value: -1.0,
            },
        ],
    };
    for t in [0.0, 0.1, 0.25, 0.5, 0.9, 1.0] {
        assert!((speed.position(t) - (0.4 + t - t * t)).abs() < 1e-7);
    }
    i.reset();
    i.set_asset(asset(vec![SliceRegion::default()], speed));
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 8000, 13);
    assert!((pcm[4000] - 0.65 * SCALE).abs() < 1e-6);
}
#[test]
fn per_note_curves_release_and_partition_invariance() {
    let a = Arc::new(
        PlaybackAsset::new(
            [Some(source()), None],
            vec![SliceRegion::default()],
            PlaybackCurve::default(),
            vec![(
                60,
                PlaybackCurve {
                    points: vec![
                        PlaybackPoint {
                            time: 0.0,
                            value: 0.7,
                        },
                        PlaybackPoint {
                            time: 1.0,
                            value: 0.7,
                        },
                    ],
                    ..Default::default()
                },
            )],
        )
        .unwrap(),
    );
    let run = |block| {
        let mut i = WaveRide::default();
        i.prepare(RATE, 64);
        i.set_asset(a.clone());
        i.set_params(&WaveRideParams {
            release_ms: 1.0,
            level: 1.0,
            ..Default::default()
        });
        i.note_on_instance(NoteInstanceId(1), 60, 1.0, 0.0, NoteExpression::default());
        let mut pcm = render(&mut i, 400, block);
        i.note_on_instance(NoteInstanceId(2), 60, 1.0, 0.0, NoteExpression::default());
        i.note_off_instance(NoteInstanceId(1), 60);
        pcm.extend(render(&mut i, 64, block));
        assert_eq!(i.active_voices(), 1);
        i.all_notes_off();
        pcm.extend(render(&mut i, 64, block));
        assert_eq!(i.active_voices(), 0);
        pcm
    };
    let first = run(512);
    for block in [1, 7, 127] {
        assert_eq!(first, run(block));
    }
    assert!((first[100] - 0.7 * SCALE).abs() < 1e-6);
}
#[test]
fn scalar_params_exclude_assets_and_keep_prefix_indices() {
    assert!(std::mem::size_of::<GrainCloudParams>() <= 40);
    for (kind, count) in [
        (crate::InstrumentKind::SliceMap, 19),
        (crate::InstrumentKind::SliceDeck, 67),
        (crate::InstrumentKind::GrainCloud, 9),
        (crate::InstrumentKind::WaveRide, 13),
    ] {
        let p = kind.default_params();
        let json = serde_json::to_value(p).unwrap();
        assert!(json.get("table").is_none());
        assert_eq!(kind.descriptors().len(), count);
        for (index, info) in kind.descriptors().iter().enumerate() {
            assert_eq!(p.get(index), Some(info.default));
        }
        assert_eq!(
            serde_json::from_value::<crate::InstrumentParams>(json).unwrap(),
            p
        );
    }
}
#[test]
fn invalid_assets_fail_before_publication() {
    assert!(
        PlaybackAsset::new(
            [
                Some(AudioBuffer::from_interleaved(8000, 1, vec![f32::NAN])),
                None
            ],
            vec![SliceRegion::default()],
            PlaybackCurve::default(),
            vec![]
        )
        .is_err()
    );
    assert!(
        validate_slices(&[SliceRegion {
            start: 0.9,
            end: 0.2,
            ..Default::default()
        }])
        .is_err()
    );
    assert!(
        PlaybackCurve {
            points: vec![
                PlaybackPoint {
                    time: 0.0,
                    value: 0.0
                };
                2
            ],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn original_slice_scalar_indices_override_only_the_edited_asset_controls() {
    let a = asset(
        vec![SliceRegion {
            start: 0.25,
            end: 0.75,
            gain: 0.6,
            ..Default::default()
        }],
        PlaybackCurve::default(),
    );
    let mut i = SliceDeck::default();
    i.prepare(RATE, 64);
    i.set_asset(a.clone());
    let mut p = SliceDeckParams {
        level: 1.0,
        ..Default::default()
    };
    // Writing the original default is still an explicit edit: asset start .25
    // must become zero without overwriting its unrelated gain/end controls.
    assert!(p.set(3, 0.0));
    i.set_params(&p);
    i.note_on(60, 1.0);
    let pcm = render(&mut i, 1000, 7);
    for (frame, &sample) in pcm.iter().enumerate().take(990).skip(8) {
        let expected = frame as f32 / 15999.0 * SCALE * 0.6;
        assert!((sample - expected).abs() < 1e-6, "frame {frame}");
    }
    // The original pad gain index remains audible on a held voice.
    assert!(p.set(20, 0.2));
    i.set_params(&p);
    render(&mut i, 2000, 17);
    let pcm = render(&mut i, 100, 7);
    assert!((pcm[50] - 3050.0 / 15999.0 * SCALE * 0.2).abs() < 1e-6);
    let encoded = serde_json::to_string(&p).unwrap();
    assert_eq!(
        serde_json::from_str::<SliceDeckParams>(&encoded).unwrap(),
        p
    );
    assert!(p.sanitized().legacy_edits[0] & 1 != 0);
}

#[test]
fn original_position_indices_overlay_authored_curves_and_are_partition_invariant() {
    let curve = PlaybackCurve {
        points: vec![
            PlaybackPoint {
                time: 0.0,
                value: 0.8,
            },
            PlaybackPoint {
                time: 1.0,
                value: 0.8,
            },
        ],
        ..Default::default()
    };
    let sound = |block| {
        let mut i = WaveRide::default();
        i.prepare(RATE, 64);
        i.set_asset(asset(vec![SliceRegion::default()], curve.clone()));
        let mut p = WaveRideParams {
            level: 1.0,
            ..Default::default()
        };
        assert!(p.set(5, 0.0));
        i.set_params(&p);
        i.note_on(60, 1.0);
        render(&mut i, 4000, block)
    };
    let pcm = sound(1);
    assert_eq!(pcm, sound(257));
    for n in [100, 500, 1000] {
        let position = 0.8 * (n as f64 / 8000.0 * 7.0);
        assert!((f64::from(pcm[n]) - position * f64::from(SCALE)).abs() < 1e-6);
    }
    assert!((pcm[3000] - 0.8 * SCALE).abs() < 1e-6);
}

#[test]
fn migrated_maps_preserve_modulo_keys_and_global_release_without_changing_new_maps() {
    let regions = (0..16)
        .map(|index| SliceRegion {
            start: index as f32 / 16.0,
            end: (index + 1) as f32 / 16.0,
            release_ms: 30.0,
            ..Default::default()
        })
        .collect();
    let prepared = PlaybackAsset::new(
        [Some(source()), None],
        regions,
        PlaybackCurve::default(),
        vec![],
    )
    .unwrap()
    .with_legacy_key_wrap(true);
    let mut instrument = SliceMap::default();
    instrument.prepare(RATE, 64);
    instrument.set_asset(Arc::new(prepared));
    instrument.set_params(&SliceMapParams {
        level: 1.0,
        ..Default::default()
    });
    instrument.note_on(59, 1.0);
    let below = render(&mut instrument, 128, 17);
    assert!((below[64] - 15064.0 / 15999.0 * SCALE).abs() < 1e-6);
    instrument.note_off(59);
    let release = render(&mut instrument, 300, 13);
    assert!(release[64] > 0.0);
    assert!(release[240..].iter().all(|value| *value == 0.0));
    instrument.reset();
    instrument.note_on(75, 1.0);
    assert_eq!(below, render(&mut instrument, 128, 1));
    // The standard asset path remains bounded; key 59 cannot choose the last slice.
    instrument.reset();
    instrument.set_asset(asset(
        vec![SliceRegion::default()],
        PlaybackCurve::default(),
    ));
    instrument.note_on(59, 1.0);
    assert_eq!(instrument.active_voices(), 0);
}
