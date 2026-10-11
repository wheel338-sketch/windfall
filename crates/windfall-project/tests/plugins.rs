use windfall_project::*;

fn binding() -> PluginBinding {
    PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        wrapper: Default::default(),
        target: PluginTarget::Instrument {
            channel: ChannelId(999),
        },
        format: "clap".into(),
        path: "missing/synth.clap".into(),
        id: "test.synth".into(),
        name: "Test synth".into(),
        state: vec![1, 2, 3],
        parameters: vec![PluginParameter {
            id: 43,
            name: "Frequency".into(),
            min: 20.0,
            max: 20_000.0,
            value: 440.0,
            stepped: false,
            read_only: false,
            automatable: true,
        }],
    }
}

#[test]
fn missing_plugin_data_survives_commands_and_history() {
    let mut doc = Document::new(Project::new("Plugins"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None)
        .unwrap();
    let original = doc.project().plugins[0].clone();
    let PluginTarget::Instrument { channel } = original.target else {
        panic!()
    };
    doc.project().check().unwrap();
    doc.dispatch(Command::DuplicateChannel { id: channel }, None)
        .unwrap();
    assert_eq!(doc.project().plugins.len(), 2);
    assert_eq!(doc.project().plugins[1].state, original.state);
    assert_ne!(doc.project().plugins[1].target, original.target);
    doc.undo().unwrap();
    assert_eq!(
        doc.project().plugins.as_slice(),
        std::slice::from_ref(&original)
    );
    doc.dispatch(
        Command::SetPluginParam {
            target: original.target,
            id: 43,
            value: 25_000.0,
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().plugins[0].parameters[0].value, 20_000.0);
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins[0].parameters[0].value, 440.0);
    doc.dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert!(doc.project().plugins.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins, [original]);
    doc.project().check().unwrap();
}

#[test]
fn plugin_automation_uses_native_parameter_ranges_and_names() {
    let mut doc = Document::new(Project::new("Plugins"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None)
        .unwrap();
    let PluginTarget::Instrument { channel } = doc.project().plugins[0].target else {
        panic!()
    };
    let target = AutomationTarget::InstrumentParam { channel, param: 0 };
    assert_eq!(doc.project().automation_stored_value(&target), Some(440.0));
    let range = doc.project().automation_range(&target).unwrap();
    assert_eq!(range.min, 20.0);
    assert_eq!(range.max, 20_000.0);
    doc.dispatch(
        Command::AddAutomation {
            name: None,
            target,
            points: None,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().automations[0].name.ends_with("Frequency"));
    doc.project().check().unwrap();
}

#[test]
fn rejects_duplicate_owners_and_invalid_parameter_ids() {
    let mut doc = Document::new(Project::new("Plugins"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None)
        .unwrap();
    let mut project = doc.project().clone();
    project.plugins.push(project.plugins[0].clone());
    assert!(project.check().is_err());
    let mut plugin = binding();
    plugin.parameters.push(plugin.parameters[0].clone());
    assert!(plugin.validate().is_err());
    let before = doc.project().clone();
    assert!(
        doc.dispatch(
            Command::SetPluginParam {
                target: before.plugins[0].target,
                id: 999,
                value: 1.0
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &before);
}

#[test]
fn wrapper_options_preserve_legacy_json_and_roundtrip_history_clone_and_reopen() {
    let mut doc = Document::new(Project::new("Wrapper"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None).unwrap();
    let original = doc.project().plugins[0].clone();
    let json = file::to_json(doc.project()).unwrap();
    assert!(!json.contains("\"wrapper\""));
    assert_eq!(file::from_json(&json).unwrap().plugins[0].wrapper, PluginWrapperOptions::default());
    let options = PluginWrapperOptions { smart_disable: true, fixed_buffer: Some(128),
        processing: PluginProcessingMode::Bridged, editor_scale: 1.5, detached: false };
    doc.dispatch(Command::SetPluginWrapper { target: original.target, options }, None).unwrap();
    let changed = doc.project().clone();
    assert_eq!(file::from_json(&file::to_json(&changed).unwrap()).unwrap(), changed);
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins[0], original);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &changed);
    let PluginTarget::Instrument { channel } = original.target else { panic!() };
    doc.dispatch(Command::DuplicateChannel { id: channel }, None).unwrap();
    assert_eq!(doc.project().plugins[1].wrapper, options);
    let before = doc.project().clone();
    for options in [PluginWrapperOptions { fixed_buffer: Some(0), ..Default::default() },
        PluginWrapperOptions { editor_scale: f32::NAN, ..Default::default() }] {
        assert!(doc.dispatch(Command::SetPluginWrapper { target: original.target, options }, None).is_err());
        assert_eq!(doc.project(), &before);
    }
}
