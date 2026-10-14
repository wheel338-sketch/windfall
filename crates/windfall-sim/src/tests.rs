use serde_json::{Value, json};
use windfall_project::{
    Command, CommandError, Document, FORMAT_VERSION, LoadError, PatternId, Project, file,
};

use super::*;

/// Runs an operation and gives back its value, or its error message.
fn run(op: Op, handle: u32, input: &Value) -> Result<Value, String> {
    let reply: Value = serde_json::from_str(&call(op, handle, &input.to_string())).unwrap();
    match (reply.get("ok"), reply.get("error")) {
        (Some(value), None) => Ok(value.clone()),
        (None, Some(Value::String(message))) => Err(message.clone()),
        _ => panic!("{reply} is neither a value nor an error"),
    }
}

fn new_document() -> u32 {
    let project = run(ops::project_new, 0, &json!("Test")).unwrap();
    handle_of(run(ops::doc_new, 0, &project).unwrap())
}

fn handle_of(value: Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn dispatch(handle: u32, command: Value) -> Result<Value, String> {
    run(ops::doc_dispatch, handle, &json!({ "command": command }))
}

fn project_of(handle: u32) -> Project {
    let snapshot = run(ops::doc_snapshot, handle, &Value::Null).unwrap();
    serde_json::from_value(snapshot["project"].clone()).unwrap()
}

#[test]
fn library_query_and_tags_use_the_shared_grammar_without_a_document() {
    let input = json!({
        "query": "(DRUMS\\K?CKS AND *.WAV) OR \"vocal chop\"",
        "paths": ["/factory/Drums/Kicks/Kick 01.wav", "/samples/Vocal Chop.flac", "/samples/Snare.wav"]
    });
    assert_eq!(run(ops::browser_query, 0, &input).unwrap(), json!([0, 1]));
    assert!(
        run(
            ops::browser_query,
            0,
            &json!({ "query": "kick OR", "paths": [] })
        )
        .unwrap_err()
        .contains("term")
    );
    assert!(
        run(
            ops::browser_query,
            0,
            &json!({ "query": "", "paths": ["x".repeat(4097)] })
        )
        .is_err()
    );
    assert_eq!(
        run(ops::browser_tags, 0, &json!([" Warm ", "WARM", "drums"])).unwrap(),
        json!(["drums", "warm"])
    );
    assert!(run(ops::browser_tags, 0, &json!(["x".repeat(33)])).is_err());
}

#[test]
fn a_new_project_is_the_models_own() {
    let project = run(ops::project_new, 0, &json!("Song")).unwrap();
    assert_eq!(
        serde_json::from_value::<Project>(project).unwrap(),
        Project::new("Song")
    );
}

#[test]
fn dispatch_undo_and_redo_answer_as_the_shell_does() {
    let doc = new_document();
    let added = dispatch(doc, json!({ "type": "addChannel", "name": "Kick" })).unwrap();
    // The channel, then the mixer track made for it.
    assert_eq!(added["created"], json!([2, 3]));
    let patch = &added["patch"];
    assert_eq!(patch["revision"], 1);
    assert_eq!(patch["dirty"], true);
    assert_eq!(patch["channels"][0]["name"], "Kick");
    assert_eq!(patch["mixer"]["tracks"][1]["name"], "Kick");
    assert_eq!(
        patch["history"]["entries"],
        json!([{ "label": "Add channel" }])
    );
    assert!(patch.get("settings").is_none());

    let undone = run(ops::doc_undo, doc, &Value::Null).unwrap();
    assert_eq!(undone["revision"], 2);
    assert_eq!(undone["channels"], json!([]));
    assert_eq!(undone["dirty"], false);
    assert_eq!(run(ops::doc_undo, doc, &Value::Null), Ok(Value::Null));

    let redone = run(ops::doc_redo, doc, &Value::Null).unwrap();
    assert_eq!(redone["channels"][0]["id"], 2);
    assert_eq!(run(ops::doc_redo, doc, &Value::Null), Ok(Value::Null));

    let jumped = run(ops::doc_jump, doc, &json!(0)).unwrap();
    assert_eq!(jumped["history"]["cursor"], 0);
    // A cursor past the end means the end.
    let jumped = run(ops::doc_jump, doc, &json!(99)).unwrap();
    assert_eq!(jumped["history"]["cursor"], 1);

    let snapshot = run(ops::doc_snapshot, doc, &json!("/songs/a.windfall")).unwrap();
    assert_eq!(snapshot["revision"], 5);
    assert_eq!(snapshot["path"], "/songs/a.windfall");
    assert_eq!(snapshot["project"]["channels"][0]["name"], "Kick");
    run(ops::doc_free, doc, &Value::Null).unwrap();
}

#[test]
fn a_gesture_is_one_undo_step() {
    let doc = new_document();
    for tempo in [130, 140, 150] {
        let input = json!({
            "command": { "type": "updateSettings", "patch": { "tempoBpm": tempo } },
            // Larger than a u32, as the UI's gesture ids are.
            "gesture": 1_125_899_906_842_625_u64,
        });
        run(ops::doc_dispatch, doc, &input).unwrap();
    }
    assert_eq!(project_of(doc).settings.tempo_bpm, 150.0);
    run(ops::doc_undo, doc, &Value::Null).unwrap();
    assert_eq!(project_of(doc).settings.tempo_bpm, 120.0);
    run(ops::doc_free, doc, &Value::Null).unwrap();
}

#[test]
fn errors_carry_the_documents_own_words() {
    let doc = new_document();
    let missing = dispatch(doc, json!({ "type": "removeChannel", "id": 77 }));
    assert_eq!(
        missing,
        Err(CommandError::not_found("channel", 77_u32).to_string())
    );

    let last = dispatch(doc, json!({ "type": "removePattern", "id": 1 })).unwrap_err();
    let mut document = Document::new(Project::new("Test"));
    let command = Command::RemovePattern { id: PatternId(1) };
    assert_eq!(
        last,
        document.dispatch(command, None).unwrap_err().to_string()
    );

    // A failed command leaves no patch behind: the next one is revision 1.
    let added = dispatch(doc, json!({ "type": "addPattern" })).unwrap();
    assert_eq!(added["patch"]["revision"], 1);
    run(ops::doc_free, doc, &Value::Null).unwrap();
}

#[test]
fn input_that_is_not_a_command_is_refused_and_changes_nothing() {
    let doc = new_document();
    let before = project_of(doc);
    for input in [
        json!({ "command": { "type": "noSuchCommand" } }),
        json!({ "command": { "type": "toggleStep", "pattern": 1, "channel": 2, "step": -1 } }),
        json!({ "gesture": 4 }),
    ] {
        let error = run(ops::doc_dispatch, doc, &input).unwrap_err();
        assert!(error.starts_with("invalid command: "), "{error}");
    }
    let broken = call(ops::doc_dispatch, doc, "{\"command\":");
    assert!(
        broken.starts_with("{\"error\":\"invalid command: "),
        "{broken}"
    );
    assert_eq!(project_of(doc), before);
    run(ops::doc_free, doc, &Value::Null).unwrap();
}

#[test]
fn a_project_that_breaks_a_rule_does_not_become_a_document() {
    let mut project = Project::new("Test");
    project.next_id = 1;
    let error = run(ops::doc_new, 0, &serde_json::to_value(&project).unwrap()).unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        format!(
            "the project breaks a rule of the project model: {}",
            windfall_core::english::english(&project.check().unwrap_err())
        )
    );
}

#[test]
fn a_closed_handle_is_an_error_and_is_never_handed_out_again() {
    let first = new_document();
    run(ops::doc_free, first, &Value::Null).unwrap();
    let message = format!("document handle {first} is not open");
    assert_eq!(
        said(run(ops::doc_free, first, &Value::Null)),
        Err(message.clone())
    );
    assert_eq!(
        said(run(ops::doc_undo, first, &Value::Null)),
        Err(message.clone())
    );
    assert_eq!(
        said(dispatch(first, json!({ "type": "addPattern" }))),
        Err(message)
    );
    assert_eq!(
        said(run(ops::doc_snapshot, 0, &Value::Null)),
        Err("document handle 0 is not open".to_owned())
    );

    let second = new_document();
    assert!(second > first);
    run(ops::doc_free, second, &Value::Null).unwrap();
}

#[test]
fn a_saved_file_is_the_real_format_and_opens_again() {
    let doc = new_document();
    dispatch(doc, json!({ "type": "addChannel", "name": "Kick" })).unwrap();
    dispatch(
        doc,
        json!({ "type": "toggleStep", "pattern": 1, "channel": 2, "step": 4 }),
    )
    .unwrap();
    let project = project_of(doc);

    let text = run(ops::doc_to_file_json, doc, &Value::Null).unwrap();
    let text = text.as_str().unwrap();
    assert_eq!(text, file::to_json(&project).unwrap());

    let saved = run(ops::doc_mark_saved, doc, &Value::Null).unwrap();
    assert_eq!(saved["dirty"], false);
    assert_eq!(saved["revision"], 3);
    assert!(saved.get("channels").is_none());

    let opened = handle_of(run(ops::doc_from_file_json, 0, &json!(text)).unwrap());
    assert_eq!(project_of(opened), project);
    let snapshot = run(ops::doc_snapshot, opened, &Value::Null).unwrap();
    assert_eq!(snapshot["dirty"], false);
    assert_eq!(snapshot["history"]["entries"], json!([]));
    for handle in [doc, opened] {
        run(ops::doc_free, handle, &Value::Null).unwrap();
    }
}

#[test]
fn a_file_that_cannot_be_loaded_says_why_in_the_loaders_words() {
    let open = |text: &str| run(ops::doc_from_file_json, 0, &json!(text)).unwrap_err();

    let garbage = open("not a project");
    assert_eq!(
        garbage,
        file::from_json("not a project").unwrap_err().to_string()
    );
    assert!(
        windfall_core::english::english(&garbage).starts_with("this is not a Windfall project: ")
    );

    let mut newer = serde_json::to_value(Project::new("Later")).unwrap();
    newer["formatVersion"] = json!(FORMAT_VERSION + 1);
    let too_new = LoadError::TooNew {
        found: FORMAT_VERSION + 1,
        supported: FORMAT_VERSION,
    };
    assert_eq!(open(&newer.to_string()), too_new.to_string());

    let mut damaged = Project::new("Damaged");
    damaged.patterns.clear();
    let damaged = serde_json::to_string(&damaged).unwrap();
    assert_eq!(
        open(&damaged),
        file::from_json(&damaged).unwrap_err().to_string()
    );
    assert!(
        windfall_core::english::english(&open(&damaged))
            .starts_with("the project file is damaged: ")
    );
}

/// The shell adds a channel for an audio file with one batch, in which the
/// channel names a sample by the id the project hands out next.
#[test]
fn the_next_id_names_a_sample_the_same_batch_adds() {
    let doc = new_document();
    let sample = run(ops::doc_next_id, doc, &Value::Null).unwrap();
    assert_eq!(sample, 2);
    let batch = json!({
        "type": "batch",
        "label": "Add channel",
        "commands": [
            { "type": "addSample", "name": "Kick", "path": { "kind": "external", "path": "/kick.wav" } },
            { "type": "addChannel", "name": "Kick", "sample": sample },
        ],
    });
    let added = dispatch(doc, batch).unwrap();
    assert_eq!(added["created"], json!([2, 3, 4]));
    assert_eq!(added["patch"]["channels"][0]["source"]["sample"], 2);
    assert_eq!(run(ops::doc_next_id, doc, &Value::Null).unwrap(), 5);
    run(ops::doc_free, doc, &Value::Null).unwrap();
}

#[test]
fn a_panic_is_an_error_that_closes_only_the_document_in_use() {
    let crashed = new_document();
    let bystander = new_document();
    dispatch(bystander, json!({ "type": "addPattern" })).unwrap();

    let error = windfall_core::english::english(
        &run(ops::sim_panic, crashed, &json!("on purpose")).unwrap_err(),
    );
    assert!(
        error.starts_with("the document engine crashed: "),
        "{error}"
    );
    assert!(error.contains("on purpose"), "{error}");
    assert_eq!(
        said(run(ops::doc_undo, crashed, &Value::Null)),
        Err(format!("document handle {crashed} is not open"))
    );

    assert_eq!(project_of(bystander).patterns.len(), 2);
    run(ops::doc_undo, bystander, &Value::Null).unwrap();
    assert_eq!(project_of(bystander).patterns.len(), 1);
    run(ops::doc_free, bystander, &Value::Null).unwrap();
}

/// A result with its refusal said in English.
fn said<T>(result: Result<T, String>) -> Result<T, String> {
    result.map_err(|error| windfall_core::english::english(&error))
}

/// Calls an exported function the way the JavaScript glue does.
fn through_pointers(
    export: unsafe extern "C" fn(u32, *const u8, usize) -> *mut u8,
    handle: u32,
    input: &[u8],
) -> String {
    let buffer = sim_alloc(input.len());
    // SAFETY: `buffer` is `input.len()` bytes from `sim_alloc`, the result
    // is a length followed by that many bytes, and each buffer is freed once
    // with the length it was made with.
    unsafe {
        buffer.copy_from_nonoverlapping(input.as_ptr(), input.len());
        let result = export(handle, buffer, input.len());
        sim_dealloc(buffer, input.len());

        let mut header = [0_u8; 4];
        result.copy_to_nonoverlapping(header.as_mut_ptr(), 4);
        let len = u32::from_le_bytes(header) as usize;
        let text = std::slice::from_raw_parts(result.add(4), len).to_vec();
        sim_dealloc(result, 4 + len);
        String::from_utf8(text).unwrap()
    }
}

#[test]
fn the_exported_functions_pass_json_through_buffers() {
    let project = through_pointers(project_new, 0, b"\"Pointers\"");
    let project: Value = serde_json::from_str(&project).unwrap();
    assert_eq!(project["ok"]["settings"]["name"], "Pointers");

    let opened = through_pointers(doc_new, 0, project["ok"].to_string().as_bytes());
    let opened: Value = serde_json::from_str(&opened).unwrap();
    let doc = handle_of(opened["ok"].clone());

    // An operation without input takes an empty buffer.
    assert_eq!(through_pointers(doc_next_id, doc, b""), "{\"ok\":2}");
    assert_eq!(through_pointers(doc_undo, doc, b""), "{\"ok\":null}");
    assert_eq!(
        through_pointers(doc_dispatch, doc, &[0xFF, 0xFE]),
        "{\"error\":\"the input is not UTF-8\"}"
    );
    assert_eq!(through_pointers(doc_free, doc, b""), "{\"ok\":null}");
}
