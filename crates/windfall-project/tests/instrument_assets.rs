use windfall_dsp::drums::{DrumMode, DrumVoiceParams};
use windfall_dsp::{DrumKeyPatch, InstrumentAsset};
use windfall_project::*;

fn add(doc: &mut Document) -> ChannelId {
    ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::DrumVoice),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    )
}
#[test]
fn immutable_bank_edits_have_history_stale_guards_and_disk_persistence() {
    let mut doc = Document::new(Project::new("Key programs"));
    let channel = add(&mut doc);
    let expected = doc.project().channel(channel).unwrap().source.clone();
    let command = Command::SetInstrumentAsset {
        channel,
        expected: expected.clone(),
        asset: Some(InstrumentAsset::DrumKeys {
            patches: vec![DrumKeyPatch {
                key: 127,
                params: DrumVoiceParams {
                    mode: DrumMode::Hat,
                    ..Default::default()
                },
            }],
        }),
    };
    doc.dispatch(command.clone(), None).unwrap();
    let edited = doc.project().clone();
    let cursor = doc.history().cursor;
    assert!(doc.dispatch(command, None).is_err());
    assert_eq!(doc.project(), &edited);
    assert_eq!(doc.history().cursor, cursor);
    doc.undo().unwrap();
    assert_eq!(doc.project().channel(channel).unwrap().source, expected);
    doc.redo().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("programs.windfall");
    file::save(doc.project(), &path).unwrap();
    assert_eq!(file::load(&path).unwrap(), edited);
    // Ordinary scalar parameter edits must retain immutable data.
    doc.dispatch(
        Command::SetInstrumentParam {
            channel,
            param: 5,
            value: 0.6,
        },
        None,
    )
    .unwrap();
    let ChannelSource::Instrument { asset, .. } = &doc.project().channel(channel).unwrap().source
    else {
        panic!()
    };
    assert!(asset.is_some());
}
#[test]
fn duplicate_unsorted_and_invalid_key_programs_are_atomically_refused() {
    let mut doc = Document::new(Project::new("Invalid programs"));
    let channel = add(&mut doc);
    for keys in [vec![36, 36], vec![127, 36], vec![128]] {
        let before = doc.project().clone();
        let expected = before.channel(channel).unwrap().source.clone();
        let asset = InstrumentAsset::DrumKeys {
            patches: keys
                .into_iter()
                .map(|key| DrumKeyPatch {
                    key,
                    params: DrumVoiceParams::default(),
                })
                .collect(),
        };
        assert!(doc
            .dispatch(
                Command::SetInstrumentAsset {
                    channel,
                    expected,
                    asset: Some(asset)
                },
                None
            )
            .is_err());
        assert_eq!(doc.project(), &before);
    }
}
#[test]
fn legacy_instrument_json_omits_assets_and_loads_without_them() {
    let mut doc = Document::new(Project::new("Legacy"));
    add(&mut doc);
    let json = serde_json::to_value(doc.project()).unwrap();
    assert!(json["channels"][0]["source"].get("asset").is_none());
    let restored: Project = serde_json::from_value(json).unwrap();
    restored.check().unwrap();
    assert_eq!(restored, *doc.project());
}

#[test]
fn instrument_plugin_presets_capture_replace_validate_and_undo_immutable_data() {
    use windfall_project::preset::{PluginPreset, PluginPresetState};
    let mut doc = Document::new(Project::new("Preset assets"));
    let channel = add(&mut doc);
    let target = PluginTarget::Instrument { channel };
    let empty = PluginPreset::capture(doc.project(), target).unwrap();
    let expected = doc.project().channel(channel).unwrap().source.clone();
    let asset = InstrumentAsset::DrumKeys {
        patches: vec![DrumKeyPatch {
            key: 127,
            params: DrumVoiceParams {
                mode: DrumMode::Hat,
                ..Default::default()
            },
        }],
    };
    doc.dispatch(
        Command::SetInstrumentAsset {
            channel,
            expected,
            asset: Some(asset.clone()),
        },
        None,
    )
    .unwrap();
    let captured = PluginPreset::capture(doc.project(), target).unwrap();
    let encoded = serde_json::to_string(&captured).unwrap();
    let captured: PluginPreset = serde_json::from_str(&encoded).unwrap();
    assert!(
        matches!(&captured.state,PluginPresetState::Instrument{asset:Some(stored),..} if stored==&asset)
    );
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected: captured.state.clone(),
            preset: empty,
        },
        None,
    )
    .unwrap();
    assert!(matches!(
        &doc.project().channel(channel).unwrap().source,
        ChannelSource::Instrument { asset: None, .. }
    ));
    doc.undo().unwrap();
    assert_eq!(
        PluginPreset::capture(doc.project(), target).unwrap(),
        captured
    );
    let mut invalid = captured.clone();
    if let PluginPresetState::Instrument { asset, .. } = &mut invalid.state {
        *asset = Some(InstrumentAsset::Pcm(
            windfall_dsp::instrument_asset::LayerPcm::from_samples(48000, &[0.0, 0.5]),
        ));
    }
    assert!(invalid.validate().is_err());
    let before = doc.project().clone();
    assert!(doc
        .dispatch(
            Command::ApplyPluginPreset {
                target,
                expected: captured.state,
                preset: invalid
            },
            None
        )
        .is_err());
    assert_eq!(doc.project(), &before);
}
