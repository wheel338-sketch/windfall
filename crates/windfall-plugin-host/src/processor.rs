//! The audio-thread half of a plugin.
//!
//! [`PluginInstance::activate`](crate::PluginInstance::activate) makes a
//! [`PluginProcessor`] on the main thread, with every buffer it will need.
//! It is `Send`: hand it to the audio thread, call it there, and hand it
//! back to be deactivated. Nothing in it allocates, locks or blocks while
//! it processes. The plugin's own code may, and the host cannot prevent
//! that. What it does about a plugin that misbehaves is described in
//! [`containment`](crate::containment).
//!
//! # The seam
//!
//! Everything a plugin format has to do for one block is behind
//! [`ProcessorBackend`]. The processor around it holds what is the same for
//! every format: the event queue, the hand-over to and from the main
//! thread, block splitting, scrubbing and the watchdog. A backend that
//! talks to a plugin in another process would fit the same trait. See the
//! crate documentation for how its data would travel.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use crate::containment::{HealthCells, NoDenormals, PluginHealth, scrub};
use crate::events::{HostEvent, PluginEvent, Transport};

/// Ordinary events admitted for one block. More are refused and counted in
/// [`PluginHealth::dropped_events`]. Instrument adapter releases have a separate
/// bounded reserve.
pub const EVENT_CAPACITY: usize = 1024;

/// The instrument adapter releases channel/key pairs at frame zero. Once
/// ordinary admission stops, no new note-ons can intervene, so at most one
/// release per channel/key pair and one global panic need extra space.
pub(crate) const IMMEDIATE_RELEASE_CAPACITY: usize = 16 * 128 + 2 * crate::events::MAX_NOTE_INSTANCES + 1;

/// A tail length that stands for "never ends".
pub(crate) const ENDLESS_TAIL: u32 = u32::MAX;

/// What a plugin says after a block about the blocks to come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Keep processing.
    Continue,
    /// Keep processing unless the output has gone quiet.
    ContinueIfNotQuiet,
    /// The plugin is ringing out. Process for as long as its tail.
    Tail,
    /// Nothing more will come out until there is new input or a new event.
    Sleep,
    /// The plugin failed and is no longer called.
    Failed,
}

/// The audio of one block, as the backend is to hand it to the plugin.
pub(crate) enum AudioIo<'a> {
    /// The plugin reads its input from the buffers it writes its output to.
    InPlace {
        left: &'a mut [f32],
        right: &'a mut [f32],
    },
    /// Input and output are different memory. A plugin with no audio input
    /// ignores the input. The input is the processor's own copy, so a
    /// plugin that scribbles on it harms nothing.
    Separate {
        in_left: &'a mut [f32],
        in_right: &'a mut [f32],
        out_left: &'a mut [f32],
        out_right: &'a mut [f32],
    },
}

impl AudioIo<'_> {
    pub fn frames(&self) -> usize {
        match self {
            Self::InPlace { left, .. } => left.len(),
            Self::Separate { out_left, .. } => out_left.len(),
        }
    }
}

/// What a backend reports after one block.
pub(crate) struct BlockResult {
    pub status: ProcessStatus,
    /// The plugin's tail in frames, when the backend has just read it.
    pub tail: Option<u32>,
    pub dropped_events: u32,
}

/// The plugin reported an error from its process call, or could not be
/// started.
pub(crate) struct ProcessFailed;

/// One plugin format's way of running a block. The processor calls these
/// on the audio thread only, never two at once.
pub(crate) trait ProcessorBackend: Send + 'static {
    fn supports_note_instances(&self) -> bool { false }
    fn note_expression_mask(&self, _channel: u8) -> u32 { 0 }
    fn set_sidechain_input(&mut self, _input: Option<u32>) {}
    /// Processes one block of at most the block size the plugin was
    /// activated with. `events` are sorted by time and all lie inside the
    /// block. Must not allocate, lock or block in the host's own code.
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed>;

    /// Optional auxiliary audio, copied into format-owned input buffers.
    fn process_sidechain(&mut self, audio: AudioIo<'_>, _key: Option<&[[f32; 2]]>, events: &[HostEvent], transport: &Transport, steady_time: u64, out: &mut dyn FnMut(PluginEvent)) -> Result<BlockResult, ProcessFailed> {
        self.process(audio, events, transport, steady_time, out)
    }

    /// Translation drops from the just-failed block must still reach health.
    /// Defaults to zero for backends with no separate translation buffers.
    fn failure_dropped_events(&self) -> u32 {
        0
    }

    /// Clears the plugin's memory of past audio and ends its notes.
    fn reset(&mut self);

    /// Tells the plugin that processing pauses. The next block starts it
    /// again.
    fn stop(&mut self);

    /// Gives the backend back to the instance that made it.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// What the main thread and the audio thread both see of an active plugin.
#[derive(Debug, Default)]
pub(crate) struct ProcessorShared {
    pub health: HealthCells,
    pub latency: AtomicU32,
    pub tail: AtomicU32,
}

/// How the plugin's ports are fed, decided at activation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AudioShape {
    /// The plugin has an audio input for the track's sound.
    pub has_input: bool,
    /// The plugin allows its input and output to be the same memory.
    pub in_place: bool,
}

/// The audio-thread half of an active plugin. See the [module](self).
pub struct PluginProcessor {
    backend: Box<dyn ProcessorBackend>,
    shared: Arc<ProcessorShared>,
    shape: AudioShape,
    sample_rate: f64,
    max_block: usize,
    /// Events for the next block, sorted by time.
    pending: Vec<HostEvent>,
    from_main: rtrb::Consumer<HostEvent>,
    to_main: rtrb::Producer<PluginEvent>,
    transport: Transport,
    /// The input of a block, copied here when the plugin must not read it
    /// from where it writes.
    scratch: [Box<[f32]>; 2],
    steady_time: u64,
    realtime: bool,
    status: ProcessStatus,
}

/// What [`PluginProcessor::new`] is built from.
pub(crate) struct ProcessorParts {
    pub backend: Box<dyn ProcessorBackend>,
    pub shared: Arc<ProcessorShared>,
    pub shape: AudioShape,
    pub sample_rate: f64,
    pub max_block: usize,
    pub from_main: rtrb::Consumer<HostEvent>,
    pub to_main: rtrb::Producer<PluginEvent>,
}

impl PluginProcessor {
    pub fn supports_note_instances(&self) -> bool { self.backend.supports_note_instances() }
    pub fn note_expression_mask(&self, channel: u8) -> u32 { self.backend.note_expression_mask(channel.min(15)) }
    /// All-or-nothing admission keeps a note-on and its initial controls on
    /// the same frame without exposing a partially initialized voice.
    pub fn push_events(&mut self, events: &[HostEvent]) -> bool {
        if events.iter().any(|event| !event.valid()) || self.pending.len().saturating_add(events.len()) > EVENT_CAPACITY {
            self.shared.health.dropped_events.fetch_add(events.len().min(u32::MAX as usize) as u32, Ordering::Relaxed);
            return false;
        }
        for event in events { let at = self.pending.partition_point(|queued| queued.time() <= event.time()); self.pending.insert(at, *event); }
        true
    }
    pub fn set_sidechain_input(&mut self, input: Option<u32>) { self.backend.set_sidechain_input(input); }
    pub(crate) fn backend_mut(&mut self) -> &mut dyn ProcessorBackend {
        self.backend.as_mut()
    }
    pub(crate) fn new(parts: ProcessorParts) -> Self {
        let max_block = parts.max_block.max(1);
        Self {
            backend: parts.backend,
            shared: parts.shared,
            shape: parts.shape,
            sample_rate: parts.sample_rate,
            max_block,
            pending: Vec::with_capacity(EVENT_CAPACITY + IMMEDIATE_RELEASE_CAPACITY),
            from_main: parts.from_main,
            to_main: parts.to_main,
            transport: Transport::default(),
            scratch: [
                vec![0.0; max_block].into_boxed_slice(),
                vec![0.0; max_block].into_boxed_slice(),
            ],
            steady_time: 0,
            realtime: true,
            status: ProcessStatus::Continue,
        }
    }

    /// Owner-thread retirement preserves edits that have not reached a block.
    /// Notes belong to the abandoned audio timeline and are deliberately omitted.
    pub(crate) fn into_backend(mut self) -> (Box<dyn ProcessorBackend>, Vec<HostEvent>) {
        for _ in 0..crate::instance::QUEUE_CAPACITY {
            let Ok(event) = self.from_main.pop() else {
                break;
            };
            self.pending.push(event);
        }
        self.pending.sort_by_key(HostEvent::time);
        self.pending
            .retain(|event| matches!(event, HostEvent::Param { .. }));
        (self.backend, self.pending)
    }

    pub(crate) fn shared(&self) -> &Arc<ProcessorShared> {
        &self.shared
    }

    /// The sample rate the plugin was activated at.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// The longest block the plugin is handed at once. Longer blocks passed
    /// to [`process`](Self::process) are split.
    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// True if the plugin takes the track's sound as input, as an effect
    /// does. False for an instrument.
    pub fn has_audio_input(&self) -> bool {
        self.shape.has_input
    }

    /// Queues an event for the next block, in time order. Events for the
    /// same frame keep the order they were queued in. Returns false, and
    /// counts a dropped event, if the queue is full or the event has invalid
    /// key/channel, non-finite values or an invalid parameter id.
    pub fn push_event(&mut self, event: HostEvent) -> bool {
        if !event.valid() || self.pending.len() >= EVENT_CAPACITY {
            self.shared
                .health
                .dropped_events
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let at = self
            .pending
            .partition_point(|queued| queued.time() <= event.time());
        self.pending.insert(at, event);
        true
    }

    /// Guaranteed admission for the adapter's immediate releases. The public
    /// timed-event API keeps its ordinary capacity and rejection semantics.
    /// Under pressure, a repeated release is redundant only if no matching
    /// note-on follows the previous release at the same frame.
    fn push_immediate_release(&mut self, event: HostEvent) {
        let at = self.pending.partition_point(|queued| queued.time() == 0);
        if self.pending.len() >= EVENT_CAPACITY {
            let released =
                self.pending[..at]
                    .iter()
                    .rev()
                    .find_map(|queued| match (event, *queued) {
                        (HostEvent::NoteOffInstance { id, .. }, HostEvent::NoteOnInstance { id: other, .. }) if id == other => Some(false),
                        (HostEvent::NoteOffInstance { id, .. }, HostEvent::NoteOffInstance { id: other, .. }) if id == other => Some(true),
                        (HostEvent::AllNotesOff { .. }, HostEvent::NoteOnInstance { .. }) => Some(false),
                        (
                            HostEvent::NoteOff { key, channel, .. },
                            HostEvent::NoteOn {
                                key: other,
                                channel: other_channel,
                                ..
                            },
                        ) if key == other && channel == other_channel => Some(false),
                        (
                            HostEvent::NoteOff { key, channel, .. },
                            HostEvent::NoteOff {
                                key: other,
                                channel: other_channel,
                                ..
                            },
                        ) if key == other && channel == other_channel => Some(true),
                        (HostEvent::AllNotesOff { .. }, HostEvent::NoteOn { .. }) => Some(false),
                        (_, HostEvent::AllNotesOff { .. }) => Some(true),
                        _ => None,
                    });
            if released == Some(true) {
                return;
            }
        }
        // Every additional event is a distinct channel/key release or a panic, and
        // ordinary push_event cannot add note-ons while this reserve is used.
        debug_assert!(self.pending.len() < EVENT_CAPACITY + IMMEDIATE_RELEASE_CAPACITY);
        self.pending.insert(at, event);
    }

    pub(crate) fn release_note(&mut self, key: u8) {
        self.release_note_on_channel(key, 0);
    }

    pub(crate) fn release_note_on_channel(&mut self, key: u8, channel: u8) {
        self.push_immediate_release(HostEvent::NoteOff {
            time: 0,
            key: key.min(127),
            channel: channel.min(15),
            velocity: 0.0,
        });
    }

    pub(crate) fn release_all_notes(&mut self) {
        self.push_immediate_release(HostEvent::AllNotesOff { time: 0 });
    }

    pub fn release_note_instance(&mut self, id: u32, key: u8, channel: u8, velocity: f32) -> bool {
        let event = HostEvent::NoteOffInstance { time: 0, id, key, channel, velocity };
        if !event.valid() || !self.supports_note_instances() { return false; }
        self.push_immediate_release(event);
        true
    }

    /// Starts a note on frame `time` of the next block. `velocity` runs
    /// from 0 to 1.
    pub fn note_on(&mut self, time: u32, key: u8, velocity: f32) -> bool {
        self.note_on_channel(time, key, 0, velocity)
    }

    pub fn note_on_channel(&mut self, time: u32, key: u8, channel: u8, velocity: f32) -> bool {
        self.push_event(HostEvent::NoteOn {
            time,
            key: key.min(127),
            channel,
            velocity: velocity.clamp(0.0, 1.0),
        })
    }

    /// Lets go of a note on frame `time` of the next block.
    pub fn note_off(&mut self, time: u32, key: u8) -> bool {
        self.push_event(HostEvent::NoteOff {
            time,
            key: key.min(127),
            channel: 0,
            velocity: 0.0,
        })
    }

    /// Stops every note at once on frame `time` of the next block.
    pub fn all_notes_off(&mut self, time: u32) -> bool {
        self.push_event(HostEvent::AllNotesOff { time })
    }

    /// Sets a parameter on frame `time` of the next block, by the plugin's
    /// id and in the plugin's units.
    pub fn set_param(&mut self, time: u32, id: u32, value: f64) -> bool {
        self.push_event(HostEvent::Param { time, id, value })
    }

    /// Says where the song is at the start of the next block. While the
    /// transport is playing the processor moves the position on by itself
    /// after every block, so this is only needed when something changes.
    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
    }

    /// The transport the next block will be given.
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Sets the tempo and leaves the rest of the transport alone.
    pub fn set_tempo(&mut self, bpm: f64) {
        if bpm.is_finite() && bpm > 0.0 {
            self.transport.tempo_bpm = bpm;
        }
    }

    /// Says whether blocks are being played as they are made. Only then is
    /// a slow block an overrun. An offline render turns this off.
    pub fn set_realtime(&mut self, realtime: bool) {
        self.realtime = realtime;
    }

    /// Samples by which the plugin's output lags its input.
    pub fn latency_samples(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }

    /// Samples for which the plugin can keep sounding after its input has
    /// gone silent, or `None` if it never ends by itself.
    pub fn tail_samples(&self) -> Option<u32> {
        match self.shared.tail.load(Ordering::Relaxed) {
            ENDLESS_TAIL => None,
            tail => Some(tail),
        }
    }

    /// How the plugin has behaved since it was activated.
    pub fn health(&self) -> PluginHealth {
        self.shared.health.read()
    }

    /// What the plugin said after the last block.
    pub fn status(&self) -> ProcessStatus {
        self.status
    }

    /// Clears the plugin's memory of past audio and ends its notes, and
    /// forgets the events that were queued.
    pub fn reset(&mut self) {
        self.pending.clear();
        if !self.shared.health.failed.load(Ordering::Relaxed) {
            self.backend.reset();
        }
    }

    /// Tells the plugin that processing pauses, as CLAP wants before a
    /// plugin is deactivated. Call it on the audio thread before sending
    /// the processor back. The next [`process`](Self::process) starts the
    /// plugin again.
    pub fn stop(&mut self) {
        self.backend.stop();
    }

    fn take_main_thread_events(&mut self) {
        // These parameter edits were already accepted by the main-thread
        // queue. Leave excess there until a block has room, including after
        // a zero-frame boundary or while immediate releases use the reserve.
        // This also bounds work if the producer keeps filling concurrently.
        for _ in 0..EVENT_CAPACITY.saturating_sub(self.pending.len()) {
            let Ok(event) = self.from_main.pop() else {
                break;
            };
            self.push_event(event);
        }
    }

    /// Processes one block in place: the plugin's input is what `left` and
    /// `right` hold, and its output replaces it. An instrument ignores what
    /// they hold. The two slices should be the same length. If they are
    /// not, the shorter one decides.
    ///
    /// The block can be any length. One longer than
    /// [`max_block`](Self::max_block) is handed to the plugin in pieces.
    /// Events queued with a time at or past the end of the block take
    /// effect on its last frame.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) -> ProcessStatus { self.process_sidechain(left, right, None) }

    pub fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) -> ProcessStatus {
        let frames = left.len().min(right.len());
        let (left, right) = (&mut left[..frames], &mut right[..frames]);
        self.take_main_thread_events();
        if frames == 0 {
            return self.status;
        }
        if self.shared.health.failed.load(Ordering::Relaxed) {
            self.pending.clear();
            if !self.shape.has_input {
                left.fill(0.0);
                right.fill(0.0);
            }
            return ProcessStatus::Failed;
        }

        let Self {
            backend,
            shared,
            shape,
            pending,
            to_main,
            scratch,
            ..
        } = self;
        let [scratch_left, scratch_right] = scratch;
        let mut out = |event: PluginEvent| {
            if to_main.push(event).is_err() {
                shared.health.dropped_events.fetch_add(1, Ordering::Relaxed);
            }
        };

        let mut done = 0;
        while done < frames {
            let length = (frames - done).min(self.max_block);
            let last = done + length == frames;
            let (chunk_left, chunk_right) = (
                &mut left[done..done + length],
                &mut right[done..done + length],
            );

            let due = if last {
                for event in pending.iter_mut() {
                    event.set_time(event.time().min(length as u32 - 1));
                }
                pending.len()
            } else {
                pending.partition_point(|event| (event.time() as usize) < length)
            };

            let separate = !shape.has_input || !shape.in_place;
            let audio = if separate {
                let (in_left, in_right) =
                    (&mut scratch_left[..length], &mut scratch_right[..length]);
                if shape.has_input {
                    in_left.copy_from_slice(chunk_left);
                    in_right.copy_from_slice(chunk_right);
                }
                AudioIo::Separate {
                    in_left,
                    in_right,
                    out_left: chunk_left,
                    out_right: chunk_right,
                }
            } else {
                AudioIo::InPlace {
                    left: chunk_left,
                    right: chunk_right,
                }
            };

            let started = Instant::now();
            let result = {
                let _flush = NoDenormals::enter();
                backend.process_sidechain(
                    audio,
                    // The part of the key that lies in this piece. A key that
                    // ends early is silent from there on, not from the start
                    // of the piece it ends in.
                    key.map(|key| &key[done.min(key.len())..(done + length).min(key.len())]),
                    &pending[..due],
                    &self.transport,
                    self.steady_time,
                    &mut out,
                )
            };
            let elapsed = started.elapsed();

            let (chunk_left, chunk_right) = (
                &mut left[done..done + length],
                &mut right[done..done + length],
            );
            let Ok(result) = result else {
                shared
                    .health
                    .dropped_events
                    .fetch_add(backend.failure_dropped_events(), Ordering::Relaxed);
                shared.health.failed.store(true, Ordering::Relaxed);
                pending.clear();
                // What the plugin left in its output cannot be trusted. An
                // effect's input is still at hand if it was copied aside.
                if shape.has_input && separate {
                    chunk_left.copy_from_slice(&scratch_left[..length]);
                    chunk_right.copy_from_slice(&scratch_right[..length]);
                } else {
                    chunk_left.fill(0.0);
                    chunk_right.fill(0.0);
                }
                if !shape.has_input {
                    left[done + length..].fill(0.0);
                    right[done + length..].fill(0.0);
                }
                self.status = ProcessStatus::Failed;
                return self.status;
            };

            let bad = scrub(chunk_left) + scrub(chunk_right);
            if bad > 0 {
                shared
                    .health
                    .scrubbed_samples
                    .fetch_add(u64::from(bad), Ordering::Relaxed);
            }
            if self.realtime {
                let allowed = length as f64 / self.sample_rate;
                let over = elapsed.as_secs_f64() - allowed;
                if over > 0.0 {
                    let micros = (over * 1.0e6).min(f64::from(u32::MAX)) as u32;
                    shared.health.overruns.fetch_add(1, Ordering::Relaxed);
                    shared
                        .health
                        .worst_overrun_micros
                        .fetch_max(micros, Ordering::Relaxed);
                }
            }
            if let Some(tail) = result.tail {
                shared.tail.store(tail, Ordering::Relaxed);
            }
            shared
                .health
                .dropped_events
                .fetch_add(result.dropped_events, Ordering::Relaxed);
            self.status = result.status;

            pending.drain(..due);
            for event in pending.iter_mut() {
                event.set_time(event.time() - length as u32);
            }
            self.transport.advance(length, self.sample_rate);
            self.steady_time += length as u64;
            done += length;
        }
        self.status
    }
}
