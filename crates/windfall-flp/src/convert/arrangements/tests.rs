//! Synthetic import projects are original test data, CC0-1.0.
use crate::convert::{ConvertOptions, convert};
use crate::model::{
    Arrangement, Channel, ChannelKind, FlpProject, LegacyPlaylistItem, Pattern, PlaylistItem,
    PlaylistSource, Plugin, TimeMarker, Track,
};
use windfall_project::{ChannelSource, Command, Document};

fn item(position: u32, track: u16) -> PlaylistItem {
    PlaylistItem {
        position,
        length: 384,
        track,
        group: 0,
        flags: 0,
        source: PlaylistSource::Pattern {
            pattern: 1,
            start: Some(0),
            end: None,
        },
        extra: None,
    }
}

fn source() -> FlpProject {
    FlpProject {
        patterns: vec![Pattern {
            iid: 1,
            length: Some(384),
            ..Default::default()
        }],
        arrangements: vec![
            Arrangement {
                index: 2,
                name: Some("Short version".into()),
                items: vec![item(384, 1), item(1152, 9)],
                markers: vec![TimeMarker {
                    position: 0,
                    name: Some("Alternate opening".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            Arrangement {
                index: 7,
                name: Some("Full version".into()),
                items: vec![item(384, 1), item(768, 1)],
                tracks: vec![Track {
                    iid: 2,
                    name: Some("Lead".into()),
                    enabled: true,
                    height: 1.0,
                    ..Default::default()
                }],
                markers: vec![
                    TimeMarker {
                        position: 0,
                        kind: 8,
                        numerator: Some(3),
                        denominator: Some(4),
                        ..Default::default()
                    },
                    TimeMarker {
                        position: 768,
                        name: Some("Chorus".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ],
        current_arrangement: Some(7),
        ..Default::default()
    }
}

#[test]
fn independent_arrangements_keep_distinct_clips_lanes_and_selected_timeline() {
    let source = source();
    let imported = convert(&source, &ConvertOptions::default());
    imported.project.check().unwrap();
    let playlist = &imported.project.playlist;
    let book = &playlist.arrangement_book;
    assert_eq!(
        book.arrangements
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["Short version", "Full version"]
    );
    assert_eq!(book.active, Some(book.arrangements[1].id));
    assert_eq!(playlist.clips.len(), 4);
    assert_eq!(book.arrangements[0].clips.len(), 2);
    assert_eq!(book.arrangements[1].clips.len(), 2);
    assert!(
        book.arrangements[0]
            .clips
            .iter()
            .all(|id| !book.arrangements[1].clips.contains(id))
    );
    assert_eq!(book.arrangements[0].tracks.len(), 2);
    assert_eq!(book.arrangements[1].tracks.len(), 1);
    let positions = |a: &windfall_project::arrangement::Arrangement| {
        a.clips
            .iter()
            .map(|id| playlist.clips.iter().find(|c| c.id == *id).unwrap().start)
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&book.arrangements[0]), [3840, 11520]);
    assert_eq!(positions(&book.arrangements[1]), [3840, 7680]);
    assert_eq!(playlist.timeline.markers[0].name, "Chorus");
    assert_eq!(playlist.timeline.meters[0].signature.numerator, 3);
    assert_eq!(book.timelines.len(), 2);
    assert_eq!(book.timelines[&book.arrangements[0].id].markers[0].name, "Alternate opening");
    assert_eq!(imported, convert(&source, &ConvertOptions::default()));
    let mut document = Document::new(imported.project.clone());
    document
        .dispatch(
            Command::SwitchArrangement {
                id: book.arrangements[0].id,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        document.project().playlist.arrangement_book.active,
        Some(book.arrangements[0].id)
    );
    assert_eq!(document.project().playlist.clips, playlist.clips);
    assert_eq!(document.project().playlist.timeline.markers[0].name, "Alternate opening");
    assert!(document.project().playlist.timeline.meters.is_empty());
    document.undo().unwrap();
    assert_eq!(document.project().playlist.timeline, playlist.timeline);
    document.redo().unwrap();
    let json = windfall_project::file::to_json(document.project()).unwrap();
    let reopened: windfall_project::Project = windfall_project::file::from_json(&json).unwrap();
    reopened.check().unwrap();
    assert_eq!(reopened, *document.project());
}

#[test]
fn one_arrangement_keeps_legacy_book_and_allocator() {
    let mut source = source();
    source.arrangements.remove(0);
    let named = convert(&source, &ConvertOptions::default());
    source.arrangements[0].name = None;
    let unnamed = convert(&source, &ConvertOptions::default());
    assert!(named.project.playlist.arrangement_book.is_empty());
    assert_eq!(named, unnamed);
}

#[test]
fn identical_and_legacy_placements_are_owned_independently() {
    let mut source = source();
    for arrangement in &mut source.arrangements {
        arrangement.items = vec![item(384, 1), item(384, 1)];
        arrangement.legacy_items = vec![LegacyPlaylistItem { bar: 2, pattern: 1 }];
    }
    let imported = convert(&source, &ConvertOptions::default());
    assert_eq!(imported.project.playlist.clips.len(), 6);
    assert!(
        imported
            .project
            .playlist
            .arrangement_book
            .arrangements
            .iter()
            .all(|a| a.clips.len() == 3)
    );
    imported.project.check().unwrap();
}

#[test]
fn name_limits_do_not_lose_entries_and_unsupported_native_state_stays_silent() {
    let mut source = source();
    source.arrangements[0].name = Some("\0  ".into());
    source.arrangements[1].name = Some("é".repeat(129));
    source.channels.push(Channel {
        iid: 0,
        kind: ChannelKind::Generator,
        plugin: Some(Plugin {
            internal_name: "Original unsupported generator".into(),
            state: vec![1, 2, 3],
            ..Default::default()
        }),
        ..Default::default()
    });
    let imported = convert(&source, &ConvertOptions::default());
    let book = &imported.project.playlist.arrangement_book;
    assert_eq!(book.arrangements.len(), 2);
    assert_eq!(book.arrangements[0].name, "Arrangement 3");
    assert_eq!(book.arrangements[1].name, "é".repeat(128));
    assert!(
        matches!(&imported.project.channels[0].source, ChannelSource::Sampler(sampler) if sampler.sample.is_none())
    );
    assert_eq!(imported.plugins[0].state, [1, 2, 3]);
    assert_eq!(imported.project.retained_plugins[0].state, [1, 2, 3]);
    assert!(format!("{:?}", imported.report).contains("Original unsupported generator"));
    imported.project.check().unwrap();
}
