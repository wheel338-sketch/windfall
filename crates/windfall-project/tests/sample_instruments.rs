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

#[test]
fn incompatible_instrument_changes_clear_bank_references_and_undo_restores_them() {
    let mut doc = Document::new(Project::new("Bank kind change"));
    let source = sample(&mut doc);
    let id = channel(&mut doc, InstrumentKind::MultiSampler);
    doc.dispatch(
        Command::SetSampleInstrumentAsset {
            channel: id,
            expected: None,
            asset: Some(SampleInstrumentAsset::Zones {
                zones: vec![SampleZone {
                    sample: source.0,
                    ..Default::default()
                }],
            }),
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    assert!(
        ChannelPreset::capture(doc.project(), id).is_err(),
        "unsupported export must not silently lose mappings"
    );
    let current = PluginPreset::capture(doc.project(), PluginTarget::Instrument { channel: id })
        .unwrap()
        .state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target: PluginTarget::Instrument { channel: id },
            expected: current,
            preset: PluginPreset {
                state: PluginPresetState::Instrument {
                    asset: None,
                    params: InstrumentKind::PianoKeys.default_params(),
                },
            },
        },
        None,
    )
    .unwrap();
    assert!(
        doc.project()
            .channel(id)
            .unwrap()
            .sample_instrument
            .is_none()
    );
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}

#[test]
fn source_relink_is_checked_undoable_and_preserves_player_bank_identity() {
    let mut doc = Document::new(Project::new("Relink"));
    let source = sample(&mut doc); let player = channel(&mut doc, InstrumentKind::SamplePlayer);
    let bank = SampleInstrumentAsset::Zones { zones: vec![SampleZone { sample: source.0, ..Default::default() }] };
    doc.dispatch(Command::SetSampleInstrumentAsset { channel: player, expected: None, asset: Some(bank.clone()) }, None).unwrap();
    let before = doc.project().clone(); let path = before.sample(source).unwrap().path.clone();
    assert!(doc.dispatch(Command::RelinkSamplePath { sample: source, expected: SamplePath::External("C:/stale.wav".into()), path: SamplePath::External("C:/new.wav".into()) }, None).is_err());
    assert!(doc.dispatch(Command::RelinkSamplePath { sample: source, expected: path.clone(), path: SamplePath::Project("../escape.wav".into()) }, None).is_err());
    assert_eq!(doc.project(), &before);
    doc.dispatch(Command::RelinkSamplePath { sample: source, expected: path, path: SamplePath::External("C:/new.wav".into()) }, None).unwrap();
    let linked = doc.project().clone();
    assert_eq!(linked.channel(player).unwrap().sample_instrument, Some(bank));
    assert_eq!(linked.samples.len(), before.samples.len());
    let saved: Project = serde_json::from_str(&serde_json::to_string(&linked).unwrap()).unwrap();
    assert_eq!(saved, linked); saved.check().unwrap();
    doc.undo().unwrap(); assert_eq!(doc.project(), &before);
    doc.redo().unwrap(); assert_eq!(doc.project(), &linked);
}

#[test]
fn multisampler_modulation_and_effects_persist_and_legacy_fields_default_to_dry() {
    let mut doc = Document::new(Project::new("Modulation")); let source=sample(&mut doc); let id=channel(&mut doc,InstrumentKind::MultiSampler);
    let modulation=windfall_dsp::sample_bank::ZoneModulation { envelope_pitch_cents:1200.0,..Default::default() };
    doc.dispatch(Command::EditSampleZones { channel:id,expected:vec![],zones:vec![SampleZone { sample:source.0,modulation_group:2,modulation,..Default::default() }] },None).unwrap();
    let before=doc.project().clone();
    doc.dispatch(Command::SetInstrumentParam { channel:id,param:9,value:0.3 },None).unwrap();
    let saved=serde_json::to_value(doc.project()).unwrap(); let loaded:Project=serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(loaded,doc.project().clone()); loaded.check().unwrap();
    doc.undo().unwrap();assert_eq!(doc.project(),&before);doc.redo().unwrap();assert_eq!(doc.project(),&loaded);
    let mut legacy=saved;
    let channel=legacy["channels"].as_array_mut().unwrap().iter_mut().find(|c| c["id"]==id.0).unwrap();
    let params=channel["source"]["params"].as_object_mut().unwrap(); for name in ["chorus","delay","reverb"] { params.remove(name); }
    let zone=channel["sampleInstrument"]["zones"][0].as_object_mut().unwrap(); for name in ["modulationGroup","modulation","delayMs"] { zone.remove(name); }
    let legacy:Project=serde_json::from_value(legacy).unwrap(); legacy.check().unwrap();
    let ChannelSource::Instrument { params:InstrumentParams::MultiSampler(params),.. }=legacy.channel(id).unwrap().source else { panic!() };
    assert_eq!(params.delay.mix,0.0);assert_eq!(params.chorus.mix,0.0);assert_eq!(params.reverb.mix,0.0);
    let Some(SampleInstrumentAsset::Zones { zones })=&legacy.channel(id).unwrap().sample_instrument else { panic!() };
    assert_eq!(zones[0].modulation,Default::default());assert_eq!(zones[0].modulation_group,0);
}
