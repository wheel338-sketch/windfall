//! Written, not executed. Real checked session / synthetic worker boundaries.
use super::Rig;
use windfall_project::proposal::CommandProposal;
use windfall_project::scripting::{
    ControllerScript, PianoScriptRequest, ScriptAsset, ScriptKind, ScriptLibrary, ScriptReference,
};
use windfall_project::{ChannelId, Command, SettingsPatch};

#[test]
fn assistant_review_apply_guarded_undo_and_stale_preparation_are_atomic() {
    let rig = Rig::new();
    let original = rig.project();
    let proposal = rig
        .session
        .assistant_respond("set tempo 127")
        .unwrap()
        .proposal
        .unwrap();
    assert_eq!(rig.project(), original);
    let applied = rig.session.proposal_apply(proposal.clone()).unwrap();
    assert_eq!(rig.project().settings.tempo_bpm, 127.0);
    assert!(rig.session.proposal_apply(proposal.clone()).is_err());
    rig.session
        .proposal_undo(windfall_project::proposal::ProposalStamp {
            revision: applied.patch.revision,
            ..proposal.stamp
        })
        .unwrap();
    assert_eq!(rig.project().settings, original.settings);
    let proposal = rig
        .session
        .assistant_respond("create pattern Verse")
        .unwrap()
        .proposal
        .unwrap();
    let hold = rig.session.hold("proposal:prepared");
    let worker = rig
        .session
        .background(move |session| session.proposal_apply(proposal));
    hold.wait();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(130.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let current = rig.project();
    hold.release();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(rig.project(), current);
}

#[test]
fn invalid_proposal_rolls_back_ids_history_and_replacement_invalidates_approval() {
    let rig = Rig::new();
    let before = rig.session.document_snapshot();
    let proposal = CommandProposal {
        stamp: rig.session.proposal_stamp(),
        label: "Failure".into(),
        explanation: String::new(),
        commands: vec![
            Command::AddPattern { name: None },
            Command::UpdateChannel {
                id: ChannelId(u32::MAX),
                patch: Default::default(),
            },
        ],
    };
    assert!(rig.session.proposal_apply(proposal).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
    let proposal = rig
        .session
        .assistant_respond("set tempo 127")
        .unwrap()
        .proposal
        .unwrap();
    rig.session.project_new().unwrap();
    let replacement = rig.session.document_snapshot();
    assert!(rig.session.proposal_apply(proposal).is_err());
    assert_eq!(rig.session.document_snapshot(), replacement);
}

#[test]
fn piano_worker_selected_fixture_preview_apply_undo_cancel_and_save() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    let created = rig
        .session
        .dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![windfall_project::NoteInit {
                    start: 0,
                    length: 240,
                    key: 60,
                    velocity: Some(0.5),
                    pan: Some(-0.25),
                    expression: None,
                }],
            },
            None,
        )
        .unwrap()
        .created;
    let input = PianoScriptRequest {
        stamp: rig.session.proposal_stamp(),
        pattern,
        channel,
        selected: vec![windfall_project::NoteId(created[0])],
        source: "param amount -24 24 12\nselected\nset key key amount +\nemit\nend".into(),
        parameters: vec![7.0],
        replace: true,
    };
    let before = rig.session.document_snapshot();
    let request = rig.session.script_begin();
    let result = rig.session.script_preview(request, input.clone()).unwrap();
    assert_eq!(result.notes[0].key, 67);
    assert_eq!(rig.session.document_snapshot(), before);
    let applied = rig.session.proposal_apply(result.proposal).unwrap();
    assert_eq!(applied.patch.history.cursor, before.history.cursor + 1);
    let edited = rig.project();
    let notes = &edited
        .pattern(pattern)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.channel == channel)
        .unwrap()
        .notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].key, 67);
    assert_ne!(notes[0].id.0, created[0]);
    rig.session.undo().unwrap();
    assert_eq!(
        rig.project().pattern(pattern),
        before.project.pattern(pattern)
    );
    let request = rig.session.script_begin();
    rig.session.script_cancel(request);
    assert!(rig.session.script_preview(request, input).is_err());
    let reference = ScriptReference {
        id: "saved-piano".into(),
        revision: 1,
    };
    rig.session
        .dispatch(
            Command::SetScriptLibrary {
                library: ScriptLibrary {
                    assets: vec![ScriptAsset {
                        reference,
                        language_version: 1,
                        kind: ScriptKind::Piano,
                        name: "Saved".into(),
                        source: "emit".into(),
                    }],
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let scripts = rig.project().settings.scripts;
    let path = rig.file("scripts.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    rig.session.project_open(&path).unwrap();
    assert_eq!(rig.project().settings.scripts, scripts);
}

#[test]
fn controller_script_pump_maps_actions_and_refuses_partial_output() {
    use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
    let rig = Rig::new();
    let reference = ScriptReference {
        id: "controller".into(),
        revision: 1,
    };
    let source = windfall_project::ControlSource::Midi {
        device: None,
        channel: Some(1),
        control: windfall_project::MidiControl::Cc { number: 7 },
    };
    let library = ScriptLibrary {
        assets: vec![ScriptAsset {
            reference: reference.clone(),
            language_version: 1,
            kind: ScriptKind::Controller,
            name: "Controller".into(),
            source: "tempo value 100 * 60 +".into(),
        }],
        controllers: vec![ControllerScript {
            map: windfall_project::ControlMap { pickup: windfall_project::PickupMode::Off, ..Default::default() },
            source,
            script: reference,
            parameters: vec![],
            target: None,
            feedback_channel: None,
        }],
        ..Default::default()
    };
    rig.session
        .dispatch(Command::SetScriptLibrary { library }, None)
        .unwrap();
    let ordinary = rig.channel(0);
    rig.session
        .dispatch(
            Command::SetControllerLinks {
                links: vec![windfall_project::ControllerLink {
                    source: windfall_project::ControlSource::Midi {
                        device: None,
                        channel: Some(1),
                        control: windfall_project::MidiControl::Cc { number: 8 },
                    },
                    target: windfall_project::LinkTarget::Param {
                        target: windfall_project::AutomationTarget::ChannelVolume {
                            channel: ordinary,
                        },
                    },
                    map: Default::default(),
                }],
            },
            None,
        )
        .unwrap();
    rig.session.controllers_pump(std::time::Instant::now());
    let before = rig.session.document_snapshot();
    rig.session.controller_message(InputMessage {
        device: 0,
        message: MidiMessage::Controller {
            channel: 0,
            number: 8,
            value: 127,
        },
        at: std::time::Instant::now(),
        stamp_micros: 0,
    });
    assert!(rig.session.controller_message(InputMessage {
        device: 0,
        message: MidiMessage::Controller {
            channel: 0,
            number: 7,
            value: 127
        },
        at: std::time::Instant::now(),
        stamp_micros: 0
    }));
    rig.session.controllers_pump(std::time::Instant::now());
    assert_eq!(rig.project().settings.tempo_bpm, 160.0);
    assert_eq!(
        rig.project().channel(ordinary).unwrap().volume,
        windfall_project::MAX_GAIN
    );
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    rig.session.undo().unwrap();
    assert_eq!(
        rig.project().settings.tempo_bpm,
        before.project.settings.tempo_bpm
    );
    assert_eq!(
        rig.project().channel(ordinary),
        before.project.channel(ordinary)
    );
    let mut library = rig.project().settings.scripts;
    library.assets[0].reference.revision = 2;
    library.controllers[0].script.revision = 2;
    library.assets[0].source = "tempo 140\nmap 1 0 /".into();
    rig.session
        .dispatch(Command::SetScriptLibrary { library }, None)
        .unwrap();
    rig.session.controllers_pump(std::time::Instant::now());
    let before = rig.session.document_snapshot();
    rig.session.controller_message(InputMessage {
        device: 0,
        message: MidiMessage::Controller {
            channel: 0,
            number: 8,
            value: 0,
        },
        at: std::time::Instant::now(),
        stamp_micros: 0,
    });
    rig.session.controller_message(InputMessage {
        device: 0,
        message: MidiMessage::Controller {
            channel: 0,
            number: 7,
            value: 127,
        },
        at: std::time::Instant::now(),
        stamp_micros: 0,
    });
    rig.session.controllers_pump(std::time::Instant::now());
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.controller_state().error.is_some());
}

fn installed_script(rig: &Rig, control: windfall_project::MidiControl, target: windfall_project::LinkTarget,
    map: windfall_project::ControlMap, source_text: &str) {
    let script = ScriptReference { id: "recorded-controller".into(), revision: 1 };
    let library = ScriptLibrary {
        assets: vec![ScriptAsset { reference: script.clone(), language_version: 1, kind: ScriptKind::Controller,
            name: "Recorded controller stream".into(), source: source_text.into() }],
        controllers: vec![ControllerScript { source: windfall_project::ControlSource::Midi {
            device: None, channel: Some(1), control }, script, parameters: vec![], target: Some(target),
            feedback_channel: None, map }], ..Default::default()
    };
    rig.session.dispatch(Command::SetScriptLibrary { library }, None).unwrap();
}
fn recorded_packet(rig: &Rig, bytes: &[u8], at: std::time::Instant) {
    use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
    assert!(rig.session.controller_message(InputMessage { device: 0, message: MidiMessage::parse(bytes).unwrap(), at, stamp_micros: 0 }));
}

#[test]
fn recorded_script_stream_pickup_smoothing_gestures_and_reopen_match_link_runtime() {
    use windfall_project::{AutomationTarget, ControlMap, LinkTarget, MidiControl, PickupMode};
    use std::time::{Duration, Instant};
    let rig = Rig::new();
    let channel = rig.channel(0);
    let target = AutomationTarget::ChannelPan { channel };
    installed_script(&rig, MidiControl::Cc { number: 7 }, LinkTarget::Param { target },
        ControlMap { pickup: PickupMode::On, smoothing_ms: 100.0, ..Default::default() }, "map value");
    let start = Instant::now();
    rig.session.controllers_pump(start);
    let before = rig.session.document_snapshot();
    recorded_packet(&rig, &[0xb0, 7, 0], start);
    rig.session.controllers_pump(start);
    assert_eq!(rig.project().channel(channel).unwrap().pan, 0.0);
    assert_eq!(rig.session.controller_state().waiting, vec![target]);
    rig.session.controllers_pump(start + Duration::from_millis(10));
    assert_eq!(rig.session.controller_state().waiting, vec![target]);
    recorded_packet(&rig, &[0xb0, 7, 64], start + Duration::from_millis(20));
    rig.session.controllers_pump(start + Duration::from_millis(20));
    recorded_packet(&rig, &[0xb0, 7, 127], start + Duration::from_millis(30));
    rig.session.controllers_pump(start + Duration::from_millis(30));
    let old = rig.project().channel(channel).unwrap().pan;
    rig.session.controllers_pump(start + Duration::from_millis(130));
    let expected = old + (1.0 - old) * (1.0 - (-1.0_f32).exp());
    assert!((rig.project().channel(channel).unwrap().pan - expected).abs() < 1e-6);
    assert!(rig.session.controller_state().waiting.is_empty());
    assert_eq!(rig.session.document_snapshot().history.cursor, before.history.cursor + 1);
    let edited = rig.project();
    rig.session.undo().unwrap();
    assert_eq!(rig.project().channel(channel).unwrap().pan, 0.0);
    rig.session.redo().unwrap();
    let saved = rig.file("controller-stream.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project(), edited);
}

#[test]
fn recorded_script_wheel_stream_records_pattern_automation_in_the_same_gesture() {
    use windfall_project::{AutomationTarget, ControlMap, LinkTarget, MidiControl, PickupMode};
    use windfall_ipc::{PlayMode, TransportPatch};
    let mut rig = Rig::new();
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    let target = AutomationTarget::ChannelPan { channel };
    installed_script(&rig, MidiControl::PitchBend, LinkTarget::Param { target },
        ControlMap { pickup: PickupMode::Off, ..Default::default() }, "map value");
    rig.session.controllers_pump(std::time::Instant::now());
    rig.session.transport_set(TransportPatch { mode: Some(PlayMode::Pattern), pattern: Some(pattern), ..Default::default() }).unwrap();
    rig.session.transport_play().unwrap();
    rig.run(480);
    let head = rig.session.controller().playhead();
    let now = std::time::Instant::now();
    rig.session.controller_idle(now, &head);
    rig.session.automation_record_arm(true);
    let before = rig.session.document_snapshot();
    for (ms, bytes) in [(0, [0xe0, 0, 0]), (20, [0xe0, 0, 64]), (40, [0xe0, 127, 127])] {
        let at = now + std::time::Duration::from_millis(ms);
        recorded_packet(&rig, &bytes, at);
        rig.session.controllers_pump(at);
    }
    rig.session.automation_record_arm(false);
    let project = rig.project();
    let lane = project.pattern(pattern).unwrap().events.iter().find(|lane| lane.target == target).unwrap();
    assert_eq!(lane.points.last().unwrap().value, 1.0);
    assert!(lane.points.iter().any(|point| point.value == 0.0));
    assert_eq!(rig.session.document_snapshot().history.cursor, before.history.cursor + 1);
    rig.session.undo().unwrap();
    assert_eq!(rig.project().pattern(pattern).unwrap().events, before.project.pattern(pattern).unwrap().events);
    rig.session.redo().unwrap();
    let file = rig.file("script-automation.windfall");
    rig.session.project_save(Some(&file)).unwrap();
    rig.session.project_open(&file).unwrap();
    assert_eq!(rig.project().pattern(pattern).unwrap().events, project.pattern(pattern).unwrap().events);
}

#[test]
fn scripted_pad_actions_use_rising_edges_and_over_budget_streams_refuse_the_whole_batch() {
    use windfall_project::{ControlMap, ControlAction, LinkTarget, MidiControl};
    let rig = Rig::new();
    installed_script(&rig, MidiControl::Note { key: 60 }, LinkTarget::Action { action: ControlAction::ToggleMetronome },
        ControlMap::default(), "map value");
    let now = std::time::Instant::now();
    rig.session.controllers_pump(now);
    let before = rig.session.transport_state().metronome.unwrap_or_default().enabled;
    recorded_packet(&rig, &[0x90, 60, 100], now);
    recorded_packet(&rig, &[0x90, 60, 100], now);
    rig.session.controllers_pump(now);
    assert_eq!(rig.session.transport_state().metronome.unwrap_or_default().enabled, !before);
    recorded_packet(&rig, &[0x80, 60, 0], now);
    rig.session.controllers_pump(now);
    let before = rig.session.document_snapshot();
    for _ in 0..33 { recorded_packet(&rig, &[0x90, 60, 100], now); }
    rig.session.controllers_pump(now);
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.controller_state().error.unwrap().contains("budget"));
}

#[test]
fn recording_and_worker_busy_exclusions_are_explicit() {
    let rig = Rig::new();
    // Synthetic active admission flag exercises the same recording gate as
    // recording completion without opening any device or writing a take.
    rig.session
        .inner
        .recording_finishing
        .store(true, std::sync::atomic::Ordering::Release);
    let p = rig
        .session
        .assistant_respond("set tempo 127")
        .unwrap()
        .proposal
        .unwrap();
    let before = rig.project();
    assert!(rig.session.proposal_apply(p).is_err());
    assert_eq!(rig.project(), before);
    rig.session
        .inner
        .recording_finishing
        .store(false, std::sync::atomic::Ordering::Release);
    let request = rig.session.script_begin();
    let _gate = crate::sync::lock(&rig.session.inner.script_worker);
    let input = PianoScriptRequest {
        stamp: rig.session.proposal_stamp(),
        pattern: rig.pattern(),
        channel: rig.channel(0),
        selected: vec![],
        source: "emit".into(),
        parameters: vec![],
        replace: false,
    };
    assert!(
        rig.session
            .script_preview(request, input)
            .unwrap_err()
            .contains("already running")
    );
}

#[test]
fn proposal_recording_admission_is_rechecked_after_engine_preparation() {
    let rig = Rig::new();
    let proposal = rig
        .session
        .assistant_respond("create pattern Locked")
        .unwrap()
        .proposal
        .unwrap();
    let before = rig.session.document_snapshot();
    let hold = rig.session.hold("proposal:prepared");
    let worker = rig
        .session
        .background(move |session| session.proposal_apply(proposal));
    hold.wait();
    rig.session
        .inner
        .recording_finishing
        .store(true, std::sync::atomic::Ordering::Release);
    hold.release();
    let result = worker.join().unwrap();
    rig.session
        .inner
        .recording_finishing
        .store(false, std::sync::atomic::Ordering::Release);
    assert!(result.is_err());
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn byok_fake_endpoint_is_reviewed_atomic_stale_guarded_and_never_saved_in_project() {
    use crate::assistant::tests::{completion, fake};
    use windfall_ipc::AssistantConnectionUpdate;
    let rig = Rig::new();
    let before = rig.session.document_snapshot();
    let request = rig.session.assistant_request_begin();
    assert!(
        rig.session
            .assistant_remote(request, "help")
            .unwrap_err()
            .contains("OFF")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let content = r#"{"answer":"New pattern","label":"Pattern","explanation":"Review pattern","commands":[{"type":"addPattern","name":"BYOK Verse"}]}"#;
    let (url, server) = fake(completion(content), std::time::Duration::ZERO);
    let configure = |url| AssistantConnectionUpdate {
        base_url: url,
        model: "fake-model".into(),
        enabled: true,
        api_key: Some("session-private-key".into()),
    };
    rig.session.assistant_configure(configure(url)).unwrap();
    let request = rig.session.assistant_request_begin();
    let reply = rig
        .session
        .assistant_remote(request, "create pattern BYOK Verse")
        .unwrap();
    server.join().unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    let proposal = reply.proposal.unwrap();
    let result = rig.session.proposal_apply(proposal.clone()).unwrap();
    assert_eq!(result.patch.history.cursor, before.history.cursor + 1);
    assert!(rig.session.proposal_apply(proposal.clone()).is_err());
    rig.session
        .proposal_undo(windfall_project::proposal::ProposalStamp {
            revision: result.patch.revision,
            ..proposal.stamp
        })
        .unwrap();
    assert_eq!(rig.project().patterns, before.project.patterns);
    let text = windfall_project::file::to_json(&rig.project()).unwrap();
    assert!(!text.contains("session-private-key"));
    assert!(!text.contains("fake-model"));
    assert!(
        std::fs::read_to_string(rig.file("assistant-connection.json"))
            .unwrap()
            .contains("session-private-key")
    );

    // A later invalid command must not leak the earlier valid command/ID.
    let bad = r#"{"answer":"Bad batch","label":"Bad","explanation":"Must reject","commands":[{"type":"addPattern","name":"Never created"},{"type":"updateChannel","id":4294967295,"patch":{"muted":true}}]}"#;
    let (url, server) = fake(completion(bad), std::time::Duration::ZERO);
    rig.session.assistant_configure(configure(url)).unwrap();
    let before = rig.session.document_snapshot();
    let request = rig.session.assistant_request_begin();
    assert!(rig.session.assistant_remote(request, "bad batch").is_err());
    server.join().unwrap();
    assert_eq!(rig.session.document_snapshot(), before);

    let (url, server) = fake(completion(content), std::time::Duration::from_millis(200));
    rig.session.assistant_configure(configure(url)).unwrap();
    let request = rig.session.assistant_request_begin();
    let hold = rig.session.hold("assistant:request-snapshot");
    let job = rig
        .session
        .background(move |s| s.assistant_remote(request, "create pattern"));
    hold.wait();
    rig.session
        .dispatch(
            Command::AddPattern {
                name: Some("Concurrent edit".into()),
            },
            None,
        )
        .unwrap();
    let before = rig.session.document_snapshot();
    hold.release();
    assert!(job.join().unwrap().unwrap_err().contains("changed"));
    server.join().unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
}
