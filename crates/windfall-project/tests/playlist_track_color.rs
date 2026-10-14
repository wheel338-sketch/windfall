use windfall_project::{Command, Document, PlaylistTrackId, PlaylistTrackPatch, Project, file};

#[test]
fn playlist_track_color_loads_as_zero_and_patch_is_saved() {
    let mut doc = Document::new(Project::new("Track color"));
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
    assert_eq!(doc.project().playlist.tracks[0].color, 0);
    let legacy = serde_json::to_value(doc.project()).unwrap();
    assert!(legacy["playlist"]["tracks"][0].get("color").is_none());
    let loaded = file::from_json(&legacy.to_string()).unwrap();
    assert_eq!(loaded.playlist.tracks[0].color, 0);
    let mut doc = Document::new(loaded);

    for color in [0x12a594, 0] {
        let before = doc.project().clone();
        let applied = doc
            .dispatch(
                Command::UpdatePlaylistTrack {
                    id,
                    patch: PlaylistTrackPatch {
                        color: Some(color),
                        ..PlaylistTrackPatch::default()
                    },
                },
                None,
            )
            .unwrap();
        assert_eq!(
            windfall_core::english::english(&applied.label),
            "Color playlist track"
        );
        assert!(applied.touched.playlist);
        assert_eq!(doc.project().playlist.tracks[0].color, color);
        doc.project().check().unwrap();
        let after = doc.project().clone();
        doc.undo().unwrap();
        assert_eq!(*doc.project(), before);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), after);

        let saved = file::to_json(doc.project()).unwrap();
        assert_eq!(file::from_json(&saved).unwrap(), *doc.project());
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        if color == 0 {
            assert!(value["playlist"]["tracks"][0].get("color").is_none());
        } else {
            assert_eq!(value["playlist"]["tracks"][0]["color"], color);
        }
    }
}
