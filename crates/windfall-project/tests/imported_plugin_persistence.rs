//! Live hosted instances and opaque imported source states have different owners.
use windfall_project::{file, *};

fn owner(binding: &PluginBinding) -> ChannelId {
    let PluginTarget::Instrument { channel } = binding.target else {
        panic!("the fixture is an instrument binding")
    };
    channel
}

#[test]
fn imported_source_state_and_missing_live_instances_survive_file_and_history_roundtrips() {
    let folder = tempfile::tempdir().unwrap();
    let plugin_path = folder.path().join("not-installed.clap");
    assert!(!plugin_path.exists());
    let mut seed = Document::new(Project::new("Recovered arrangement"));
    seed.dispatch(
        Command::AddPluginInstrument {
            plugin: PluginBinding {
                sidechain_input: None,
                auxiliary_inputs: Vec::new(),
                // AddPluginInstrument allocates the actual owner, independent of this hint.
                target: PluginTarget::Instrument {
                    channel: ChannelId(999),
                },
                format: "clap".into(),
                path: plugin_path.to_string_lossy().into_owned(),
                id: "org.windfall.test.missing-instrument".into(),
                name: "Missing live synth".into(),
                state: vec![b'W', b'F', b'P', b'S', 1, 0, 255, 0, 17],
                // Deliberately out of id order: native ids are not parameter indices.
                parameters: vec![
                    PluginParameter {
                        id: 9001,
                        name: "Frequency".into(),
                        min: 20.0,
                        max: 20_000.0,
                        value: 440.0,
                        stepped: false,
                        read_only: false,
                        automatable: true,
                    },
                    PluginParameter {
                        id: 17,
                        name: "Voices".into(),
                        min: 1.0,
                        max: 8.0,
                        value: 2.0,
                        stepped: true,
                        read_only: false,
                        automatable: true,
                    },
                ],
            },
        },
        None,
    )
    .unwrap();
    let original_channel = owner(&seed.project().plugins[0]);
    let retained = RetainedPluginState {
        source: "flStudio".into(),
        internal_name: "Legacy wrapper".into(),
        name: Some("Imported unavailable synth".into()),
        vendor: Some("Fixture vendor".into()),
        format: Some("VST2".into()),
        path: Some("C:\\old-machine\\unavailable.dll".into()),
        channel: Some(original_channel),
        track: None,
        slot: None,
        state: vec![0, 255, 127, 1, 0, 254],
    };
    let mut project = seed.project().clone();
    project.retained_plugins.push(retained.clone());
    project.check().unwrap();
    let file_path = folder.path().join("combined.windfall");
    file::save(&project, &file_path).unwrap();
    let reopened = file::load(&file_path).unwrap();
    assert_eq!(
        reopened, project,
        "missing binaries must not discard either state type"
    );
    let mut doc = Document::new(reopened);
    let original = doc.project().plugins[0].clone();

    doc.dispatch(
        Command::DuplicateChannel {
            id: original_channel,
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().plugins.len(), 2);
    let copy = doc
        .project()
        .plugins
        .iter()
        .find(|p| p.target != original.target)
        .unwrap()
        .clone();
    let copy_channel = owner(&copy);
    assert_ne!(copy_channel, original_channel);
    assert_eq!(copy.state, original.state);
    assert_eq!(copy.parameters, original.parameters);
    // Import metadata describes the original source; duplication must not clone or retarget it.
    assert_eq!(
        doc.project().retained_plugins.as_slice(),
        std::slice::from_ref(&retained)
    );
    doc.dispatch(
        Command::SetPluginParam {
            target: copy.target,
            id: 17,
            value: 3.0,
        },
        None,
    )
    .unwrap();
    let changed_copy = doc.project().plugin(copy.target).unwrap().clone();
    assert_eq!(changed_copy.parameters[0], original.parameters[0]);
    assert_eq!(changed_copy.parameters[1].id, 17);
    assert_eq!(changed_copy.parameters[1].value, 3.0);
    assert_eq!(doc.project().plugin(original.target), Some(&original));

    doc.dispatch(
        Command::RemoveChannel {
            id: original_channel,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().channel(original_channel).is_none());
    assert_eq!(
        doc.project().plugins.as_slice(),
        std::slice::from_ref(&changed_copy)
    );
    assert_eq!(
        doc.project().retained_plugins.as_slice(),
        std::slice::from_ref(&retained)
    );
    doc.project().check().unwrap(); // Descriptive source ids may now be dangling.
    file::save(doc.project(), &file_path).unwrap();
    assert_eq!(file::load(&file_path).unwrap(), *doc.project());
    doc.mark_saved();
    assert!(!doc.is_dirty());
    doc.undo().unwrap();
    assert!(doc.is_dirty());
    assert_eq!(doc.project().plugin(original.target), Some(&original));
    assert_eq!(doc.project().plugin(copy.target), Some(&changed_copy));
    assert_eq!(
        doc.project().retained_plugins.as_slice(),
        std::slice::from_ref(&retained)
    );
    doc.redo().unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.project().plugins, [changed_copy]);

    // The file has no undo history: on a reopened document, deletion and undo
    // still restore live ownership without interpreting or rewriting imported bytes.
    let mut reopened = Document::new(file::load(&file_path).unwrap());
    reopened
        .dispatch(Command::RemoveChannel { id: copy_channel }, None)
        .unwrap();
    assert!(reopened.project().plugins.is_empty());
    assert_eq!(
        reopened.project().retained_plugins.as_slice(),
        std::slice::from_ref(&retained)
    );
    reopened.undo().unwrap();
    assert_eq!(owner(&reopened.project().plugins[0]), copy_channel);
    assert_eq!(reopened.project().plugins[0].state, original.state);
    assert_eq!(reopened.project().retained_plugins, [retained]);
    reopened.project().check().unwrap();
    file::save(reopened.project(), &file_path).unwrap();
    assert_eq!(file::load(&file_path).unwrap(), *reopened.project());
}

#[test]
fn empty_legacy_files_need_neither_plugin_field_and_remain_byte_compatible() {
    let empty = Project::new("Legacy empty project");
    let json = file::to_json(&empty).unwrap();
    let encoded: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(encoded.get("plugins").is_none());
    assert!(encoded.get("retainedPlugins").is_none());
    let loaded = file::from_json(&json).unwrap();
    assert!(loaded.plugins.is_empty());
    assert!(loaded.retained_plugins.is_empty());
    assert_eq!(file::to_json(&loaded).unwrap(), json);
}
