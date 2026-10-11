use windfall_core::AudioBuffer;
use windfall_dsp::granular::{PlaybackCurve, PlaybackCurveMode, PlaybackPoint, SliceRegion};
use windfall_project::*;
fn fixture(kind: InstrumentKind) -> (Document, ChannelId, SampleId) {
    let mut d = Document::new(Project::new("sample assets"));
    let sample = SampleId(
        d.dispatch(
            Command::AddSample {
                name: "Original loop".into(),
                path: SamplePath::Factory("User/original.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let channel = ChannelId(
        d.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(kind),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    (d, channel, sample)
}
#[test]
fn asset_edits_pattern_generation_undo_redo_and_file_roundtrip() {
    let (mut d, c, s) = fixture(InstrumentKind::SliceDeck);
    let before = d.project().clone();
    let asset = SamplePlayback {
        sample: Some(s),
        sample_b: Some(s),
        slices: vec![
            SliceRegion {
                start: 0.5,
                end: 1.0,
                reverse: true,
                deck: 1,
                gain: 0.7,
                pitch: 7.0,
                ..Default::default()
            },
            SliceRegion {
                start: 0.0,
                end: 0.5,
                attack_ms: 15.0,
                release_ms: 50.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let edited = d
        .dispatch(
            Command::SetSamplePlayback {
                channel: c,
                asset: Some(asset.clone()),
            },
            None,
        )
        .unwrap();
    assert!(edited.touched.channels);
    assert_eq!(
        d.project().channel(c).unwrap().sample_playback,
        Some(asset.clone())
    );
    d.undo().unwrap();
    assert_eq!(*d.project(), before);
    d.redo().unwrap();
    let pattern = d.project().patterns[0].id;
    let result = d
        .dispatch(
            Command::GenerateSlicePattern {
                channel: c,
                pattern,
            },
            None,
        )
        .unwrap();
    assert_eq!(result.created.len(), 2);
    let notes = &d.project().patterns[0].lanes[0].notes;
    assert_eq!(
        notes
            .iter()
            .map(|n| (n.start, n.length, n.key))
            .collect::<Vec<_>>(),
        [(0, 1920, 60), (1920, 1920, 61)]
    );
    let generated = d.project().clone();
    d.undo().unwrap();
    assert!(d.project().patterns[0].lanes.is_empty());
    d.redo().unwrap();
    assert_eq!(*d.project(), generated);
    assert_eq!(
        file::from_json(&file::to_json(d.project()).unwrap()).unwrap(),
        generated
    );
    assert!(d.dispatch(Command::RemoveSample { id: s }, None).is_err());
    d.dispatch(Command::RemoveChannel { id: c }, None).unwrap();
    d.undo().unwrap();
    assert_eq!(*d.project(), generated);
}
#[test]
fn source_and_curve_validation_rejection_is_atomic() {
    let (mut d, c, s) = fixture(InstrumentKind::WaveRide);
    let before = d.project().clone();
    for asset in [
        SamplePlayback {
            sample: Some(SampleId(900)),
            ..Default::default()
        },
        SamplePlayback {
            sample: Some(s),
            slices: vec![SliceRegion {
                start: 1.0,
                ..Default::default()
            }],
            ..Default::default()
        },
        SamplePlayback {
            curve: PlaybackCurve {
                points: vec![
                    PlaybackPoint {
                        time: 0.0,
                        value: 0.0
                    };
                    2
                ],
                ..Default::default()
            },
            ..Default::default()
        },
    ] {
        assert!(
            d.dispatch(
                Command::SetSamplePlayback {
                    channel: c,
                    asset: Some(asset)
                },
                None
            )
            .is_err()
        );
        assert_eq!(*d.project(), before);
    }
    let valid = SamplePlayback {
        sample: Some(s),
        key_curves: vec![KeyPlaybackCurve {
            key: 60,
            curve: PlaybackCurve {
                mode: PlaybackCurveMode::Speed,
                origin: 0.5,
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
            },
        }],
        ..Default::default()
    };
    d.dispatch(
        Command::SetSamplePlayback {
            channel: c,
            asset: Some(valid),
        },
        None,
    )
    .unwrap();
    d.project().check().unwrap();
    assert_eq!(
        file::from_json(&file::to_json(d.project()).unwrap()).unwrap(),
        *d.project()
    );
}
#[test]
fn transient_detection_on_authored_stereo_and_beat_grid_follow_source_tempo() {
    let mut pcm = vec![0.0; 32000];
    for frame in [2000, 6000, 10000] {
        for f in frame..frame + 120 {
            pcm[2 * f] = 0.8;
            pcm[2 * f + 1] = -0.8;
        }
    }
    let audio = AudioBuffer::from_interleaved(8000, 2, pcm);
    let transient = sample_playback::detect(
        &audio,
        SampleId(2),
        120.0,
        slicer::SliceOptions::Transients { sensitivity: 1.0 },
    )
    .unwrap();
    assert_eq!(transient.slices.len(), 4);
    for (slice, expected) in transient.slices.iter().skip(1).zip([0.125, 0.375, 0.625]) {
        assert!((slice.start - expected).abs() < 0.001);
    }
    let a = sample_playback::detect(
        &audio,
        SampleId(2),
        120.0,
        slicer::SliceOptions::Grid { grid_ticks: PPQ },
    )
    .unwrap();
    let b = sample_playback::detect(
        &audio,
        SampleId(2),
        60.0,
        slicer::SliceOptions::Grid { grid_ticks: PPQ },
    )
    .unwrap();
    assert_eq!(a.slices.len(), 4);
    assert_eq!(b.slices.len(), 2);
    assert_eq!(a.source_beats, 4.0);
    assert_eq!(b.source_beats, 2.0);
}
#[test]
fn legacy_v1_source_map_and_curve_migrate_without_pcm_in_copy_controls() {
    let (d, _, _) = fixture(InstrumentKind::SliceDeck);
    let mut json = serde_json::to_value(d.project()).unwrap();
    json["channels"][0]["source"]["params"]["table"] =
        serde_json::json!({"sampleRate":8000,"len":4,"data":[0.2,0.4,0.6,0.8]});
    json["channels"][0]["source"]["params"]["sliceStarts"] =
        serde_json::json!((0..16).map(|i| i as f32 / 16.0).collect::<Vec<_>>());
    let loaded: Project = serde_json::from_value(json).unwrap();
    loaded.check().unwrap();
    let asset = loaded.channels[0].sample_playback.as_ref().unwrap();
    assert_eq!(
        asset.inline_source.as_ref().unwrap().data,
        [0.2, 0.4, 0.6, 0.8]
    );
    assert_eq!(asset.slices.len(), 16);
    assert!(!file::to_json(&loaded).unwrap().contains("sliceStarts"));
    assert!(!file::to_json(&loaded).unwrap().contains("\"table\""));
    let (d, _, _) = fixture(InstrumentKind::WaveRide);
    let mut json = serde_json::to_value(d.project()).unwrap();
    json["channels"][0]["source"]["params"]["positions"] =
        serde_json::json!([0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
    let loaded: Project = serde_json::from_value(json).unwrap();
    assert_eq!(
        loaded.channels[0]
            .sample_playback
            .as_ref()
            .unwrap()
            .curve
            .points
            .len(),
        8
    );
}
#[test]
fn old_v1_channels_without_assets_keep_exact_json_and_defaults() {
    let (d, _, _) = fixture(InstrumentKind::SubtractiveSynth);
    let text = file::to_json(d.project()).unwrap();
    assert!(!text.contains("samplePlayback"));
    assert_eq!(
        file::to_json(&file::from_json(&text).unwrap()).unwrap(),
        text
    );
}
