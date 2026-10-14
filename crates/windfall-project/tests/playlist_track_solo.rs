use windfall_project::{Command, Document, PlaylistTrackId, PlaylistTrackPatch, Project, file};

#[test]
fn playlist_track_solo_defaults_false_and_patch_round_trips_with_history() {
    let mut doc = Document::new(Project::new("Track solo"));
    let added = doc
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap();
    let id = PlaylistTrackId(added.created[0]);
    assert!(!doc.project().playlist.tracks[0].solo);
    let legacy = serde_json::to_value(doc.project()).unwrap();
    assert!(legacy["playlist"]["tracks"][0].get("solo").is_none());
    let loaded = file::from_json(&legacy.to_string()).unwrap();
    assert!(!loaded.playlist.tracks[0].solo);
    let mut doc = Document::new(loaded);
    // Solo never overwrites a track's independently stored mute.
    doc.dispatch(
        Command::UpdatePlaylistTrack {
            id,
            patch: PlaylistTrackPatch {
                muted: Some(true),
                ..Default::default()
            },
        },
        None,
    )
    .unwrap();

    for (solo, label) in [
        (true, "Solo playlist track"),
        (false, "Unsolo playlist track"),
    ] {
        let before = doc.project().clone();
        let applied = doc
            .dispatch(
                Command::UpdatePlaylistTrack {
                    id,
                    patch: PlaylistTrackPatch {
                        solo: Some(solo),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
        assert_eq!(windfall_core::english::english(&applied.label), label);
        assert!(applied.touched.playlist);
        assert_eq!(doc.project().playlist.tracks[0].solo, solo);
        assert!(doc.project().playlist.tracks[0].muted);
        doc.project().check().unwrap();
        let after = doc.project().clone();
        doc.undo().unwrap();
        assert_eq!(*doc.project(), before);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), after);

        let saved = file::to_json(doc.project()).unwrap();
        assert_eq!(file::from_json(&saved).unwrap(), *doc.project());
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        if solo {
            assert_eq!(value["playlist"]["tracks"][0]["solo"], true);
        } else {
            assert!(value["playlist"]["tracks"][0].get("solo").is_none());
        }
    }
}
