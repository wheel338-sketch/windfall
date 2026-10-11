//! Native MIDI control. One worker owns all port handles and output sends.
//! The input callback only parses fixed-size supported messages, reads atomics
//! and the monotonic clock, and pushes a Copy packet into a preallocated SPSC
//! queue. No project access.
//!
//! Besides auditioning notes, the worker hands every accepted message to its
//! [`Audition`] sink with the moment it was captured, sends MIDI clock for the
//! transport, and sends what MIDI output channels queue on the audio thread.
//! Nothing here runs on the audio thread: it only leaves fixed-size messages
//! in a preallocated queue, and this worker sends them.
pub mod clock;
mod message;
mod native;
#[cfg(test)]
mod routing_tests;
mod scheduling;
#[cfg(test)]
mod tests;

pub use message::MidiMessage;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use windfall_ipc::{MidiHardwareSettings, MidiHardwareState, MidiPort};

use crate::{Controller, Playhead};
use clock::ClockOut;

pub const INPUT_CAPACITY: usize = 1024;
/// Messages of MIDI output channels that can wait for their time to be sent.
pub const OUTPUT_PENDING: usize = 2048;
const POLL: Duration = Duration::from_millis(500);
const SERVICE: Duration = Duration::from_millis(2);
/// A MIDI output message due later than this is not believed: the audio
/// clock it was timed against has gone away. It is sent at once.

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    Note { channel: u8, key: u8, velocity: u8 },
    Sustain { channel: u8, down: bool },
    Panic { channel: u8 },
}

impl Event {
    /// What a message means for auditioning notes, if anything.
    fn of(message: MidiMessage) -> Option<Self> {
        match message {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity,
            } => Some(Self::Note {
                channel,
                key,
                velocity,
            }),
            MidiMessage::NoteOff { channel, key } => Some(Self::Note {
                channel,
                key,
                velocity: 0,
            }),
            MidiMessage::Controller {
                channel,
                number: 64,
                value,
            } => Some(Self::Sustain {
                channel,
                down: value >= 64,
            }),
            MidiMessage::Controller {
                channel,
                number: 121,
                ..
            } => Some(Self::Sustain {
                channel,
                down: false,
            }),
            MidiMessage::Controller {
                channel,
                number: 120 | 123,
                ..
            } => Some(Self::Panic { channel }),
            _ => None,
        }
    }

    #[cfg(test)]
    fn parse(bytes: &[u8]) -> Option<Self> {
        MidiMessage::parse(bytes).and_then(Self::of)
    }
}

#[derive(Clone, Copy)]
struct Packet {
    epoch: u64,
    device: u8,
    message: MidiMessage,
    at: Instant,
    stamp_micros: u64,
}

/// One accepted message from an input, as the sink sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputMessage {
    /// Which input it came from: the place of that input in
    /// [`MidiHardwareSettings::inputs`], the main input being 0.
    pub device: u8,
    pub message: MidiMessage,
    /// When the input callback took it, on the monotonic clock.
    pub at: Instant,
    /// The backend's own timestamp in microseconds, or 0 when it gave none.
    /// Only differences between two stamps of one input mean anything.
    pub stamp_micros: u64,
}

/// Moved into the backend callback and used by deterministic fake inputs too.
pub struct Ingress {
    tx: Producer<Packet>,
    controller: Controller,
    dropped: Arc<AtomicU32>,
    device: u8,
}

impl Ingress {
    pub fn receive(&mut self, bytes: &[u8]) {
        self.receive_at(0, bytes);
    }

    /// As [`receive`](Self::receive), with the backend's timestamp. Reads the
    /// monotonic clock, parses and pushes; never allocates, locks or waits.
    pub fn receive_at(&mut self, stamp_micros: u64, bytes: &[u8]) {
        let Some(message) = MidiMessage::parse(bytes) else {
            return;
        };
        let packet = Packet {
            epoch: self.controller.hardware_epoch(),
            device: self.device,
            message,
            at: Instant::now(),
            stamp_micros,
        };
        if self.tx.push(packet).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.controller.panic_hardware();
        }
    }
}

/// Invoked on the control worker, never on either hardware callback.
/// Verify the session generation/target and enqueue the engine note atomically
/// with respect to project changes. False invalidates the whole held-note set.
pub trait Audition: Send + Sync + 'static {
    fn note(&self, epoch: u64, key: u8, velocity: u8) -> bool;

    /// As [`note`](Self::note), with the moment the message that caused it
    /// was captured. A note let go by the sustain pedal carries the pedal's
    /// moment.
    fn note_at(&self, epoch: u64, key: u8, velocity: u8, at: Instant) -> bool {
        let _ = at;
        self.note(epoch, key, velocity)
    }

    /// Every accepted message, before it is auditioned. Answering true takes
    /// the message: a note, pedal or panic so taken is not auditioned, which
    /// is how a key linked to a control stays silent.
    fn message(&self, input: InputMessage) -> bool {
        let _ = input;
        false
    }

    /// Once every turn of the worker, about every two milliseconds, with
    /// the transport as it then reads.
    fn idle(&self, now: Instant, playhead: &Playhead) {
        let _ = (now, playhead);
    }
}

pub trait Output: Send {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Reported driver/surface output delay. Unknown drivers report zero.
    fn latency_nanos(&self) -> u64 {
        0
    }
}

/// Device API seam. Construction and all calls happen on the owning worker.
pub trait Ports: Send + 'static {
    fn enumerate(&mut self) -> Result<(Vec<MidiPort>, Vec<MidiPort>), String>;
    fn input(&mut self, id: &str, ingress: Ingress) -> Result<Box<dyn Send>, String>;
    fn output(&mut self, id: &str) -> Result<Box<dyn Output>, String>;
}

enum Request {
    Configure(MidiHardwareSettings, mpsc::SyncSender<MidiHardwareState>),
    Refresh(mpsc::SyncSender<MidiHardwareState>),
    Feedback(Vec<Vec<u8>>),
}

/// Thread-safe control handle; native handles never leave its worker.
pub struct Runtime {
    tx: Option<mpsc::SyncSender<Request>>,
    state: Arc<Mutex<MidiHardwareState>>,
    controller: Controller,
    thread: Option<JoinHandle<()>>,
}

impl Runtime {
    pub fn start(
        controller: Controller,
        audition: Arc<dyn Audition>,
        settings: MidiHardwareSettings,
    ) -> Result<Self, String> {
        Self::start_with(controller, audition, settings, || Box::new(native::Native))
    }

    pub fn start_with(
        controller: Controller,
        audition: Arc<dyn Audition>,
        settings: MidiHardwareSettings,
        factory: impl FnOnce() -> Box<dyn Ports> + Send + 'static,
    ) -> Result<Self, String> {
        settings.validate()?;
        let (tx, rx) = mpsc::sync_channel(16);
        let state = Arc::new(Mutex::new(MidiHardwareState::default()));
        let published = state.clone();
        let worker_controller = controller.clone();
        let thread = thread::Builder::new()
            .name("windfall-midi".into())
            .spawn(move || {
                let mut worker = Worker::new(factory(), worker_controller, audition);
                worker.configure(settings);
                let mut polled = Instant::now();
                'serve: loop {
                    // Sleeping, not waiting on the queue: the wait is cut
                    // short for the next clock pulse or timed message, and a
                    // sleep keeps to its length where a queue wait may not.
                    thread::sleep(worker.wait(Instant::now()));
                    loop {
                        match rx.try_recv() {
                            Ok(Request::Configure(settings, reply)) => {
                                worker.configure(settings);
                                let _ = reply.send(worker.state());
                            }
                            Ok(Request::Refresh(reply)) => {
                                worker.refresh();
                                let _ = reply.send(worker.state());
                            }
                            Ok(Request::Feedback(messages)) => worker.feedback(&messages),
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break 'serve,
                        }
                    }
                    if polled.elapsed() >= POLL {
                        worker.refresh();
                        polled = Instant::now();
                    }
                    worker.service();
                    *published.lock().unwrap_or_else(|e| e.into_inner()) = worker.state();
                }
                worker.shutdown();
                // All connections drop here, after output cleanup, on this worker.
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            tx: Some(tx),
            state,
            controller,
            thread: Some(thread),
        })
    }

    pub fn state(&self) -> MidiHardwareState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn configure(&self, settings: MidiHardwareSettings) -> Result<MidiHardwareState, String> {
        settings.validate()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.request(Request::Configure(settings, tx), rx)
    }

    /// Bounded control-side surface updates; every output call stays on the worker.
    pub fn feedback(&self, messages: Vec<Vec<u8>>) -> Result<(), String> {
        if messages.len() > 256
            || messages
                .iter()
                .any(|message| message.is_empty() || message.len() > 128)
        {
            return Err("Surface feedback exceeds the MIDI worker budget.".into());
        }
        self.tx
            .as_ref()
            .ok_or("MIDI worker stopped.")?
            .try_send(Request::Feedback(messages))
            .map_err(|_| "MIDI feedback queue is busy.".to_owned())
    }

    pub fn refresh(&self) -> Result<MidiHardwareState, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.request(Request::Refresh(tx), rx)
    }

    fn request(
        &self,
        request: Request,
        reply: mpsc::Receiver<MidiHardwareState>,
    ) -> Result<MidiHardwareState, String> {
        self.tx
            .as_ref()
            .ok_or("MIDI worker stopped.")?
            .try_send(request)
            .map_err(|_| "MIDI worker is busy or stopped.".to_string())?;
        reply.recv().map_err(|_| "MIDI worker stopped.".to_string())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.controller.panic_hardware();
        self.tx.take();
        // A session can be released by its MIDI worker's last weak upgrade.
        // Never join ourselves; the disconnected control queue ends that worker.
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

struct Input {
    id: String,
    /// Dropping it closes the port.
    _connection: Box<dyn Send>,
    rx: Consumer<Packet>,
}

/// A message of a MIDI output channel and when to send it, in the
/// nanoseconds of the engine's recording clock.
#[derive(Clone, Copy)]
struct Timed {
    due: u64,
    bytes: [u8; 3],
    len: u8,
}

struct Worker {
    ports: Box<dyn Ports>,
    controller: Controller,
    audition: Arc<dyn Audition>,
    state: MidiHardwareState,
    inputs: Vec<Input>,
    output: Option<Box<dyn Output>>,
    dropped: Arc<AtomicU32>,
    epoch: u64,
    /// One bit per input channel/key, and a separate sustain bit. Fixed storage.
    down: [[bool; 128]; 16],
    sounding: [[bool; 128]; 16],
    sustain: [bool; 16],
    output_keys: [bool; 128],
    clock: ClockOut,
    /// MIDI output channel messages in the order they were made.
    pending: VecDeque<Timed>,
    /// The latest time given to a pending message, so none overtakes another.
    last_due: u64,
    /// One bit per MIDI channel a MIDI output channel has sent on.
    used_channels: u16,
}

impl Worker {
    fn new(ports: Box<dyn Ports>, controller: Controller, audition: Arc<dyn Audition>) -> Self {
        let epoch = controller.hardware_epoch();
        Self {
            ports,
            controller,
            audition,
            state: MidiHardwareState::default(),
            inputs: Vec::new(),
            output: None,
            dropped: Arc::new(AtomicU32::new(0)),
            epoch,
            down: [[false; 128]; 16],
            sounding: [[false; 128]; 16],
            sustain: [false; 16],
            output_keys: [false; 128],
            clock: ClockOut::new(Instant::now()),
            pending: VecDeque::with_capacity(OUTPUT_PENDING),
            last_due: 0,
            used_channels: 0,
        }
    }

    fn state(&self) -> MidiHardwareState {
        let main = self.state.settings.input.as_deref();
        MidiHardwareState {
            input_connected: self
                .inputs
                .iter()
                .any(|input| Some(input.id.as_str()) == main),
            output_connected: self.output.is_some(),
            dropped_events: self.dropped.load(Ordering::Relaxed),
            connected_inputs: self.inputs.iter().map(|input| input.id.clone()).collect(),
            clock_running: self.clock.running(),
            output_dropped: self.controller.midi_out_dropped(),
            ..self.state.clone()
        }
    }

    fn configure(&mut self, settings: MidiHardwareSettings) {
        self.panic();
        self.stop_clock();
        self.inputs.clear();
        self.output.take();
        self.pending.clear();
        self.state.settings = settings;
        self.state.error = None;
        self.refresh();
        if let Some(id) = self.state.settings.output.clone() {
            match self.ports.output(&id) {
                Ok(output) => self.output = Some(output),
                Err(error) => self.state.error = Some(error),
            }
        }
        let wanted: Vec<String> = self
            .state
            .settings
            .inputs()
            .map(|(id, _)| id.to_owned())
            .collect();
        for (device, id) in wanted.into_iter().enumerate() {
            let (tx, rx) = RingBuffer::new(INPUT_CAPACITY);
            let ingress = Ingress {
                tx,
                controller: self.controller.clone(),
                dropped: self.dropped.clone(),
                device: device as u8,
            };
            match self.ports.input(&id, ingress) {
                Ok(connection) => self.inputs.push(Input {
                    id,
                    _connection: connection,
                    rx,
                }),
                Err(error) => self.state.error = Some(error),
            }
        }
    }

    fn refresh(&mut self) {
        match self.ports.enumerate() {
            Ok((inputs, outputs)) => {
                let present = |id: &str| inputs.iter().any(|port| port.id == id);
                let lost_input = self.inputs.iter().any(|input| !present(&input.id));
                let lost_output = self.output.is_some()
                    && !outputs
                        .iter()
                        .any(|p| Some(&p.id) == self.state.settings.output.as_ref());
                if lost_input || lost_output {
                    self.panic();
                    if lost_input {
                        self.inputs.retain(|input| present(&input.id));
                    }
                    if lost_output {
                        self.output.take();
                        self.pending.clear();
                    }
                    self.state.error = Some("MIDI device disconnected. Refresh and apply the device settings to reconnect.".into());
                }
                self.state.inputs = inputs;
                self.state.outputs = outputs;
            }
            Err(error) => {
                self.panic();
                self.inputs.clear();
                self.output.take();
                self.pending.clear();
                self.state.error = Some(error);
            }
        }
    }

    fn panic(&mut self) {
        self.controller.panic_hardware();
        self.reset();
    }

    /// The last thing the worker does: stop the clock and quiet the output.
    fn shutdown(&mut self) {
        self.stop_clock();
        self.panic();
    }

    fn stop_clock(&mut self) {
        let mut stopped = false;
        self.clock.stop(&mut |_| stopped = true);
        if stopped {
            self.send(&[0xfc]);
        }
    }

    fn reset(&mut self) {
        // Exact note-offs plus standard panic messages; no output call from audio.
        let channel = self.state.settings.output_channel.saturating_sub(1);
        for key in 0..128 {
            if self.output_keys[key] {
                self.send(&[0x80 | channel, key as u8, 0]);
            }
        }
        self.send(&[0xb0 | channel, 64, 0]);
        self.send(&[0xb0 | channel, 123, 0]);
        self.send(&[0xb0 | channel, 120, 0]);
        // MIDI output channels may have notes on other channels. Their own
        // note-offs are discarded with the queue, so each channel they used
        // is told to stop instead.
        let used = std::mem::take(&mut self.used_channels);
        self.pending.clear();
        for other in 0..16_u8 {
            if used & (1 << other) != 0 && other != channel {
                self.send(&[0xb0 | other, 123, 0]);
                self.send(&[0xb0 | other, 120, 0]);
            }
        }
        self.down = [[false; 128]; 16];
        self.sounding = [[false; 128]; 16];
        self.sustain = [false; 16];
        self.output_keys = [false; 128];
        self.epoch = self.controller.hardware_epoch();
    }

    fn feedback(&mut self, messages: &[Vec<u8>]) {
        for bytes in messages.iter().take(256) {
            if !bytes.is_empty() && bytes.len() <= 128 {
                self.send(bytes);
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if let Some(output) = &mut self.output
            && let Err(error) = output.send(bytes)
        {
            self.state.error = Some(error);
            self.output.take();
            self.pending.clear();
            self.controller.panic_hardware();
        }
    }

    /// How long the worker may sleep before something is due.
    fn wait(&self, now: Instant) -> Duration {
        let mut wait = SERVICE;
        if let Some(due) = self.clock.next_due() {
            wait = wait.min(due.saturating_duration_since(now));
        }
        if let Some(front) = self.pending.front() {
            let now_nanos = self.controller.recording_clock().now_nanos();
            wait = wait.min(Duration::from_nanos(front.due.saturating_sub(now_nanos)));
        }
        wait
    }

    fn service(&mut self) {
        self.service_at(Instant::now());
    }

    /// One turn of the worker, with the time passed in so tests can make
    /// the clock's time up.
    fn service_at(&mut self, now: Instant) {
        if self.epoch != self.controller.hardware_epoch() {
            self.reset();
        }
        // Bounded service budget; control requests and disconnect checks cannot starve.
        let mut budget = INPUT_CAPACITY;
        'inputs: while budget > 0 {
            let mut any = false;
            for slot in 0..self.inputs.len() {
                let Some(packet) = self
                    .inputs
                    .get_mut(slot)
                    .and_then(|input| input.rx.pop().ok())
                else {
                    continue;
                };
                any = true;
                budget -= 1;
                if self.epoch != self.controller.hardware_epoch() {
                    self.reset();
                }
                if packet.epoch == self.epoch {
                    self.event(packet);
                }
                if budget == 0 {
                    break 'inputs;
                }
            }
            if !any {
                break;
            }
        }
        let playhead = self.controller.playhead();
        self.audition.idle(now, &playhead);
        self.outputs(now, &playhead);
    }

    /// Sends what is due: MIDI output channel messages, then the clock.
    fn outputs(&mut self, now: Instant, playhead: &Playhead) {
        let recording_clock = self.controller.recording_clock();
        let now_nanos = recording_clock.now_nanos();
        self.outputs_at(now, playhead, now_nanos, recording_clock.output());
    }

    /// Injected monotonic/audio clocks drive the same scheduler used by real ports.
    fn outputs_at(
        &mut self,
        now: Instant,
        playhead: &Playhead,
        now_nanos: u64,
        output_clock: Option<crate::recording_clock::OutputClock>,
    ) {
        let latency = self
            .output
            .as_ref()
            .map_or(0, |output| output.latency_nanos());
        let connected = self.output.is_some();
        let pending = &mut self.pending;
        let last_due = &mut self.last_due;
        // Drained whether or not an output is open, so the queue never
        // fills and nothing stale is sent to a port opened later.
        self.controller.drain_midi_out(|frame, bytes| {
            if !connected || pending.len() >= OUTPUT_PENDING || bytes.is_empty() {
                return;
            }
            let believed = scheduling::deadline(output_clock, frame, now_nanos, latency);
            // A message never goes before one made earlier.
            let due = believed.max(*last_due);
            *last_due = due;
            let mut stored = [0; 3];
            stored[..bytes.len()].copy_from_slice(bytes);
            pending.push_back(Timed {
                due,
                bytes: stored,
                len: bytes.len() as u8,
            });
        });
        while let Some(front) = self.pending.front().copied() {
            if front.due > now_nanos {
                break;
            }
            self.pending.pop_front();
            if front.bytes[0] < 0xf0 {
                self.used_channels |= 1 << (front.bytes[0] & 15);
            }
            self.send(&front.bytes[..usize::from(front.len)]);
        }
        if self.pending.is_empty() {
            self.last_due = 0;
        }

        if !(self.state.settings.send_clock && self.output.is_some()) {
            self.stop_clock();
            return;
        }
        // The clock cannot borrow the worker to send, so what it wants sent
        // is kept in fixed storage for a moment: at most a song position, a
        // continue and a short run of pulses.
        let mut wanted = [([0_u8; 3], 0_usize); 8];
        let mut count = 0;
        // When DAC playback is still ahead, wait for the compensated start rather
        // than sending tick-zero pulses immediately at callback time.
        if !self.clock.running()
            && playhead.playing
            && let Some(clock) = output_clock
        {
            let elapsed =
                (playhead.tick.max(0.0) / playhead.ticks_per_second.max(1.0) * 1e9) as u64;
            let start = clock
                .nanos_at(self.controller.playhead_output_frame())
                .saturating_sub(elapsed)
                .saturating_sub(latency);
            if now_nanos < start {
                return;
            }
        }
        let aligned = scheduling::aligned_head(
            playhead,
            self.controller.playhead_output_frame(),
            output_clock,
            now_nanos,
            latency,
        );
        self.clock.poll(&aligned, now, &mut |bytes| {
            if count < wanted.len() && bytes.len() <= 3 {
                wanted[count].0[..bytes.len()].copy_from_slice(bytes);
                wanted[count].1 = bytes.len();
                count += 1;
            }
        });
        for (bytes, len) in &wanted[..count] {
            self.send(&bytes[..*len]);
        }
    }

    fn event(&mut self, packet: Packet) {
        let taken = self.audition.message(InputMessage {
            device: packet.device,
            message: packet.message,
            at: packet.at,
            stamp_micros: packet.stamp_micros,
        });
        let Some(event) = Event::of(packet.message) else {
            return;
        };
        if taken {
            return;
        }
        let channel = match event {
            Event::Note { channel, .. }
            | Event::Sustain { channel, .. }
            | Event::Panic { channel } => channel,
        };
        if self
            .state
            .settings
            .input_channel
            .is_some_and(|filter| filter != channel + 1)
        {
            return;
        }
        let c = usize::from(channel);
        match event {
            Event::Note { key, velocity, .. } => {
                let k = usize::from(key);
                self.down[c][k] = velocity > 0;
                if velocity > 0 {
                    self.sounding[c][k] = true;
                    self.deliver(key, velocity, packet.at);
                } else if !self.sustain[c] {
                    self.release(c, k, packet.at);
                }
            }
            Event::Sustain { down, .. } => {
                self.sustain[c] = down;
                if !down {
                    for key in 0..128 {
                        if !self.down[c][key] {
                            self.release(c, key, packet.at);
                        }
                    }
                }
            }
            Event::Panic { .. } => self.panic(),
        }
    }

    fn release(&mut self, channel: usize, key: usize, at: Instant) {
        if self.sounding[channel][key] {
            self.sounding[channel][key] = false;
            // All accepted MIDI channels feed one destination. Keep the key held
            // until every contributing channel releases it.
            if !self.sounding.iter().any(|keys| keys[key]) {
                self.deliver(key as u8, 0, at);
            }
        }
    }

    fn deliver(&mut self, key: u8, velocity: u8, at: Instant) {
        if !self.audition.note_at(self.epoch, key, velocity, at) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.panic();
            return;
        }
        if self.epoch != self.controller.hardware_epoch() {
            self.reset();
            return;
        }
        let status = if velocity > 0 { 0x90 } else { 0x80 };
        let channel = self.state.settings.output_channel - 1;
        // The engine holds one note per key. Balance output retriggers too,
        // including when several accepted input channels play the same key.
        if velocity > 0 && self.output_keys[usize::from(key)] {
            self.send(&[0x80 | channel, key, 0]);
        }
        self.send(&[status | channel, key, velocity]);
        self.output_keys[usize::from(key)] = velocity > 0 && self.output.is_some();
    }
}
