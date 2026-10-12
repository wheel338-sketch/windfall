//! Synthetic MIDI files through the real shell, sample loader and history.

use windfall_ipc::{MidiExportOptions, MidiImportOptions};
use windfall_midi::{
    KeySignatureChange, Marker, MidiNote, MidiSong, MidiTrack, TempoChange, WriteOptions,
};
use windfall_project::{Command, PlayMode};

use super::Rig;

fn fixture(rig: &Rig, channel: u8) -> String {
    let path = rig.file("authored.mid");
    let song = MidiSong {
        tracks: vec![MidiTrack {
            name: Some("Imported lead".to_owned()),
            notes: vec![MidiNote {
                start: 0,
                length: 960,
                key: 36,
                velocity: 100,
                release: 64,
                channel,
            }],
            ..Default::default()
        }],
        tempos: vec![
            TempoChange::from_bpm(0, 120.0),
            TempoChange::from_bpm(960, 100.0),
        ],
        markers: vec![Marker {
            tick: 0,
            text: "Verse".to_owned(),
        }],
        length: 1920,
        ..Default::default()
    };
    windfall_midi::write_file(&path, &song, &WriteOptions::default()).expect("fixture writes");
    path
}

fn export_options(rig: &Rig, mode: PlayMode) -> MidiExportOptions {
    MidiExportOptions {
        mode,
        pattern: rig.pattern(),
        single_track: false,
        ppq: 960,
        running_status: true,
        swing: true,
        expression: Default::default(),
    }
}

#[test]
fn a_checked_import_failure_keeps_document_history_and_review_intact() {
    let rig = Rig::new();
    let preview = rig
        .session
        .midi_preview(&fixture(&rig, 0), MidiImportOptions::default())
        .expect("reviews");
    {
        let mut state = rig.session.state();
        let mut project = state.document.project().clone();
        project.next_id = u32::MAX - 1;
        project.check().expect("valid exhausted project");
        state.document = windfall_project::Document::new(project);
    }
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .import_midi(preview.token)
            .expect_err("no ids")
            .contains("run out of ids")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    rig.session.midi_discard(preview.token);
}

#[test]
fn an_older_slow_preview_cannot_replace_a_newer_review() {
    let rig = Rig::new();
    let path = fixture(&rig, 0);
    let old_path = path.clone();
    let hold = rig.session.hold("midi:read");
    let work = rig
        .session
        .background(move |session| session.midi_preview(&old_path, MidiImportOptions::default()));
    hold.wait();
    let newer = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("newer reviews");
    hold.release();
    assert!(work.join().expect("worker completed").is_err());
    rig.session.import_midi(newer.token).expect("newer imports");
}

#[test]
fn reviewed_import_appends_one_undo_step_and_keeps_project_file() {
    let rig = Rig::new();
    rig.session
        .project_save(Some(&rig.file("current.windfall")))
        .expect("saves");
    let before = rig.session.document_snapshot();
    let preview = rig
        .session
        .midi_preview(&fixture(&rig, 0), MidiImportOptions::default())
        .expect("reviews");
    assert_eq!(preview.notes, 1);
    assert_eq!(preview.channels, ["Imported lead"]);
    assert!(
        preview.adjustments.is_empty(),
        "tempo and markers are supported"
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let result = rig.session.import_midi(preview.token).expect("imports");
    let after = rig.session.document_snapshot();
    after.project.check().expect("valid");
    assert_eq!(after.path, before.path);
    assert_eq!(
        after.project.channels.len(),
        before.project.channels.len() + 1
    );
    assert_eq!(after.history.cursor, before.history.cursor + 1);
    assert_eq!(
        after
            .project
            .playlist
            .timeline
            .markers
            .iter()
            .map(|marker| (marker.tick, marker.name.as_str(), marker.kind))
            .collect::<Vec<_>>(),
        [(0, "Verse", windfall_project::MarkerKind::Named)]
    );
    assert!(result.patch.dirty);
    rig.session.undo().expect("undo");
    let mut restored = rig.project();
    restored.next_id = before.project.next_id;
    assert_eq!(restored, before.project);
    rig.session.redo().expect("redo");
    assert_eq!(rig.project(), after.project);
    assert!(rig.session.import_midi(preview.token).is_err());
}

#[test]
fn import_uses_the_reviewed_bytes_and_allocates_after_intervening_edits() {
    let rig = Rig::new();
    let path = fixture(&rig, 0);
    let preview = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("reviews");
    std::fs::write(path, b"not MIDI any more").expect("changes file");
    rig.session
        .dispatch(
            Command::AddPattern {
                name: Some("New pattern".to_owned()),
            },
            None,
        )
        .expect("edits");
    rig.session
        .import_midi(preview.token)
        .expect("appends reviewed plan");
    rig.project().check().expect("valid ids");
    assert_eq!(
        rig.project()
            .channels
            .last()
            .expect("imported channel")
            .name,
        "Imported lead"
    );
}

#[test]
fn cancelled_and_superseded_previews_cannot_import() {
    let rig = Rig::new();
    let path = fixture(&rig, 0);
    let first = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("reviews");
    let second = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("reviews again");
    assert!(rig.session.import_midi(first.token).is_err());
    rig.session.midi_discard(second.token);
    assert!(rig.session.import_midi(second.token).is_err());
}

#[test]
fn malformed_and_missing_files_leave_document_and_history_untouched() {
    let rig = Rig::new();
    let before = rig.session.document_snapshot();
    let path = rig.file("broken.mid");
    std::fs::write(&path, b"MThd\0\0").expect("writes bad file");
    assert!(
        rig.session
            .midi_preview(&path, MidiImportOptions::default())
            .is_err()
    );
    assert!(
        rig.session
            .midi_preview(&rig.file("missing.mid"), MidiImportOptions::default())
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn project_replacement_invalidates_prepared_and_in_flight_imports() {
    let rig = Rig::new();
    let path = fixture(&rig, 0);
    let preview = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("reviews");
    rig.session.project_new().expect("new project");
    assert!(rig.session.import_midi(preview.token).is_err());
    let hold = rig.session.hold("midi:read");
    let work = rig
        .session
        .background(move |session| session.midi_preview(&path, MidiImportOptions::default()));
    hold.wait();
    rig.session.project_new().expect("another project");
    let before = rig.session.document_snapshot();
    hold.release();
    assert!(work.join().expect("worker completed").is_err());
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn mapped_drums_load_through_the_existing_sample_pool() {
    let rig = Rig::new();
    let preview = rig
        .session
        .midi_preview(
            &fixture(&rig, 9),
            MidiImportOptions {
                factory_drums: true,
                ..Default::default()
            },
        )
        .expect("reviews drums");
    rig.session.import_midi(preview.token).expect("imports kit");
    let project = rig.project();
    let channel = project.channels.last().expect("drum channel");
    let sample = channel.source.sample().expect("a mapped sampler");
    rig.wait_until_loaded_or_failed(sample);
    assert!(rig.has_audio(sample));
}

#[test]
fn song_and_pattern_exports_write_real_midi_without_editing_the_document() {
    let rig = Rig::new();
    let preview = rig
        .session
        .midi_preview(&fixture(&rig, 0), MidiImportOptions::default())
        .expect("reviews");
    rig.session.import_midi(preview.token).expect("imports");
    let before = rig.session.document_snapshot();
    let path = rig
        .session
        .export_midi(&rig.file("song"), export_options(&rig, PlayMode::Song))
        .expect("song exports");
    assert!(path.ends_with(".mid"));
    let song = windfall_midi::read_file(path).expect("reads export");
    assert_eq!(song.note_count(), 1);
    assert_eq!(song.tempos.len(), 2);
    assert_eq!(
        song.tempos,
        [
            TempoChange::from_bpm(0, 120.0),
            TempoChange::from_bpm(960, 100.0)
        ]
    );
    assert_eq!(
        song.markers,
        [Marker {
            tick: 0,
            text: "Verse".into()
        }]
    );
    let mut options = export_options(&rig, PlayMode::Pattern);
    options.pattern = rig.project().patterns.last().expect("imported pattern").id;
    options.single_track = true;
    options.ppq = 480;
    let pattern_path = rig.file("pattern.MIDI");
    rig.session
        .export_midi(&pattern_path, options)
        .expect("pattern exports");
    let bytes = std::fs::read(pattern_path).expect("written");
    assert_eq!(&bytes[8..10], &[0, 0]);
    assert_eq!(&bytes[12..14], &480_u16.to_be_bytes());
    let pattern = windfall_midi::read(&bytes).expect("reads");
    assert_eq!(pattern.note_count(), 1);
    assert_eq!(
        pattern.markers,
        [Marker {
            tick: 0,
            text: "Verse".into()
        }]
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn reviewed_import_still_reports_unsupported_key_signature_without_false_marker_loss() {
    let rig = Rig::new();
    let path = fixture(&rig, 0);
    let mut song = windfall_midi::read_file(&path).expect("reads fixture");
    song.key_signatures.push(KeySignatureChange {
        tick: 0,
        sharps: 2,
        minor: false,
    });
    windfall_midi::write_file(&path, &song, &WriteOptions::default())
        .expect("writes unsupported metadata");
    let before = rig.session.document_snapshot();
    let preview = rig
        .session
        .midi_preview(&path, MidiImportOptions::default())
        .expect("reviews");
    assert!(
        preview
            .adjustments
            .iter()
            .any(|message| message.contains("key signature"))
    );
    assert!(
        !preview
            .adjustments
            .iter()
            .any(|message| message.contains("marker"))
    );
    assert_eq!(rig.session.document_snapshot(), before);
    rig.session
        .import_midi(preview.token)
        .expect("imports supported data");
    let after = rig.session.document_snapshot();
    assert_eq!(after.history.cursor, before.history.cursor + 1);
    assert_eq!(after.project.playlist.timeline.markers[0].name, "Verse");
    assert_eq!(after.project.playlist.timeline.markers[0].tick, 0);
}

#[test]
fn refused_and_failed_exports_preserve_existing_files_and_project() {
    let rig = Rig::new();
    let before = rig.session.document_snapshot();
    let path = rig.file("existing.mid");
    std::fs::write(&path, b"keep this").expect("writes original");
    let mut options = export_options(&rig, PlayMode::Pattern);
    options.ppq = 32768;
    assert!(rig.session.export_midi(&path, options).is_err());
    assert_eq!(std::fs::read(path).expect("still there"), b"keep this");
    assert!(
        rig.session
            .export_midi(&rig.file("audio.wav"), export_options(&rig, PlayMode::Song))
            .is_err()
    );
    std::fs::write(rig.file("blocked"), b"a file, not a directory").expect("blocks parent");
    assert!(
        rig.session
            .export_midi(
                &rig.file("blocked/export.mid"),
                export_options(&rig, PlayMode::Song)
            )
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
}
