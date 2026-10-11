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
fn nested_rack_preset_and_project_round_trip_preserve_ids_wires_and_history() {
    let (mut doc, channel) = document();
    let mut nested = graph();
    nested.nodes[2].processor = RackProcessor::Rack {
        graph: Box::new(RackGraph {
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
        next_id: 1,
        ..Default::default()
    };
    for _ in 0..5 {
        deep = RackGraph {
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
