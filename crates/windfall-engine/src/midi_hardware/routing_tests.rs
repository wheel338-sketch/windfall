//! Several inputs, controllers and the wheel reaching the sink, MIDI clock
//! and MIDI output channels, all against fake ports and an engine the test
//! runs by hand.
use super::*;
use crate::{Processor, SamplePool};
use std::collections::HashMap;
use windfall_ipc::{MidiInputSettings, PlayMode, TransportPatch};
use windfall_project::{
    ChannelId, Command, Document, InstrumentKind, InstrumentParams, NoteInit, PatternId, Project,
};

#[derive(Default)]
struct Fake {
    ingress: HashMap<String, Ingress>,
    /// Everything sent to the output, in order.
    sent: Vec<Vec<u8>>,
    routed: Vec<(String, Vec<u8>)>,
    /// Port ids that exist.
    present: Vec<String>,
}
type Handle = Arc<Mutex<Fake>>;
struct FakePorts(Handle);
struct FakeInput(Handle, String);
impl Drop for FakeInput {
    fn drop(&mut self) {
        self.0.lock().unwrap().ingress.remove(&self.1);
    }
}
struct FakeOutput(Handle, String);
impl Output for FakeOutput {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut fake = self.0.lock().unwrap();
        fake.sent.push(bytes.to_vec());
        fake.routed.push((self.1.clone(), bytes.to_vec()));
        Ok(())
    }
}
impl Ports for FakePorts {
    fn enumerate(&mut self) -> Result<(Vec<MidiPort>, Vec<MidiPort>), String> {
        let ports: Vec<MidiPort> = self
            .0
            .lock()
            .unwrap()
            .present
            .iter()
            .map(|id| MidiPort {
                id: id.clone(),
                name: format!("Fake {id}"),
            })
            .collect();
        Ok((ports.clone(), ports))
    }
    fn input(&mut self, id: &str, ingress: Ingress) -> Result<Box<dyn Send>, String> {
        let mut fake = self.0.lock().unwrap();
        if !fake.present.iter().any(|port| port == id) {
            return Err(format!("{id} is unavailable"));
        }
        fake.ingress.insert(id.to_owned(), ingress);
        Ok(Box::new(FakeInput(self.0.clone(), id.to_owned())))
    }
    fn output(&mut self, id: &str) -> Result<Box<dyn Output>, String> {
        if !self.0.lock().unwrap().present.iter().any(|port| port == id) {
            return Err(format!("{id} is unavailable"));
        }
        Ok(Box::new(FakeOutput(self.0.clone(), id.to_owned())))
    }
}

/// A sink that keeps what it is told and can be told to take notes.
#[derive(Default)]
struct Sink {
    messages: Mutex<Vec<InputMessage>>,
    notes: Mutex<Vec<(u8, u8)>>,
    take_key: Mutex<Option<u8>>,
    idles: AtomicU32,
}
impl Audition for Sink {
    fn note(&self, _epoch: u64, key: u8, velocity: u8) -> bool {
        self.notes.lock().unwrap().push((key, velocity));
        true
    }
    fn message(&self, input: InputMessage) -> bool {
        self.messages.lock().unwrap().push(input);
        let taken = *self.take_key.lock().unwrap();
        match input.message {
            MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                taken == Some(key)
            }
            _ => false,
        }
    }
    fn idle(&self, _now: Instant, _playhead: &Playhead) {
        self.idles.fetch_add(1, Ordering::Relaxed);
    }
}

fn fake(present: &[&str]) -> Handle {
    Arc::new(Mutex::new(Fake {
        present: present.iter().map(|id| (*id).to_owned()).collect(),
        ..Fake::default()
    }))
}

fn worker(
    controller: &Controller,
    fake: &Handle,
    settings: MidiHardwareSettings,
) -> (Worker, Arc<Sink>) {
    let sink = Arc::new(Sink::default());
    let mut worker = Worker::new(
        Box::new(FakePorts(fake.clone())),
        controller.clone(),
        sink.clone(),
    );
    worker.configure(settings);
    (worker, sink)
}

fn input(fake: &Handle, id: &str, bytes: &[u8]) {
    fake.lock()
        .unwrap()
        .ingress
        .get_mut(id)
        .expect("the input is open")
        .receive_at(7, bytes);
}

fn two_inputs() -> MidiHardwareSettings {
    MidiHardwareSettings {
        input: Some("keys".into()),
        extra_inputs: vec![MidiInputSettings {
            id: "knobs".into(),
            profile: None,
        }],
        ..Default::default()
    }
}

#[test]
fn every_message_reaches_the_sink_with_its_input_and_capture_time() {
    let (_processor, controller) = Processor::new(48_000);
    let fake = fake(&["keys", "knobs"]);
    let (mut worker, sink) = worker(&controller, &fake, two_inputs());
    let state = worker.state();
    assert!(state.input_connected);
    assert_eq!(state.connected_inputs, ["keys", "knobs"]);

    let before = Instant::now();
    input(&fake, "knobs", &[0xb2, 74, 100]);
    input(&fake, "knobs", &[0xe2, 0, 0x40]);
    input(&fake, "knobs", &[0xd2, 55]);
    input(&fake, "knobs", &[0xc2, 9]);
    input(&fake, "knobs", &[0xf8]);
    input(&fake, "keys", &[0x90, 60, 100]);
    input(&fake, "keys", &[0x80, 60, 0]);
    // Not read: system exclusive and active sensing never reach the queue.
    input(&fake, "keys", &[0xf0, 1, 2, 0xf7]);
    input(&fake, "keys", &[0xfe]);
    worker.service();
    let after = Instant::now();

    let seen = sink.messages.lock().unwrap().clone();
    assert_eq!(seen.len(), 7);
    let from = |device: u8| -> Vec<MidiMessage> {
        seen.iter()
            .filter(|message| message.device == device)
            .map(|message| message.message)
            .collect()
    };
    // The main input is device 0; each input keeps its own order.
    assert_eq!(
        from(0),
        [
            MidiMessage::NoteOn {
                channel: 0,
                key: 60,
                velocity: 100
            },
            MidiMessage::NoteOff {
                channel: 0,
                key: 60
            },
        ]
    );
    assert_eq!(
        from(1),
        [
            MidiMessage::Controller {
                channel: 2,
                number: 74,
                value: 100
            },
            MidiMessage::PitchBend {
                channel: 2,
                value: 8192
            },
            MidiMessage::ChannelPressure {
                channel: 2,
                value: 55
            },
            MidiMessage::ProgramChange {
                channel: 2,
                program: 9
            },
            MidiMessage::Clock,
        ]
    );
    for message in &seen {
        assert!(message.at >= before && message.at <= after);
        assert_eq!(message.stamp_micros, 7);
    }
    // Notes from either input are auditioned; controllers are not notes.
    assert_eq!(*sink.notes.lock().unwrap(), [(60, 100), (60, 0)]);
    assert!(sink.idles.load(Ordering::Relaxed) >= 1);
}

#[test]
fn a_note_the_sink_takes_is_not_auditioned_and_the_callback_never_allocates() {
    let (_processor, controller) = Processor::new(48_000);
    let fake = fake(&["keys"]);
    let settings = MidiHardwareSettings {
        input: Some("keys".into()),
        ..Default::default()
    };
    let (mut worker, sink) = worker(&controller, &fake, settings);
    *sink.take_key.lock().unwrap() = Some(36);
    input(&fake, "keys", &[0x99, 36, 127]);
    input(&fake, "keys", &[0x89, 36, 0]);
    input(&fake, "keys", &[0x90, 40, 90]);
    worker.service();
    assert_eq!(sink.messages.lock().unwrap().len(), 3);
    assert_eq!(*sink.notes.lock().unwrap(), [(40, 90)]);

    let mut ingress = fake.lock().unwrap().ingress.remove("keys").unwrap();
    for bytes in [&[0xb0, 1, 64][..], &[0xe0, 1, 2], &[0xf8], &[0x90, 61, 1]] {
        let calls = crate::test_alloc::allocator_calls(|| ingress.receive_at(1, bytes));
        assert_eq!(calls, 0, "{bytes:?}");
    }
}

#[test]
fn losing_one_input_keeps_the_other_and_a_missing_one_is_reported() {
    let (_processor, controller) = Processor::new(48_000);
    let fake = fake(&["keys", "knobs"]);
    let (mut worker, sink) = worker(&controller, &fake, two_inputs());
    fake.lock().unwrap().present.retain(|id| id != "knobs");
    worker.refresh();
    let state = worker.state();
    assert!(state.input_connected);
    assert_eq!(state.connected_inputs, ["keys"]);
    assert!(state.error.as_deref().unwrap().contains("disconnected"));
    input(&fake, "keys", &[0x90, 60, 100]);
    worker.service();
    assert_eq!(*sink.notes.lock().unwrap(), [(60, 100)]);

    // Configuring an input that is not there opens the rest and says so.
    worker.configure(two_inputs());
    let state = worker.state();
    assert_eq!(state.connected_inputs, ["keys"]);
    assert!(state.error.as_deref().unwrap().contains("knobs"));
    // The main input keeps device 0 even when another fails to open.
    input(&fake, "keys", &[0xb0, 7, 1]);
    worker.service();
    assert_eq!(sink.messages.lock().unwrap().last().unwrap().device, 0);
}

/// A project with one MIDI output channel and a pattern of two notes.
fn midi_out_project(channel: i32) -> (Document, ChannelId, PatternId) {
    let mut doc = Document::new(Project::new("MIDI out"));
    let created = doc
        .dispatch(
            Command::AddChannel {
                name: Some("Hardware synth".into()),
                sample: None,
                instrument: Some(InstrumentKind::MidiOut),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created;
    let id = ChannelId(created[0]);
    let mut params = InstrumentKind::MidiOut.default_params();
    let InstrumentParams::MidiOut(out) = &mut params else {
        panic!("the default settings of a MIDI output are its own");
    };
    out.channel = channel;
    out.program = 12;
    doc.dispatch(
        Command::SetInstrumentParams {
            channel: id,
            params,
        },
        None,
    )
    .unwrap();
    let pattern = doc.project().patterns[0].id;
    let note = |start, key| NoteInit {
        start,
        length: 240,
        key,
        velocity: Some(1.0),
        pan: None,
        expression: None,
    };
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel: id,
            notes: vec![note(0, 60), note(480, 64)],
        },
        None,
    )
    .unwrap();
    (doc, id, pattern)
}

fn play_pattern(controller: &Controller, pattern: PatternId) {
    controller.set_transport(TransportPatch {
        mode: Some(PlayMode::Pattern),
        pattern: Some(pattern),
        ..Default::default()
    });
    controller.play();
}

#[test]
fn a_midi_output_channel_queues_its_pattern_with_exact_frames_and_no_allocation() {
    let (mut processor, controller) = Processor::new(48_000);
    let (doc, _, pattern) = midi_out_project(3);
    controller.set_project(doc.project(), &SamplePool::new());
    let mut out = [1.0_f32; 960];
    processor.process(&mut out);
    play_pattern(&controller, pattern);
    // 120 bpm at 48 kHz is 25 frames a tick: the second note ends at
    // tick 720, frame 18 000. Run a little past it.
    for _ in 0..40 {
        let calls = crate::test_alloc::allocator_calls(|| processor.process(&mut out));
        assert_eq!(calls, 0);
        assert!(out.iter().all(|sample| *sample == 0.0), "it makes no sound");
    }
    let mut queued = Vec::new();
    controller.drain_midi_out(|frame, bytes| queued.push((frame, bytes.to_vec())));
    // Status and first data byte of each message, in order.
    let heads: Vec<(u8, u8)> = queued
        .iter()
        .map(|(_, bytes)| (bytes[0], bytes[1]))
        .collect();
    assert_eq!(
        heads,
        [
            // The program is told first, on the channel's own MIDI channel.
            (0xb2, 0),
            (0xb2, 32),
            (0xc2, 11),
            (0x92, 60),
            (0x82, 60),
            (0x92, 64),
            (0x82, 64),
        ]
    );
    // A note-on carries a velocity; a program change is two bytes.
    assert!(queued[3].1[2] > 0 && queued[5].1[2] > 0);
    assert_eq!(queued[2].1.len(), 2);
    let frame = |index: usize| queued[index].0;
    // Each note is as long as it is written, and they are a beat's half apart.
    assert_eq!(frame(4) - frame(3), 240 * 25);
    assert_eq!(frame(5) - frame(3), 480 * 25);
    assert_eq!(frame(6) - frame(5), 240 * 25);
    assert_eq!(controller.midi_out_dropped(), 0);
}

#[test]
fn the_worker_sends_a_midi_output_channel_in_order_and_stop_ends_its_notes() {
    let (mut processor, controller) = Processor::new(48_000);
    let (doc, _, pattern) = midi_out_project(1);
    controller.set_project(doc.project(), &SamplePool::new());
    let fake = fake(&["synth"]);
    let settings = MidiHardwareSettings {
        output: Some("synth".into()),
        output_channel: 16,
        ..Default::default()
    };
    let (mut worker, _sink) = worker(&controller, &fake, settings);
    let mut out = [0.0_f32; 960];
    processor.process(&mut out);
    worker.service();
    fake.lock().unwrap().sent.clear();
    play_pattern(&controller, pattern);
    // Stop while the first note is still held: 100 buffers of 480 frames
    // would reach it, three do not.
    for _ in 0..3 {
        processor.process(&mut out);
        worker.service();
    }
    let playing = fake.lock().unwrap().sent.clone();
    assert_eq!(playing.last().unwrap()[..2], [0x90, 60]);
    assert!(!playing.iter().any(|bytes| bytes[0] == 0x80));

    controller.stop();
    processor.process(&mut out);
    worker.service();
    let sent = fake.lock().unwrap().sent.clone();
    let after = &sent[playing.len()..];
    // The channel's own release, or the panic's all-notes-off on its MIDI
    // channel: either way the hardware is told to let go.
    assert!(
        after.contains(&vec![0x80, 60, 0]) || after.contains(&vec![0xb0, 123, 0]),
        "{after:?}"
    );
    // The live-note channel is quieted by the panic as before.
    assert!(after.contains(&vec![0xbf, 123, 0]));
    assert!(!after.iter().any(|bytes| bytes[0] == 0x90));
}

#[test]
fn with_no_output_open_midi_output_messages_are_discarded_not_kept() {
    let (mut processor, controller) = Processor::new(48_000);
    let (doc, _, pattern) = midi_out_project(1);
    controller.set_project(doc.project(), &SamplePool::new());
    let fake = fake(&["synth"]);
    let (mut worker, _sink) = worker(&controller, &fake, MidiHardwareSettings::default());
    let mut out = [0.0_f32; 960];
    play_pattern(&controller, pattern);
    for _ in 0..40 {
        processor.process(&mut out);
        worker.service();
    }
    // Opening a port later does not replay what was played before.
    worker.configure(MidiHardwareSettings {
        output: Some("synth".into()),
        ..Default::default()
    });
    worker.service();
    let sent = fake.lock().unwrap().sent.clone();
    assert!(
        !sent.iter().any(|bytes| bytes[0] & 0xf0 == 0x90),
        "{sent:?}"
    );
    assert_eq!(controller.midi_out_dropped(), 0);
}

#[test]
fn the_worker_sends_clock_start_pulses_and_stop_for_the_transport() {
    let (mut processor, controller) = Processor::new(48_000);
    let (doc, _, pattern) = midi_out_project(1);
    controller.set_project(doc.project(), &SamplePool::new());
    let fake = fake(&["synth"]);
    let settings = MidiHardwareSettings {
        output: Some("synth".into()),
        send_clock: true,
        ..Default::default()
    };
    let (mut worker, _sink) = worker(&controller, &fake, settings);
    let realtime = |sent: &[Vec<u8>]| -> Vec<u8> {
        sent.iter()
            .filter(|bytes| bytes[0] >= 0xf8)
            .map(|bytes| bytes[0])
            .collect()
    };
    let start = Instant::now();
    let mut out = [0.0_f32; 960];
    // Stopped: a turn of the worker sends no clock.
    processor.process(&mut out);
    worker.service_at(start);
    assert!(realtime(&fake.lock().unwrap().sent).is_empty());
    assert!(!worker.state().clock_running);

    play_pattern(&controller, pattern);
    // Half a second: 480 frames of audio for every 10 ms of the clock's time.
    for step in 1..=50_u32 {
        processor.process(&mut out);
        worker.service_at(start + Duration::from_millis(10) * step);
    }
    assert!(worker.state().clock_running);
    let sent = realtime(&fake.lock().unwrap().sent);
    assert_eq!(sent[0], 0xfa, "{sent:x?}");
    let pulses = sent.iter().filter(|byte| **byte == 0xf8).count();
    // One beat at 120 bpm is twenty-four pulses.
    assert!((23..=26).contains(&pulses), "{pulses} pulses");
    assert!(!sent.contains(&0xfc));

    controller.stop();
    processor.process(&mut out);
    worker.service_at(start + Duration::from_millis(520));
    let sent = realtime(&fake.lock().unwrap().sent);
    assert_eq!(sent.last(), Some(&0xfc));
    assert_eq!(sent.iter().filter(|byte| **byte == 0xfc).count(), 1);
    assert!(!worker.state().clock_running);

    // Switching the clock off while playing sends stop once.
    play_pattern(&controller, pattern);
    processor.process(&mut out);
    worker.service_at(start + Duration::from_millis(530));
    assert!(worker.state().clock_running);
    worker.configure(MidiHardwareSettings {
        output: Some("synth".into()),
        send_clock: false,
        ..Default::default()
    });
    assert!(!worker.state().clock_running);
}

/// Opt-in check against the MIDI ports this machine really has. It lists
/// every port, then sends one second of clock at 120 bpm to the first
/// output and reports how evenly the sends were spaced. It measures when
/// the send calls returned, not what arrived on a wire.
///
/// `cargo test -p windfall-engine real_ports -- --ignored --nocapture`
#[test]
#[ignore = "opens real MIDI ports; run with `--ignored --nocapture` where they exist"]
fn real_ports_are_listed_and_the_first_output_takes_a_second_of_clock() {
    let mut ports = native::Native;
    let (inputs, outputs) = ports.enumerate().expect("the MIDI backend lists its ports");
    for port in &inputs {
        println!("input: {} ({})", port.name, port.id);
    }
    for port in &outputs {
        println!("output: {} ({})", port.name, port.id);
    }
    let Some(port) = outputs.first() else {
        println!("no MIDI output on this machine: nothing was sent");
        return;
    };
    let mut output = ports.output(&port.id).expect("the first output opens");
    let start = Instant::now();
    let mut clock = ClockOut::new(start);
    let mut pulses: Vec<Duration> = Vec::new();
    while start.elapsed() < Duration::from_secs(1) {
        let now = Instant::now();
        let playhead = Playhead {
            playing: true,
            tick: now.duration_since(start).as_secs_f64() * 1_920.0,
            ticks_per_second: 1_920.0,
            mode: PlayMode::Song,
            pattern: PatternId(1),
            pattern_ticks: None,
            loop_song: false,
        };
        let mut due: Vec<Vec<u8>> = Vec::new();
        clock.poll(&playhead, now, &mut |bytes| due.push(bytes.to_vec()));
        for bytes in due {
            output.send(&bytes).expect("the output takes the message");
            if bytes == [0xf8] {
                pulses.push(start.elapsed());
            }
        }
        let wait = clock.next_due().map_or(SERVICE, |due| {
            due.saturating_duration_since(Instant::now()).min(SERVICE)
        });
        thread::sleep(wait);
    }
    output.send(&[0xfc]).expect("the output takes stop");
    let ideal = 1_000.0 / 48.0;
    let worst = pulses
        .windows(2)
        .map(|pair| ((pair[1] - pair[0]).as_secs_f64() * 1_000.0 - ideal).abs())
        .fold(0.0, f64::max);
    println!(
        "{} pulses to {}; worst gap is {worst:.3} ms off {ideal:.3} ms",
        pulses.len(),
        port.name
    );
    assert!(pulses.len() >= 40, "about 48 pulses in a second");
}

#[test]
fn injected_audio_clock_and_port_measure_compensated_notes_and_clock_jitter() {
    use crate::recording_clock::OutputClock;
    use std::sync::atomic::AtomicU64;
    struct Measured {
        now: Arc<AtomicU64>,
        sent: Arc<Mutex<Vec<(u64, Vec<u8>)>>>,
    }
    impl Output for Measured {
        fn latency_nanos(&self) -> u64 {
            3_000_000
        }
        fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.sent
                .lock()
                .unwrap()
                .push((self.now.load(Ordering::Relaxed), bytes.to_vec()));
            Ok(())
        }
    }
    let (_, controller) = Processor::new(48_000);
    let fake = fake(&["out"]);
    let (mut worker, _) = worker(
        &controller,
        &fake,
        MidiHardwareSettings {
            output: Some("out".into()),
            send_clock: true,
            ..Default::default()
        },
    );
    let now = Arc::new(AtomicU64::new(0));
    let sent = Arc::new(Mutex::new(Vec::new()));
    worker.output = Some(Box::new(Measured {
        now: now.clone(),
        sent: sent.clone(),
    }));
    let base = Instant::now();
    // Callback frame zero is heard at 10 ms. Its note must be submitted at
    // 7 ms so the surface's reported 3 ms delay lands it with the DAC.
    let clock = OutputClock {
        generation: 1,
        frame: 0,
        nanos: 10_000_000,
        sample_rate: 48_000,
        nanos_per_frame: 1e9 / 48_000.0,
        latency_nanos: 10_000_000,
        measured: true,
    };
    controller.shared().push_midi_out(0, [0x90, 60, 100], 3);
    controller.shared().push_midi_out(4800, [0x80, 60, 0], 3);
    let mut head = controller.playhead();
    head.playing = true;
    head.tick = 0.0;
    head.ticks_per_second = 1920.0;
    // Inject bounded irregular wake-ups, without sleeping or real ports.
    for step in 0..=550_u64 {
        let ns = step * 1_000_000 + [0, 125_000, 80_000][step as usize % 3];
        now.store(ns, Ordering::Relaxed);
        worker.outputs_at(base + Duration::from_nanos(ns), &head, ns, Some(clock));
    }
    let sent = sent.lock().unwrap();
    let on = sent
        .iter()
        .find(|(_, bytes)| bytes == &[0x90, 60, 100])
        .unwrap()
        .0;
    let off = sent
        .iter()
        .find(|(_, bytes)| bytes == &[0x80, 60, 0])
        .unwrap()
        .0;
    assert!((7_000_000..8_125_001).contains(&on));
    assert!((107_000_000..108_125_001).contains(&off));
    let pulses: Vec<_> = sent
        .iter()
        .filter(|(_, bytes)| bytes == &[0xf8])
        .map(|(ns, _)| *ns)
        .collect();
    assert!(pulses.len() >= 24);
    let errors: Vec<_> = pulses
        .iter()
        .enumerate()
        .map(|(index, ns)| {
            let reference = 7_000_000.0 + index as f64 * 1e9 / 48.0;
            *ns as f64 - reference
        })
        .collect();
    assert!(
        errors
            .iter()
            .all(|error| *error >= -20_000.0 && *error <= 1_125_000.0),
        "deadline errors: {errors:?}"
    );
    let mean = errors.iter().sum::<f64>() / errors.len() as f64;
    let rms = (errors
        .iter()
        .map(|error| (error - mean).powi(2))
        .sum::<f64>()
        / errors.len() as f64)
        .sqrt();
    assert!(rms < 600_000.0, "synthetic clock jitter RMS {rms} ns");
}

#[test]
fn declarative_surface_feedback_emits_led_motor_and_display_bytes_through_the_worker() {
    use windfall_project::controller_links::{FeedbackMessage, FeedbackSource, ProfileFeedback};
    let (_, controller) = Processor::new(48_000);
    let fake = fake(&["surface"]);
    let (mut worker, _) = worker(
        &controller,
        &fake,
        MidiHardwareSettings {
            output: Some("surface".into()),
            ..Default::default()
        },
    );
    let rules = [
        ProfileFeedback {
            source: FeedbackSource::Playing,
            message: FeedbackMessage::NoteLed {
                channel: 1,
                key: 94,
            },
        },
        ProfileFeedback {
            source: FeedbackSource::Slot {
                slot: windfall_project::SlotTarget::MasterVolume,
            },
            message: FeedbackMessage::PitchBend { channel: 9 },
        },
        ProfileFeedback {
            source: FeedbackSource::MixerName { index: 0 },
            message: FeedbackMessage::Display {
                prefix: vec![0xf0, 0, 0, 0x66, 0x14, 0x12, 0],
                suffix: vec![0xf7],
                width: 7,
            },
        },
    ];
    let messages = vec![
        rules[0].encode(1.0, ""),
        rules[1].encode(0.5, ""),
        rules[2].encode(0.0, "Bass"),
    ];
    assert!(rules.iter().all(|rule| rule.problem().is_none()));
    fake.lock().unwrap().sent.clear();
    worker.feedback(&messages);
    assert_eq!(
        fake.lock().unwrap().sent,
        vec![
            vec![0x90, 94, 127],
            vec![0xe8, 0, 64],
            vec![
                0xf0, 0, 0, 0x66, 0x14, 0x12, 0, b'B', b'a', b's', b's', b' ', b' ', b' ', 0xf7
            ],
        ]
    );
    worker.feedback(&[rules[0].encode(0.0, "")]);
    assert_eq!(fake.lock().unwrap().sent.last().unwrap(), &[0x90, 94, 0]);
    let invalid = ProfileFeedback {
        source: FeedbackSource::Playing,
        message: FeedbackMessage::Display {
            prefix: vec![0xf0, 0xff],
            suffix: vec![0xf7],
            width: 7,
        },
    };
    assert!(invalid.problem().is_some());
    assert!(invalid.encode(0.0, "invalid").is_empty());
}

#[test]
fn independent_output_slots_route_and_removal_releases_without_global_stop() {
    let (mut processor, controller) = Processor::new(48_000);
    let (mut doc, source, pattern) = midi_out_project(4);
    let mut params = InstrumentKind::MidiOut.default_params();
    if let InstrumentParams::MidiOut(out) = &mut params {
        out.channel = 4;
        out.output_port = 2;
    }
    doc.dispatch(
        Command::SetInstrumentParams {
            channel: source,
            params,
        },
        None,
    )
    .unwrap();
    let fake = fake(&["main", "aux1", "aux2"]);
    let (mut worker, _) = worker(
        &controller,
        &fake,
        MidiHardwareSettings {
            output: Some("main".into()),
            extra_outputs: vec!["aux1".into(), "aux2".into()],
            ..Default::default()
        },
    );
    controller.set_project(doc.project(), &SamplePool::new());
    let mut pcm = [0.0; 512];
    processor.process(&mut pcm);
    worker.service();
    fake.lock().unwrap().routed.clear();
    play_pattern(&controller, pattern);
    processor.process(&mut pcm);
    worker.service();
    assert!(
        fake.lock()
            .unwrap()
            .routed
            .contains(&("aux2".into(), vec![0x93, 60, 127]))
    );
    assert!(
        !fake
            .lock()
            .unwrap()
            .routed
            .iter()
            .any(|(port, bytes)| port != "aux2" && bytes[0] & 0xf0 == 0x90)
    );
    doc.dispatch(Command::RemoveChannel { id: source }, None)
        .unwrap();
    controller.set_project(doc.project(), &SamplePool::new());
    assert_eq!(
        crate::test_alloc::allocator_calls(|| processor.process(&mut pcm)),
        0
    );
    worker.service();
    assert!(controller.playhead().playing, "no Stop/Panic required");
    assert!(
        fake.lock()
            .unwrap()
            .routed
            .contains(&("aux2".into(), vec![0x83, 60, 0]))
    );
    fake.lock().unwrap().present.retain(|id| id != "aux2");
    worker.refresh();
    assert!(worker.extra_outputs[1].is_none());
    assert!(worker.state().error.is_some());
    assert!(worker.extra_outputs[0].is_some());
    doc.undo().unwrap();
    let saved = windfall_project::file::to_string(doc.project()).unwrap();
    assert_eq!(
        windfall_project::file::from_str(&saved).unwrap(),
        *doc.project()
    );
}

#[test]
fn midi_output_plugin_destination_receives_notes_program_cc_and_exact_release_pcm() {
    use crate::plugins::{HostedEffect, HostedInstrument, PluginFactory};
    use std::sync::atomic::AtomicU64;
    use windfall_project::{PluginBinding, PluginTarget};
    #[derive(Debug)]
    struct Probe {
        count: AtomicU32,
        messages: [AtomicU64; 64],
    }
    struct Unit {
        probe: Arc<Probe>,
        frame: u64,
        held: bool,
    }
    impl HostedEffect for Unit {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            left.fill(if self.held { 0.25 } else { 0.0 });
            right.copy_from_slice(left);
            self.frame += left.len() as u64;
        }
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            0
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl HostedInstrument for Unit {
        fn midi_message(&mut self, bytes: &[u8]) -> bool {
            let at = self.probe.count.fetch_add(1, Ordering::Relaxed) as usize;
            if at < 64 {
                self.probe.messages[at].store(
                    self.frame << 24
                        | u64::from(bytes[0])
                        | u64::from(*bytes.get(1).unwrap_or(&0)) << 8
                        | u64::from(*bytes.get(2).unwrap_or(&0)) << 16,
                    Ordering::Relaxed,
                );
            }
            if bytes[0] & 0xf0 == 0x90 {
                self.held = bytes[2] != 0;
            }
            if bytes[0] & 0xf0 == 0x80 || bytes[0] & 0xf0 == 0xb0 && bytes[1] == 123 {
                self.held = false;
            }
            true
        }
        fn note_on(&mut self, _: u8, _: f32) {}
        fn note_off(&mut self, _: u8) {}
        fn all_notes_off(&mut self) {
            self.held = false;
        }
        fn voices(&self) -> usize {
            usize::from(self.held)
        }
    }
    #[derive(Debug)]
    struct Factory(Arc<Probe>);
    impl PluginFactory for Factory {
        fn effect(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            Err("instrument only".into())
        }
        fn instrument(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            Ok(Box::new(Unit {
                probe: self.0.clone(),
                frame: 0,
                held: false,
            }))
        }
    }
    let (mut doc, source, pattern) = midi_out_project(3);
    let target = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: Some("Hosted destination".into()),
                sample: None,
                instrument: Some(InstrumentKind::TripleOsc),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let mut project = doc.project().clone();
    project.plugins.push(PluginBinding {
        target: PluginTarget::Instrument { channel: target },
        format: "clap".into(),
        path: "synthetic-midi".into(),
        id: "org.windfall.test.midi".into(),
        name: "Synthetic MIDI".into(),
        state: Vec::new(),
        parameters: Vec::new(),
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
    });
    doc = Document::new(project);
    let mut params = InstrumentKind::MidiOut.default_params();
    if let InstrumentParams::MidiOut(out) = &mut params {
        out.channel = 3;
        out.program = 12;
        out.controllers[0].number = 74;
        out.controllers[0].value = 99;
        out.pitch_bend = 1.0;
        out.plugin_destination = target.0 as i32;
    }
    doc.dispatch(
        Command::SetInstrumentParams {
            channel: source,
            params,
        },
        None,
    )
    .unwrap();
    doc.project().check().unwrap();
    let probe = Arc::new(Probe {
        count: AtomicU32::new(0),
        messages: std::array::from_fn(|_| AtomicU64::new(0)),
    });
    let mut pool = SamplePool::new();
    pool.set_plugin_factory(Arc::new(Factory(probe.clone())));
    let (mut processor, controller) = Processor::new(48_000);
    controller.set_project(doc.project(), &pool);
    let mut pcm = [0.0; 512];
    processor.process(&mut pcm);
    play_pattern(&controller, pattern);
    for _ in 0..30 {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut pcm)),
            0
        );
    }
    let events: Vec<_> = probe.messages[..probe.count.load(Ordering::Relaxed) as usize]
        .iter()
        .map(|event| event.load(Ordering::Relaxed))
        .collect();
    let status = |kind| events.iter().find(|event| **event as u8 == kind).unwrap();
    assert_eq!(*status(0xc2) as u32 & 0xffff, 0x0bc2);
    assert!(
        events
            .iter()
            .any(|event| *event as u32 & 0xffffff == 0x634ab2)
    );
    assert!(
        events
            .iter()
            .any(|event| *event as u32 & 0xffffff == 0x7f7fe2)
    );
    assert_eq!((*status(0x82) >> 24) - (*status(0x92) >> 24), 240 * 25);
    assert_eq!(controller.midi_out_dropped(), 0);
    let mut hardware = Vec::new();
    controller.drain_midi_out(|_, bytes| hardware.push(bytes.to_vec()));
    assert!(hardware.is_empty());
    let rendered = crate::render(
        doc.project(),
        &pool,
        &crate::RenderOptions::default(),
        &mut |_| true,
    );
    // Independent PCM boundary reference: held 240 ticks then exact silence.
    assert!(rendered.samples()[1000 * 2].abs() > 0.01);
    assert_eq!(rendered.samples()[7000 * 2], 0.0);
    doc.dispatch(Command::RemoveChannel { id: target }, None)
        .unwrap();
    doc.project().check().unwrap();
    let windfall_project::ChannelSource::Instrument {
        params: InstrumentParams::MidiOut(out),
        ..
    } = &doc.project().channel(source).unwrap().source
    else {
        panic!()
    };
    assert_eq!(
        out.plugin_destination, -1,
        "deleting destination never redirects to hardware"
    );
    doc.undo().unwrap();
    assert_eq!(doc.project().plugins.len(), 1);
}
