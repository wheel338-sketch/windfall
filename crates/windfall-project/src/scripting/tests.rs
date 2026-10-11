use super::*;
use crate::{Command, Document, Project, SettingsPatch};
use crate::proposal::{CommandProposal, ProposalStamp};

#[test]
fn interpreter_postfix_parameters_selected_and_generation_fixtures() {
    let program = Program::compile("param transpose -24 24 12\nselected\nset key key transpose + 0 127 clamp\nemit\nend").unwrap();
    let input = [ScriptNote { key: 60, start: 240, ..Default::default() }, ScriptNote { key: 126, ..Default::default() }];
    let run = || program.run(ScriptInput { selected: &input, ..Default::default() }, RunLimits::default(), || false).unwrap();
    let result = run();
    assert_eq!(result, run());
    assert_eq!(result.notes.iter().map(|n| n.key).collect::<Vec<_>>(), vec![72, 127]);
    assert_eq!(result.notes[0].start, 240);
    let generation = Program::compile("repeat 4\nset start index 240 *\nset key 60 index +\nemit\nend").unwrap().run(ScriptInput::default(), RunLimits::default(), || false).unwrap();
    assert_eq!(generation.notes.iter().map(|n| (n.start, n.key)).collect::<Vec<_>>(), vec![(0,60), (240,61), (480,62), (720,63)]);
}

#[test]
fn source_stack_step_memory_deadline_and_errors_are_bounded() {
    assert!(Program::compile(&"#".repeat(MAX_SOURCE_BYTES + 1)).unwrap_err().message.contains("byte limit"));
    assert_eq!(Program::compile("set key +").unwrap_err().line, 1);
    assert!(Program::compile("repeat 2\nselected\nend\nend").is_err());
    assert!(Program::compile(&format!("set key {}", "1 ".repeat(33))).is_err());
    assert!(Program::compile(&"set key 60\n".repeat(MAX_OPS)).is_err());
    let program = Program::compile("repeat 4096\nemit\nend").unwrap();
    assert!(program.run(ScriptInput::default(), RunLimits { steps: 2, ..Default::default() }, || false).unwrap_err().message.contains("step limit"));
    assert!(program.run(ScriptInput::default(), RunLimits { notes: 1, ..Default::default() }, || false).unwrap_err().message.contains("memory limit"));
    assert!(program.run(ScriptInput::default(), RunLimits::default(), || true).unwrap_err().message.contains("time limit"));
    let zero = Program::compile("set velocity 1 0 /\nemit").unwrap();
    assert_eq!(zero.run(ScriptInput::default(), RunLimits::default(), || false).unwrap_err().line, 1);
    assert!(Program::compile("param p 0 1 0.5\nemit").unwrap().validate_parameters(&[f64::NAN]).is_err());
}

#[test]
fn compiled_transform_equals_interpreted_result_and_rejects_unbounded_surface() {
    let program = Program::compile("param amount -24 24 7\nset key key amount + 0 127 clamp\nset velocity velocity 0.5 *\nemit").unwrap();
    let transform = program.note_transform(&[]).unwrap();
    for key in 0..=127 {
        let input = ScriptNote { key, ..Default::default() };
        let interpreted = program.run(ScriptInput { selected: &[], ..Default::default() }, RunLimits::default(), || false).unwrap();
        let selected = Program::compile("param amount -24 24 7\nselected\nset key key amount + 0 127 clamp\nset velocity velocity 0.5 *\nemit\nend").unwrap().run(ScriptInput { selected: &[input], ..Default::default() }, RunLimits::default(), || false).unwrap();
        assert_eq!(transform.evaluate(input).unwrap(), selected.notes[0]);
        if key == 60 { assert_eq!(selected.notes, interpreted.notes); }
    }
    for source in ["repeat 2\nemit\nend", "tempo 120\nemit", "map value\nemit", "emit\nemit"] {
        assert!(Program::compile(source).unwrap().note_transform(&[]).is_err());
    }
}

#[test]
fn controller_fixture_maps_cc_and_emits_checked_tempo_and_feedback() {
    let program = Program::compile("map 1 value -\ntempo value 100 * 60 +\nfeedback 7 value 127 *").unwrap();
    let result = program.run(ScriptInput { value: 0.5, number: 7, ..Default::default() }, RunLimits::default(), || false).unwrap();
    assert_eq!(result.mapped, Some(0.5));
    assert_eq!(result.tempos, vec![110.0]);
    assert_eq!(result.feedback, vec![[0xb0,7,64]]);
    assert!(Program::compile("feedback 1 128").unwrap().run(ScriptInput::default(), RunLimits::default(), || false).is_err());
}

fn proposal(document: &Document, commands: Vec<Command>) -> CommandProposal {
    CommandProposal { stamp: ProposalStamp { generation: 1, revision: document.revision() }, label: "Reviewed batch".into(), explanation: "Fixture".into(), commands }
}

#[test]
fn proposal_registry_stale_recording_size_and_failed_batch_preserve_everything() {
    let mut document = Document::new(Project::new("Proposal"));
    let before = document.snapshot(None);
    let good = Command::AddPattern { name: Some("New".into()) };
    let bad = Command::UpdateChannel { id: crate::ChannelId(u32::MAX), patch: Default::default() };
    let p = proposal(&document, vec![good.clone(), bad]);
    assert!(p.apply(&mut document, p.stamp, false).is_err());
    assert_eq!(document.snapshot(None), before);
    let p = proposal(&document, vec![good.clone()]);
    assert!(p.apply(&mut document, p.stamp, true).is_err());
    assert!(p.apply(&mut document, ProposalStamp { generation: 2, ..p.stamp }, false).is_err());
    assert!(proposal(&document, vec![good; 129]).batch().is_err());
    assert!(proposal(&document, vec![Command::Batch { label: None, commands: vec![] }]).batch().is_err());
    assert!(proposal(&document, vec![Command::AddPattern { name: Some("x".repeat(524_288)) }]).batch().is_err());
    assert!(proposal(&document, vec![Command::UpdateSettings { patch: SettingsPatch { name: Some("Not allowed".into()), ..Default::default() } }]).batch().is_err());
    assert_eq!(document.snapshot(None), before);
}

#[test]
fn successful_proposal_is_one_undo_step_and_cannot_replay_after_patch() {
    let mut document = Document::new(Project::new("Batch"));
    let original = document.project().clone();
    let p = proposal(&document, vec![Command::AddPattern { name: None }, Command::UpdateSettings { patch: SettingsPatch { tempo_bpm: Some(120.0), ..Default::default() } }]);
    let applied = p.apply(&mut document, p.stamp, false).unwrap();
    document.patch(&applied.touched);
    assert_eq!(document.history().entries.len(), 1);
    assert!(p.apply(&mut document, p.stamp, false).is_err());
    document.undo().unwrap();
    assert_eq!(document.project().patterns, original.patterns);
    assert_eq!(document.project().settings, original.settings);
    document.redo().unwrap();
    assert_eq!(document.project().settings.tempo_bpm, 120.0);
}

#[test]
fn script_revisions_save_reopen_undo_and_reject_unknown_language() {
    let mut document = Document::new(Project::new("Scripts"));
    let asset = ScriptAsset { reference: ScriptReference { id: "transpose".into(), revision: 1 }, language_version: 1, kind: ScriptKind::NoteTransform, name: "Transpose".into(), source: "set key key 12 + 0 127 clamp\nemit".into() };
    let library = ScriptLibrary { assets: vec![asset.clone()], ..Default::default() };
    document.dispatch(Command::SetScriptLibrary { library: library.clone() }, None).unwrap();
    let loaded = crate::file::from_json(&crate::file::to_json(document.project()).unwrap()).unwrap();
    assert_eq!(loaded.settings.scripts, library);
    let mut changed = library.clone(); changed.assets[0].source = "emit".into();
    assert!(document.dispatch(Command::SetScriptLibrary { library: changed }, None).is_err());
    document.undo().unwrap();
    assert!(document.project().settings.scripts.is_empty());
    document.redo().unwrap();
    let mut future = library; future.assets[0].language_version = 99;
    assert!(future.check().is_err());
    let legacy = crate::file::to_json(&Project::new("Legacy")).unwrap();
    assert!(!legacy.contains("scripts"));
    assert!(crate::file::from_json(&legacy).unwrap().settings.scripts.is_empty());
}
