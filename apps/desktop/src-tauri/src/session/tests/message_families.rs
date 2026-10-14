//! One error of each family, made through the call that makes it, comes out
//! as the code and values the interface translates, not as English: a
//! project file that breaks a rule, a refused edit, an FL Studio import
//! review line and a file that is not one, an audio file that will not
//! decode, and a refusal of the desktop itself.

use windfall_core::notice::{Arg, Decoded, decode};
use windfall_project::{Command, Document, Project, SettingsPatch};

use super::Rig;

fn decoded(text: &str) -> Decoded {
    decode(text).unwrap_or_else(|| panic!("not a coded message: {text}"))
}

fn number(message: &Decoded, name: &str) -> f64 {
    match message.param(name) {
        Some(Arg::Int(value)) => *value as f64,
        Some(Arg::UInt(value)) => *value as f64,
        Some(Arg::Float(value)) => *value,
        other => panic!("{}: {name} is {other:?}, not a number", message.code),
    }
}

/// The word a coded message carries as a term, for the catalog to translate.
fn term(message: &Decoded, name: &str) -> String {
    let value = message
        .text(name)
        .unwrap_or_else(|| panic!("{}: no {name}", message.code));
    let term = decoded(&value);
    assert_eq!(term.code, "text.term", "{}: {name}", message.code);
    term.text("text").unwrap()
}

/// An FL Studio project file: the version text, then the tempo in
/// thousandths of a beat per minute.
fn flp_with_tempo(millibpm: u32) -> Vec<u8> {
    let mut events = vec![199, 12];
    events.extend(b"20.8.4.2576\0");
    events.push(156);
    events.extend(millibpm.to_le_bytes());
    let mut bytes = b"FLhd".to_vec();
    bytes.extend(6_u32.to_le_bytes());
    bytes.extend(0_i16.to_le_bytes());
    bytes.extend(0_u16.to_le_bytes());
    bytes.extend(96_u16.to_le_bytes());
    bytes.extend(b"FLdt");
    bytes.extend((events.len() as u32).to_le_bytes());
    bytes.extend(events);
    bytes
}

#[test]
fn a_project_file_that_breaks_a_rule_names_the_rule_and_its_values() {
    let mut project = Project::new("Rules");
    project.settings.tempo_bpm = 600.0;
    let json = windfall_project::file::to_json(&project).unwrap();
    let error = windfall_project::file::from_json(&json)
        .unwrap_err()
        .to_string();
    let damaged = decoded(&error);
    assert_eq!(damaged.code, "project.load.damaged");
    let rule = decoded(&damaged.text("problem").unwrap());
    assert_eq!(rule.code, "project.check.tempo");
    assert_eq!(number(&rule, "tempo"), 600.0);
    assert_eq!(number(&rule, "min"), windfall_project::MIN_TEMPO_BPM);
    assert_eq!(number(&rule, "max"), windfall_project::MAX_TEMPO_BPM);
}

#[test]
fn a_refused_edit_names_the_field_as_a_term_and_its_limit() {
    let mut document = Document::new(Project::new("Edits"));
    let error = document
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    genre: Some("g".repeat(129)),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap_err()
        .to_string();
    let refusal = decoded(&error);
    assert_eq!(refusal.code, "project.edit.fieldTooLong");
    assert_eq!(term(&refusal, "field"), "genre");
    assert_eq!(number(&refusal, "max"), 128.0);
}

#[test]
fn an_fl_import_review_line_and_an_unreadable_file_are_coded() {
    let conversion = windfall_flp::import(
        &flp_with_tempo(600_000),
        &windfall_flp::ConvertOptions::default(),
    )
    .unwrap();
    let lines: Vec<Decoded> = conversion
        .report
        .categories
        .iter()
        .flat_map(|category| &category.lines)
        .map(|line| decoded(&line.text))
        .collect();
    let tempo = lines
        .iter()
        .find(|line| line.code == "flp.project.tempo")
        .unwrap_or_else(|| panic!("no tempo line in {lines:#?}"));
    assert_eq!(number(tempo, "bpm"), 600.0);
    assert_eq!(number(tempo, "max"), windfall_project::MAX_TEMPO_BPM);

    let error = windfall_flp::import(b"FLhd", &windfall_flp::ConvertOptions::default())
        .unwrap_err()
        .to_string();
    let short = decoded(&error);
    assert_eq!(short.code, "flp.error.tooShort");
    assert_eq!(number(&short, "size"), 4.0);
}

#[test]
fn an_audio_file_that_will_not_decode_is_coded() {
    let empty = windfall_codec::decode_bytes(&[], Some("wav"))
        .unwrap_err()
        .to_string();
    assert_eq!(decoded(&empty).code, "codec.noAudio");
    let corrupt = windfall_codec::CodecError::Corrupt("missing stream rate".into()).to_string();
    let corrupt = decoded(&corrupt);
    assert_eq!(corrupt.code, "codec.corrupt");
    assert_eq!(term(&corrupt, "reason"), "missing stream rate");
}

#[test]
fn a_refusal_of_the_desktop_gives_its_code_and_the_name() {
    let rig = Rig::new();
    let factory = rig
        .session
        .templates()
        .unwrap()
        .into_iter()
        .find(|template| template.factory)
        .unwrap();
    let error = rig.session.template_save(&factory.name).unwrap_err();
    let refusal = decoded(&error);
    assert_eq!(refusal.code, "templates.factoryName");
    assert_eq!(refusal.text("name").unwrap(), factory.name);
}
