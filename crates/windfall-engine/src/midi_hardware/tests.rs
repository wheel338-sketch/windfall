use super::*;
use crate::{Processor, SamplePool};
use windfall_core::AudioBuffer;
use windfall_project::{
    Channel, ChannelId, ChannelSource, Project, SampleId, SamplerLoopMode, SamplerSettings, TrackId,
};

#[derive(Default)]
struct FakeState {
    ingress: Option<Ingress>,
    messages: Vec<[u8; 3]>,
    present: bool,
    fail_output: bool,
    owners: Vec<thread::ThreadId>,
}
struct FakePorts(Arc<Mutex<FakeState>>);
struct FakeInput(Arc<Mutex<FakeState>>);
impl Drop for FakeInput {
    fn drop(&mut self) {
        self.0.lock().unwrap().ingress.take();
    }
}
struct FakeOutput(Arc<Mutex<FakeState>>);
impl Output for FakeOutput {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.owners.push(thread::current().id());
        if state.fail_output {
            return Err("fake output failed".into());
        }
        state.messages.push(bytes.try_into().unwrap());
        Ok(())
    }
}
impl Ports for FakePorts {
    fn enumerate(&mut self) -> Result<(Vec<MidiPort>, Vec<MidiPort>), String> {
        let mut state = self.0.lock().unwrap();
        state.owners.push(thread::current().id());
        let ports = if state.present {
            vec![MidiPort {
                id: "port".into(),
                name: "Fake port".into(),
            }]
        } else {
            vec![]
        };
        Ok((ports.clone(), ports))
    }
    fn input(&mut self, id: &str, ingress: Ingress) -> Result<Box<dyn Send>, String> {
        let mut state = self.0.lock().unwrap();
        state.owners.push(thread::current().id());
        if !state.present || id != "port" {
            return Err("fake input unavailable".into());
        }
        state.ingress = Some(ingress);
        Ok(Box::new(FakeInput(self.0.clone())))
    }
    fn output(&mut self, id: &str) -> Result<Box<dyn Output>, String> {
        if !self.0.lock().unwrap().present || id != "port" {
            return Err("fake output unavailable".into());
        }
        Ok(Box::new(FakeOutput(self.0.clone())))
    }
}
struct Target(Controller);
impl Audition for Target {
    fn note(&self, epoch: u64, key: u8, velocity: u8) -> bool {
        self.0.hardware_note(epoch, ChannelId(901), key, velocity)
    }
}
fn engine() -> (Processor, Controller) {
    engine_with_loop(SamplerLoopMode::Off)
}
fn engine_with_loop(loop_mode: SamplerLoopMode) -> (Processor, Controller) {
    let (processor, controller) = Processor::new(48_000);
    let mut project = Project::new("MIDI");
    project.channels.push(Channel {
                sample_playback: None,
        id: ChannelId(901),
        name: "Test".into(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        group: String::new(),
        voice: Default::default(),
        expression_map: Default::default(),
        timing: windfall_project::ChannelTiming::default(),
        mixer_track: TrackId::MASTER,
        source: ChannelSource::Sampler(SamplerSettings {
            sample: Some(SampleId(900)),
            loop_mode,
            ..Default::default()
        }),
    });
    let mut pool = SamplePool::new();
    pool.insert(
        SampleId(900),
        AudioBuffer::from_interleaved(48_000, 1, vec![0.5; 48_000]),
    );
    controller.set_project(&project, &pool);
    (processor, controller)
}
fn rig() -> (Processor, Controller, Worker, Arc<Mutex<FakeState>>) {
    rig_with_loop(SamplerLoopMode::Off)
}
fn rig_with_loop(mode: SamplerLoopMode) -> (Processor, Controller, Worker, Arc<Mutex<FakeState>>) {
    let (mut processor, controller) = engine_with_loop(mode);
    processor.process(&mut [0.0; 128]);
    let state = Arc::new(Mutex::new(FakeState {
        present: true,
        ..Default::default()
    }));
    let mut worker = Worker::new(
        Box::new(FakePorts(state.clone())),
        controller.clone(),
        Arc::new(Target(controller.clone())),
    );
    worker.configure(MidiHardwareSettings {
        input: Some("port".into()),
        output: Some("port".into()),
        output_channel: 3,
        ..Default::default()
    });
    (processor, controller, worker, state)
}
fn input(state: &Mutex<FakeState>, bytes: &[u8]) {
    state
        .lock()
        .unwrap()
        .ingress
        .as_mut()
        .unwrap()
        .receive(bytes);
}

#[test]
fn hardware_loop_sustain_release_and_panic_preserve_callback_allocation_contract() {
    for mode in [SamplerLoopMode::Forward, SamplerLoopMode::PingPong] {
        let (mut processor, controller, mut worker, state) = rig_with_loop(mode);
        let mut out = [0.0; 2048];
        input(&state, &[0x90, 60, 100]);
        worker.service();
        // Both modes must keep playing beyond the source's one-second length.
        for _ in 0..64 {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
                0
            );
        }
        assert_eq!(controller.frame().voices, 1);
        assert!(out.iter().any(|sample| *sample != 0.0));

        input(&state, &[0xb0, 64, 127]);
        input(&state, &[0x80, 60, 0]);
        worker.service();
        processor.process(&mut out);
        assert_eq!(controller.frame().voices, 1);

        input(&state, &[0xb0, 64, 0]);
        worker.service();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(controller.frame().voices, 0);
        assert_eq!(state.lock().unwrap().messages.last(), Some(&[0x82, 60, 0]));

        input(&state, &[0x90, 60, 100]);
        worker.service();
        processor.process(&mut out);
        assert_eq!(controller.frame().voices, 1);
        controller.panic_hardware();
        worker.service();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(controller.frame().voices, 0);
    }
}

#[test]
fn hardware_parser_is_bounded_and_velocity_zero_means_note_off() {
    assert_eq!(
        Event::parse(&[0x9f, 60, 0]),
        Some(Event::Note {
            channel: 15,
            key: 60,
            velocity: 0
        })
    );
    assert_eq!(
        Event::parse(&[0x80, 60, 99]),
        Some(Event::Note {
            channel: 0,
            key: 60,
            velocity: 0
        })
    );
    for bytes in [
        &[][..],
        &[0x90][..],
        &[0x90, 60][..],
        &[0x90, 128, 5][..],
        &[0x90, 60, 128][..],
        &[0xf0, 1, 2, 0xf7][..],
        &[0xe0, 0, 64][..],
    ] {
        assert_eq!(Event::parse(bytes), None);
    }
}

#[test]
fn hardware_sustain_filter_and_multichannel_release_are_exact() {
    let (_, _, mut worker, state) = rig();
    input(&state, &[0x90, 60, 100]);
    input(&state, &[0xb0, 64, 127]);
    input(&state, &[0x80, 60, 0]);
    worker.service();
    assert_eq!(state.lock().unwrap().messages, vec![[0x92, 60, 100]]);
    input(&state, &[0x91, 60, 90]);
    worker.service();
    let count = state.lock().unwrap().messages.len();
    input(&state, &[0xb0, 64, 0]);
    worker.service();
    assert_eq!(state.lock().unwrap().messages.len(), count);
    input(&state, &[0x81, 60, 0]);
    worker.service();
    assert_eq!(state.lock().unwrap().messages.last(), Some(&[0x82, 60, 0]));
    worker.state.settings.input_channel = Some(1);
    let count = state.lock().unwrap().messages.len();
    input(&state, &[0x91, 64, 100]);
    worker.service();
    assert_eq!(state.lock().unwrap().messages.len(), count);
}

#[test]
fn hardware_callbacks_and_audio_panic_never_allocate_or_free() {
    let (mut processor, controller, mut worker, state) = rig();
    let mut ingress = state.lock().unwrap().ingress.take().unwrap();
    let calls = crate::test_alloc::allocator_calls(|| ingress.receive(&[0x90, 60, 100]));
    assert_eq!(calls, 0);
    worker.service();
    let mut out = [0.0; 2048];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    assert!(out.iter().any(|x| *x != 0.0));
    controller.panic_hardware();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    assert_eq!(controller.frame().voices, 0);
}

#[test]
fn hardware_synth_panic_preserves_ui_notes_and_new_streams_discard_old_epochs() {
    use windfall_project::{Command, Document, InstrumentKind};
    let (mut processor, controller) = Processor::new(48_000);
    let mut doc = Document::new(Project::new("Synth"));
    doc.dispatch(
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        },
        None,
    )
    .unwrap();
    let channel = doc.project().channels[0].id;
    controller.set_project(doc.project(), &SamplePool::new());
    controller.note_on(channel, 69, 1.0);
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 60, 100));
    let mut out = [0.0; 2048];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    controller.panic_hardware();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
        0
    );
    assert!(out.iter().any(|sample| sample.abs() > 0.01));
    controller.note_off(channel, 69);
    for _ in 0..64 {
        processor.process(&mut out);
    }
    assert_eq!(controller.frame().voices, 0);
    let old = controller.hardware_epoch();
    assert!(controller.hardware_note(old, channel, 60, 100));
    controller.suspend();
    processor = controller.attach(48_000);
    assert!(!controller.hardware_note(old, channel, 60, 100));
    processor.process(&mut out);
    assert_eq!(controller.frame().voices, 0);
}

#[test]
fn hardware_overflow_invalidates_stale_notes_and_outputs_panic() {
    let (mut processor, controller, mut worker, state) = rig();
    input(&state, &[0x90, 60, 100]);
    worker.service();
    processor.process(&mut [0.0; 128]);
    for _ in 0..=INPUT_CAPACITY {
        input(&state, &[0x90, 61, 100]);
    }
    worker.service();
    processor.process(&mut [0.0; 2048]);
    assert_eq!(controller.frame().voices, 0);
    assert!(worker.state().dropped_events > 0);
    let sent = &state.lock().unwrap().messages;
    assert!(sent.contains(&[0x82, 60, 0]));
    assert!(sent.contains(&[0xb2, 120, 0]));
    assert!(!sent.contains(&[0x92, 61, 100]));
}

#[test]
fn hardware_stop_full_engine_queue_disconnect_and_device_change_cannot_hang() {
    let (mut processor, controller, mut worker, state) = rig();
    input(&state, &[0x90, 60, 100]);
    worker.service();
    processor.process(&mut [0.0; 128]);
    let old = controller.hardware_epoch();
    controller.stop();
    worker.service();
    assert!(!controller.hardware_note(old, ChannelId(901), 61, 100));
    processor.process(&mut [0.0; 2048]);
    assert_eq!(controller.frame().voices, 0);
    input(&state, &[0x90, 60, 100]);
    worker.service();
    let epoch = controller.hardware_epoch();
    let mut accepted = 0;
    while controller.hardware_note(epoch, ChannelId(901), 60, 100) {
        accepted += 1;
    }
    assert!(accepted <= crate::message::MESSAGE_CAPACITY);
    input(&state, &[0x80, 60, 0]);
    worker.service();
    processor.process(&mut [0.0; 2048]);
    assert_eq!(controller.frame().voices, 0);
    input(&state, &[0x90, 60, 100]);
    worker.service();
    state.lock().unwrap().present = false;
    worker.refresh();
    assert!(!worker.state().input_connected && !worker.state().output_connected);
    assert!(
        worker
            .state()
            .error
            .as_deref()
            .unwrap()
            .contains("disconnected")
    );
    processor.process(&mut [0.0; 2048]);
    assert_eq!(controller.frame().voices, 0);
    worker.configure(MidiHardwareSettings::default());
    assert!(!worker.state().input_connected);
}

#[test]
fn hardware_runtime_keeps_ports_on_one_worker_and_reports_open_send_failures() {
    let (_processor, controller) = engine();
    let fake = Arc::new(Mutex::new(FakeState {
        present: true,
        ..Default::default()
    }));
    let ports = fake.clone();
    let runtime = Runtime::start_with(
        controller.clone(),
        Arc::new(Target(controller)),
        MidiHardwareSettings::default(),
        move || Box::new(FakePorts(ports)),
    )
    .unwrap();
    let configured = runtime
        .configure(MidiHardwareSettings {
            input: Some("port".into()),
            output: Some("port".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(configured.input_connected && configured.output_connected);
    let failed = runtime
        .configure(MidiHardwareSettings {
            input: Some("missing".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(!failed.input_connected && failed.error.is_some());
    drop(runtime);
    let owners = &fake.lock().unwrap().owners;
    assert!(owners.iter().all(|owner| *owner == owners[0]));
    assert_ne!(owners[0], thread::current().id());
    let (_, controller, mut worker, fake) = rig();
    fake.lock().unwrap().fail_output = true;
    input(&fake, &[0x90, 60, 100]);
    worker.service();
    assert!(!worker.state().output_connected);
    assert!(worker.state().error.is_some());
    assert_ne!(worker.epoch, controller.hardware_epoch());
}
