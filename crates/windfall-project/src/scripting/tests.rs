use super::*;
use crate::proposal::{CommandProposal, ProposalStamp};
use crate::{Command, Document, Project, SettingsPatch};

#[test]
fn interpreter_postfix_parameters_selected_and_generation_fixtures() {
    let program = Program::compile(
        "param transpose -24 24 12\nselected\nset key key transpose + 0 127 clamp\nemit\nend",
    )
    .unwrap();
    let input = [
        ScriptNote {
            key: 60,
            start: 240,
            ..Default::default()
        },
        ScriptNote {
            key: 126,
            ..Default::default()
        },
    ];
    let run = || {
        program
            .run(
                ScriptInput {
                    selected: &input,
                    ..Default::default()
                },
                RunLimits::default(),
                || false,
            )
            .unwrap()
    };
    let result = run();
    assert_eq!(result, run());
    assert_eq!(
        result.notes.iter().map(|n| n.key).collect::<Vec<_>>(),
        vec![72, 127]
    );
    assert_eq!(result.notes[0].start, 240);
    let generation =
        Program::compile("repeat 4\nset start index 240 *\nset key 60 index +\nemit\nend")
            .unwrap()
            .run(ScriptInput::default(), RunLimits::default(), || false)
            .unwrap();
    assert_eq!(
        generation
            .notes
            .iter()
            .map(|n| (n.start, n.key))
            .collect::<Vec<_>>(),
        vec![(0, 60), (240, 61), (480, 62), (720, 63)]
    );
}

#[test]
fn source_stack_step_memory_deadline_and_errors_are_bounded() {
    assert!(
        Program::compile(&"#".repeat(MAX_SOURCE_BYTES + 1))
            .unwrap_err()
            .message
            .contains("byte limit")
    );
    assert_eq!(Program::compile("set key +").unwrap_err().line, 1);
    assert!(Program::compile("repeat 2\nselected\nend\nend").is_err());
    assert!(Program::compile(&format!("set key {}", "1 ".repeat(33))).is_err());
    assert!(Program::compile(&"set key 60\n".repeat(MAX_OPS)).is_err());
    let program = Program::compile("repeat 4096\nemit\nend").unwrap();
    assert!(
        program
            .run(
                ScriptInput::default(),
                RunLimits {
                    steps: 2,
                    ..Default::default()
                },
                || false
            )
            .unwrap_err()
            .message
            .contains("step limit")
    );
    assert!(
        program
            .run(
                ScriptInput::default(),
                RunLimits {
                    notes: 1,
                    ..Default::default()
                },
                || false
            )
            .unwrap_err()
            .message
            .contains("memory limit")
    );
    assert!(
        program
            .run(ScriptInput::default(), RunLimits::default(), || true)
            .unwrap_err()
            .message
            .contains("time limit")
    );
    let zero = Program::compile("set velocity 1 0 /\nemit").unwrap();
    assert_eq!(
        zero.run(ScriptInput::default(), RunLimits::default(), || false)
            .unwrap_err()
            .line,
        1
    );
    assert!(
        Program::compile("param p 0 1 0.5\nemit")
            .unwrap()
            .validate_parameters(&[f64::NAN])
            .is_err()
    );
}

#[test]
fn compiled_transform_equals_interpreted_result_and_rejects_unbounded_surface() {
    let program = Program::compile("param amount -24 24 7\nset key key amount + 0 127 clamp\nset velocity velocity 0.5 *\nemit").unwrap();
    let transform = program.note_transform(&[]).unwrap();
    for key in 0..=127 {
        let input = ScriptNote {
            key,
            ..Default::default()
        };
        let interpreted = program
            .run(
                ScriptInput {
                    selected: &[],
                    ..Default::default()
                },
                RunLimits::default(),
                || false,
            )
            .unwrap();
        let selected = Program::compile("param amount -24 24 7\nselected\nset key key amount + 0 127 clamp\nset velocity velocity 0.5 *\nemit\nend").unwrap().run(ScriptInput { selected: &[input], ..Default::default() }, RunLimits::default(), || false).unwrap();
        assert_eq!(transform.evaluate(input).unwrap(), selected.notes[0]);
        if key == 60 {
            assert_eq!(selected.notes, interpreted.notes);
        }
    }
    for source in [
        "repeat 2\nemit\nend",
        "tempo 120\nemit",
        "map value\nemit",
        "emit\nemit",
    ] {
        assert!(
            Program::compile(source)
                .unwrap()
                .note_transform(&[])
                .is_err()
        );
    }
}

#[test]
fn bounded_note_copies_preserve_lengths_offsets_keys_and_refuse_partial_results() {
    let transform = Program::compile("set length length 2 *\ncopies 3 7 240\nemit")
        .unwrap()
        .note_transform(&[])
        .unwrap();
    let input = ScriptNote {
        start: 120,
        length: 180,
        key: 60,
        ..Default::default()
    };
    let notes = transform.evaluate_notes(input).unwrap();
    assert_eq!(
        notes
            .as_slice()
            .iter()
            .map(|n| (n.start, n.length, n.key))
            .collect::<Vec<_>>(),
        vec![(120, 360, 60), (360, 360, 67), (600, 360, 74)]
    );
    assert_eq!(
        transform
            .evaluate_notes(ScriptNote { key: 126, ..input })
            .unwrap_err(),
        TransformError::NoteBounds
    );
    assert_eq!(
        transform
            .evaluate_notes(ScriptNote {
                start: crate::MAX_PATTERN_TICKS - 240,
                ..input
            })
            .unwrap_err(),
        TransformError::NoteBounds
    );
    assert!(Program::compile("copies 17 0 0\nemit").is_err());
    assert!(
        Program::compile("copies 2 0 0\ncopies 2 0 0\nemit")
            .unwrap()
            .note_transform(&[])
            .is_err()
    );
}

#[test]
fn legacy_controller_bindings_default_to_immediate_takeover_and_new_maps_roundtrip() {
    let mut value = serde_json::json!({"source":{"type":"midi","channel":1,"control":{"type":"cc","number":7}},
        "script":{"id":"legacy","revision":1},"parameters":[],"target":null,"feedbackChannel":null});
    let legacy: ControllerScript = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(legacy.map.pickup, crate::PickupMode::Off);
    value["map"] =
        serde_json::json!({"pickup":"on","smoothingMs":250,"mode":"absolute","min":0,"max":1});
    let bound: ControllerScript = serde_json::from_value(value).unwrap();
    assert_eq!(bound.map.smoothing_ms, 250.0);
    assert_eq!(
        serde_json::from_str::<ControllerScript>(&serde_json::to_string(&bound).unwrap()).unwrap(),
        bound
    );
}

#[test]
fn scripted_clip_expansion_past_song_bound_is_an_atomic_checked_refusal() {
    let mut document = Document::new(Project::new("Script clip extent"));
    let channel = crate::ChannelId(
        document
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(crate::InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let pattern = document.project().patterns[0].id;
    document
        .dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![crate::NoteInit {
                    start: 0,
                    length: 240,
                    key: 60,
                    velocity: None,
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
    let track = crate::PlaylistTrackId(
        document
            .dispatch(
                Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    document
        .dispatch(
            Command::AddClips {
                clips: vec![crate::ClipInit {
                    track,
                    start: crate::MAX_SONG_TICKS - 3840,
                    length: Some(3840),
                    offset: None,
                    muted: None,
                    content: crate::ClipContent::Pattern { pattern },
                }],
            },
            None,
        )
        .unwrap();
    let script = ScriptReference {
        id: "extent".into(),
        revision: 1,
    };
    let before = document.snapshot(None);
    let result = document.dispatch(
        Command::SetScriptLibrary {
            library: ScriptLibrary {
                assets: vec![ScriptAsset {
                    reference: script.clone(),
                    language_version: 1,
                    kind: ScriptKind::NoteTransform,
                    name: "Extent".into(),
                    source: "copies 2 0 3840\nemit".into(),
                }],
                channels: vec![ChannelScript {
                    channel,
                    script,
                    parameters: vec![],
                }],
                controllers: vec![],
        parameter_addresses: vec![],
            },
        },
        None,
    );
    assert!(result.unwrap_err().to_string().contains("song bound"));
    assert_eq!(document.snapshot(None), before);
}

#[test]
fn controller_fixture_maps_cc_and_emits_checked_tempo_and_feedback() {
    let program =
        Program::compile("map 1 value -\ntempo value 100 * 60 +\nfeedback 7 value 127 *").unwrap();
    let result = program
        .run(
            ScriptInput {
                value: 0.5,
                number: 7,
                ..Default::default()
            },
            RunLimits::default(),
            || false,
        )
        .unwrap();
    assert_eq!(result.mapped, Some(0.5));
    assert_eq!(result.tempos, vec![110.0]);
    assert_eq!(result.feedback, vec![[0xb0, 7, 64]]);
    assert!(
        Program::compile("feedback 1 128")
            .unwrap()
            .run(ScriptInput::default(), RunLimits::default(), || false)
            .is_err()
    );
}

fn proposal(document: &Document, commands: Vec<Command>) -> CommandProposal {
    CommandProposal {
        stamp: ProposalStamp {
            generation: 1,
            revision: document.revision(),
        },
        label: "Reviewed batch".into(),
        explanation: "Fixture".into(),
        commands,
    }
}

#[test]
fn proposal_registry_stale_recording_size_and_failed_batch_preserve_everything() {
    let mut document = Document::new(Project::new("Proposal"));
    let before = document.snapshot(None);
    let good = Command::AddPattern {
        name: Some("New".into()),
    };
    let bad = Command::UpdateChannel {
        id: crate::ChannelId(u32::MAX),
        patch: Default::default(),
    };
    let p = proposal(&document, vec![good.clone(), bad]);
    assert!(p.apply(&mut document, p.stamp, false).is_err());
    assert_eq!(document.snapshot(None), before);
    let p = proposal(&document, vec![good.clone()]);
    assert!(p.apply(&mut document, p.stamp, true).is_err());
    assert!(
        p.apply(
            &mut document,
            ProposalStamp {
                generation: 2,
                ..p.stamp
            },
            false
        )
        .is_err()
    );
    assert!(proposal(&document, vec![good; 129]).batch().is_err());
    assert!(
        proposal(
            &document,
            vec![Command::Batch {
                label: None,
                commands: vec![]
            }]
        )
        .batch()
        .is_err()
    );
    assert!(
        proposal(
            &document,
            vec![Command::AddPattern {
                name: Some("x".repeat(524_288))
            }]
        )
        .batch()
        .is_err()
    );
    assert!(
        proposal(
            &document,
            vec![Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("Not allowed".into()),
                    ..Default::default()
                }
            }]
        )
        .batch()
        .is_err()
    );
    assert_eq!(document.snapshot(None), before);
}

#[test]
fn successful_proposal_is_one_undo_step_and_cannot_replay_after_patch() {
    let mut document = Document::new(Project::new("Batch"));
    let original = document.project().clone();
    let p = proposal(
        &document,
        vec![
            Command::AddPattern { name: None },
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(127.0),
                    ..Default::default()
                },
            },
        ],
    );
    let applied = p.apply(&mut document, p.stamp, false).unwrap();
    document.patch(&applied.touched);
    assert_eq!(document.history().entries.len(), 1);
    assert!(p.apply(&mut document, p.stamp, false).is_err());
    document.undo().unwrap();
    assert_eq!(document.project().patterns, original.patterns);
    assert_eq!(document.project().settings, original.settings);
    document.redo().unwrap();
    assert_eq!(document.project().settings.tempo_bpm, 127.0);
}

#[test]
fn script_revisions_save_reopen_undo_and_reject_unknown_language() {
    let mut document = Document::new(Project::new("Scripts"));
    let asset = ScriptAsset {
        reference: ScriptReference {
            id: "transpose".into(),
            revision: 1,
        },
        language_version: 1,
        kind: ScriptKind::NoteTransform,
        name: "Transpose".into(),
        source: "set key key 12 + 0 127 clamp\nemit".into(),
    };
    let library = ScriptLibrary {
        assets: vec![asset.clone()],
        ..Default::default()
    };
    document
        .dispatch(
            Command::SetScriptLibrary {
                library: library.clone(),
            },
            None,
        )
        .unwrap();
    let loaded =
        crate::file::from_json(&crate::file::to_json(document.project()).unwrap()).unwrap();
    assert_eq!(loaded.settings.scripts, library);
    let mut changed = library.clone();
    changed.assets[0].source = "emit".into();
    assert!(
        document
            .dispatch(Command::SetScriptLibrary { library: changed }, None)
            .is_err()
    );
    document.undo().unwrap();
    assert!(document.project().settings.scripts.is_empty());
    document.redo().unwrap();
    let mut future = library;
    future.assets[0].language_version = 99;
    assert!(future.check().is_err());
    let legacy = crate::file::to_json(&Project::new("Legacy")).unwrap();
    assert!(!legacy.contains("scripts"));
    assert!(
        crate::file::from_json(&legacy)
            .unwrap()
            .settings
            .scripts
            .is_empty()
    );
}

#[test]
fn ineffective_proposal_preserves_prior_history_and_redo() {
    let mut document = Document::new(Project::new("No-op"));
    document
        .dispatch(Command::AddPattern { name: None }, None)
        .unwrap();
    document.undo().unwrap();
    let before = document.snapshot(None);
    let p = proposal(
        &document,
        vec![Command::UpdateSettings {
            patch: SettingsPatch {
                tempo_bpm: Some(document.project().settings.tempo_bpm),
                ..Default::default()
            },
        }],
    );
    assert!(
        p.apply(&mut document, p.stamp, false)
            .unwrap_err()
            .contains("no changes")
    );
    assert_eq!(document.snapshot(None), before);
    assert!(document.redo().is_some());
}

#[test]
fn controller_adapter_checks_target_range_opt_in_and_message_budget() {
    let reference = ScriptReference {
        id: "device-fixture".into(),
        revision: 1,
    };
    let mut project = Project::new("Controller");
    let mut binding = ControllerScript {
        map: crate::ControlMap {
            pickup: crate::PickupMode::Off,
            ..Default::default()
        },
        source: crate::ControlSource::Midi {
            device: None,
            channel: Some(1),
            control: crate::MidiControl::Cc { number: 7 },
        },
        script: reference.clone(),
        parameters: vec![],
        target: Some(crate::LinkTarget::Slot {
            slot: crate::SlotTarget::MasterVolume,
        }),
        feedback_channel: None,
    };
    project.settings.scripts.assets.push(ScriptAsset {
        reference,
        language_version: SCRIPT_VERSION,
        kind: ScriptKind::Controller,
        name: "Device".into(),
        source: "map 1 value -\nfeedback 7 value 127 *".into(),
    });
    let disabled = controller_event(&project, &binding, 1.0, 7, 0, || false).unwrap();
    assert!(disabled.feedback.is_empty());
    let mut document = Document::new(project.clone());
    document
        .dispatch(
            Command::Batch {
                label: None,
                commands: disabled.commands,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        document
            .project()
            .mixer
            .track(crate::TrackId::MASTER)
            .unwrap()
            .volume,
        0.0
    );
    binding.feedback_channel = Some(16);
    assert_eq!(
        controller_event(&project, &binding, 0.5, 7, 0, || false)
            .unwrap()
            .feedback,
        vec![[0xbf, 7, 64]]
    );
    binding.feedback_channel = Some(0);
    assert!(controller_event(&project, &binding, 0.5, 7, 0, || false).is_err());
    binding.feedback_channel = None;
    binding.target = None;
    assert!(controller_event(&project, &binding, 0.5, 7, 0, || false).is_err());
    project.settings.scripts.assets[0].source = "repeat 17\nfeedback 7 0\nend".into();
    assert!(
        controller_event(&project, &binding, 0.5, 7, 0, || false)
            .unwrap_err()
            .contains("memory limit")
    );
}

#[test]
fn assigned_transform_rejects_invalid_notes_rolls_back_ids_and_suspends_on_delete() {
    let mut document = Document::new(Project::new("Transform admission"));
    let created = document
        .dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(crate::InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created;
    let channel = crate::ChannelId(created[0]);
    let pattern = document.project().patterns[0].id;
    let reference = ScriptReference {
        id: "unsafe-octave".into(),
        revision: 1,
    };
    let library = ScriptLibrary {
        assets: vec![ScriptAsset {
            reference: reference.clone(),
            language_version: SCRIPT_VERSION,
            kind: ScriptKind::NoteTransform,
            name: "Octave".into(),
            source: "set key key 12 +\nemit".into(),
        }],
        channels: vec![ChannelScript {
            channel,
            script: reference,
            parameters: vec![],
        }],
        ..Default::default()
    };
    document
        .dispatch(
            Command::SetScriptLibrary {
                library: library.clone(),
            },
            None,
        )
        .unwrap();
    let before = document.snapshot(None);
    let notes = vec![crate::NoteInit {
        start: 0,
        length: 240,
        key: 126,
        velocity: None,
        pan: None,
        expression: None,
    }];
    assert!(
        document
            .dispatch(
                Command::Batch {
                    label: None,
                    commands: vec![
                        Command::AddPattern { name: None },
                        Command::AddNotes {
                            pattern,
                            channel,
                            notes
                        }
                    ]
                },
                None
            )
            .is_err()
    );
    assert_eq!(document.snapshot(None), before);
    document
        .dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert_eq!(document.project().settings.scripts, library);
    document.project().check().unwrap();
    document.undo().unwrap();
    assert!(document.project().channel(channel).is_some());
    assert_eq!(document.project().settings.scripts, library);
    let mut invalid = library;
    invalid.channels[0].channel = crate::ChannelId(u32::MAX);
    assert!(
        document
            .dispatch(Command::SetScriptLibrary { library: invalid }, None)
            .is_err()
    );
}

#[test]
fn script_parameter_addresses_are_checked_append_only_undoable_and_reopen_with_tombstones() {
    let mut document = Document::new(Project::new("Script parameters"));
    let channel = crate::ChannelId(document.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: None, index: None, mixer_track: None }, None).unwrap().created[0]);
    let reference = ScriptReference { id: "parameterized".into(), revision: 1 };
    let library = ScriptLibrary { assets: vec![ScriptAsset { reference: reference.clone(), language_version: 1,
        kind: ScriptKind::NoteTransform, name: "Parameterized".into(),
        source: "param transpose 0 12 0\nset key key transpose + 0 127 clamp\nemit".into() }],
        channels: vec![ChannelScript { channel, script: reference, parameters: vec![] }], ..Default::default() };
    document.dispatch(Command::SetScriptLibrary { library }, None).unwrap();
    let target = crate::AutomationTarget::InstrumentParam { channel, param: SCRIPT_PARAMETER_BASE };
    assert_eq!(document.project().automation_range(&target).unwrap().max, 12.0);
    document.dispatch(Command::SetInstrumentParam { channel, param: SCRIPT_PARAMETER_BASE, value: 7.0 }, Some(5)).unwrap();
    assert_eq!(document.project().automation_stored_value(&target), Some(7.0));
    document.undo().unwrap();
    assert_eq!(document.project().automation_stored_value(&target), Some(0.0));
    document.redo().unwrap();
    let before = document.snapshot(None);
    assert!(document.dispatch(Command::SetInstrumentParam { channel, param: SCRIPT_PARAMETER_BASE, value: 13.0 }, None).is_err());
    assert_eq!(document.snapshot(None), before);
    let saved = document.project().settings.scripts.clone();
    let mut next = saved.clone();
    next.assets.push(ScriptAsset { reference: ScriptReference { id: "parameterized".into(), revision: 2 },
        source: "param gain 0 1 1\nparam transpose 0 12 0\nset velocity velocity gain *\nset key key transpose + 0 127 clamp\nemit".into(),
        ..saved.assets[0].clone() });
    next.channels[0].script.revision = 2;
    next.channels[0].parameters.clear();
    document.dispatch(Command::SetScriptLibrary { library: next }, None).unwrap();
    let registry = &document.project().settings.scripts.parameter_addresses;
    assert_eq!(registry[0].name, "transpose");
    assert_eq!(registry[1].name, "gain");
    assert_eq!(document.project().settings.scripts.parameter(channel, SCRIPT_PARAMETER_BASE).unwrap().0, 1);
    let mut removed = document.project().settings.scripts.clone();
    removed.channels.clear();
    document.dispatch(Command::SetScriptLibrary { library: removed }, None).unwrap();
    assert!(document.project().automation_range(&target).is_none());
    assert_eq!(document.project().settings.scripts.parameter_addresses.len(), 2);
    document.undo().unwrap();
    let reopened = crate::file::from_json(&crate::file::to_json(document.project()).unwrap()).unwrap();
    assert_eq!(reopened, *document.project());
    assert_eq!(reopened.settings.scripts.parameter_addresses[0].name, "transpose");
}

#[test]
fn script_time_extent_is_proven_over_parameter_bounds_and_invalid_division_is_refused() {
    let mut transform = Program::compile("param delay 0 480 0\nparam gate 1 960 240\nset start start delay +\nset length gate\ncopies 3 7 60\nemit")
        .unwrap().note_transform(&[]).unwrap();
    let note = ScriptNote { start: 240, ..Default::default() };
    assert_eq!(transform.maximum_end(note), Ok(1800));
    assert!(transform.set_parameter(0, 480.0));
    assert!(transform.set_parameter(1, 960.0));
    assert_eq!(transform.evaluate_notes(note).unwrap().as_slice()[2].start, 840);
    assert_eq!(transform.evaluate_notes(note).unwrap().as_slice()[2].length, 960);
    assert!(!transform.set_parameter(0, f32::NAN));
    let unbounded = Program::compile("param divisor -1 1 1\nset length length divisor /\nemit").unwrap().note_transform(&[]).unwrap();
    assert_eq!(unbounded.maximum_end(note), Err(TransformError::Arithmetic));
}

#[test]
fn legacy_parameterized_baked_scores_load_and_unsafe_time_automation_is_refused_atomically() {
    let mut document = Document::new(Project::new("legacy script"));
    let channel = crate::ChannelId(document.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: None, index: None, mixer_track: None }, None).unwrap().created[0]);
    let mut project = document.project().clone();
    let note = crate::NoteId(project.next_id);
    project.next_id += 1;
    project.patterns[0].lanes.push(crate::Lane { channel, notes: vec![crate::Note { id: note,
        start: 480, length: 240, key: 60, velocity: 0.8, pan: 0.0, expression: Default::default() }] });
    let reference = ScriptReference { id: "legacy".into(), revision: 1 };
    project.settings.scripts = ScriptLibrary { assets: vec![ScriptAsset { reference: reference.clone(), language_version: 1,
        kind: ScriptKind::NoteTransform, name: "Legacy".into(), source: "param amount 0 1 1\nset start 0\nemit".into() }],
        channels: vec![ChannelScript { channel, script: reference, parameters: vec![] }], ..Default::default() };
    let reopened = crate::file::from_json(&crate::file::to_json(&project).unwrap()).unwrap();
    assert_eq!(reopened.settings.scripts.parameter_addresses.len(), 1);
    let mut document = Document::new(reopened);
    let before = document.snapshot(None);
    assert!(document.dispatch(Command::AddAutomation { name: None,
        target: crate::AutomationTarget::InstrumentParam { channel, param: SCRIPT_PARAMETER_BASE }, points: None }, None).is_err());
    assert_eq!(document.snapshot(None), before);
}
