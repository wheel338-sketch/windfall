use windfall_dsp::sample_bank::SampleZone;
use windfall_project::*;

#[test]
fn soundfont_source_identity_relink_history_json_and_legacy_upgrade() {
    let mut doc=Document::new(Project::new("SoundFont identity"));
    let source=SampleId(doc.dispatch(Command::AddSample {name:"Own bank".into(),
        path:SamplePath::External("C:/Own/bank.sf2".into())},None).unwrap().created[0]);
    let channel=channel(&mut doc,InstrumentKind::SoundFont);
    doc.dispatch(Command::RegisterSoundFontSource {sample:source,expected:doc.project().sample(source).unwrap().path.clone()},None).unwrap();
    let asset=SampleInstrumentAsset::SoundFont {source:Some(source),path:SamplePath::External("C:/Own/bank.sf2".into()),bank:0,program:0};
    doc.dispatch(Command::SetSampleInstrumentAsset {channel,expected:None,asset:Some(asset.clone())},None).unwrap();
    assert!(doc.dispatch(Command::RemoveSample{id:source},None).is_err());
    let before=doc.project().clone();
    let replacement=SamplePath::External("C:/Recovered/bank.sf2".into());
    doc.dispatch(Command::RelinkSamplePath {sample:source,expected:before.sample(source).unwrap().path.clone(),path:replacement.clone()},None).unwrap();
    assert_eq!(asset.soundfont_path(doc.project()),Some(&replacement));
    assert_eq!(doc.project().channel(channel).unwrap().sample_instrument,Some(asset));
    let after=doc.project().clone();doc.undo().unwrap();assert_eq!(doc.project(),&before);
    doc.redo().unwrap();assert_eq!(doc.project(),&after);
    let reopened=file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();assert_eq!(reopened,after);
    let mut legacy=before.clone();
    if let Some(SampleInstrumentAsset::SoundFont{source,..})=&mut legacy.channels.iter_mut().find(|c|c.id==channel).unwrap().sample_instrument {*source=None;}
    let json=file::to_json(&legacy).unwrap();
    let upgraded=file::from_json(&json).unwrap();
    assert_eq!(upgraded.channel(channel).unwrap().sample_instrument.as_ref().unwrap().soundfont_source(),Some(source));
    assert_eq!(upgraded.samples.len(),before.samples.len());
}

#[test]
fn raw_soundfont_role_is_saved_after_preset_replacement_and_cannot_alias_audio() {
    let mut doc=Document::new(Project::new("Bank source kinds"));let source=sample(&mut doc);
    let before=doc.project().clone();let path=doc.project().sample(source).unwrap().path.clone();
    doc.dispatch(Command::RegisterSoundFontSource{sample:source,expected:path.clone()},None).unwrap();
    assert!(doc.project().is_soundfont_source(source));
    let reopened=file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
    assert!(reopened.is_soundfont_source(source));
    doc.undo().unwrap();assert_eq!(doc.project(),&before);doc.redo().unwrap();
    let audio=channel(&mut doc,InstrumentKind::MultiSampler);
    assert!(doc.dispatch(Command::EditSampleZones{channel:audio,expected:vec![],zones:vec![SampleZone {sample:source.0,..Default::default()}]},None).is_err());
    let mut legacy=serde_json::to_value(&before).unwrap();
    for sample in legacy["samples"].as_array_mut().unwrap() {sample.as_object_mut().unwrap().remove("soundfont");}
    let old:Project=serde_json::from_value(legacy).unwrap();assert!(!old.is_soundfont_source(source));
}

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
                tables: None,
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
    let source = sample(&mut doc);
    let player = channel(&mut doc, InstrumentKind::SamplePlayer);
    let bank = SampleInstrumentAsset::Zones {
        zones: vec![SampleZone {
            sample: source.0,
            ..Default::default()
        }],
    };
    doc.dispatch(
        Command::SetSampleInstrumentAsset {
            channel: player,
            expected: None,
            asset: Some(bank.clone()),
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    let path = before.sample(source).unwrap().path.clone();
    assert!(
        doc.dispatch(
            Command::RelinkSamplePath {
                sample: source,
                expected: SamplePath::External("C:/stale.wav".into()),
                path: SamplePath::External("C:/new.wav".into())
            },
            None
        )
        .is_err()
    );
    assert!(
        doc.dispatch(
            Command::RelinkSamplePath {
                sample: source,
                expected: path.clone(),
                path: SamplePath::Project("../escape.wav".into())
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &before);
    doc.dispatch(
        Command::RelinkSamplePath {
            sample: source,
            expected: path,
            path: SamplePath::External("C:/new.wav".into()),
        },
        None,
    )
    .unwrap();
    let linked = doc.project().clone();
    assert_eq!(
        linked.channel(player).unwrap().sample_instrument,
        Some(bank)
    );
    assert_eq!(linked.samples.len(), before.samples.len());
    let saved: Project = serde_json::from_str(&serde_json::to_string(&linked).unwrap()).unwrap();
    assert_eq!(saved, linked);
    saved.check().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &linked);
}

#[test]
fn multisampler_modulation_and_effects_persist_and_legacy_fields_default_to_dry() {
    let mut doc = Document::new(Project::new("Modulation"));
    let source = sample(&mut doc);
    let id = channel(&mut doc, InstrumentKind::MultiSampler);
    let modulation = windfall_dsp::sample_bank::ZoneModulation {
        envelope_pitch_cents: 1200.0,
        ..Default::default()
    };
    doc.dispatch(
        Command::EditSampleZones {
            channel: id,
            expected: vec![],
            zones: vec![SampleZone {
                sample: source.0,
                modulation_group: 2,
                modulation,
                ..Default::default()
            }],
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    doc.dispatch(
        Command::SetInstrumentParam {
            channel: id,
            param: 9,
            value: 0.3,
        },
        None,
    )
    .unwrap();
    let saved = serde_json::to_value(doc.project()).unwrap();
    let loaded: Project = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(loaded, doc.project().clone());
    loaded.check().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &loaded);
    let mut legacy = saved;
    let channel = legacy["channels"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id.0)
        .unwrap();
    let params = channel["source"]["params"].as_object_mut().unwrap();
    for name in ["chorus", "delay", "reverb"] {
        params.remove(name);
    }
    let zone = channel["sampleInstrument"]["zones"][0]
        .as_object_mut()
        .unwrap();
    for name in ["modulationGroup", "modulation", "delayMs"] {
        zone.remove(name);
    }
    let legacy: Project = serde_json::from_value(legacy).unwrap();
    legacy.check().unwrap();
    let ChannelSource::Instrument {
        params: InstrumentParams::MultiSampler(params),
        ..
    } = legacy.channel(id).unwrap().source
    else {
        panic!()
    };
    assert_eq!(params.delay.mix, 0.0);
    assert_eq!(params.chorus.mix, 0.0);
    assert_eq!(params.reverb.mix, 0.0);
    let Some(SampleInstrumentAsset::Zones { zones }) =
        &legacy.channel(id).unwrap().sample_instrument
    else {
        panic!()
    };
    assert_eq!(zones[0].modulation, Default::default());
    assert_eq!(zones[0].modulation_group, 0);
}

#[test]
fn project_feedback_routing_is_checked_saved_and_restored_by_undo() {
    let mut doc = Document::new(Project::new("Feedback"));
    let id = channel(&mut doc, InstrumentKind::MidiOut);
    let before = doc.project().clone();
    let mut params = InstrumentKind::MidiOut.default_params();
    if let InstrumentParams::MidiOut(params) = &mut params {
        params.feedback_enabled = true;
        params.feedback_controller = 3;
        params.feedback_motor = 2;
        params.feedback_strength = 0.4;
        params.feedback_curve = 2.0;
    }
    doc.dispatch(
        Command::SetInstrumentParams {
            channel: id,
            params,
        },
        None,
    )
    .unwrap();
    let edited = doc.project().clone();
    edited.check().unwrap();
    let loaded: Project = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(loaded, edited);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &edited);
    let mut invalid = edited.clone();
    if let ChannelSource::Instrument {
        params: InstrumentParams::MidiOut(params),
        ..
    } = &mut invalid
        .channels
        .iter_mut()
        .find(|c| c.id == id)
        .unwrap()
        .source
    {
        params.feedback_controller = 4;
    }
    assert!(invalid.check().is_err());
    let mut legacy = serde_json::to_value(&edited).unwrap();
    let params = legacy["channels"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id.0)
        .unwrap()["source"]["params"]
        .as_object_mut()
        .unwrap();
    for field in [
        "feedbackEnabled",
        "feedbackController",
        "feedbackMotor",
        "feedbackStrength",
        "feedbackCurve",
    ] {
        params.remove(field);
    }
    let old: Project = serde_json::from_value(legacy).unwrap();
    let ChannelSource::Instrument {
        params: InstrumentParams::MidiOut(params),
        ..
    } = old.channel(id).unwrap().source
    else {
        panic!()
    };
    assert!(!params.feedback_enabled);
    assert_eq!(params.feedback_strength, 1.0);
    assert_eq!(params.feedback_curve, 1.0);
}

#[test]
fn music_lighting_is_a_saved_mixer_effect_with_checked_history() {
    let mut doc = Document::new(Project::new("Lighting effect"));
    let effect = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind: EffectKind::MusicLighting,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let before = doc.project().clone();
    doc.dispatch(
        Command::SetEffectParam {
            track: TrackId::MASTER,
            effect,
            param: 1,
            value: 4.0,
        },
        None,
    )
    .unwrap();
    let saved = doc.project().clone();
    saved.check().unwrap();
    let loaded: Project = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    assert_eq!(loaded, saved);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &saved);
    assert_eq!(EffectKind::ALL[67], EffectKind::MusicLighting);
}
