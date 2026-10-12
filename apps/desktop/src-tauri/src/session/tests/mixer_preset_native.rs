//! Joined preset acceptance: real scanner/helper, running audio owner, disk
//! preset, another track, history and project reopen. No device is opened.
use super::{Rig, SAMPLE_RATE};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{Command, EffectId, MixerTrackPatch, PluginTarget, TrackId};

fn settle(rig: &mut Rig, expected: [f32; 2]) {
    rig.session.transport_seek(0.0);
    rig.session.transport_play().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut consecutive = 0;
    let mut output = [0.0; 960];
    loop {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut output)),
            0
        );
        let matches = output.as_chunks::<2>().0.iter().all(|frame| {
            (frame[0] - expected[0]).abs() < 2e-4 && (frame[1] - expected[1]).abs() < 2e-4
        });
        consecutive = if matches { consecutive + 1 } else { 0 };
        if consecutive >= 12 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native preset PCM never settled to {expected:?}; last {:?}",
            &output[900..904]
        );
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn joined(vst3: bool) {
    let mut rig = Rig::new();
    let fixture = PathBuf::from(
        std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("prebuilt native fixture"),
    );
    let helper = PathBuf::from(
        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER").expect("current desktop helper"),
    );
    let path = rig
        .folder
        .path()
        .join(if vst3 { "preset.vst3" } else { "preset.clap" });
    std::fs::copy(fixture, &path).unwrap();
    let manager = crate::plugins::PluginManager::bridge_fixture(rig.folder.path(), &path, &helper);
    rig.session.install_plugins(manager.clone());
    let entry = manager
        .state()
        .entries
        .into_iter()
        .find(|entry| {
            entry.name
                == if vst3 {
                    "VST3 Bridge Delayed Effect"
                } else {
                    "Test Bridge Delayed Effect"
                }
        })
        .expect("real scanner found the stateful delayed fixture");
    let add = |name: &str| {
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
    };
    let source = add("Running source");
    let destination = add("Restored destination");
    for id in [TrackId::MASTER, source, destination] {
        rig.session
            .dispatch(
                Command::UpdateMixerTrack {
                    id,
                    patch: MixerTrackPatch {
                        volume: Some(1.0),
                        pan: Some(0.0),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
    }
    let audio = rig.file("steady.wav");
    windfall_codec::write_wav(
        Path::new(&audio),
        &windfall_core::AudioBuffer::from_interleaved(
            SAMPLE_RATE,
            2,
            (0..SAMPLE_RATE * 30).flat_map(|_| [0.2, -0.1]).collect(),
        ),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .add_audio_clip_from_file(
            &audio,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: Some(source),
            },
        )
        .unwrap();
    let target = PluginTarget::Effect {
        effect: EffectId(0),
    };
    let binding = manager.binding(&entry.path, &entry.id, target).unwrap();
    let discovery_state = binding.state.clone();
    let effect = EffectId(
        *rig.session
            .dispatch(
                Command::AddPluginEffect {
                    track: source,
                    plugin: binding,
                },
                None,
            )
            .unwrap()
            .created
            .last()
            .unwrap(),
    );
    let target = PluginTarget::Effect { effect };
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 7,
                value: 0.625,
            },
            None,
        )
        .unwrap();
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
    settle(&mut rig, [0.125, -0.0625]);
    let owner = manager
        .runtime
        .bridge_status()
        .into_iter()
        .find(|status| status.0 == target)
        .unwrap();
    assert_ne!(owner.2.process_id, std::process::id());
    assert!(!owner.2.failed && !owner.2.reaped);
    // Capture while playing and continue to service the actual audio owner.
    let state = rig.session.timeline_state();
    let capture = rig.session.background(move |session| {
        session.mixer_preset_capture(source, state.generation, state.revision)
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    while !capture.is_finished() {
        assert!(
            Instant::now() < deadline,
            "running-owner preset capture hung"
        );
        rig.run(480);
        std::thread::sleep(Duration::from_millis(8));
    }
    let preset = capture.join().unwrap().unwrap();
    assert_eq!(preset.plugins.len(), 1);
    assert!(preset.plugins[0].state.starts_with(b"WFPS"));
    assert_ne!(
        preset.plugins[0].state, discovery_state,
        "captured processed native state"
    );
    rig.session.transport_stop();
    let file = rig
        .session
        .mixer_preset_library_save(preset.clone())
        .unwrap();
    let preset = rig
        .session
        .mixer_preset_library_read(file.file.clone())
        .unwrap();
    let before = rig.project().mixer.track(destination).unwrap().clone();
    let cursor = rig.session.document_snapshot().history.cursor;
    rig.session
        .dispatch(
            Command::ApplyMixerTrackPreset {
                id: destination,
                expected: before.clone(),
                preset,
                name_color: false,
            },
            None,
        )
        .unwrap();
    assert_eq!(rig.session.document_snapshot().history.cursor, cursor + 1);
    let restored = rig.project().mixer.track(destination).unwrap().clone();
    assert_ne!(restored.effects[0].id, effect);
    let restored_target = PluginTarget::Effect {
        effect: restored.effects[0].id,
    };
    assert_eq!(
        rig.project().plugin(restored_target).unwrap().state,
        rig.session
            .mixer_preset_library_read(file.file.clone())
            .unwrap()
            .plugins[0]
            .state
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.project().mixer.track(destination), Some(&before));
    rig.session.redo().unwrap();
    // Route the identical source through the newly restored chain.
    let clip = rig.project().playlist.clips[0].id;
    rig.session
        .dispatch(
            Command::UpdateAudioClips {
                updates: vec![windfall_project::AudioClipUpdate {
                    id: clip,
                    patch: windfall_project::AudioClipPatch {
                        mixer_track: Some(destination),
                        ..Default::default()
                    },
                }],
            },
            None,
        )
        .unwrap();
    settle(&mut rig, [0.125, -0.0625]);
    let save = rig.file("restored.windfall");
    let save_path = save.clone();
    let task = rig
        .session
        .background(move |session| session.project_save(Some(&save_path)));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !task.is_finished() {
        assert!(Instant::now() < deadline);
        rig.run(480);
        std::thread::sleep(Duration::from_millis(8));
    }
    task.join().unwrap().unwrap();
    rig.session.transport_stop();
    rig.session.project_new().unwrap();
    rig.session.project_open(&save).unwrap();
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
    settle(&mut rig, [0.125, -0.0625]);
}

#[test]
fn clap_running_helper_preset_restores_pcm_history_and_reopen() {
    joined(false);
}
#[test]
fn vst3_running_helper_preset_restores_pcm_history_and_reopen() {
    joined(true);
}
