use windfall_project::*;

fn setup() -> (Document, SampleId, SampleId, AudioEditRecipe) {
    let mut doc = Document::new(Project::new("Analysis"));
    let source = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Original".into(),
                path: SamplePath::Factory("original.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let derived = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Edited".into(),
                path: SamplePath::Factory("derived.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let track = PlaylistTrackId(
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
    let clip = Clip {
        id: ClipId(doc.project().next_id),
        track,
        start: 0,
        length: 1920,
        offset: 0,
        muted: false,
        content: ClipContent::Audio {
            sample: source,
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
    };
    let recipe = AudioEditRecipe {
        source_clip: clip,
        analysis: AudioAnalysisData {
            frames: 16000,
            sample_rate: 16000,
            polyphonic: false,
            notes: vec![DetectedAudioNote {
                start_frame: 4000,
                end_frame: 12000,
                midi_pitch: 69.0,
                confidence: 0.9,
                rms: 0.1,
            }],
            transients: vec![4000],
        },
        overrides: vec![],
        markers: vec![
            AudioWarpMarker {
                source_frame: 0,
                target_frame: 0,
            },
            AudioWarpMarker {
                source_frame: 16000,
                target_frame: 16000,
            },
        ],
        denoise: None,
    };
    (doc, source, derived, recipe)
}
#[test]
fn processing_metadata_legacy_defaults_history_reopen_and_retained_source() {
    let (mut doc, source, derived, mut recipe) = setup();
    let old = file::to_json(doc.project()).unwrap();
    assert!(!old.contains("processing"));
    assert_eq!(file::from_json(&old).unwrap(), *doc.project());
    let cursor = doc.history().cursor;
    recipe.overrides.push(AudioNoteOverride {
        note: 0,
        midi_pitch: 70.0,
        start_frame: 5000,
        end_frame: 13000,
    });
    doc.dispatch(
        Command::SetSampleProcessing {
            id: derived,
            processing: Some(recipe.clone()),
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.history().cursor, cursor + 1);
    assert!(
        doc.dispatch(Command::RemoveSample { id: source }, None)
            .is_err()
    );
    let json = file::to_json(doc.project()).unwrap();
    let loaded = file::from_json(&json).unwrap();
    assert_eq!(loaded.sample(derived).unwrap().processing, Some(recipe));
    doc.undo().unwrap();
    assert!(doc.project().sample(derived).unwrap().processing.is_none());
    doc.redo().unwrap();
    assert_eq!(doc.project(), &loaded);
}
#[test]
fn shorter_warp_duration_and_crossing_note_anchors_are_checked_atomically() {
    let (mut doc, _, derived, mut recipe) = setup();
    recipe.markers[1].target_frame = 8000;
    assert!(recipe.check().is_ok());
    recipe.overrides.push(AudioNoteOverride {
        note: 0,
        midi_pitch: 69.0,
        start_frame: 2000,
        end_frame: 7999,
    });
    let before = doc.project().clone();
    let cursor = doc.history().cursor;
    assert!(
        doc.dispatch(
            Command::SetSampleProcessing {
                id: derived,
                processing: Some(recipe)
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history().cursor, cursor);
}
