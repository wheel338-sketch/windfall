//! Score files: taking a lane's notes and curves, the file, and loading it
//! into another lane as one undo step.

use serde_json::{Value, json};
use windfall_project::score::{MAX_SCORE_NOTES, SCORE_SCHEMA, SCORE_VERSION};
use windfall_project::*;

fn synth(doc: &mut Document, name: &str) -> ChannelId {
    ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: Some(name.into()),
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    )
}

fn note(start: u32, length: u32, key: u8, velocity: f32, pan: f32) -> NoteInit {
    NoteInit {
        start,
        length,
        key,
        velocity: Some(velocity),
        pan: Some(pan),
        expression: Some(NoteExpression {
            release: 0.75,
            ..NoteExpression::default()
        }),
    }
}

fn curve(note_index: u32, parameter: NoteCurveParameter, values: [f32; 2]) -> NoteCurveInsert {
    NoteCurveInsert {
        note_index,
        parameter,
        points: vec![
            NoteCurvePoint {
                position: 0.0,
                value: values[0],
                curve: 0.25,
                hold: false,
            },
            NoteCurvePoint {
                position: 1.0,
                value: values[1],
                curve: 0.0,
                hold: false,
            },
        ],
    }
}

/// A two-bar pattern whose lead lane has three notes, two of them with
/// curves, and a second lane that must never be touched.
fn written() -> (Document, PatternId, ChannelId, ChannelId) {
    let mut doc = Document::new(Project::new("Scores"));
    let pattern = doc.project().patterns[0].id;
    doc.dispatch(
        Command::UpdatePattern {
            id: pattern,
            patch: PatternPatch {
                name: Some("Verse".into()),
                length_steps: Some(32),
                ..PatternPatch::default()
            },
        },
        None,
    )
    .unwrap();
    let lead = synth(&mut doc, "Lead");
    let bass = synth(&mut doc, "Bass");
    doc.dispatch(
        Command::AddNotesWithCurves {
            pattern,
            channel: lead,
            notes: vec![
                note(0, 480, 60, 0.9, -0.5),
                note(960, 240, 64, 0.5, 0.0),
                note(7200, 480, 67, 0.25, 0.75),
            ],
            curves: vec![
                curve(0, NoteCurveParameter::Pan, [-1.0, 1.0]),
                curve(0, NoteCurveParameter::FinePitchCents, [0.0, 200.0]),
                curve(2, NoteCurveParameter::ModulationX, [0.1, 0.9]),
            ],
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel: bass,
            notes: vec![note(0, 960, 36, 0.8, 0.0)],
        },
        None,
    )
    .unwrap();
    (doc, pattern, lead, bass)
}

/// What a lane holds, without the ids: every note with its curves.
fn contents(project: &Project, pattern: PatternId, channel: ChannelId) -> Vec<String> {
    let pattern = project.pattern(pattern).unwrap();
    let Some(lane) = pattern.lane(channel) else {
        return Vec::new();
    };
    lane.notes
        .iter()
        .map(|note| {
            let mut curves: Vec<_> = pattern
                .note_curves
                .iter()
                .filter(|curve| curve.note == note.id)
                .map(|curve| format!("{:?}{:?}", curve.parameter, curve.points))
                .collect();
            curves.sort();
            format!(
                "{} {} {} {} {} {:?} {curves:?}",
                note.start, note.length, note.key, note.velocity, note.pan, note.expression
            )
        })
        .collect()
}

#[test]
fn a_score_holds_a_lanes_notes_and_curves_and_nothing_of_the_project() {
    let (doc, pattern, lead, _) = written();
    let score = Score::capture(doc.project(), pattern, lead, None).unwrap();
    assert_eq!(score.schema, SCORE_SCHEMA);
    assert_eq!(score.version, SCORE_VERSION);
    assert_eq!(score.ppq, 960);
    assert_eq!(score.name, "Verse - Lead");
    assert_eq!(score.length_ticks, 32 * 240);
    assert_eq!(score.notes.len(), 3);
    assert_eq!(
        score.notes[0],
        ScoreNote {
            start: 0,
            length: 480,
            key: 60,
            velocity: 0.9,
            pan: -0.5,
            expression: NoteExpression {
                release: 0.75,
                ..NoteExpression::default()
            },
        }
    );
    assert_eq!(score.curves.len(), 3);
    assert_eq!(
        score
            .curves
            .iter()
            .map(|curve| (curve.note_index, curve.parameter))
            .collect::<Vec<_>>(),
        [
            (0, NoteCurveParameter::Pan),
            (0, NoteCurveParameter::FinePitchCents),
            (2, NoteCurveParameter::ModulationX),
        ]
    );

    let text = score.to_json().unwrap();
    assert_eq!(Score::from_json(text.as_bytes()).unwrap(), score);
    // The same score is always the same file.
    assert_eq!(score.to_json().unwrap(), text);
    let file: Value = serde_json::from_str(&text).unwrap();
    for key in ["id", "channel", "pattern"] {
        assert!(!text.contains(&format!("\"{key}\"")), "{key} in {file}");
    }

    // A selection keeps the positions its notes have and only their curves.
    let ids: Vec<NoteId> = doc
        .project()
        .pattern(pattern)
        .unwrap()
        .lane(lead)
        .unwrap()
        .notes[1..]
        .iter()
        .map(|note| note.id)
        .collect();
    let part = Score::capture(doc.project(), pattern, lead, Some(&ids)).unwrap();
    assert_eq!(
        part.notes.iter().map(|n| n.start).collect::<Vec<_>>(),
        [960, 7200]
    );
    assert_eq!(part.curves.len(), 1);
    assert_eq!(part.curves[0].note_index, 1);

    assert!(Score::capture(doc.project(), pattern, lead, Some(&[])).is_err());
    assert!(Score::capture(doc.project(), PatternId(9_999), lead, None).is_err());
    assert!(Score::capture(doc.project(), pattern, ChannelId(9_999), None).is_err());
}

#[test]
fn loading_replaces_or_merges_in_one_undo_step_with_new_note_ids() {
    let (mut doc, pattern, lead, bass) = written();
    let score = Score::from_json(
        Score::capture(doc.project(), pattern, lead, None)
            .unwrap()
            .to_json()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let source = contents(doc.project(), pattern, lead);

    // Into the other lane of a shorter pattern, in place of what it has.
    let short = PatternId(
        doc.dispatch(Command::AddPattern { name: None }, None)
            .unwrap()
            .created[0],
    );
    doc.dispatch(
        Command::AddNotes {
            pattern: short,
            channel: bass,
            notes: vec![note(240, 240, 40, 0.6, 0.0), note(480, 240, 41, 0.6, 0.0)],
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddNotes {
            pattern: short,
            channel: lead,
            notes: vec![note(0, 240, 72, 0.6, 0.0)],
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    let history = doc.history().entries.len();
    let old_ids: Vec<NoteId> = before
        .patterns
        .iter()
        .flat_map(|pattern| &pattern.lanes)
        .flat_map(|lane| &lane.notes)
        .map(|note| note.id)
        .collect();

    let command = score
        .load_command(doc.project(), short, bass, ScoreLoadMode::Replace)
        .unwrap();
    let applied = doc.dispatch(command, None).unwrap();
    assert_eq!(applied.label, "Load score");
    assert_eq!(doc.history().entries.len(), history + 1);
    assert_eq!(contents(doc.project(), short, bass), source);
    // The pattern grew to play all of the score; the other lane is as it was.
    assert_eq!(doc.project().pattern(short).unwrap().length_steps, 32);
    assert_eq!(
        contents(doc.project(), short, lead),
        contents(&before, short, lead)
    );
    assert_eq!(doc.project().pattern(pattern), before.pattern(pattern));
    let loaded = doc.project().pattern(short).unwrap().lane(bass).unwrap();
    assert!(loaded.notes.iter().all(|note| !old_ids.contains(&note.id)));
    assert_eq!(applied.created.len(), 3);
    doc.project().check().unwrap();
    let after = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&after).unwrap()).unwrap(),
        after
    );
    doc.undo().unwrap();
    let mut restored = before.clone();
    restored.next_id = after.next_id;
    assert_eq!(*doc.project(), restored);
    doc.redo().unwrap();
    assert_eq!(*doc.project(), after);

    // Merging adds to what is there and leaves a long enough pattern alone.
    let lane_before = contents(doc.project(), pattern, bass);
    let command = score
        .load_command(doc.project(), pattern, bass, ScoreLoadMode::Merge)
        .unwrap();
    let Command::Batch { commands, .. } = &command else {
        panic!("{command:?}")
    };
    assert_eq!(commands.len(), 1, "no resize and nothing cleared");
    doc.dispatch(command, None).unwrap();
    let mut expected = lane_before;
    expected.extend(source.clone());
    let mut merged = contents(doc.project(), pattern, bass);
    expected.sort();
    merged.sort();
    assert_eq!(merged, expected);
    doc.project().check().unwrap();

    // Replacing an empty lane clears nothing.
    let empty = synth(&mut doc, "Keys");
    let command = score
        .load_command(doc.project(), pattern, empty, ScoreLoadMode::Replace)
        .unwrap();
    let Command::Batch { commands, .. } = &command else {
        panic!("{command:?}")
    };
    assert_eq!(commands.len(), 1);
    doc.dispatch(command, None).unwrap();
    assert_eq!(contents(doc.project(), pattern, empty), source);

    assert!(
        score
            .load_command(doc.project(), PatternId(9_999), bass, ScoreLoadMode::Merge)
            .is_err()
    );
    assert!(
        score
            .load_command(
                doc.project(),
                pattern,
                ChannelId(9_999),
                ScoreLoadMode::Merge
            )
            .is_err()
    );
}

fn file_of(score: &Score) -> Value {
    serde_json::from_str(&score.to_json().unwrap()).unwrap()
}

fn read(value: &Value) -> Result<Score, String> {
    Score::from_json(serde_json::to_string(value).unwrap().as_bytes())
}

#[test]
fn files_that_are_not_readable_scores_are_refused_with_the_reason() {
    let (doc, pattern, lead, _) = written();
    let score = Score::capture(doc.project(), pattern, lead, None).unwrap();
    let file = file_of(&score);
    read(&file).unwrap();

    assert!(
        Score::from_json(b"{}")
            .unwrap_err()
            .contains("not a Windfall score")
    );
    assert!(Score::from_json(b"MThd").is_err());
    let mut other = file.clone();
    other["schema"] = json!("windfall.preset");
    assert!(read(&other).unwrap_err().contains("not a Windfall score"));

    let mut newer = file.clone();
    newer["version"] = json!(SCORE_VERSION + 1);
    newer["notes"] = json!("a shape this build does not know");
    let message = read(&newer).unwrap_err();
    assert!(message.contains("newer Windfall"), "{message}");
    let mut unversioned = file.clone();
    unversioned.as_object_mut().unwrap().remove("version");
    assert!(read(&unversioned).is_err());

    let mut halved = file.clone();
    halved["ppq"] = json!(480);
    assert!(read(&halved).unwrap_err().contains("960"));

    for (field, value) in [
        ("length", json!(0)),
        ("key", json!(128)),
        ("velocity", json!(1.5)),
        ("velocity", json!(-0.1)),
        ("pan", json!(2.0)),
        // Past the score's own length.
        ("start", json!(32 * 240)),
        ("length", json!(32 * 240 + 1)),
    ] {
        let mut invalid = file.clone();
        invalid["notes"][0][field] = value.clone();
        assert!(read(&invalid).is_err(), "{field} = {value}");
    }
    let mut long = file.clone();
    long["lengthTicks"] = json!(MAX_PATTERN_TICKS + 1);
    assert!(read(&long).is_err());

    // Curves must sit on a note of the score, once per parameter, in range.
    let mut orphan = file.clone();
    orphan["curves"][0]["noteIndex"] = json!(3);
    assert!(read(&orphan).is_err());
    let mut twice = file.clone();
    let first = twice["curves"][0].clone();
    twice["curves"].as_array_mut().unwrap().push(first);
    assert!(read(&twice).is_err());
    let mut out_of_range = file.clone();
    out_of_range["curves"][0]["points"][1]["value"] = json!(9.0);
    assert!(read(&out_of_range).is_err());
    let mut backwards = file.clone();
    backwards["curves"][0]["points"][1]["position"] = json!(0.0);
    assert!(read(&backwards).is_err());

    let mut crowded = Score::new("Crowded", MAX_PATTERN_TICKS);
    crowded.notes = vec![
        ScoreNote {
            start: 0,
            length: 1,
            key: 60,
            velocity: 0.5,
            pan: 0.0,
            expression: NoteExpression::default(),
        };
        MAX_SCORE_NOTES + 1
    ];
    assert!(crowded.validate().is_err());
    assert!(crowded.to_json().is_err());

    // An empty score is a valid file, and there is nothing in it to load.
    let empty = Score::new("Empty", 3840);
    assert_eq!(read(&file_of(&empty)).unwrap(), empty);
    assert!(
        empty
            .load_command(doc.project(), pattern, lead, ScoreLoadMode::Merge)
            .is_err()
    );
}
