//! Preallocated callback half. Collection and scheduling add exactly two blocks.

use super::{
    protocol::*,
    slots::{InputBlock, NoteSnapshot, OutputBlock, Region, finite_input},
};
use crate::{HostEvent, Transport};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy)]
pub struct ParameterSpec {
    pub id: u32,
    pub min: f64,
    pub max: f64,
    pub value: f64,
    pub read_only: bool,
    pub stepped: bool,
}
impl ParameterSpec {
    pub fn valid(self) -> bool {
        self.id != u32::MAX
            && self.min.is_finite()
            && self.max.is_finite()
            && self.value.is_finite()
            && self.min <= self.value
            && self.value <= self.max
            && (!self.stepped || self.value == self.value.round())
    }
    fn clamp(self, value: f64) -> f64 {
        let value = value.clamp(self.min, self.max);
        if self.stepped { value.round() } else { value }
    }
}

#[derive(Default)]
pub struct Signals {
    pub failed: AtomicBool,
    pub retired: AtomicBool,
    pub submitted: AtomicU64,
    pub processed_generation: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Health {
    pub submitted_blocks: u64,
    pub completed_blocks: u64,
    pub missed_blocks: u64,
    pub corrupt_blocks: u64,
    pub full_blocks: u64,
    pub dropped_events: u64,
    pub native_drops: u64,
    pub unknown_blocks: u64,
    pub invalid_controls: u64,
    pub acknowledged_generation: u64,
}

pub struct Audio {
    sidechain_input: Option<u32>,
    region: Region,
    signals: Arc<Signals>,
    input: InputBlock,
    output: OutputBlock,
    parameters: Box<[ParameterSpec]>,
    held: [f32; NOTE_KEYS],
    instances: Box<[NoteSnapshot]>,
    note_instances: bool,
    expression_masks: [u32; 16],
    generation: u64,
    epoch: u64,
    sequence: u64,
    blocks_since_reset: u64,
    cursor: usize,
    transport: Transport,
    dry: Box<[[f32; 2]]>,
    dry_cursor: usize,
    available: bool,
    transition: bool,
    correction: [f32; 2],
    ramp_remaining: usize,
    last: [f32; 2],
    health: Health,
    completed_proof: Option<(u64, u64, u64)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineError {
    Cancelled,
    Deadline,
    Failed,
    Corrupt,
    Shape,
}
impl Audio {
    /// Allocate and validate on the control side, after native latency negotiation.
    pub fn new(
        region: Region,
        signals: Arc<Signals>,
        parameters: &[ParameterSpec],
    ) -> Result<Self, ProtocolError> {
        region.config.validate()?;
        if region.timeline_epoch()? != 1 {
            return Err(ProtocolError::Sequence);
        }
        if parameters.len() > PARAM_CAPACITY
            || parameters.iter().any(|parameter| !parameter.valid())
            || parameters.iter().enumerate().any(|(index, parameter)| {
                parameters[..index]
                    .iter()
                    .any(|other| other.id == parameter.id)
            })
        {
            return Err(ProtocolError::Parameter);
        }
        let latency = region.config.latency();
        let mut audio = Self {
            sidechain_input: None,
            region,
            signals,
            input: InputBlock::new(),
            output: OutputBlock::silent(),
            parameters: parameters.to_vec().into_boxed_slice(),
            held: [0.0; NOTE_KEYS],
            instances: vec![NoteSnapshot::EMPTY; crate::MAX_NOTE_INSTANCES].into_boxed_slice(),
            note_instances: false,
            expression_masks: [0; 16],
            generation: 1,
            epoch: 1,
            sequence: 0,
            blocks_since_reset: 0,
            cursor: 0,
            transport: Transport::default(),
            dry: vec![[0.0; 2]; latency].into_boxed_slice(),
            dry_cursor: 0,
            available: false,
            transition: false,
            correction: [0.0; 2],
            ramp_remaining: 0,
            last: [0.0; 2],
            health: Health::default(),
            completed_proof: None,
        };
        audio.begin_block();
        Ok(audio)
    }
    pub fn signals(&self) -> &Arc<Signals> {
        &self.signals
    }
    pub fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.sidechain_input = input.filter(|index| *index < 64);
        // Commit at the next complete block, preserving collection phase.
        if self.cursor == 0 {
            self.input.sidechain_input = self.sidechain_input;
        }
    }
    pub fn health(&self) -> Health {
        self.health
    }
    pub fn latency(&self) -> usize {
        self.region.config.latency()
    }
    pub fn desired_generation(&self) -> u64 {
        self.generation
    }
    pub fn parameters(&self) -> &[ParameterSpec] { &self.parameters }
    /// Plugin hard reset additionally removes transition ramps and old PCM.
    /// Ordinary seek retains the click-suppression transition via reset_timeline.
    pub fn reset_hard(&mut self) {
        self.all_notes_off();
        self.reset_timeline();
        self.output.left.fill(0.0);
        self.output.right.fill(0.0);
        self.last = [0.0; 2];
        self.correction = [0.0; 2];
        self.ramp_remaining = 0;
        self.transition = false;
    }
    /// Local collection intent, never evidence of native admission/processing.
    pub fn collection_frontier(&self) -> (u64, u64, u64) {
        (self.epoch, self.sequence, self.generation)
    }
    /// Only matching, timely COMPLETE output supplies this DSP proof.
    pub fn completed_proof(&self) -> Option<(u64, u64, u64)> {
        self.completed_proof
    }
    pub fn voices(&self) -> usize {
        self.held.iter().filter(|value| **value > 0.0).count()
            + self.instances.iter().filter(|note| note.id != 0).count()
    }
    fn advance_generation(&mut self) -> bool {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
            true
        } else {
            self.signals.failed.store(true, Ordering::Release);
            false
        }
    }
    fn begin_block(&mut self) {
        self.input.sidechain_input = self.sidechain_input;
        // A valid host position can advance past the explicit wire bound.
        // Exhaustion must never reach a fallible encoder on the callback.
        if encode_transport(self.transport).is_err() {
            self.signals.failed.store(true, Ordering::Release);
            self.transport = Transport::default();
        }
        self.input.epoch = self.epoch;
        self.input.transport = self.transport;
        self.input.notes = self.held;
        self.input.instance_count = 0;
        for note in self.instances.iter().filter(|note| note.id != 0) {
            self.input.instances[self.input.instance_count] = *note;
            self.input.instance_count += 1;
        }
        self.input.control_start = self.generation;
        self.input.control_end = self.generation;
        self.input.controls_complete = true;
        self.input.event_count = 0;
        self.input.parameter_count = self.parameters.len();
        for (destination, source) in self.input.parameters.iter_mut().zip(self.parameters.iter()) {
            *destination = Parameter {
                id: source.id,
                value: source.value,
            };
        }
    }
    fn admit(&mut self, mut event: HostEvent, release: bool) -> bool {
        event.set_time(self.cursor as u32);
        if encode_event(event, self.region.config.block).is_err() {
            return false;
        }
        let count = self.input.event_count;
        if release {
            // No admitted note-on can follow the start of reserve use. Walk
            // back only to the latest relevant note event; an earlier release
            // without a intervening note-on already covers this new release.
            for previous in self.input.events[..count].iter().rev() {
                match (*previous, event) {
                    (HostEvent::AllNotesOff { .. }, _) => return true,
                    (
                        HostEvent::NoteOn {
                            key: previous,
                            channel: pc,
                            ..
                        },
                        HostEvent::NoteOff { key, channel, .. },
                    ) if previous == key && pc == channel => {
                        break;
                    }
                    (
                        HostEvent::NoteOff {
                            key: previous,
                            channel: pc,
                            ..
                        },
                        HostEvent::NoteOff { key, channel, .. },
                    ) if previous == key && pc == channel => {
                        return true;
                    }
                    (
                        HostEvent::NoteOnInstance { id: previous, .. },
                        HostEvent::NoteOffInstance { id, .. },
                    ) if previous == id => break,
                    (
                        HostEvent::NoteOffInstance { id: previous, .. },
                        HostEvent::NoteOffInstance { id, .. },
                    ) if previous == id => return true,
                    (
                        HostEvent::NoteOn { .. } | HostEvent::NoteOnInstance { .. },
                        HostEvent::AllNotesOff { .. },
                    ) => break,
                    _ => {}
                }
            }
        }
        let limit = if release {
            EVENT_CAPACITY
        } else {
            ORDINARY_EVENTS
        };
        if count >= limit {
            self.input.controls_complete = false;
            self.health.dropped_events = self.health.dropped_events.saturating_add(1);
            return false;
        }
        self.input.events[count] = event;
        self.input.event_count += 1;
        true
    }
    pub fn set_param(&mut self, id: u32, value: f64) -> bool {
        let Some(index) = self
            .parameters
            .iter()
            .position(|parameter| parameter.id == id)
        else {
            self.health.invalid_controls = self.health.invalid_controls.saturating_add(1);
            return false;
        };
        let parameter = &mut self.parameters[index];
        if parameter.read_only || !value.is_finite() {
            self.health.invalid_controls = self.health.invalid_controls.saturating_add(1);
            return false;
        }
        let value = parameter.clamp(value);
        if parameter.value == value {
            return true;
        }
        parameter.value = value;
        if !self.advance_generation() {
            return false;
        }
        self.admit(HostEvent::Param { time: 0, id, value }, false)
    }
    /// Fixed event admission for a MIDI-output channel, including ownership-safe
    /// channel/key notes and raw wheel/CC/program messages.
    pub fn midi_message(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > 3 || bytes[1..].iter().any(|byte| *byte >= 128) {
            return false;
        }
        let channel = bytes[0] & 15;
        let kind = bytes[0] & 0xf0;
        let expected = if matches!(kind, 0xc0 | 0xd0) { 2 } else { 3 };
        if bytes.len() != expected || !(0x80..=0xe0).contains(&kind) {
            return false;
        }
        let key = bytes[1];
        let release = kind == 0x80 || (kind == 0x90 && bytes[2] == 0);
        if kind == 0xb0 && matches!(key, 120 | 123) {
            self.held[usize::from(channel) * 128..(usize::from(channel) + 1) * 128].fill(0.0);
        }
        let event = if kind == 0x90 && !release {
            self.held[usize::from(channel) * 128 + key as usize] = f32::from(bytes[2]) / 127.0;
            HostEvent::NoteOn {
                time: 0,
                key,
                channel,
                velocity: f32::from(bytes[2]) / 127.0,
            }
        } else if release {
            self.held[usize::from(channel) * 128 + key as usize] = 0.0;
            HostEvent::NoteOff {
                time: 0,
                key,
                channel,
                velocity: 0.0,
            }
        } else {
            let mut stored = [0; 3];
            stored[..bytes.len()].copy_from_slice(bytes);
            HostEvent::Midi {
                time: 0,
                bytes: stored,
                len: bytes.len() as u8,
            }
        };
        self.advance_generation() && self.admit(event, release)
    }

    pub fn configure_note_capabilities(
        &mut self,
        instances: bool,
        masks: [u32; 16],
    ) -> Result<(), ProtocolError> {
        if masks.iter().any(|mask| *mask & !0x7f != 0)
            || (!instances && masks.iter().any(|mask| *mask != 0))
        {
            return Err(ProtocolError::Configuration);
        }
        self.note_instances = instances;
        self.expression_masks = masks;
        Ok(())
    }
    pub fn supports_note_instances(&self) -> bool {
        self.note_instances
    }
    pub fn supports_note_pitch(&self) -> bool {
        self.expression_masks
            .iter()
            .any(|mask| mask & crate::NoteExpressionKind::Tuning.mask() != 0)
    }
    pub fn note_on_instance(&mut self, note: NoteSnapshot) -> bool {
        if !self.note_instances || !note.valid() || self.instances.iter().any(|n| n.id == note.id) {
            return false;
        }
        let Some(slot) = self.instances.iter().position(|n| n.id == 0) else {
            self.health.dropped_events = self.health.dropped_events.saturating_add(1);
            return false;
        };
        let mut events = [HostEvent::AllNotesOff { time: 0 }; 6];
        let count = note.events(&self.expression_masks, &mut events);
        self.instances[slot] = note;
        if !self.advance_generation() {
            return false;
        }
        if self.input.event_count + count > ORDINARY_EVENTS {
            self.input.controls_complete = false;
            self.health.dropped_events = self.health.dropped_events.saturating_add(count as u64);
            return false;
        }
        for event in &events[..count] {
            self.admit(*event, false);
        }
        true
    }
    pub fn set_note_controls(&mut self, id: u32, controls: crate::PluginNoteControls) -> bool {
        let Some(slot) = self.instances.iter().position(|n| n.id == id && id != 0) else {
            return false;
        };
        if !controls.valid(self.instances[slot].key) {
            return false;
        }
        if self.instances[slot].controls == controls {
            return true;
        }
        self.instances[slot].controls = controls;
        if !self.advance_generation() {
            return false;
        }
        let mut events = [HostEvent::AllNotesOff { time: 0 }; 5];
        let count = self.instances[slot].expression_events(&self.expression_masks, &mut events);
        if self.input.event_count + count > ORDINARY_EVENTS {
            self.input.controls_complete = false;
            self.health.dropped_events = self.health.dropped_events.saturating_add(count as u64);
            return false;
        }
        for event in &events[..count] {
            self.admit(*event, false);
        }
        true
    }
    pub fn note_off_instance(&mut self, id: u32, velocity: f32) -> bool {
        if !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return false;
        }
        let Some(slot) = self.instances.iter().position(|n| n.id == id && id != 0) else {
            return true;
        };
        let note = self.instances[slot];
        self.instances[slot] = NoteSnapshot::EMPTY;
        if !self.advance_generation() {
            return false;
        }
        self.admit(
            HostEvent::NoteOffInstance {
                time: 0,
                id,
                key: note.key,
                channel: note.channel,
                velocity,
            },
            true,
        )
    }
    pub fn note_on(&mut self, key: u8, velocity: f32) -> bool {
        self.note_on_channel(key, 0, velocity)
    }
    pub fn note_on_channel(&mut self, key: u8, channel: u8, velocity: f32) -> bool {
        if channel > 15 || key > 127 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return false;
        }
        if velocity == 0.0 {
            return self.note_off_channel(key, channel);
        }
        self.held[channel as usize * 128 + key as usize] = velocity;
        if !self.advance_generation() {
            return false;
        }
        self.admit(
            HostEvent::NoteOn {
                time: 0,
                key,
                channel,
                velocity,
            },
            false,
        )
    }
    pub fn note_off(&mut self, key: u8) -> bool {
        self.note_off_channel(key, 0)
    }
    pub fn note_off_channel(&mut self, key: u8, channel: u8) -> bool {
        if channel > 15 || key > 127 {
            return false;
        }
        self.held[channel as usize * 128 + key as usize] = 0.0;
        if !self.advance_generation() {
            return false;
        }
        self.admit(
            HostEvent::NoteOff {
                time: 0,
                key,
                channel,
                velocity: 0.0,
            },
            true,
        )
    }
    pub fn all_notes_off(&mut self) {
        self.held.fill(0.0);
        self.instances.fill(NoteSnapshot::EMPTY);
        if self.advance_generation() {
            self.admit(HostEvent::AllNotesOff { time: 0 }, true);
        }
    }
    /// Explicit discontinuity: invalidate old audio without reusing a helper
    /// slot. Current held ownership is retained; the caller supplies releases.
    pub fn reset_timeline(&mut self) {
        let previous = self.epoch;
        let next = previous
            .checked_add(1)
            .filter(|epoch| *epoch <= u64::from(u32::MAX));
        let sequence = self.sequence.checked_add(1);
        // Invalidate proof even if reset cannot be published. No failure may
        // retain an apparently current acknowledgement.
        self.health.acknowledged_generation = 0;
        self.completed_proof = None;
        self.signals
            .processed_generation
            .store(0, Ordering::Release);
        self.available = false;
        self.transition = true;
        let (Some(epoch), Some(sequence)) = (next, sequence) else {
            self.signals.failed.store(true, Ordering::Release);
            self.region.latch_helper_failure();
            return;
        };
        if self.region.timeline_epoch() != Ok(previous) {
            self.signals.failed.store(true, Ordering::Release);
            self.region.latch_helper_failure();
            return;
        }
        self.epoch = epoch;
        // Partially collected sequence must never later be published under the
        // same number; gaps instruct the helper to reconstruct native controls.
        self.sequence = sequence;
        self.cursor = 0;
        self.blocks_since_reset = 0;
        self.dry.fill([0.0; 2]);
        self.dry_cursor = 0;
        self.region.discard_before(self.sequence);
        self.begin_block();
        // Reset linearizes here. All local old proof is gone; new READY cannot
        // be submitted until this single strong CAS succeeds.
        if self.region.publish_timeline_epoch(previous, epoch).is_err() {
            self.signals.failed.store(true, Ordering::Release);
            self.region.latch_helper_failure();
        }
    }
    fn fail_timeline(&mut self) {
        self.health.acknowledged_generation = 0;
        self.completed_proof = None;
        self.signals
            .processed_generation
            .store(0, Ordering::Release);
        self.transition |= self.available;
        self.available = false;
        self.signals.failed.store(true, Ordering::Release);
        self.region.latch_helper_failure();
    }
    pub fn set_transport(&mut self, transport: Transport) -> bool {
        if encode_transport(transport).is_err() {
            self.health.invalid_controls = self.health.invalid_controls.saturating_add(1);
            return false;
        }
        let tolerance = 2.0 / f64::from(self.region.config.sample_rate);
        let discontinuity =
            (transport.position_seconds - self.transport.position_seconds).abs() > tolerance;
        self.transport = transport;
        if discontinuity {
            self.reset_timeline();
        }
        if self.cursor == 0 {
            self.input.transport = transport;
        }
        true
    }
    pub fn set_tempo(&mut self, bpm: f64) -> bool {
        let transport = Transport {
            tempo_bpm: bpm,
            ..self.transport
        };
        if encode_transport(transport).is_err() {
            return false;
        }
        self.transport = transport;
        if self.cursor == 0 {
            self.input.transport = transport;
        }
        true
    }
    fn choose_output(&mut self) {
        if self.blocks_since_reset < 2 {
            return;
        }
        let expected = self.sequence - 2;
        let available = if self.signals.failed.load(Ordering::Acquire)
            || self.region.helper_failed()
        {
            false
        } else {
            match self.region.receive(expected, self.epoch, &mut self.output) {
                Ok(true) => {
                    self.health.completed_blocks = self.health.completed_blocks.saturating_add(1);
                    self.health.native_drops = self
                        .health
                        .native_drops
                        .saturating_add(u64::from(self.output.native_drops));
                    if self.output.processed_generation == 0 {
                        self.health.unknown_blocks = self.health.unknown_blocks.saturating_add(1);
                    } else {
                        self.completed_proof =
                            Some((self.epoch, expected, self.output.processed_generation));
                    }
                    self.health.acknowledged_generation = self
                        .health
                        .acknowledged_generation
                        .max(self.output.processed_generation);
                    self.signals
                        .processed_generation
                        .store(self.health.acknowledged_generation, Ordering::Release);
                    true
                }
                Ok(false) => false,
                Err(_) => {
                    self.health.corrupt_blocks = self.health.corrupt_blocks.saturating_add(1);
                    false
                }
            }
        };
        if !available {
            self.health.missed_blocks = self.health.missed_blocks.saturating_add(1);
        }
        if available != self.available && self.blocks_since_reset > 2 {
            self.transition = true;
        }
        self.available = available;
        // An expired busy slot remains helper-owned. Late completion will be
        // discarded on a future boundary without native-generation ack.
        self.region.discard_before(expected + 1);
    }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        if left.len() != right.len() {
            self.signals.failed.store(true, Ordering::Release);
            return;
        }
        if self.region.timeline_epoch() != Ok(self.epoch) {
            self.fail_timeline();
        }
        for (index, (left, right)) in left.iter_mut().zip(right).enumerate() {
            if self.cursor == 0 {
                self.choose_output();
            }
            let input = [finite_input(*left), finite_input(*right)];
            self.input.left[self.cursor] = input[0];
            self.input.right[self.cursor] = input[1];
            self.input.key[self.cursor] = key
                .and_then(|key| key.get(index))
                .copied()
                .unwrap_or([0.0; 2])
                .map(finite_input);
            let dry = self.dry[self.dry_cursor];
            self.dry[self.dry_cursor] = input;
            self.dry_cursor += 1;
            if self.dry_cursor == self.dry.len() {
                self.dry_cursor = 0;
            }
            let target = if self.available {
                [
                    self.output.left[self.cursor],
                    self.output.right[self.cursor],
                ]
            } else if self.region.config.kind == Kind::Effect {
                dry
            } else {
                [0.0; 2]
            };
            if self.transition {
                self.correction = [self.last[0] - target[0], self.last[1] - target[1]];
                self.ramp_remaining = 64;
                self.transition = false;
            }
            let fraction = self.ramp_remaining as f32 / 64.0;
            self.last = [
                finite_input(target[0] + self.correction[0] * fraction),
                finite_input(target[1] + self.correction[1] * fraction),
            ];
            self.ramp_remaining = self.ramp_remaining.saturating_sub(1);
            *left = self.last[0];
            *right = self.last[1];
            self.transport
                .advance(1, f64::from(self.region.config.sample_rate));
            self.cursor += 1;
            if self.cursor == self.region.config.block {
                self.input.control_end = self.generation;
                let next_sequence = self.sequence.checked_add(1);
                if next_sequence.is_none() {
                    self.signals.failed.store(true, Ordering::Release);
                }
                if !self.signals.failed.load(Ordering::Acquire) {
                    if self.region.submit(self.sequence, &self.input) {
                        self.health.submitted_blocks =
                            self.health.submitted_blocks.saturating_add(1);
                        self.signals
                            .submitted
                            .store(next_sequence.unwrap_or(u64::MAX), Ordering::Release);
                    } else {
                        self.health.full_blocks = self.health.full_blocks.saturating_add(1);
                    }
                }
                if let Some(sequence) = next_sequence {
                    self.sequence = sequence;
                } else {
                    self.signals.failed.store(true, Ordering::Release);
                }
                self.blocks_since_reset = self.blocks_since_reset.saturating_add(1);
                self.cursor = 0;
                self.begin_block();
            }
        }
    }
    /// Separate render-only scheduling API. Call only off realtime; the live
    /// facade must always use `process`, which contains no clock or waits.
    pub fn process_offline(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
    ) -> Result<(), OfflineError> {
        self.process_offline_with_scheduler(
            left,
            right,
            deadline,
            cancelled,
            std::time::Instant::now,
            || std::thread::sleep(std::time::Duration::from_micros(200)),
        )
    }
    // Private off-realtime scheduler seam for deterministic DONE/cancel races.
    fn process_offline_with_scheduler(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
        now: impl FnMut() -> std::time::Instant,
        wait: impl FnMut(),
    ) -> Result<(), OfflineError> {
        self.process_offline_key_with_scheduler(left, right, None, deadline, cancelled, now, wait)
    }
    pub fn process_offline_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
    ) -> Result<(), OfflineError> {
        self.process_offline_key_with_scheduler(
            left,
            right,
            key,
            deadline,
            cancelled,
            std::time::Instant::now,
            || std::thread::sleep(std::time::Duration::from_micros(200)),
        )
    }
    // Keep stereo/optional-key buffers and independent deadline, cancellation,
    // clock and wait seams explicit so deterministic race tests exercise the
    // same off-realtime scheduler as production, without changing public APIs.
    #[allow(clippy::too_many_arguments)]
    fn process_offline_key_with_scheduler(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
        mut now: impl FnMut() -> std::time::Instant,
        mut wait: impl FnMut(),
    ) -> Result<(), OfflineError> {
        let result =
            self.process_offline_inner(left, right, key, deadline, cancelled, &mut now, &mut wait);
        if result.is_err() {
            // A staging caller receives no retained partial/fallback output.
            // It must still discard its complete render artifact on this error.
            left.fill(0.0);
            right.fill(0.0);
        }
        result
    }
    // Mirrors the scheduler boundary above; each buffer/authority is borrowed
    // independently and none is retained or allocated while processing.
    #[allow(clippy::too_many_arguments)]
    fn process_offline_inner(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
        now: &mut dyn FnMut() -> std::time::Instant,
        wait: &mut dyn FnMut(),
    ) -> Result<(), OfflineError> {
        if left.len() != right.len() {
            return Err(OfflineError::Shape);
        }
        let mut offset = 0;
        while offset < left.len() {
            self.offline_status(deadline, cancelled, now())?;
            if self.cursor == 0 && self.blocks_since_reset >= 2 {
                while !self.region.output_finished(self.sequence - 2) {
                    self.offline_status(deadline, cancelled, now())?;
                    wait();
                }
            }
            // DONE can arrive with cancellation/expiry during the wait.
            self.offline_status(deadline, cancelled, now())?;
            let frames = (self.region.config.block - self.cursor).min(left.len() - offset);
            let corrupt = self.health.corrupt_blocks;
            let full = self.health.full_blocks;
            let unknown = self.health.unknown_blocks;
            self.process_sidechain(
                &mut left[offset..offset + frames],
                &mut right[offset..offset + frames],
                // A key shorter than the audio is silent past its own end only.
                key.map(|key| &key[offset.min(key.len())..(offset + frames).min(key.len())]),
            );
            if self.health.corrupt_blocks != corrupt {
                return Err(OfflineError::Corrupt);
            }
            if self.health.full_blocks != full {
                return Err(OfflineError::Failed);
            }
            if self.health.unknown_blocks != unknown
                || self.signals.failed.load(Ordering::Acquire)
                || self.region.helper_failed()
            {
                return Err(OfflineError::Failed);
            }
            offset += frames;
        }
        self.offline_status(deadline, cancelled, now())?;
        Ok(())
    }
    fn offline_status(
        &self,
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
        now: std::time::Instant,
    ) -> Result<(), OfflineError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(OfflineError::Cancelled);
        }
        if self.signals.failed.load(Ordering::Acquire) || self.region.helper_failed() {
            return Err(OfflineError::Failed);
        }
        if now >= deadline {
            return Err(OfflineError::Deadline);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::slots::LocalWords;
    use super::*;
    fn make(kind: Kind, latency: usize) -> (Audio, Region) {
        let region = Region::initialize(
            LocalWords::new(),
            Config {
                identity: Identity {
                    session: 1,
                    token: 2,
                    revision: 3,
                    binding: 4,
                },
                sample_rate: 48_000,
                block: 64,
                native_latency: latency,
                kind,
            },
        )
        .unwrap();
        (
            Audio::new(
                region.clone(),
                Arc::new(Signals::default()),
                &[ParameterSpec {
                    id: 7,
                    min: 0.0,
                    max: 2.0,
                    value: 1.0,
                    read_only: false,
                    stepped: false,
                }],
            )
            .unwrap(),
            region,
        )
    }
    #[test]
    fn hard_reset_clears_held_notes_pcm_ramps_and_old_completion_proof() {
        for kind in [Kind::Instrument, Kind::Effect] {
            let (mut audio, region) = make(kind, 17);
            audio.note_on(60, 1.0);
            audio.last = [0.75, -0.25]; audio.correction = audio.last;
            audio.ramp_remaining = 64; audio.available = true;
            audio.output.left.fill(0.75); audio.output.right.fill(-0.25);
            audio.dry.fill([0.5, -0.5]); audio.completed_proof = Some((1, 0, 2));
            audio.reset_hard();
            assert_eq!(audio.voices(), 0); assert_eq!(audio.completed_proof(), None);
            assert_eq!(region.timeline_epoch(), Ok(2));
            let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
            audio.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|v| *v == 0.0));
            assert_eq!(audio.collection_frontier().0, 2);
        }
    }
    #[test]
    fn instance_snapshot_retains_overlap_expression_and_all_channels_after_reset() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        audio.configure_note_capabilities(true, [0x7f; 16]).unwrap();
        let a = NoteSnapshot {
            id: crate::FIRST_NOTE_INSTANCE_ID,
            key: 60,
            channel: 3,
            velocity: 0.75,
            controls: crate::PluginNoteControls {
                volume: 1.0,
                pan: -0.75,
                pitch: 60.5,
                modulation_x: 0.2,
                modulation_y: 0.7,
            },
        };
        let b = NoteSnapshot {
            id: a.id + 1,
            channel: 15,
            ..a
        };
        assert!(audio.note_on_instance(a));
        assert!(audio.note_on_instance(b));
        assert!(audio.note_on_channel(72, 14, 0.25));
        audio.reset_timeline();
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        let mut input = InputBlock::new();
        region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(&input.instances[..input.instance_count], &[a, b]);
        assert_eq!(input.notes[14 * 128 + 72], 0.25);
        assert_eq!(input.event_count, 0);
        assert_eq!(audio.completed_proof(), None);
        assert!(audio.note_off_instance(a.id, 0.4));
        audio.reset_timeline();
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(&input.instances[..input.instance_count], &[b]);
        assert_eq!(audio.voices(), 2);
    }
    #[test]
    fn saturated_instance_expression_recovers_desired_controls_and_guarantees_release() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        audio.configure_note_capabilities(true, [0x7f; 16]).unwrap();
        let note = NoteSnapshot {
            id: crate::FIRST_NOTE_INSTANCE_ID,
            key: 60,
            channel: 0,
            velocity: 0.5,
            controls: crate::PluginNoteControls {
                volume: 1.0,
                pan: 0.0,
                pitch: 60.0,
                modulation_x: 0.5,
                modulation_y: 0.5,
            },
        };
        assert!(audio.note_on_instance(note));
        for index in 0..ORDINARY_EVENTS {
            audio.set_param(7, if index % 2 == 0 { 0.5 } else { 0.75 });
        }
        let changed = crate::PluginNoteControls {
            pitch: 72.0,
            ..note.controls
        };
        assert!(!audio.set_note_controls(note.id, changed));
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        let mut input = InputBlock::new();
        region.take_input(&mut input).unwrap().unwrap();
        assert!(!input.controls_complete);
        assert!(audio.note_off_instance(note.id, 0.25));
        audio.reset_timeline();
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(input.instance_count, 0);
        assert_eq!(audio.voices(), 0);
        assert!(audio.health().dropped_events > 0);
    }
    fn complete(region: &Region, input: &mut InputBlock, output: &mut OutputBlock) {
        while let Some((slot, sequence)) = region.take_input(input).unwrap() {
            output.left.copy_from_slice(&input.left);
            output.right.copy_from_slice(&input.right);
            output.epoch = input.epoch;
            output.processed_generation = if input.controls_complete {
                input.control_end
            } else {
                0
            };
            region.complete(slot, sequence, output, OUTPUT_OK);
        }
    }
    #[test]
    fn offline_done_cancellation_race_fails_and_clears_both_buffers() {
        offline_done_race(true);
    }
    #[test]
    fn routed_midi_snapshots_preserve_wire_channels_and_control_bytes() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        assert!(audio.midi_message(&[0x92, 60, 100]));
        assert!(audio.midi_message(&[0x94, 60, 90]));
        assert!(audio.midi_message(&[0xb4, 74, 99]));
        assert!(audio.midi_message(&[0xc4, 11]));
        assert!(audio.midi_message(&[0xe4, 127, 127]));
        assert!(audio.midi_message(&[0x82, 60, 0]));
        assert!(!audio.midi_message(&[0xb4, 128, 0]));
        assert!(!audio.midi_message(&[0xc4, 0, 0]));
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        let mut input = InputBlock::new();
        region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(input.notes[2 * 128 + 60], 0.0);
        // The snapshot precedes this packet's timed events. Its note-on
        // becomes held state for the next packet, not a duplicate first note.
        assert_eq!(input.notes[4 * 128 + 60], 0.0);
        let events = &input.events[..input.event_count];
        assert!(events.contains(&HostEvent::NoteOn {
            time: 0,
            key: 60,
            channel: 4,
            velocity: 90.0 / 127.0
        }));
        assert!(events.contains(&HostEvent::Midi {
            time: 0,
            bytes: [0xb4, 74, 99],
            len: 3
        }));
        assert!(events.contains(&HostEvent::Midi {
            time: 0,
            bytes: [0xc4, 11, 0],
            len: 2
        }));
        assert!(events.contains(&HostEvent::Midi {
            time: 0,
            bytes: [0xe4, 127, 127],
            len: 3
        }));
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(input.notes[2 * 128 + 60], 0.0);
        assert_eq!(input.notes[4 * 128 + 60], 90.0 / 127.0);
    }

    #[test]
    fn offline_done_deadline_race_fails_and_clears_both_buffers() {
        offline_done_race(false);
    }
    fn offline_done_race(cancellation: bool) {
        let (mut audio, region) = make(Kind::Effect, 0);
        audio.process(&mut [0.5; 128], &mut [0.5; 128]);
        let mut input = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        let mut output = OutputBlock::silent();
        output.processed_generation = input.control_end;
        output.left.fill(0.25);
        output.right.fill(0.25);
        region.complete(slot, sequence, &output, OUTPUT_OK);
        // A valid first chunk is already available. The second/final chunk
        // hits the wait boundary; errors must erase the earlier returned prefix.
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        let start = std::time::Instant::now();
        let deadline = start + std::time::Duration::from_secs(1);
        let now = std::cell::Cell::new(start);
        let cancelled = AtomicBool::new(false);
        let mut left = [0.5; 128];
        let mut right = left;
        let result = audio.process_offline_with_scheduler(
            &mut left,
            &mut right,
            deadline,
            &cancelled,
            || now.get(),
            || {
                // Exactly the polling sleep boundary, no scheduling guess.
                if cancellation {
                    cancelled.store(true, Ordering::Release);
                } else {
                    now.set(deadline);
                }
                region.complete(slot, sequence, &output, OUTPUT_OK);
            },
        );
        assert_eq!(
            result,
            Err(if cancellation {
                OfflineError::Cancelled
            } else {
                OfflineError::Deadline
            })
        );
        assert_eq!(left, [0.0; 128]);
        assert_eq!(right, [0.0; 128]);
        assert_eq!(audio.health().completed_blocks, 1);
    }
    #[test]
    fn healthy_impulse_measures_two_blocks_with_irregular_callbacks() {
        // The deterministic helper gets one scheduling interval between input
        // publication and output choice. Host fragments remain irregular.
        for callback in [1, 7, 64, 480, 512] {
            let (mut audio, region) = make(Kind::Effect, 0);
            let mut input = InputBlock::new();
            let mut output = OutputBlock::silent();
            let mut actual = vec![];
            let mut source = 0;
            while source < 2048 {
                let end = (source + callback).min(2048);
                while source < end {
                    let until_boundary = 64 - audio.cursor;
                    let count = (end - source).min(until_boundary);
                    let mut left = [0.0; MAX_BLOCK];
                    let mut right = [0.0; MAX_BLOCK];
                    if source == 0 {
                        left[0] = 1.0;
                        right[0] = 1.0;
                    }
                    audio.process(&mut left[..count], &mut right[..count]);
                    actual.extend_from_slice(&left[..count]);
                    source += count;
                    complete(&region, &mut input, &mut output);
                }
            }
            assert_eq!(
                actual.iter().position(|sample| *sample != 0.0),
                Some(128),
                "callback {callback}"
            );
            assert_eq!(actual[128], 1.0);
            assert_eq!(audio.latency(), 128);
            assert_eq!(audio.health().missed_blocks, 0);
        }
    }
    #[test]
    fn delayed_fallback_includes_native_latency_and_instruments_stay_silent() {
        for kind in [Kind::Effect, Kind::Instrument] {
            let (mut audio, _) = make(kind, 17);
            let mut left = [0.0; 512];
            let mut right = left;
            left[0] = 0.5;
            right[0] = 0.5;
            audio.process(&mut left, &mut right);
            assert_eq!(audio.latency(), 145);
            assert_eq!(
                left.iter().position(|sample| *sample != 0.0),
                if kind == Kind::Effect {
                    Some(145)
                } else {
                    None
                }
            );
            assert_eq!(audio.health().acknowledged_generation, 0);
        }
    }
    #[test]
    fn proof_getters_are_readonly_and_require_collection_of_matching_complete_output() {
        let (mut audio, region) = make(Kind::Effect, 0);
        let initial = audio.collection_frontier();
        assert_eq!(initial, (1, 0, audio.desired_generation()));
        assert_eq!(audio.collection_frontier(), initial);
        assert_eq!(audio.completed_proof(), None);
        assert!(audio.set_param(7, 0.75));
        let desired = audio.desired_generation();
        assert_eq!(audio.collection_frontier(), (1, 0, desired));
        assert_eq!(audio.desired_generation(), desired);
        audio.process(&mut [0.5; 64], &mut [0.5; 64]);
        assert_eq!(audio.completed_proof(), None);
        let mut input = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        let output = OutputBlock {
            epoch: input.epoch,
            processed_generation: input.control_end,
            ..OutputBlock::silent()
        };
        region.complete(slot, sequence, &output, OUTPUT_OK);
        assert_eq!(audio.completed_proof(), None);
        audio.process(&mut [0.5; 64], &mut [0.5; 64]);
        assert_eq!(audio.completed_proof(), None);
        audio.process(&mut [0.5; 64], &mut [0.5; 64]);
        assert_eq!(audio.completed_proof(), Some((1, sequence, desired)));
        let proof = audio.completed_proof();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        let unknown = OutputBlock {
            epoch: input.epoch,
            ..OutputBlock::silent()
        };
        region.complete(slot, sequence, &unknown, OUTPUT_OK);
        audio.process(&mut [0.5; 64], &mut [0.5; 64]);
        assert_eq!(audio.completed_proof(), proof);
        assert_eq!(audio.desired_generation(), desired);
        audio.reset_timeline();
        assert_eq!(audio.completed_proof(), None);
    }
    #[test]
    fn native_ack_requires_matching_complete_and_old_epoch_cannot_restore_audio() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        assert!(audio.set_param(7, 0.75));
        let desired = audio.desired_generation();
        let mut left = [0.0; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert_eq!(audio.completed_proof(), None);
        let mut worker = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut worker).unwrap().unwrap();
        let mut output = OutputBlock::silent();
        output.left.fill(1.0);
        output.epoch = worker.epoch;
        output.processed_generation = desired;
        audio.reset_timeline();
        region.complete(slot, sequence, &output, OUTPUT_OK);
        for _ in 0..5 {
            audio.process(&mut left, &mut right);
            assert!(left.iter().all(|sample| *sample == 0.0));
        }
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert_eq!(audio.completed_proof(), None);
    }
    #[test]
    fn reset_authority_exhaustion_and_corruption_clear_proof_without_wrap() {
        use super::super::slots::WordStorage;
        for failure in [0, 1, 2] {
            let (mut audio, region) = make(Kind::Effect, 0);
            audio.health.acknowledged_generation = 7;
            audio.completed_proof = Some((1, 0, 7));
            audio
                .signals
                .processed_generation
                .store(7, Ordering::Release);
            if failure == 0 {
                audio.epoch = u64::from(u32::MAX);
            } else if failure == 1 {
                audio.sequence = u64::MAX;
            } else {
                // A foreign/corrupt header disagrees with this sole endpoint.
                region.publish_timeline_epoch(1, 2).unwrap();
            }
            let desired = audio.desired_generation();
            audio.reset_timeline();
            assert!(audio.signals.failed.load(Ordering::Acquire));
            assert!(region.helper_failed());
            assert_eq!(audio.health.acknowledged_generation, 0);
            assert_eq!(audio.completed_proof(), None);
            assert_eq!(
                audio.signals.processed_generation.load(Ordering::Acquire),
                0
            );
            assert_eq!(audio.desired_generation(), desired);
            let mut left = [0.25; 512];
            let mut right = left;
            audio.process(&mut left, &mut right);
            assert!(region.pending_sequence().is_none());
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        }
        let storage = LocalWords::new();
        let region = Region::initialize(storage.clone(), make(Kind::Effect, 0).1.config).unwrap();
        let mut audio = Audio::new(region, Arc::new(Signals::default()), &[]).unwrap();
        // Reach the actual maximum consistently, without billions of resets.
        audio.epoch = u64::from(u32::MAX);
        storage.words()[TIMELINE_EPOCH].store(u32::MAX.to_le(), Ordering::Release);
        audio.reset_timeline();
        assert_eq!(
            u32::from_le(storage.words()[TIMELINE_EPOCH].load(Ordering::Acquire)),
            u32::MAX
        );
        assert!(audio.signals.failed.load(Ordering::Acquire));
    }
    #[test]
    fn corrupt_authority_cannot_collect_an_old_done_proof_without_a_reset_call() {
        use super::super::slots::WordStorage;
        let storage = LocalWords::new();
        let region = Region::initialize(storage.clone(), make(Kind::Effect, 0).1.config).unwrap();
        let mut audio = Audio::new(region.clone(), Arc::new(Signals::default()), &[]).unwrap();
        audio.process(&mut [0.25; 128], &mut [0.25; 128]);
        let mut input = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        region.complete(
            slot,
            sequence,
            &OutputBlock {
                processed_generation: 1,
                ..OutputBlock::silent()
            },
            OUTPUT_OK,
        );
        storage.words()[TIMELINE_EPOCH].store(2u32.to_le(), Ordering::Release);
        audio.process(&mut [0.25; 64], &mut [0.25; 64]);
        assert!(audio.signals.failed.load(Ordering::Acquire));
        assert!(region.helper_failed());
        assert_eq!(audio.completed_proof(), None);
        assert_eq!(audio.health.acknowledged_generation, 0);
    }
    #[test]
    fn persistent_failure_finishes_the_fallback_ramp_instead_of_restarting_it() {
        let (mut audio, region) = make(Kind::Effect, 0);
        let mut input = InputBlock::new();
        for _ in 0..4 {
            audio.process(&mut [0.25; 64], &mut [0.25; 64]);
            let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
            region.complete(
                slot,
                sequence,
                &OutputBlock {
                    epoch: 1,
                    processed_generation: input.control_end,
                    left: [0.125; MAX_BLOCK],
                    right: [0.125; MAX_BLOCK],
                    native_drops: 0,
                },
                OUTPUT_OK,
            );
        }
        region.latch_helper_failure();
        audio.process(&mut [0.25; 64], &mut [0.25; 64]); // One 64-frame transition.
        let mut left = [0.25; 64];
        let mut right = left;
        for _ in 0..4 {
            audio.process(&mut left, &mut right);
            assert_eq!(left, [0.25; 64]);
            assert_eq!(right, left);
        }
    }
    #[test]
    fn saturation_reserves_releases_and_retains_rejected_current_controls() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        for _ in 0..ORDINARY_EVENTS {
            assert!(audio.note_on(60, 1.0));
        }
        assert!(!audio.set_param(7, 0.75));
        assert!(!audio.note_on(61, 0.5));
        for _ in 0..8 {
            for key in 0..128 {
                assert!(audio.note_off(key));
            }
            audio.all_notes_off();
        }
        assert!(audio.input.event_count <= EVENT_CAPACITY);
        assert_eq!(audio.voices(), 0);
        assert!(!audio.input.controls_complete);
        let mut left = [0.0; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        let mut worker = InputBlock::new();
        let mut output = OutputBlock::silent();
        complete(&region, &mut worker, &mut output);
        assert!(!worker.controls_complete);
        audio.process(&mut left, &mut right);
        complete(&region, &mut worker, &mut output);
        assert!(worker.controls_complete);
        assert_eq!(worker.parameters[0].value, 0.75);
        assert_eq!(worker.notes, [0.0; NOTE_KEYS]);
    }

    #[test]
    fn offline_rejects_shared_native_failure_before_the_supervisor_signal() {
        let (mut audio, region) = make(Kind::Effect, 0);
        audio.process(&mut [0.25; 128], &mut [0.25; 128]);
        let mut input = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        region.latch_helper_failure();
        region.complete(slot, sequence, &OutputBlock::silent(), OUTPUT_FAILED);
        assert!(!audio.signals().failed.load(Ordering::Acquire));
        let mut left = [0.5; 64];
        let mut right = left;
        let result = audio.process_offline(
            &mut left,
            &mut right,
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            &AtomicBool::new(false),
        );
        assert_eq!(result, Err(OfflineError::Failed));
        assert!(left.iter().chain(&right).all(|value| *value == 0.0));
    }

    #[test]
    fn advancing_past_wire_transport_bounds_fails_closed_without_callback_panic() {
        let (mut audio, _) = make(Kind::Effect, 0);
        let transport = Transport {
            position_seconds: 1e9,
            position_beats: 1e9,
            playing: true,
            ..Transport::default()
        };
        assert!(audio.set_transport(transport));
        audio.process(&mut [0.25; 512], &mut [0.25; 512]);
        assert!(audio.signals().failed.load(Ordering::Acquire));
    }

    #[test]
    fn a_second_panic_releases_notes_admitted_after_the_first_panic() {
        let (mut audio, region) = make(Kind::Instrument, 0);
        audio.all_notes_off();
        assert!(audio.note_on(60, 0.75));
        audio.all_notes_off();
        audio.process(&mut [0.0; 64], &mut [0.0; 64]);
        let mut input = InputBlock::new();
        let _owned = region.take_input(&mut input).unwrap().unwrap();
        assert_eq!(input.event_count, 3);
        assert!(matches!(input.events[2], HostEvent::AllNotesOff { .. }));
        assert_eq!(audio.voices(), 0);
    }

    #[test]
    fn missing_output_transition_is_continuous_and_counters_never_wrap() {
        let (mut audio, region) = make(Kind::Effect, 0);
        let mut left = [0.5; 64];
        let mut right = left;
        let mut worker = InputBlock::new();
        audio.process(&mut left, &mut right);
        let (slot, sequence) = region.take_input(&mut worker).unwrap().unwrap();
        let mut output = OutputBlock::silent();
        output.left.fill(0.25);
        output.right.fill(0.25);
        output.processed_generation = worker.control_end;
        region.complete(slot, sequence, &output, OUTPUT_OK);
        left.fill(0.5);
        right.fill(0.5);
        audio.process(&mut left, &mut right);
        left.fill(0.5);
        right.fill(0.5);
        audio.process(&mut left, &mut right);
        assert_eq!(left, [0.25; 64]);
        left.fill(0.5);
        right.fill(0.5);
        audio.process(&mut left, &mut right);
        assert_eq!(left[0], 0.25);
        assert!(
            left.windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() <= 0.25 / 64.0 + 1e-7)
        );
        audio.generation = u64::MAX;
        assert!(!audio.set_param(7, 0.75));
        assert!(audio.signals.failed.load(Ordering::Acquire));
        let (mut audio, _) = make(Kind::Effect, 0);
        audio.sequence = u64::MAX;
        audio.process(&mut left, &mut right);
        assert!(audio.signals.failed.load(Ordering::Acquire));
        assert_eq!(audio.health.submitted_blocks, 0);
    }
}
