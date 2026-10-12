//! Bounded ownership of atomic payload words. No shared Rust object layout.

use super::protocol::*;
use crate::{HostEvent, Transport};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// A mapping owner exposes aligned atomic words and keeps them mapped until
/// every endpoint has been retired on a control thread.
pub trait WordStorage: Send + Sync {
    fn words(&self) -> &[AtomicU32];
}

/// Local backing for deterministic protocol/adapter tests, never crash isolation.
pub struct LocalWords(Box<[AtomicU32]>);
impl LocalWords {
    pub fn new() -> Arc<Self> {
        Arc::new(Self((0..REGION_WORDS).map(|_| AtomicU32::new(0)).collect()))
    }
}
impl WordStorage for LocalWords {
    fn words(&self) -> &[AtomicU32] {
        &self.0
    }
}

#[derive(Clone)]
pub struct Region {
    storage: Arc<dyn WordStorage>,
    pub config: Config,
}
impl Region {
    /// Control-side initialization before either endpoint is installed.
    pub fn initialize(
        storage: Arc<dyn WordStorage>,
        config: Config,
    ) -> Result<Self, ProtocolError> {
        if storage.words().len() != REGION_WORDS {
            return Err(ProtocolError::Layout);
        }
        let header = config.header()?;
        // Pre-touch every page before installation, including otherwise unused
        // slots. OS residency remains outside a headless deadline guarantee.
        for cell in storage.words() {
            cell.store(0, Ordering::Relaxed);
        }
        for (index, (cell, word)) in storage.words()[..HEADER_WORDS]
            .iter()
            .zip(header)
            .enumerate()
        {
            if index != TIMELINE_EPOCH {
                cell.store(word.to_le(), Ordering::Relaxed);
            }
        }
        // The first nonzero authority is this Release, not an earlier relaxed
        // copy of the same value that attach could otherwise acquire.
        storage.words()[TIMELINE_EPOCH].store(1u32.to_le(), Ordering::Release);
        Ok(Self { storage, config })
    }
    /// Check the entire exact header before loading native code.
    pub fn attach(storage: Arc<dyn WordStorage>, expected: Config) -> Result<Self, ProtocolError> {
        if storage.words().len() != REGION_WORDS {
            return Err(ProtocolError::Layout);
        }
        if u32::from_le(storage.words()[TIMELINE_EPOCH].load(Ordering::Acquire)) != 1 {
            return Err(ProtocolError::Sequence);
        }
        let header = std::array::from_fn::<_, HEADER_WORDS, _>(|index| {
            u32::from_le(storage.words()[index].load(Ordering::Relaxed))
        });
        let config = Config::from_header(&header, storage.words().len() * 4)?;
        if config != expected {
            return Err(ProtocolError::Identity);
        }
        Ok(Self { storage, config })
    }
    /// Startup negotiation only; neither endpoint may process yet.
    pub fn negotiate_latency(&mut self, native_latency: usize) -> Result<(), ProtocolError> {
        let config = Config {
            native_latency,
            ..self.config
        }
        .validate()?;
        self.storage.words()[11].store((native_latency as u32).to_le(), Ordering::Release);
        self.config = config;
        Ok(())
    }
    pub(super) fn timeline_epoch(&self) -> Result<u64, ProtocolError> {
        let epoch = u32::from_le(self.storage.words()[TIMELINE_EPOCH].load(Ordering::Acquire));
        if epoch == 0 || self.helper_failed() {
            return Err(ProtocolError::Sequence);
        }
        Ok(u64::from(epoch))
    }
    /// Sole audio publisher, one attempt. Called after local proof invalidation
    /// and before any new-epoch READY. Never changes the helper's slot ownership.
    pub(super) fn publish_timeline_epoch(
        &self,
        previous: u64,
        next: u64,
    ) -> Result<(), ProtocolError> {
        let previous = u32::try_from(previous).map_err(|_| ProtocolError::Sequence)?;
        let next = u32::try_from(next).map_err(|_| ProtocolError::Sequence)?;
        if previous == 0 || previous.checked_add(1) != Some(next) || self.helper_failed() {
            return Err(ProtocolError::Sequence);
        }
        self.storage.words()[TIMELINE_EPOCH]
            .compare_exchange(
                previous.to_le(),
                next.to_le(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| ProtocolError::Sequence)
    }
    /// Helper-only return of a claimed stale input, after its last payload read.
    /// No native call, DONE, liveness increment or DSP proof is implied.
    #[cfg(any(windows, test))]
    pub(super) fn retire_input(&self, slot: usize) -> Result<(), ProtocolError> {
        self.state(slot)
            .compare_exchange(
                HELPER_WRITE.to_le(),
                EMPTY.to_le(),
                Ordering::Release,
                Ordering::Relaxed,
            )
            .map(|_| ())
            .map_err(|_| ProtocolError::Sequence)
    }
    /// Native owner publication, off the callback. Exhaustion fails this helper.
    #[cfg(windows)]
    pub(super) fn publish_native_change(&self) -> Result<(), ProtocolError> {
        let word = &self.storage.words()[NATIVE_CHANGES];
        let current = u32::from_le(word.load(Ordering::Relaxed));
        let next = current.checked_add(1).ok_or(ProtocolError::Sequence)?;
        word.store(next.to_le(), Ordering::Release);
        Ok(())
    }
    pub fn native_change_serial(&self) -> u64 {
        u64::from(u32::from_le(
            self.storage.words()[NATIVE_CHANGES].load(Ordering::Acquire),
        ))
    }
    fn offset(slot: usize, offset: usize) -> usize {
        HEADER_WORDS + slot * SLOT_WORDS + offset
    }
    fn get(&self, slot: usize, offset: usize) -> u32 {
        u32::from_le(self.storage.words()[Self::offset(slot, offset)].load(Ordering::Relaxed))
    }
    fn put(&self, slot: usize, offset: usize, value: u32) {
        self.storage.words()[Self::offset(slot, offset)].store(value.to_le(), Ordering::Relaxed);
    }
    fn state(&self, slot: usize) -> &AtomicU32 {
        &self.storage.words()[Self::offset(slot, STATE)]
    }
    fn claim(&self, slot: usize, before: u32, after: u32) -> bool {
        self.state(slot)
            .compare_exchange(
                before.to_le(),
                after.to_le(),
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }
    fn publish(&self, slot: usize, state: u32) {
        self.state(slot).store(state.to_le(), Ordering::Release);
    }
    fn state_value(&self, slot: usize) -> u32 {
        u32::from_le(self.state(slot).load(Ordering::Acquire))
    }
    fn read_identity(&self, slot: usize, offset: usize) -> Result<Identity, ProtocolError> {
        Identity::decode(std::array::from_fn(|index| self.get(slot, offset + index)))
    }
    fn write_identity(&self, slot: usize, offset: usize) {
        for (index, word) in self.config.identity.encode().iter().enumerate() {
            self.put(slot, offset + index, *word);
        }
    }
    fn sequence(&self, slot: usize, offset: usize) -> u64 {
        pair(self.get(slot, offset), self.get(slot, offset + 1))
    }
    fn write_sequence(&self, slot: usize, offset: usize, sequence: u64) {
        self.put(slot, offset, sequence as u32);
        self.put(slot, offset + 1, (sequence >> 32) as u32);
    }
    /// The host can recycle expired pending/completed slots, never a live
    /// helper-owned one. Exactly four single-attempt CAS operations.
    pub fn discard_before(&self, sequence: u64) {
        for slot in 0..SLOT_COUNT {
            let state = self.state_value(slot);
            if matches!(state, READY | DONE)
                && self.sequence(slot, SEQUENCE) < sequence
                && self.claim(slot, state, HOST_WRITE)
            {
                self.publish(slot, EMPTY);
            }
        }
    }
    pub fn submit(&self, sequence: u64, block: &InputBlock) -> bool {
        if block.instance_count > crate::MAX_NOTE_INSTANCES
            || block.instances[..block.instance_count]
                .iter()
                .any(|note| !note.valid())
            || block
                .notes
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || block.parameter_count > PARAM_CAPACITY
            || block.sidechain_input.is_some_and(|index| index >= 64)
            || block.event_count > EVENT_CAPACITY
            || self.timeline_epoch() != Ok(block.epoch)
        {
            return false;
        }
        let Ok(transport) = encode_transport(block.transport) else {
            return false;
        };
        if block.parameters[..block.parameter_count]
            .iter()
            .any(|parameter| parameter.encode().is_err())
            || block.events[..block.event_count]
                .iter()
                .any(|event| encode_event(*event, self.config.block).is_err())
        {
            return false;
        }
        let Some(slot) = (0..SLOT_COUNT).find(|slot| self.claim(*slot, EMPTY, HOST_WRITE)) else {
            return false;
        };
        self.write_identity(slot, IDENTITY);
        self.write_sequence(slot, SEQUENCE, sequence);
        self.put(slot, FRAMES, self.config.block as u32);
        self.put(slot, EVENT_COUNT, block.event_count as u32);
        self.put(slot, PARAM_COUNT, block.parameter_count as u32);
        self.put(slot, FLAGS, u32::from(!block.controls_complete));
        self.put(slot, NOTE_CHANNEL, 0);
        self.put(slot, INSTANCE_COUNT, block.instance_count as u32);
        self.put(
            slot,
            SIDECHAIN_PORT,
            block.sidechain_input.map_or(0, |index| index + 1),
        );
        self.write_sequence(slot, EPOCH, block.epoch);
        self.write_sequence(slot, CONTROL_START, block.control_start);
        self.write_sequence(slot, CONTROL_END, block.control_end);
        // Values were checked on admission; invalid transport is retained locally.
        for (index, word) in transport.iter().enumerate() {
            self.put(slot, TRANSPORT + index, *word);
        }
        for index in 0..self.config.block {
            self.put(
                slot,
                KEY_INPUT + index,
                finite_input(block.key[index][0]).to_bits(),
            );
            self.put(
                slot,
                KEY_INPUT + MAX_BLOCK + index,
                finite_input(block.key[index][1]).to_bits(),
            );
            self.put(
                slot,
                INPUT + index,
                finite_input(block.left[index]).to_bits(),
            );
            self.put(
                slot,
                INPUT + MAX_BLOCK + index,
                finite_input(block.right[index]).to_bits(),
            );
        }
        for (index, velocity) in block.notes.iter().enumerate() {
            self.put(slot, NOTES + index, velocity.to_bits());
        }
        for (index, note) in block.instances[..block.instance_count].iter().enumerate() {
            for (word, value) in note.encode().into_iter().enumerate() {
                self.put(slot, INSTANCES + index * NOTE_SNAPSHOT_WORDS + word, value);
            }
        }
        for (index, parameter) in block.parameters[..block.parameter_count].iter().enumerate() {
            let words = parameter.encode().expect("admitted parameter");
            for (word, value) in words.iter().enumerate() {
                self.put(slot, PARAMETERS + index * 3 + word, *value);
            }
        }
        for (index, event) in block.events[..block.event_count].iter().enumerate() {
            let words = encode_event(*event, self.config.block).expect("admitted event");
            for (word, value) in words.iter().enumerate() {
                self.put(slot, EVENTS + index * EVENT_WORDS + word, *value);
            }
        }
        self.publish(slot, READY);
        true
    }
    /// Native owner thread claims the oldest ready sequence. No payload is
    /// accessed before the acquire; the native plugin never sees mapped pointers.
    pub fn take_input(
        &self,
        block: &mut InputBlock,
    ) -> Result<Option<(usize, u64)>, ProtocolError> {
        let slot = (0..SLOT_COUNT)
            .filter(|slot| self.state_value(*slot) == READY)
            .min_by_key(|slot| self.sequence(*slot, SEQUENCE));
        let Some(slot) = slot else {
            return Ok(None);
        };
        if !self.claim(slot, READY, HELPER_WRITE) {
            return Ok(None);
        }
        let sequence = self.sequence(slot, SEQUENCE);
        let result = self.read_input(slot, block);
        if let Err(error) = result {
            self.complete(slot, sequence, &OutputBlock::silent(), OUTPUT_FAILED);
            return Err(error);
        }
        Ok(Some((slot, sequence)))
    }
    fn read_input(&self, slot: usize, block: &mut InputBlock) -> Result<(), ProtocolError> {
        if self.read_identity(slot, IDENTITY)? != self.config.identity {
            return Err(ProtocolError::Identity);
        }
        let frames = self.get(slot, FRAMES) as usize;
        let events = self.get(slot, EVENT_COUNT) as usize;
        let params = self.get(slot, PARAM_COUNT) as usize;
        let instances = self.get(slot, INSTANCE_COUNT) as usize;
        if frames != self.config.block
            || events > EVENT_CAPACITY
            || params > PARAM_CAPACITY
            || instances > crate::MAX_NOTE_INSTANCES
            || self.get(slot, FLAGS) > 1
            || self.get(slot, NOTE_CHANNEL) != 0
        {
            return Err(ProtocolError::Layout);
        }
        block.transport = decode_transport(std::array::from_fn(|index| {
            self.get(slot, TRANSPORT + index)
        }))?;
        let input = self.get(slot, SIDECHAIN_PORT);
        if input > 64 {
            return Err(ProtocolError::Layout);
        }
        block.sidechain_input = input.checked_sub(1);
        for index in 0..frames {
            block.left[index] = f32::from_bits(self.get(slot, INPUT + index));
            block.right[index] = f32::from_bits(self.get(slot, INPUT + MAX_BLOCK + index));
            block.key[index] = [
                f32::from_bits(self.get(slot, KEY_INPUT + index)),
                f32::from_bits(self.get(slot, KEY_INPUT + MAX_BLOCK + index)),
            ];
            if !block.left[index].is_finite()
                || !block.right[index].is_finite()
                || block.key[index].iter().any(|sample| !sample.is_finite())
            {
                return Err(ProtocolError::Audio);
            }
        }
        for (index, note) in block.notes.iter_mut().enumerate() {
            *note = f32::from_bits(self.get(slot, NOTES + index));
            if !note.is_finite() || !(0.0..=1.0).contains(note) {
                return Err(ProtocolError::Event);
            }
        }
        for index in 0..instances {
            let note = NoteSnapshot::decode(std::array::from_fn(|word| {
                self.get(slot, INSTANCES + index * NOTE_SNAPSHOT_WORDS + word)
            }))?;
            if block.instances[..index]
                .iter()
                .any(|other| other.id == note.id)
            {
                return Err(ProtocolError::Event);
            }
            block.instances[index] = note;
        }
        block.instance_count = instances;
        for index in 0..params {
            block.parameters[index] = Parameter::decode(std::array::from_fn(|word| {
                self.get(slot, PARAMETERS + index * 3 + word)
            }))?;
        }
        for index in 0..events {
            block.events[index] = decode_event(
                std::array::from_fn(|word| self.get(slot, EVENTS + index * EVENT_WORDS + word)),
                frames,
            )?;
            if index > 0 && block.events[index - 1].time() > block.events[index].time() {
                return Err(ProtocolError::Event);
            }
        }
        block.parameter_count = params;
        block.event_count = events;
        block.epoch = self.sequence(slot, EPOCH);
        block.control_start = self.sequence(slot, CONTROL_START);
        block.control_end = self.sequence(slot, CONTROL_END);
        block.controls_complete = self.get(slot, FLAGS) == 0;
        if block.epoch == 0
            || block.epoch > u64::from(u32::MAX)
            || block.control_start > block.control_end
        {
            return Err(ProtocolError::Sequence);
        }
        Ok(())
    }
    /// Only the helper owning this slot calls complete. Failed blocks contain
    /// no usable output; the host validates reply identity and sequence again.
    pub fn complete(&self, slot: usize, sequence: u64, output: &OutputBlock, status: u32) {
        self.write_identity(slot, REPLY_IDENTITY);
        self.write_sequence(slot, REPLY_SEQUENCE, sequence);
        self.put(slot, REPLY_FRAMES, self.config.block as u32);
        self.put(slot, REPLY_STATUS, status);
        self.write_sequence(slot, REPLY_EPOCH, output.epoch);
        self.write_sequence(slot, PROCESSED_GENERATION, output.processed_generation);
        self.put(slot, NATIVE_DROPS, output.native_drops);
        for index in 0..self.config.block {
            self.put(slot, OUTPUT + index, output.left[index].to_bits());
            self.put(
                slot,
                OUTPUT + MAX_BLOCK + index,
                output.right[index].to_bits(),
            );
        }
        self.publish(slot, DONE);
    }
    /// Callback: inspect four slots and copy only one acquired matching reply.
    /// No retries, allocation, clock, locks, wait or process/pipe/file operation.
    pub fn receive(
        &self,
        sequence: u64,
        epoch: u64,
        output: &mut OutputBlock,
    ) -> Result<bool, ProtocolError> {
        for slot in 0..SLOT_COUNT {
            if self.state_value(slot) != DONE || self.sequence(slot, SEQUENCE) != sequence {
                continue;
            }
            if !self.claim(slot, DONE, HOST_READ) {
                continue;
            }
            let result = self.read_output(slot, sequence, epoch, output);
            self.publish(slot, EMPTY);
            return result.map(|()| true);
        }
        Ok(false)
    }
    fn read_output(
        &self,
        slot: usize,
        sequence: u64,
        epoch: u64,
        output: &mut OutputBlock,
    ) -> Result<(), ProtocolError> {
        if self.read_identity(slot, REPLY_IDENTITY)? != self.config.identity {
            return Err(ProtocolError::Identity);
        }
        if self.sequence(slot, REPLY_SEQUENCE) != sequence {
            return Err(ProtocolError::Sequence);
        }
        if self.sequence(slot, REPLY_EPOCH) != epoch {
            return Err(ProtocolError::Sequence);
        }
        if self.get(slot, REPLY_FRAMES) as usize != self.config.block
            || self.get(slot, REPLY_STATUS) != OUTPUT_OK
        {
            return Err(ProtocolError::Audio);
        }
        for index in 0..self.config.block {
            output.left[index] = f32::from_bits(self.get(slot, OUTPUT + index));
            output.right[index] = f32::from_bits(self.get(slot, OUTPUT + MAX_BLOCK + index));
            if !output.left[index].is_finite() || !output.right[index].is_finite() {
                return Err(ProtocolError::Audio);
            }
        }
        // Even a malformed peer changing metadata during copy cannot make a
        // stale sequence or owner acceptable. All payload accesses remain atomic.
        if self.read_identity(slot, REPLY_IDENTITY)? != self.config.identity
            || self.sequence(slot, REPLY_SEQUENCE) != sequence
            || self.sequence(slot, REPLY_EPOCH) != epoch
        {
            return Err(ProtocolError::Identity);
        }
        output.epoch = epoch;
        output.processed_generation = self.sequence(slot, PROCESSED_GENERATION);
        output.native_drops = self.get(slot, NATIVE_DROPS);
        if output.processed_generation > self.sequence(slot, CONTROL_END) {
            return Err(ProtocolError::Sequence);
        }
        Ok(())
    }
    /// Supervisor observation only; performs no ownership transition.
    pub fn busy_sequence(&self) -> Option<u64> {
        (0..SLOT_COUNT)
            .filter(|slot| self.state_value(*slot) == HELPER_WRITE)
            .map(|slot| self.sequence(slot, SEQUENCE))
            .min()
    }
    pub fn pending_sequence(&self) -> Option<u64> {
        (0..SLOT_COUNT)
            .filter(|slot| matches!(self.state_value(*slot), READY | HELPER_WRITE))
            .map(|slot| self.sequence(slot, SEQUENCE))
            .min()
    }
    /// Offline scheduling observation. The callback never waits on this.
    pub fn output_finished(&self, sequence: u64) -> bool {
        (0..SLOT_COUNT)
            .any(|slot| self.state_value(slot) == DONE && self.sequence(slot, SEQUENCE) == sequence)
    }
    /// Helper owner only, after a native process/control/idle call returned.
    /// A liveness observation, never an acknowledgement of parameter/note DSP.
    pub fn owner_completed(&self) -> Result<(), ProtocolError> {
        let cell = &self.storage.words()[OWNER_COMPLETIONS];
        let previous = u32::from_le(cell.load(Ordering::Relaxed));
        let next = previous.checked_add(1).ok_or(ProtocolError::Sequence)?;
        cell.store(next.to_le(), Ordering::Release);
        Ok(())
    }
    pub fn owner_completions(&self) -> u32 {
        u32::from_le(self.storage.words()[OWNER_COMPLETIONS].load(Ordering::Acquire))
    }
    pub fn latch_helper_failure(&self) {
        self.storage.words()[HELPER_FAILURE].store(1, Ordering::Release);
    }
    pub fn helper_failed(&self) -> bool {
        self.storage.words()[HELPER_FAILURE].load(Ordering::Acquire) != 0
    }
}

pub fn finite_input(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1024.0, 1024.0)
    } else {
        0.0
    }
}

/// Held ownership at the first frame, before this block's timed events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSnapshot {
    pub id: u32,
    pub key: u8,
    pub channel: u8,
    pub velocity: f32,
    pub controls: crate::PluginNoteControls,
}
impl NoteSnapshot {
    pub const EMPTY: Self = Self {
        id: 0,
        key: 0,
        channel: 0,
        velocity: 0.0,
        controls: crate::PluginNoteControls {
                volume: 1.0,
            pan: 0.0,
            pitch: 0.0,
            modulation_x: 0.5,
            modulation_y: 0.5,
        },
    };
    pub fn valid(self) -> bool {
        (crate::FIRST_NOTE_INSTANCE_ID..=i32::MAX as u32).contains(&self.id)
            && self.key <= 127
            && self.channel <= 15
            && self.velocity.is_finite()
            && (0.0..=1.0).contains(&self.velocity)
            && self.controls.valid(self.key)
    }
    fn encode(self) -> [u32; NOTE_SNAPSHOT_WORDS] {
        [
            self.id,
            u32::from(self.key),
            u32::from(self.channel),
            self.velocity.to_bits(),
            self.controls.pan.to_bits(),
            self.controls.pitch.to_bits(),
            self.controls.modulation_x.to_bits(),
            self.controls.modulation_y.to_bits(),
            self.controls.volume.to_bits(),
        ]
    }
    fn decode(words: [u32; NOTE_SNAPSHOT_WORDS]) -> Result<Self, ProtocolError> {
        let note = Self {
            id: words[0],
            key: u8::try_from(words[1]).map_err(|_| ProtocolError::Event)?,
            channel: u8::try_from(words[2]).map_err(|_| ProtocolError::Event)?,
            velocity: f32::from_bits(words[3]),
            controls: crate::PluginNoteControls {
                volume: f32::from_bits(words[8]),
                pan: f32::from_bits(words[4]),
                pitch: f32::from_bits(words[5]),
                modulation_x: f32::from_bits(words[6]),
                modulation_y: f32::from_bits(words[7]),
            },
        };
        if !note.valid() {
            return Err(ProtocolError::Event);
        }
        Ok(note)
    }
    pub fn events(self, masks: &[u32; 16], out: &mut [HostEvent; 6]) -> usize {
        out[0] = HostEvent::NoteOnInstance {
            time: 0,
            id: self.id,
            key: self.key,
            channel: self.channel,
            velocity: self.velocity,
        };
        1 + self.expression_events(masks, &mut out[1..])
    }
    pub fn expression_events(self, masks: &[u32; 16], out: &mut [HostEvent]) -> usize {
        use crate::NoteExpressionKind as K;
        let mut count = 0;
        for (kind, value) in [
            (K::Pan, (f64::from(self.controls.pan) + 1.0) / 2.0),
            (
                K::Tuning,
                f64::from(self.controls.pitch) - f64::from(self.key),
            ),
            (K::Brightness, f64::from(self.controls.modulation_x)),
            (K::Expression, f64::from(self.controls.modulation_y)),
            (K::Volume, f64::from(self.controls.volume)),
        ] {
            if masks[self.channel as usize] & kind.mask() != 0 {
                out[count] = HostEvent::NoteExpression {
                    time: 0,
                    id: self.id,
                    key: self.key,
                    channel: self.channel,
                    kind,
                    value,
                };
                count += 1;
            }
        }
        count
    }
}

pub struct InputBlock {
    pub epoch: u64,
    pub control_start: u64,
    pub control_end: u64,
    pub controls_complete: bool,
    pub left: [f32; MAX_BLOCK],
    pub right: [f32; MAX_BLOCK],
    pub key: [[f32; 2]; MAX_BLOCK],
    pub sidechain_input: Option<u32>,
    pub transport: Transport,
    pub notes: [f32; NOTE_KEYS],
    pub instances: Box<[NoteSnapshot]>,
    pub instance_count: usize,
    pub parameters: Box<[Parameter]>,
    pub parameter_count: usize,
    pub events: Box<[HostEvent]>,
    pub event_count: usize,
}
impl InputBlock {
    pub fn new() -> Self {
        Self {
            epoch: 1,
            control_start: 0,
            control_end: 0,
            controls_complete: true,
            left: [0.0; MAX_BLOCK],
            right: [0.0; MAX_BLOCK],
            key: [[0.0; 2]; MAX_BLOCK],
            sidechain_input: None,
            transport: Transport::default(),
            notes: [0.0; NOTE_KEYS],
            instances: vec![NoteSnapshot::EMPTY; crate::MAX_NOTE_INSTANCES].into_boxed_slice(),
            instance_count: 0,
            parameters: vec![Parameter { id: 0, value: 0.0 }; PARAM_CAPACITY].into_boxed_slice(),
            parameter_count: 0,
            events: vec![HostEvent::AllNotesOff { time: 0 }; EVENT_CAPACITY].into_boxed_slice(),
            event_count: 0,
        }
    }
}
impl Default for InputBlock {
    fn default() -> Self {
        Self::new()
    }
}
pub struct OutputBlock {
    pub epoch: u64,
    pub processed_generation: u64,
    pub native_drops: u32,
    pub left: [f32; MAX_BLOCK],
    pub right: [f32; MAX_BLOCK],
}
impl OutputBlock {
    pub const fn silent() -> Self {
        Self {
            epoch: 1,
            processed_generation: 0,
            native_drops: 0,
            left: [0.0; MAX_BLOCK],
            right: [0.0; MAX_BLOCK],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn region() -> Region {
        Region::initialize(
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
                native_latency: 0,
                kind: Kind::Effect,
            },
        )
        .unwrap()
    }
    #[test]
    fn native_owner_completion_fails_before_counter_wrap() {
        let region = region();
        region.storage.words()[OWNER_COMPLETIONS].store((u32::MAX - 1).to_le(), Ordering::Relaxed);
        region.owner_completed().unwrap();
        assert_eq!(region.owner_completions(), u32::MAX);
        assert_eq!(region.owner_completed(), Err(ProtocolError::Sequence));
        assert_eq!(region.owner_completions(), u32::MAX);
    }
    #[test]
    fn reset_header_layout_initialization_and_publication_are_disjoint() {
        let region = region();
        assert_eq!(TIMELINE_EPOCH * 4, 112);
        assert_eq!(OWNER_COMPLETIONS * 4, 108);
        assert_eq!((TRANSPORT, EPOCH, REPLY_IDENTITY), (16, 30, 32));
        assert_eq!(
            (HEADER_WORDS * 4, SLOT_WORDS * 4, REGION_BYTES),
            (256, 266_560, 1_066_496)
        );
        let before: Vec<_> = region
            .storage
            .words()
            .iter()
            .map(|cell| cell.load(Ordering::Relaxed))
            .collect();
        region.publish_timeline_epoch(1, 2).unwrap();
        assert_eq!(region.timeline_epoch(), Ok(2));
        for (index, (word, previous)) in region.storage.words().iter().zip(before).enumerate() {
            if index != TIMELINE_EPOCH {
                assert_eq!(word.load(Ordering::Relaxed), previous, "word {index}");
            }
        }
        assert_eq!(region.owner_completions(), 0);
        assert!(Region::attach(region.storage.clone(), region.config).is_err()); // No hot reattach.
        let input = InputBlock {
            epoch: 2,
            ..InputBlock::new()
        };
        assert!(region.submit(1, &input));
        let mut received = InputBlock::new();
        assert!(region.take_input(&mut received).unwrap().is_some());
        assert_eq!(received.epoch, 2);
        assert_eq!(received.transport, input.transport);
    }
    #[test]
    fn authority_cas_mismatch_zero_and_rollover_never_publish_a_new_epoch() {
        let region = region();
        assert!(region.publish_timeline_epoch(0, 1).is_err());
        assert!(region.publish_timeline_epoch(1, 3).is_err());
        assert!(region.publish_timeline_epoch(2, 3).is_err());
        assert_eq!(region.timeline_epoch(), Ok(1));
        region.storage.words()[TIMELINE_EPOCH].store(u32::MAX.to_le(), Ordering::Release);
        assert!(
            region
                .publish_timeline_epoch(u64::from(u32::MAX), u64::from(u32::MAX) + 1)
                .is_err()
        );
        assert_eq!(region.timeline_epoch(), Ok(u64::from(u32::MAX)));
        region.storage.words()[TIMELINE_EPOCH].store(0, Ordering::Release);
        assert!(region.timeline_epoch().is_err());
        assert!(!region.submit(0, &InputBlock::new()));
    }
    #[test]
    fn helper_alone_retires_claimed_stale_input_without_done_or_liveness() {
        let region = region();
        assert!(region.submit(0, &InputBlock::new()));
        let (slot, _) = region.take_input(&mut InputBlock::new()).unwrap().unwrap();
        region.discard_before(100);
        region.publish_timeline_epoch(1, 2).unwrap();
        assert_eq!(region.busy_sequence(), Some(0));
        region.retire_input(slot).unwrap();
        assert_eq!(region.busy_sequence(), None);
        assert!(!region.output_finished(0));
        assert_eq!(region.owner_completions(), 0);
        assert!(region.retire_input(slot).is_err()); // Cannot take another owner's state.
    }
    #[test]
    fn claimed_late_slot_is_not_reused_and_matching_output_roundtrips() {
        let region = region();
        let input = InputBlock::new();
        assert!(region.submit(0, &input));
        let mut worker = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut worker).unwrap().unwrap();
        region.discard_before(100);
        assert_eq!(region.busy_sequence(), Some(0));
        for sequence in 1..4 {
            assert!(region.submit(sequence, &input));
        }
        assert!(!region.submit(4, &input));
        let mut output = OutputBlock::silent();
        output.left[..64].fill(0.5);
        region.complete(slot, sequence, &output, OUTPUT_OK);
        let mut received = OutputBlock::silent();
        assert_eq!(region.receive(1, 1, &mut received), Ok(false));
        assert_eq!(region.receive(0, 1, &mut received), Ok(true));
        assert_eq!(&received.left[..64], &[0.5; 64]);
        assert_eq!(region.receive(0, 1, &mut received), Ok(false));
    }
    #[test]
    fn invalid_tokens_sequences_samples_and_counts_are_refused() {
        for corrupt in [
            REPLY_IDENTITY,
            REPLY_SEQUENCE,
            REPLY_FRAMES,
            REPLY_STATUS,
            OUTPUT,
        ] {
            let region = region();
            assert!(region.submit(7, &InputBlock::new()));
            let (slot, sequence) = region.take_input(&mut InputBlock::new()).unwrap().unwrap();
            region.complete(slot, sequence, &OutputBlock::silent(), OUTPUT_OK);
            region.put(
                slot,
                corrupt,
                if corrupt == OUTPUT {
                    f32::NAN.to_bits()
                } else {
                    u32::MAX
                },
            );
            assert!(
                region.receive(7, 1, &mut OutputBlock::silent()).is_err(),
                "offset {corrupt}"
            );
            assert_eq!(region.state_value(slot), EMPTY);
        }
        let region = region();
        assert!(region.submit(0, &InputBlock::new()));
        region.put(0, EVENT_COUNT, u32::MAX);
        assert!(region.take_input(&mut InputBlock::new()).is_err());
        assert_eq!(region.state_value(0), DONE);
    }
}
