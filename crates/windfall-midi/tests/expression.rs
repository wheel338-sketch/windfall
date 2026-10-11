//! Note expression in a MIDI file: what a project's fine pitch, slides,
//! portamento, modulation and colour groups become as pitch bend and
//! controllers, read back from the bytes that were written.

use windfall_midi::expression::{
    CC_DATA_ENTRY, CC_MODULATION, CC_RPN_COARSE, CC_RPN_FINE, CC_TIMBRE, DEFAULT_BEND_RANGE,
    MPE_BEND_RANGE, bend_from_semitones, pressure_from_midi, pressure_to_midi, timbre_from_midi,
    timbre_to_midi,
};
use windfall_midi::*;
use windfall_project::{
    ChannelId, Command, Document, InstrumentKind, NoteArticulation, NoteCurveParameter,
    NoteCurvePoint, NoteExpression, NoteExpressionCurve, NoteInit, PatternId, Project,
};

const FIRST_PATTERN: PatternId = PatternId(1);
const CENTER: u16 = 8_192;

fn synth(document: &mut Document, name: &str) -> ChannelId {
    let applied = document
        .dispatch(
            Command::AddChannel {
                name: Some(name.to_owned()),
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .expect("a channel");
    ChannelId(applied.created[0])
}

fn note(start: u32, length: u32, key: u8, expression: NoteExpression) -> NoteInit {
    NoteInit {
        start,
        length,
        key,
        velocity: Some(0.8),
        pan: None,
        expression: Some(expression),
    }
}

fn add(document: &mut Document, channel: ChannelId, notes: Vec<NoteInit>) {
    document
        .dispatch(
            Command::AddNotes {
                pattern: FIRST_PATTERN,
                channel,
                notes,
            },
            None,
        )
        .expect("notes");
    document.project().check().expect("a valid project");
}

fn voice() -> ExportOptions {
    ExportOptions {
        expression: Some(ExpressionOptions::channel_voice()),
        ..ExportOptions::default()
    }
}

/// Exports the first pattern, writes it and reads the bytes back.
fn round(document: &Document, options: &ExportOptions) -> (MidiSong, ExportReport) {
    let exported =
        export_pattern_reported(document.project(), FIRST_PATTERN, options).expect("it exports");
    let bytes = write(&exported.song, &WriteOptions::default());
    (read(&bytes).expect("it reads back"), exported.report)
}

fn points(song: &MidiSong, channel: u8, kind: ControlKind) -> Vec<(u32, u16)> {
    song.tracks
        .iter()
        .filter_map(|track| track.control(channel, kind))
        .flat_map(|curve| curve.points.iter().map(|point| (point.tick, point.value)))
        .collect()
}

fn value_at(points: &[(u32, u16)], tick: u32) -> Option<u16> {
    points
        .iter()
        .rev()
        .find(|point| point.0 <= tick)
        .map(|point| point.1)
}

fn plain() -> NoteExpression {
    NoteExpression::default()
}

#[test]
fn leaving_expression_out_writes_the_file_it_always_did_and_says_what_is_missing() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(0, 480, 60, plain()),
            note(
                480,
                480,
                62,
                NoteExpression {
                    fine_pitch_cents: 50.0,
                    ..plain()
                },
            ),
            note(
                960,
                240,
                64,
                NoteExpression {
                    articulation: NoteArticulation::Slide,
                    ..plain()
                },
            ),
        ],
    );
    let off = ExportOptions::default();
    assert_eq!(off.expression, None);
    let exported =
        export_pattern_reported(document.project(), FIRST_PATTERN, &off).expect("it exports");
    let song = export_pattern(document.project(), FIRST_PATTERN, &off).expect("it exports");
    assert_eq!(exported.song, song);
    // Notes only: the slide plays no note and nothing bends.
    assert_eq!(song.tracks[0].notes.len(), 2);
    assert!(
        song.tracks
            .iter()
            .all(|track| track.control(0, ControlKind::PitchBend).is_none())
    );
    // The report counts the two notes that have something a plain file drops.
    assert_eq!(exported.report.mode, None);
    assert_eq!(exported.report.unexported_notes, 2);
    assert!(!exported.report.is_clean());
}

#[test]
fn fine_pitch_and_modulation_go_to_the_notes_channel_and_come_back_to_rest() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(
                0,
                480,
                60,
                NoteExpression {
                    fine_pitch_cents: 100.0,
                    modulation_x: 1.0,
                    modulation_y: 0.75,
                    ..plain()
                },
            ),
            // A second note, clear of the first, that wants nothing.
            note(960, 480, 62, plain()),
        ],
    );
    let (song, report) = round(&document, &voice());
    let channel = song.tracks[0].notes[0].channel;
    let bend = points(&song, channel, ControlKind::PitchBend);
    let (wanted, clamped) = bend_from_semitones(1.0, DEFAULT_BEND_RANGE);
    assert!(!clamped);
    assert_eq!(value_at(&bend, 0), Some(wanted));
    assert_eq!(wanted, CENTER + (f64::from(CENTER) / 12.0).round() as u16);
    // At the note's end everything it moved goes back, so the next note is plain.
    assert_eq!(value_at(&bend, 480), Some(CENTER));
    assert_eq!(value_at(&bend, 960), Some(CENTER));

    let x = points(&song, channel, ControlKind::Controller(CC_TIMBRE));
    assert_eq!(value_at(&x, 0), Some(u16::from(timbre_to_midi(1.0))));
    assert_eq!(value_at(&x, 480), Some(64));
    let y = points(&song, channel, ControlKind::Controller(CC_MODULATION));
    assert_eq!(value_at(&y, 0), Some(u16::from(pressure_to_midi(0.75).0)));
    assert_eq!(value_at(&y, 480), Some(0));

    // The bend range is set, as registered parameter 0, before the first bend.
    let coarse = points(&song, channel, ControlKind::Controller(CC_RPN_COARSE));
    let fine = points(&song, channel, ControlKind::Controller(CC_RPN_FINE));
    let data = points(&song, channel, ControlKind::Controller(CC_DATA_ENTRY));
    assert_eq!(coarse.first(), Some(&(0, 0)));
    assert_eq!(fine.first(), Some(&(0, 0)));
    assert_eq!(data.first(), Some(&(0, u16::from(DEFAULT_BEND_RANGE))));

    assert_eq!(report.mode, Some(ExpressionMode::ChannelVoice));
    assert_eq!(report.bend_range, DEFAULT_BEND_RANGE);
    assert_eq!(report.overlap_notes, 0);
    assert_eq!(report.bend_clamped_notes, 0);
    assert!(report.is_clean());
    assert!(report.messages > 0);
}

#[test]
fn a_slide_bends_the_held_note_without_a_note_of_its_own() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(0, 1_920, 60, plain()),
            // Up a fifth over a quarter note, starting a beat in.
            note(
                480,
                480,
                67,
                NoteExpression {
                    articulation: NoteArticulation::Slide,
                    ..plain()
                },
            ),
        ],
    );
    let (song, report) = round(&document, &voice());
    assert_eq!(song.tracks[0].notes.len(), 1, "the slide is no note-on");
    assert_eq!(report.slides, 1);
    assert_eq!(report.slides_without_target, 0);
    let channel = song.tracks[0].notes[0].channel;
    let bend = points(&song, channel, ControlKind::PitchBend);
    // Level until the slide, rising all the way through it, level after.
    assert_eq!(value_at(&bend, 479).unwrap_or(CENTER), CENTER);
    let top = bend_from_semitones(7.0, DEFAULT_BEND_RANGE).0;
    assert_eq!(value_at(&bend, 960), Some(top));
    assert_eq!(value_at(&bend, 1_900), Some(top));
    let rising: Vec<u16> = bend
        .iter()
        .filter(|point| (480..=960).contains(&point.0))
        .map(|point| point.1)
        .collect();
    assert!(rising.len() > 10, "the ramp is written in steps");
    assert!(rising.windows(2).all(|pair| pair[0] < pair[1]));
    let middle = value_at(&bend, 720).expect("a value half way");
    let half = bend_from_semitones(3.5, DEFAULT_BEND_RANGE).0;
    assert!(middle.abs_diff(half) < 40, "{middle} is not near {half}");
    // And back to rest when the held note ends.
    assert_eq!(value_at(&bend, 1_920), Some(CENTER));
}

#[test]
fn a_slide_with_nothing_held_writes_nothing_and_is_counted() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(0, 240, 60, plain()),
            note(
                960,
                240,
                67,
                NoteExpression {
                    articulation: NoteArticulation::Slide,
                    ..plain()
                },
            ),
        ],
    );
    let (song, report) = round(&document, &voice());
    assert_eq!(report.slides, 0);
    assert_eq!(report.slides_without_target, 1);
    let channel = song.tracks[0].notes[0].channel;
    assert!(points(&song, channel, ControlKind::PitchBend).is_empty());
}

#[test]
fn a_portamento_starts_at_the_note_before_and_glides_to_its_own_pitch() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(0, 960, 60, plain()),
            note(
                480,
                960,
                64,
                NoteExpression {
                    articulation: NoteArticulation::Portamento,
                    glide_ticks: 240,
                    ..plain()
                },
            ),
        ],
    );
    let (song, report) = round(&document, &voice());
    assert_eq!(
        song.tracks[0].notes.len(),
        2,
        "the destination is a real note"
    );
    assert_eq!(report.glides, 1);
    let channel = song.tracks[0].notes[1].channel;
    let bend = points(&song, channel, ControlKind::PitchBend);
    // Four semitones below its key when it starts: the pitch of the note before.
    let from = bend_from_semitones(-4.0, DEFAULT_BEND_RANGE).0;
    assert_eq!(value_at(&bend, 480), Some(from));
    assert_eq!(value_at(&bend, 720), Some(CENTER));
    assert_eq!(value_at(&bend, 1_200), Some(CENTER));
    // The held note is bent along with it, which the report says.
    assert_eq!(report.overlap_notes, 1);
    assert!(!report.is_clean());
}

#[test]
fn colour_groups_choose_the_channel_and_each_channel_has_its_own_bend() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    let group = |group: u8, cents: f32| NoteExpression {
        color_group: Some(group),
        fine_pitch_cents: cents,
        ..plain()
    };
    add(
        &mut document,
        lead,
        vec![
            note(0, 480, 60, group(3, 200.0)),
            // The drum channel is a colour like any other.
            note(0, 480, 62, group(9, -200.0)),
            note(0, 480, 64, group(15, 0.0)),
        ],
    );
    let (song, report) = round(&document, &voice());
    let channels: Vec<u8> = song.tracks[0]
        .notes
        .iter()
        .map(|note| note.channel)
        .collect();
    assert_eq!(channels, [3, 9, 15]);
    let up = bend_from_semitones(2.0, DEFAULT_BEND_RANGE).0;
    let down = bend_from_semitones(-2.0, DEFAULT_BEND_RANGE).0;
    assert_eq!(
        value_at(&points(&song, 3, ControlKind::PitchBend), 0),
        Some(up)
    );
    assert_eq!(
        value_at(&points(&song, 9, ControlKind::PitchBend), 0),
        Some(down)
    );
    assert!(points(&song, 15, ControlKind::PitchBend).is_empty());
    // Three notes at once on three channels overlap nothing.
    assert_eq!(report.overlap_notes, 0);
}

#[test]
fn a_bend_past_the_range_is_held_at_its_end_and_counted() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![note(
            0,
            480,
            60,
            NoteExpression {
                fine_pitch_cents: 1_200.0,
                ..plain()
            },
        )],
    );
    let narrow = ExportOptions {
        expression: Some(ExpressionOptions {
            bend_range: 2,
            ..ExpressionOptions::channel_voice()
        }),
        ..ExportOptions::default()
    };
    let (song, report) = round(&document, &narrow);
    let channel = song.tracks[0].notes[0].channel;
    assert_eq!(
        value_at(&points(&song, channel, ControlKind::PitchBend), 0),
        Some(16_383)
    );
    assert_eq!(report.bend_clamped_notes, 1);
    assert_eq!(report.bend_range, 2);
    let data = points(&song, channel, ControlKind::Controller(CC_DATA_ENTRY));
    assert_eq!(data.first(), Some(&(0, 2)));
}

#[test]
fn a_saved_curve_is_sampled_along_its_note() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(&mut document, lead, vec![note(0, 960, 60, plain())]);
    let stored = document.project().pattern(FIRST_PATTERN).unwrap().lanes[0].notes[0];
    document
        .dispatch(
            Command::SetNoteExpressionCurves {
                pattern: FIRST_PATTERN,
                channel: lead,
                expected: vec![stored],
                expected_curves: Vec::new(),
                curves: vec![NoteExpressionCurve {
                    note: stored.id,
                    parameter: NoteCurveParameter::ModulationX,
                    points: vec![
                        NoteCurvePoint {
                            position: 0.0,
                            value: 0.5,
                            curve: 0.0,
                            hold: false,
                        },
                        NoteCurvePoint {
                            position: 1.0,
                            value: 1.0,
                            curve: 0.0,
                            hold: false,
                        },
                    ],
                }],
            },
            None,
        )
        .expect("a curve");
    let (song, _) = round(&document, &voice());
    let channel = song.tracks[0].notes[0].channel;
    let x = points(&song, channel, ControlKind::Controller(CC_TIMBRE));
    let inside: Vec<u16> = x
        .iter()
        .filter(|point| point.0 > 0 && point.0 < 960)
        .map(|point| point.1)
        .collect();
    assert!(inside.len() > 20, "the curve is written in steps");
    assert!(inside.windows(2).all(|pair| pair[0] < pair[1]));
    let middle = value_at(&x, 480).expect("a value half way");
    assert!(middle.abs_diff(u16::from(timbre_to_midi(0.75))) <= 2);
    assert_eq!(value_at(&x, 960), Some(64));
}

#[test]
fn mpe_gives_every_note_a_member_channel_and_sets_the_zone() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    let bent = |cents: f32| NoteExpression {
        fine_pitch_cents: cents,
        modulation_y: 1.0,
        ..plain()
    };
    add(
        &mut document,
        lead,
        vec![
            note(0, 960, 60, bent(100.0)),
            note(0, 960, 64, bent(-100.0)),
            note(0, 960, 67, bent(0.0)),
            // After the chord: the channel that has been silent longest.
            note(960, 480, 72, bent(50.0)),
        ],
    );
    let mpe = ExportOptions {
        expression: Some(ExpressionOptions::mpe()),
        ..ExportOptions::default()
    };
    let (song, report) = round(&document, &mpe);
    let notes = &song.tracks[0].notes;
    let channels: Vec<u8> = notes.iter().map(|note| note.channel).collect();
    // Member channels 2 to 16 are 1 to 15 counted from 0; none is the master.
    assert!(channels.iter().all(|channel| (1..=15).contains(channel)));
    let mut chord = channels[..3].to_vec();
    chord.sort_unstable();
    chord.dedup();
    assert_eq!(chord.len(), 3, "the notes of a chord each have a channel");
    assert!(!channels[..3].contains(&channels[3]));

    // Each note bends its own channel, by the MPE range, with pressure for Y.
    for (note, cents) in notes.iter().zip([100.0, -100.0, 0.0, 50.0]) {
        let bend = points(&song, note.channel, ControlKind::PitchBend);
        let wanted = bend_from_semitones(cents / 100.0, MPE_BEND_RANGE).0;
        assert_eq!(value_at(&bend, note.start).unwrap_or(CENTER), wanted);
        let pressure = points(&song, note.channel, ControlKind::ChannelPressure);
        assert_eq!(value_at(&pressure, note.start), Some(127));
    }
    // The zone is declared on the master channel: registered parameter 6, 15 members.
    let fine = points(&song, 0, ControlKind::Controller(CC_RPN_FINE));
    let data = points(&song, 0, ControlKind::Controller(CC_DATA_ENTRY));
    assert_eq!(fine.first(), Some(&(0, 6)));
    assert_eq!(data.first(), Some(&(0, 15)));
    assert_eq!(report.mode, Some(ExpressionMode::Mpe));
    assert_eq!(report.mpe_shared_notes, 0);
    assert_eq!(report.overlap_notes, 0);
}

#[test]
fn a_file_with_expression_imports_its_fine_pitch_and_modulation() {
    let mut document = Document::new(Project::new("Test"));
    let lead = synth(&mut document, "Lead");
    add(
        &mut document,
        lead,
        vec![
            note(
                0,
                480,
                60,
                NoteExpression {
                    fine_pitch_cents: 50.0,
                    modulation_x: 0.25,
                    modulation_y: 1.0,
                    color_group: Some(4),
                    ..plain()
                },
            ),
            note(
                960,
                480,
                62,
                NoteExpression {
                    color_group: Some(4),
                    ..plain()
                },
            ),
        ],
    );
    let (song, _) = round(&document, &voice());
    let plan = import(&song, &ImportOptions::default());
    let mut imported = Document::new(Project::new("Back"));
    let command = plan.command(imported.project());
    imported.dispatch(command, None).expect("the plan applies");
    let notes: Vec<_> = imported
        .project()
        .patterns
        .iter()
        .flat_map(|pattern| &pattern.lanes)
        .flat_map(|lane| &lane.notes)
        .collect();
    assert_eq!(notes.len(), 2);
    let first = notes
        .iter()
        .find(|note| note.key == 60)
        .expect("the bent note");
    let second = notes
        .iter()
        .find(|note| note.key == 62)
        .expect("the plain note");
    assert!((first.expression.fine_pitch_cents - 50.0).abs() < 0.2);
    assert_eq!(
        first.expression.modulation_x,
        timbre_from_midi(timbre_to_midi(0.25))
    );
    assert_eq!(first.expression.modulation_y, pressure_from_midi(127));
    assert_eq!(first.expression.color_group, Some(4));
    assert_eq!(second.expression.fine_pitch_cents, 0.0);
    assert_eq!(second.expression.modulation_x, 0.5);
    assert_eq!(second.expression.modulation_y, 0.5);
    assert!(
        plan.adjustments
            .iter()
            .any(|adjustment| matches!(adjustment, Adjustment::ExpressionImported { notes: 1 }))
    );

    // Told not to, the import leaves the bend behind and says so.
    let plain_import = ImportOptions {
        note_expression: false,
        ..ImportOptions::default()
    };
    let plan = import(&song, &plain_import);
    assert!(plan.adjustments.iter().any(|adjustment| matches!(
        adjustment,
        Adjustment::NotImported {
            what: Unsupported::PitchBend,
            ..
        }
    )));
}
