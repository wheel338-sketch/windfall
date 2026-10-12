use windfall_project::arrangement::{Arrangement, ClipGroup, TrackKind};
use windfall_project::*;

fn edit(doc: &mut Document, command: Command) -> Vec<u32> {
    let before = doc.project().clone();
    let history = doc.history().cursor;
    let applied = doc.dispatch(command, None).unwrap();
    let after = doc.project().clone();
    doc.project().check().unwrap();
    assert_eq!(
        doc.history().cursor,
        history + u32::from(!applied.touched.is_empty())
    );
    if !applied.touched.is_empty() {
        doc.undo().unwrap();
        let mut expected = before;
        expected.next_id = after.next_id;
        assert_eq!(*doc.project(), expected);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), after);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("arrangement.windfall");
        file::save(doc.project(), &path).unwrap();
        assert_eq!(file::load(&path).unwrap(), after);
    }
    applied.created
}

fn fixture() -> (Document, PlaylistTrackId, Vec<ClipId>) {
    let mut doc = Document::new(Project::new("Arrangements"));
    let track = PlaylistTrackId(
        edit(
            &mut doc,
            Command::AddPlaylistTrack {
                name: Some("Keys".into()),
                index: None,
            },
        )[0],
    );
    let clips = edit(
        &mut doc,
        Command::AddClips {
            clips: (0..2)
                .map(|index| ClipInit {
                    track,
                    start: index * 3840,
                    length: Some(3840),
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern {
                        pattern: PatternId(1),
                    },
                })
                .collect(),
        },
    )
    .into_iter()
    .map(ClipId)
    .collect();
    (doc, track, clips)
}

#[test]
fn additions_join_only_active_arrangement_and_survive_undo_redo() {
    let (mut doc, track, _) = fixture();
    let active = edit(
        &mut doc,
        Command::AddArrangement {
            name: "Active".into(),
            clips: vec![],
            tracks: vec![],
        },
    )[0];
    let alternate = edit(
        &mut doc,
        Command::AddArrangement {
            name: "Alternate".into(),
            clips: vec![],
            tracks: vec![],
        },
    )[0];
    edit(&mut doc, Command::SwitchArrangement { id: active });
    let new_track = PlaylistTrackId(
        edit(
            &mut doc,
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
        )[0],
    );
    assert_eq!(
        doc.project().playlist.arrangement_book.arrangements[0].tracks,
        vec![new_track]
    );
    let new_clips: Vec<_> = edit(
        &mut doc,
        Command::AddClips {
            clips: [track, new_track, track]
                .into_iter()
                .enumerate()
                .map(|(index, track)| ClipInit {
                    track,
                    start: index as u32 * 3840,
                    length: Some(3840),
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern {
                        pattern: PatternId(1),
                    },
                })
                .collect(),
        },
    )
    .into_iter()
    .map(ClipId)
    .collect();
    let book = &doc.project().playlist.arrangement_book;
    assert_eq!(book.arrangements[0].clips, new_clips);
    assert_eq!(book.arrangements[0].tracks, vec![new_track, track]);
    assert_eq!(book.arrangements[1].id, alternate);
    assert!(book.arrangements[1].clips.is_empty());
    assert!(book.arrangements[1].tracks.is_empty());
    let before = doc.snapshot(None);
    assert!(
        doc.dispatch(
            Command::Batch {
                label: None,
                commands: vec![
                    Command::AddPlaylistTrack {
                        name: None,
                        index: None
                    },
                    Command::AddClips {
                        clips: vec![ClipInit {
                            track: PlaylistTrackId(999),
                            start: 0,
                            length: Some(3840),
                            offset: None,
                            muted: None,
                            content: ClipContent::Pattern {
                                pattern: PatternId(1)
                            },
                        }]
                    },
                ]
            },
            None
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(None), before);
}

#[test]
fn arrangement_commands_are_single_undo_edits_and_survive_save_reload() {
    let (mut doc, track, clips) = fixture();
    let first = edit(
        &mut doc,
        Command::AddArrangement {
            name: "Verse".into(),
            clips: clips.clone(),
            tracks: vec![track],
        },
    )[0];
    let second = edit(
        &mut doc,
        Command::AddArrangement {
            name: "Chorus".into(),
            clips: vec![],
            tracks: vec![],
        },
    )[0];
    edit(
        &mut doc,
        Command::RenameArrangement {
            id: first,
            name: "Intro".into(),
        },
    );
    edit(
        &mut doc,
        Command::SetArrangementReferences {
            id: second,
            clips: clips.iter().rev().copied().collect(),
            tracks: vec![track],
        },
    );
    let pool = doc.project().playlist.clips.clone();
    edit(&mut doc, Command::SwitchArrangement { id: second });
    assert_eq!(doc.project().playlist.clips, pool);
    assert_eq!(doc.project().playlist.arrangement_book.active, Some(second));
    edit(&mut doc, Command::RemoveArrangement { id: first });
    let parent = edit(
        &mut doc,
        Command::AddTrackGroup {
            name: "Band".into(),
            parent: None,
        },
    )[0];
    let child = edit(
        &mut doc,
        Command::AddTrackGroup {
            name: "Keys".into(),
            parent: Some(parent),
        },
    )[0];
    edit(
        &mut doc,
        Command::RenameTrackGroup {
            id: child,
            name: "Piano".into(),
        },
    );
    edit(
        &mut doc,
        Command::MoveTrackGroup {
            id: child,
            parent: None,
        },
    );
    edit(
        &mut doc,
        Command::MoveTrackGroup {
            id: child,
            parent: Some(parent),
        },
    );
    edit(
        &mut doc,
        Command::MoveTrackToGroup {
            track,
            parent: Some(child),
        },
    );
    edit(&mut doc, Command::RemoveTrackGroup { id: child });
    assert_eq!(
        doc.project().playlist.arrangement_book.track_parents[&track],
        parent
    );
    edit(
        &mut doc,
        Command::MoveTrackToGroup {
            track,
            parent: None,
        },
    );
    edit(&mut doc, Command::RemoveTrackGroup { id: parent });
    let group = edit(
        &mut doc,
        Command::AddClipGroup {
            clips: clips.clone(),
        },
    )[0];
    edit(&mut doc, Command::RemoveClipGroup { id: group });
    let channel = ChannelId(
        edit(
            &mut doc,
            serde_json::from_value(serde_json::json!({"type":"addChannel", "name":"Piano"}))
                .unwrap(),
        )[0],
    );
    edit(
        &mut doc,
        Command::LinkTrack {
            track,
            kind: Some(TrackKind::Instrument { channel }),
        },
    );
    edit(&mut doc, Command::LinkTrack { track, kind: None });
    let sample = SampleId(
        edit(
            &mut doc,
            Command::AddSample {
                name: "Vocal".into(),
                path: SamplePath::Factory("vocal.wav".into()),
            },
        )[0],
    );
    edit(
        &mut doc,
        Command::LinkTrack {
            track,
            kind: Some(TrackKind::Audio { source: sample }),
        },
    );
}

#[test]
fn failures_preserve_book_history_dirty_state_and_allocator() {
    let (mut doc, track, clips) = fixture();
    let id = edit(
        &mut doc,
        Command::AddArrangement {
            name: "A".into(),
            clips: clips.clone(),
            tracks: vec![track],
        },
    )[0];
    for command in [
        Command::AddArrangement {
            name: " ".into(),
            clips: vec![],
            tracks: vec![],
        },
        Command::AddArrangement {
            name: "Missing clip".into(),
            clips: vec![ClipId(999)],
            tracks: vec![track],
        },
        Command::AddArrangement {
            name: "Missing track".into(),
            clips: vec![],
            tracks: vec![PlaylistTrackId(999)],
        },
        Command::RemoveArrangement { id },
        Command::SwitchArrangement { id: 999 },
        Command::SetArrangementReferences {
            id,
            clips: vec![ClipId(999)],
            tracks: vec![],
        },
        Command::MoveTrackToGroup {
            track: PlaylistTrackId(999),
            parent: None,
        },
        Command::LinkTrack {
            track,
            kind: Some(TrackKind::Instrument {
                channel: ChannelId(999),
            }),
        },
        Command::LinkTrack {
            track,
            kind: Some(TrackKind::Audio {
                source: SampleId(999),
            }),
        },
        Command::AddClipGroup {
            clips: vec![clips[0], ClipId(999)],
        },
    ] {
        let before = doc.snapshot(None);
        assert!(doc.dispatch(command, None).is_err());
        assert_eq!(doc.snapshot(None), before);
    }
    let retired = edit(
        &mut doc,
        Command::AddTrackGroup {
            name: "Undo me".into(),
            parent: None,
        },
    )[0];
    doc.undo().unwrap();
    let fresh = doc
        .dispatch(
            Command::AddTrackGroup {
                name: "Fresh".into(),
                parent: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    assert!(fresh > retired);
}

#[test]
fn removing_material_prunes_layout_and_group_references_with_undo() {
    let (mut doc, track, clips) = fixture();
    edit(
        &mut doc,
        Command::AddArrangement {
            name: "Layout".into(),
            clips: clips.clone(),
            tracks: vec![track],
        },
    );
    edit(
        &mut doc,
        Command::AddClipGroup {
            clips: clips.clone(),
        },
    );
    let group = edit(
        &mut doc,
        Command::AddTrackGroup {
            name: "Group".into(),
            parent: None,
        },
    )[0];
    edit(
        &mut doc,
        Command::MoveTrackToGroup {
            track,
            parent: Some(group),
        },
    );
    let channel = ChannelId(
        edit(
            &mut doc,
            serde_json::from_value(serde_json::json!({"type":"addChannel", "name":"Linked"}))
                .unwrap(),
        )[0],
    );
    edit(
        &mut doc,
        Command::LinkTrack {
            track,
            kind: Some(TrackKind::Instrument { channel }),
        },
    );
    edit(
        &mut doc,
        Command::RemoveClips {
            clips: vec![clips[0]],
        },
    );
    assert_eq!(
        doc.project().playlist.arrangement_book.arrangements[0].clips,
        vec![clips[1]]
    );
    assert!(
        doc.project()
            .playlist
            .arrangement_book
            .clip_groups
            .is_empty()
    );
    edit(&mut doc, Command::RemoveChannel { id: channel });
    assert!(
        doc.project()
            .playlist
            .arrangement_book
            .linked_tracks
            .is_empty()
    );
    edit(&mut doc, Command::RemovePlaylistTrack { id: track });
    let book = &doc.project().playlist.arrangement_book;
    assert!(book.arrangements[0].clips.is_empty());
    assert!(book.arrangements[0].tracks.is_empty());
    assert!(book.track_parents.is_empty());
    assert_eq!(book.track_groups.len(), 1);
}

#[test]
fn load_checks_all_book_references_and_defaults_old_projects() {
    let (doc, track, clips) = fixture();
    let legacy = file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
    assert!(legacy.playlist.arrangement_book.is_empty());
    let mut project = legacy;
    project.next_id += 1;
    let id = project.next_id - 1;
    let books = [
        ArrangementBook {
            arrangements: vec![Arrangement {
                id,
                name: "A".into(),
                clips: vec![ClipId(999)],
                tracks: vec![],
            }],
            active: Some(id),
            ..Default::default()
        },
        ArrangementBook {
            arrangements: vec![Arrangement {
                id,
                name: "A".into(),
                clips: clips.clone(),
                tracks: vec![PlaylistTrackId(999)],
            }],
            active: Some(id),
            ..Default::default()
        },
        ArrangementBook {
            clip_groups: vec![ClipGroup {
                id,
                clips: vec![clips[0], ClipId(999)],
            }],
            ..Default::default()
        },
        ArrangementBook {
            linked_tracks: [(
                track,
                TrackKind::Instrument {
                    channel: ChannelId(999),
                },
            )]
            .into(),
            ..Default::default()
        },
        ArrangementBook {
            linked_tracks: [(
                track,
                TrackKind::Audio {
                    source: SampleId(999),
                },
            )]
            .into(),
            ..Default::default()
        },
        ArrangementBook {
            linked_tracks: [(
                PlaylistTrackId(999),
                TrackKind::Audio {
                    source: SampleId(999),
                },
            )]
            .into(),
            ..Default::default()
        },
    ];
    for book in books {
        project.playlist.arrangement_book = book;
        assert!(project.check().is_err());
        assert!(file::from_json(&file::to_json(&project).unwrap()).is_err());
    }
    project.playlist.arrangement_book = ArrangementBook {
        active: Some(id),
        ..Default::default()
    };
    assert!(project.check().is_err());
}

#[test]
fn make_unique_copies_sources_and_redirects_only_chosen_clip() {
    let (mut doc, track, clips) = fixture();
    let channel = edit(
        &mut doc,
        serde_json::from_value(serde_json::json!({"type":"addChannel", "name":"Piano"})).unwrap(),
    )[0];
    edit(&mut doc, serde_json::from_value(serde_json::json!({"type":"addNotes", "pattern":1, "channel":channel, "notes":[{"start":0,"length":240,"key":60}]})).unwrap());
    let original = doc.project().patterns[0].clone();
    let other = doc.project().playlist.clips[1].clone();
    let fresh = PatternId(edit(&mut doc, Command::MakeUnique { clip: clips[0] })[0]);
    assert_ne!(fresh, original.id);
    assert_ne!(
        doc.project().pattern(fresh).unwrap().lanes[0].notes[0].id,
        original.lanes[0].notes[0].id
    );
    assert_eq!(
        doc.project().pattern(fresh).unwrap().lanes[0].channel,
        ChannelId(channel)
    );
    assert_eq!(doc.project().pattern(original.id).unwrap(), &original);
    assert_eq!(doc.project().playlist.clips[1], other);
    let sample = SampleId(
        edit(
            &mut doc,
            Command::AddSample {
                name: "Vocal".into(),
                path: SamplePath::Factory("vocal.wav".into()),
            },
        )[0],
    );
    let audio = edit(&mut doc, serde_json::from_value(serde_json::json!({"type":"addClips", "clips":[{"track":track.0,"start":8000,"length":1200,"offset":17,"content":{"type":"audio","sample":sample.0,"mixerTrack":0,"gain":0.7,"pan":0.2,"fadeIn":33,"fadeOut":45,"reverse":true,"pitch":2,"stretch":{"mode":"spectral","ratio":1.5,"quality":"high","formants":true}}}]})).unwrap())[0];
    let before = doc
        .project()
        .playlist
        .clips
        .iter()
        .find(|clip| clip.id.0 == audio)
        .unwrap()
        .clone();
    let copy = SampleId(
        edit(
            &mut doc,
            Command::MakeUnique {
                clip: ClipId(audio),
            },
        )[0],
    );
    assert_ne!(sample, copy);
    assert_eq!(
        doc.project().sample(sample).unwrap().path,
        doc.project().sample(copy).unwrap().path
    );
    let mut expected = before;
    if let ClipContent::Audio { sample, .. } = &mut expected.content {
        *sample = copy;
    }
    assert_eq!(
        doc.project()
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id.0 == audio)
            .unwrap(),
        &expected
    );
}

#[test]
fn independent_timeline_capture_switch_copy_delete_and_legacy_load_are_checked_and_undoable() {
    let (mut doc, _, _) = fixture();
    let first = edit(
        &mut doc,
        Command::AddArrangement {
            name: "Verse".into(),
            clips: vec![],
            tracks: vec![],
        },
    )[0];
    edit(
        &mut doc,
        Command::AddTimelineMarker {
            tick: 960,
            name: "Verse".into(),
            kind: MarkerKind::Named,
        },
    );
    edit(&mut doc, Command::CaptureArrangementTimeline { id: first });
    let second = edit(
        &mut doc,
        Command::AddArrangementLayout {
            name: "Chorus".into(),
            copy: false,
        },
    )[0];
    assert!(doc.project().playlist.timeline.is_empty());
    edit(
        &mut doc,
        Command::AddMeterChange {
            tick: 0,
            signature: TimeSignature {
                numerator: 3,
                denominator: 4,
            },
        },
    );
    edit(
        &mut doc,
        Command::AddTimelineMarker {
            tick: 1920,
            name: "Chorus".into(),
            kind: MarkerKind::Named,
        },
    );
    let chorus = doc.project().playlist.timeline.clone();
    edit(&mut doc, Command::SwitchArrangement { id: first });
    assert_eq!(doc.project().playlist.timeline.markers[0].name, "Verse");
    edit(&mut doc, Command::SwitchArrangement { id: second });
    assert_eq!(doc.project().playlist.timeline, chorus);
    let copied = edit(
        &mut doc,
        Command::AddArrangementLayout {
            name: "Chorus copy".into(),
            copy: true,
        },
    )[0];
    assert_eq!(doc.project().playlist.timeline.markers[0].name, "Chorus");
    assert_ne!(
        doc.project().playlist.timeline.markers[0].id,
        chorus.markers[0].id
    );
    edit(&mut doc, Command::RemoveArrangement { id: copied });
    assert_eq!(doc.project().playlist.timeline, chorus);
    let before = doc.project().clone();
    let history = doc.history().cursor;
    assert!(
        doc.dispatch(Command::CaptureArrangementTimeline { id: first }, None)
            .is_err()
    );
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history().cursor, history);
    // A v1 document without the added field keeps the historical shared timeline.
    let mut legacy: serde_json::Value =
        serde_json::from_str(&file::to_json(doc.project()).unwrap()).unwrap();
    legacy["playlist"]["arrangementBook"]
        .as_object_mut()
        .unwrap()
        .remove("timelines");
    let loaded = file::from_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
    assert!(loaded.playlist.arrangement_book.timelines.is_empty());
    let mut legacy_doc = Document::new(loaded);
    legacy_doc
        .dispatch(Command::SwitchArrangement { id: first }, None)
        .unwrap();
    assert_eq!(legacy_doc.project().playlist.timeline, chorus);
}
