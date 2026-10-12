use windfall_project::*;

#[test]
fn stage_settings_legacy_omission_history_and_roundtrip() {
    let mut doc = Document::new(Project::new("Stage"));
    let legacy = serde_json::to_string(doc.project()).unwrap();
    assert!(!legacy.contains("stageMedia"));
    let mut media = StageMedia::default();
    media.pads.push(PerformancePad {
        track: 0,
        scene: 0,
        pattern: doc.project().patterns[0].id,
    });
    media.videos.push(VideoPlacement {
        path: "C:/own-video.webm".into(),
        start_tick: 960,
        length_ticks: 3840,
        offset_seconds: 1.25,
        muted: true,
    });
    doc.dispatch(
        Command::SetStageMedia {
            media: media.clone(),
        },
        None,
    )
    .unwrap();
    let saved = serde_json::to_string(doc.project()).unwrap();
    let reopened: Project = serde_json::from_str(&saved).unwrap();
    reopened.check().unwrap();
    assert_eq!(reopened.settings.stage_media, media);
    doc.undo().unwrap();
    assert_eq!(serde_json::to_string(doc.project()).unwrap(), legacy);
    doc.redo().unwrap();
    assert_eq!(doc.project().settings.stage_media, media);
}

#[test]
fn invalid_media_is_atomic_and_deletion_restores_references_on_undo() {
    let mut doc = Document::new(Project::new("Stage"));
    let added = doc
        .dispatch(Command::AddPattern { name: None }, None)
        .unwrap();
    let pattern = PatternId(added.created[0]);
    let mut media = StageMedia::default();
    media.pads.push(PerformancePad {
        track: 1,
        scene: 2,
        pattern,
    });
    doc.dispatch(
        Command::SetStageMedia {
            media: media.clone(),
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    let mut invalid = media.clone();
    invalid.pads.push(invalid.pads[0]);
    assert!(
        doc.dispatch(Command::SetStageMedia { media: invalid }, None)
            .is_err()
    );
    assert_eq!(doc.project(), &before);
    doc.dispatch(Command::RemovePattern { id: pattern }, None)
        .unwrap();
    assert!(doc.project().settings.stage_media.pads.is_empty());
    doc.project().check().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project().settings.stage_media, media);
}

#[test]
fn performance_midi_action_uses_checked_controller_contract() {
    let mut doc = Document::new(Project::new("MIDI stage"));
    let link = ControllerLink {
        source: ControlSource::Midi {
            device: None,
            channel: Some(1),
            control: MidiControl::Note { key: 36 },
        },
        target: LinkTarget::Action {
            action: ControlAction::PerformancePad { track: 1, scene: 2 },
        },
        map: ControlMap::default(),
    };
    doc.dispatch(
        Command::SetControllerLinks {
            links: vec![link.clone()],
        },
        None,
    )
    .unwrap();
    let mut invalid = link;
    invalid.target = LinkTarget::Action {
        action: ControlAction::PerformanceStop { track: 32 },
    };
    assert!(
        doc.dispatch(
            Command::SetControllerLinks {
                links: vec![invalid]
            },
            None
        )
        .is_err()
    );
}
