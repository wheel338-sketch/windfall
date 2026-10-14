//! Mixer track presets through the session: capture, real library and
//! single files, apply, history, guards and a hosted plugin that is not
//! installed (`win-mixer-track-presets`).
//!
//! Everything here goes through the calls the Tauri commands make. The
//! hosted plugin in the last case is deliberately one this machine does not
//! have: what is checked is that its binding and opaque state survive, not
//! that it sounds. Live state captured from a running native plugin is the
//! business of the plugin runtime's own tests.
use super::Rig;
use std::fs;
use windfall_dsp::EffectKind;
use windfall_project::{
    Command, EffectId, MixerTrack, MixerTrackPatch, MixerTrackPreset, PluginBinding, PluginTarget,
    TrackId,
};

fn add_track(rig: &Rig, name: &str) -> TrackId {
    TrackId(
        rig.session
            .dispatch(
                Command::AddMixerTrack {
                    name: Some(name.into()),
                },
                None,
            )
            .unwrap()
            .created[0],
    )
}

fn add_effect(rig: &Rig, track: TrackId, kind: EffectKind) -> EffectId {
    EffectId(
        rig.session
            .dispatch(
                Command::AddEffect {
                    track,
                    kind,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    )
}

fn track(rig: &Rig, id: TrackId) -> MixerTrack {
    rig.project()
        .mixer
        .track(id)
        .expect("the track exists")
        .clone()
}

/// Captures the way the panel does: with the document's current numbers.
fn capture(rig: &Rig, id: TrackId) -> Result<MixerTrackPreset, String> {
    let state = rig.session.timeline_state();
    rig.session
        .mixer_preset_capture(id, state.generation, state.revision)
}

fn apply(
    rig: &Rig,
    id: TrackId,
    preset: &MixerTrackPreset,
    name_color: bool,
) -> Result<(), String> {
    rig.session
        .dispatch(
            Command::ApplyMixerTrackPreset {
                id,
                expected: track(rig, id),
                preset: preset.clone(),
                name_color,
            },
            None,
        )
        .map(|_| ())
}

/// A source with two effects and settings worth restoring, and an empty
/// destination with routing of its own.
fn fixture() -> (Rig, TrackId, TrackId) {
    let rig = Rig::new();
    let source = add_track(&rig, "QA Source");
    let destination = add_track(&rig, "QA Destination");
    add_effect(&rig, source, EffectKind::Eq);
    add_effect(&rig, source, EffectKind::Compressor);
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: source,
                patch: MixerTrackPatch {
                    volume: Some(0.5),
                    pan: Some(-0.25),
                    latency_offset_ms: Some(3.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::SetSend {
                from: destination,
                to: source,
                gain: Some(0.5),
            },
            None,
        )
        .unwrap();
    (rig, source, destination)
}

#[test]
fn qa_library_saves_lists_reads_applies_and_deletes_real_files() {
    let (rig, source, destination) = fixture();
    let folder = rig.session.mixer_preset_library_dir();
    // Nothing saved yet: an empty library, not an error, and no folder made.
    let empty = rig.session.mixer_preset_library().unwrap();
    assert!(empty.entries.is_empty() && !empty.truncated);
    assert!(!folder.exists());

    let mut preset = capture(&rig, source).unwrap();
    preset.name = "Drums: \"tight\"".into();
    let saved = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    assert_eq!(saved.file, "Drums_ _tight_.wfmixer");
    assert_eq!(saved.name, "Drums: \"tight\"");
    assert_eq!(saved.effects, 2);
    assert!(saved.plugins.is_empty());
    assert_eq!(saved.error, None);
    assert!(saved.bytes > 0 && saved.modified.is_some());
    assert!(folder.join(&saved.file).is_file());

    // The same name again takes the next number: nothing is overwritten.
    let again = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    assert_eq!(again.file, "Drums_ _tight_ 2.wfmixer");
    let third = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    assert_eq!(third.file, "Drums_ _tight_ 3.wfmixer");
    let listed = rig.session.mixer_preset_library().unwrap();
    assert_eq!(listed.entries.len(), 3);
    assert!(
        listed
            .entries
            .iter()
            .all(|entry| entry.error.is_none() && entry.effects == 2)
    );
    assert_eq!(listed.folder, crate::paths::display(&folder));
    // No temporary or claim files are left beside them.
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 3);

    // What is read back is what was saved.
    assert_eq!(
        rig.session
            .mixer_preset_library_read(saved.file.clone())
            .unwrap(),
        preset
    );

    // Apply to the destination: fresh effect ids, the source's settings,
    // the destination's own name and routing.
    let before = rig.project();
    let read = rig
        .session
        .mixer_preset_library_read(again.file.clone())
        .unwrap();
    apply(&rig, destination, &read, false).unwrap();
    let loaded = track(&rig, destination);
    let source_now = track(&rig, source);
    assert_eq!(loaded.effects.len(), 2);
    for (slot, original) in loaded.effects.iter().zip(&source_now.effects) {
        assert_ne!(slot.id, original.id);
        assert_eq!(slot.params, original.params);
        assert_eq!((slot.enabled, slot.mix), (original.enabled, original.mix));
    }
    assert_eq!((loaded.volume, loaded.pan), (0.5, -0.25));
    assert_eq!(loaded.latency_offset_ms, 3.0);
    assert_eq!(loaded.name, "QA Destination");
    assert_eq!(loaded.sends, track(&rig, destination).sends);
    assert_eq!(loaded.sends.len(), 1);
    assert_eq!(source_now, *before.mixer.track(source).unwrap());

    // One undo step, and redo.
    assert!(rig.session.undo().is_some());
    let mut undone = rig.project();
    undone.next_id = before.next_id;
    assert_eq!(undone, before);
    assert!(rig.session.redo().is_some());
    assert_eq!(track(&rig, destination), loaded);

    // With the option, the preset's name and color come too.
    apply(&rig, destination, &read, true).unwrap();
    assert_eq!(track(&rig, destination).name, "Drums: \"tight\"");

    // Delete one; the others stay. Deleting it again says it is gone.
    rig.session
        .mixer_preset_library_delete(again.file.clone())
        .unwrap();
    let left = rig.session.mixer_preset_library().unwrap();
    let files: Vec<&str> = left
        .entries
        .iter()
        .map(|entry| entry.file.as_str())
        .collect();
    assert_eq!(
        files,
        ["Drums_ _tight_ 3.wfmixer", "Drums_ _tight_.wfmixer"]
    );
    let gone = rig
        .session
        .mixer_preset_library_delete(again.file.clone())
        .unwrap_err();
    assert!(gone.contains("no longer in the library"), "{gone}");
    assert!(rig.session.mixer_preset_library_read(again.file).is_err());

    // The library belongs to the user, not the project: another run of the
    // app on the same settings finds it.
    let rig = rig.restart();
    assert_eq!(rig.session.mixer_preset_library().unwrap().entries.len(), 2);
}

#[test]
fn qa_library_lists_damaged_files_with_their_reason_and_names_nothing_outside_itself() {
    let (rig, source, _) = fixture();
    let preset = capture(&rig, source).unwrap();
    let good = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    let folder = rig.session.mixer_preset_library_dir();
    let text = serde_json::to_string(&preset).unwrap();

    fs::write(folder.join("Cut short.wfmixer"), &text[..text.len() / 2]).unwrap();
    fs::write(folder.join("Not json.wfmixer"), b"\x00\x01 not a preset").unwrap();
    let mut newer = preset.clone();
    newer.version = 2;
    newer.name = "From the future".into();
    fs::write(
        folder.join("Newer.wfmixer"),
        serde_json::to_vec(&newer).unwrap(),
    )
    .unwrap();
    let mut bad = preset.clone();
    bad.volume = 99.0;
    bad.name = "Out of range".into();
    fs::write(
        folder.join("Out of range.WFMIXER"),
        serde_json::to_vec(&bad).unwrap(),
    )
    .unwrap();
    // Not presets at all: ignored, not listed as damaged.
    fs::write(folder.join("notes.txt"), b"mine").unwrap();
    fs::create_dir(folder.join("Folder.wfmixer")).unwrap();
    // A valid preset larger than the size that is checked whole when
    // listing: padded with white space, which JSON allows after a value.
    let mut large = serde_json::to_vec(&MixerTrackPreset {
        name: "Large".into(),
        ..preset.clone()
    })
    .unwrap();
    large.resize(9 * 1024 * 1024, b' ');
    fs::write(folder.join("Large.wfmixer"), &large).unwrap();

    let listed = rig.session.mixer_preset_library().unwrap();
    let by_file = |file: &str| {
        listed
            .entries
            .iter()
            .find(|entry| entry.file == file)
            .unwrap_or_else(|| panic!("{file} is not listed: {:#?}", listed.entries))
    };
    assert_eq!(listed.entries.len(), 6);
    assert_eq!(by_file(&good.file).error, None);
    for (file, reason) in [
        // The refusal is a code now; its words are the catalog's.
        ("Cut short.wfmixer", "mixerPresets.invalid"),
        ("Not json.wfmixer", "mixerPresets.invalid"),
        ("Newer.wfmixer", "Unsupported mixer preset version"),
        ("Out of range.WFMIXER", "invalid track settings"),
    ] {
        let entry = by_file(file);
        let error = entry
            .error
            .as_deref()
            .unwrap_or_else(|| panic!("{file} must carry an error"));
        assert!(error.contains(reason), "{file}: {error}");
        // The file's own name stands in for the one that cannot be read.
        assert_eq!(entry.name, file.rsplit_once('.').unwrap().0);
        assert_eq!(entry.effects, 0);
        // And reading it to apply fails with the same reason.
        let read = rig
            .session
            .mixer_preset_library_read(file.to_owned())
            .unwrap_err();
        assert!(read.contains(reason), "{file}: {read}");
    }
    let large = by_file("Large.wfmixer");
    assert_eq!(
        (large.name.as_str(), large.effects, &large.error),
        ("Large", 2, &None)
    );
    assert!(large.bytes >= 9 * 1024 * 1024);
    let read = rig
        .session
        .mixer_preset_library_read("Large.wfmixer".into())
        .unwrap();
    assert_eq!(read.effects, preset.effects);

    // The UI names a library file by its name. Anything else is refused,
    // for reading and for deleting, and nothing outside is touched.
    let outside = rig.file("outside.wfmixer");
    fs::write(&outside, &text).unwrap();
    for name in [
        "../outside.wfmixer",
        "..\\outside.wfmixer",
        "sub/inner.wfmixer",
        outside.as_str(),
        "notes.txt",
        "",
        ".",
        "..",
    ] {
        let refused = rig.session.mixer_preset_library_read(name.to_owned());
        assert!(refused.is_err(), "reading {name:?} must be refused");
        let refused = rig.session.mixer_preset_library_delete(name.to_owned());
        assert!(refused.is_err(), "deleting {name:?} must be refused");
    }
    assert!(std::path::Path::new(&outside).is_file());
    assert!(folder.join("notes.txt").is_file());
}

#[test]
fn qa_single_files_round_trip_and_invalid_ones_are_refused_both_ways() {
    let (rig, source, destination) = fixture();
    let preset = capture(&rig, source).unwrap();
    let path = rig.file("Chain.wfmixer");
    let shown = rig
        .session
        .mixer_preset_write(path.clone(), preset.clone())
        .unwrap();
    assert!(shown.ends_with("Chain.wfmixer"), "{shown}");
    assert_eq!(rig.session.mixer_preset_read(path.clone()).unwrap(), preset);

    // Writing over it replaces it whole.
    let mut renamed = preset.clone();
    renamed.name = "Renamed".into();
    rig.session
        .mixer_preset_write(path.clone(), renamed.clone())
        .unwrap();
    assert_eq!(
        rig.session.mixer_preset_read(path.clone()).unwrap(),
        renamed
    );

    // An invalid preset is not written, and the file there is left alone.
    let mut invalid = preset.clone();
    invalid.version = 7;
    assert!(
        rig.session
            .mixer_preset_write(path.clone(), invalid)
            .is_err()
    );
    let mut invalid = preset.clone();
    invalid.pan = f32::NAN;
    assert!(
        rig.session
            .mixer_preset_write(path.clone(), invalid)
            .is_err()
    );
    assert_eq!(
        rig.session.mixer_preset_read(path.clone()).unwrap(),
        renamed
    );

    // Reading: missing, cut short, another version, a repeated effect id.
    let missing = rig
        .session
        .mixer_preset_read(rig.file("nowhere.wfmixer"))
        .unwrap_err();
    assert!(missing.contains("mixerPresets.openFailed"), "{missing}");
    let text = serde_json::to_string(&preset).unwrap();
    let cut = rig.file("cut.wfmixer");
    fs::write(&cut, &text[..text.len() - 20]).unwrap();
    let error = rig.session.mixer_preset_read(cut).unwrap_err();
    assert!(error.contains("mixerPresets.invalid"), "{error}");
    let mut repeated = preset.clone();
    repeated.effects[1].id = repeated.effects[0].id;
    let twice = rig.file("twice.wfmixer");
    fs::write(&twice, serde_json::to_vec(&repeated).unwrap()).unwrap();
    assert!(rig.session.mixer_preset_read(twice).is_err());

    // A file that was read applies, and a refused apply changes nothing.
    let read = rig.session.mixer_preset_read(path).unwrap();
    let before = rig.project();
    let stale = Command::ApplyMixerTrackPreset {
        id: destination,
        expected: MixerTrack {
            volume: 0.123,
            ..track(&rig, destination)
        },
        preset: read.clone(),
        name_color: false,
    };
    let refused = rig.session.dispatch(stale, None).unwrap_err();
    assert!(refused.contains("destination track changed"), "{refused}");
    assert_eq!(rig.project(), before);
    apply(&rig, destination, &read, false).unwrap();
    assert_eq!(track(&rig, destination).effects.len(), 2);
}

#[test]
fn qa_capture_refuses_a_document_that_moved_on_and_a_track_that_is_gone() {
    let (rig, source, destination) = fixture();
    let state = rig.session.timeline_state();
    // An edit lands between reading the numbers and capturing.
    add_effect(&rig, destination, EffectKind::Limiter);
    let moved = rig
        .session
        .mixer_preset_capture(source, state.generation, state.revision)
        .unwrap_err();
    assert!(moved.contains("project changed"), "{moved}");
    // With the current numbers it captures.
    assert_eq!(capture(&rig, source).unwrap().effects.len(), 2);

    // A track that no longer exists.
    rig.session
        .dispatch(Command::RemoveMixerTrack { id: source }, None)
        .unwrap();
    assert!(capture(&rig, source).is_err());

    // Another project took this one's place: the old numbers are refused
    // even though the new project has tracks with the same ids.
    let old = rig.session.timeline_state();
    rig.session.project_new().unwrap();
    let replaced = rig
        .session
        .mixer_preset_capture(TrackId::MASTER, old.generation, old.revision)
        .unwrap_err();
    assert!(replaced.contains("project changed"), "{replaced}");
    assert!(capture(&rig, TrackId::MASTER).is_ok());
}

#[test]
fn qa_a_preset_with_a_plugin_that_is_not_installed_keeps_its_binding_and_state() {
    let (rig, source, destination) = fixture();
    // The source's second effect is a hosted plugin this machine lacks.
    let hosted = add_effect(&rig, source, EffectKind::Balance);
    let state: Vec<u8> = (0..=255).collect();
    let mut preset = capture(&rig, source).unwrap();
    preset.plugins.push(PluginBinding {
        target: PluginTarget::Effect { effect: hosted },
        format: "clap".into(),
        path: "C:/Not Installed/Tape Echo.clap".into(),
        id: "vendor.tape-echo".into(),
        name: "Tape Echo".into(),
        state: state.clone(),
        parameters: Vec::new(),
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
    });
    preset.validate().unwrap();

    // The library lists what the preset refers to, without the state.
    let saved = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    assert_eq!(saved.plugins.len(), 1);
    assert_eq!(
        (
            saved.plugins[0].format.as_str(),
            saved.plugins[0].id.as_str(),
            saved.plugins[0].name.as_str()
        ),
        ("clap", "vendor.tape-echo", "Tape Echo")
    );
    let read = rig.session.mixer_preset_library_read(saved.file).unwrap();
    assert_eq!(read.plugins[0].state, state);

    // Loading succeeds without the plugin. Its binding moves to the new
    // effect and keeps its state.
    let before = rig.project();
    apply(&rig, destination, &read, false).unwrap();
    let loaded = track(&rig, destination);
    assert_eq!(loaded.effects.len(), 3);
    let bound = |rig: &Rig| -> Vec<PluginBinding> {
        let effects = track(rig, destination).effects;
        rig.project()
            .plugins
            .into_iter()
            .filter(|plugin| {
                matches!(plugin.target, PluginTarget::Effect { effect }
                    if effects.iter().any(|slot| slot.id == effect))
            })
            .collect()
    };
    let binding = bound(&rig);
    assert_eq!(binding.len(), 1);
    assert_eq!(
        binding[0].target,
        PluginTarget::Effect {
            effect: loaded.effects[2].id
        }
    );
    assert_ne!(loaded.effects[2].id, hosted);
    assert_eq!(binding[0].state, state);
    assert_eq!(binding[0].id, "vendor.tape-echo");

    // The session keeps running with the plugin absent.
    let mut rig = rig;
    assert!(rig.run(4_800).iter().all(|sample| sample.is_finite()));

    // Undo removes the binding with the effects, redo brings both back.
    assert!(rig.session.undo().is_some());
    let mut undone = rig.project();
    undone.next_id = before.next_id;
    assert_eq!(undone, before);
    assert!(rig.session.redo().is_some());
    assert_eq!(bound(&rig), binding);

    // Saved and reopened, the binding and its state are still there.
    let path = rig.file("with-missing-plugin.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&path).unwrap();
    assert_eq!(bound(&rig), binding);

    // Captured again from the reopened project, the preset still carries
    // the opaque state, so it survives until the plugin is installed.
    let again = capture(&rig, destination).unwrap();
    assert_eq!(again.plugins.len(), 1);
    assert_eq!(again.plugins[0].state, state);
    assert_eq!(again.plugins[0].target, binding[0].target);
}
