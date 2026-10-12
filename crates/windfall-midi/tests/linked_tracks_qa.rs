//! Instrument-lane filtering must be identical in playback and MIDI files.
use windfall_midi::{export_pattern, export_song, read, write, ExportOptions, WriteOptions};
use windfall_project::{arrangement::TrackKind, *};

#[test]
fn song_export_filters_each_placement_without_filtering_shared_patterns() {
    let mut doc = Document::new(Project::new("Linked MIDI"));
    let mut run = |command| doc.dispatch(command, None).unwrap().created;
    let a = ChannelId(
        run(Command::AddChannel {
            name: Some("Lead".into()),
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        })[0],
    );
    let b = ChannelId(
        run(Command::AddChannel {
            name: Some("Bass".into()),
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        })[0],
    );
    let lane = PlaylistTrackId(
        run(Command::AddPlaylistTrack {
            name: None,
            index: None,
        })[0],
    );
    let pattern = doc.project().patterns[0].id;
    for (channel, key) in [(a, 72), (b, 36)] {
        doc.dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![NoteInit {
                    start: 0,
                    length: 240,
                    key,
                    velocity: Some(1.0),
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
    }
    doc.dispatch(
        Command::AddClips {
            clips: vec![ClipInit {
                track: lane,
                start: 0,
                length: Some(7680),
                offset: Some(0),
                muted: None,
                content: ClipContent::Pattern { pattern },
            }],
        },
        None,
    )
    .unwrap();
    let all = export_song(doc.project(), &ExportOptions::default()).unwrap();
    doc.dispatch(
        Command::LinkTrack {
            track: lane,
            kind: Some(TrackKind::Instrument { channel: a }),
        },
        None,
    )
    .unwrap();
    let filtered = export_song(doc.project(), &ExportOptions::default()).unwrap();
    let notes: Vec<_> = filtered
        .tracks
        .iter()
        .flat_map(|track| &track.notes)
        .collect();
    assert_eq!(notes.len(), 2, "both repetitions of the linked channel");
    assert!(notes.iter().all(|note| note.key == 72));
    assert_eq!(
        read(&write(&filtered, &WriteOptions::default())).unwrap(),
        filtered.clone().normalized()
    );
    let pattern_song = export_pattern(doc.project(), pattern, &ExportOptions::default()).unwrap();
    assert_eq!(
        pattern_song
            .tracks
            .iter()
            .map(|track| track.notes.len())
            .sum::<usize>(),
        2
    );
    let reopened: Project =
        windfall_project::file::from_json(&windfall_project::file::to_json(doc.project()).unwrap()).unwrap();
    assert_eq!(
        export_song(&reopened, &ExportOptions::default()).unwrap(),
        filtered
    );
    doc.undo().unwrap();
    assert_eq!(
        export_song(doc.project(), &ExportOptions::default()).unwrap(),
        all
    );
    doc.redo().unwrap();
    assert_eq!(
        export_song(doc.project(), &ExportOptions::default()).unwrap(),
        filtered
    );
}
