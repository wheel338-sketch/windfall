use super::{Rig, rms, still_running};
use crate::session::{
    ClipPlace,
    recording::{CaptureHandle, Sink},
};
use std::{fs, path::Path};
use windfall_core::AudioBuffer;
use windfall_ipc::{AudioEditOperation, AudioEditRequest, RecordingSource, SliceOptions};
use windfall_project::{
    ChannelId, ClipId, Command, PluginBinding, PluginParameter, PluginTarget, SamplePath,
    SettingsPatch, file,
};

fn tone(path: &str, value: f32) {
    fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
    windfall_codec::write_wav(
        path,
        &AudioBuffer::from_interleaved(48000, 2, vec![value; 48000]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
}
fn edit(rig: &Rig) {
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("Concurrent edit".into()),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
}
struct Capture;
impl CaptureHandle for Capture {
    fn frames(&self) -> u64 {
        480
    }
    fn failed(&self) -> bool {
        false
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Ok(480)
    }
}
pub(super) fn start_recording(rig: &Rig) {
    start_recording_session(&rig.session);
}
pub(super) fn start_recording_session(session: &crate::session::Session) {
    session
        .recording_start_with(
            RecordingSource {
                host: "Fake".into(),
                device: "Fake".into(),
                left: 0,
                right: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
                armed_tracks: None,
                mixer_tap: None,
            },
            960,
            None,
            |_, _, mut sink: Sink| {
                sink(&[0.25; 960])?;
                Ok(Box::new(Capture))
            },
        )
        .unwrap();
}
#[test]
fn all_audio_origins_edited_and_sliced_sources_survive_removal_and_archive_open() {
    let mut rig = Rig::new();
    let private_factory = rig.folder.path().join("factory-copy");
    for sample in &rig.project().samples {
        if let SamplePath::Factory(relative) = &sample.path {
            let target = private_factory.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(rig.session.factory_dir().join(relative), target).unwrap();
        }
    }
    rig.session = crate::session::Session::new(crate::session::SessionConfig {
        controller: rig.session.controller().clone(),
        audio: rig.device.clone(),
        events: rig.events.clone(),
        factory_dir: private_factory,
        settings: crate::settings::SettingsStore::load(
            rig.folder.path().join(crate::settings::SETTINGS_FILE),
        ),
    });
    rig.session.project_save(Some(&rig.file("Song"))).unwrap();
    // Relative nested project source, external source of the same basename,
    // Unicode metadata, real recorded and audio-edited sources plus factory kit.
    let nested = rig.file("nested/音 space/tone.wav");
    let external_dir = tempfile::tempdir().unwrap();
    let external = external_dir
        .path()
        .join("tone.wav")
        .to_string_lossy()
        .into_owned();
    tone(&nested, 0.2);
    tone(&external, 0.4);
    rig.session.add_channel_from_file(&nested, None).unwrap();
    let result = rig
        .session
        .add_audio_clip_from_file(
            &external,
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let clip = ClipId(*result.created.last().unwrap());
    let preview = rig.session.audio_editor_open(clip).unwrap();
    rig.session
        .audio_editor_apply(AudioEditRequest {
            token: preview.token,
            operation: AudioEditOperation::Reverse,
            start_frame: 0,
            end_frame: 4800,
        })
        .unwrap();
    let clip = rig.project().playlist.clips[0].id;
    let slices = rig
        .session
        .slice_analyze(
            clip,
            SliceOptions::Grid {
                grid_ticks: windfall_core::PPQ / 4,
            },
        )
        .unwrap();
    rig.session
        .slice_apply(
            slices.token,
            slices.analysis.markers.iter().map(|m| m.tick).collect(),
        )
        .unwrap();
    start_recording(&rig);
    rig.session.recording_stop().unwrap();
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(windfall_ipc::PlayMode::Song),
            loop_song: Some(true),
            ..Default::default()
        })
        .unwrap();
    let before = rig.session.document_snapshot();
    let buffers: Vec<_> = before
        .project
        .samples
        .iter()
        .map(|s| {
            (
                s.id,
                rig.session
                    .state()
                    .pool
                    .get(s.id)
                    .unwrap()
                    .samples()
                    .to_vec(),
            )
        })
        .collect();
    let archive = rig
        .session
        .project_archive_save(&rig.file("portable.zip"))
        .unwrap();
    assert_eq!(
        rig.session.document_snapshot(),
        before,
        "packaging must preserve live paths/history/dirty"
    );
    // Factory content is a private fixture copy, safe to remove as well.
    for sample in &before.project.samples {
        let path = file::resolve_sample_path(
            &sample.path,
            Some(rig.folder.path()),
            rig.session.factory_dir(),
        )
        .unwrap();
        let _ = fs::remove_file(path);
    }
    rig.session.project_new().unwrap();
    let loaded = rig.session.project_open(&archive).unwrap();
    assert_eq!(loaded.path, None);
    assert!(!loaded.dirty);
    assert_eq!(
        rig.session.transport_state().mode,
        windfall_ipc::PlayMode::Song
    );
    assert!(rig.session.transport_state().loop_song);
    assert_eq!(loaded.project.playlist, before.project.playlist);
    assert_eq!(loaded.project.channels, before.project.channels);
    for (id, bytes) in buffers {
        assert!(rig.has_audio(id));
        assert_eq!(rig.session.state().pool.get(id).unwrap().samples(), bytes);
        let sample = loaded.project.samples.iter().find(|s| s.id == id).unwrap();
        assert!(matches!(sample.path, SamplePath::Project(_)));
    }
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12000)) > 0.0);
    let saved = rig
        .session
        .project_save(Some(&rig.file("reopened/Song")))
        .unwrap();
    rig.session.project_open(&saved).unwrap();
    assert!(rig.project().samples.iter().all(|s| rig.has_audio(s.id)));
}
#[test]
fn missing_plugins_retain_opaque_state_and_parameters_without_bundling_binaries() {
    let rig = Rig::new();
    let plugin = PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
        target: PluginTarget::Instrument {
            channel: ChannelId(0),
        },
        format: "vst3".into(),
        path: rig.file("missing.vst3"),
        id: "missing".into(),
        name: "Unavailable".into(),
        state: vec![0, 255, 7, 23],
        parameters: vec![PluginParameter {
            id: 7,
            name: "Gain".into(),
            min: 0.0,
            max: 1.0,
            value: 0.75,
            stepped: false,
            read_only: false,
            automatable: true,
        }],
    };
    rig.session
        .dispatch(Command::AddPluginInstrument { plugin }, None)
        .unwrap();
    let captured = rig.project().plugins;
    let archive = rig
        .session
        .project_archive_save(&rig.file("missing-plugin.zip"))
        .unwrap();
    rig.session.project_new().unwrap();
    let opened = rig.session.project_open(&archive).unwrap();
    assert_eq!(opened.project.plugins, captured);
}
#[test]
fn decoded_audio_has_individual_and_aggregate_limits_without_changing_live_pool() {
    let rig = Rig::new();
    let project = rig.project();
    let before = rig.session.document_snapshot();
    let job = rig.session.archive_job().unwrap();
    let decoded = rig
        .session
        .archive_audio(
            &project,
            None,
            &job,
            crate::session::archive::AudioLimits {
                entry: 1,
                total: u64::MAX,
            },
        )
        .unwrap();
    assert_eq!(decoded.failed.len(), project.samples.len());
    assert!(decoded.loaded.is_empty());
    assert!(
        decoded
            .warnings
            .iter()
            .all(|warning| warning.contains("1 bytes per source"))
    );
    let first = &project.samples[0];
    let path = file::resolve_sample_path(&first.path, None, rig.session.factory_dir()).unwrap();
    let bytes = std::mem::size_of_val(windfall_codec::decode_file(path).unwrap().samples()) as u64;
    let decoded = rig
        .session
        .archive_audio(
            &project,
            None,
            &job,
            crate::session::archive::AudioLimits {
                entry: u64::MAX,
                total: bytes,
            },
        )
        .unwrap();
    assert_eq!(decoded.loaded.len(), 1);
    assert!(decoded.loaded.contains(&first.id));
    assert_eq!(decoded.failed.len(), project.samples.len() - 1);
    assert_eq!(rig.session.document_snapshot(), before);
}
#[test]
fn invalid_missing_and_failed_archives_leave_document_untouched() {
    let rig = Rig::new();
    fs::write(rig.file("bad.zip"), b"not a zip").unwrap();
    let before = rig.session.document_snapshot();
    assert!(rig.session.project_open(&rig.file("bad.zip")).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(!rig.folder.path().join("projects").exists());
    rig.session
        .dispatch(
            Command::AddSample {
                name: "Lost take".into(),
                path: SamplePath::External(rig.file("missing.wav")),
            },
            None,
        )
        .unwrap();
    let before = rig.session.document_snapshot();
    let error = rig
        .session
        .project_archive_save(&rig.file("missing.zip"))
        .unwrap_err();
    assert!(error.contains("Lost take"));
    assert!(!Path::new(&rig.file("missing.zip")).exists());
    assert_eq!(rig.session.document_snapshot(), before);
    let invalid = Rig::new();
    fs::write(
        invalid.file("not-audio.wav"),
        b"not audio or a bundled plugin",
    )
    .unwrap();
    invalid
        .session
        .dispatch(
            Command::AddSample {
                name: "Unreadable source".into(),
                path: SamplePath::External(invalid.file("not-audio.wav")),
            },
            None,
        )
        .unwrap();
    let before = invalid.session.document_snapshot();
    assert!(
        invalid
            .session
            .project_archive_save(&invalid.file("bad-audio.zip"))
            .unwrap_err()
            .contains("Unreadable source")
    );
    assert_eq!(invalid.session.document_snapshot(), before);
    assert!(!Path::new(&invalid.file("bad-audio.zip")).exists());
    // A structurally valid ZIP can still have unusable sample content. Reject
    // it before installation and clean only its managed extraction.
    windfall_archive::create(
        &invalid.project(),
        None,
        None,
        invalid.session.factory_dir(),
        Path::new(&invalid.file("unreadable.zip")),
        windfall_archive::Limits::default(),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert!(
        invalid
            .session
            .project_open(&invalid.file("unreadable.zip"))
            .unwrap_err()
            .contains("unreadable audio")
    );
    assert_eq!(invalid.session.document_snapshot(), before);
    assert_eq!(
        fs::read_dir(invalid.folder.path().join("projects"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn replaced_edited_or_recording_document_refuses_pending_open_and_cleans_extraction() {
    for stage in ["archive:ready", "archive:install"] {
        for action in ["replace", "edit", "record", "cancel", "reload"] {
            let rig = Rig::new();
            let archive = rig
                .session
                .project_archive_save(&rig.file("Song.zip"))
                .unwrap();
            let held = rig.session.hold(stage);
            let work = rig.session.background(move |s| s.project_open(&archive));
            held.wait();
            match action {
                "replace" => {
                    rig.session.project_new().unwrap();
                }
                "edit" => edit(&rig),
                "record" => start_recording(&rig),
                "cancel" => rig.session.project_archive_cancel(),
                "reload" => {
                    rig.session.samples_reload();
                }
                _ => unreachable!(),
            }
            let before = rig.session.document_snapshot();
            held.release();
            let result = work.join().unwrap();
            let roots = fs::read_dir(rig.folder.path().join("projects"))
                .unwrap()
                .count();
            if action == "reload" {
                assert!(result.is_ok(), "reload does not edit musical state");
                assert_eq!(roots, 1);
                assert!(rig.project().samples.iter().all(|s| rig.has_audio(s.id)));
            } else {
                assert!(result.is_err(), "{action}");
                assert_eq!(rig.session.document_snapshot(), before);
                assert_eq!(roots, 0);
            }
            if action == "record" {
                rig.session.recording_cancel();
            }
        }
    }
}
#[test]
fn packaging_cancellation_and_capture_recording_guard_preserve_live_state() {
    let rig = Rig::new();
    let queued = rig.session.archive_job().unwrap();
    rig.session.project_archive_cancel();
    assert!(
        rig.session
            .project_archive_save_with(&rig.file("queued.zip"), queued)
            .unwrap_err()
            .contains("cancelled")
    );
    assert!(!Path::new(&rig.file("queued.zip")).exists());
    for stage in ["archive:capture", "archive:publish"] {
        let rig = Rig::new();
        let target = rig.file("Song.zip");
        let held = rig.session.hold(stage);
        let request_target = target.clone();
        let work = rig
            .session
            .background(move |s| s.project_archive_save(&request_target));
        held.wait();
        assert!(
            rig.session
                .project_archive_save(&rig.file("second.zip"))
                .is_err()
        );
        rig.session.project_archive_cancel();
        edit(&rig);
        let before = rig.session.document_snapshot();
        held.release();
        assert!(work.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(!Path::new(&target).exists());
        assert_eq!(rig.session.document_snapshot(), before);
    }
    let rig = Rig::new();
    let held = rig.session.hold("archive:capture");
    let publishing = rig.session.hold("archive:publish");
    let target = rig.file("recording.zip");
    let work = rig
        .session
        .background(move |s| s.project_archive_save(&target));
    held.wait();
    let recording = rig.session.background(start_recording_session);
    assert!(
        still_running(&recording),
        "recording must wait for selected-owner capture"
    );
    held.release();
    publishing.wait();
    recording.join().unwrap();
    assert!(rig.session.recording_state().active);
    publishing.release();
    work.join().unwrap().unwrap();
    assert!(
        rig.session
            .project_save_new_version(Some(&rig.file("take")))
            .is_err()
    );
    assert!(
        rig.session
            .project_archive_save(&rig.file("take.zip"))
            .is_err()
    );
    rig.session.recording_cancel();
}

#[test]
fn captured_package_finishes_without_mutating_a_concurrent_edit_replacement_or_reload() {
    for action in ["edit", "replace", "reload"] {
        let rig = Rig::new();
        let original = rig.project();
        let target = rig.file("snapshot.zip");
        let held = rig.session.hold("archive:publish");
        let job_target = target.clone();
        let work = rig
            .session
            .background(move |s| s.project_archive_save(&job_target));
        held.wait();
        match action {
            "edit" => edit(&rig),
            "replace" => {
                rig.session.project_new().unwrap();
                edit(&rig);
            }
            "reload" => {
                rig.session.samples_reload();
            }
            _ => unreachable!(),
        }
        let before = rig.session.document_snapshot();
        held.release();
        work.join().unwrap().unwrap();
        assert_eq!(rig.session.document_snapshot(), before);
        let extracted = windfall_archive::extract(
            Path::new(&target),
            &rig.folder.path().join("inspect"),
            windfall_archive::Limits::default(),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(extracted.project.settings, original.settings);
        assert_eq!(extracted.project.channels, original.channels);
    }
}
#[cfg(windows)]
#[test]
fn selected_native_plugin_capture_archive_and_export_round_trip() {
    let mut rig = Rig::new();
    let (manager, path) = super::plugins::manager(&rig);
    let binding = manager
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
            .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target: PluginTarget::Instrument { channel },
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
    let captured = manager.runtime.capture(rig.project()).unwrap().plugins;
    let version = rig
        .session
        .project_save_new_version(Some(&rig.file("native.windfall")))
        .unwrap();
    assert_eq!(file::load(version).unwrap().plugins, captured);
    let archive = rig
        .session
        .project_archive_save(&rig.file("native.zip"))
        .unwrap();
    rig.session.project_open(&archive).unwrap();
    rig.run(256);
    assert_eq!(rig.project().plugins, captured);
    let options = windfall_ipc::ExportOptions {
        path: rig.file("native.wav"),
        mode: windfall_ipc::PlayMode::Pattern,
        tail_secs: 0.0,
        ..Default::default()
    };
    rig.session.export_audio(options.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert!(rms(windfall_codec::decode_file(options.path).unwrap().samples()) > 0.01);
    assert_eq!(
        manager.runtime.capture(rig.project()).unwrap().plugins,
        captured
    );
}
