//! The panning law and the timebase as project settings: files, history,
//! refusals and the grid playback puts notes and clips on.

use std::borrow::Cow;

use windfall_project::timebase::{self, DEFAULT_TIMEBASE_PPQ, TIMEBASES, playback_project};
use windfall_project::{
    ChannelId, ClipContent, ClipInit, Command, Document, InstrumentKind, NoteInit, PanLaw,
    PatternId, PlaylistTrackId, Project, SettingsPatch, file,
};

fn note(start: u32, length: u32, key: u8) -> NoteInit {
    NoteInit {
        start,
        length,
        key,
        velocity: None,
        pan: None,
        expression: None,
    }
}

/// A project with one synth channel, notes off every grid and one pattern
/// clip that starts, ends and is offset off every grid.
fn off_grid() -> (Document, PatternId, ChannelId) {
    let mut doc = Document::new(Project::new("Timing"));
    let pattern = doc.project().patterns[0].id;
    let channel = ChannelId(
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
    );
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![
                note(3, 7, 60),
                note(244, 233, 62),
                note(485, 1, 64),
                note(3838, 2, 65),
                // Two notes that round onto one position keep their order.
                note(1196, 100, 67),
                note(1204, 100, 66),
            ],
        },
        None,
    )
    .unwrap();
    let track = PlaylistTrackId(
        doc.dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 3847,
                length: Some(1913),
                offset: Some(119),
                muted: None,
                content: ClipContent::Pattern { pattern },
            }],
        },
        None,
    )
    .unwrap();
    (doc, pattern, channel)
}

fn set(doc: &mut Document, patch: SettingsPatch) -> String {
    doc.dispatch(Command::UpdateSettings { patch }, None)
        .unwrap()
        .label
}

#[test]
fn a_file_that_names_neither_setting_has_the_balance_law_and_960_ticks() {
    let project = Project::new("Before the settings");
    let json: serde_json::Value = serde_json::from_str(&file::to_json(&project).unwrap()).unwrap();
    let settings = &json["settings"];
    assert!(settings.is_object(), "{json}");
    // Defaults are left out, so a file an older build wrote is the same
    // file this build writes for it.
    assert!(settings.get("panLaw").is_none());
    assert!(settings.get("timebasePpq").is_none());

    let loaded = file::from_json(&file::to_json(&project).unwrap()).unwrap();
    assert_eq!(loaded.settings.pan_law, PanLaw::Balance);
    assert_eq!(loaded.settings.timebase_ppq, DEFAULT_TIMEBASE_PPQ);
    assert_eq!(loaded, project);
    assert_eq!(
        serde_json::from_str::<SettingsPatch>("{}").unwrap(),
        SettingsPatch::default()
    );
}

#[test]
fn each_setting_is_one_named_undo_step_and_survives_a_file() {
    let mut doc = Document::new(Project::new("Settings"));
    let untouched = doc.project().clone();

    for law in [PanLaw::ConstantPower, PanLaw::Compromise, PanLaw::Linear] {
        let before = doc.project().clone();
        let label = set(
            &mut doc,
            SettingsPatch {
                pan_law: Some(law),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(label, "Change panning law");
        assert_eq!(doc.project().settings.pan_law, law);
        // Nothing but the law moved.
        let mut expected = before.clone();
        expected.settings.pan_law = law;
        assert_eq!(*doc.project(), expected);
        let saved = file::to_json(doc.project()).unwrap();
        assert_eq!(file::from_json(&saved).unwrap(), expected);
        doc.undo().unwrap();
        assert_eq!(*doc.project(), before);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), expected);
    }

    for ppq in TIMEBASES {
        let before = doc.project().clone();
        let label = set(
            &mut doc,
            SettingsPatch {
                timebase_ppq: Some(ppq),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(label, "Change timebase");
        assert_eq!(doc.project().settings.timebase_ppq, ppq);
        doc.project().check().unwrap();
        let saved = file::to_json(doc.project()).unwrap();
        assert_eq!(file::from_json(&saved).unwrap(), *doc.project());
        if ppq != before.settings.timebase_ppq {
            doc.undo().unwrap();
            assert_eq!(*doc.project(), before);
            doc.redo().unwrap();
        }
    }

    // Both at once is one step with the general label.
    let before = doc.project().clone();
    let label = set(
        &mut doc,
        SettingsPatch {
            pan_law: Some(PanLaw::Balance),
            timebase_ppq: Some(96),
            ..SettingsPatch::default()
        },
    );
    assert_eq!(label, "Change project settings");
    doc.undo().unwrap();
    assert_eq!(*doc.project(), before);

    while doc.undo().is_some() {}
    assert_eq!(*doc.project(), untouched);
}

#[test]
fn a_timebase_windfall_does_not_have_is_refused_by_the_command_and_by_the_file() {
    let mut doc = Document::new(Project::new("Refused"));
    for ppq in [0, 1, 72, 100, 144, 384, 768, 961, u16::MAX] {
        let before = doc.project().clone();
        let history = doc.history().entries.len();
        let refused = doc.dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    // A valid change in the same patch must not slip through.
                    pan_law: Some(PanLaw::Linear),
                    timebase_ppq: Some(ppq),
                    ..SettingsPatch::default()
                },
            },
            None,
        );
        assert!(refused.is_err(), "{ppq}");
        assert_eq!(*doc.project(), before, "{ppq}");
        assert_eq!(doc.history().entries.len(), history, "{ppq}");

        let mut invalid = before.clone();
        invalid.settings.timebase_ppq = ppq;
        assert!(invalid.check().is_err(), "{ppq}");
        // The text a file would hold, written without the save-time check.
        let json = serde_json::to_string(&invalid).unwrap();
        assert!(
            matches!(file::from_json(&json), Err(file::LoadError::Invalid(_))),
            "{ppq}"
        );
    }
    assert!(serde_json::from_str::<SettingsPatch>(r#"{"panLaw":"circular"}"#).is_err());
}

#[test]
fn playback_puts_notes_and_clips_on_the_grid_and_leaves_the_document_as_drawn() {
    let (mut doc, pattern, channel) = off_grid();
    let drawn = doc.project().clone();
    assert!(matches!(playback_project(&drawn), Cow::Borrowed(_)));

    for ppq in TIMEBASES {
        set(
            &mut doc,
            SettingsPatch {
                timebase_ppq: Some(ppq),
                ..SettingsPatch::default()
            },
        );
        let grid = timebase::grid_ticks(ppq);
        let stored = doc.project().clone();
        let played = playback_project(&stored);

        // The document keeps what was drawn, to the tick.
        assert_eq!(
            stored.pattern(pattern).unwrap().lanes,
            drawn.pattern(pattern).unwrap().lanes,
            "{ppq}"
        );
        assert_eq!(stored.playlist.clips, drawn.playlist.clips, "{ppq}");

        let lane = played.pattern(pattern).unwrap().lane(channel).unwrap();
        let source = drawn.pattern(pattern).unwrap().lane(channel).unwrap();
        let pattern_ticks = played.pattern(pattern).unwrap().length_ticks();
        assert_eq!(lane.notes.len(), source.notes.len());
        assert!(lane.notes.is_sorted_by_key(|note| note.sort_key()), "{ppq}");
        for original in &source.notes {
            let note = lane.notes.iter().find(|n| n.id == original.id).unwrap();
            assert_eq!(note.start % grid, 0, "{ppq}: {note:?}");
            assert_eq!(note.length % grid, 0, "{ppq}: {note:?}");
            assert!(note.length >= grid, "{ppq}: {note:?}");
            assert!(note.start.abs_diff(original.start) <= grid, "{ppq}");
            // Every note that played still plays.
            assert!(note.start < pattern_ticks, "{ppq}: {note:?}");
            assert_eq!(
                (note.key, note.velocity, note.pan),
                (original.key, original.velocity, original.pan)
            );
        }
        let clip = &played.playlist.clips[0];
        let original = &drawn.playlist.clips[0];
        assert_eq!(clip.id, original.id);
        for value in [clip.start, clip.length, clip.offset] {
            assert_eq!(value % grid, 0, "{ppq}: {clip:?}");
        }
        assert!(clip.length >= grid);
        assert!(clip.start.abs_diff(original.start) * 2 <= grid, "{ppq}");
        assert!(clip.offset.abs_diff(original.offset) * 2 <= grid, "{ppq}");

        if ppq == DEFAULT_TIMEBASE_PPQ {
            assert!(matches!(played, Cow::Borrowed(_)));
            assert_eq!(*played, drawn);
        } else {
            assert!(matches!(played, Cow::Owned(_)));
        }
    }
}

#[test]
fn the_grid_positions_are_the_exact_ones_the_rounding_rule_gives() {
    let (mut doc, pattern, channel) = off_grid();
    set(
        &mut doc,
        SettingsPatch {
            timebase_ppq: Some(96),
            ..SettingsPatch::default()
        },
    );
    let stored = doc.project().clone();
    let played = playback_project(&stored);
    let mut notes: Vec<(u8, u32, u32)> = played
        .pattern(pattern)
        .unwrap()
        .lane(channel)
        .unwrap()
        .notes
        .iter()
        .map(|note| (note.key, note.start, note.length))
        .collect();
    notes.sort();
    assert_eq!(
        notes,
        [
            // 3..10 rounds to 0..10.
            (60, 0, 10),
            // 244..477 rounds to 240..480.
            (62, 240, 240),
            // 485 is exactly half way and goes later; one tick becomes ten.
            (64, 490, 10),
            // The last two ticks of the bar stay inside it.
            (65, 3830, 10),
            // 1204..1304 rounds to 1200..1300.
            (66, 1200, 100),
            // 1196..1296 rounds to 1200..1300.
            (67, 1200, 100),
        ]
    );
    let clip = &played.playlist.clips[0];
    assert_eq!((clip.start, clip.length, clip.offset), (3850, 1910, 120));

    // The widest grid: 24 ticks to the quarter note, 40 stored ticks each.
    set(
        &mut doc,
        SettingsPatch {
            timebase_ppq: Some(24),
            ..SettingsPatch::default()
        },
    );
    let stored = doc.project().clone();
    let played = playback_project(&stored);
    let clip = &played.playlist.clips[0];
    assert_eq!((clip.start, clip.length, clip.offset), (3840, 1920, 120));
}
