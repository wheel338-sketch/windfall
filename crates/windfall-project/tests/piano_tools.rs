use windfall_project::piano_tools::{MAX_TOOL_NOTES, transform_selected_notes};
use windfall_project::*;

#[path = "piano_tools/rhythm.rs"]
mod rhythm;

fn note(id: u32, start: u32, length: u32, key: u8) -> Note {
    Note {
        id: NoteId(id),
        start,
        length,
        key,
        velocity: 0.5,
        pan: -0.25,
        expression: Default::default(),
    }
}

#[test]
fn articulation_uses_distinct_onsets_and_preserves_last_chord() {
    let notes = [
        note(1, 0, 40, 60),
        note(2, 0, 100, 64),
        note(3, 240, 500, 67),
    ];
    let legato = transform_selected_notes(&notes, NoteTransform::Legato).unwrap();
    assert_eq!(
        legato.iter().map(|n| n.length).collect::<Vec<_>>(),
        [240, 240, 500]
    );
    let short = transform_selected_notes(
        &[note(1, 0, 3, 60)],
        NoteTransform::Staccato { factor: 0.5 },
    )
    .unwrap();
    assert_eq!(short[0].length, 2);
    assert_eq!(short[0].pan, -0.25);
}

#[test]
fn chop_is_absolute_grid_keeps_remainder_and_first_id() {
    let result =
        transform_selected_notes(&[note(9, 50, 500, 60)], NoteTransform::Chop { grid: 240 })
            .unwrap();
    assert_eq!(
        result,
        [
            note(9, 50, 190, 60),
            note(0, 240, 240, 60),
            note(0, 480, 70, 60)
        ]
    );
    assert!(
        transform_selected_notes(
            &[note(9, 0, MAX_PATTERN_TICKS, 60)],
            NoteTransform::Chop { grid: 1 }
        )
        .is_err()
    );
}

#[test]
fn glue_unions_transitively_but_preserves_incompatible_notes() {
    let mut different = note(4, 150, 100, 60);
    different.velocity = 0.7;
    let mut pan = note(5, 100, 50, 60);
    pan.pan = 0.25;
    let notes = [
        note(3, 190, 100, 60),
        different,
        note(1, 0, 100, 60),
        pan,
        note(2, 100, 100, 60),
        note(6, 290, 40, 61),
    ];
    let result = transform_selected_notes(&notes, NoteTransform::Glue).unwrap();
    assert_eq!(
        result,
        [note(1, 0, 290, 60), pan, different, note(6, 290, 40, 61)]
    );
    assert_eq!(
        transform_selected_notes(
            &[note(2, 0, 100, 60), note(1, 0, 100, 60)],
            NoteTransform::Glue
        )
        .unwrap(),
        [note(1, 0, 100, 60)]
    );
    // Signed zero is numerically compatible. Other pan groups must not split
    // its touching notes simply because total float ordering distinguishes it.
    let mut zero = note(1, 0, 100, 60);
    zero.velocity = -0.0;
    zero.pan = -0.0;
    let mut incompatible = note(2, 0, 100, 60);
    incompatible.velocity = -0.0;
    incompatible.pan = 0.5;
    let mut touching = note(3, 100, 100, 60);
    touching.velocity = 0.0;
    touching.pan = 0.0;
    let mut united = zero;
    united.length = 200;
    assert_eq!(
        transform_selected_notes(&[zero, incompatible, touching], NoteTransform::Glue).unwrap(),
        [united, incompatible]
    );
}

#[test]
fn strum_orders_by_pitch_then_id_and_clamps_velocity() {
    let notes = [
        note(3, 100, 80, 67),
        note(1, 100, 80, 60),
        note(2, 100, 80, 64),
        note(4, 500, 80, 72),
    ];
    let result = transform_selected_notes(
        &notes,
        NoteTransform::Strum {
            spacing: 20,
            velocity_step: 0.4,
            descending: false,
        },
    )
    .unwrap();
    assert_eq!(
        result
            .iter()
            .map(|n| (n.start, n.key, n.velocity))
            .collect::<Vec<_>>(),
        [
            (100, 60, 0.5),
            (120, 64, 0.9),
            (140, 67, 1.0),
            (500, 72, 0.5)
        ]
    );
    let result = transform_selected_notes(
        &notes,
        NoteTransform::Strum {
            spacing: 20,
            velocity_step: -0.4,
            descending: true,
        },
    )
    .unwrap();
    assert_eq!(result[0].key, 67);
    assert_eq!(result[2].velocity, 0.0);
    assert!(
        transform_selected_notes(
            &[
                note(1, MAX_PATTERN_TICKS - 2, 2, 60),
                note(2, MAX_PATTERN_TICKS - 2, 2, 64)
            ],
            NoteTransform::Strum {
                spacing: 1,
                velocity_step: 0.0,
                descending: false
            }
        )
        .is_err()
    );
}

#[test]
fn flips_are_involutions_and_preserve_lengths_properties() {
    let notes = [note(1, 100, 50, 100), note(2, 240, 150, 127)];
    let flipped = transform_selected_notes(&notes, NoteTransform::FlipTime).unwrap();
    assert_eq!(flipped, [note(2, 100, 150, 127), note(1, 340, 50, 100)]);
    for tool in [NoteTransform::FlipTime, NoteTransform::FlipPitch] {
        assert_eq!(
            transform_selected_notes(
                &transform_selected_notes(&notes, tool.clone()).unwrap(),
                tool
            )
            .unwrap(),
            notes
        );
    }
}

#[test]
fn range_transposes_clamps_or_folds_with_a_deliberate_narrow_range_fallback() {
    let notes = [note(1, 0, 1, 0), note(2, 1, 1, 127)];
    let result = transform_selected_notes(
        &notes,
        NoteTransform::KeyRange {
            low: 48,
            high: 72,
            transpose: 12,
            octaves: false,
        },
    )
    .unwrap();
    assert_eq!(result.iter().map(|n| n.key).collect::<Vec<_>>(), [48, 72]);
    let result = transform_selected_notes(
        &notes,
        NoteTransform::KeyRange {
            low: 48,
            high: 72,
            transpose: 0,
            octaves: true,
        },
    )
    .unwrap();
    assert_eq!(result.iter().map(|n| n.key).collect::<Vec<_>>(), [48, 67]);
    let result = transform_selected_notes(
        &notes,
        NoteTransform::KeyRange {
            low: 60,
            high: 60,
            transpose: 0,
            octaves: true,
        },
    )
    .unwrap();
    assert_eq!(result.iter().map(|n| n.key).collect::<Vec<_>>(), [60, 60]);
}

#[test]
fn quantize_strength_grooves_ties_and_boundaries() {
    let q = |edge, groove, strength| NoteTransform::Quantize {
        musical: None,
        grid: 240,
        strength,
        edge,
        groove,
    };
    let notes = [note(1, 250, 100, 60)];
    assert_eq!(
        transform_selected_notes(&notes, q(NoteEdge::Start, NoteGroove::Straight, 0.5)).unwrap()[0]
            .start,
        245
    );
    for (groove, target) in [
        (NoteGroove::Straight, 240),
        (NoteGroove::Swing, 280),
        (NoteGroove::LatePairs, 300),
    ] {
        assert_eq!(
            transform_selected_notes(&notes, q(NoteEdge::Start, groove, 1.0)).unwrap()[0].start,
            target
        );
    }
    assert_eq!(
        transform_selected_notes(
            &[note(1, 700, 100, 60)],
            q(NoteEdge::Start, NoteGroove::PushFour, 1.0)
        )
        .unwrap()[0]
            .start,
        680
    );
    assert_eq!(
        transform_selected_notes(
            &[note(1, 120, 1, 60)],
            q(NoteEdge::Start, NoteGroove::Straight, 1.0)
        )
        .unwrap()[0]
            .start,
        240
    );
    assert_eq!(
        transform_selected_notes(
            &[note(1, 1, 1, 60)],
            q(NoteEdge::End, NoteGroove::Straight, 1.0)
        )
        .unwrap()[0]
            .length,
        1
    );
    let last = [note(1, MAX_PATTERN_TICKS - 2, 2, 60)];
    assert_eq!(
        transform_selected_notes(&last, q(NoteEdge::Start, NoteGroove::Straight, 1.0)).unwrap(),
        last
    );
    assert_eq!(
        transform_selected_notes(&notes, q(NoteEdge::Start, NoteGroove::Swing, 0.0)).unwrap(),
        notes
    );
}

#[test]
fn invalid_inputs_and_duplicate_order_are_deliberate() {
    let notes = [
        note(2, 240, 240, 64),
        note(1, 0, 240, 60),
        note(1, 0, 240, 60),
    ];
    let normalized =
        transform_selected_notes(&notes, NoteTransform::ScaleVelocity { factor: 1.0 }).unwrap();
    assert_eq!(normalized, [notes[1], notes[0]]);
    for factor in [f64::NAN, f64::INFINITY, -1.0, 5.0] {
        assert!(transform_selected_notes(&notes, NoteTransform::ScaleVelocity { factor }).is_err());
    }
    for bad in [
        note(1, 0, 0, 60),
        note(1, MAX_PATTERN_TICKS, 1, 60),
        note(1, MAX_PATTERN_TICKS - 1, 2, 60),
        note(1, 0, 1, 128),
        Note {
            velocity: f32::NAN,
            ..notes[1]
        },
    ] {
        assert!(transform_selected_notes(&[bad], NoteTransform::Legato).is_err());
    }
    assert!(transform_selected_notes(&[], NoteTransform::Glue).is_err());
    assert!(
        transform_selected_notes(&vec![notes[0]; MAX_TOOL_NOTES + 1], NoteTransform::Glue).is_err()
    );
    assert!(
        transform_selected_notes(
            &[
                notes[0],
                Note {
                    length: 5,
                    ..notes[0]
                }
            ],
            NoteTransform::Glue
        )
        .is_err()
    );
    assert!(transform_selected_notes(&notes, NoteTransform::Chop { grid: 0 }).is_err());
    assert!(
        transform_selected_notes(
            &notes,
            NoteTransform::KeyRange {
                low: 70,
                high: 60,
                transpose: 0,
                octaves: false
            }
        )
        .is_err()
    );
}

fn fixture() -> (Document, ChannelId, ChannelId) {
    let mut doc = Document::new(Project::new("Piano tools"));
    let mut add = || {
        ChannelId(
            doc.dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: None,
                    mixer_track: None,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
        )
    };
    let a = add();
    let b = add();
    for channel in [a, b] {
        doc.dispatch(
            Command::AddNotes {
                pattern: PatternId(1),
                channel,
                notes: vec![
                    NoteInit {
                        start: 3800,
                        length: 400,
                        key: 60,
                        velocity: Some(0.5),
                        pan: Some(-0.25),
                        expression: None,
                    },
                    NoteInit {
                        start: 3840,
                        length: 240,
                        key: 64,
                        velocity: Some(0.7),
                        pan: Some(0.25),
                        expression: None,
                    },
                ],
            },
            None,
        )
        .unwrap();
    }
    (doc, a, b)
}

fn selected(doc: &Document, channel: ChannelId) -> Vec<Note> {
    doc.project().patterns[0]
        .lanes
        .iter()
        .find(|l| l.channel == channel)
        .unwrap()
        .notes
        .clone()
}

#[test]
fn each_tool_is_one_atomic_undoable_persistable_command() {
    let tools = [
        NoteTransform::Legato,
        NoteTransform::Staccato { factor: 0.5 },
        NoteTransform::Chop { grid: 120 },
        NoteTransform::Glue,
        NoteTransform::Strum {
            spacing: 20,
            velocity_step: -0.1,
            descending: true,
        },
        NoteTransform::FlipTime,
        NoteTransform::FlipPitch,
        NoteTransform::KeyRange {
            low: 65,
            high: 72,
            transpose: 12,
            octaves: false,
        },
        NoteTransform::ScaleVelocity { factor: 2.0 },
        NoteTransform::Quantize {
            musical: None,
            grid: 240,
            strength: 0.75,
            edge: NoteEdge::Start,
            groove: NoteGroove::Swing,
        },
    ];
    for transform in tools {
        let (mut doc, channel, other) = fixture();
        let before = doc.project().clone();
        let cursor = doc.history().cursor;
        let mut notes = selected(&doc, channel);
        // Make glue and strum meaningful; the remaining fixture note stays unrelated.
        if matches!(transform, NoteTransform::Glue | NoteTransform::Strum { .. }) {
            let first = notes[0];
            doc.dispatch(
                Command::AddNotes {
                    pattern: PatternId(1),
                    channel,
                    notes: vec![NoteInit {
                        start: first.start,
                        length: 200,
                        key: first.key,
                        velocity: Some(first.velocity),
                        pan: Some(first.pan),
                        expression: None,
                    }],
                },
                None,
            )
            .unwrap();
            notes = selected(&doc, channel);
        }
        let actual_before = doc.project().clone();
        let actual_cursor = doc.history().cursor;
        let command = Command::TransformNotes {
            pattern: PatternId(1),
            channel,
            notes,
            transform: transform.clone(),
        };
        let encoded = serde_json::to_string(&command).unwrap();
        let applied = doc
            .dispatch(serde_json::from_str(&encoded).unwrap(), None)
            .unwrap();
        assert_eq!(
            windfall_core::english::english(&applied.label),
            transform.label()
        );
        assert_eq!(
            selected(&doc, other),
            selected(&Document::new(before), other)
        );
        let after = doc.project().clone();
        if !applied.touched.is_empty() {
            assert_eq!(doc.history().cursor, actual_cursor + 1);
            let end = selected(&doc, channel)
                .iter()
                .map(|n| n.start + n.length)
                .max()
                .unwrap();
            assert_eq!(
                after.patterns[0].length_steps,
                actual_before.patterns[0]
                    .length_steps
                    .max(end.div_ceil(TICKS_PER_STEP))
            );
            doc.undo().unwrap();
            let mut expected = actual_before;
            expected.next_id = doc.project().next_id;
            assert_eq!(doc.project(), &expected);
            doc.redo().unwrap();
            assert_eq!(doc.project(), &after);
        }
        assert!(doc.history().cursor >= cursor);
        after.check().unwrap();
        let reopened =
            windfall_project::file::from_json(&windfall_project::file::to_json(&after).unwrap())
                .unwrap();
        assert_eq!(reopened, after);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("tools.windfall");
        file::save(&after, &path).unwrap();
        assert_eq!(file::load(&path).unwrap(), after);
    }
}

#[test]
fn stale_wrong_lane_empty_and_overflow_leave_project_history_and_ids_intact() {
    let (mut doc, channel, other) = fixture();
    let original = selected(&doc, channel);
    for (target, notes, transform) in [
        (other, original.clone(), NoteTransform::FlipPitch),
        (channel, vec![], NoteTransform::Glue),
        (
            channel,
            vec![Note {
                velocity: 0.9,
                ..original[0]
            }],
            NoteTransform::FlipPitch,
        ),
        (channel, original.clone(), NoteTransform::Chop { grid: 0 }),
    ] {
        let before = doc.project().clone();
        let history = doc.history();
        assert!(
            doc.dispatch(
                Command::TransformNotes {
                    pattern: PatternId(1),
                    channel: target,
                    notes,
                    transform
                },
                None
            )
            .is_err()
        );
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
    }
    let mut exhausted = doc.project().clone();
    exhausted.next_id = u32::MAX;
    let mut doc = Document::new(exhausted.clone());
    assert!(
        doc.dispatch(
            Command::TransformNotes {
                pattern: PatternId(1),
                channel,
                notes: original,
                transform: NoteTransform::Chop { grid: 120 }
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.project(), &exhausted);
    assert_eq!(doc.history().cursor, 0);
}

#[test]
fn no_op_keeps_redo_and_dirty_state() {
    let (mut doc, channel, _) = fixture();
    let notes = selected(&doc, channel);
    doc.dispatch(
        Command::TransformNotes {
            pattern: PatternId(1),
            channel,
            notes: notes.clone(),
            transform: NoteTransform::ScaleVelocity { factor: 2.0 },
        },
        None,
    )
    .unwrap();
    doc.undo();
    doc.mark_saved();
    let history = doc.history();
    let applied = doc
        .dispatch(
            Command::TransformNotes {
                pattern: PatternId(1),
                channel,
                notes,
                transform: NoteTransform::ScaleVelocity { factor: 1.0 },
            },
            None,
        )
        .unwrap();
    assert!(applied.touched.is_empty());
    assert_eq!(doc.history(), history);
    assert!(!doc.is_dirty());
}

#[test]
fn every_parameter_is_checked_and_results_are_order_independent() {
    let notes = [note(2, 240, 180, 64), note(1, 0, 100, 60)];
    for transform in [
        NoteTransform::Staccato { factor: 0.0 },
        NoteTransform::Staccato { factor: f64::NAN },
        NoteTransform::Staccato { factor: 1.1 },
        NoteTransform::Chop {
            grid: MAX_PATTERN_TICKS + 1,
        },
        NoteTransform::Strum {
            spacing: MAX_PATTERN_TICKS + 1,
            velocity_step: 0.0,
            descending: false,
        },
        NoteTransform::Strum {
            spacing: 0,
            velocity_step: f64::INFINITY,
            descending: true,
        },
        NoteTransform::KeyRange {
            low: 0,
            high: 128,
            transpose: 0,
            octaves: true,
        },
        NoteTransform::KeyRange {
            low: 0,
            high: 127,
            transpose: 128,
            octaves: false,
        },
        NoteTransform::Quantize {
            musical: None,
            grid: 0,
            strength: 1.0,
            edge: NoteEdge::Start,
            groove: NoteGroove::Straight,
        },
        NoteTransform::Quantize {
            musical: None,
            grid: 240,
            strength: f64::NAN,
            edge: NoteEdge::End,
            groove: NoteGroove::Swing,
        },
        NoteTransform::Quantize {
            musical: None,
            grid: 240,
            strength: 1.1,
            edge: NoteEdge::Start,
            groove: NoteGroove::Straight,
        },
    ] {
        assert!(transform_selected_notes(&notes, transform).is_err());
    }
    for transform in [
        NoteTransform::Legato,
        NoteTransform::Staccato { factor: 0.5 },
        NoteTransform::Chop { grid: 120 },
        NoteTransform::Glue,
        NoteTransform::Strum {
            spacing: 1,
            velocity_step: 0.1,
            descending: true,
        },
        NoteTransform::FlipTime,
        NoteTransform::FlipPitch,
        NoteTransform::KeyRange {
            low: 48,
            high: 72,
            transpose: 5,
            octaves: true,
        },
        NoteTransform::ScaleVelocity { factor: 4.0 },
        NoteTransform::Quantize {
            musical: None,
            grid: 240,
            strength: 0.5,
            edge: NoteEdge::End,
            groove: NoteGroove::LatePairs,
        },
    ] {
        assert_eq!(
            transform_selected_notes(&notes, transform.clone()),
            transform_selected_notes(&[notes[1], notes[0], notes[1]], transform)
        );
    }
}
