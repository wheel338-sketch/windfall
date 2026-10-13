//! Fixed-storage channel voice tools. Settings are persisted; prepared state is
//! transient. These modulations supplement the sampler's volume envelope.

#[cfg(test)]
use serde::Deserialize;
use windfall_dsp::blocks::adsr::Adsr;

pub const MAX_CHORD_NOTES: usize = 16;
pub const MAX_HELD_NOTES: usize = 32;
/// Split larger blocks and consume each returned batch before continuing.
pub const MAX_BLOCK_FRAMES: usize = 256;
pub const MAX_EVENTS: usize = MAX_BLOCK_FRAMES * 2;
pub const ECHO_EVENT_CAPACITY: usize = 256;
/// Echoes below -60 dB velocity are not scheduled.
pub const ECHO_VELOCITY_FLOOR: f32 = 0.001;
/// The most repeats one note can have; the settings clamp to it.
pub const MAX_ECHO_REPEATS: usize = 8;
/// Release time of an echo whose source note is still held.
const HELD_OPEN: u64 = u64::MAX;

fn finite(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}
fn sample_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(1.0, 384_000.0)
    } else {
        48_000.0
    }
}
pub use windfall_project::channel_voice::*;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HeldNote {
    pub key: u8,
    pub velocity: f32,
}
impl HeldNote {
    pub fn sanitized(self) -> Self {
        Self {
            key: self.key.min(127),
            velocity: finite(self.velocity, 0.0, 1.0, 0.0),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoteEventKind {
    #[default]
    On,
    Off,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NoteEvent {
    /// Stable within one processor until reset. Match on/off by this id,
    /// scoped to the channel and processor, rather than by MIDI key.
    pub id: u64,
    /// Offset inside the processed portion of this block.
    pub frame: usize,
    pub kind: NoteEventKind,
    pub note: HeldNote,
}
/// Bounded, chronological events. Off precedes on at a shared frame.
pub struct NoteEvents {
    events: [NoteEvent; MAX_EVENTS],
    len: usize,
    pub processed_frames: usize,
}
impl Default for NoteEvents {
    fn default() -> Self {
        Self {
            events: [NoteEvent::default(); MAX_EVENTS],
            len: 0,
            processed_frames: 0,
        }
    }
}
impl NoteEvents {
    pub fn as_slice(&self) -> &[NoteEvent] {
        &self.events[..self.len]
    }
    fn push(&mut self, frame: usize, kind: NoteEventKind, note: HeldNote, id: u64) {
        // Block and queue bounds guarantee capacity, including gate=1.
        assert!(self.len < MAX_EVENTS);
        self.events[self.len] = NoteEvent {
            frame,
            kind,
            note,
            id,
        };
        self.len += 1;
    }
    fn begin(&mut self, frames: usize) {
        self.len = 0;
        self.processed_frames = frames.min(MAX_BLOCK_FRAMES);
    }
}

pub struct Arpeggiator {
    settings: ArpeggiatorSettings,
    chord: [HeldNote; MAX_CHORD_NOTES],
    chord_len: usize,
    expanded: [HeldNote; MAX_CHORD_NOTES * 4],
    expanded_len: usize,
    step: usize,
    until_step: f64,
    until_off: f64,
    sounding: Option<(HeldNote, u64)>,
    next_id: u64,
    release_pending: bool,
}
impl Arpeggiator {
    pub fn prepare(settings: ArpeggiatorSettings) -> Self {
        Self {
            settings: settings.sanitized(),
            chord: [HeldNote::default(); MAX_CHORD_NOTES],
            chord_len: 0,
            expanded: [HeldNote::default(); MAX_CHORD_NOTES * 4],
            expanded_len: 0,
            step: 0,
            until_step: 0.0,
            until_off: 0.0,
            sounding: None,
            next_id: 1,
            release_pending: false,
        }
    }
    /// Input order is play order. Duplicate keys are ignored; first 16 win.
    /// A chord edit releases the old generated note at the next processed frame.
    pub fn set_chord(&mut self, notes: &[HeldNote]) {
        self.chord_len = 0;
        for &note in notes {
            let note = note.sanitized();
            if self.chord[..self.chord_len]
                .iter()
                .any(|held| held.key == note.key)
            {
                continue;
            }
            if self.chord_len == MAX_CHORD_NOTES {
                break;
            }
            self.chord[self.chord_len] = note;
            self.chord_len += 1;
        }
        self.expanded_len = 0;
        for octave in 0..self.settings.range_octaves {
            for note in &self.chord[..self.chord_len] {
                let key = u16::from(note.key) + u16::from(octave) * 12;
                if key <= 127 {
                    self.expanded[self.expanded_len] = HeldNote {
                        key: key as u8,
                        ..*note
                    };
                    self.expanded_len += 1;
                }
            }
        }
        if self.settings.mode != ArpeggiatorMode::AsPlayed {
            self.expanded[..self.expanded_len].sort_unstable_by_key(|note| note.key);
        }
        self.step = 0;
        self.until_step = 0.0;
        self.release_pending = self.sounding.is_some();
    }
    /// Remaining known gate after the current one-frame routing subblock.
    pub(crate) fn gate_frames(&self) -> usize {
        (self.until_off.ceil().max(0.0) as usize).saturating_add(1)
    }
    /// Reset on stop/seek. The caller releases the returned sounding note.
    pub fn reset(&mut self) -> Option<(HeldNote, u64)> {
        self.step = 0;
        self.until_step = 0.0;
        self.release_pending = false;
        self.sounding.take()
    }
    pub fn process_block(&mut self, rate: f64, bpm: f64, frames: usize, out: &mut NoteEvents) {
        out.begin(frames);
        let interval = self.settings.rate.frames(rate, bpm);
        for frame in 0..out.processed_frames {
            if self.release_pending || self.until_off <= 0.0 {
                if let Some((note, id)) = self.sounding.take() {
                    out.push(frame, NoteEventKind::Off, note, id);
                }
                self.release_pending = false;
            }
            if self.until_step <= 0.0
                && self.expanded_len > 0
                && self.settings.mode != ArpeggiatorMode::Off
            {
                if let Some((note, id)) = self.sounding.take() {
                    out.push(frame, NoteEventKind::Off, note, id);
                }
                let len = self.expanded_len;
                let index = match self.settings.mode {
                    ArpeggiatorMode::Down => len - 1 - self.step % len,
                    ArpeggiatorMode::UpDown if len > 1 => {
                        let position = self.step % (2 * (len - 1));
                        position.min(2 * (len - 1) - position)
                    }
                    _ => self.step % len,
                };
                let note = self.expanded[index];
                if self.settings.gate > 0.0 && note.velocity > 0.0 {
                    let id = self.next_id;
                    self.next_id = self.next_id.wrapping_add(1);
                    out.push(frame, NoteEventKind::On, note, id);
                    self.sounding = Some((note, id));
                    // Include fractional step residual so gate=1 meets the next
                    // step without a gap at non-integral note divisions.
                    self.until_off = (interval * f64::from(self.settings.gate) + self.until_step)
                        .max(1.0)
                        .min(interval);
                }
                self.step = (self.step + 1) % (2 * len.max(2) * (len.max(2) - 1));
                self.until_step += interval;
            }
            self.until_step = (self.until_step - 1.0).max(-1.0);
            self.until_off -= 1.0;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DelayedEvent {
    id: u64,
    at: u64,
    kind: NoteEventKind,
    note: HeldNote,
}
/// The echoes scheduled for one held note, to release them when it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldEcho {
    first_id: u64,
    count: u8,
    start: u64,
    delay: f64,
}
impl HeldEcho {
    /// How many repeats were queued.
    pub fn repeats(&self) -> usize {
        usize::from(self.count)
    }
    /// The id of repeat `index`, counted from zero.
    pub fn id(&self, index: usize) -> u64 {
        self.first_id.wrapping_add(index as u64)
    }
}
pub struct NoteEcho {
    settings: NoteEchoSettings,
    queue: [Option<DelayedEvent>; ECHO_EVENT_CAPACITY],
    frame: u64,
    next_id: u64,
}
impl NoteEcho {
    pub fn prepare(settings: NoteEchoSettings) -> Self {
        Self {
            settings: settings.sanitized(),
            queue: [None; ECHO_EVENT_CAPACITY],
            frame: 0,
            next_id: 1,
        }
    }
    fn delay_frames(&self, rate: f64, bpm: f64) -> f64 {
        match self.settings.time {
            EchoTime::Milliseconds { ms } => (sample_rate(rate) * f64::from(ms) / 1000.0).max(1.0),
            EchoTime::Division { division } => division.frames(rate, bpm),
        }
    }
    /// The repeats one note gets: `(start frame, note)` for repeat 1 onward.
    /// The list ends at the first repeat that falls below the velocity floor
    /// or leaves the MIDI range.
    fn repeats(
        &self,
        note: HeldNote,
        start: u64,
        delay: f64,
    ) -> ([(u64, HeldNote); MAX_ECHO_REPEATS], usize) {
        let note = note.sanitized();
        let mut out = [(0, HeldNote::default()); MAX_ECHO_REPEATS];
        let mut count = 0;
        let mut velocity = note.velocity;
        for repeat in 1..=self.settings.repeats.min(MAX_ECHO_REPEATS as u8) {
            velocity *= self.settings.feedback;
            if velocity < ECHO_VELOCITY_FLOOR {
                break;
            }
            let key =
                i16::from(note.key) + i16::from(self.settings.pitch_semitones) * i16::from(repeat);
            if !(0..=127).contains(&key) {
                break;
            }
            let at = start.saturating_add((delay * f64::from(repeat)).round() as u64);
            out[count] = (
                at,
                HeldNote {
                    key: key as u8,
                    velocity,
                },
            );
            count += 1;
        }
        (out, count)
    }
    /// Queues every pair or none of them. `off` gives each repeat's release.
    fn enqueue(&mut self, repeats: &[(u64, HeldNote)], off: impl Fn(u64) -> u64) -> Option<u64> {
        let count = repeats.len() * 2;
        if self.queue.iter().filter(|slot| slot.is_none()).count() < count {
            return None;
        }
        let first = self.next_id;
        let mut free = self.queue.iter_mut().filter(|slot| slot.is_none());
        for (index, &(at, note)) in repeats.iter().enumerate() {
            let id = first.wrapping_add(index as u64);
            for (at, kind) in [(at, NoteEventKind::On), (off(at), NoteEventKind::Off)] {
                *free.next().expect("the free slots were counted") =
                    Some(DelayedEvent { id, at, kind, note });
            }
        }
        self.next_id = self.next_id.wrapping_add(repeats.len() as u64);
        Some(first)
    }
    /// Schedule echoes of one completed note; original is not emitted.
    /// `offset` is relative to the next block; `duration` is the original gate.
    /// Repeat n adds n*pitch offset and multiplies velocity by feedback^n.
    /// False rejects the entire request for capacity. Never feed echoes back.
    pub fn schedule_note(
        &mut self,
        note: HeldNote,
        offset: usize,
        duration: usize,
        rate: f64,
        bpm: f64,
    ) -> bool {
        if !self.settings.enabled {
            return true;
        }
        let delay = self.delay_frames(rate, bpm);
        let start = self.frame.saturating_add(offset as u64);
        let (repeats, count) = self.repeats(note, start, delay);
        let gate = duration.max(1) as u64;
        self.enqueue(&repeats[..count], |at| at.saturating_add(gate))
            .is_some()
    }
    /// Schedule echoes of a note that is still held, whose length is not
    /// known yet. Every repeat's note-on is queued now and its release is
    /// reserved but left open, so capacity is settled here and the later
    /// [`Self::release_held`] cannot fail. `None` rejects the whole request
    /// for capacity. A disabled echo, or one with no audible repeat, returns
    /// a handle with nothing to release.
    pub fn schedule_held(
        &mut self,
        note: HeldNote,
        offset: usize,
        rate: f64,
        bpm: f64,
    ) -> Option<HeldEcho> {
        let delay = self.delay_frames(rate, bpm);
        let start = self.frame.saturating_add(offset as u64);
        let mut held = HeldEcho {
            first_id: self.next_id,
            count: 0,
            start,
            delay,
        };
        if !self.settings.enabled {
            return Some(held);
        }
        let (repeats, count) = self.repeats(note, start, delay);
        held.first_id = self.enqueue(&repeats[..count], |_| HELD_OPEN)?;
        held.count = count as u8;
        Some(held)
    }
    /// Ends the echoes of a held note: each repeat is released as long after
    /// this moment as it started after the note did, so every echo has the
    /// length the note turned out to have (at least one frame). `offset` is
    /// relative to the next block. Repeats already discarded by a reset are
    /// skipped.
    pub fn release_held(&mut self, held: HeldEcho, offset: usize) {
        let released = self.frame.saturating_add(offset as u64);
        for index in 0..u64::from(held.count) {
            let id = held.first_id.wrapping_add(index);
            let shift = (held.delay * (index + 1) as f64).round() as u64;
            let on = held.start.saturating_add(shift);
            let at = released.saturating_add(shift).max(on.saturating_add(1));
            for event in self.queue.iter_mut().flatten() {
                if event.id == id && event.kind == NoteEventKind::Off && event.at == HELD_OPEN {
                    event.at = at;
                }
            }
        }
    }
    /// Discards what is still queued for one repeat. The caller releases
    /// its voice if it is already sounding.
    pub fn cancel(&mut self, id: u64) {
        for slot in &mut self.queue {
            if slot.is_some_and(|event| event.id == id) {
                *slot = None;
            }
        }
    }
    /// Discard future events on stop/seek after the caller releases voices.
    pub fn reset(&mut self) {
        self.queue.fill(None);
        self.frame = 0;
    }
    /// Identity range assigned by the next successful scheduling call.
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id
    }
    pub fn pending_events(&self) -> usize {
        self.queue.iter().filter(|event| event.is_some()).count()
    }
    pub fn process_block(&mut self, frames: usize, out: &mut NoteEvents) {
        out.begin(frames);
        let end = self.frame.saturating_add(out.processed_frames as u64);
        // Scan the fixed event queue once per block, rather than once per
        // sample. Unstable sorting is allocation-free and ids break ties.
        for slot in &mut self.queue {
            if let Some(event) = *slot
                && event.at < end
                && event.at != HELD_OPEN
            {
                let frame = event.at.saturating_sub(self.frame) as usize;
                out.push(frame, event.kind, event.note, event.id);
                *slot = None;
            }
        }
        out.events[..out.len].sort_unstable_by_key(|event| {
            (
                event.frame,
                if event.kind == NoteEventKind::Off {
                    0
                } else {
                    1
                },
                event.id,
            )
        });
        self.frame = end;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolyphonyDecision {
    released: [u8; MAX_HELD_NOTES],
    released_len: usize,
    /// False only when another mono-legato note is still held.
    pub retrigger: bool,
}

impl PolyphonyDecision {
    /// Admits a note without entering it in the held table: it retriggers
    /// and releases nothing. For echoes, which sound alongside the note
    /// they repeat instead of replacing it.
    pub fn alongside() -> Self {
        Self {
            retrigger: true,
            ..Default::default()
        }
    }
    /// Release these existing keys before admitting the new note. Includes
    /// duplicate-key retriggers and all victims when a voice limit shrinks.
    pub fn released_keys(&self) -> &[u8] {
        &self.released[..self.released_len]
    }
}
/// Surviving keys, oldest first. Evicted keys are not remembered for fallback.
pub struct HeldNotes {
    keys: [u8; MAX_HELD_NOTES],
    len: usize,
}
impl Default for HeldNotes {
    fn default() -> Self {
        Self {
            keys: [0; MAX_HELD_NOTES],
            len: 0,
        }
    }
}
impl HeldNotes {
    pub fn as_slice(&self) -> &[u8] {
        &self.keys[..self.len]
    }
    pub fn note_off(&mut self, key: u8) {
        if let Some(index) = self.as_slice().iter().position(|&held| held == key) {
            self.keys.copy_within(index + 1..self.len, index);
            self.len -= 1;
        }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
}
/// Mono legato enforces one survivor even with max_voices > 1. Oldest keys
/// are evicted first, and the decision includes every victim if limits shrink.
pub fn admit_note(held: &mut HeldNotes, key: u8, settings: PolyphonySettings) -> PolyphonyDecision {
    let settings = settings.sanitized();
    let key = key.min(127);
    let overlapping = held.len > 0;
    let duplicate = held.as_slice().contains(&key);
    held.note_off(key);
    let limit = if settings.mono_legato {
        1
    } else {
        usize::from(settings.max_voices)
    };
    let mut decision = PolyphonyDecision {
        retrigger: !(settings.mono_legato && overlapping),
        ..Default::default()
    };
    if duplicate {
        decision.released[0] = key;
        decision.released_len = 1;
    }
    while held.len >= limit {
        decision.released[decision.released_len] = held.keys[0];
        decision.released_len += 1;
        held.keys.copy_within(1..held.len, 0);
        held.len -= 1;
    }
    held.keys[held.len] = key;
    held.len += 1;
    decision
}

/// One instance per sounding voice, separate from its volume envelope.
#[derive(Clone, Copy)]
pub struct PreparedChannelEnvelopes {
    settings: ChannelEnvelopes,
    /// Cutoff, pitch, pan, resonance.
    generators: [Adsr; 4],
    /// The assignable LFO.
    phase: f64,
    /// The dedicated LFOs: volume, pan, pitch, cutoff, resonance.
    phases: [f64; 5],
    /// Frames each dedicated LFO stays out after a note starts.
    delays: [u32; 5],
    /// Frames since the last note start, saturating.
    since_gate: u32,
    /// Frames an LFO takes to fade in once its delay has passed.
    fade: u32,
    rate: f64,
}
pub struct ModulationBlock {
    /// Octaves: multiply base cutoff by 2^offset, then clamp below Nyquist.
    pub filter_cutoff_offset: [f32; MAX_BLOCK_FRAMES],
    pub pitch_cents: [f32; MAX_BLOCK_FRAMES],
    pub pan: [f32; MAX_BLOCK_FRAMES],
    /// Added to the filter's resonance amount, then clamped to 0..1.
    pub resonance: [f32; MAX_BLOCK_FRAMES],
    /// Tremolo gain, 0 to 1. One when no volume LFO runs.
    pub volume: [f32; MAX_BLOCK_FRAMES],
    pub frames: usize,
}
impl Default for ModulationBlock {
    fn default() -> Self {
        Self {
            filter_cutoff_offset: [0.0; MAX_BLOCK_FRAMES],
            pitch_cents: [0.0; MAX_BLOCK_FRAMES],
            pan: [0.0; MAX_BLOCK_FRAMES],
            resonance: [0.0; MAX_BLOCK_FRAMES],
            volume: [1.0; MAX_BLOCK_FRAMES],
            frames: 0,
        }
    }
}
fn lfo_wave(shape: LfoShape, phase: f64) -> f32 {
    (match shape {
        LfoShape::Sine => (phase * std::f64::consts::TAU).sin(),
        LfoShape::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        LfoShape::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }) as f32
}
/// Tremolo gain for an LFO value and its depth: the gain swings between
/// `1 - |depth|` and 1, and a negative depth swaps which half of the wave
/// is the loud one.
fn tremolo(wave: f32, depth: f32) -> f32 {
    (1.0 - depth.abs() * (1.0 - wave * depth.signum()) * 0.5).clamp(0.0, 1.0)
}
impl PreparedChannelEnvelopes {
    pub fn prepare(settings: ChannelEnvelopes, rate: f64) -> Self {
        let settings = settings.sanitized();
        let rate = sample_rate(rate);
        let mut generators = [Adsr::default(); 4];
        for (generator, envelope) in generators.iter_mut().zip([
            settings.filter,
            settings.pitch,
            settings.pan,
            settings.resonance,
        ]) {
            generator.configure(
                envelope.attack_ms,
                envelope.decay_ms,
                envelope.sustain,
                envelope.release_ms,
                rate as f32,
            );
        }
        let lfos = settings.lfos;
        let delays = [
            lfos.volume,
            lfos.pan,
            lfos.pitch,
            lfos.cutoff,
            lfos.resonance,
        ]
        .map(|lfo| (f64::from(lfo.delay_ms) * 0.001 * rate).round() as u32);
        Self {
            settings,
            generators,
            phase: 0.0,
            phases: [0.0; 5],
            delays,
            since_gate: 0,
            fade: (rate * 0.005).round().max(1.0) as u32,
            rate,
        }
    }
    pub(crate) fn enabled(&self) -> bool {
        self.settings.filter.enabled
            || self.settings.pitch.enabled
            || self.settings.pan.enabled
            || self.settings.resonance.enabled
            || self.settings.lfo.enabled
            || self.settings.lfos.any_enabled()
            || self.settings.filter_settings.enabled
    }
    /// The channel filter these modulations move.
    pub(crate) fn filter(&self) -> VoiceFilter {
        self.settings.filter_settings
    }
    /// Call only on retrigger; mono legato preserves this state.
    pub fn gate_on(&mut self) {
        for envelope in &mut self.generators {
            envelope.gate_on();
        }
        self.since_gate = 0;
        let lfos = self.settings.lfos;
        for (phase, lfo) in self.phases.iter_mut().zip([
            lfos.volume,
            lfos.pan,
            lfos.pitch,
            lfos.cutoff,
            lfos.resonance,
        ]) {
            if lfo.retrigger {
                *phase = 0.0;
            }
        }
    }
    pub fn gate_off(&mut self) {
        for envelope in &mut self.generators {
            envelope.gate_off();
        }
    }
    pub fn reset(&mut self) {
        for envelope in &mut self.generators {
            envelope.reset();
        }
        self.phase = 0.0;
        self.phases = [0.0; 5];
        self.since_gate = 0;
    }
    pub fn process_block(&mut self, frames: usize, out: &mut ModulationBlock) {
        out.frames = frames.min(MAX_BLOCK_FRAMES);
        out.filter_cutoff_offset.fill(0.0);
        out.pitch_cents.fill(0.0);
        out.pan.fill(0.0);
        out.resonance.fill(0.0);
        out.volume.fill(1.0);
        let envelopes = [
            self.settings.filter,
            self.settings.pitch,
            self.settings.pan,
            self.settings.resonance,
        ];
        let lfo = self.settings.lfo;
        let lfos = self.settings.lfos;
        let targets = [
            lfos.volume,
            lfos.pan,
            lfos.pitch,
            lfos.cutoff,
            lfos.resonance,
        ];
        for frame in 0..out.frames {
            // Cutoff octaves, pitch cents, pan, resonance amount.
            let mut values = [0.0; 4];
            for (index, settings) in envelopes.iter().enumerate() {
                let level = self.generators[index].tick();
                if settings.enabled {
                    values[index] = level * settings.depth;
                }
            }
            let mut volume = 1.0;
            if lfo.enabled {
                let wave = lfo_wave(lfo.shape, self.phase);
                match lfo.target {
                    LfoTarget::Filter => values[0] += wave * lfo.depth * 8.0,
                    LfoTarget::Pitch => values[1] += wave * lfo.depth * 2400.0,
                    LfoTarget::Pan => values[2] += wave * lfo.depth,
                    LfoTarget::Resonance => values[3] += wave * lfo.depth,
                    LfoTarget::Volume => volume *= tremolo(wave, lfo.depth),
                }
            }
            self.phase = (self.phase + f64::from(lfo.rate_hz) / self.rate).rem_euclid(1.0);
            for (index, target) in targets.iter().enumerate() {
                // Out during its delay, then in over the fade. A retriggered
                // wave waits at the start of its cycle until then.
                let waiting = self.since_gate < self.delays[index];
                if target.enabled && !waiting {
                    let passed = self.since_gate - self.delays[index];
                    let depth = if self.delays[index] > 0 && passed < self.fade {
                        target.depth * passed as f32 / self.fade as f32
                    } else {
                        target.depth
                    };
                    let wave = lfo_wave(target.shape, self.phases[index]);
                    match index {
                        0 => volume *= tremolo(wave, depth),
                        1 => values[2] += wave * depth,
                        2 => values[1] += wave * depth * 2400.0,
                        3 => values[0] += wave * depth * 8.0,
                        _ => values[3] += wave * depth,
                    }
                }
                if !(waiting && target.retrigger) {
                    self.phases[index] = (self.phases[index]
                        + f64::from(target.rate_hz) / self.rate)
                        .rem_euclid(1.0);
                }
            }
            self.since_gate = self.since_gate.saturating_add(1);
            out.filter_cutoff_offset[frame] = values[0].clamp(-8.0, 8.0);
            out.pitch_cents[frame] = values[1].clamp(-2400.0, 2400.0);
            out.pan[frame] = values[2].clamp(-1.0, 1.0);
            out.resonance[frame] = values[3].clamp(-1.0, 1.0);
            out.volume[frame] = volume;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(key: u8) -> HeldNote {
        HeldNote { key, velocity: 1.0 }
    }

    #[test]
    fn arp_up_orders_low_to_high_inside_octave_range() {
        let mut arp = Arpeggiator::prepare(ArpeggiatorSettings {
            mode: ArpeggiatorMode::Up,
            range_octaves: 2,
            rate: NoteDivision::ThirtySecond,
            ..Default::default()
        });
        arp.set_chord(&[note(67), note(60), note(64)]);
        let mut events = NoteEvents::default();
        arp.process_block(80.0, 120.0, 36, &mut events);
        let keys: Vec<_> = events
            .as_slice()
            .iter()
            .filter(|e| e.kind == NoteEventKind::On)
            .map(|e| e.note.key)
            .collect();
        assert_eq!(keys, [60, 64, 67, 72, 76, 79, 60, 64]);
        assert!(keys.iter().all(|key| (60..=79).contains(key)));
    }

    #[test]
    fn arp_modes_and_gate_off_order() {
        for (mode, expected) in [
            (ArpeggiatorMode::Down, [67, 64, 60, 67]),
            (ArpeggiatorMode::UpDown, [60, 64, 67, 64]),
            (ArpeggiatorMode::AsPlayed, [64, 60, 67, 64]),
        ] {
            let mut arp = Arpeggiator::prepare(ArpeggiatorSettings {
                mode,
                gate: 1.0,
                rate: NoteDivision::ThirtySecond,
                ..Default::default()
            });
            arp.set_chord(&[note(64), note(60), note(67)]);
            let mut events = NoteEvents::default();
            arp.process_block(80.0, 120.0, 16, &mut events);
            let keys: Vec<_> = events
                .as_slice()
                .iter()
                .filter(|e| e.kind == NoteEventKind::On)
                .map(|e| e.note.key)
                .collect();
            assert_eq!(keys, expected);
            assert_eq!(events.as_slice()[1].kind, NoteEventKind::Off);
            assert_eq!(events.as_slice()[1].frame, events.as_slice()[2].frame);
            assert_eq!(events.as_slice()[0].id, events.as_slice()[1].id);
            arp.set_chord(&[]);
            arp.process_block(80.0, 120.0, 1, &mut events);
            assert_eq!(events.as_slice().len(), 1);
            assert_eq!(events.as_slice()[0].kind, NoteEventKind::Off);
        }
    }

    #[test]
    fn arp_fractional_timing_is_block_independent_and_midi_bounded() {
        let settings = ArpeggiatorSettings {
            mode: ArpeggiatorMode::Up,
            range_octaves: 4,
            gate: 0.4,
            ..Default::default()
        };
        let mut whole = Arpeggiator::prepare(settings);
        let mut split = Arpeggiator::prepare(settings);
        whole.set_chord(&[note(126)]);
        split.set_chord(&[note(126)]);
        let mut out = NoteEvents::default();
        whole.process_block(100.0, 137.0, 200, &mut out);
        let expected = out.as_slice().to_vec();
        let mut got = Vec::new();
        for base in (0..200).step_by(10) {
            split.process_block(100.0, 137.0, 10, &mut out);
            got.extend(out.as_slice().iter().map(|event| NoteEvent {
                frame: event.frame + base,
                ..*event
            }));
        }
        assert_eq!(expected, got);
        assert!(got.iter().all(|event| event.note.key == 126));
    }

    #[test]
    fn echo_requested_repeats_and_feedback_bound() {
        let mut echo = NoteEcho::prepare(NoteEchoSettings {
            enabled: true,
            repeats: 8,
            feedback: 1.0,
            time: EchoTime::Milliseconds { ms: 2.0 },
            pitch_semitones: 1,
        });
        assert_eq!(echo.settings.feedback, 0.95);
        assert!(echo.schedule_note(note(60), 0, 1, 1000.0, 120.0));
        let mut out = NoteEvents::default();
        echo.process_block(32, &mut out);
        let ons: Vec<_> = out
            .as_slice()
            .iter()
            .filter(|event| event.kind == NoteEventKind::On)
            .collect();
        assert_eq!(ons.len(), 8);
        assert_eq!(ons[7].note.key, 68);
        for (index, event) in ons.iter().enumerate() {
            assert!((event.note.velocity - 0.95_f32.powi(index as i32 + 1)).abs() < 1e-6);
            let off = out
                .as_slice()
                .iter()
                .find(|off| off.id == event.id && off.kind == NoteEventKind::Off)
                .unwrap();
            assert_eq!(off.frame, event.frame + 1);
        }
        assert_eq!(echo.pending_events(), 0);
        echo.process_block(32, &mut out);
        assert!(out.as_slice().is_empty());
        let mut quiet = NoteEcho::prepare(NoteEchoSettings {
            enabled: true,
            repeats: 8,
            feedback: 0.1,
            ..Default::default()
        });
        assert!(quiet.schedule_note(note(60), 0, 1, 1000.0, 120.0));
        assert_eq!(quiet.pending_events(), 6);
        let mut zero = NoteEcho::prepare(NoteEchoSettings {
            enabled: true,
            feedback: 0.0,
            ..Default::default()
        });
        assert!(zero.schedule_note(note(60), 0, 1, 1000.0, 120.0));
        assert_eq!(zero.pending_events(), 0);
    }

    #[test]
    fn echo_capacity_rejects_atomically_and_division_is_tempo_synced() {
        let mut echo = NoteEcho::prepare(NoteEchoSettings {
            enabled: true,
            repeats: 8,
            time: EchoTime::Division {
                division: NoteDivision::Quarter,
            },
            feedback: 0.95,
            ..Default::default()
        });
        for _ in 0..16 {
            assert!(echo.schedule_note(note(60), 0, 20, 100.0, 120.0));
        }
        assert!(!echo.schedule_note(note(60), 0, 20, 100.0, 120.0));
        assert_eq!(echo.pending_events(), ECHO_EVENT_CAPACITY);
        let mut out = NoteEvents::default();
        echo.process_block(51, &mut out);
        assert_eq!(out.as_slice().len(), 16);
        assert!(out.as_slice().iter().all(|event| event.frame == 50));
        echo.reset();
        assert_eq!(echo.pending_events(), 0);
    }

    #[test]
    fn polyphony_one_replaces_and_legato_preserves_envelopes() {
        let mut held = HeldNotes::default();
        let settings = PolyphonySettings {
            max_voices: 1,
            ..Default::default()
        };
        assert!(admit_note(&mut held, 60, settings).retrigger);
        assert_eq!(admit_note(&mut held, 64, settings).released_keys(), [60]);
        assert_eq!(held.as_slice(), [64]);
        let legato = admit_note(
            &mut held,
            67,
            PolyphonySettings {
                mono_legato: true,
                ..settings
            },
        );
        assert!(!legato.retrigger);
        assert_eq!(legato.released_keys(), [64]);
        held.note_off(67);
        assert!(held.as_slice().is_empty());
    }

    #[test]
    fn polyphony_limit_reduction_reports_all_victims_and_duplicate_retriggers() {
        let mut held = HeldNotes::default();
        for key in 60..64 {
            admit_note(&mut held, key, PolyphonySettings::default());
        }
        let decision = admit_note(
            &mut held,
            67,
            PolyphonySettings {
                max_voices: 1,
                ..Default::default()
            },
        );
        assert_eq!(decision.released_keys(), [60, 61, 62, 63]);
        assert_eq!(held.as_slice(), [67]);
        assert_eq!(
            admit_note(&mut held, 67, PolyphonySettings::default()).released_keys(),
            [67]
        );
        assert_eq!(held.as_slice(), [67]);
    }

    #[test]
    fn portamento_moves_toward_target() {
        let coefficient = PolyphonySettings {
            portamento_ms: 100.0,
            ..Default::default()
        }
        .glide_coefficient(1000.0);
        let mut pitch = 60.0;
        for _ in 0..100 {
            let before = pitch;
            pitch += (72.0 - pitch) * coefficient;
            assert!(pitch > before && pitch < 72.0);
        }
        assert!((pitch - (60.0 + 12.0 * (1.0 - (-1.0_f64).exp()))).abs() < 1e-9);
        assert_eq!(PolyphonySettings::default().glide_coefficient(1000.0), 1.0);
    }

    #[test]
    fn envelope_attack_rises_and_release_falls() {
        let settings = ChannelEnvelopes {
            pitch: ModulationEnvelope {
                enabled: true,
                attack_ms: 10.0,
                sustain: 1.0,
                release_ms: 10.0,
                depth: 100.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut envelope = PreparedChannelEnvelopes::prepare(settings, 1000.0);
        let mut out = ModulationBlock::default();
        envelope.gate_on();
        envelope.process_block(12, &mut out);
        assert!(
            out.pitch_cents[..10]
                .windows(2)
                .all(|pair| pair[1] >= pair[0])
        );
        assert!(out.pitch_cents[9] > 99.0);
        envelope.gate_off();
        envelope.process_block(20, &mut out);
        assert!(
            out.pitch_cents[..20]
                .windows(2)
                .all(|pair| pair[1] <= pair[0])
        );
        assert_eq!(out.pitch_cents[19], 0.0);
    }

    #[test]
    fn lfo_sine_changes_sign_over_period() {
        let settings = ChannelEnvelopes {
            lfo: ChannelLfoSettings {
                enabled: true,
                target: LfoTarget::Pan,
                rate_hz: 10.0,
                depth: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut envelopes = PreparedChannelEnvelopes::prepare(settings, 1000.0);
        let mut out = ModulationBlock::default();
        envelopes.process_block(100, &mut out);
        assert!(out.pan[25] > 0.99);
        assert!(out.pan[75] < -0.99);
        assert!(out.pan[50].abs() < 1e-5);
    }

    #[test]
    fn sanitizer_clamps_and_recovers_non_finite_values() {
        let mut settings = ChannelVoiceSettings::default();
        settings.arpeggiator.gate = 3.0;
        settings.arpeggiator.range_octaves = 0;
        settings.echo.feedback = 10.0;
        settings.echo.repeats = 255;
        settings.echo.pitch_semitones = -127;
        settings.echo.time = EchoTime::Milliseconds { ms: f32::NAN };
        settings.polyphony.max_voices = 0;
        settings.polyphony.portamento_ms = -1.0;
        settings.envelopes.pitch.sustain = f32::INFINITY;
        settings.envelopes.pan.depth = 4.0;
        settings.envelopes.lfo.rate_hz = 100.0;
        let settings = settings.sanitized();
        assert_eq!(settings.arpeggiator.gate, 1.0);
        assert_eq!(settings.arpeggiator.range_octaves, 1);
        assert_eq!(settings.echo.feedback, 0.95);
        assert_eq!(settings.echo.repeats, 8);
        assert_eq!(settings.echo.pitch_semitones, -48);
        assert_eq!(settings.echo.time, EchoTime::Milliseconds { ms: 250.0 });
        assert_eq!(settings.polyphony.max_voices, 1);
        assert_eq!(settings.polyphony.portamento_ms, 0.0);
        assert_eq!(settings.envelopes.pitch.sustain, 0.8);
        assert_eq!(settings.envelopes.pan.depth, 1.0);
        assert_eq!(settings.envelopes.lfo.rate_hz, 30.0);
    }

    #[test]
    fn processing_is_bounded_and_defaults_are_neutral() {
        let mut arp = Arpeggiator::prepare(ArpeggiatorSettings {
            mode: ArpeggiatorMode::Up,
            gate: 1.0,
            ..Default::default()
        });
        arp.set_chord(&[note(60)]);
        let mut events = NoteEvents::default();
        arp.process_block(1.0, 999.0, 10_000, &mut events);
        assert_eq!(events.processed_frames, MAX_BLOCK_FRAMES);
        assert_eq!(events.as_slice().len(), 511);
        let mut envelopes =
            PreparedChannelEnvelopes::prepare(ChannelEnvelopes::default(), 48_000.0);
        let mut out = ModulationBlock::default();
        envelopes.gate_on();
        envelopes.process_block(256, &mut out);
        assert!(
            out.filter_cutoff_offset
                .iter()
                .chain(&out.pitch_cents)
                .chain(&out.pan)
                .all(|value| *value == 0.0)
        );
    }

    #[test]
    fn omitted_settings_deserialize_to_defaults() {
        let empty = serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
            std::iter::empty::<(String, i32)>(),
        );
        let settings = ChannelVoiceSettings::deserialize(empty).unwrap();
        assert_eq!(settings, ChannelVoiceSettings::default());
    }

    /// Pitch-LFO cents for `frames` frames after the state `envelopes` is in.
    fn pitch_lfo(envelopes: &mut PreparedChannelEnvelopes, frames: usize) -> Vec<f32> {
        let mut out = ModulationBlock::default();
        let mut cents = Vec::with_capacity(frames);
        while cents.len() < frames {
            let block = (frames - cents.len()).min(MAX_BLOCK_FRAMES);
            envelopes.process_block(block, &mut out);
            cents.extend_from_slice(&out.pitch_cents[..block]);
        }
        cents
    }

    #[test]
    fn target_lfo_delay_holds_the_wave_out_then_fades_it_in() {
        // 5 Hz at half depth is 1200 cents of swing; 100 ms is 4800 frames
        // at 48 kHz and the fade is 240 frames.
        let lfo = |retrigger| TargetLfo {
            enabled: true,
            shape: LfoShape::Sine,
            rate_hz: 5.0,
            depth: 0.5,
            delay_ms: 100.0,
            retrigger,
        };
        let wave = |frame: f64| (std::f64::consts::TAU * 5.0 * frame / 48_000.0).sin() * 1200.0;
        for retrigger in [true, false] {
            let mut settings = ChannelEnvelopes::default();
            settings.lfos.pitch = lfo(retrigger);
            let mut envelopes = PreparedChannelEnvelopes::prepare(settings, 48_000.0);
            for gate in 0..2_usize {
                envelopes.gate_on();
                let cents = pitch_lfo(&mut envelopes, 12_000);
                let mut worst = 0.0_f64;
                for (n, got) in cents.iter().enumerate() {
                    let want = if n < 4800 {
                        0.0
                    } else {
                        let fade = ((n - 4800) as f64 / 240.0).min(1.0);
                        // Retriggered, the wave starts when the delay ends;
                        // left running, it has been turning since the first
                        // frame of the first note.
                        let frame = if retrigger {
                            (n - 4800) as f64
                        } else {
                            (n + gate * 12_000) as f64
                        };
                        wave(frame) * fade
                    };
                    worst = worst.max((f64::from(*got) - want).abs());
                }
                println!("LFO delay retrigger={retrigger} gate={gate} worst={worst:.4e} cents");
                assert!(worst < 0.01, "retrigger {retrigger} gate {gate}: {worst}");
                assert!(cents[..4800].iter().all(|value| *value == 0.0));
                assert!(cents.iter().any(|value| value.abs() > 1000.0));
            }
        }
        // No delay is the LFO as it was: full depth from the first frame.
        let mut settings = ChannelEnvelopes::default();
        settings.lfos.pitch = TargetLfo {
            delay_ms: 0.0,
            ..lfo(false)
        };
        let mut envelopes = PreparedChannelEnvelopes::prepare(settings, 48_000.0);
        envelopes.gate_on();
        for (n, got) in pitch_lfo(&mut envelopes, 4800).iter().enumerate() {
            assert!((f64::from(*got) - wave(n as f64)).abs() < 0.01, "frame {n}");
        }
        let old = lfo(false);
        let hostile = TargetLfo {
            delay_ms: f32::NAN,
            ..old
        };
        assert_eq!(hostile.sanitized().delay_ms, 0.0);
        assert_eq!(
            TargetLfo {
                delay_ms: 1.0e9,
                ..old
            }
            .sanitized()
            .delay_ms,
            TargetLfo::MAX_DELAY_MS
        );
    }
}
