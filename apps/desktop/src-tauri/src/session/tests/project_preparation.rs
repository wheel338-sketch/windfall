//! Actual Session admission, using a synchronous scripted native constructor.
use super::Rig;
use crate::session::{Session, WeakSession};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory, PluginTransport};
use windfall_project::{
    Command, Document, EffectId, PluginBinding, PluginParameter, PluginTarget, TrackId,
};

#[derive(Default)]
struct Evidence {
    revision: AtomicU64,
    made: AtomicUsize,
    selected: AtomicUsize,
    dropped: AtomicUsize,
    fail: AtomicBool,
    panic_at: AtomicUsize,
    settle_loads: AtomicUsize,
}
struct Factory {
    session: WeakSession,
    evidence: Arc<Evidence>,
}
impl std::fmt::Debug for Factory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session native proof provider")
    }
}
fn unlocked(session: &Session) {
    assert!(
        !matches!(
            session.inner.recording.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ),
        "native work under recording guard"
    );
    assert!(
        !matches!(
            session.inner.state.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ),
        "native work under document guard"
    );
    // A held controller guard would prevent this ordinary control query.
    session.controller().transport();
}
struct Unit {
    session: WeakSession,
    evidence: Arc<Evidence>,
}
impl Drop for Unit {
    fn drop(&mut self) {
        if let Some(session) = self.session.upgrade() {
            unlocked(&session);
        }
        self.evidence.dropped.fetch_add(1, Ordering::Relaxed);
    }
}
impl HostedEffect for Unit {
    fn process(&mut self, _: &mut [f32], _: &mut [f32]) {}
    fn transport(&mut self, _: PluginTransport) {
        self.evidence.selected.fetch_add(1, Ordering::Relaxed);
    }
    fn set_param(&mut self, _: u32, _: f32) {}
    fn set_tempo(&mut self, _: f32) {}
    fn latency(&self) -> usize {
        37
    }
    fn tail(&self) -> usize {
        0
    }
}
impl PluginFactory for Factory {
    fn provider_identity(&self) -> u64 {
        Arc::as_ptr(&self.evidence) as usize as u64
    }
    fn revision(&self) -> u64 {
        self.evidence.revision.load(Ordering::Relaxed)
    }
    fn effect(
        &self,
        _: &PluginBinding,
        rate: u32,
        block: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        assert_eq!((rate, block), (48_000, 256));
        let session = self.session.upgrade().unwrap();
        unlocked(&session);
        let made = self.evidence.made.fetch_add(1, Ordering::Relaxed) + 1;
        session.pause("project:native-constructor");
        unlocked(&session);
        let left = self.evidence.settle_loads.load(Ordering::Relaxed);
        if left != 0
            && self
                .evidence
                .settle_loads
                .compare_exchange(left, left - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            // Model one completed pending decoder per off-guard constructor;
            // the separate test above exercises the actual file worker.
            let mut state = session.state();
            let id = *state.loading.iter().next().expect("pending decoder");
            state.loading.remove(&id);
        }
        assert_ne!(
            self.evidence.panic_at.load(Ordering::Relaxed),
            made,
            "scripted constructor unwind"
        );
        if self.evidence.fail.load(Ordering::Relaxed) {
            return Err("scripted authenticated helper load failed".into());
        }
        Ok(Box::new(Unit {
            session: session.downgrade(),
            evidence: self.evidence.clone(),
        }))
    }
    fn instrument(
        &self,
        _: &PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn HostedInstrument>, String> {
        Err("effect-only proof provider".into())
    }
}
fn factory(rig: &Rig) -> Arc<Evidence> {
    let evidence = Arc::new(Evidence::default());
    rig.session
        .state()
        .pool
        .set_plugin_factory(Arc::new(Factory {
            session: rig.session.downgrade(),
            evidence: evidence.clone(),
        }));
    evidence
}
fn add() -> Command {
    Command::AddPluginEffect {
        track: TrackId(0),
        plugin: PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            wrapper: Default::default(),
            target: PluginTarget::Effect {
                effect: EffectId(0),
            },
            format: "clap".into(),
            path: "scripted.clap".into(),
            id: "proof".into(),
            name: "Proof".into(),
            state: vec![],
            parameters: vec![PluginParameter {
                id: 1,
                name: "Level".into(),
                min: 0.0,
                max: 1.0,
                value: 0.5,
                stepped: false,
                read_only: false,
                automatable: true,
            }],
        },
    }
}
fn name() -> Command {
    Command::UpdateSettings {
        patch: windfall_project::SettingsPatch {
            name: Some("concurrent edit".into()),
            ..Default::default()
        },
    }
}

#[test]
fn p1_session_blocked_constructor_leaves_control_document_and_recording_available_and_refuses_stale_commit()
 {
    for interrupt in [
        "edit",
        "new",
        "open",
        "revision",
        "provider",
        "source",
        "recording",
        "stream",
    ] {
        let mut rig = Rig::new();
        let evidence = factory(&rig);
        let hold = rig.session.hold("project:native-constructor");
        let worker = rig
            .session
            .background(|session| session.dispatch(add(), None));
        hold.wait();
        unlocked(&rig.session);
        rig.session.realtime_tick();
        assert!(!rig.session.recording_state().active);
        match interrupt {
            "edit" => {
                rig.session.dispatch(name(), None).unwrap();
            }
            "new" => {
                rig.session.project_new().unwrap();
            }
            "open" => {
                let path = rig.file("other.windfall");
                std::fs::write(
                    &path,
                    windfall_project::file::to_json(&rig.project()).unwrap(),
                )
                .unwrap();
                rig.session.project_open(&path).unwrap();
            }
            "revision" => {
                evidence.revision.fetch_add(1, Ordering::Relaxed);
            }
            "provider" => {
                factory(&rig);
            }
            "source" => {
                rig.session.state().pool.insert(
                    windfall_project::SampleId(999),
                    windfall_core::AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 10]),
                );
            }
            "recording" => {
                struct Capture;
                impl crate::session::recording::CaptureHandle for Capture {
                    fn frames(&self) -> u64 {
                        1
                    }
                    fn failed(&self) -> bool {
                        false
                    }
                    fn finish(self: Box<Self>) -> Result<u64, String> {
                        Ok(1)
                    }
                }
                rig.session
                    .recording_start_with(
                        windfall_ipc::RecordingSource {
                            host: "Fake".into(),
                            device: "Fake".into(),
                            left: 0,
                            right: None,
                            armed_tracks: None,
                            mixer_tap: None,
                            alignment: None,
                            loop_recording: None,
                            monitor: None,
                        },
                        0,
                        None,
                        |_, _, mut sink| {
                            sink(&[0.25, 0.25])?;
                            Ok(Box::new(Capture))
                        },
                    )
                    .unwrap();
            }
            "stream" => {
                let (replacement, _) = windfall_engine::Processor::new(48_000);
                rig.processor = replacement; // actual old consumer abandonment
            }
            _ => unreachable!(),
        }
        let document = rig.session.document_snapshot();
        let pool = rig.session.state().pool.clone();
        let transport = rig.session.controller().transport();
        hold.release();
        assert!(worker.join().unwrap().is_err(), "{interrupt}");
        assert_eq!(rig.session.document_snapshot(), document, "{interrupt}");
        assert!(rig.session.state().pool.same_sources(&pool), "{interrupt}");
        assert_eq!(
            rig.session.controller().transport(),
            transport,
            "{interrupt}"
        );
        assert_eq!(
            evidence.selected.load(Ordering::Relaxed),
            0,
            "candidate selected: {interrupt}"
        );
        assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
        rig.session.recording_cancel();
    }
}

#[test]
fn p1_session_edits_history_refresh_and_retirement_use_actual_ready_owners() {
    let mut rig = Rig::new();
    let evidence = factory(&rig);
    let added = rig.session.dispatch(add(), None).unwrap();
    let target = PluginTarget::Effect {
        effect: EffectId(added.created[0]),
    };
    assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    rig.run(64);
    assert!(evidence.selected.load(Ordering::Relaxed) > 0);
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 1,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    rig.session.dispatch(name(), None).unwrap();
    rig.session.undo().unwrap();
    rig.session.redo().unwrap();
    assert_eq!(
        evidence.made.load(Ordering::Relaxed),
        1,
        "actual owner reused across checked edits/history"
    );
    evidence.revision.fetch_add(1, Ordering::Relaxed);
    rig.session.refresh_plugins().unwrap();
    assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
    rig.run(64);
    rig.session.realtime_tick();
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn p1_session_open_uses_replacement_intent_with_late_transport_and_true_readiness() {
    let mut rig = Rig::new();
    let evidence = factory(&rig);
    let mut document = Document::new(rig.project());
    document.dispatch(add(), None).unwrap();
    let path = rig.file("native.windfall");
    std::fs::write(
        &path,
        windfall_project::file::to_json_with(
            document.project(),
            Some(&windfall_project::ProjectSession {
                mode: windfall_project::PlayMode::Song,
                loop_song: true,
                pattern: Some(document.project().patterns[0].id),
            }),
        )
        .unwrap(),
    )
    .unwrap();
    let hold = rig.session.hold("project:native-constructor");
    let worker = rig
        .session
        .background(move |session| session.project_open(&path));
    hold.wait();
    rig.session.controller().play();
    rig.session.controller().seek(720.0);
    rig.session.controller().stop();
    rig.session.controller().play();
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    hold.release();
    let installed = worker.join().unwrap().unwrap();
    assert_eq!(installed.project, *document.project());
    let transport = rig.session.controller().transport();
    assert!(!transport.playing);
    assert!(transport.loop_song);
    assert_eq!(transport.mode, windfall_project::PlayMode::Song);
    assert_eq!(rig.session.controller().frame().tick, 0.0);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    rig.run(64);
    assert!(evidence.selected.load(Ordering::Relaxed) > 0);
}

#[test]
fn p1_session_native_error_and_meter_refusal_leave_document_pool_history_transport_unchanged() {
    let rig = Rig::new();
    let evidence = factory(&rig);
    evidence.fail.store(true, Ordering::Relaxed);
    let document = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    let transport = rig.session.controller().transport();
    let error = rig.session.dispatch(add(), None).unwrap_err();
    assert!(
        error.contains("Native plugin preparation failed"),
        "{error}"
    );
    assert_eq!(rig.session.document_snapshot(), document);
    assert!(rig.session.state().pool.same_sources(&pool));
    assert_eq!(rig.session.controller().transport(), transport);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
    // The engine typed-meter producer remains authoritative; checked Session
    // commands also reject invalid maps before actual Document dispatch.
    assert!(
        rig.session
            .dispatch(
                Command::UpdateSettings {
                    patch: windfall_project::SettingsPatch {
                        time_signature: Some(windfall_project::TimeSignature {
                            numerator: 0,
                            denominator: 4
                        }),
                        ..Default::default()
                    }
                },
                None
            )
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), document);
}

#[test]
fn p1_session_recording_finish_prepares_native_off_mutex_and_keeps_completion_exclusion() {
    struct Capture;
    impl crate::session::recording::CaptureHandle for Capture {
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
    let mut rig = Rig::new();
    let evidence = factory(&rig);
    rig.session.dispatch(add(), None).unwrap();
    rig.run(64);
    rig.session
        .recording_start_with(
            windfall_ipc::RecordingSource {
                host: "Fake".into(),
                device: "Fake".into(),
                left: 0,
                right: None,
                armed_tracks: None,
                mixer_tap: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
            },
            0,
            None,
            |_, _, mut sink| {
                sink(&[0.25; 960])?;
                Ok(Box::new(Capture))
            },
        )
        .unwrap();
    evidence.revision.fetch_add(1, Ordering::Relaxed);
    let before = rig.session.document_snapshot();
    let selected = evidence.selected.load(Ordering::Relaxed);
    let hold = rig.session.hold("project:native-constructor");
    let worker = rig.session.background(Session::recording_stop);
    hold.wait();
    unlocked(&rig.session);
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(
        rig.session
            .inner
            .recording_finishing
            .load(Ordering::Acquire)
    );
    assert!(
        rig.session
            .dispatch(name(), None)
            .unwrap_err()
            .contains("recording")
    );
    assert_eq!(evidence.selected.load(Ordering::Relaxed), selected);
    hold.release();
    let result = worker.join().unwrap().unwrap();
    assert!(!result.created.is_empty());
    assert!(
        !rig.session
            .inner
            .recording_finishing
            .load(Ordering::Acquire)
    );
    assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), selected);
    rig.run(64);
    rig.session.realtime_tick();
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn p1_session_constructor_and_post_install_unwind_keep_native_retirement_outside_poisoned_guards() {
    let rig = Rig::new();
    let evidence = factory(&rig);
    evidence.panic_at.store(2, Ordering::Relaxed);
    let before = rig.session.document_snapshot();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rig.session
                .dispatch(
                    Command::Batch {
                        label: None,
                        commands: vec![add(), add()],
                    },
                    None,
                )
                .unwrap();
        }))
        .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    assert!(!rig.session.inner.state.is_poisoned());
    assert!(!rig.session.inner.recording.is_poisoned());

    let mut rig = Rig::new();
    let evidence = factory(&rig);
    *crate::sync::lock(&rig.events.hook) = Some(Box::new(|event| {
        if matches!(event, crate::events::Event::ProjectPatch(_)) {
            panic!("notification unwind after guaranteed install");
        }
    }));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rig.session.dispatch(add(), None).unwrap();
        }))
        .is_err()
    );
    assert!(rig.session.inner.state.is_poisoned());
    assert!(rig.session.inner.recording.is_poisoned());
    let project = rig.project();
    assert_eq!(project.plugins.len(), 1);
    rig.run(64);
    assert!(
        evidence.selected.load(Ordering::Relaxed) > 0,
        "the committed document has its exact installed unit"
    );
    // Same-id native state replacement retires the old installed owner at
    // serial adoption, independently of a departing transfer's fade clock.
    rig.session
        .dispatch(
            Command::SetPluginState {
                target: PluginTarget::Effect {
                    effect: project.mixer.tracks[0].effects[0].id,
                },
                state: vec![1],
            },
            None,
        )
        .unwrap();
    rig.run(64);
    rig.session.realtime_tick();
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn p1_session_replace_full_one_short_and_exact_queue_space_admit_before_document_mutation() {
    for available in [0, 3, 4] {
        let mut rig = Rig::new();
        rig.run(1);
        rig.session.realtime_tick();
        // The existing engine's bounded serial queue is 1024 entries. Ordinary
        // numeric controls still use its existing backlog policy; this creates
        // no backlog and reserves no group/processor transaction.
        for index in 0..1024 - available {
            rig.session.controller().seek(index as f64);
        }
        let before = rig.session.document_snapshot();
        let pool = rig.session.state().pool.clone();
        let generation = rig.session.state().generation;
        let transport = rig.session.controller().transport();
        let clock = rig.session.controller().frame().tick;
        let result = rig.session.project_new();
        if available == 4 {
            assert!(result.is_ok());
            assert_eq!(rig.session.state().generation, generation + 1);
            assert_eq!(rig.session.controller().frame().tick, 0.0);
            rig.run(16);
            assert!(!rig.session.controller().transport().playing);
            assert_eq!(rig.session.controller().frame().tick, 0.0);
        } else {
            assert!(result.is_err());
            assert_eq!(rig.session.document_snapshot(), before);
            assert_eq!(rig.session.state().generation, generation);
            assert!(rig.session.state().pool.same_sources(&pool));
            assert_eq!(rig.session.controller().transport(), transport);
            assert_eq!(rig.session.controller().frame().tick, clock);
            assert!(crate::sync::lock(&rig.events.events).iter().any(|event| {
                matches!(event, crate::events::Event::ProjectWarnings(warnings) if warnings.iter().any(|warning| warning.contains("QueueFull")))
            }));
        }
    }
}

#[test]
fn p1_session_pending_decoder_completion_retries_with_exact_fresh_sources_and_off_guard_candidate_retirement()
 {
    for unrelated_change in [false, true] {
        let mut rig = Rig::new();
        let evidence = factory(&rig);
        let decoded = rig.session.hold("samples:decoded");
        let missing = rig
            .session
            .dispatch(
                Command::AddSample {
                    name: "Missing".into(),
                    path: windfall_project::SamplePath::External(rig.file("missing.wav")),
                },
                None,
            )
            .unwrap();
        let missing = windfall_project::SampleId(missing.created[0]);
        decoded.wait();
        assert!(rig.session.state().loading.contains(&missing));
        let before = rig.session.document_snapshot();
        let blocked = rig.session.hold("project:native-constructor");
        let worker = rig.session.background(|session| {
            session.dispatch(
                Command::Batch {
                    label: None,
                    commands: vec![
                        add(),
                        Command::AddSample {
                            name: "Homeless".into(),
                            path: windfall_project::SamplePath::Project("sounds/own.wav".into()),
                        },
                    ],
                },
                None,
            )
        });
        blocked.wait();
        decoded.release();
        let deadline = std::time::Instant::now() + super::PATIENCE;
        while rig.session.state().loading.contains(&missing) {
            assert!(
                std::time::Instant::now() < deadline,
                "decoder did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(rig.session.state().failed.contains(&missing));
        if unrelated_change {
            rig.session.state().pool.insert(
                windfall_project::SampleId(999),
                windfall_core::AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 10]),
            );
        }
        assert_eq!(rig.session.document_snapshot(), before);
        blocked.release();
        let result = worker.join().unwrap();
        assert_eq!(
            evidence.selected.load(Ordering::Relaxed),
            0,
            "candidate was never selected"
        );
        assert_eq!(
            evidence.dropped.load(Ordering::Relaxed),
            1,
            "refused candidate retired off guards"
        );
        if unrelated_change {
            assert!(result.unwrap_err().contains("source changed"));
            assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
            assert_eq!(rig.session.document_snapshot(), before);
        } else {
            let result = result.unwrap();
            assert_eq!(
                evidence.made.load(Ordering::Relaxed),
                2,
                "fresh preparation after truthful refusal"
            );
            assert_eq!(
                rig.project().samples.len(),
                before.project.samples.len() + 1
            );
            let sample = windfall_project::SampleId(*result.created.last().unwrap());
            assert_eq!(
                rig.session.sample_info_by_id(sample).unwrap_err(),
                "Missing sample: sounds/own.wav"
            );
            assert!(rig.session.state().failed.contains(&missing));
            rig.run(256);
            assert!(evidence.selected.load(Ordering::Relaxed) > 0);
        }
    }
}

#[test]
fn p1_session_repeated_pending_completions_exhaust_visible_retry_bound_without_musical_commit() {
    let rig = Rig::new();
    let evidence = factory(&rig);
    rig.session
        .dispatch(
            Command::Batch {
                label: None,
                commands: (0..4)
                    .map(|id| Command::AddSample {
                        name: format!("pending {id}"),
                        path: windfall_project::SamplePath::Project(format!("pending-{id}.wav")),
                    })
                    .collect(),
            },
            None,
        )
        .unwrap();
    let pending = rig
        .project()
        .samples
        .iter()
        .take(8)
        .map(|sample| sample.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(pending.len(), 8);
    rig.session.state().loading = pending;
    evidence.settle_loads.store(8, Ordering::Relaxed);
    let before = rig.session.document_snapshot();
    let source_pool = rig.session.state().pool.clone();
    let transport = rig.session.controller().transport();
    let error = rig.session.dispatch(add(), None).unwrap_err();
    assert!(error.contains("source changed"), "{error}");
    assert!(error.contains("retry limit (8 attempts)"), "{error}");
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.state().pool.same_sources(&source_pool));
    assert_eq!(rig.session.controller().transport(), transport);
    assert_eq!(evidence.made.load(Ordering::Relaxed), 8);
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 8);
    assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
    assert!(rig.session.state().loading.is_empty());
}

#[test]
fn p1_session_recording_artifact_survives_postinstall_event_unwind() {
    struct Capture;
    impl crate::session::recording::CaptureHandle for Capture {
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
    let mut rig = Rig::new();
    let evidence = factory(&rig);
    rig.session.dispatch(add(), None).unwrap();
    rig.run(64);
    rig.session
        .recording_start_with(
            windfall_ipc::RecordingSource {
                host: "Fake".into(),
                device: "Fake".into(),
                left: 0,
                right: None,
                armed_tracks: None,
                mixer_tap: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
            },
            0,
            None,
            |_, _, mut sink| {
                sink(&[0.25; 960])?;
                Ok(Box::new(Capture))
            },
        )
        .unwrap();
    evidence.revision.fetch_add(1, Ordering::Relaxed);
    let before = rig.session.document_snapshot();
    *crate::sync::lock(&rig.events.hook) = Some(Box::new(|event| {
        if matches!(event, crate::events::Event::ProjectPatch(_)) {
            panic!("installed recording patch event unwind");
        }
    }));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rig
            .session
            .recording_stop()))
        .is_err()
    );
    let installed = rig.session.document_snapshot();
    assert_eq!(
        installed.project.samples.len(),
        before.project.samples.len() + 1
    );
    let sample = installed.project.samples.last().unwrap();
    let windfall_project::SamplePath::External(path) = &sample.path else {
        panic!("recording artifact path");
    };
    assert!(
        std::path::Path::new(path).is_file(),
        "committed recording source was not deleted on unwind"
    );
    assert!(rig.session.sample_info_by_id(sample.id).is_ok());
    assert!(rig.session.state().pool.contains(sample.id));
    assert!(
        !rig.session
            .inner
            .recording_finishing
            .load(Ordering::Acquire)
    );
    unlocked(&rig.session);
    rig.run(256);
    rig.session.realtime_tick();
    assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
    assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
    assert!(evidence.selected.load(Ordering::Relaxed) > 0);
}

#[cfg(windows)]
#[test]
fn p1_session_ready_capture_accepts_unconfirmed_ack_and_retries_only_bookkeeping() {
    use crate::plugins::{CaptureAcknowledgement, CaptureOutcome};
    let mut rig = Rig::new();
    let (runtime, binding) = crate::plugins::vst3_fixture(7);
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
    rig.run(256);
    let deadline = std::time::Instant::now() + super::PATIENCE;
    let request = loop {
        if let Some(request) = runtime
            .drain()
            .into_iter()
            .find(|request| matches!(request.update, crate::plugins::Update::Capture { .. }))
        {
            break request;
        }
        assert!(std::time::Instant::now() < deadline, "native dirty request");
        rig.run(128);
        std::thread::yield_now();
    };
    let before = rig.session.document_snapshot();
    let predecessor = runtime.selected_token(target).unwrap();
    let receipt_loss = runtime.unconfirm_next_capture_acknowledgement(target);
    let ready = rig.session.hold("plugin-capture:prepared");
    let ready_wait = std::thread::spawn(move || {
        ready.wait();
        ready
    });
    let capture_runtime = runtime.clone();
    let task = rig
        .session
        .background(move |session| session.capture_plugin_update(&capture_runtime, request));
    while !ready_wait.is_finished() {
        assert!(std::time::Instant::now() < deadline, "capture readiness");
        rig.run(128);
        std::thread::yield_now();
    }
    let ready = ready_wait.join().unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(runtime.selected_token(target), Some(predecessor));
    unlocked(&rig.session);
    ready.release();
    let outcome = task.join().unwrap().unwrap();
    let CaptureOutcome::Accepted {
        restart: true,
        acknowledgement: CaptureAcknowledgement::Pending { ticket, warning },
    } = outcome
    else {
        panic!("actual Doc and ready install must remain accepted after receipt loss");
    };
    assert_eq!(
        warning,
        "Test bookkeeping acknowledgement delivery is unconfirmed"
    );
    unlocked(&rig.session);
    let installed = rig.session.document_snapshot();
    assert_ne!(installed, before);
    let plugin = installed.project.plugin(target).unwrap();
    assert_eq!(
        plugin
            .parameters
            .iter()
            .find(|parameter| parameter.id == 7)
            .unwrap()
            .value,
        0.375,
        "actual captured native rescan was committed"
    );
    assert_ne!(plugin.state, before.project.plugin(target).unwrap().state);
    assert_eq!(runtime.selected_token(target), Some(predecessor));
    let confirmed = runtime.retry_capture_ack(ticket).unwrap();
    assert_eq!(runtime.retry_capture_ack(ticket).unwrap(), confirmed);
    assert_eq!(rig.session.document_snapshot(), installed);
    drop(receipt_loss);
    rig.run(256);
    assert_ne!(runtime.selected_token(target), Some(predecessor));
    rig.session.realtime_tick();
    assert!(runtime.retry_capture_ack(ticket).is_ok());
    assert_eq!(rig.session.document_snapshot(), installed);
}
