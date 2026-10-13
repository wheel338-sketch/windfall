//! Written, not executed: graph validation, history, persistence and ownership.
use windfall_dsp::rack_note::RackNoteModule;
use windfall_project::rack::*;
use windfall_project::*;

fn node(id: u32, processor: RackProcessor) -> RackNode {
    RackNode {
        id,
        name: format!("Module {id}"),
        position: [id as f32 * 10.0, 0.0],
        muted: false,
        solo: false,
        hosted: None,
        asset: None,
        processor,
    }
}
fn edge(from: u32, port: u16, to: u32, input: u16) -> RackEdge {
    RackEdge {
        from: RackEndpoint { node: from, port },
        to: RackEndpoint {
            node: to,
            port: input,
        },
        gain: 1.0,
        parameter: None,
    }
}
fn graph() -> RackGraph {
    RackGraph {
        parameters: Vec::new(),
        next_id: 5,
        nodes: vec![
            node(1, RackProcessor::NoteInput),
            node(
                2,
                RackProcessor::Instrument {
                    params: InstrumentKind::SubtractiveSynth.default_params(),
                },
            ),
            node(
                3,
                RackProcessor::Effect {
                    params: EffectKind::Polarity.default_params(),
                    mix: 1.0,
                },
            ),
            node(4, RackProcessor::AudioOutput),
        ],
        edges: vec![
            edge(1, NOTE_OUT, 2, NOTE_IN),
            edge(2, AUDIO_OUT, 3, AUDIO_IN),
            edge(3, AUDIO_OUT, 4, AUDIO_IN),
        ],
    }
}
fn document() -> (Document, ChannelId) {
    let mut document = Document::new(Project::new("Modular"));
    let channel = ChannelId(
        document
            .dispatch(
                Command::AddChannel {
                    name: Some("Rack".into()),
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    sample: None,
                    index: None,
                    mixer_track: Some(TrackId::MASTER),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    (document, channel)
}

#[test]
fn rack_node_assets_are_owned_portable_checked_and_undoable_with_legacy_defaults() {
    let (mut doc, channel) = document();
    let mut rack = graph();
    rack.nodes[1].processor = RackProcessor::Instrument { params: InstrumentKind::LayerKick.default_params() };
    rack.nodes[1].asset = Some(RackNodeAsset::Instrument { asset: windfall_dsp::InstrumentAsset::Pcm(windfall_dsp::instrument_asset::LayerPcm::from_samples(48000, &[0.25, -0.5, 0.125])) });
    rack.nodes[2].processor = RackProcessor::Effect { params: EffectKind::Convolver.default_params(), mix: 1.0 };
    rack.nodes[2].asset = Some(RackNodeAsset::Impulse { impulse: ConvolverImpulse { name: "Owned IR".into(), sample_rate: 48000, group_delay: 0, eq_gains: None, frames: vec![[1.0, 0.5], [0.25, -0.125]] } });
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    let saved = doc.project().clone();
    let reopened: Project = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    reopened.check().unwrap();
    assert_eq!(reopened.settings.modular_racks[0].graph, rack);
    let (body, _) = ChannelPreset::capture(&saved, channel).unwrap();
    let file = Preset::of_channel("Owned rack", body, None);
    let loaded = Preset::from_json(file.to_json().unwrap().as_bytes()).unwrap().0;
    assert_eq!(loaded.channel.unwrap().rack, Some(rack.clone()));
    let previous = rack.clone();
    rack.nodes[2].asset = Some(RackNodeAsset::Impulse { impulse: ConvolverImpulse { name: "Replacement".into(), sample_rate: 48000, group_delay: 0, eq_gains: None, frames: vec![[0.125; 2]] } });
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project().settings.modular_racks[0].graph, previous);
    doc.redo().unwrap();
    assert_eq!(doc.project().settings.modular_racks[0].graph, rack);
    rack.nodes[2].asset = previous.nodes[1].asset.clone();
    let before = doc.project().clone();
    assert!(set(&mut doc, channel, Some(rack)).is_err());
    assert_eq!(*doc.project(), before);
    let mut legacy = serde_json::to_value(graph()).unwrap();
    for node in legacy["nodes"].as_array_mut().unwrap() { node.as_object_mut().unwrap().remove("asset"); }
    let legacy: RackGraph = serde_json::from_value(legacy).unwrap();
    assert!(legacy.nodes.iter().all(|n| n.asset.is_none()));
}

#[test]
fn rack_sample_zones_preserve_missing_sources_and_relink_as_one_history_step() {
    let (mut doc, channel) = document();
    let mut rack = graph();
    rack.nodes[1].processor = RackProcessor::Instrument { params: InstrumentKind::MultiSampler.default_params() };
    let missing = RackNodeAsset::Zones { zones: vec![RackSampleZone {
        mapping: windfall_dsp::sample_bank::SampleZone::default(),
        audio: RackAudioAsset { name: "Missing take.wav".into(), sample_rate: 48000, frames: None },
    }] };
    rack.nodes[1].asset = Some(missing.clone());
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    assert_eq!(missing.missing_sources(), ["Missing take.wav"]);
    let RackNodeAsset::Zones { zones } = rack.nodes[1].asset.as_mut().unwrap() else { unreachable!() };
    zones[0].audio.frames = Some(vec![[0.25, -0.5]; 32]);
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    assert!(rack.nodes[1].asset.as_ref().unwrap().missing_sources().is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project().settings.modular_racks[0].graph.nodes[1].asset, Some(missing));
    doc.redo().unwrap();
    let restored = &doc.project().settings.modular_racks[0].graph;
    assert_eq!(restored, &rack);
    let RackNodeAsset::Zones { zones } = rack.nodes[1].asset.as_mut().unwrap() else { unreachable!() };
    zones[0].mapping.sample = 44;
    assert!(set(&mut doc, channel, Some(rack)).is_err());
}

#[test]
fn nested_hosted_node_owners_capture_state_parameters_presets_and_removal_history_without_aliasing() {
    let (mut doc, channel) = document();
    let mut inner = RackGraph { next_id: 2, nodes: vec![node(1, RackProcessor::Effect { params: EffectKind::Polarity.default_params(), mix: 1.0 })], ..Default::default() };
    inner.nodes[0].hosted = Some(PluginBinding {
        capabilities: Default::default(),
        wrapper: Default::default(), target: PluginTarget::Effect { effect: EffectId(0) }, format: "clap".into(), path: "C:/owned/fixture.clap".into(), id: "org.windfall.test.bridge-delayed".into(), name: "Owned fixture".into(), state: vec![10, 20, 30], parameters: vec![PluginParameter { id: 7, name: "Gain".into(), min: 0.0, max: 1.0, value: 0.5, stepped: false, read_only: false, automatable: true }], sidechain_input: None, auxiliary_inputs: Vec::new(),
    });
    let mut rack = RackGraph { next_id: 2, nodes: vec![node(1, RackProcessor::Rack { graph: Box::new(inner) })], ..Default::default() };
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    let target = PluginTarget::Rack { channel, path: [1, 1, 0, 0], instrument: false };
    assert_eq!(doc.project().plugin_bindings().len(), 1);
    assert_eq!(doc.project().plugin(target).unwrap().state, [10, 20, 30]);
    doc.dispatch(Command::SetPluginParam { target, id: 7, value: 0.75 }, None).unwrap();
    assert_eq!(doc.project().plugin(target).unwrap().parameters[0].value, 0.75);
    doc.undo().unwrap(); assert_eq!(doc.project().plugin(target).unwrap().parameters[0].value, 0.5);
    doc.redo().unwrap(); assert_eq!(doc.project().plugin(target).unwrap().parameters[0].value, 0.75);
    doc.dispatch(Command::SetPluginState { target, state: vec![40, 50] }, None).unwrap();
    let before = doc.project().clone();
    let loaded = file::from_json(&file::to_json(&before).unwrap()).unwrap();
    assert_eq!(loaded.plugin(target).unwrap().state, [40, 50]);
    let mut relinked = before.settings.modular_racks[0].graph.clone();
    let mut provider = before.plugin(target).unwrap().clone(); provider.path = "D:/new machine/fixture.clap".into(); provider.state.clear(); provider.parameters[0].value = 0.0;
    relinked.replace_hosted_provider(&[1, 1], provider).unwrap();
    let binding = relinked.node_at(&[1, 1]).unwrap().hosted.as_ref().unwrap();
    assert_eq!(binding.path, "D:/new machine/fixture.clap"); assert_eq!(binding.state, [40, 50]); assert_eq!(binding.parameters[0].value, 0.75);
    let (body, _) = ChannelPreset::capture(&before, channel).unwrap();
    let preset = Preset::of_channel("Hosted rack", body, None);
    assert_eq!(Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap().0.channel.unwrap().rack, Some(before.settings.modular_racks[0].graph.clone()));
    rack.nodes.clear(); set(&mut doc, channel, Some(rack)).unwrap();
    assert!(doc.project().plugin(target).is_none());
    doc.undo().unwrap(); assert_eq!(doc.project().plugin(target).unwrap().state, [40, 50]);
    doc.redo().unwrap(); assert!(doc.project().plugin(target).is_none());
}
fn set(
    document: &mut Document,
    id: ChannelId,
    graph: Option<RackGraph>,
) -> Result<Applied, CommandError> {
    document.dispatch(
        Command::SetChannelRack {
            id,
            graph,
            expected: None,
        },
        None,
    )
}

#[test]
fn note_parameter_registry_is_append_only_nested_checked_and_restored_with_lanes_and_links() {
    let (mut doc, channel) = document();
    let mut inner = RackGraph {
        next_id: 3,
        ..Default::default()
    };
    inner.nodes.push(node(
        1,
        RackProcessor::Note {
            module: RackNoteModule::KeyMapper {
                params: Default::default(),
                chord: [0; 4],
                voices: 1,
            },
        },
    ));
    inner.nodes.push(node(
        2,
        RackProcessor::Note {
            module: RackNoteModule::Envelope {
                params: Default::default(),
            },
        },
    ));
    let mut rack = graph();
    rack.nodes[2].processor = RackProcessor::Rack {
        graph: Box::new(inner),
    };
    // This nested graph is note-only: disconnect its old audio wires.
    rack.edges
        .retain(|edge| edge.to.node != 3 && edge.from.node != 3);
    set(&mut doc, channel, Some(rack)).unwrap();
    let rack = doc.project().settings.modular_racks[0].graph.clone();
    let address = rack
        .parameters
        .iter()
        .position(|a| a.path == [3, 1] && a.parameter == 60)
        .unwrap();
    let param = 32 + address as u32;
    let target = AutomationTarget::InstrumentParam { channel, param };
    assert_eq!(doc.project().automation_range(&target).unwrap().max, 127.0);
    assert_eq!(doc.project().automation_stored_value(&target), Some(60.0));
    doc.dispatch(
        Command::SetInstrumentParam {
            channel,
            param,
            value: 72.0,
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().automation_stored_value(&target), Some(72.0));
    doc.dispatch(
        Command::AddAutomation {
            target,
            name: None,
            points: None,
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::SetControllerLinks {
            links: vec![ControllerLink {
                source: ControlSource::Midi {
                    device: None,
                    channel: None,
                    control: MidiControl::Cc { number: 1 },
                },
                target: LinkTarget::Param { target },
                map: Default::default(),
            }],
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    let text = file::to_json(&before).unwrap();
    assert_eq!(file::from_json(&text).unwrap(), before);
    let mut removed = before.settings.modular_racks[0].graph.clone();
    let RackProcessor::Rack { graph: nested } = &mut removed.nodes[2].processor else {
        panic!("nested rack");
    };
    nested.nodes.remove(0);
    set(&mut doc, channel, Some(removed)).unwrap();
    assert!(doc.project().automations.is_empty());
    assert!(doc.project().settings.controller_links.is_empty());
    assert_eq!(
        doc.project().settings.modular_racks[0].graph.parameters,
        rack.parameters
    );
    assert!(doc.project().automation_range(&target).is_none());
    let mut added = doc.project().settings.modular_racks[0].graph.clone();
    let RackProcessor::Rack { graph: nested } = &mut added.nodes[2].processor else {
        panic!("nested rack");
    };
    nested.nodes.push(node(
        3,
        RackProcessor::Note {
            module: RackNoteModule::ColorRouter {
                params: Default::default(),
            },
        },
    ));
    nested.next_id = 4;
    set(&mut doc, channel, Some(added)).unwrap();
    let next = &doc.project().settings.modular_racks[0].graph;
    assert_eq!(&next.parameters[..rack.parameters.len()], rack.parameters);
    assert_eq!(next.parameters[rack.parameters.len()].path, [3, 3]);
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert!(doc.project().automation_range(&target).is_none());
    let unchanged = doc.project().clone();
    assert!(
        doc.dispatch(
            Command::SetInstrumentParam {
                channel,
                param,
                value: 24.0
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &unchanged);
}

#[test]
fn every_note_module_parameter_round_trips_checked_commands_history_and_legacy_registry_default() {
    let modules = [
        RackNoteModule::ColorRouter {
            params: Default::default(),
        },
        RackNoteModule::Envelope {
            params: Default::default(),
        },
        RackNoteModule::PropertyScaler {
            params: Default::default(),
        },
        RackNoteModule::KeySplitter {
            params: Default::default(),
            preserve_key: true,
        },
        RackNoteModule::KeyMapper {
            params: Default::default(),
            chord: [0; 4],
            voices: 1,
        },
        RackNoteModule::Sequencer {
            params: Default::default(),
        },
    ];
    for module in modules {
        let (mut doc, channel) = document();
        let mut rack = graph();
        rack.nodes.push(node(5, RackProcessor::Note { module }));
        rack.next_id = 6;
        set(&mut doc, channel, Some(rack)).unwrap();
        let before = doc.project().clone();
        let parameter = &before.settings.modular_racks[0].graph.parameters[0];
        assert_eq!(parameter.path, [5]);
        let info = module.parameter_info(0).unwrap();
        doc.dispatch(
            Command::SetInstrumentParam {
                channel,
                param: 32,
                value: info.max,
            },
            None,
        )
        .unwrap();
        let target = AutomationTarget::InstrumentParam { channel, param: 32 };
        assert_eq!(
            doc.project().automation_stored_value(&target),
            Some(info.max)
        );
        let after = doc.project().clone();
        assert_eq!(
            file::from_json(&file::to_json(&after).unwrap()).unwrap(),
            after
        );
        doc.undo().unwrap();
        assert_eq!(doc.project(), &before);
        doc.redo().unwrap();
        assert_eq!(doc.project(), &after);
    }
    let old: RackGraph = serde_json::from_str(r#"{"nextId":1,"nodes":[],"edges":[]}"#).unwrap();
    assert!(old.parameters.is_empty());
}

#[test]
fn aggregate_admission_counts_transit_nested_cartesian_fanout_and_automatable_chord_maximum() {
    // Four voices through each of four stages dispatch 1+4+16+64+256
    // transit events, although the old terminal-only check accepted 256.
    let mapper = RackNoteModule::KeyMapper {
        params: Default::default(),
        chord: [0; 4],
        voices: 1,
    };
    let mut rack = RackGraph {
        next_id: 6,
        ..Default::default()
    };
    rack.nodes = (1..=4)
        .map(|id| node(id, RackProcessor::Note { module: mapper }))
        .collect();
    rack.nodes.push(node(5, RackProcessor::NoteOutput));
    rack.edges = (1..5)
        .map(|id| edge(id, NOTE_OUT, id + 1, NOTE_IN))
        .collect();
    assert!(rack.check().unwrap_err().contains("aggregate fan-out"));
    let inner = RackGraph {
        next_id: 3,
        nodes: vec![
            node(1, RackProcessor::NoteInput),
            node(2, RackProcessor::NoteOutput),
        ],
        edges: vec![edge(1, NOTE_OUT, 2, NOTE_IN)],
        ..Default::default()
    };
    rack.nodes[2].processor = RackProcessor::Rack {
        graph: Box::new(inner),
    };
    rack.check().unwrap(); // three chord stages have 1+4+16+64 plus buses
    let inner = RackGraph {
        next_id: 5,
        nodes: vec![
            node(1, RackProcessor::NoteInput),
            node(2, RackProcessor::NoteInput),
            node(3, RackProcessor::NoteOutput),
            node(4, RackProcessor::NoteOutput),
        ],
        edges: vec![
            edge(1, NOTE_OUT, 3, NOTE_IN),
            edge(1, NOTE_OUT, 4, NOTE_IN),
            edge(2, NOTE_OUT, 3, NOTE_IN),
            edge(2, NOTE_OUT, 4, NOTE_IN),
        ],
        ..Default::default()
    };
    rack.nodes[2].processor = RackProcessor::Rack {
        graph: Box::new(inner),
    };
    assert!(rack.check().unwrap_err().contains("aggregate fan-out"));
}

#[test]
fn ninth_serial_envelope_is_refused_before_publication_without_changing_history() {
    let (mut doc, channel) = document();
    let before = doc.project().clone();
    let history = doc.history();
    let rack = RackGraph {
        next_id: 10,
        nodes: (1..=9)
            .map(|id| {
                node(
                    id,
                    RackProcessor::Note {
                        module: RackNoteModule::Envelope {
                            params: Default::default(),
                        },
                    },
                )
            })
            .collect(),
        edges: (1..9)
            .map(|id| edge(id, NOTE_OUT, id + 1, NOTE_IN))
            .collect(),
        ..Default::default()
    };
    assert!(
        set(&mut doc, channel, Some(rack))
            .unwrap_err()
            .to_string()
            .contains("eight composed")
    );
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history(), history);
}

#[test]
fn auxiliary_ports_are_typed_checked_persistent_and_refuse_unadvertised_outputs() {
    let mut rack = graph();
    rack.nodes[2].processor = RackProcessor::Effect {
        params: EffectKind::BandSplit.default_params(),
        mix: 1.0,
    };
    for port in 0..3 {
        rack.edges[2].from.port = AUX_AUDIO_BASE + port;
        rack.check().unwrap();
        let preset = RackPreset::new("Split".into(), rack.clone()).unwrap();
        assert_eq!(
            RackPreset::from_json(&serde_json::to_string(&preset).unwrap()).unwrap(),
            preset
        );
    }
    rack.edges[2].from.port = AUX_AUDIO_BASE + 3;
    assert_eq!(rack.check().unwrap_err(), "Unknown rack output port");
}

#[test]
fn creating_a_rack_does_not_reassign_old_instrument_parameter_lanes_to_note_addresses() {
    let (mut doc, channel) = document();
    let target = AutomationTarget::InstrumentParam { channel, param: 32 };
    assert!(doc.project().automation_range(&target).is_some());
    doc.dispatch(
        Command::AddAutomation {
            target,
            name: None,
            points: None,
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    let mut rack = graph();
    rack.next_id = 6;
    rack.nodes.push(node(
        5,
        RackProcessor::Note {
            module: RackNoteModule::ColorRouter {
                params: Default::default(),
            },
        },
    ));
    set(&mut doc, channel, Some(rack)).unwrap();
    assert!(doc.project().automation_range(&target).is_some());
    assert!(doc.project().automations.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}

#[test]
fn nested_rack_preset_and_project_round_trip_preserve_ids_wires_and_history() {
    let (mut doc, channel) = document();
    let mut nested = graph();
    nested.nodes[2].processor = RackProcessor::Rack {
        graph: Box::new(RackGraph {
            parameters: Vec::new(),
            next_id: 3,
            nodes: vec![
                node(1, RackProcessor::AudioInput),
                node(2, RackProcessor::AudioOutput),
            ],
            edges: vec![edge(1, AUDIO_OUT, 2, AUDIO_IN)],
        }),
    };
    let preset = RackPreset::new("Nested".into(), nested.clone()).unwrap();
    let text = serde_json::to_string(&preset).unwrap();
    assert_eq!(RackPreset::from_json(&text).unwrap(), preset);
    let before = doc.project().clone();
    let edit = set(&mut doc, channel, Some(nested.clone())).unwrap();
    assert!(edit.touched.settings);
    assert_eq!(doc.project().settings.modular_racks[0].graph, nested);
    let after = doc.project().clone();
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("rack.windfall");
    file::save(doc.project(), &file).unwrap();
    assert_eq!(file::load(&file).unwrap(), after);
}

#[test]
fn cycles_type_mismatch_missing_ports_and_stale_edits_are_atomic() {
    let (mut doc, channel) = document();
    set(&mut doc, channel, Some(graph())).unwrap();
    let before = doc.project().clone();
    let history = doc.history();
    let mut cycle = graph();
    cycle.edges.push(edge(3, AUDIO_OUT, 3, AUDIO_IN));
    let mut wrong_type = graph();
    wrong_type.edges.push(edge(1, NOTE_OUT, 3, AUDIO_IN));
    let mut missing = graph();
    missing.edges.push(edge(88, AUDIO_OUT, 3, AUDIO_IN));
    let mut missing_port = graph();
    missing_port.edges[1].from.port = 65535;
    for invalid in [cycle, wrong_type, missing, missing_port] {
        assert!(set(&mut doc, channel, Some(invalid)).is_err());
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
    }
    let mut stale = graph();
    stale.nodes[0].name = "Old".into();
    assert!(
        doc.dispatch(
            Command::SetChannelRack {
                id: channel,
                graph: None,
                expected: Some(stale)
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history(), history);
}

#[test]
fn macro_automation_follows_its_owner_and_removed_owner_is_undoable() {
    let (mut doc, channel) = document();
    let mut rack = graph();
    rack.next_id = 6;
    rack.nodes.push(node(
        5,
        RackProcessor::ControlInput {
            value: 0.25,
            index: 0,
        },
    ));
    rack.edges.push(RackEdge {
        parameter: Some("left".into()),
        ..edge(5, CONTROL_OUT, 3, CONTROL_IN)
    });
    set(&mut doc, channel, Some(rack.clone())).unwrap();
    let target = AutomationTarget::InstrumentParam { channel, param: 0 };
    assert_eq!(doc.project().automation_stored_value(&target), Some(0.25));
    doc.dispatch(
        Command::SetInstrumentParam {
            channel,
            param: 0,
            value: 0.75,
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().automation_stored_value(&target), Some(0.75));
    doc.dispatch(
        Command::AddAutomation {
            target,
            name: None,
            points: None,
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    // A replacement inlet at the same macro index cannot inherit the lane.
    rack.nodes.last_mut().unwrap().id = 6;
    rack.next_id = 7;
    rack.edges.last_mut().unwrap().from.node = 6;
    set(&mut doc, channel, Some(rack)).unwrap();
    assert!(doc.project().automations.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert!(doc.project().settings.modular_racks.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}

#[test]
fn bounded_graphs_refuse_excess_nodes_depth_duplicate_ids_and_note_explosion() {
    let full = RackGraph {
        parameters: Vec::new(),
        next_id: 65,
        nodes: (1..=64)
            .map(|id| node(id, RackProcessor::NoteInput))
            .collect(),
        edges: Vec::new(),
    };
    assert!(full.check().is_ok());
    let mut too_many = full.clone();
    too_many.nodes.push(node(65, RackProcessor::NoteInput));
    too_many.next_id = 66;
    assert!(too_many.check().is_err());
    let mut duplicate = graph();
    duplicate.nodes[1].id = 1;
    assert!(duplicate.check().is_err());
    let mut deep = RackGraph {
        parameters: Vec::new(),
        next_id: 1,
        ..Default::default()
    };
    for _ in 0..5 {
        deep = RackGraph {
            parameters: Vec::new(),
            next_id: 2,
            nodes: vec![node(
                1,
                RackProcessor::Rack {
                    graph: Box::new(deep),
                },
            )],
            edges: Vec::new(),
        };
    }
    assert!(deep.check().is_err());
    let mapper = RackNoteModule::KeyMapper {
        params: Default::default(),
        chord: [0, 4, 7, 12],
        voices: 4,
    };
    let exploding = RackGraph {
        parameters: Vec::new(),
        next_id: 7,
        nodes: (1..=6)
            .map(|id| node(id, RackProcessor::Note { module: mapper }))
            .collect(),
        edges: (1..6)
            .map(|id| edge(id, NOTE_OUT, id + 1, NOTE_IN))
            .collect(),
    };
    assert!(exploding.check().is_err());
}

#[test]
fn legacy_projects_omit_racks_and_duplicate_channels_copy_an_independent_graph() {
    let (mut doc, channel) = document();
    let legacy = serde_json::to_string(doc.project()).unwrap();
    assert!(!legacy.contains("modularRacks"));
    let reopened: Project = serde_json::from_str(&legacy).unwrap();
    assert!(reopened.settings.modular_racks.is_empty());
    set(&mut doc, channel, Some(graph())).unwrap();
    let duplicate = ChannelId(
        doc.dispatch(Command::DuplicateChannel { id: channel }, None)
            .unwrap()
            .created[0],
    );
    assert_eq!(doc.project().settings.modular_racks.len(), 2);
    let mut changed = graph();
    changed.nodes[0].name = "Other".into();
    set(&mut doc, duplicate, Some(changed)).unwrap();
    assert_eq!(
        doc.project()
            .settings
            .modular_racks
            .iter()
            .find(|rack| rack.channel == channel)
            .unwrap()
            .graph,
        graph()
    );
}

#[test]
fn latency_changing_control_wire_is_refused_before_publication() {
    let mut rack = graph();
    rack.nodes[2].processor = RackProcessor::Effect {
        params: EffectKind::Limiter.default_params(),
        mix: 1.0,
    };
    rack.next_id = 6;
    rack.nodes.push(node(
        5,
        RackProcessor::ControlInput {
            index: 0,
            value: 0.5,
        },
    ));
    rack.edges.push(RackEdge {
        parameter: Some("lookaheadMs".into()),
        ..edge(5, CONTROL_OUT, 3, CONTROL_IN)
    });
    assert!(rack.check().is_err());
}

#[test]
fn channel_preset_capture_keeps_nested_racks_and_loading_plain_sound_removes_them() {
    let (mut doc, channel) = document();
    let (plain, _) = preset::ChannelPreset::capture(doc.project(), channel).unwrap();
    set(&mut doc, channel, Some(graph())).unwrap();
    let (body, asset) = preset::ChannelPreset::capture(doc.project(), channel).unwrap();
    assert!(asset.is_none());
    assert_eq!(body.rack, Some(graph()));
    let preset = preset::Preset::of_channel("Modular sound", body, None);
    let json = preset.to_json().unwrap();
    let (loaded, notes) = preset::Preset::from_json(json.as_bytes()).unwrap();
    assert!(notes.is_empty());
    assert_eq!(loaded, preset);
    let before = doc.project().clone();
    let expected = doc.project().channel(channel).unwrap().clone();
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: channel,
            expected,
            preset: plain,
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().settings.modular_racks.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    let expected = doc.project().channel(channel).unwrap().clone();
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: channel,
            expected,
            preset: loaded.channel.unwrap(),
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().settings.modular_racks[0].graph, graph());
}
