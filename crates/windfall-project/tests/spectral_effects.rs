use windfall_project::{
    Command, ConvolverImpulse, Document, EffectId, EffectKind, Project, TrackId, file,
};
fn add(doc: &mut Document, kind: EffectKind) -> EffectId {
    EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    )
}
fn asset() -> ConvolverImpulse {
    ConvolverImpulse {
        name: "Own stereo IR".into(),
        sample_rate: 48000,
        group_delay: 0,
        eq_gains: None,
        frames: vec![[1.0, 0.5], [0.0, -0.25]],
    }
}
#[test]
fn impulse_asset_history_clone_and_disk_roundtrip() {
    let mut doc = Document::new(Project::new("Spectral"));
    let effect = add(&mut doc, EffectKind::Convolver);
    let command = Command::SetConvolverImpulse {
        track: TrackId::MASTER,
        effect,
        impulse: Some(asset()),
    };
    let applied = doc.dispatch(command, None).unwrap();
    assert!(applied.touched.mixer);
    assert_eq!(
        doc.project().mixer.tracks[0].effects[0].impulse,
        Some(asset())
    );
    doc.undo().unwrap();
    assert!(doc.project().mixer.tracks[0].effects[0].impulse.is_none());
    doc.redo().unwrap();
    doc.dispatch(
        Command::DuplicateEffect {
            track: TrackId::MASTER,
            effect,
        },
        None,
    )
    .unwrap();
    assert_eq!(
        doc.project().mixer.tracks[0].effects[1].impulse,
        Some(asset())
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("impulse.windfall");
    file::save(doc.project(), &path).unwrap();
    assert_eq!(file::load(path).unwrap(), *doc.project());
    let json = file::to_json(doc.project()).unwrap();
    assert!(!json.contains("impulseLength"));
    assert_eq!(file::from_json(&json).unwrap(), *doc.project());
}
#[test]
fn impulse_refusal_is_atomic_and_legacy_payload_migrates() {
    let mut doc = Document::new(Project::new("IR admission"));
    let effect = add(&mut doc, EffectKind::Convolver);
    for frames in [vec![], vec![[f32::NAN, 0.0]], vec![[0.0; 2]; 65537]] {
        let before = doc.project().clone();
        let history = doc.history().cursor;
        let mut ir = asset();
        ir.frames = frames;
        assert!(
            doc.dispatch(
                Command::SetConvolverImpulse {
                    track: TrackId::MASTER,
                    effect,
                    impulse: Some(ir)
                },
                None
            )
            .is_err()
        );
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history().cursor, history);
    }
    let other = add(&mut doc, EffectKind::Vocoder);
    assert!(
        doc.dispatch(
            Command::SetConvolverImpulse {
                track: TrackId::MASTER,
                effect: other,
                impulse: Some(asset())
            },
            None
        )
        .is_err()
    );
    let slot: windfall_project::EffectSlot=serde_json::from_str(r#"{"id":2,"enabled":true,"mix":1,"params":{"type":"convolver","impulse":[1,0.5],"impulseLength":2,"mix":1,"gain":1}}"#).unwrap();
    assert_eq!(slot.impulse.unwrap().frames, vec![[1.0; 2], [0.5; 2]]);
}
#[test]
fn every_spectral_scalar_control_persists_and_undoes() {
    for kind in [
        EffectKind::Convolver,
        EffectKind::FrequencyShifter,
        EffectKind::PitchShift,
        EffectKind::PitchCorrect,
        EffectKind::Vocoder,
    ] {
        let mut doc = Document::new(Project::new("Spectral controls"));
        let effect = add(&mut doc, kind);
        for (param, info) in kind.descriptors().iter().enumerate() {
            let before = doc.project().mixer.tracks[0].effects[0].params;
            let value = if info.default == info.max {
                info.min
            } else {
                info.max
            };
            doc.dispatch(
                Command::SetEffectParam {
                    track: TrackId::MASTER,
                    effect,
                    param: param as u32,
                    value,
                },
                None,
            )
            .unwrap();
            let after = doc.project().mixer.tracks[0].effects[0].params;
            assert_ne!(before, after);
            doc.undo().unwrap();
            assert_eq!(doc.project().mixer.tracks[0].effects[0].params, before);
            doc.redo().unwrap();
            assert_eq!(doc.project().mixer.tracks[0].effects[0].params, after);
            assert_eq!(
                file::from_json(&file::to_json(doc.project()).unwrap()).unwrap(),
                *doc.project()
            );
        }
    }
}
#[test]
fn rate_conversion_preserves_stereo_and_refuses_runtime_overflow() {
    let mut ir = asset();
    ir.frames = vec![[0.25, -0.5]; 8];
    assert_eq!(ir.at_rate(96000).unwrap(), vec![[0.25, -0.5]; 16]);
    ir.frames = vec![[0.0; 2]; 65536];
    assert!(ir.at_rate(96000).is_err());
}

#[test]
fn harmony_source_reference_persists_separately_from_scalar_automation() {
    let mut doc = Document::new(Project::new("MIDI harmony"));
    let effect = add(&mut doc, EffectKind::PitchCorrect);
    let windfall_project::EffectParams::PitchCorrect(mut params) =
        doc.project().mixer.tracks[0].effects[0].params
    else {
        panic!("correct kind")
    };
    params.harmony_channel = 123;
    params.harmony_mix = 0.75;
    doc.dispatch(
        Command::SetEffectParams {
            track: TrackId::MASTER,
            effect,
            params: windfall_project::EffectParams::PitchCorrect(params),
        },
        None,
    )
    .unwrap();
    let project = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&project).unwrap()).unwrap(),
        project
    );
    doc.undo().unwrap();
    assert_ne!(*doc.project(), project);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &project);
}

#[test]
fn linear_phase_delay_and_eq_controls_persist_and_invalid_metadata_is_refused() {
    let mut impulse = asset();
    impulse.frames = vec![[0.0; 2]; 1025];
    impulse.frames[512] = [1.0; 2];
    impulse.group_delay = 512;
    impulse.eq_gains = Some([0.0; 3]);
    impulse.check().unwrap();
    assert_eq!(impulse.group_delay_at(48000), 512);
    assert_eq!(impulse.group_delay_at(96000), 1024);
    let copy: ConvolverImpulse =
        serde_json::from_str(&serde_json::to_string(&impulse).unwrap()).unwrap();
    assert_eq!(copy, impulse);
    impulse.frames[0][0] = 0.5;
    assert!(impulse.check().is_err());
}

#[test]
fn convolver_plugin_presets_carry_assets_and_refuse_stale_destinations() {
    use windfall_project::{PluginPreset, PluginPresetState, PluginTarget, Preset};
    let mut doc = Document::new(Project::new("Portable effect presets"));
    let source = add(&mut doc, EffectKind::Convolver);
    doc.dispatch(
        Command::SetConvolverImpulse {
            track: TrackId::MASTER,
            effect: source,
            impulse: Some(asset()),
        },
        None,
    )
    .unwrap();
    let captured =
        PluginPreset::capture(doc.project(), PluginTarget::Effect { effect: source }).unwrap();
    let envelope = Preset::of_plugin("Own IR", captured.clone());
    let (loaded, notes) = Preset::from_json(envelope.to_json().unwrap().as_bytes()).unwrap();
    assert!(notes.is_empty());
    assert_eq!(loaded, envelope);
    let destination = add(&mut doc, EffectKind::Delay);
    let target = PluginTarget::Effect {
        effect: destination,
    };
    let before = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected: before.clone(),
            preset: captured.clone(),
        },
        None,
    )
    .unwrap();
    assert_eq!(
        doc.project().mixer.tracks[0].effects[1].impulse,
        Some(asset())
    );
    doc.undo().unwrap();
    assert_eq!(
        doc.project().mixer.tracks[0].effects[1].params.kind(),
        EffectKind::Delay
    );
    assert!(doc.project().mixer.tracks[0].effects[1].impulse.is_none());
    doc.redo().unwrap();
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::SetConvolverImpulse {
            track: TrackId::MASTER,
            effect: destination,
            impulse: None,
        },
        None,
    )
    .unwrap();
    let project = doc.project().clone();
    assert!(
        doc.dispatch(
            Command::ApplyPluginPreset {
                target,
                expected,
                preset: captured.clone()
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &project);
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected,
            preset: captured,
        },
        None,
    )
    .unwrap();
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected,
            preset: PluginPreset {
                state: PluginPresetState::Effect {
                    params: EffectKind::Delay.default_params(),
                    impulse: None,
                },
            },
        },
        None,
    )
    .unwrap();
    assert!(doc.project().mixer.tracks[0].effects[1].impulse.is_none());
    doc.project().check().unwrap();
}
#[test]
fn legacy_ir_and_harmony_source_survive_preset_migration() {
    use windfall_project::{PluginPreset, PluginPresetState, Preset};
    let preset = Preset::of_plugin(
        "Legacy IR",
        PluginPreset {
            state: PluginPresetState::Effect {
                params: EffectKind::Convolver.default_params(),
                impulse: None,
            },
        },
    );
    let mut json = serde_json::to_value(preset).unwrap();
    json["plugin"]["state"]["params"]["impulseLength"] = serde_json::json!(2);
    json["plugin"]["state"]["params"]["impulse"] = serde_json::json!([1.0, -0.5]);
    let (loaded, _) = Preset::from_json(&serde_json::to_vec(&json).unwrap()).unwrap();
    let PluginPresetState::Effect {
        impulse: Some(ir), ..
    } = loaded.plugin.unwrap().state
    else {
        panic!("legacy IR missing")
    };
    assert_eq!(ir.frames, vec![[1.0; 2], [-0.5; 2]]);
    let preset = Preset::of_plugin(
        "Harmony",
        PluginPreset {
            state: PluginPresetState::Effect {
                params: windfall_project::EffectParams::PitchCorrect(
                    windfall_dsp::spectral::PitchCorrectParams {
                        harmony_channel: 123,
                        ..Default::default()
                    },
                ),
                impulse: None,
            },
        },
    );
    let (loaded, notes) = Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap();
    assert_eq!(loaded, preset);
    assert!(notes.is_empty());
}
