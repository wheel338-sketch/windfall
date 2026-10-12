//! Real VST3 fixtures through the production desktop native owner.
use super::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windfall_project::{Command, Document, EffectId, TrackId};

fn clap_expression_fixture(midi: bool) -> (Arc<Runtime>, PluginBinding) {
    static CLAP: OnceLock<PathBuf> = OnceLock::new();
    let path = CLAP.get_or_init(|| {
        let (_, binding) = fixture(2);
        let path = PathBuf::from(&binding.path).with_extension("clap");
        std::fs::copy(binding.path, &path).unwrap();
        path
    });
    let runtime = Arc::new(Runtime::new().unwrap());
    let id = if midi {
        "org.windfall.test.midi-sine"
    } else {
        "org.windfall.test.sine"
    };
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: path.to_string_lossy().into_owned(),
        id: id.into(),
        format: "clap".into(),
        name: "Expression fixture".into(),
        vendor: "Fixture".into(),
        instrument: true,
        usable: true,
        error: None,
    }]);
    let binding = runtime
        .discover(PluginBinding {
            wrapper: Default::default(),
            target: PluginTarget::Instrument {
                channel: windfall_project::ChannelId(900),
            },
            format: "clap".into(),
            path: path.to_string_lossy().into_owned(),
            id: id.into(),
            name: "Expression fixture".into(),
            state: vec![],
            parameters: vec![],
            sidechain_input: None,
            auxiliary_inputs: vec![],
        })
        .unwrap();
    (runtime, binding)
}

fn expression_audio(audio: &mut dyn HostedInstrument, frames: usize) -> Vec<[f32; 2]> {
    let mut output = Vec::with_capacity(frames);
    for _ in 0..frames.div_ceil(64) {
        let mut left = [0.0; 64];
        let mut right = [0.0; 64];
        assert_eq!(
            crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
            0
        );
        output.extend(
            left.into_iter()
                .zip(right)
                .map(|(left, right)| [left, right]),
        );
    }
    output.truncate(frames);
    output
}

fn expression_frequency(audio: &[[f32; 2]], side: usize) -> f64 {
    let crossings: Vec<_> = audio
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0][side] < 0.0 && pair[1][side] >= 0.0)
        .map(|(index, _)| index)
        .collect();
    assert!(crossings.len() > 3);
    48_000.0 * (crossings.len() - 1) as f64 / (crossings[crossings.len() - 1] - crossings[0]) as f64
}

#[test]
fn native_expression_map_groups_and_latest_instance_survive_owner_exchange_without_allocation() {
    use windfall_dsp::{
        ExpressionMap, NoteExpression, NoteInstanceId, PluginExpressionDestination,
    };
    let (runtime, binding) = clap_expression_fixture(false);
    let mut audio = runtime.instrument(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    assert!(audio.supports_note_instances() && audio.supports_note_pitch());
    audio.set_param(1, 1.0);
    let mut map = ExpressionMap::default();
    map.modulation_x.plugin = PluginExpressionDestination::Pressure;
    map.modulation_x.amount = 0.5;
    map.modulation_x.invert = true;
    let note = NoteExpression {
        modulation_x: 1.0,
        color_group: Some(2),
        ..NoteExpression::NEUTRAL
    };
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.set_expression_map(map);
            audio.note_on_instance(NoteInstanceId(1), 69, 1.0, -1.0, note);
            audio.note_on_instance(
                NoteInstanceId(2),
                69,
                1.0,
                1.0,
                NoteExpression {
                    color_group: Some(5),
                    ..note
                },
            );
            audio.set_note_pitch(NoteInstanceId(2), 81.0);
        }),
        0
    );
    let initial = expression_audio(audio.as_mut(), 8192);
    assert!((expression_frequency(&initial[64..], 0) - 440.0).abs() < 1.0);
    assert!((expression_frequency(&initial[64..], 1) - 880.0).abs() < 1.0);
    let peak = initial
        .iter()
        .map(|frame| frame[0].abs())
        .fold(0.0, f32::max);
    assert!(
        (peak - 0.5).abs() < 0.01,
        "mapped pressure .25, hard pan doubles the fixture level"
    );
    let resume = pause_adapter(&runtime, audio.as_mut());
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.note_off_instance(NoteInstanceId(1), 69);
            audio.set_note_expression(
                NoteInstanceId(2),
                1.0,
                NoteExpression {
                    modulation_x: 0.0,
                    color_group: Some(5),
                    ..note
                },
            );
            audio.set_note_pitch(NoteInstanceId(2), 72.0);
        }),
        0
    );
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    audio.control_boundary();
    let replayed = expression_audio(audio.as_mut(), 8192);
    assert!(replayed[64..].iter().all(|frame| frame[0] == 0.0));
    assert!((expression_frequency(&replayed[64..], 1) - 523.251).abs() < 1.0);
    assert!(replayed.iter().any(|frame| frame[1].abs() > 1.4));
    audio.all_notes_off();
    assert!(
        expression_audio(audio.as_mut(), 128)[64..]
            .iter()
            .all(|frame| *frame == [0.0; 2])
    );
    assert_eq!(audio.voices(), 0);
}

#[test]
fn midi_only_input_receives_bend_before_onset_and_replays_the_latest_group_pitch() {
    use windfall_dsp::{NoteExpression, NoteInstanceId};
    let (runtime, binding) = clap_expression_fixture(true);
    let mut audio = runtime.instrument(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    assert!(audio.supports_note_pitch());
    let note = NoteExpression {
        fine_pitch_cents: 100.0,
        color_group: Some(7),
        ..NoteExpression::NEUTRAL
    };
    audio.note_on_instance(NoteInstanceId(1), 69, 1.0, 0.0, note);
    let bent = expression_audio(audio.as_mut(), 8192);
    assert!((expression_frequency(&bent[64..], 0) - 466.164).abs() < 1.0);
    let resume = pause_adapter(&runtime, audio.as_mut());
    audio.set_note_pitch(NoteInstanceId(1), 71.0);
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    audio.control_boundary();
    let replayed = expression_audio(audio.as_mut(), 8192);
    assert!((expression_frequency(&replayed[64..], 0) - 493.883).abs() < 1.0);
    audio.note_off_on_channel(NoteInstanceId(1), 69, 7);
    assert!(
        expression_audio(audio.as_mut(), 128)[64..]
            .iter()
            .all(|frame| *frame == [0.0; 2])
    );
}

#[test]
fn vst3_cached_midi_mapping_routes_color_channels_replays_owner_and_matches_render() {
    use windfall_dsp::{NoteExpression, NoteInstanceId};
    let (runtime, binding) = fixture(24);
    let mut audio = runtime.instrument(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    assert!(audio.supports_note_pitch() && audio.supports_note_instances());
    let left = NoteExpression {
        fine_pitch_cents: 100.0,
        color_group: Some(2),
        ..NoteExpression::NEUTRAL
    };
    let right = NoteExpression {
        fine_pitch_cents: -100.0,
        color_group: Some(5),
        ..NoteExpression::NEUTRAL
    };
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.note_on_instance(NoteInstanceId(1), 69, 1.0, 0.0, left);
            audio.note_on_instance(NoteInstanceId(2), 69, 1.0, 0.0, right);
        }),
        0
    );
    let native = expression_audio(audio.as_mut(), 8192);
    assert!((expression_frequency(&native, 0) - 466.164).abs() < 1.0);
    assert!((expression_frequency(&native, 1) - 415.305).abs() < 1.0);
    let factory = runtime.render_factory().unwrap();
    let mut offline = factory.instrument(&binding, 48_000, 64).unwrap();
    offline.transport(transport());
    offline.note_on_instance(NoteInstanceId(1), 69, 1.0, 0.0, left);
    offline.note_on_instance(NoteInstanceId(2), 69, 1.0, 0.0, right);
    assert_eq!(native, expression_audio(offline.as_mut(), 8192));
    let resume = pause_adapter(&runtime, audio.as_mut());
    audio.note_off_on_channel(NoteInstanceId(1), 69, 2);
    audio.set_note_pitch(NoteInstanceId(2), 71.0);
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    audio.control_boundary();
    let replayed = expression_audio(audio.as_mut(), 8192);
    assert!(replayed.iter().all(|frame| frame[0] == 0.0));
    assert!((expression_frequency(&replayed, 1) - 493.883).abs() < 1.0);
    audio.all_notes_off();
    assert!(
        expression_audio(audio.as_mut(), 128)
            .iter()
            .all(|frame| *frame == [0.0; 2])
    );
}

pub(crate) fn fixture(index: usize) -> (Arc<Runtime>, PluginBinding) {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    let path = FILE.get_or_init(|| {
        if let Some(source) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") {
            let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/vst3-ownership-fixture");
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join(format!("fixture-{}.vst3", std::process::id()));
            std::fs::copy(source, &path).unwrap();
            return path;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let target = root.join("target/vst3-ownership-fixture");
        let output = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(root.join("crates/windfall-plugin-host/test-plugins/Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = target.join("fixture.vst3");
        std::fs::copy(target.join("debug/windfall_test_plugins.dll"), &path).unwrap();
        path
    });
    let module = PluginHost::windfall().load(path).unwrap();
    let descriptor = module.descriptors()[index].clone();
    let runtime = Arc::new(Runtime::new().unwrap());
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: path.to_string_lossy().into_owned(),
        id: descriptor.id.clone(),
        format: "vst3".into(),
        name: descriptor.name.clone(),
        vendor: "Fixture".into(),
        instrument: index == 2 || index == 23 || index == 24,
        usable: true,
        error: None,
    }]);
    let binding = runtime
        .discover(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            wrapper: Default::default(),
            target: if index == 2 || index == 23 || index == 24 {
                PluginTarget::Instrument {
                    channel: windfall_project::ChannelId(900),
                }
            } else {
                PluginTarget::Effect {
                    effect: EffectId(900),
                }
            },
            format: "vst3".into(),
            path: path.to_string_lossy().into_owned(),
            id: descriptor.id,
            name: descriptor.name,
            state: vec![],
            parameters: vec![],
        })
        .unwrap();
    (runtime, binding)
}

fn transport() -> windfall_engine::plugins::PluginTransport {
    windfall_engine::plugins::PluginTransport {
        playing: false,
        tempo_bpm: 120.0,
        position_beats: 0.0,
        position_seconds: 0.0,
        numerator: 4,
        denominator: 4,
        meter_anchor: None,
    }
}

fn capture(
    runtime: &Arc<Runtime>,
    project: Project,
    audio: &mut dyn HostedEffect,
) -> Result<Project, String> {
    let runtime = runtime.clone();
    let task = std::thread::spawn(move || runtime.capture(project));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !task.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "capture did not complete"
        );
        audio.control_boundary();
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = task.join().unwrap();
    audio.control_boundary();
    result
}

/// Hold exclusive ownership off audio until the test has delivered its edits.
fn pause_adapter<T: HostedEffect + ?Sized>(runtime: &Runtime, audio: &mut T) -> mpsc::Sender<()> {
    let (ready, ready_rx) = mpsc::channel();
    let (resume, resume_rx) = mpsc::channel();
    let token = runtime
        .selected_token(PluginTarget::Effect {
            effect: EffectId(900),
        })
        .or_else(|| {
            selected(
                &runtime.selection,
                PluginTarget::Instrument {
                    channel: windfall_project::ChannelId(900),
                },
            )
        })
        .unwrap();
    runtime
        .jobs
        .send(Box::new(move |owner| {
            let record = owner.instances.get_mut(&token).unwrap();
            record.ownership.request();
            let adapter = loop {
                if let Some(adapter) = record.ownership.take_returned() {
                    break adapter;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            record.release(adapter).unwrap();
            ready.send(()).unwrap();
            resume_rx.recv().unwrap();
            record.restore().unwrap();
        }))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        audio.control_boundary();
        if ready_rx.try_recv().is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    resume
}

#[test]
fn runtime_repair_capture_reconciles_absent_controls_and_keeps_timeout_intent() {
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    audio.set_param(7, 0.5);
    audio.process(&mut [1.0; 64], &mut [1.0; 64]);
    let token = runtime.selected_token(binding.target).unwrap();
    let resume = pause_adapter(&runtime, audio.as_mut());
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.set_param(7, 0.9);
            audio.set_param(7, 0.75);
            audio.process(&mut [], &mut []);
        }),
        0
    );
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    let mut project = Project::new("Pending inactive capture");
    project.plugins.push(binding);
    project.plugins[0]
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.75;
    runtime.commit_parameters(&project);
    let error = runtime.capture(project.clone()).unwrap_err();
    assert!(error.contains("boundary"), "{error}");
    assert_eq!(
        runtime.selected_token(project.plugins[0].target),
        Some(token)
    );
    let saved = capture(&runtime, project, audio.as_mut()).unwrap();
    assert_eq!(
        saved.plugins[0]
            .parameters
            .iter()
            .find(|p| p.id == 7)
            .unwrap()
            .value,
        0.75
    );
    assert_eq!(runtime.selected_token(saved.plugins[0].target), Some(token));
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
        0
    );
    assert_eq!(left, [1.5; 64]);
    let mut reopened = runtime
        .render_factory()
        .unwrap()
        .effect(&saved.plugins[0], 48_000, 64)
        .unwrap();
    left.fill(1.0);
    right.fill(1.0);
    reopened.process(&mut left, &mut right);
    assert_eq!(left, [1.5; 64]);
}

#[test]
fn runtime_repair_capture_refusal_retains_pending_controls_and_stale_snapshot_is_rejected() {
    let (runtime, binding) = fixture(6);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    audio.set_param(7, 0.5);
    audio.process(&mut [1.0; 64], &mut [1.0; 64]);
    let token = runtime.selected_token(binding.target).unwrap();
    let mut project = Project::new("Pending refusing capture");
    project.plugins.push(binding);
    let stale = project.clone();
    project.plugins[0]
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.75;
    runtime.commit_parameters(&project);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.set_param(7, 0.75)),
        0
    );
    let error = runtime.capture(stale).unwrap_err();
    assert!(error.contains("parameters changed"), "{error}");
    let error = capture(&runtime, project.clone(), audio.as_mut()).unwrap_err();
    assert!(error.contains("setActive(false) refused"), "{error}");
    assert_eq!(
        runtime.selected_token(project.plugins[0].target),
        Some(token)
    );
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
        0
    );
    assert_eq!(
        left, [1.5; 64],
        "refusal must preserve the exact accepted queue"
    );
    audio.set_param(7, 0.25);
    left.fill(1.0);
    right.fill(1.0);
    audio.process(&mut left, &mut right);
    project.plugins[0]
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.625;
    runtime.commit_parameters(&project);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.set_param(7, 0.625)),
        0
    );
    let saved = capture(&runtime, project, audio.as_mut()).unwrap();
    assert_eq!(
        saved.plugins[0]
            .parameters
            .iter()
            .find(|p| p.id == 7)
            .unwrap()
            .value,
        0.625
    );
    assert_eq!(runtime.selected_token(saved.plugins[0].target), Some(token));
    left.fill(1.0);
    right.fill(1.0);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
        0
    );
    assert_eq!(left, [1.25; 64]);
}

#[test]
fn runtime_repair_completed_automation_readback_supersedes_observed_document_control() {
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    let mut project = Project::new("Later automation");
    project.plugins.push(binding);
    project.plugins[0]
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.75;
    runtime.commit_parameters(&project);
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.set_param(7, 0.75);
            audio.process(&mut left, &mut right);
            // Once the document control has processed, newer automation is not
            // masked by its older stored value during save.
            audio.set_param(7, 0.25);
            left.fill(1.0);
            right.fill(1.0);
            audio.process(&mut left, &mut right);
        }),
        0
    );
    assert_eq!(left, [0.5; 64]);
    let saved = capture(&runtime, project, audio.as_mut()).unwrap();
    assert_eq!(
        saved.plugins[0]
            .parameters
            .iter()
            .find(|p| p.id == 7)
            .unwrap()
            .value,
        0.25
    );
}

#[test]
fn r4_document_adoption_matches_current_intent_and_never_acks_before_processing() {
    use std::sync::atomic::Ordering;
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    let token = runtime.selected_token(binding.target).unwrap();
    let controls = runtime
        .controls
        .lock()
        .unwrap()
        .get(&token)
        .unwrap()
        .2
        .clone();
    let control = controls.iter().find(|control| control.id == 7).unwrap();
    let mut project = Project::new("Document adoption");
    project.plugins.push(binding.clone());
    project.plugins[0]
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.75;
    runtime.commit_parameters(&project);
    let document = control.document_generation.load(Ordering::Acquire);
    let desired = control.generation.load(Ordering::Acquire);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.adopt_parameters(&binding.parameters); // stale plan still has 0.5
        }),
        0
    );
    assert_eq!(control.observed_document.load(Ordering::Acquire), 2);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.adopt_parameters(&project.plugins[0].parameters);
        }),
        0
    );
    assert_eq!(control.observed_document.load(Ordering::Acquire), document);
    assert_eq!(control.applied_document.load(Ordering::Acquire), 2);
    assert_eq!(
        control.generation.load(Ordering::Acquire),
        desired,
        "adoption must not enqueue controls"
    );
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.set_param(7, 0.25); // automation after the matching plan
            audio.process(&mut left, &mut right);
        }),
        0
    );
    assert_eq!(left, [0.5; 64]);
    assert_eq!(control.applied_document.load(Ordering::Acquire), document);
    let captured = capture(&runtime, project, audio.as_mut()).unwrap();
    assert_eq!(
        captured.plugins[0]
            .parameters
            .iter()
            .find(|p| p.id == 7)
            .unwrap()
            .value,
        0.25
    );
    assert_eq!(runtime.selected_token(binding.target), Some(token));
}

#[test]
fn paused_effect_retains_the_latest_parameter_without_an_engine_resend() {
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    let resume = pause_adapter(&runtime, audio.as_mut());
    audio.set_param(7, 0.2);
    audio.set_param(7, 0.8);
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
        0
    );
    assert_eq!(left, [1.0; 64], "paused effect is dry");
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.control_boundary();
            audio.process(&mut left, &mut right);
        }),
        0
    );
    assert_eq!(left, [1.6; 64]);
}

#[test]
fn paused_instrument_reconstructs_only_the_latest_held_keys() {
    let (runtime, mut binding) = fixture(2);
    binding.target = PluginTarget::Instrument {
        channel: windfall_project::ChannelId(900),
    };
    let mut audio = runtime.instrument(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    audio.note_on(60, 0.9);
    let mut left = [0.0; 64];
    let mut right = left;
    audio.process(&mut left, &mut right);
    for mode in 0..3 {
        let resume = pause_adapter(&runtime, audio.as_mut());
        audio.note_off(60);
        audio.note_on(62, 0.4);
        audio.note_on(65, 0.7);
        audio.note_off(62);
        if mode == 1 {
            audio.all_notes_off();
        }
        audio.process(&mut left, &mut right);
        assert_eq!(left, [0.0; 64], "paused instrument is silent");
        resume.send(()).unwrap();
        runtime.call(|_| Ok(())).unwrap();
        audio.control_boundary();
        if mode == 2 {
            audio.note_off(65);
        } // release after boundary, before sound
        audio.process(&mut left, &mut right);
        if mode == 0 {
            assert_eq!(audio.voices(), 1);
            assert!(left.iter().any(|sample| sample.abs() > 0.01));
        } else {
            assert_eq!(audio.voices(), 0);
            assert_eq!(left, [0.0; 64], "release/panic cannot resurrect keys");
        }
    }
}

#[test]
fn engine_cache_live_hardware_epochs_and_stop_are_reconciled_during_pause() {
    let (runtime, binding) = fixture(2);
    let mut document = Document::new(Project::new("Paused engine events"));
    document
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = document.project().channels[0].id;
    let target = PluginTarget::Instrument { channel };
    let mut pool = windfall_engine::SamplePool::new();
    pool.set_plugin_factory(runtime.clone());
    let (mut processor, controller) = windfall_engine::Processor::new(48_000);
    controller.set_project(document.project(), &pool);
    let mut out = [0.0; 256];
    controller.note_on(channel, 60, 0.8);
    processor.process(&mut out);
    for mode in 0..3 {
        let token = runtime.selected_token(target).unwrap();
        let (ready, ready_rx) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        runtime
            .jobs
            .send(Box::new(move |owner| {
                let record = owner.instances.get_mut(&token).unwrap();
                record.ownership.request();
                let adapter = loop {
                    if let Some(adapter) = record.ownership.take_returned() {
                        break adapter;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                };
                record.release(adapter).unwrap();
                ready.send(()).unwrap();
                resume_rx.recv().unwrap();
                record.restore().unwrap();
            }))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
                0
            );
            if ready_rx.try_recv().is_ok() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        document
            .dispatch(
                Command::SetPluginParam {
                    target,
                    id: 7,
                    value: 0.2 + mode as f32 * 0.1,
                },
                None,
            )
            .unwrap();
        controller.set_project(document.project(), &pool);
        controller.note_off(channel, 60);
        let epoch = controller.hardware_epoch();
        assert!(controller.hardware_note(epoch, channel, 62, 100));
        controller.note_on(channel, 65, 0.7);
        processor.process(&mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
        controller.panic_hardware();
        assert!(!controller.hardware_note(epoch, channel, 62, 100));
        if mode == 1 {
            controller.note_off(channel, 65);
        }
        if mode == 2 {
            controller.stop();
        }
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        resume.send(()).unwrap();
        runtime.call(|_| Ok(())).unwrap();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        if mode == 0 {
            assert!(out.iter().any(|sample| sample.abs() > 0.01));
            assert_eq!(
                controller.frame().voices,
                1,
                "hardware panic preserves UI ownership"
            );
            let captured = runtime
                .call(move |owner| {
                    Ok(owner
                        .instances
                        .get_mut(&token)
                        .unwrap()
                        .plugin
                        .param_value(7))
                })
                .unwrap();
            assert_eq!(
                captured,
                Some(0.2_f32 as f64),
                "cached final parameter must reach native without resend"
            );
        } else {
            // Drain only the fixed PDC buffers after release/stop.
            for _ in 0..8 {
                processor.process(&mut out);
            }
            assert_eq!(controller.frame().voices, 0);
            assert!(out.iter().all(|sample| *sample == 0.0));
        }
    }
}

#[test]
fn song_automation_and_sequenced_expiry_reconcile_before_resumed_processing() {
    use windfall_project::{
        AutomationId, AutomationPoint, AutomationTarget, ClipContent, ClipInit, NoteInit,
    };
    let (runtime, binding) = fixture(2);
    let mut document = Document::new(Project::new("Paused song automation"));
    document
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = document.project().channels[0].id;
    let pattern = document.project().patterns[0].id;
    let target = PluginTarget::Instrument { channel };
    document
        .dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![NoteInit {
                    start: 0,
                    length: 24,
                    key: 60,
                    velocity: Some(0.8),
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
    document
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap();
    let track = document.project().playlist.tracks[0].id;
    let point = |tick, value| AutomationPoint {
        tick,
        value,
        curve: 0.0,
        hold: true,
    };
    let added = document
        .dispatch(
            Command::AddAutomation {
                name: None,
                target: AutomationTarget::InstrumentParam { channel, param: 0 },
                points: Some(vec![point(0, 0.25), point(48, 0.8)]),
            },
            None,
        )
        .unwrap();
    let automation = AutomationId(added.created[0]);
    document
        .dispatch(
            Command::AddClips {
                clips: vec![
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Pattern { pattern },
                    },
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation { automation },
                    },
                ],
            },
            None,
        )
        .unwrap();
    let mut pool = windfall_engine::SamplePool::new();
    pool.set_plugin_factory(runtime.clone());
    let (mut processor, controller) = windfall_engine::Processor::new(48_000);
    controller.set_project(document.project(), &pool);
    controller.set_transport(windfall_ipc::TransportPatch {
        mode: Some(windfall_ipc::PlayMode::Song),
        ..Default::default()
    });
    controller.play();
    let mut out = [0.0; 256];
    processor.process(&mut out);
    let token = runtime.selected_token(target).unwrap();
    let (ready, ready_rx) = mpsc::channel();
    let (resume, resume_rx) = mpsc::channel();
    runtime
        .jobs
        .send(Box::new(move |owner| {
            let record = owner.instances.get_mut(&token).unwrap();
            record.ownership.request();
            let adapter = loop {
                if let Some(adapter) = record.ownership.take_returned() {
                    break adapter;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            record.release(adapter).unwrap();
            ready.send(()).unwrap();
            resume_rx.recv().unwrap();
            record.restore().unwrap();
        }))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        if ready_rx.try_recv().is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Pass the sequenced note end and the automation corner while native
    // ownership is absent. Engine caches will not send this held value again.
    for _ in 0..20 {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
    }
    assert!(
        controller
            .frame()
            .automated
            .iter()
            .any(|value| value.automation == automation && value.value == 0.8)
    );
    resume.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    for _ in 0..8 {
        processor.process(&mut out);
    }
    assert_eq!(
        controller.frame().voices,
        0,
        "expired sequencer notes must not be replayed"
    );
    assert!(out.iter().all(|sample| *sample == 0.0));
    assert_eq!(
        runtime
            .call(move |owner| Ok(owner
                .instances
                .get_mut(&token)
                .unwrap()
                .plugin
                .param_value(7)))
            .unwrap(),
        Some(0.8_f32 as f64)
    );
}

#[test]
fn state_replacement_preserves_current_key_owners_and_removal_silences_them() {
    let (runtime, binding) = fixture(2);
    let mut document = Document::new(Project::new("Native state replacement"));
    document
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = document.project().channels[0].id;
    let target = PluginTarget::Instrument { channel };
    let mut pool = windfall_engine::SamplePool::new();
    pool.set_plugin_factory(runtime.clone());
    let (mut processor, controller) = windfall_engine::Processor::new(48_000);
    controller.set_project(document.project(), &pool);
    controller.note_on(channel, 60, 0.7);
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 65, 100));
    let mut out = [0.0; 256];
    processor.process(&mut out);
    let before = runtime.selected_token(target).unwrap();
    // Force a native replacement as a dirty state/rescan refresh does.
    runtime.retry();
    controller.set_project(document.project(), &pool);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    assert_ne!(runtime.selected_token(target), Some(before));
    assert_eq!(controller.frame().voices, 2);
    controller.panic_hardware();
    processor.process(&mut out);
    assert_eq!(controller.frame().voices, 1);
    controller.note_off(channel, 60);
    processor.process(&mut out);
    assert_eq!(controller.frame().voices, 0);
    runtime.retry();
    controller.set_project(document.project(), &pool);
    processor.process(&mut out);
    assert_eq!(
        controller.frame().voices,
        0,
        "replacement must not revive released keys"
    );
    controller.note_on(channel, 67, 0.8);
    processor.process(&mut out);
    document
        .dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    controller.set_project(document.project(), &pool);
    for _ in 0..40 {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
    }
    assert_eq!(controller.frame().voices, 0);
    assert!(out.iter().all(|sample| *sample == 0.0));
}

#[test]
fn capture_refusal_returns_the_exact_active_adapter_and_owner_retirement_cleans_up() {
    for (gain, recover) in [(0.5, false), (0.5, true), (0.75, false), (0.75, true)] {
        let (runtime, binding) = fixture(6);
        let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
        audio.transport(transport());
        audio.set_param(7, gain);
        audio.process(&mut [1.0; 64], &mut [1.0; 64]);
        let token = runtime.selected_token(binding.target).unwrap();
        let mut project = Project::new("Refusal");
        project.plugins.push(binding.clone());
        let error = capture(&runtime, project, audio.as_mut()).unwrap_err();
        assert!(
            error.contains(if gain == 0.75 {
                "setProcessing(false) refused"
            } else {
                "setActive(false) refused"
            }),
            "{error}"
        );
        runtime
            .call(move |owner| {
                assert!(owner.instances[&token].plugin.is_active());
                Ok(())
            })
            .unwrap();
        if recover {
            audio.set_param(7, 0.25);
        }
        let mut left = [1.0; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        assert_eq!(left, [if recover { 0.5 } else { gain * 2.0 }; 64]);
        drop(audio);
        runtime
            .call(|owner| {
                assert!(owner.instances.is_empty());
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn recovery_failure_stays_dry_and_can_be_retired_on_its_owner() {
    let (runtime, binding) = fixture(8);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    let mut project = Project::new("Recovery failure");
    project.plugins.push(binding);
    let error = capture(&runtime, project, audio.as_mut()).unwrap_err();
    assert!(error.contains("activated"), "{error}");
    let mut left = [0.25; 64];
    let mut right = left;
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            audio.set_param(7, 0.1);
            audio.control_boundary();
            audio.process(&mut left, &mut right);
        }),
        0
    );
    assert_eq!(left, [0.25; 64]);
    drop(audio);
    runtime
        .call(|owner| {
            assert!(owner.instances.is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn queued_timeout_cancels_execution_and_running_timeout_joins_recovery() {
    #[derive(Debug)]
    struct OwnerResult(std::thread::ThreadId, mpsc::Sender<()>);
    impl Drop for OwnerResult {
        fn drop(&mut self) {
            assert_eq!(
                std::thread::current().id(),
                self.0,
                "cancelled results must stay on their creating owner"
            );
            self.1.send(()).unwrap();
        }
    }
    let runtime = Runtime::new().unwrap();
    let (release, blocked) = mpsc::channel();
    let (started, wait_started) = mpsc::channel();
    runtime
        .jobs
        .send(Box::new(move |_| {
            started.send(()).unwrap();
            blocked.recv().unwrap();
        }))
        .unwrap();
    wait_started.recv().unwrap();
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let check = ran.clone();
    assert!(
        runtime
            .call_with_lifetime(Duration::from_millis(5), move |_, _| {
                check.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })
            .is_err()
    );
    release.send(()).unwrap();
    runtime.call(|_| Ok(())).unwrap();
    assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
    let (started, wait_started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let handle = runtime.clone();
    let (dropped, wait_dropped) = mpsc::channel();
    let task = std::thread::spawn(move || {
        handle.call_with_lifetime(Duration::from_millis(5), move |_, lifetime| {
            started.send(()).unwrap();
            while !lifetime.cancelled() {
                std::thread::yield_now();
            }
            blocked.recv().unwrap(); // native work/recovery still owns the exclusion
            Ok(OwnerResult(std::thread::current().id(), dropped))
        })
    });
    wait_started.recv().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        !task.is_finished(),
        "a running native job cannot outlive its caller's guard"
    );
    release.send(()).unwrap();
    assert!(task.join().unwrap().unwrap_err().contains("cancelled"));
    wait_dropped.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn vst3_return_capture_reacquire_save_reopen_and_offline_render() {
    let (runtime, mut binding) = fixture(0);
    binding
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.75;
    let mut document = Document::new(Project::new("VST3 ownership"));
    document
        .dispatch(
            Command::AddPluginEffect {
                track: TrackId(0),
                plugin: binding.clone(),
            },
            None,
        )
        .unwrap();
    binding = document.project().plugins[0].clone();
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    audio.set_param(7, 0.75); // still pending at the ownership boundary
    let token = runtime.selected_token(binding.target).unwrap();
    let saved = capture(&runtime, document.project().clone(), audio.as_mut()).unwrap();
    assert_ne!(saved.plugins[0].state, binding.state);
    assert_eq!(runtime.selected_token(binding.target), Some(token));
    assert_eq!(
        audio.latency(),
        17,
        "pause retains the installed compensation"
    );
    let mut left = [1.0; 64];
    let mut right = left;
    audio.process(&mut left, &mut right);
    assert_eq!(left, [1.5; 64]);
    // Once deferred edits reach process, the same semantic state has a
    // canonical encoding without deferred overrides. Compare export ownership
    // against this stable pre-export state, not the earlier pending encoding.
    let steady = capture(&runtime, document.project().clone(), audio.as_mut()).unwrap();

    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("fixture.windfall");
    windfall_project::file::save(&saved, &path).unwrap();
    let reopened = windfall_project::file::load(&path).unwrap();
    assert_eq!(reopened.plugins, saved.plugins);
    let render = runtime.render_factory().unwrap();
    let mut offline = render.effect(&reopened.plugins[0], 48_000, 64).unwrap();
    offline.transport(transport());
    left.fill(1.0);
    right.fill(1.0);
    offline.process(&mut left, &mut right);
    // The document's explicit parameter value takes precedence over opaque
    // state on reopen, matching CLAP's existing discovery/load policy.
    let value = reopened.plugins[0]
        .parameters
        .iter()
        .find(|p| p.id == 7)
        .unwrap()
        .value;
    assert_eq!(left, [value * 2.0; 64]);
    assert_eq!(
        runtime.selected_token(binding.target),
        Some(token),
        "offline instances cannot become playback owners"
    );
    let again = capture(&runtime, document.project().clone(), audio.as_mut()).unwrap();
    assert_eq!(again.plugins[0].state, steady.plugins[0].state);
    drop(offline);
    drop(audio);
    runtime
        .call(|owner| {
            assert!(owner.instances.is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn vst3_instrument_engine_playback_state_reopen_and_wav_export_are_deterministic() {
    let (runtime, mut binding) = fixture(2);
    binding
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = 0.6;
    let mut document = Document::new(Project::new("VST3 instrument"));
    document
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = document.project().channels[0].id;
    let pattern = document.project().patterns[0].id;
    document
        .dispatch(
            Command::ToggleStep {
                pattern,
                channel,
                step: 0,
            },
            None,
        )
        .unwrap();
    let project = document.project().clone();
    let mut pool = windfall_engine::SamplePool::new();
    pool.set_plugin_factory(runtime.clone());
    let (mut processor, controller) = windfall_engine::Processor::new(48_000);
    controller.set_project(&project, &pool);
    controller.play();
    let mut output = [0.0; 512];
    processor.process(&mut output);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 0.01));
    let token = runtime.selected_token(project.plugins[0].target).unwrap();
    let capture_runtime = runtime.clone();
    let capture_project = project.clone();
    let task = std::thread::spawn(move || capture_runtime.capture(capture_project));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        processor.process(&mut output);
        std::thread::sleep(Duration::from_millis(1));
    }
    let captured = task.join().unwrap().unwrap();
    processor.process(&mut output);
    assert_eq!(
        runtime.selected_token(project.plugins[0].target),
        Some(token)
    );
    let folder = tempfile::tempdir().unwrap();
    let saved = folder.path().join("instrument.windfall");
    windfall_project::file::save(&captured, &saved).unwrap();
    let reopened = windfall_project::file::load(&saved).unwrap();
    assert_eq!(captured.plugins, reopened.plugins);
    let options = windfall_engine::RenderOptions {
        tail_secs: 0.0,
        ..Default::default()
    };
    let first = windfall_engine::render(&captured, &pool, &options, &mut |_| true);
    let second = windfall_engine::render(&reopened, &pool, &options, &mut |_| true);
    assert_eq!(first.samples(), second.samples());
    assert!(first.samples().iter().any(|sample| sample.abs() > 0.01));
    let exported = folder.path().join("instrument.wav");
    windfall_codec::write_wav(&exported, &second, windfall_codec::WavSampleFormat::Float32)
        .unwrap();
    assert_eq!(
        windfall_codec::decode_file(&exported).unwrap().samples(),
        second.samples()
    );
    assert_eq!(
        runtime.selected_token(project.plugins[0].target),
        Some(token)
    );
}

#[test]
fn vst3_no_callback_refuses_capture_without_deactivating_the_processor() {
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    let mut project = Project::new("Disconnected callback");
    project.plugins = vec![binding];
    let error = runtime.capture(project).unwrap_err();
    assert!(error.contains("block boundary"), "{error}");
    audio.control_boundary();
    let mut left = [1.0; 64];
    let mut right = left;
    audio.process(&mut left, &mut right);
    assert_eq!(left, [1.0; 64]);
    runtime
        .call(|owner| {
            assert!(
                owner
                    .instances
                    .values()
                    .all(|record| record.plugin.is_active())
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn vst3_late_return_and_retirement_are_serviced_by_the_owner() {
    let (runtime, binding) = fixture(0);
    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    runtime
        .call(|owner| {
            owner.instances.values().next().unwrap().ownership.request();
            Ok(())
        })
        .unwrap();
    audio.control_boundary();
    // Model timeout cancellation after audio has already relinquished ownership.
    runtime
        .call(|owner| {
            owner.instances.values().next().unwrap().ownership.cancel();
            Ok(())
        })
        .unwrap();
    runtime.call(|_| Ok(())).unwrap(); // owner maintenance completes before next job
    audio.control_boundary();
    let mut left = [1.0; 64];
    let mut right = left;
    audio.process(&mut left, &mut right);
    assert_eq!(left, [1.0; 64]);
    // Also retire while a resume is queued but not yet reacquired.
    runtime
        .call(|owner| {
            owner.instances.values().next().unwrap().ownership.request();
            Ok(())
        })
        .unwrap();
    audio.control_boundary();
    runtime.call(|_| Ok(())).unwrap();
    drop(audio);
    runtime
        .call(|owner| {
            assert!(owner.instances.is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn vst3_speculation_revision_binding_and_failed_units_keep_their_roles() {
    let (runtime, binding) = fixture(0);
    let mut first = runtime.effect(&binding, 48_000, 64).unwrap();
    first.transport(transport());
    let token = runtime.selected_token(binding.target).unwrap();
    let staged = runtime.prepare_document();
    let speculative = staged.effect(&binding, 48_000, 64).unwrap();
    assert_eq!(runtime.selected_token(binding.target), Some(token));
    drop(speculative);
    drop(staged);
    let mut project = Project::new("Stale snapshot");
    project.plugins = vec![binding.clone()];
    let revision = runtime.document_revision();
    runtime.retry();
    assert_eq!(
        runtime
            .capture_at(project.clone(), revision)
            .unwrap()
            .plugins,
        project.plugins
    );
    assert!(runtime.editor(binding.target, false).is_err());

    let (failed_runtime, failed_binding) = fixture(4);
    let mut failed = failed_runtime.effect(&failed_binding, 48_000, 64).unwrap();
    failed.transport(transport());
    let mut left = [0.25; 64];
    let mut right = left;
    failed.process(&mut left, &mut right);
    project.plugins = vec![failed_binding];
    let error = capture(&failed_runtime, project, failed.as_mut()).unwrap_err();
    assert!(error.contains("failed plugin"), "{error}");
    failed.process(&mut left, &mut right);
    assert_eq!(left, [0.25; 64], "failed effects remain bypassed");
}

#[test]
fn production_vst3_requires_scanned_file_identity_format_id_and_role() {
    let (_, binding) = fixture(0);
    let runtime = Runtime::new().unwrap();
    assert!(
        runtime
            .discover(binding.clone())
            .unwrap_err()
            .contains("unscanned")
    );
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: binding.path.clone(),
        id: binding.id.clone(),
        format: "vst3".into(),
        name: binding.name.clone(),
        vendor: "Fixture".into(),
        instrument: false,
        usable: true,
        error: None,
    }]);
    assert!(runtime.discover(binding.clone()).is_ok());
    let mut wrong = binding.clone();
    wrong.format = "clap".into();
    assert!(
        runtime
            .discover(wrong)
            .unwrap_err()
            .contains("declared format")
    );
    let mut wrong = binding.clone();
    wrong.target = PluginTarget::Instrument {
        channel: windfall_project::ChannelId(900),
    };
    assert!(runtime.discover(wrong).unwrap_err().contains("role"));
    let mut wrong = binding.clone();
    wrong.id = "ffffffffffffffffffffffffffffffff".into();
    assert!(runtime.discover(wrong).unwrap_err().contains("unscanned"));
    runtime.approve(&[]);
    assert!(runtime.discover(binding).unwrap_err().contains("unscanned"));
}
