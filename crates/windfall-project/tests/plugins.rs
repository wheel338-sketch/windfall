use windfall_project::*;

fn binding() -> PluginBinding {
    PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
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
fn audio_unit_registry_bindings_survive_history_and_portable_project_reopen() {
    let mut plugin = binding();
    plugin.format = "au".into();
    plugin.id = "61756D75:646C7320:6170706C".into();
    plugin.path = format!("au://{}", plugin.id);
    let mut document = Document::new(Project::new("Audio Units"));
    document
        .dispatch(Command::AddPluginInstrument { plugin }, None)
        .unwrap();
    let expected = document.project().plugins[0].clone();
    document.undo().unwrap();
    assert!(document.project().plugins.is_empty());
    document.redo().unwrap();
    assert_eq!(document.project().plugins[0], expected);
    let reopened = file::from_json(&file::to_json(document.project()).unwrap()).unwrap();
    assert_eq!(reopened.plugins[0], expected);
    reopened.check().unwrap();
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
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None)
        .unwrap();
    let original = doc.project().plugins[0].clone();
    let json = file::to_json(doc.project()).unwrap();
    assert!(!json.contains("\"wrapper\""));
    assert_eq!(
        file::from_json(&json).unwrap().plugins[0].wrapper,
        PluginWrapperOptions::default()
    );
    let options = PluginWrapperOptions {
        smart_disable: true,
        fixed_buffer: Some(128),
        processing: PluginProcessingMode::Bridged,
        editor_scale: 1.5,
        detached: false,
    };
    doc.dispatch(
        Command::SetPluginWrapper {
            target: original.target,
            options,
        },
        None,
    )
    .unwrap();
    let changed = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&changed).unwrap()).unwrap(),
        changed
    );
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins[0], original);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &changed);
    let PluginTarget::Instrument { channel } = original.target else {
        panic!()
    };
    doc.dispatch(Command::DuplicateChannel { id: channel }, None)
        .unwrap();
    assert_eq!(doc.project().plugins[1].wrapper, options);
    let before = doc.project().clone();
    for options in [
        PluginWrapperOptions {
            fixed_buffer: Some(0),
            ..Default::default()
        },
        PluginWrapperOptions {
            editor_scale: f32::NAN,
            ..Default::default()
        },
    ] {
        assert!(
            doc.dispatch(
                Command::SetPluginWrapper {
                    target: original.target,
                    options
                },
                None
            )
            .is_err()
        );
        assert_eq!(doc.project(), &before);
    }
}

#[test]
fn readmission_keeps_tombstone_addresses_automation_links_routes_and_history() {
    let mut doc = Document::new(Project::new("Dynamic capabilities"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None).unwrap();
    let original = doc.project().plugins[0].clone();
    let PluginTarget::Instrument { channel } = original.target else { unreachable!() };
    let target = AutomationTarget::InstrumentParam { channel, param: 0 };
    doc.dispatch(Command::AddAutomation { name: None, target, points: None }, None).unwrap();
    doc.dispatch(Command::SetControllerLinks { links: vec![ControllerLink {
        source: ControlSource::Midi { device: None, channel: None, control: MidiControl::Cc { number: 7 } },
        target: LinkTarget::Param { target }, map: Default::default(),
    }] }, None).unwrap();
    let links = doc.project().settings.controller_links.clone();
    let automations = doc.project().automations.clone();
    let mut new = original.parameters[0].clone(); new.id = 99; new.name = "New control".into();
    let readmit = |parameters| Command::ReadmitPlugin { target: original.target, state: vec![4, 5], parameters, auxiliary_inputs: vec![], outputs: vec![PluginAuxInput { index: 2, name: "Extra".into(), channels: 2 }] };
    doc.dispatch(readmit(vec![new.clone()]), None).unwrap();
    assert_eq!(doc.project().plugins[0].parameters.iter().map(|param| param.id).collect::<Vec<_>>(), [43, 99]);
    assert_eq!(doc.project().plugins[0].capabilities.unavailable_parameters, [43]);
    assert_eq!(doc.project().automations, automations);
    assert_eq!(doc.project().settings.controller_links, links);
    doc.dispatch(Command::SetPluginOutputRoute { target: original.target, output: 2, track: Some(TrackId::MASTER) }, None).unwrap();
    let routed = doc.project().plugins[0].clone();
    doc.dispatch(Command::ReadmitPlugin { target: original.target, state: vec![6], parameters: vec![new.clone()], auxiliary_inputs: vec![], outputs: vec![] }, None).unwrap();
    assert_eq!(doc.project().plugins[0].capabilities.routes, routed.capabilities.routes);
    assert_eq!(doc.project().plugins[0].capability_diagnostics().len(), 2);
    let reopened = file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
    assert_eq!(reopened.plugins, doc.project().plugins); reopened.check().unwrap();
    doc.undo().unwrap(); assert_eq!(doc.project().plugins[0], routed);
    doc.redo().unwrap();
    doc.dispatch(readmit(vec![new, original.parameters[0].clone()]), None).unwrap();
    assert_eq!(doc.project().plugins[0].parameters.iter().map(|param| param.id).collect::<Vec<_>>(), [43, 99]);
    assert!(doc.project().plugins[0].capabilities.unavailable_parameters.is_empty());
    assert!(doc.project().plugins[0].capability_diagnostics().is_empty());
    assert_eq!(doc.project().automations, automations);
    assert_eq!(doc.project().settings.controller_links, links);
}

#[test]
fn legacy_binding_omits_capabilities_and_loads_with_empty_admission() {
    let mut value = serde_json::to_value(binding()).unwrap();
    assert!(value.get("capabilities").is_none());
    value.as_object_mut().unwrap().remove("wrapper");
    let binding: PluginBinding = serde_json::from_value(value).unwrap();
    assert!(binding.capabilities.is_default()); binding.validate().unwrap();
}

#[test]
fn undo_and_discarded_redo_cannot_reuse_a_dynamic_parameter_address() {
    let mut doc = Document::new(Project::new("Permanent addresses"));
    doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None).unwrap();
    let target = doc.project().plugins[0].target;
    let mut param = doc.project().plugins[0].parameters[0].clone(); param.id = 99;
    let command = |parameters| Command::ReadmitPlugin { target, state: vec![], parameters, auxiliary_inputs: vec![], outputs: vec![] };
    doc.dispatch(command(vec![param.clone()]), None).unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins[0].parameters[1].id, 99);
    assert_eq!(doc.project().plugins[0].capabilities.unavailable_parameters, [99]);
    param.id = 100;
    doc.dispatch(command(vec![param]), None).unwrap();
    assert_eq!(doc.project().plugins[0].parameters.iter().map(|p| p.id).collect::<Vec<_>>(), [43, 99, 100]);
    let before = doc.project().clone();
    let mut duplicate = before.plugins[0].parameters[0].clone(); duplicate.id = 101;
    assert!(doc.dispatch(command(vec![duplicate.clone(), duplicate]), None).is_err());
    assert_eq!(doc.project(), &before);
}
