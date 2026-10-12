//! Original shipped patches through the same preset reader/command as user files.
use windfall_project::effect_examples::{EFFECT_EXAMPLE_LICENSE, EFFECT_EXAMPLES};
use windfall_project::*;

#[test]
fn every_original_effect_example_loads_without_changing_its_authored_values() {
    assert_eq!(EFFECT_EXAMPLE_LICENSE, "CC0-1.0");
    assert_eq!(EFFECT_EXAMPLES.len(), 17);
    let mut names = std::collections::HashSet::new();
    let mut kinds = std::collections::HashSet::new();
    for example in EFFECT_EXAMPLES {
        assert!(names.insert(example.name));
        assert!(kinds.insert(example.kind));
        assert!(example.file_name.ends_with(".wfpreset"));
        let raw: Preset = serde_json::from_slice(example.bytes).unwrap();
        let (loaded, _) = example.load().unwrap();
        assert_eq!(loaded, raw, "reader altered {}", example.name);
        assert_eq!(loaded.name, example.name);
        assert_eq!(loaded.kind, PresetKind::Plugin);
        assert!(loaded.assets.is_empty());
        let plugin = loaded.plugin.as_ref().unwrap();
        let PluginPresetState::Effect { params, impulse } = &plugin.state else {
            panic!("not an effect: {}", example.name);
        };
        assert!(impulse.is_none());
        assert_eq!(params.kind(), example.kind);
        assert_eq!(params.sanitized(), *params, "{}", example.name);
        assert_ne!(*params, example.kind.default_params(), "{}", example.name);
        let (reopened, notes) =
            Preset::from_json(loaded.to_json().unwrap().as_bytes()).unwrap();
        assert_eq!(reopened, loaded);
        assert!(notes.is_empty(), "{}: {notes:?}", example.name);
    }
}

#[test]
fn every_original_effect_example_is_one_undo_step_and_survives_disk_save_reopen() {
    let directory = tempfile::tempdir().unwrap();
    for example in EFFECT_EXAMPLES {
        let (preset, _) = example.load().unwrap();
        let mut doc = Document::new(Project::new(example.name));
        let effect = EffectId(
            doc.dispatch(
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: example.kind,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
        );
        let target = PluginTarget::Effect { effect };
        let before = PluginPreset::capture(doc.project(), target).unwrap();
        let history = doc.history().entries.len();
        let patch = preset.plugin.unwrap();
        doc.dispatch(
            Command::ApplyPluginPreset {
                target,
                expected: before.state.clone(),
                preset: patch.clone(),
            },
            None,
        )
        .unwrap();
        assert_eq!(doc.history().entries.len(), history + 1);
        assert_eq!(PluginPreset::capture(doc.project(), target).unwrap(), patch);
        doc.undo().unwrap();
        assert_eq!(PluginPreset::capture(doc.project(), target).unwrap(), before);
        doc.redo().unwrap();
        assert_eq!(PluginPreset::capture(doc.project(), target).unwrap(), patch);
        doc.project().check().unwrap();
        let path = directory.path().join(example.file_name).with_extension("windfall");
        file::save(doc.project(), &path).unwrap();
        let reopened = file::load(&path).unwrap();
        assert_eq!(&reopened, doc.project(), "{}", example.name);
        assert_eq!(PluginPreset::capture(&reopened, target).unwrap(), patch);

        // A stale load must leave the document and history untouched.
        let project = doc.project().clone();
        let history = doc.history().entries.len();
        assert!(
            doc.dispatch(
                Command::ApplyPluginPreset {
                    target,
                    expected: before.state,
                    preset: patch,
                },
                None,
            )
            .is_err()
        );
        assert_eq!(doc.project(), &project);
        assert_eq!(doc.history().entries.len(), history);
    }
}
