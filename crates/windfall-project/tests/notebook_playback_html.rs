use windfall_project::{Command, Document, MAX_SONG_TICKS, Notebook, NotebookPage, Project};

#[test]
fn notebook_cues_html_history_and_legacy_json_roundtrip() {
    let plain: NotebookPage = serde_json::from_str(r#"{"title":"Old","body":"Notes"}"#).unwrap();
    assert!(!plain.html);
    assert_eq!(plain.start_tick, None);
    assert_eq!(
        serde_json::to_string(&plain).unwrap(),
        r#"{"title":"Old","body":"Notes"}"#
    );
    let mut document = Document::new(Project::new("Song"));
    let notebook = Notebook {
        pages: vec![NotebookPage {
            title: "Verse".into(),
            body: "<strong>Lyrics</strong><script>inert project data</script>".into(),
            html: true,
            start_tick: Some(3840),
        }],
    };
    let applied = document
        .dispatch(
            Command::ReplaceNotebook {
                notebook: notebook.clone(),
            },
            None,
        )
        .unwrap();
    assert!(applied.touched.notebook);
    assert_eq!(
        document.patch(&applied.touched).notebook,
        Some(notebook.clone())
    );
    let restored: Project =
        serde_json::from_str(&serde_json::to_string(document.project()).unwrap()).unwrap();
    assert_eq!(restored.notebook, notebook);
    assert!(restored.check().is_ok());
    document.undo().unwrap();
    assert!(document.project().notebook.is_empty());
    document.redo().unwrap();
    assert_eq!(document.project().notebook, notebook);
}

#[test]
fn notebook_out_of_range_cue_is_refused_atomically_and_on_load() {
    let mut document = Document::new(Project::new("Song"));
    let before = document.snapshot(None);
    let notebook = Notebook {
        pages: vec![NotebookPage {
            start_tick: Some(MAX_SONG_TICKS + 1),
            ..Default::default()
        }],
    };
    assert!(
        document
            .dispatch(
                Command::ReplaceNotebook {
                    notebook: notebook.clone()
                },
                None
            )
            .is_err()
    );
    assert_eq!(document.snapshot(None), before);
    let mut invalid = document.project().clone();
    invalid.notebook = notebook;
    assert!(invalid.check().is_err());
}
