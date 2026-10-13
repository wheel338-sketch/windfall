use windfall_project::{AudioRegion, Command, Document, Project, SampleId, SamplePath};

#[test]
fn audio_regions_have_guarded_atomic_history_and_default_compatible_persistence() {
    let mut doc = Document::new(Project::new("Regions"));
    let sample = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Source".into(),
                path: SamplePath::External("source.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let empty = doc.project().clone();
    assert!(
        !serde_json::to_string(&empty)
            .unwrap()
            .contains("\"regions\"")
    );
    let regions = vec![
        AudioRegion {
            name: "Phrase".into(),
            start_frame: 100,
            end_frame: 500,
            notes: vec![
                "Original cue note".into(),
                "WindfallSlice/1 0.5 7 2 10 1 0".into(),
            ],
        },
        AudioRegion {
            name: "Cue".into(),
            start_frame: 250,
            end_frame: 250,
            notes: Vec::new(),
        },
    ];
    doc.dispatch(
        Command::SetSampleRegions {
            sample,
            expected: Vec::new(),
            regions: regions.clone(),
        },
        None,
    )
    .unwrap();
    let edited = doc.project().clone();
    assert_eq!(edited.sample(sample).unwrap().regions, regions);
    let loaded: Project = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(loaded, edited);
    assert!(doc.undo().is_some());
    assert_eq!(doc.project(), &empty);
    assert!(doc.redo().is_some());
    assert_eq!(doc.project(), &edited);
    assert!(
        doc.dispatch(
            Command::SetSampleRegions {
                sample,
                expected: Vec::new(),
                regions: Vec::new()
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &edited);
    assert!(
        doc.dispatch(
            Command::SetSampleRegions {
                sample,
                expected: regions,
                regions: vec![AudioRegion {
                    name: "Backwards".into(),
                    start_frame: 2,
                    end_frame: 1,
                    notes: Vec::new(),
                }]
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &edited);
}

#[test]
fn audio_region_notes_have_legacy_defaults_and_bounded_checked_history() {
    let old: AudioRegion =
        serde_json::from_str(r#"{"name":"Old cue","startFrame":1,"endFrame":1}"#).unwrap();
    assert!(old.notes.is_empty());
    assert!(!serde_json::to_string(&old).unwrap().contains("notes"));
    for notes in [
        vec!["x".repeat(1025)],
        vec!["contains\0nul".into()],
        vec!["note".into(); 17],
    ] {
        let region = AudioRegion {
            notes,
            ..old.clone()
        };
        assert!(windfall_project::model::check_audio_regions(&[region]).is_err());
    }
    let region = AudioRegion {
        notes: vec!["é".repeat(512); 16],
        ..old
    };
    windfall_project::model::check_audio_regions(&[region]).unwrap();
}
