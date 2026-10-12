//! Plain project notes use the real document history and current disk format.
use windfall_project::notebook::{
    MAX_NOTEBOOK_BODY_BYTES, MAX_NOTEBOOK_PAGES, MAX_NOTEBOOK_TITLE_BYTES,
};
use windfall_project::{file, *};

fn page(title: &str, body: &str) -> NotebookPage {
    NotebookPage {
        title: title.into(),
        body: body.into(),
        ..Default::default()
    }
}

fn replace(doc: &mut Document, pages: Vec<NotebookPage>) {
    let before = doc.project().clone();
    let entries = doc.history().entries.len();
    let applied = doc
        .dispatch(
            Command::ReplaceNotebook {
                notebook: Notebook { pages },
            },
            None,
        )
        .unwrap();
    assert!(applied.touched.notebook);
    assert_eq!(doc.history().entries.len(), entries + 1);
    let edited = doc.project().clone();
    assert!(doc.undo().unwrap().notebook);
    assert_eq!(doc.project(), &before);
    assert!(doc.redo().unwrap().notebook);
    assert_eq!(doc.project(), &edited);
}

#[test]
fn notebook_page_edits_addition_removal_history_and_current_file_roundtrip() {
    let mut doc = Document::new(Project::new("Notebook acceptance"));
    let original = doc.project().clone();
    replace(
        &mut doc,
        vec![
            page("  Verse  ", "  Café\nFirst line  "),
            page("Mix", "Remember the quiet bridge"),
        ],
    );
    assert_eq!(doc.project().notebook.pages[0].title, "Verse");
    assert_eq!(doc.project().notebook.pages[0].body, "  Café\nFirst line  ");
    let mut pages = doc.project().notebook.pages.clone();
    pages[1].body.push_str("\nKeep <lyrics> as plain text.");
    replace(&mut doc, pages);
    let mut pages = doc.project().notebook.pages.clone();
    pages.push(page("Chorus", "Second voice: 日本語"));
    replace(&mut doc, pages);
    let mut pages = doc.project().notebook.pages.clone();
    pages.remove(0);
    replace(&mut doc, pages);
    assert_eq!(doc.project().notebook.pages.len(), 2);
    assert_eq!(doc.project().notebook.pages[0].title, "Mix");
    // Notebook commands must not modify settings, patterns, IDs or other song state.
    let mut without_notes = doc.project().clone();
    without_notes.notebook = Notebook::default();
    assert_eq!(without_notes, original);

    let encoded = file::to_json(doc.project()).unwrap();
    assert_eq!(file::from_json(&encoded).unwrap(), *doc.project());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.windfall");
    file::save(doc.project(), &path).unwrap();
    let restored = file::load(&path).unwrap();
    assert_eq!(restored, *doc.project());
    let mut reopened = Document::new(restored);
    assert!(reopened.history().entries.is_empty());
    assert!(!reopened.is_dirty());
    replace(&mut reopened, vec![]);
    file::save(reopened.project(), &path).unwrap();
    assert!(file::load(&path).unwrap().notebook.is_empty());
    reopened.undo().unwrap();
    file::save(reopened.project(), &path).unwrap();
    assert_eq!(file::load(&path).unwrap(), *doc.project());
}

#[test]
fn invalid_notebook_commands_and_files_preserve_document_redo_and_saved_project() {
    let mut doc = Document::new(Project::new("Conserved notebook"));
    replace(&mut doc, vec![page("Lyrics", "Existing song notes")]);
    replace(&mut doc, vec![page("Lyrics", "Temporary edit")]);
    doc.undo().unwrap();
    let before = doc.snapshot(None);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.windfall");
    file::save(doc.project(), &path).unwrap();
    let saved_bytes = std::fs::read(&path).unwrap();
    for notebook in [
        Notebook {
            pages: vec![NotebookPage::default(); MAX_NOTEBOOK_PAGES + 1],
        },
        Notebook {
            pages: vec![page(&"é".repeat(MAX_NOTEBOOK_TITLE_BYTES / 2 + 1), "valid")],
        },
        Notebook {
            pages: vec![page("valid", &"é".repeat(MAX_NOTEBOOK_BODY_BYTES / 2 + 1))],
        },
    ] {
        assert!(matches!(
            doc.dispatch(
                Command::ReplaceNotebook {
                    notebook: notebook.clone()
                },
                None
            ),
            Err(CommandError::Invalid(_))
        ));
        assert_eq!(doc.snapshot(None), before);
        let mut damaged = doc.project().clone();
        damaged.notebook = notebook;
        assert!(matches!(
            file::save(&damaged, &path),
            Err(file::SaveError::Invalid(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
        assert!(matches!(
            file::from_json(&file::to_json(&damaged).unwrap()),
            Err(file::LoadError::Invalid(_))
        ));
        assert_eq!(file::load(&path).unwrap(), *doc.project());
    }
    assert!(doc.redo().unwrap().notebook);
    assert_eq!(doc.project().notebook.pages[0].body, "Temporary edit");
}
