use windfall_dsp::sample_bank::SampleZone;
use windfall_project::*;

fn channel(doc: &mut Document, kind: InstrumentKind) -> ChannelId {
    ChannelId(
        doc.dispatch(
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
    )
}
fn sample(doc: &mut Document) -> SampleId {
    SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Own source".into(),
                path: SamplePath::External("C:/Own/source.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    )
}
#[test]
fn bank_references_survive_history_json_clone_and_legacy_omission() {
    let mut doc = Document::new(Project::new("Banks"));
    let source = sample(&mut doc);
    let id = channel(&mut doc, InstrumentKind::MultiSampler);
    let before = doc.project().clone();
    let zones = vec![SampleZone {
        sample: source.0,
        key_low: 48,
        key_high: 72,
        ..Default::default()
    }];
    doc.dispatch(
        Command::EditSampleZones {
            channel: id,
            expected: vec![],
            zones: zones.clone(),
        },
        None,
    )
    .unwrap();
    let json = serde_json::to_string(doc.project()).unwrap();
    assert!(!json.contains("data\""));
    let saved: Project = serde_json::from_str(&json).unwrap();
    saved.check().unwrap();
    assert_eq!(
        saved.channel(id).unwrap().sample_instrument,
        Some(SampleInstrumentAsset::Zones { zones })
    );
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &saved);
    let duplicate = ChannelId(
        doc.dispatch(Command::DuplicateChannel { id }, None)
            .unwrap()
            .created[0],
    );
    assert_eq!(
        doc.project().channel(duplicate).unwrap().sample_instrument,
        doc.project().channel(id).unwrap().sample_instrument
    );
    let legacy = serde_json::to_value(&before).unwrap();
    assert!(legacy["channels"][0].get("sampleInstrument").is_none());
    let reopened: Project = serde_json::from_value(legacy).unwrap();
    assert!(reopened.channel(id).unwrap().sample_instrument.is_none());
}
#[test]
fn stale_missing_invalid_pad_and_player_mapping_edits_are_atomic() {
    let mut doc = Document::new(Project::new("Refusals"));
    let source = sample(&mut doc);
    let player = channel(&mut doc, InstrumentKind::SamplePlayer);
    let pad = channel(&mut doc, InstrumentKind::PadBank);
    let before = doc.project().clone();
    let zone = SampleZone {
        sample: source.0,
        ..Default::default()
    };
    assert!(
        doc.dispatch(
            Command::EditSampleZones {
                channel: player,
                expected: vec![],
                zones: vec![zone]
            },
            None
        )
        .is_err()
    );
    assert!(
        doc.dispatch(
            Command::EditSampleZones {
                channel: pad,
                expected: vec![],
                zones: vec![zone]
            },
            None
        )
        .is_err()
    );
    assert!(
        doc.dispatch(
            Command::SetSampleInstrumentAsset {
                channel: player,
                expected: Some(SampleInstrumentAsset::Zones { zones: vec![] }),
                asset: None
            },
            None
        )
        .is_err()
    );
    assert!(
        doc.dispatch(
            Command::SetSampleInstrumentAsset {
                channel: player,
                expected: None,
                asset: Some(SampleInstrumentAsset::Zones {
                    zones: vec![SampleZone {
                        sample: 999,
                        ..zone
                    }]
                })
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &before);
    doc.dispatch(
        Command::SetSampleInstrumentAsset {
            channel: player,
            expected: None,
            asset: Some(SampleInstrumentAsset::Zones { zones: vec![zone] }),
        },
        None,
    )
    .unwrap();
    assert!(
        doc.dispatch(Command::RemoveSample { id: source }, None)
            .is_err()
    );
}
