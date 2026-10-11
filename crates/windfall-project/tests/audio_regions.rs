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
        },
        AudioRegion {
            name: "Cue".into(),
            start_frame: 250,
            end_frame: 250,
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
    doc.undo();
    assert_eq!(doc.project(), &empty);
    doc.redo();
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
                    end_frame: 1
                }]
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &edited);
}
