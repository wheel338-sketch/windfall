//! Preset, score and template files through the session, on a real disk.

use std::fs;
use std::path::Path;

use super::{Rig, factory_file};
use windfall_core::AudioBuffer;
use windfall_ipc::PresetCapture;
use windfall_project::{
    ChannelSource, Command, EffectKind, NoteInit, PluginBinding, PluginPresetState, PluginTarget,
    PresetKind, SamplePath, Score, ScoreLoadMode, SettingsPatch, TrackId,
};

fn tone(path: &str, value: f32) {
    fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
    windfall_codec::write_wav(
        path,
        &AudioBuffer::from_interleaved(48000, 2, vec![value; 4800]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
}

/// The generation and revision a capture is asked for with.
fn seen(rig: &Rig) -> (u64, u64) {
    let generation = rig.session.state().generation;
    (generation, rig.session.document_snapshot().revision)
}

fn capture_channel(rig: &Rig, index: usize) -> PresetCapture {
    let (generation, revision) = seen(rig);
    rig.session
        .preset_capture_channel(rig.channel(index), generation, revision)
        .unwrap()
}

#[test]
fn modular_rack_native_file_library_capture_and_apply_preserve_parameter_addresses_and_history() {
    use windfall_project::rack::{RackGraph, RackNode, RackNoteModule, RackProcessor};
    use windfall_project::{ChannelId, InstrumentKind};
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: Some("Native rack".into()),
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    sample: None,
                    index: None,
                    mixer_track: Some(TrackId::MASTER),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let graph = RackGraph {
        next_id: 2,
        nodes: vec![RackNode {
            id: 1,
            name: "Envelope".into(),
            position: [0.0; 2],
            muted: false,
            solo: false,
            processor: RackProcessor::Note {
                module: RackNoteModule::Envelope {
                    params: Default::default(),
                },
            },
        }],
        ..Default::default()
    };
    rig.session
        .dispatch(
            Command::SetChannelRack {
                id: channel,
                graph: Some(graph),
                expected: None,
            },
            None,
        )
        .unwrap();
    let saved = rig.project().settings.modular_racks[0].graph.clone();
    assert_eq!(saved.parameters.len(), 6);
    let (generation, revision) = seen(&rig);
    let capture = rig
        .session
        .preset_capture_channel(channel, generation, revision)
        .unwrap();
    assert_eq!(
        capture.preset.channel.as_ref().unwrap().rack,
        Some(saved.clone())
    );
    let path = rig
        .session
        .preset_write(&rig.file("rack.wfpreset"), capture.clone())
        .unwrap();
    let review = rig.session.preset_read(&path).unwrap();
    assert!(review.notes.is_empty());
    assert!(review.missing_audio.is_empty());
    assert_eq!(review.preset.channel.unwrap().rack, Some(saved.clone()));
    let entry = rig
        .session
        .preset_library_save("Original rack", capture)
        .unwrap();
    assert!(
        rig.session
            .preset_library()
            .unwrap()
            .iter()
            .any(|item| item.path == entry.path)
    );
    assert_eq!(
        rig.session
            .preset_read(&entry.path)
            .unwrap()
            .preset
            .channel
            .unwrap()
            .rack,
        Some(saved.clone())
    );
    rig.session
        .dispatch(
            Command::SetChannelRack {
                id: channel,
                graph: None,
                expected: None,
            },
            None,
        )
        .unwrap();
    let before = rig.project();
    let expected = before.channel(channel).unwrap().clone();
    rig.session
        .preset_apply_channel(&path, channel, expected, false, false)
        .unwrap();
    assert_eq!(rig.project().settings.modular_racks[0].graph, saved);
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project().settings.modular_racks[0].graph, saved);
}

#[test]
fn a_sampler_preset_takes_its_audio_along_and_loads_as_one_undo_step() {
    let rig = Rig::new();
    let kick = rig.project().channels[0].clone();
    let capture = capture_channel(&rig, 0);
    assert_eq!(capture.preset.kind, PresetKind::Channel);
    let source = capture.audio.clone().expect("the kick has audio");
    assert!(Path::new(&source).is_file());

    // Without the extension, in a folder that does not exist yet.
    let written = rig
        .session
        .preset_write(&rig.file("presets/Deep kick"), capture)
        .unwrap();
    assert!(written.ends_with("Deep kick.wfpreset"));
    let folder = Path::new(&written).parent().unwrap();
    let review = rig.session.preset_read(&written).unwrap();
    assert_eq!(review.notes, Vec::<String>::new());
    assert_eq!(review.missing_audio, Vec::<String>::new());
    let asset = &review.preset.assets[0];
    assert!(asset.path.starts_with("Deep kick audio/"), "{}", asset.path);
    let copied = folder.join(&asset.path);
    assert_eq!(fs::read(&copied).unwrap(), fs::read(&source).unwrap());
    assert_eq!(asset.bytes, fs::metadata(&copied).unwrap().len());
    // Nothing in the file is a path of this machine.
    let text = fs::read_to_string(&written).unwrap();
    assert!(!text.contains(&rig.folder.path().display().to_string()));

    // Onto the clap's channel.
    let destination = rig.channel(1);
    let expected = rig.project().channels[1].clone();
    let history = rig.session.document_snapshot().history.entries.len();
    let result = rig
        .session
        .preset_apply_channel(&written, destination, expected.clone(), false, false)
        .unwrap();
    let loaded = rig.project().channels[1].clone();
    assert_eq!(
        rig.session.document_snapshot().history.entries.len(),
        history + 1
    );
    assert_eq!(loaded.id, expected.id);
    assert_eq!(loaded.name, expected.name);
    assert_eq!(loaded.mixer_track, expected.mixer_track);
    assert_eq!(loaded.volume, kick.volume);
    let sample = loaded.source.sample().expect("the preset's audio");
    assert_eq!(result.created, [sample.0]);
    let project = rig.project();
    let added = project.sample(sample).unwrap();
    // The audio plays from beside the preset, not from where it came from.
    assert_eq!(
        added.path,
        SamplePath::External(crate::paths::display(&crate::paths::clean(&copied)))
    );
    rig.wait_until_loaded_or_failed(sample);
    assert!(rig.has_audio(sample));

    rig.session.undo().unwrap();
    assert_eq!(rig.project().channels[1], expected);
    assert!(rig.project().sample(sample).is_none());
    rig.session.redo().unwrap();
    assert_eq!(rig.project().channels[1], loaded);

    // A channel that changed since the preset was chosen is refused whole.
    let before = rig.project();
    let refused = rig
        .session
        .preset_apply_channel(&written, destination, expected, false, false);
    assert!(refused.unwrap_err().contains("changed"));
    assert_eq!(rig.project(), before);
}

#[test]
fn missing_audio_is_reported_and_loads_only_when_asked_to() {
    let rig = Rig::new();
    let written = rig
        .session
        .preset_write(&rig.file("Kick.wfpreset"), capture_channel(&rig, 0))
        .unwrap();
    let review = rig.session.preset_read(&written).unwrap();
    let audio = Path::new(&written)
        .parent()
        .unwrap()
        .join(&review.preset.assets[0].path);
    fs::remove_file(&audio).unwrap();

    let review = rig.session.preset_read(&written).unwrap();
    assert_eq!(review.missing_audio, [review.preset.assets[0].name.clone()]);
    let destination = rig.channel(1);
    let expected = rig.project().channels[1].clone();
    let before = rig.project();
    let refused =
        rig.session
            .preset_apply_channel(&written, destination, expected.clone(), false, false);
    assert!(refused.unwrap_err().contains("not beside it"));
    assert_eq!(rig.project(), before);

    // Asked to, the settings load and the sampler is left with no sample.
    rig.session
        .preset_apply_channel(&written, destination, expected, false, true)
        .unwrap();
    assert_eq!(rig.project().channels[1].source.sample(), None);
    assert_eq!(rig.project().samples.len(), before.samples.len());
    rig.project().check().unwrap();
}

#[test]
fn a_capture_for_a_project_that_moved_on_is_refused() {
    let rig = Rig::new();
    let (generation, revision) = seen(&rig);
    let channel = rig.channel(0);
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(99.0),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap();
    // The revision the caller saw is no longer the project's.
    let stale = rig
        .session
        .preset_capture_channel(channel, generation, revision);
    assert!(stale.unwrap_err().contains("changed"));
    let target = PluginTarget::Instrument { channel };
    assert!(
        rig.session
            .preset_capture_plugin(target, generation, revision)
            .is_err()
    );
    // Nor is the generation, once another project is open.
    let (_, revision) = seen(&rig);
    rig.session.undo().unwrap();
    rig.session.project_new().unwrap();
    assert!(
        rig.session
            .preset_capture_channel(channel, generation, revision)
            .is_err()
    );
}

#[test]
fn plugin_presets_keep_a_missing_plugins_state_and_the_library_lists_them() {
    let rig = Rig::new();
    let effect = windfall_project::EffectId(
        rig.session
            .dispatch(
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: EffectKind::Limiter,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let state: Vec<u8> = (0..=255).collect();
    let synth = windfall_project::ChannelId(
        rig.session
            .dispatch(
                Command::AddPluginInstrument {
                    plugin: PluginBinding {
                        sidechain_input: None,
                        auxiliary_inputs: Vec::new(),
                        capabilities: Default::default(),
                        wrapper: Default::default(),
                        target: PluginTarget::Instrument {
                            channel: windfall_project::ChannelId(0),
                        },
                        format: "clap".into(),
                        path: rig.file("not installed/synth.clap"),
                        id: "org.example.synth".into(),
                        name: "Example synth".into(),
                        state: state.clone(),
                        parameters: Vec::new(),
                    },
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    assert!(rig.session.preset_library().unwrap().is_empty());

    let (generation, revision) = seen(&rig);
    let limiter = rig
        .session
        .preset_capture_plugin(PluginTarget::Effect { effect }, generation, revision)
        .unwrap();
    assert!(limiter.audio.is_none());
    let hosted = rig
        .session
        .preset_capture_plugin(
            PluginTarget::Instrument { channel: synth },
            generation,
            revision,
        )
        .unwrap();
    let saved = rig
        .session
        .preset_library_save("Loud / louder", limiter)
        .unwrap();
    assert_eq!(saved.name, "Loud / louder");
    assert!(saved.path.ends_with("Loud _ louder.wfpreset"));
    assert_eq!(saved.kind, PresetKind::Plugin);
    assert_eq!(saved.plugin.as_ref().unwrap().format, "builtin");
    let lead = rig.session.preset_library_save("Lead", hosted).unwrap();
    assert_eq!(lead.plugin.as_ref().unwrap().id, "org.example.synth");
    assert!(lead.plugin.as_ref().unwrap().instrument);
    let listed = rig.session.preset_library().unwrap();
    assert_eq!(listed, [lead.clone(), saved.clone()]);

    // The plugin is not installed: the review says so and the state is whole.
    let review = rig.session.preset_read(&lead.path).unwrap();
    assert_eq!(review.notes.len(), 1, "{:?}", review.notes);
    assert!(review.notes[0].contains("not installed"));
    let Some(PluginPresetState::Hosted { binding, .. }) =
        review.preset.plugin.as_ref().map(|plugin| &plugin.state)
    else {
        panic!("{:?}", review.preset)
    };
    assert_eq!(binding.state, state);

    // The limiter's preset loads onto the effect through the command.
    let review = rig.session.preset_read(&saved.path).unwrap();
    let target = PluginTarget::Effect { effect };
    let expected = windfall_project::PluginPreset::capture(&rig.project(), target)
        .unwrap()
        .state;
    rig.session
        .dispatch(
            Command::ApplyPluginPreset {
                target,
                expected,
                preset: review.preset.plugin.unwrap(),
            },
            None,
        )
        .unwrap();

    // Only presets of the library are deleted through it.
    let outside = rig
        .session
        .preset_write(&rig.file("Elsewhere.wfpreset"), {
            let (generation, revision) = seen(&rig);
            rig.session
                .preset_capture_plugin(target, generation, revision)
                .unwrap()
        })
        .unwrap();
    assert!(rig.session.preset_library_delete(&outside).is_err());
    assert!(Path::new(&outside).is_file());
    rig.session.preset_library_delete(&lead.path).unwrap();
    assert_eq!(rig.session.preset_library().unwrap(), [saved]);
    for name in ["", "  ", "CON", ".."] {
        let (generation, revision) = seen(&rig);
        let capture = rig
            .session
            .preset_capture_plugin(target, generation, revision)
            .unwrap();
        assert!(
            rig.session.preset_library_save(name, capture).is_err(),
            "{name:?}"
        );
    }
    // A file from a newer build is refused by name.
    let newer = rig.file("Newer.wfpreset");
    fs::write(
        &newer,
        br#"{"schema":"windfall.preset","version":99,"kind":"plugin"}"#,
    )
    .unwrap();
    assert!(
        rig.session
            .preset_read(&newer)
            .unwrap_err()
            .contains("newer Windfall")
    );
}

#[test]
fn a_score_is_written_as_a_score_or_a_midi_file_and_read_back_from_either() {
    let rig = Rig::new();
    let (pattern, channel) = (rig.pattern(), rig.channel(0));
    rig.session
        .dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![
                    NoteInit {
                        start: 0,
                        length: 480,
                        key: 60,
                        velocity: Some(1.0),
                        pan: Some(-0.5),
                        expression: None,
                    },
                    NoteInit {
                        start: 960,
                        length: 240,
                        key: 64,
                        velocity: Some(64.0 / 127.0),
                        pan: None,
                        expression: None,
                    },
                ],
            },
            None,
        )
        .unwrap();
    let score = Score::capture(&rig.project(), pattern, channel, None).unwrap();
    let before = rig.project();

    let native = rig
        .session
        .score_write(&rig.file("scores/Riff"), score.clone())
        .unwrap();
    assert!(native.path.ends_with("Riff.wfscore"));
    assert_eq!(native.notes, Vec::<String>::new());
    let read = rig.session.score_read(&native.path).unwrap();
    assert_eq!(read.score, score);
    assert_eq!(read.notes, Vec::<String>::new());

    let midi = rig
        .session
        .score_write(&rig.file("scores/Riff.mid"), score.clone())
        .unwrap();
    assert!(midi.path.ends_with("Riff.mid"));
    // The one panned note is what a MIDI file does not hold.
    assert_eq!(midi.notes.len(), 1, "{:?}", midi.notes);
    assert_eq!(&fs::read(&midi.path).unwrap()[..4], b"MThd");
    let read = rig.session.score_read(&midi.path).unwrap();
    assert_eq!(
        read.score
            .notes
            .iter()
            .map(|note| (note.start, note.length, note.key, note.velocity))
            .collect::<Vec<_>>(),
        score
            .notes
            .iter()
            .map(|note| (note.start, note.length, note.key, note.velocity))
            .collect::<Vec<_>>()
    );
    // A MIDI file is one whatever it is called.
    let renamed = rig.file("scores/Riff as midi.wfscore");
    fs::copy(&midi.path, &renamed).unwrap();
    assert_eq!(rig.session.score_read(&renamed).unwrap().score, read.score);

    // Neither writing nor reading touched the project.
    assert_eq!(rig.project(), before);
    assert!(
        rig.session
            .score_read(&rig.file("scores/None.wfscore"))
            .is_err()
    );
    fs::write(rig.file("scores/Bad.wfscore"), b"{}").unwrap();
    assert!(
        rig.session
            .score_read(&rig.file("scores/Bad.wfscore"))
            .is_err()
    );

    // What was read loads into another lane as one step.
    let other = rig.channel(1);
    let command = read
        .score
        .load_command(&rig.project(), pattern, other, ScoreLoadMode::Replace)
        .unwrap();
    rig.session.dispatch(command, None).unwrap();
    let project = rig.project();
    let lane = project.pattern(pattern).unwrap().lane(other).unwrap();
    assert_eq!(lane.notes.len(), 2);
    rig.session.undo().unwrap();
    // Ids are never handed out twice, so the counter stays where it got to.
    let mut restored = before.clone();
    restored.next_id = rig.project().next_id;
    assert_eq!(rig.project(), restored);
}

#[test]
fn factory_templates_start_fresh_projects_and_cannot_be_deleted_or_shadowed() {
    let rig = Rig::new();
    let templates = rig.session.templates().unwrap();
    assert!(templates.len() >= 4);
    assert!(templates.iter().all(|template| template.factory));
    for template in &templates {
        let snapshot = rig.session.template_new(&template.id).unwrap();
        assert_eq!(snapshot.path, None, "{}", template.id);
        assert!(!snapshot.dirty, "{}", template.id);
        assert!(snapshot.history.entries.is_empty(), "{}", template.id);
        assert_eq!(snapshot.project.settings.name, "Untitled");
        snapshot.project.check().unwrap();
        assert_eq!(rig.project(), snapshot.project);
        for sample in &snapshot.project.samples {
            assert!(rig.has_audio(sample.id), "{}: {}", template.id, sample.name);
        }
        assert!(rig.session.template_delete(&template.id).is_err());
        assert!(rig.session.template_save(&template.name).is_err());
        assert!(
            rig.session
                .template_save(&template.name.to_uppercase())
                .is_err()
        );
    }
    assert_eq!(rig.session.templates().unwrap(), templates);
    assert!(rig.session.template_new("factory:nothing").is_err());
    assert!(rig.session.template_new("basic-kit").is_err());

    // The beat templates have a beat in them.
    let house = rig
        .session
        .template_new("factory:four-on-the-floor")
        .unwrap();
    assert_eq!(house.project.settings.tempo_bpm, 124.0);
    assert!(!house.project.patterns[0].lanes.is_empty());
}

#[test]
fn a_user_template_carries_its_audio_and_opens_as_a_new_project() {
    let rig = Rig::new();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("tone.wav");
    let external = external.to_string_lossy().into_owned();
    tone(&external, 0.25);
    rig.session.add_channel_from_file(&external, None).unwrap();
    rig.session
        .add_channel_from_file(&factory_file("Drums/Kicks/Kick Punch.wav"), None)
        .unwrap();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("My song".into()),
                    tempo_bpm: Some(99.0),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap();
    let saved_to = rig
        .session
        .project_save(Some(&rig.file("My song")))
        .unwrap();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    swing: Some(0.5),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap();
    let live = rig.session.document_snapshot();
    assert!(live.dirty);

    let template = rig.session.template_save("  Sketch pad ").unwrap();
    assert_eq!(template.name, "Sketch pad");
    assert_eq!(template.id, "user:Sketch pad");
    assert!(!template.factory);
    let file = rig.folder.path().join("templates/Sketch pad.wftemplate");
    assert!(file.is_file());
    let bytes = fs::read(&file).unwrap();
    // Saving a template left the open project exactly as it was.
    let after = rig.session.document_snapshot();
    assert_eq!(after.project, live.project);
    assert_eq!(after.path.as_deref(), Some(saved_to.as_str()));
    assert_eq!(after.dirty, live.dirty);
    assert_eq!(after.history.entries.len(), live.history.entries.len());
    assert!(rig.session.templates().unwrap().contains(&template));

    // A template is never overwritten, whatever the case of the name.
    for name in ["Sketch pad", "sketch PAD"] {
        assert!(
            rig.session
                .template_save(name)
                .unwrap_err()
                .contains("exists already")
        );
    }
    assert_eq!(fs::read(&file).unwrap(), bytes);

    // The source audio goes away; the template has its own.
    rig.session.project_save(None).unwrap();
    rig.session.project_new().unwrap();
    fs::remove_file(&external).unwrap();
    let started = rig.session.template_new(&template.id).unwrap();
    assert_eq!(started.path, None);
    assert!(!started.dirty);
    assert!(started.history.entries.is_empty());
    assert_eq!(started.project.settings.name, "Untitled");
    assert_eq!(started.project.settings.tempo_bpm, 99.0);
    assert_eq!(started.project.settings.swing, 0.5);
    assert_eq!(started.project.channels.len(), live.project.channels.len());
    for (channel, original) in started.project.channels.iter().zip(&live.project.channels) {
        assert_eq!(channel.name, original.name);
        assert!(matches!(channel.source, ChannelSource::Sampler(_)));
    }
    for sample in &started.project.samples {
        assert!(matches!(sample.path, SamplePath::Project(_)), "{sample:?}");
        assert!(rig.has_audio(sample.id), "{}", sample.name);
    }
    assert!(rig.session.undo().is_none());
    // Starting from it did not change it, and it can be used again.
    assert_eq!(fs::read(&file).unwrap(), bytes);
    rig.session.template_new(&template.id).unwrap();

    // Saving the new project asks for a place: it has none of its own.
    assert!(rig.session.project_save(None).is_err());

    for id in ["user:../settings", "user:", "user:Nothing", "Sketch pad"] {
        assert!(rig.session.template_new(id).is_err(), "{id}");
        assert!(rig.session.template_delete(id).is_err(), "{id}");
    }
    rig.session.template_delete(&template.id).unwrap();
    assert!(!file.exists());
    assert!(rig.session.templates().unwrap().iter().all(|t| t.factory));
    assert!(rig.session.template_new(&template.id).is_err());
}

#[test]
fn running_native_channel_and_plugin_presets_capture_apply_undo_and_reopen() {
    use windfall_project::ChannelId;
    fn exported_level(rig: &Rig, name: &str) -> f32 {
        rig.events.take();
        let options = windfall_ipc::ExportOptions {
            path: rig.file(name),
            mode: windfall_ipc::PlayMode::Pattern,
            tail_secs: 0.0,
            ..Default::default()
        };
        rig.session.export_audio(options.clone()).unwrap();
        assert_eq!(rig.events.wait_for_export().error, None);
        super::rms(windfall_codec::decode_file(options.path).unwrap().samples())
    }
    for channel_preset in [false, true] {
        let mut rig = Rig::new();
        let (manager, path) = super::plugins::manager(&rig);
        let plugin = manager
            .binding(
                &path,
                "org.windfall.test.sine",
                PluginTarget::Instrument {
                    channel: ChannelId(0),
                },
            )
            .unwrap();
        let channel = ChannelId(
            rig.session
                .dispatch(Command::AddPluginInstrument { plugin }, None)
                .unwrap()
                .created[0],
        );
        let target = PluginTarget::Instrument { channel };
        rig.session
            .dispatch(
                Command::SetPluginParam {
                    target,
                    id: 1,
                    value: 0.7,
                },
                None,
            )
            .unwrap();
        rig.session
            .dispatch(
                Command::ToggleStep {
                    pattern: rig.pattern(),
                    channel,
                    step: 0,
                },
                None,
            )
            .unwrap();
        rig.run(256);
        let original_level = exported_level(&rig, "capture-reference.wav");
        assert!(original_level > 0.01);
        let captured = manager
            .runtime
            .capture(rig.project())
            .unwrap()
            .plugin(target)
            .unwrap()
            .clone();
        let (generation, revision) = seen(&rig);
        let capture = if channel_preset {
            rig.session
                .preset_capture_channel(channel, generation, revision)
        } else {
            rig.session
                .preset_capture_plugin(target, generation, revision)
        }
        .unwrap();
        let saved = rig
            .session
            .preset_write(&rig.file("running.wfpreset"), capture)
            .unwrap();
        rig.session
            .dispatch(
                Command::SetPluginParam {
                    target,
                    id: 1,
                    value: 0.2,
                },
                None,
            )
            .unwrap();
        rig.run(256);
        let changed_level = exported_level(&rig, "changed-reference.wav");
        assert!(changed_level < original_level * 0.5);
        let expected = rig.project().channel(channel).unwrap().clone();
        let (generation, revision) = seen(&rig);
        let history = rig.session.document_snapshot().history.entries.len();
        if channel_preset {
            rig.session
                .preset_apply_channel_guarded(
                    &saved,
                    channel,
                    expected,
                    false,
                    false,
                    Some((generation, revision)),
                )
                .unwrap();
        } else {
            rig.session
                .preset_apply_plugin(&saved, target, generation, revision)
                .unwrap();
        }
        assert_eq!(
            rig.session.document_snapshot().history.entries.len(),
            history + 1
        );
        rig.run(256);
        assert_eq!(
            manager
                .runtime
                .capture(rig.project())
                .unwrap()
                .plugin(target)
                .unwrap(),
            &captured
        );
        rig.session.undo().unwrap();
        assert_eq!(
            rig.project()
                .plugin(target)
                .unwrap()
                .parameters
                .iter()
                .find(|p| p.id == 1)
                .unwrap()
                .value,
            0.2
        );
        rig.session.redo().unwrap();
        let project = rig
            .session
            .project_save(Some(&rig.file("restored.windfall")))
            .unwrap();
        rig.session.project_new().unwrap();
        rig.session.project_open(&project).unwrap();
        rig.run(256);
        assert_eq!(
            manager
                .runtime
                .capture(rig.project())
                .unwrap()
                .plugin(target)
                .unwrap(),
            &captured
        );
        let restored_level = exported_level(&rig, "restored.wav");
        assert!((restored_level - original_level).abs() < original_level * 0.001);
        assert!(restored_level > changed_level * 3.0);
        let before = rig.session.document_snapshot();
        assert!(
            rig.session
                .preset_apply_plugin(&saved, target, generation, revision)
                .unwrap_err()
                .contains("changed")
        );
        assert_eq!(rig.session.document_snapshot(), before);
    }
}

#[test]
fn a_template_install_refuses_edits_and_project_replacement_without_leaking_audio() {
    for replace in [false, true] {
        let rig = Rig::new();
        let template = rig.session.template_save("Race").unwrap();
        rig.session.project_new().unwrap();
        let held = rig.session.hold("archive:install");
        let work = rig
            .session
            .background(move |s| s.template_new(&template.id));
        held.wait();
        if replace {
            rig.session.project_new().unwrap();
        } else {
            rig.session
                .dispatch(
                    Command::UpdateSettings {
                        patch: SettingsPatch {
                            tempo_bpm: Some(91.0),
                            ..Default::default()
                        },
                    },
                    None,
                )
                .unwrap();
        }
        let before = rig.session.document_snapshot();
        held.release();
        assert!(work.join().unwrap().is_err());
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(
            fs::read_dir(rig.folder.path().join("projects"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn extracted_template_audio_is_reclaimed_after_replacement_and_capture_copies_remain_reusable() {
    let rig = Rig::new();
    let template = rig.session.template_save("Cleanup").unwrap();
    rig.session.template_new(&template.id).unwrap();
    let first = rig.session.state().sample_dir.clone().unwrap();
    assert!(first.is_dir());
    // Active document/history sources keep the extraction, even across GC.
    rig.session.prune_extractions();
    assert!(first.is_dir());
    rig.session.template_new(&template.id).unwrap();
    assert!(!first.exists());
    let second = rig.session.state().sample_dir.clone().unwrap();
    let capture = capture_channel(&rig, 0);
    rig.session.project_new().unwrap();
    // A snapshot returned to the UI can still be saved after a replacement.
    assert!(!second.exists());
    let captured_audio = capture.audio.clone().unwrap();
    let captured_bytes = fs::read(&captured_audio).unwrap();
    let preset = rig
        .session
        .preset_write(&rig.file("late.wfpreset"), capture.clone())
        .unwrap();
    assert!(
        rig.session
            .preset_read(&preset)
            .unwrap()
            .missing_audio
            .is_empty()
    );
    rig.session
        .preset_write(&rig.file("late-again.wfpreset"), capture)
        .unwrap();
    assert_eq!(fs::read(&captured_audio).unwrap(), captured_bytes);
    // An unrelated directory is never swept.
    let untracked = rig.folder.path().join("projects/project-user-files");
    fs::create_dir_all(&untracked).unwrap();
    fs::write(untracked.join("keep"), b"keep").unwrap();
    rig.session.prune_extractions();
    assert!(untracked.join("keep").exists());
    let restarted = rig.restart();
    assert!(!Path::new(&captured_audio).exists());
    assert!(untracked.join("keep").exists());
    assert!(
        restarted
            .session
            .preset_read(&preset)
            .unwrap()
            .missing_audio
            .is_empty()
    );
}

#[test]
fn ended_template_owner_recovers_complete_unsaved_project_and_exact_audio_on_restart() {
    let rig = Rig::new();
    let template = rig.session.template_save("Recover").unwrap();
    rig.session.template_new(&template.id).unwrap();
    let root = rig.session.state().sample_dir.clone().unwrap();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(91.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let project = rig.project();
    let files: Vec<_> = project
        .samples
        .iter()
        .map(|sample| {
            let path = windfall_project::file::resolve_sample_path(
                &sample.path,
                Some(&root),
                rig.session.factory_dir(),
            )
            .unwrap();
            (
                path.strip_prefix(&root).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            )
        })
        .collect();
    let restarted = rig.restart();
    assert!(!root.exists());
    let recovery = restarted
        .session
        .recent_projects()
        .into_iter()
        .find(|path| path.contains("recovered-projects"))
        .unwrap();
    let recovered = Path::new(&recovery).parent().unwrap();
    for (relative, bytes) in files {
        assert_eq!(fs::read(recovered.join(relative)).unwrap(), bytes);
    }
    restarted.session.project_open(&recovery).unwrap();
    assert_eq!(restarted.project(), project);
    for sample in &project.samples {
        assert!(restarted.has_audio(sample.id));
    }
}

#[test]
fn actual_native_scanner_rescans_blocked_and_all_then_removes_a_folder() {
    use crate::plugins::Rescan;
    use std::time::{Duration, Instant};
    fn finish(manager: &crate::plugins::PluginManager) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while manager.state().scanning {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(manager.state().error, None);
    }
    let rig = Rig::new();
    let (manager, path) = super::plugins::manager(&rig);
    let folder = Path::new(&path)
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let broken = Path::new(&folder).join("broken.clap");
    fs::write(&broken, b"not a native library").unwrap();
    manager.rescan(Rescan::Everything).unwrap();
    finish(&manager);
    let state = manager.state();
    assert!(
        state
            .entries
            .iter()
            .any(|entry| entry.id == "org.windfall.test.sine")
    );
    assert!(
        state
            .blocked
            .iter()
            .any(|entry| entry.path == crate::paths::display(&broken) && entry.error.is_some()),
        "blocked entries: {:?}",
        state.blocked
    );
    fs::copy(&path, &broken).unwrap();
    manager.rescan(Rescan::Blocked).unwrap();
    finish(&manager);
    assert!(
        manager
            .state()
            .entries
            .iter()
            .any(|entry| entry.path == crate::paths::display(&broken)
                && entry.id == "org.windfall.test.sine"
                && entry.error.is_none())
    );
    manager.rescan(Rescan::Everything).unwrap();
    finish(&manager);
    assert!(
        manager
            .state()
            .entries
            .iter()
            .filter(|entry| entry.id == "org.windfall.test.sine")
            .count()
            >= 2
    );
    manager.remove_folder(&folder).unwrap();
    manager.scan(None).unwrap();
    finish(&manager);
    assert!(
        !manager
            .state()
            .entries
            .iter()
            .any(|entry| entry.path.starts_with(&folder))
    );
    assert!(!manager.state().folders.contains(&folder));
}
