//! Real native owners with a headless device that executes the production
//! stream close/reopen protocol. No hardware or mocked plugin state.
use super::*;
use windfall_engine::{Controller, Processor};
use windfall_plugin_host::{EditorOptions, PluginHost, gui};
use windfall_project::{ChannelId, EffectId, NoteInit, PluginTarget, TrackId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, GW_CHILD, GW_OWNER, GetDlgItem, GetWindow, IsWindow, PostMessageW,
};

struct Device {
    controller: Controller,
    processor: Mutex<Option<Processor>>,
    status: Mutex<EngineStatus>,
    paced: bool,
}
impl AudioDevice for Device {
    fn suspend(&self) -> Result<(), String> {
        self.controller
            .suspend_stream(|| {
                lock(&self.processor).take();
            })
            .map_err(|error| error.to_string())?;
        lock(&self.status).running = false;
        Ok(())
    }
    fn reconfigure(&self, settings: &AudioSettings) {
        *lock(&self.processor) = Some(Processor::reopen(&self.controller, SAMPLE_RATE));
        *lock(&self.status) = FakeDevice::describe(settings);
    }
    fn status(&self) -> EngineStatus {
        lock(&self.status).clone()
    }
    fn devices(&self) -> Vec<AudioHost> {
        Vec::new()
    }
}
impl Device {
    fn run(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        if let Some(processor) = lock(&self.processor).as_mut() {
            for block in out.chunks_mut(256) {
                processor.process(block);
                if self.paced {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        out
    }
    fn finish<T>(&self, task: JoinHandle<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        while !task.is_finished() {
            assert!(
                Instant::now() < deadline,
                "native update worker never completed"
            );
            self.run(128);
            gui::pump_events(Some(Duration::from_millis(1)));
        }
        task.join().unwrap()
    }
}
struct Native {
    session: Session,
    device: Arc<Device>,
    runtime: Arc<crate::plugins::Runtime>,
    folder: TempDir,
    effect: PluginTarget,
    channel: ChannelId,
    // Native parent stays alive on its creating test GUI thread.
    parent: windfall_plugin_host::PluginInstance,
    owner: usize,
}
impl Native {
    fn new() -> Self {
        Self::new_mode(false)
    }
    fn new_mode(bridged: bool) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let settings = SettingsStore::load(folder.path().join(SETTINGS_FILE));
        let (processor, controller) = Processor::new(SAMPLE_RATE);
        let device = Arc::new(Device {
            controller: controller.clone(),
            processor: Mutex::new(Some(processor)),
            status: Mutex::new(FakeDevice::describe(&settings.settings().audio)),
            paced: bridged,
        });
        let session = Session::new(SessionConfig {
            controller,
            audio: device.clone(),
            events: Arc::new(Recorder::default()),
            factory_dir: PathBuf::from(DEV_FACTORY_DIR),
            settings,
        });
        let (mut runtime, mut effect) = crate::plugins::vst3_fixture(0);
        let (_, instrument) = crate::plugins::vst3_fixture(2);
        if bridged {
            let helper = std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../../target/debug/windfall-desktop.exe")
                });
            assert!(
                helper.is_file(),
                "integrator must supply the prebuilt desktop helper"
            );
            runtime = crate::plugins::Runtime::bridge_fixture(&helper);
        }
        let entries: Vec<_> = [&effect, &instrument]
            .into_iter()
            .map(|binding| windfall_ipc::PluginEntry {
                path: binding.path.clone(),
                id: binding.id.clone(),
                format: binding.format.clone(),
                name: binding.name.clone(),
                vendor: "Fixture".into(),
                instrument: binding == &instrument,
                usable: true,
                error: None,
            })
            .collect();
        runtime.approve(&entries);
        let module = PluginHost::windfall()
            .load(std::path::Path::new(&effect.path))
            .unwrap();
        let mut parent = module.create(&effect.id).unwrap();
        let owner = parent
            .open_editor(&EditorOptions {
                take_focus: false,
                ..Default::default()
            })
            .unwrap()
            .window
            .unwrap();
        effect.wrapper.detached = false;
        session.install_plugins(crate::plugins::PluginManager::fixture_runtime(
            folder.path(),
            runtime.clone(),
        ));
        let channel = ChannelId(
            session
                .dispatch(Command::AddPluginInstrument { plugin: instrument }, None)
                .unwrap()
                .created[0],
        );
        let added = session
            .dispatch(
                Command::AddPluginEffect {
                    track: TrackId(0),
                    plugin: effect,
                },
                None,
            )
            .unwrap();
        let effect = PluginTarget::Effect {
            effect: EffectId(*added.created.last().unwrap()),
        };
        device.run(512);
        let editor_runtime = runtime.clone();
        device.finish(std::thread::spawn(move || {
            editor_runtime.editor_binding_with_parent(effect, None, true, Some(owner))
        })).unwrap();
        Self {
            session,
            device,
            runtime,
            folder,
            effect,
            channel,
            parent,
            owner,
        }
    }
    fn capture(&self) -> Project {
        let runtime = self.runtime.clone();
        let project = self.session.document_snapshot().project;
        self.device
            .finish(std::thread::spawn(move || runtime.capture(project)))
            .unwrap()
    }
    fn editor_window(&self) -> usize {
        self.runtime.update_test_editor_window(self.effect).unwrap()
    }
    fn assert_editor_owner(&self) {
        let window = self.editor_window();
        assert_ne!(unsafe { IsWindow(window as *mut _) }, 0);
        assert_eq!(
            unsafe { GetWindow(window as *mut _, GW_OWNER) } as usize,
            self.owner
        );
    }
    fn dirty_editor(&self) {
        let window = self.editor_window();
        let button = unsafe { GetDlgItem(GetWindow(window as *mut _, GW_CHILD), 7) };
        assert!(!button.is_null());
        assert_ne!(unsafe { PostMessageW(button, BM_CLICK, 0, 0) }, 0);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if self
                .runtime
                .drain()
                .iter()
                .any(|request| matches!(request.update, crate::plugins::Update::Capture { .. }))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real native dirty notification missing"
            );
            self.device.run(128);
            gui::pump_events(Some(Duration::from_millis(2)));
        }
    }
    fn sequence(&self) {
        let pattern = self.session.document_snapshot().project.patterns[0].id;
        self.session
            .dispatch(
                Command::AddNotes {
                    pattern,
                    channel: self.channel,
                    notes: vec![NoteInit {
                        start: 0,
                        length: 1920,
                        key: 69,
                        velocity: Some(1.0),
                        pan: None,
                        expression: None,
                    }],
                },
                None,
            )
            .unwrap();
        self.session
            .transport_set(windfall_ipc::TransportPatch {
                mode: Some(windfall_ipc::PlayMode::Pattern),
                pattern: Some(pattern),
                ..Default::default()
            })
            .unwrap();
        self.session.transport_play().unwrap();
        assert!(rms(&self.device.run(4800)) > 0.01);
    }
    fn begin(&self) -> crate::session::UpdateShutdown {
        let session = self.session.clone();
        self.device
            .finish(std::thread::spawn(move || session.begin_update_shutdown()))
            .unwrap()
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.device.suspend().unwrap();
        let runtime = self.runtime.clone();
        self.device.finish(std::thread::spawn(move || runtime.update_shutdown_barrier())).unwrap();
        self.parent.close_editor();
    }
}

#[test]
fn native_cancel_at_every_shutdown_stage_and_installer_failure_restore_exact_state_and_editor_owner()
 {
    for bridged in [false, true] {
        for stage in [
            "frozen",
            "editors-retained",
            "editors-closed",
            "captured",
            "published",
            "monitoring-stopped",
            "audio-suspended",
            "owners-retired",
            "installer-failed",
        ] {
            let native = Native::new_mode(bridged);
            native.sequence();
            native.session.audition_note_on(native.channel, 72, 0.8);
            native.device.run(512);
            let expected = native.capture();
            let session = native.session.clone();
            let result = native.device.finish(std::thread::spawn(move || {
                if stage == "installer-failed" {
                    let mut shutdown = session.begin_update_shutdown()?;
                    shutdown.quiesce()?;
                    assert!(!session.engine_status().running);
                    drop(shutdown); // exact installer-refusal RAII path
                    Ok(())
                } else {
                    session
                        .begin_update_shutdown_at(|current| {
                            if current == stage {
                                Err(format!("cancel at {stage}"))
                            } else {
                                Ok(())
                            }
                        })
                        .map(|_| ())
                }
            }));
            if stage == "installer-failed" {
                result.unwrap();
            } else {
                assert!(result.unwrap_err().contains(stage));
            }
            assert!(native.device.status().running, "{stage}");
            native.assert_editor_owner();
            let restored = native.capture();
            for before in expected.plugin_bindings() {
                assert_eq!(
                    restored.plugin(before.target).unwrap().state,
                    before.state,
                    "opaque bytes changed at {stage}"
                );
            }
            assert!(
                native.session.transport_state().playing,
                "playing intent lost at {stage}"
            );
            assert!(
                rms(&native.device.run(4800)) > 0.01,
                "sequenced playback failed at {stage}"
            );
            native.session.transport_stop();
            native.device.run(4800);
            assert!(
                rms(&native.device.run(4800)) < 1e-6,
                "stuck held or sequenced notes at {stage}"
            );
            native.session.dispatch(edit(), None).unwrap();
        }
    }
}

#[test]
fn native_failed_install_does_not_resurrect_live_notes_or_override_a_user_stop() {
    for bridged in [false, true] {
        for playing in [false, true] {
            for user_stopped in [false, true] {
                let native = Native::new_mode(bridged);
                if playing {
                    native.sequence();
                }
                native.session.audition_note_on(native.channel, 72, 1.0);
                assert!(rms(&native.device.run(4800)) > 0.01);
                let shutdown = native.begin();
                assert!(!native.device.status().running);
                assert!(
                    native
                        .runtime
                        .update_editors(Duration::from_secs(1))
                        .unwrap()
                        .is_empty()
                );
                if user_stopped {
                    native.session.transport_stop();
                }
                native.device.finish(std::thread::spawn(move || shutdown.rollback())).unwrap();
                native.assert_editor_owner();
                let pcm = native.device.run(4800);
                assert_eq!(
                    native.session.transport_state().playing,
                    playing && !user_stopped
                );
                if playing && !user_stopped {
                    assert!(rms(&pcm) > 0.01);
                } else {
                    assert!(rms(&pcm) < 1e-6, "held live note was resurrected");
                }
                native.session.transport_stop();
                native.device.run(4800);
                assert!(rms(&native.device.run(4800)) < 1e-6);
            }
        }
    }
}

#[test]
fn native_unpublished_dirty_state_is_in_the_prompt_save_and_cancel_history() {
    for decision in ["save", "discard", "cancel"] {
        let native = Native::new();
        native.session.state().document.mark_saved();
        let before = native.session.document_snapshot();
        assert!(!before.dirty);
        native.dirty_editor();
        assert_eq!(
            native.session.document_snapshot(),
            before,
            "editor state has not been published"
        );
        let expected = native.capture();
        let shutdown = native.begin();
        let frozen = native.session.document_snapshot();
        assert!(frozen.dirty, "{decision} prompt must see native dirtiness");
        assert_eq!(
            frozen.project.plugin(native.effect).unwrap().state,
            expected.plugin(native.effect).unwrap().state
        );
        assert_eq!(
            frozen
                .project
                .plugin(native.effect)
                .unwrap()
                .parameters
                .iter()
                .find(|param| param.id == 7)
                .unwrap()
                .value,
            0.25
        );
        assert_eq!(
            frozen.history.entries.len(),
            before.history.entries.len() + 1
        );
        if decision == "save" {
            let path = native.folder.path().join("captured.windfall");
            shutdown.save(Some(path.to_str().unwrap())).unwrap();
            let saved = windfall_project::file::load(&path).unwrap();
            assert_eq!(
                saved.plugin(native.effect).unwrap().state,
                expected.plugin(native.effect).unwrap().state
            );
            assert_eq!(
                saved.plugin(native.effect).unwrap().parameters,
                frozen.project.plugin(native.effect).unwrap().parameters
            );
            assert!(!native.session.document_snapshot().dirty);
        }
        native.device.finish(std::thread::spawn(move || shutdown.rollback())).unwrap(); // cancel, or rollback after discard/installer failure
        native.assert_editor_owner();
        let recovered = native.capture();
        assert_eq!(
            recovered.plugin(native.effect).unwrap().state,
            expected.plugin(native.effect).unwrap().state
        );
        native.session.undo().unwrap();
        assert_eq!(native.session.document_snapshot().project, before.project);
        native.device.run(512);
        native.session.redo().unwrap();
        assert_eq!(native.session.document_snapshot().project, frozen.project);
    }
}

#[test]
fn native_inflight_capture_is_joined_before_returning_the_unsaved_decision() {
    let native = Native::new();
    native.session.state().document.mark_saved();
    native.dirty_editor();
    let hold = native.session.hold("update-test:native-captured");
    let runtime = native.runtime.clone();
    let session = native.session.clone();
    let capturing = std::thread::spawn(move || {
        session.while_recording_idle(|| {
            let project = runtime.capture(session.document_snapshot().project)?;
            session.pause("update-test:native-captured");
            Ok(project)
        })
    });
    let deadline = Instant::now() + PATIENCE;
    while !lock(&native.session.inner.pauses.points)
        .get("update-test:native-captured")
        .is_some_and(|stage| *stage == Stage::Reached)
    {
        assert!(Instant::now() < deadline);
        native.device.run(128);
        std::thread::sleep(Duration::from_millis(1));
    }
    let session = native.session.clone();
    let beginning = std::thread::spawn(move || session.begin_update_shutdown());
    assert!(still_running(&beginning));
    hold.release();
    let expected = native.device.finish(capturing).unwrap();
    let shutdown = native.device.finish(beginning).unwrap();
    let snapshot = native.session.document_snapshot();
    assert!(snapshot.dirty);
    assert_eq!(
        snapshot.project.plugin(native.effect).unwrap().state,
        expected.plugin(native.effect).unwrap().state
    );
    native.device.finish(std::thread::spawn(move || shutdown.rollback())).unwrap();
}

#[test]
fn unanswered_native_helper_capture_has_a_bounded_visible_refusal_before_the_prompt() {
    let mut rig = Rig::new();
    let (_, mut binding) = crate::plugins::vst3_fixture(14); // native getState never answers
    binding.wrapper.processing = windfall_project::PluginProcessingMode::Bridged;
    let helper = std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/debug/windfall-desktop.exe")
        });
    assert!(
        helper.is_file(),
        "integrator must supply the prebuilt desktop helper"
    );
    let runtime = crate::plugins::Runtime::bridge_fixture(&helper);
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: binding.path.clone(),
        id: binding.id.clone(),
        format: binding.format.clone(),
        name: binding.name.clone(),
        vendor: "Fixture".into(),
        instrument: false,
        usable: true,
        error: None,
    }]);
    rig.session
        .install_plugins(crate::plugins::PluginManager::fixture_runtime(
            rig.folder.path(),
            runtime.clone(),
        ));
    let added = rig
        .session
        .dispatch(
            Command::AddPluginEffect {
                track: TrackId(0),
                plugin: binding,
            },
            None,
        )
        .unwrap();
    let target = PluginTarget::Effect {
        effect: EffectId(*added.created.last().unwrap()),
    };
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 7,
                value: 0.875,
            },
            None,
        )
        .unwrap();
    rig.run(1024);
    let before = rig.session.document_snapshot();
    let started = Instant::now();
    let task = rig
        .session
        .background(|session| session.begin_update_shutdown());
    while !task.is_finished() {
        assert!(started.elapsed() < Duration::from_secs(9));
        rig.run(128);
        std::thread::sleep(Duration::from_millis(1));
    }
    let error = match task.join().unwrap() {
        Ok(_) => panic!("unanswered native state admitted an unsaved decision"),
        Err(error) => error,
    };
    assert!(
        error.contains("could not capture current native state"),
        "{error}"
    );
    assert!(started.elapsed() < Duration::from_secs(9));
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.device.status().running);
    assert!(
        !rig.session
            .inner
            .update_installing
            .load(std::sync::atomic::Ordering::Acquire)
    );
    rig.session.dispatch(edit(), None).unwrap();
}
