//! Scores as Standard MIDI Files and back, and MIDI files at a project's
//! timebase.

use windfall_midi::{
    ControlCurve, ControlKind, ControlPoint, ExportOptions, MidiNote, MidiSong, MidiTrack,
    Resolution, TempoChange, TimeSignatureChange, WriteOptions, export_pattern, read,
    score_from_song, score_to_song, write,
};
use windfall_project::score::MAX_SCORE_NOTES;
use windfall_project::timebase::playback_project;
use windfall_project::*;

fn synth(doc: &mut Document) -> ChannelId {
    ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: Some("Lead".into()),
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

fn note(start: u32, length: u32, key: u8, velocity: f32) -> NoteInit {
    NoteInit {
        start,
        length,
        key,
        velocity: Some(velocity),
        pan: None,
        expression: None,
    }
}

/// A score of three plain notes taken from a lane.
fn plain() -> Score {
    let mut doc = Document::new(Project::new("Scores"));
    let pattern = doc.project().patterns[0].id;
    let channel = synth(&mut doc);
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![
                note(0, 480, 60, 1.0),
                note(960, 240, 64, 64.0 / 127.0),
                note(3_000, 840, 67, 1.0 / 127.0),
            ],
        },
        None,
    )
    .unwrap();
    Score::capture(doc.project(), pattern, channel, None).unwrap()
}

#[test]
fn a_plain_score_survives_a_midi_file_note_for_note() {
    let score = plain();
    let (song, left_out) = score_to_song(&score, 128.0);
    assert_eq!(left_out, Vec::<String>::new());
    assert_eq!(song.tracks.len(), 1);
    assert_eq!(song.tracks[0].name.as_deref(), Some(score.name.as_str()));
    assert_eq!(song.length, score.length_ticks);
    assert_eq!(song.tempos, [TempoChange::from_bpm(0, 128.0)]);
    assert_eq!(
        song.tracks[0]
            .notes
            .iter()
            .map(|note| (note.start, note.length, note.key, note.velocity))
            .collect::<Vec<_>>(),
        [(0, 480, 60, 127), (960, 240, 64, 64), (3_000, 840, 67, 1)]
    );

    // Through the bytes of a real file, in both layouts.
    for options in [
        WriteOptions::default(),
        WriteOptions {
            format: windfall_midi::SmfFormat::Single,
            running_status: false,
            ..WriteOptions::default()
        },
    ] {
        let file = write(&song, &options);
        assert_eq!(&file[..4], b"MThd");
        let (back, left_out) = score_from_song(&read(&file).unwrap(), "fallback").unwrap();
        assert_eq!(left_out, Vec::<String>::new(), "{options:?}");
        assert_eq!(back.notes, score.notes, "{options:?}");
        assert_eq!(back.length_ticks, score.length_ticks);
        assert_eq!(back.name, score.name);
        assert!(back.curves.is_empty());
        back.validate().unwrap();
    }
}

#[test]
fn what_a_midi_file_cannot_hold_is_reported_and_the_notes_still_travel() {
    let mut score = plain();
    score.notes[0].pan = -0.5;
    score.notes[1].expression = NoteExpression {
        release: 0.25,
        ..NoteExpression::default()
    };
    score.notes[2].expression = NoteExpression {
        modulation_x: 0.9,
        ..NoteExpression::default()
    };
    score.curves.push(NoteCurveInsert {
        note_index: 0,
        parameter: NoteCurveParameter::Pan,
        points: vec![NoteCurvePoint {
            position: 0.0,
            value: 1.0,
            curve: 0.0,
            hold: false,
        }],
    });
    score.time_signature = Some(TimeSignature {
        numerator: 7,
        denominator: 8,
    });
    score.validate().unwrap();

    let (song, left_out) = score_to_song(&score, 120.0);
    assert_eq!(left_out.len(), 1, "{left_out:?}");
    assert!(left_out[0].contains("Windfall score metadata"));
    assert_eq!(
        song.time_signatures,
        [TimeSignatureChange {
            tick: 0,
            numerator: 7,
            denominator: 8
        }]
    );
    // The release is the one thing of a note's expression a file holds.
    assert_eq!(song.tracks[0].notes[1].release, 32);

    let (back, _) =
        score_from_song(&read(&write(&song, &WriteOptions::default())).unwrap(), "x").unwrap();
    assert_eq!(back.time_signature, score.time_signature);
    assert_eq!(back.notes[1].expression.release, 0.25);
    assert_eq!(back, score);
    assert_eq!(
        back.notes
            .iter()
            .map(|n| (n.start, n.length, n.key))
            .collect::<Vec<_>>(),
        score
            .notes
            .iter()
            .map(|n| (n.start, n.length, n.key))
            .collect::<Vec<_>>()
    );
}

fn midi_note(start: u32, length: u32, key: u8, channel: u8) -> MidiNote {
    MidiNote {
        start,
        length,
        key,
        velocity: 100,
        release: 64,
        channel,
    }
}

#[test]
fn any_midi_file_loads_as_one_lane_with_a_report_of_the_rest() {
    let song = MidiSong {
        name: Some("Two hands".into()),
        tracks: vec![
            MidiTrack {
                name: Some("Right".into()),
                notes: vec![midi_note(960, 480, 72, 0), midi_note(0, 480, 76, 0)],
                controls: vec![ControlCurve {
                    channel: 0,
                    kind: ControlKind::Controller(1),
                    points: vec![ControlPoint { tick: 0, value: 64 }],
                }],
                ..MidiTrack::default()
            },
            MidiTrack {
                name: Some("Left".into()),
                notes: vec![midi_note(0, 1_920, 48, 1)],
                ..MidiTrack::default()
            },
            MidiTrack::default(),
        ],
        tempos: vec![
            TempoChange::from_bpm(0, 90.0),
            TempoChange::from_bpm(960, 140.0),
        ],
        time_signatures: vec![TimeSignatureChange {
            tick: 0,
            numerator: 5,
            denominator: 4,
        }],
        length: 4_800,
        ..MidiSong::default()
    }
    .normalized();
    let (score, left_out) = score_from_song(&song, "file name").unwrap();
    assert_eq!(score.name, "Two hands");
    assert_eq!(score.length_ticks, 4_800);
    assert_eq!(
        score.time_signature,
        Some(TimeSignature {
            numerator: 5,
            denominator: 4
        })
    );
    assert_eq!(
        score
            .notes
            .iter()
            .map(|n| (n.start, n.key))
            .collect::<Vec<_>>(),
        [(0, 48), (0, 76), (960, 72)]
    );
    assert!(score.notes.iter().all(|n| n.velocity == 100.0 / 127.0));
    assert!(
        score
            .notes
            .iter()
            .all(|n| n.expression == NoteExpression::default())
    );
    assert_eq!(left_out.len(), 3, "{left_out:?}");
    assert!(left_out[0].contains("2 tracks"));
    assert!(left_out[1].contains("Controllers"));
    assert!(left_out[2].contains("tempo"));

    // It loads into a lane like any score.
    let mut doc = Document::new(Project::new("Loaded"));
    let pattern = doc.project().patterns[0].id;
    let channel = synth(&mut doc);
    let command = score
        .load_command(doc.project(), pattern, channel, ScoreLoadMode::Replace)
        .unwrap();
    doc.dispatch(command, None).unwrap();
    let lane = doc
        .project()
        .pattern(pattern)
        .unwrap()
        .lane(channel)
        .unwrap();
    assert_eq!(lane.notes.len(), 3);
    assert_eq!(doc.project().pattern(pattern).unwrap().length_steps, 20);
    doc.project().check().unwrap();

    // A file with no name is named after its first named track, then by
    // the caller.
    let mut unnamed = song.clone();
    unnamed.name = None;
    assert_eq!(
        score_from_song(&unnamed, "file name").unwrap().0.name,
        "Right"
    );
    for track in &mut unnamed.tracks {
        track.name = None;
    }
    assert_eq!(
        score_from_song(&unnamed, "file name").unwrap().0.name,
        "file name"
    );
}

#[test]
fn files_a_score_cannot_hold_are_refused_or_cut_with_a_reason() {
    assert!(
        score_from_song(&MidiSong::default(), "empty")
            .unwrap_err()
            .contains("no notes")
    );
    let crowded = MidiSong {
        tracks: vec![MidiTrack {
            notes: (0..=MAX_SCORE_NOTES as u32)
                .map(|index| midi_note(index, 1, 60, 0))
                .collect(),
            ..MidiTrack::default()
        }],
        ..MidiSong::default()
    };
    assert!(
        score_from_song(&crowded, "crowded")
            .unwrap_err()
            .contains("Import it as MIDI")
    );

    let long = MidiSong {
        tracks: vec![MidiTrack {
            notes: vec![
                midi_note(0, 480, 60, 0),
                midi_note(MAX_PATTERN_TICKS - 100, 480, 62, 0),
                midi_note(MAX_PATTERN_TICKS, 480, 64, 0),
            ],
            ..MidiTrack::default()
        }],
        ..MidiSong::default()
    }
    .normalized();
    let (score, left_out) = score_from_song(&long, "long").unwrap();
    assert_eq!(score.notes.len(), 2);
    assert_eq!(score.notes[1].length, 100);
    assert_eq!(score.length_ticks, MAX_PATTERN_TICKS);
    assert!(
        left_out
            .iter()
            .any(|line| line.contains("1 notes start past"))
    );
    assert!(
        left_out
            .iter()
            .any(|line| line.contains("1 notes were cut"))
    );
    score.validate().unwrap();
}

#[test]
fn a_midi_file_at_the_project_timebase_holds_what_plays_without_rounding() {
    let mut doc = Document::new(Project::new("Timebase"));
    let pattern = doc.project().patterns[0].id;
    let channel = synth(&mut doc);
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![
                note(3, 7, 60, 1.0),
                note(244, 233, 62, 1.0),
                note(485, 1, 64, 1.0),
            ],
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::UpdateSettings {
            patch: SettingsPatch {
                timebase_ppq: Some(96),
                ..Default::default()
            },
        },
        None,
    )
    .unwrap();
    let options = ExportOptions {
        swing: false,
        ..ExportOptions::default()
    };
    let played = playback_project(doc.project());
    let song = export_pattern(&played, pattern, &options).unwrap();
    let file = write(
        &song,
        &WriteOptions {
            resolution: Resolution::Ppq(96),
            ..WriteOptions::default()
        },
    );
    // The header's division is the timebase.
    assert_eq!(&file[12..14], &96_u16.to_be_bytes());
    let back = read(&file).unwrap();
    let notes: Vec<_> = back
        .tracks
        .iter()
        .flat_map(|track| &track.notes)
        .map(|note| (note.start, note.length, note.key))
        .collect();
    // Exactly the grid positions playback uses, back at 960 to the quarter.
    assert_eq!(notes, [(0, 10, 60), (240, 240, 62), (490, 10, 64)]);
    // And nothing was rounded on the way: the file is the song.
    assert_eq!(back, song.clone().normalized());

    // The stored notes are where they were drawn, and at the default
    // timebase the file has them there.
    let drawn = export_pattern(doc.project(), pattern, &options).unwrap();
    assert_eq!(
        drawn.tracks[0]
            .notes
            .iter()
            .map(|note| (note.start, note.length))
            .collect::<Vec<_>>(),
        [(3, 7), (244, 233), (485, 1)]
    );
    assert_eq!(Resolution::Ppq(96).ticks_per_quarter(), 96);
    assert_eq!(Resolution::Ppq(0).ticks_per_quarter(), 1);
    assert_eq!(Resolution::Ppq(u16::MAX).ticks_per_quarter(), 0x7FFF);
}

#[test]
fn expressive_scores_and_pattern_event_lanes_survive_actual_smf_bytes() {
    let mut score = plain();
    score.notes[0].expression.color_group = Some(3);
    score.notes[0].expression.articulation = NoteArticulation::Slide;
    score.events.push(ScoreEventLane {
        target: AutomationTarget::ChannelPan {
            channel: ChannelId(0),
        },
        points: vec![AutomationPoint {
            tick: 0,
            value: 0.3,
            curve: 0.2,
            hold: true,
        }],
    });
    for options in [
        WriteOptions::default(),
        WriteOptions {
            format: windfall_midi::SmfFormat::Single,
            ..Default::default()
        },
    ] {
        let (song, _) = score_to_song(&score, 123.0);
        let bytes = write(&song, &options);
        let (back, warnings) = score_from_song(&read(&bytes).unwrap(), "fallback").unwrap();
        assert_eq!(back, score);
        assert!(warnings.is_empty());
    }
    let (mut altered, _) = score_to_song(&score, 120.0);
    altered.tracks[0].notes[0].key += 1;
    assert!(
        score_from_song(&altered, "changed")
            .unwrap_err()
            .contains("changed")
    );
}
