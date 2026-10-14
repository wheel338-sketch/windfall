use windfall_project::{
    AudioClipPatch, AudioClipUpdate, ClipContent, ClipId, ClipInit, Command, Document,
    PlaylistTrackId, Project, SampleId, SamplePath, TrackId,
};

#[test]
fn audio_clip_normalize_defaults_off_and_round_trips_with_history() {
    let mut doc = Document::new(Project::new("Clip normalize"));
    let lane = PlaylistTrackId(
        doc.dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let sample = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "normalize source".into(),
                path: SamplePath::Factory("normalize.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let id = ClipId(
        doc.dispatch(
            Command::AddClips {
                clips: vec![ClipInit {
                    track: lane,
                    start: 0,
                    length: Some(960),
                    offset: None,
                    muted: None,
                    content: ClipContent::Audio {
                        sample,
                        mixer_track: TrackId::MASTER,
                        output: Default::default(),
                        normalize: false,
                        gain: 1.0,
                        pan: 0.0,
                        fade_in: 0,
                        fade_out: 0,
                        reverse: false,
                        pitch: 0.0,
                        stretch: Default::default(),
                    },
                }],
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let legacy = serde_json::to_value(doc.project()).unwrap();
    assert!(
        legacy["playlist"]["clips"][0]["content"]
            .get("normalize")
            .is_none()
    );
    let loaded = windfall_project::file::from_json(&legacy.to_string()).unwrap();
    assert!(matches!(
        loaded.playlist.clips[0].content,
        ClipContent::Audio {
            normalize: false,
            ..
        }
    ));
    let mut doc = Document::new(loaded);
    for (normalize, label) in [
        (true, "Normalize audio clip"),
        (false, "Clear audio clip normalize"),
    ] {
        let before = doc.project().clone();
        let applied = doc
            .dispatch(
                Command::UpdateAudioClips {
                    updates: vec![AudioClipUpdate {
                        id,
                        patch: AudioClipPatch {
                            normalize: Some(normalize),
                            ..Default::default()
                        },
                    }],
                },
                None,
            )
            .unwrap();
        assert_eq!(windfall_core::english::english(&applied.label), label);
        assert!(applied.touched.playlist);
        assert!(matches!(doc.project().playlist.clips[0].content,
            ClipContent::Audio { normalize: enabled, gain: 1.0, .. } if enabled == normalize));
        doc.project().check().unwrap();
        let after = doc.project().clone();
        doc.undo().unwrap();
        assert_eq!(*doc.project(), before);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), after);
        let saved = windfall_project::file::to_json(doc.project()).unwrap();
        assert_eq!(windfall_project::file::from_json(&saved).unwrap(), after);
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        if normalize {
            assert_eq!(value["playlist"]["clips"][0]["content"]["normalize"], true);
        } else {
            assert!(
                value["playlist"]["clips"][0]["content"]
                    .get("normalize")
                    .is_none()
            );
        }
    }
}
