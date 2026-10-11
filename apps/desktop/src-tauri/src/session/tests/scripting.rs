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
        .assistant_respond("set tempo 120")
        .unwrap()
        .proposal
        .unwrap();
    assert_eq!(rig.project(), original);
    let applied = rig.session.proposal_apply(proposal.clone()).unwrap();
    assert_eq!(rig.project().settings.tempo_bpm, 120.0);
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
        .assistant_respond("set tempo 120")
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
    rig.session.controllers_pump(std::time::Instant::now());
    let before = rig.session.document_snapshot();
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
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    rig.session.undo().unwrap();
    assert_eq!(
        rig.project().settings.tempo_bpm,
        before.project.settings.tempo_bpm
    );
    let mut library = rig.project().settings.scripts;
    library.assets[0].reference.revision = 2;
    library.controllers[0].script.revision = 2;
    library.assets[0].source = "tempo 120\nmap 1 0 /".into();
    rig.session
        .dispatch(Command::SetScriptLibrary { library }, None)
        .unwrap();
    rig.session.controllers_pump(std::time::Instant::now());
    let before = rig.session.document_snapshot();
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
        .assistant_respond("set tempo 120")
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
