//! Private postcommit delivery tests. Native cases use an explicit cached fixture
//! and deterministic owner/completion barriers; none starts another Cargo.
use crate::plugins::CaptureAcknowledgement;
use super::*;

fn captured_request() -> (PendingUpdate, CapturedState) {
    (
        PendingUpdate {
            binding: 11,
            token: 7,
            revision: 3,
            update: Update::Capture {
                target: PluginTarget::Effect {
                    effect: windfall_project::EffectId(9),
                },
                serial: 4,
            },
        },
        CapturedState {
            readmission: None,
            bytes: vec![1, 2, 3],
            parameters: vec![(0, 0.625)],
            restart: true,
            serial: 5,
        },
    )
}

#[test]
fn accepted_capture_survives_a_stopped_bookkeeping_owner() {
    let mut runtime = Runtime::new().unwrap();
    let (stopped, receiver) = mpsc::channel();
    drop(receiver);
    runtime.jobs = stopped;
    let (request, captured) = captured_request();
    let outcome = crate::plugins::CaptureOutcome::Accepted {
        restart: captured.restart,
        acknowledgement: runtime.acknowledge_committed_capture(&request, &captured),
    };
    assert_eq!(
        outcome,
        crate::plugins::CaptureOutcome::Accepted {
            restart: true,
            acknowledgement: crate::plugins::CaptureAcknowledgement::Pending {
                ticket: crate::plugins::capture_ack::CaptureAckTicket {
                    token: 7,
                    serial: 5,
                },
                warning: "The plugin owner stopped".into(),
            },
        }
    );
    assert_eq!(captured.bytes, [1, 2, 3]);
    assert_eq!(captured.parameters, [(0, 0.625)]);
    assert!(runtime.acknowledge_capture(&request, &captured).is_err());
}

#[test]
fn retired_owner_confirms_a_committed_capture_without_native_loading() {
    let runtime = Runtime::new().unwrap();
    let (request, captured) = captured_request();
    assert_eq!(
        runtime.acknowledge_committed_capture(&request, &captured),
        crate::plugins::CaptureAcknowledgement::Confirmed
    );
    assert_eq!(
        runtime
            .retry_capture_ack(crate::plugins::capture_ack::CaptureAckTicket {
                token: request.token,
                serial: captured.serial,
            })
            .unwrap(),
        crate::plugins::capture_ack::CaptureAckCompletion::Retired
    );
    runtime
        .call(|owner| {
            assert!(owner.instances.is_empty());
            assert_eq!(owner.next, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn acknowledgement_ticket_is_copy_and_retains_no_native_bytes() {
    use crate::plugins::capture_ack::CaptureAckTicket;
    assert_eq!(std::mem::size_of::<CaptureAckTicket>(), 16);
    assert!(!std::mem::needs_drop::<CaptureAckTicket>());
    let ticket = CaptureAckTicket {
        token: 7,
        serial: 5,
    };
    let copied = ticket;
    assert_eq!(ticket, copied);
}

fn native_owner() -> (
    Arc<Runtime>,
    Box<dyn HostedEffect>,
    PendingUpdate,
    CapturedState,
) {
    use std::path::PathBuf;
    use std::sync::OnceLock;
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    let path = FILE.get_or_init(|| {
        let source = PathBuf::from(
            std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("explicit prebuilt native fixture"),
        );
        let path = source.with_file_name("capture-ack-owner.vst3");
        std::fs::copy(source, &path).unwrap();
        path
    });
    let module = PluginHost::windfall().load(path).unwrap();
    let descriptor = module.descriptors()[10].clone();
    assert_eq!(descriptor.name, "VST3 Bridge Delayed Effect");
    let runtime = Arc::new(Runtime::new().unwrap());
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: path.to_string_lossy().into_owned(),
        id: descriptor.id.clone(),
        format: "vst3".into(),
        name: descriptor.name.clone(),
        vendor: descriptor.vendor,
        instrument: false,
        usable: true,
        error: None,
    }]);
    let binding = runtime
        .discover(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            capabilities: Default::default(),
            wrapper: Default::default(),
            target: PluginTarget::Effect {
                effect: windfall_project::EffectId(9),
            },
            format: "vst3".into(),
            path: path.to_string_lossy().into_owned(),
            id: descriptor.id,
            name: descriptor.name,
            state: Vec::new(),
            parameters: Vec::new(),
        })
        .unwrap();
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(windfall_engine::plugins::PluginTransport {
        playing: true,
        tempo_bpm: 120.0,
        position_beats: 0.0,
        position_seconds: 0.0,
        numerator: 4,
        denominator: 4,
        meter_anchor: None,
    });
    let token = runtime.selected_token(binding.target).unwrap();
    assert_ne!(token, 0);
    assert!(runtime.errors().is_empty());
    audio.set_param(7, 0.625);
    runtime
        .call(move |owner| {
            let record = owner.instances.get_mut(&token).unwrap();
            record.dirty_serial = 5;
            record.dirty = true;
            record.restart = true;
            record.capture_sent = true;
            let gain = record
                .controls
                .iter()
                .find(|control| control.id == 7)
                .unwrap();
            assert_eq!(
                gain.generation.load(std::sync::atomic::Ordering::Acquire),
                4
            );
            assert_eq!(gain.applied.load(std::sync::atomic::Ordering::Acquire), 2);
            Ok(())
        })
        .unwrap();
    let (mut request, captured) = captured_request();
    request.token = token;
    request.binding = binding_identity(&binding);
    request.revision = runtime.document_revision();
    (runtime, audio, request, captured)
}

#[test]
#[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
fn cancelled_completion_after_dirty_flags_clear_retains_ticket_and_no_dsp_ack() {
    use crate::plugins::capture_ack::{CaptureAckCompletion, CaptureOutcome, CaptureUpdates};
    use std::sync::atomic::Ordering;
    let (runtime, mut audio, request, captured) = native_owner();
    let token = request.token;
    let acknowledgement =
        runtime.acknowledge_committed_capture_with(&request, &captured, |ticket| {
            runtime.retry_capture_ack_with_checkpoint(
                ticket,
                Duration::from_secs(5),
                move |owner, lifetime| {
                    let record = owner.instances.get(&token).unwrap();
                    assert!(!record.dirty && !record.restart && !record.capture_sent);
                    assert_eq!(record.dirty_serial, 5);
                    // The actual completion fence loses after the bookkeeping
                    // effects, reproducing the generic running-cancel schedule.
                    lifetime
                        .phase
                        .compare_exchange(1, 4, Ordering::AcqRel, Ordering::Acquire)
                        .unwrap();
                },
            )
        });
    let mut acknowledgement = Some(acknowledgement);
    let mut updates = CaptureUpdates::default();
    updates.push(request.clone());
    let progress = updates.capture(|_| {
        Ok(CaptureOutcome::Accepted {
            restart: captured.restart,
            acknowledgement: acknowledgement.take().unwrap(),
        })
    });
    assert!(progress.restart);
    assert_eq!(
        updates.warning(),
        Some("The plugin owner request timed out and was cancelled")
    );
    assert!(updates.has_pending_ack());
    updates.retry_ack(|ticket| {
        let completion = runtime.retry_capture_ack(ticket)?;
        assert_eq!(completion, CaptureAckCompletion::Acknowledged);
        Ok(completion)
    });
    assert!(updates.is_empty());
    assert!(
        !updates
            .capture(|_| panic!("accepted native bytes replayed"))
            .restart
    );
    runtime
        .call(move |owner| {
            assert_eq!(
                owner.next, 1,
                "bookkeeping must not construct another native owner"
            );
            let record = owner.instances.get(&token).unwrap();
            assert_eq!(record.dirty_serial, 5);
            let gain = record
                .controls
                .iter()
                .find(|control| control.id == 7)
                .unwrap();
            assert_eq!(gain.generation.load(Ordering::Acquire), 4);
            assert_eq!(gain.applied.load(Ordering::Acquire), 2);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        runtime.selected_token(PluginTarget::Effect {
            effect: windfall_project::EffectId(9)
        }),
        Some(token)
    );
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
        0
    );
    assert_eq!(&left[..37], &[0.0; 37]);
    assert_eq!(&left[37..], &[0.625; 27]);
    assert_eq!(left, right);
    drop(audio);
    runtime
        .call(move |owner| {
            assert!(!owner.instances.contains_key(&token));
            Ok(())
        })
        .unwrap();
}

#[test]
#[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
fn older_acknowledgement_preserves_newer_dirty_restart_and_control_generation() {
    use crate::plugins::capture_ack::{CaptureAckCompletion, CaptureAckTicket};
    use std::sync::atomic::Ordering;
    let (runtime, audio, request, captured) = native_owner();
    let token = request.token;
    runtime
        .call(move |owner| {
            owner.instances.get_mut(&token).unwrap().dirty_serial = 6;
            Ok(())
        })
        .unwrap();
    for _ in 0..2 {
        let completion = runtime
            .retry_capture_ack_with_checkpoint(
                CaptureAckTicket {
                    token,
                    serial: captured.serial,
                },
                Duration::from_secs(5),
                move |owner, _| {
                    let record = owner.instances.get(&token).unwrap();
                    assert_eq!(record.dirty_serial, 6);
                    assert!(record.dirty && record.restart);
                    assert!(
                        !record.capture_sent,
                        "newer intent must be schedulable again"
                    );
                    let gain = record
                        .controls
                        .iter()
                        .find(|control| control.id == 7)
                        .unwrap();
                    assert_eq!(gain.generation.load(Ordering::Acquire), 4);
                    assert_eq!(gain.applied.load(Ordering::Acquire), 2);
                },
            )
            .unwrap();
        assert_eq!(completion, CaptureAckCompletion::Acknowledged);
    }
    drop(audio);
    runtime
        .call(move |owner| {
            assert!(!owner.instances.contains_key(&token));
            Ok(())
        })
        .unwrap();
}

struct ResumeOwner(mpsc::Sender<()>);
impl Drop for ResumeOwner {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
#[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
fn queued_cancel_keeps_dirty_intent_and_later_acknowledgement_is_only_bookkeeping() {
    let (runtime, audio, request, captured) = native_owner();
    let token = request.token;
    let (ready, started) = mpsc::channel();
    let (resume, continuation) = mpsc::channel();
    runtime
        .jobs
        .send(Box::new(move |_| {
            ready.send(()).unwrap();
            continuation.recv_timeout(Duration::from_secs(5)).unwrap();
        }))
        .unwrap();
    let pause = ResumeOwner(resume);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let acknowledgement =
        runtime.acknowledge_committed_capture_with(&request, &captured, |ticket| {
            runtime.retry_capture_ack_with_checkpoint(ticket, Duration::ZERO, |_, _| {
                panic!("queued cancellation must prevent owner bookkeeping");
            })
        });
    assert!(matches!(
        acknowledgement,
        crate::plugins::CaptureAcknowledgement::Pending { .. }
    ));
    drop(pause);
    runtime
        .call(move |owner| {
            let record = owner.instances.get(&token).unwrap();
            assert_eq!(record.dirty_serial, 5);
            assert!(record.dirty && record.restart && record.capture_sent);
            Ok(())
        })
        .unwrap();
    let crate::plugins::CaptureAcknowledgement::Pending { ticket, .. } = acknowledgement else {
        unreachable!();
    };
    runtime.retry_capture_ack(ticket).unwrap();
    runtime
        .call(move |owner| {
            let record = owner.instances.get(&token).unwrap();
            assert_eq!(record.dirty_serial, 5);
            assert!(!record.dirty && !record.restart);
            assert_eq!(owner.next, 1);
            Ok(())
        })
        .unwrap();
    drop(audio);
    runtime
        .call(move |owner| {
            assert!(!owner.instances.contains_key(&token));
            Ok(())
        })
        .unwrap();
}

#[test]
fn scoped_receipt_loss_is_targeted_one_shot_and_restores_normal_delivery() {
    let runtime = Runtime::new().unwrap();
    let (request, captured) = captured_request();
    let Update::Capture { target, .. } = request.update else {
        unreachable!()
    };
    let scope = runtime.unconfirm_next_capture_acknowledgement(target);
    let mut other = request.clone();
    other.update = Update::Capture {
        target: PluginTarget::Effect {
            effect: windfall_project::EffectId(10),
        },
        serial: 99,
    };
    assert_eq!(
        runtime.acknowledge_committed_capture(&other, &captured),
        crate::plugins::CaptureAcknowledgement::Confirmed
    );
    let acknowledgement = runtime.acknowledge_committed_capture(&request, &captured);
    assert_eq!(
        acknowledgement,
        crate::plugins::CaptureAcknowledgement::Pending {
            ticket: crate::plugins::capture_ack::CaptureAckTicket {
                token: request.token,
                serial: captured.serial
            },
            warning: "Test bookkeeping acknowledgement delivery is unconfirmed".into(),
        }
    );
    assert_eq!(
        runtime.acknowledge_committed_capture(&request, &captured),
        crate::plugins::CaptureAcknowledgement::Confirmed
    );
    drop(scope);
    let unused_scope = runtime.unconfirm_next_capture_acknowledgement(target);
    drop(unused_scope);
    assert_eq!(
        runtime.acknowledge_committed_capture(&request, &captured),
        crate::plugins::CaptureAcknowledgement::Confirmed
    );
}

#[test]
#[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
fn session_fault_seam_derives_ticket_after_real_native_capture_and_owner_effects() {
    use crate::plugins::capture_ack::{CaptureOutcome, CaptureUpdates};
    let (runtime, mut audio, request, _) = native_owner();
    let token = request.token;
    let mut binding = runtime
        .call(move |owner| Ok(owner.instances.get(&token).unwrap().binding.clone()))
        .unwrap();
    binding
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.625;
    let mut project = Project::new("Native bookkeeping capture");
    project.plugins.push(binding.clone());
    runtime.commit_parameters(&project);
    audio.adopt_parameters(&binding.parameters);
    let target = binding.target;
    let scope = runtime.unconfirm_next_capture_acknowledgement(target);
    let owner = runtime.clone();
    let capture_request = request.clone();
    let task =
        std::thread::spawn(move || owner.capture_pending(capture_request, binding.parameters));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !task.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "actual native capture did not return"
        );
        assert_eq!(
            crate::test_alloc::allocator_calls(|| audio.control_boundary()),
            0
        );
        std::thread::yield_now();
    }
    let captured = task.join().unwrap().unwrap().unwrap();
    audio.control_boundary();
    assert!(captured.bytes.starts_with(b"WFPS"));
    assert!(captured.parameters.contains(&(7, 0.625)));
    let acknowledgement = runtime.acknowledge_committed_capture(&request, &captured);
    let crate::plugins::CaptureAcknowledgement::Pending {
        ticket,
        ref warning,
    } = acknowledgement
    else {
        panic!("scoped receipt loss must follow real successful bookkeeping");
    };
    assert_eq!(ticket.token, request.token);
    assert_eq!(ticket.serial, captured.serial);
    assert_eq!(
        warning,
        "Test bookkeeping acknowledgement delivery is unconfirmed"
    );
    let serial = captured.serial;
    runtime
        .call(move |owner| {
            let record = owner.instances.get(&token).unwrap();
            assert_eq!(record.dirty_serial, serial);
            assert!(!record.dirty && !record.restart);
            assert_eq!(owner.next, 1);
            Ok(())
        })
        .unwrap();
    let mut acknowledgement = Some(acknowledgement);
    let mut updates = CaptureUpdates::default();
    updates.push(request);
    updates.capture(|_| {
        Ok(CaptureOutcome::Accepted {
            restart: captured.restart,
            acknowledgement: acknowledgement.take().unwrap(),
        })
    });
    updates.retry_ack(|ticket| runtime.retry_capture_ack(ticket));
    assert!(updates.is_empty());
    assert!(
        !updates
            .capture(|_| panic!("accepted bytes recaptured"))
            .restart
    );
    drop(scope);
    drop(audio);
    runtime
        .call(move |owner| {
            assert!(!owner.instances.contains_key(&token));
            Ok(())
        })
        .unwrap();
}

#[cfg(windows)]
#[test]
fn bridge_capture_ack_restart_replacement_native_interleaving_table() {
    use crate::plugins::capture_ack::{CaptureAckCompletion, CaptureAckTicket};
    use windfall_project::{Command, Document, Project, TrackId};
    #[derive(Clone, Copy, Debug)] enum Boundary { BeforeReply, AfterReply, AfterCommit, AfterAck }
    fn fail_during_capture(runtime: &Arc<Runtime>, token: u64) {
        let result = runtime.call(move |owner| {
            let record = owner.bridges.get(&token).unwrap();
            let pending = record.binding.parameters.iter().filter(|p| !p.read_only).map(|p| windfall_plugin_host::bridge::adapter::ParameterSpec { id: p.id, min: f64::from(p.min), max: f64::from(p.max), value: if p.id == 7 { 0.75 } else { f64::from(p.value) }, read_only: false, stepped: p.stepped }).collect::<Vec<_>>();
            record.control.readmit(1, &pending, Duration::from_millis(100)).map(|_| ())
        });
        assert!(result.is_err(), "real helper must exit/hang inside native save");
        assert!(runtime.bridge_status().iter().all(|(_, _, status)| status.reaped));
    }
    // Crash/hang x four capture/ack boundaries x replacement/no replacement,
    // for BOTH formats. Every row uses real helper state, a checked document
    // edit, a fresh helper on recovery, and retired-owner ACK delivery.
    for format in ["clap", "vst3"] {
        for (fault, class) in [("capture-exit", 13), ("capture-hang", 14)] {
            for boundary in [Boundary::BeforeReply, Boundary::AfterReply, Boundary::AfterCommit, Boundary::AfterAck] {
                for replace in [false, true] {
                    let row = format!("{format}/{fault}/{boundary:?}/replacement={replace}");
                    let folder = tempfile::tempdir().unwrap();
                    let path = folder.path().join(format!("composition.{format}"));
                    std::fs::copy(std::env::var_os("WINDFALL_BRIDGE_FIXTURE").unwrap(), &path).unwrap();
                    let helper = std::path::PathBuf::from(std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER").unwrap());
                    let runtime = Runtime::bridge_fixture(&helper);
                    let id = if format == "clap" { format!("org.windfall.test.bridge-{fault}") } else { format!("{:032X}", class + 1) };
                    runtime.approve(&[windfall_ipc::PluginEntry { path: path.to_string_lossy().into_owned(), id: id.clone(), format: format.into(), name: row.clone(), vendor: "Windfall Tests".into(), instrument: false, usable: true, error: None }]);
                    // Discovery saves a healthy low-gain checkpoint before fault.
                    let binding = runtime.discover(PluginBinding { capabilities: Default::default(), sidechain_input: None, auxiliary_inputs: vec![], wrapper: Default::default(), target: PluginTarget::Effect { effect: windfall_project::EffectId(0) }, path: path.to_string_lossy().into_owned(), id, format: format.into(), name: row.clone(), state: vec![], parameters: vec![windfall_project::PluginParameter { id: 7, name: "Gain".into(), min: 0.0, max: if format == "clap" { 2.0 } else { 1.0 }, value: 0.5, stepped: false, read_only: false, automatable: true }] }).unwrap();
                    let mut document = Document::new(Project::new("Capture composition"));
                    document.dispatch(Command::AddPluginEffect { track: TrackId::MASTER, plugin: binding }, None).unwrap();
                    let binding = document.project().plugins[0].clone(); let target = binding.target;
                    let mut audio = runtime.effect(&binding, 48_000, 128).unwrap();
                    audio.transport(windfall_engine::plugins::PluginTransport { playing: false, tempo_bpm: 120.0, position_beats: 0.0, position_seconds: 0.0, numerator: 4, denominator: 4, meter_anchor: None });
                    let token = runtime.selected_token(target).unwrap();
                    let checkpoint = runtime.call(move |owner| {
                        let record = owner.bridges.get_mut(&token).unwrap();
                        let captured = record.capture(&record.binding.parameters)?;
                        record.dirty_serial = 1; record.capture_sent = true;
                        Ok(captured.0)
                    }).unwrap();
                    let request = PendingUpdate { binding: binding_identity(&binding), token, revision: runtime.document_revision(), update: Update::Capture { target, serial: 1 } };
                    if replace && matches!(boundary, Boundary::BeforeReply) {
                        // Replace the document while the old creating owner is
                        // inside an admitted control job, before its fault reply.
                        let (entered, receive) = mpsc::channel();
                        let old_runtime = runtime.clone();
                        let fault = std::thread::spawn(move || old_runtime.call(move |owner| {
                            let record = owner.bridges.get(&token).unwrap();
                            let pending = record.binding.parameters.iter().filter(|p| !p.read_only).map(|p| windfall_plugin_host::bridge::adapter::ParameterSpec { id: p.id, min: f64::from(p.min), max: f64::from(p.max), value: if p.id == 7 { 0.75 } else { f64::from(p.value) }, read_only: false, stepped: p.stepped }).collect::<Vec<_>>();
                            entered.send(()).unwrap();
                            record.control.readmit(1, &pending, Duration::from_millis(100)).map(|_| ())
                        }));
                        receive.recv_timeout(Duration::from_secs(3)).unwrap();
                        runtime.retry(); document = Document::new(Project::new("Replaced during native capture"));
                        let replacement = document.project().clone();
                        assert!(fault.join().unwrap().is_err(), "{row}");
                        assert!(runtime.capture_pending(request.clone(), binding.parameters.clone()).unwrap().is_none(), "{row}");
                        assert_eq!(document.project(), &replacement, "{row}");
                        drop(audio); runtime.call(|_| Ok(())).unwrap();
                        assert_eq!(runtime.retry_capture_ack(CaptureAckTicket { token, serial: 1 }).unwrap(), CaptureAckCompletion::Retired, "{row}");
                        continue;
                    }
                    let mut desired = binding.parameters.clone();
                    if matches!(boundary, Boundary::BeforeReply) {
                        desired.iter_mut().find(|p| p.id == 7).unwrap().value = 0.75;
                        fail_during_capture(&runtime, token);
                    }
                    if !matches!(boundary, Boundary::BeforeReply) { audio.set_param(7, 0.625); }
                    let captured = runtime.capture_pending(request.clone(), desired.clone()).unwrap().unwrap();
                    let retried = runtime.capture_pending(request.clone(), desired.clone()).unwrap().unwrap();
                    assert_eq!(captured.bytes, retried.bytes, "completed bytes must survive retry: {row}");
                    if matches!(boundary, Boundary::BeforeReply) { assert_eq!(captured.bytes, checkpoint, "{row}"); }
                    if matches!(boundary, Boundary::AfterReply) { fail_during_capture(&runtime, token); }
                    let before = document.project().clone();
                    let mut accepted = 0;
                    if replace && matches!(boundary, Boundary::AfterReply) {
                        runtime.retry();
                        document = Document::new(Project::new("Replacement"));
                        assert!(runtime.capture_pending(request.clone(), desired).unwrap().is_none(), "{row}");
                        assert!(document.project().plugins.is_empty(), "{row}");
                    } else {
                        assert!(runtime.is_current(request.revision, token));
                        let mut commands = captured.parameters.iter().filter(|(id, _)| binding.parameters.iter().any(|p| p.id == *id && !p.read_only)).map(|&(id, value)| Command::SetPluginParam { target, id, value }).collect::<Vec<_>>();
                        commands.push(Command::SetPluginState { target, state: captured.bytes.clone() });
                        document.dispatch(Command::Batch { label: Some("Captured state".into()), commands }, None).unwrap(); accepted += 1;
                        if matches!(boundary, Boundary::AfterCommit) {
                            fail_during_capture(&runtime, token);
                            if replace { runtime.retry(); document = Document::new(Project::new("Replaced before acknowledgement")); }
                        }
                        // Lose the first actual ACK receipt AFTER its effects.
                        let acknowledgement = runtime.acknowledge_committed_capture_with(&request, &captured, |ticket| { runtime.retry_capture_ack(ticket)?; Err("receipt lost after bookkeeping".into()) });
                        assert!(matches!(acknowledgement, CaptureAcknowledgement::Pending { .. }), "{row}");
                        if matches!(boundary, Boundary::AfterAck) { fail_during_capture(&runtime, token); }
                        if replace {
                            if !matches!(boundary, Boundary::AfterCommit) { runtime.retry(); document = Document::new(Project::new("Replaced after capture acceptance")); }
                            let replacement = document.project().clone();
                            assert!(runtime.capture_pending(request.clone(), desired).unwrap().is_none(), "{row}");
                            drop(audio); runtime.call(|_| Ok(())).unwrap();
                            let ticket = CaptureAckTicket { token, serial: captured.serial };
                            assert_eq!(runtime.retry_capture_ack(ticket).unwrap(), CaptureAckCompletion::Retired, "{row}");
                            assert_eq!(document.project(), &replacement, "{row}");
                            assert_eq!(accepted, 1, "{row}");
                            continue;
                        }
                        let restored = document.project().plugin(target).unwrap().clone();
                        let mut replacement = runtime.effect(&restored, 48_000, 128).unwrap();
                        replacement.transport(windfall_engine::plugins::PluginTransport { playing: false, tempo_bpm: 120.0, position_beats: 0.0, position_seconds: 0.0, numerator: 4, denominator: 4, meter_anchor: None });
                        assert_ne!(runtime.selected_token(target), Some(token), "{row}");
                        assert_eq!(restored.state, captured.bytes, "{row}");
                        let expected = if matches!(boundary, Boundary::BeforeReply) { 0.75 } else { 0.625 };
                        let deadline = std::time::Instant::now() + Duration::from_secs(3);
                        loop {
                            let mut left = [1.0; 512]; let mut right = left;
                            assert_eq!(crate::test_alloc::allocator_calls(|| replacement.process(&mut left, &mut right)), 0);
                            if left.iter().all(|value| (*value - expected).abs() < 1e-6) && right == left { break; }
                            assert!(std::time::Instant::now() < deadline, "recovered PCM differs: {row}");
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        assert_eq!(accepted, 1, "{row}");
                        document.undo().unwrap(); assert_eq!(document.project(), &before);
                        document.redo().unwrap(); assert_eq!(document.project().plugin(target).unwrap(), &restored);
                        drop(replacement);
                    }
                    drop(audio);
                    runtime.call(|_| Ok(())).unwrap();
                    let ticket = CaptureAckTicket { token, serial: captured.serial };
                    assert_eq!(runtime.retry_capture_ack(ticket).unwrap(), CaptureAckCompletion::Retired, "{row}");
                    assert_eq!(runtime.retry_capture_ack(ticket).unwrap(), CaptureAckCompletion::Retired, "{row}");
                    assert_eq!(accepted, usize::from(!replace), "{row}");
                }
            }
        }
    }
}
