//! Plugins in the shape the engine hosts: the same operations
//! `windfall_dsp::AnyEffect` and `AnyInstrument` have.
//!
//! The engine prepares a built-in processor off the audio thread, sets its
//! parameters by position in a descriptor list, tells it the tempo, and
//! calls `process(left, right)`. A [`PluginEffect`] or [`PluginInstrument`]
//! does all of that for a third-party plugin, so an effect slot or a
//! channel can hold one with the machinery the engine has.
//!
//! One step differs. A built-in processor prepares itself. A plugin is
//! prepared by its [`PluginInstance`], on the main thread, because that is
//! where plugins allocate: [`PluginInstance::prepare_effect`] and
//! [`PluginInstance::prepare_instrument`] stand in for `prepare`. To run at
//! another sample rate, send the adapter back, [`release`] it, and prepare
//! again.
//!
//! [`release`]: PluginInstance::release_effect

use crate::containment::PluginHealth;
use crate::error::PluginError;
use crate::events::{HostEvent, MAX_NOTE_INSTANCES, NoteExpressionKind, Transport};
use crate::instance::PluginInstance;
use crate::params::PluginParam;
use crate::processor::{PluginProcessor, ProcessStatus};

/// What the audio thread needs to know of one parameter to set it by its
/// position in the list.
#[derive(Debug, Clone, Copy)]
struct ParamSlot {
    id: u32,
    min: f64,
    max: f64,
    stepped: bool,
    read_only: bool,
}

impl ParamSlot {
    fn of(param: &PluginParam) -> Self {
        Self {
            id: param.id,
            min: param.min.min(param.max),
            max: param.max.max(param.min),
            stepped: param.stepped,
            read_only: param.read_only,
        }
    }

    fn clamp(&self, value: f32) -> Option<f64> {
        if self.read_only || !value.is_finite() {
            return None;
        }
        let value = f64::from(value).clamp(self.min, self.max);
        Some(if self.stepped { value.round() } else { value })
    }
}

fn slots(instance: &PluginInstance) -> Box<[ParamSlot]> {
    instance.params().iter().map(ParamSlot::of).collect()
}

/// The longest tail the adapters report for a plugin that says its tail
/// never ends: one minute at the plugin's sample rate. The engine decides
/// when a track is done by listening, and uses the tail only as a bound.
const ENDLESS_TAIL_SECONDS: f64 = 60.0;

fn tail_of(processor: &PluginProcessor) -> usize {
    let latency = processor.latency_samples() as usize;
    match processor.tail_samples() {
        Some(tail) => tail as usize + latency,
        None => (processor.sample_rate() * ENDLESS_TAIL_SECONDS) as usize + latency,
    }
}

/// A plugin as a stereo effect that processes in place.
///
/// Everything here is for the audio thread and never allocates, locks or
/// blocks in the host's own code.
pub struct PluginEffect {
    processor: PluginProcessor,
    params: Box<[ParamSlot]>,
}

impl PluginEffect {
    pub fn copy_output(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        self.processor.copy_output(bus, output)
    }
    pub fn controls_processed(&self) -> bool {
        self.processor.controls_processed()
    }
    /// Clears the plugin's memory of past audio.
    pub fn reset(&mut self) {
        self.processor.reset();
    }

    /// Sets the parameter at `index` of the list
    /// [`PluginInstance::params`] returned when the plugin was prepared, in
    /// the plugin's own units. The value is forced into range, and the
    /// change takes effect at the start of the next block. Returns false
    /// past the end of the list and for a parameter that cannot be set.
    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(slot) = self.params.get(index) else {
            return false;
        };
        match slot.clamp(value) {
            Some(value) => self.processor.set_param(0, slot.id, value),
            None => false,
        }
    }

    /// Tells the plugin the tempo, with the transport information of the
    /// next block.
    pub fn set_tempo(&mut self, bpm: f32) {
        self.processor.set_tempo(f64::from(bpm));
    }

    /// Tells the plugin where the song is. The engine's built-in effects
    /// have no use for this. Plugins that sync to the beat do.
    pub fn set_transport(&mut self, transport: Transport) {
        self.processor.set_transport(transport);
    }

    /// Processes one block in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.processor.process(left, right);
    }
    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        self.processor.process_sidechain(left, right, key);
    }

    /// Samples by which the output lags the input.
    pub fn latency_samples(&self) -> usize {
        self.processor.latency_samples() as usize
    }

    /// Samples for which the plugin can keep sounding after its input has
    /// gone silent, including the latency.
    pub fn tail_samples(&self) -> usize {
        tail_of(&self.processor)
    }

    /// Samples for which the output can fall silent and still come back
    /// with nothing new going in. A plugin does not say, so this is its
    /// whole tail.
    pub fn gap_samples(&self) -> usize {
        self.tail_samples()
    }

    /// How the plugin has behaved since it was prepared.
    pub fn health(&self) -> PluginHealth {
        self.processor.health()
    }

    /// The processor underneath, for what the engine's own effects do not
    /// have: events at a frame of the block, the plugin's status.
    pub fn processor(&mut self) -> &mut PluginProcessor {
        &mut self.processor
    }
}

/// Channel/key pairs tracked without callback allocation.
const KEYS: usize = 16 * 128;

/// A plugin as a stereo instrument.
///
/// Like the built-in instruments it takes notes with no position in time:
/// a note starts on the first frame of the next block. The engine already
/// splits its blocks at every note, which puts each on its exact frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginNoteControls {
    pub pan: f32,
    /// Linear per-note level; used to preserve the project pan law.
    pub volume: f32,
    pub pitch: f32,
    pub modulation_x: f32,
    pub modulation_y: f32,
}
impl PluginNoteControls {
    fn values(self, key: u8) -> [f64; 5] {
        [
            (f64::from(self.pan) + 1.0) / 2.0,
            f64::from(self.pitch) - f64::from(key),
            f64::from(self.modulation_x),
            f64::from(self.modulation_y),
            f64::from(self.volume),
        ]
    }
    pub(crate) fn valid(self, key: u8) -> bool {
        let values = self.values(key);
        [
            NoteExpressionKind::Pan,
            NoteExpressionKind::Tuning,
            NoteExpressionKind::Brightness,
            NoteExpressionKind::Expression,
            NoteExpressionKind::Volume,
        ]
        .into_iter()
        .zip(values)
        .all(|(kind, value)| kind.valid(value))
    }
    pub fn mapped(self, key: u8) -> PluginNoteExpression {
        let [pan, tuning, brightness, expression, volume] = self.values(key);
        PluginNoteExpression {
            values: [volume, pan, tuning, 0.5, expression, brightness, 0.5],
            mask: 0b0110111,
        }
    }
}
/// Standard expressions, ordered like `NoteExpressionKind`. Prepared masks are
/// consulted without calling plugin discovery from the audio callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginNoteExpression {
    pub values: [f64; 7],
    pub mask: u32,
}
const EXPRESSION_KINDS: [NoteExpressionKind; 7] = [
    NoteExpressionKind::Volume,
    NoteExpressionKind::Pan,
    NoteExpressionKind::Tuning,
    NoteExpressionKind::Vibrato,
    NoteExpressionKind::Expression,
    NoteExpressionKind::Brightness,
    NoteExpressionKind::Pressure,
];
impl PluginNoteExpression {
    pub(crate) fn valid(self) -> bool {
        self.mask & !0x7f == 0
            && EXPRESSION_KINDS
                .into_iter()
                .enumerate()
                .all(|(index, kind)| self.mask & kind.mask() == 0 || kind.valid(self.values[index]))
    }
}
#[derive(Clone, Copy)]
struct HeldInstance {
    id: u32,
    key: u8,
    channel: u8,
    controls: PluginNoteExpression,
}
impl HeldInstance {
    const EMPTY: Self = Self {
        id: 0,
        key: 0,
        channel: 0,
        controls: PluginNoteExpression {
            values: [1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5],
            mask: 0,
        },
    };
}

pub struct PluginInstrument {
    processor: PluginProcessor,
    params: Box<[ParamSlot]>,
    /// The keys that are down.
    held: [bool; KEYS],
    held_count: usize,
    instances: Box<[HeldInstance]>,
    bend_range_semitones: f64,
}

impl PluginInstrument {
    pub fn copy_output(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        self.processor.copy_output(bus, output)
    }
    pub fn controls_processed(&self) -> bool {
        self.processor.controls_processed()
    }
    pub fn midi_message(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > 3 || bytes[1..].iter().any(|byte| *byte > 127) {
            return false;
        }
        let kind = bytes[0] & 0xf0;
        let channel = bytes[0] & 15;
        if bytes.len() == 3 && kind == 0x90 && bytes[2] != 0 {
            return self.note_on_channel(bytes[1], channel, f32::from(bytes[2]) / 127.0);
        }
        if bytes.len() == 3 && (kind == 0x80 || kind == 0x90) {
            return self.note_off_channel(bytes[1], channel);
        }
        let mut stored = [0; 3];
        stored[..bytes.len()].copy_from_slice(bytes);
        self.processor.push_event(HostEvent::Midi {
            time: 0,
            bytes: stored,
            len: bytes.len() as u8,
        })
    }

    /// Silences every voice at once and clears the plugin's memory.
    pub fn reset(&mut self) {
        self.processor.reset();
        self.held = [false; KEYS];
        self.held_count = 0;
        self.instances.fill(HeldInstance::EMPTY);
    }

    /// See [`PluginEffect::set_param`].
    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(slot) = self.params.get(index) else {
            return false;
        };
        match slot.clamp(value) {
            Some(value) => self.processor.set_param(0, slot.id, value),
            None => false,
        }
    }

    /// See [`PluginEffect::set_tempo`].
    pub fn set_tempo(&mut self, bpm: f32) {
        self.processor.set_tempo(f64::from(bpm));
    }

    /// See [`PluginEffect::set_transport`].
    pub fn set_transport(&mut self, transport: Transport) {
        self.processor.set_transport(transport);
    }

    /// Starts a note on the first frame of the next block. `key` is a MIDI
    /// key number and `velocity` runs from 0 to 1. Zero or less is taken as
    /// a note-off.
    pub fn note_on(&mut self, key: u8, velocity: f32) -> bool {
        self.note_on_channel(key, 0, velocity)
    }

    pub fn note_on_channel(&mut self, key: u8, channel: u8, velocity: f32) -> bool {
        if channel > 15 {
            return false;
        }
        if velocity <= 0.0 {
            return self.note_off_channel(key, channel);
        }
        let key = key.min(127);
        if !self.processor.note_on_channel(0, key, channel, velocity) {
            return false;
        }
        let slot = usize::from(channel) * 128 + usize::from(key);
        if !self.held[slot] {
            self.held[slot] = true;
            self.held_count += 1;
        }
        true
    }

    pub fn native_note_capabilities(&self) -> (bool, [u32; 16]) {
        (
            self.processor.supports_note_instances(),
            std::array::from_fn(|channel| self.processor.note_expression_mask(channel as u8)),
        )
    }
    pub fn supports_note_instances(&self) -> bool {
        self.processor.supports_note_instances() || self.processor.supports_midi_expression()
    }
    pub fn supports_note_pitch(&self) -> bool {
        self.processor.supports_midi_expression()
            || (0..16).any(|channel| {
                self.processor.note_expression_mask(channel) & NoteExpressionKind::Tuning.mask()
                    != 0
            })
    }
    pub fn set_bend_range_semitones(&mut self, range: f32) {
        if range.is_finite() {
            self.bend_range_semitones = f64::from(range.clamp(1.0, 96.0));
        }
    }
    fn controls_events(
        &self,
        note: HeldInstance,
        before: Option<PluginNoteExpression>,
        controls: PluginNoteExpression,
        events: &mut [HostEvent; 8],
        mut count: usize,
    ) -> usize {
        let mut values = controls.values;
        let driven = controls.mask | before.map_or(0, |before| before.mask);
        let neutral = [1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5];
        for (index, kind) in EXPRESSION_KINDS.into_iter().enumerate() {
            if controls.mask & kind.mask() == 0 {
                values[index] = neutral[index];
            }
        }
        let midi = self.processor.supports_midi_expression();
        let mask = if midi {
            self.processor.midi_expression_mask(note.channel)
        } else {
            self.processor.note_expression_mask(note.channel)
        };
        if midi {
            values[2] = (values[2] / self.bend_range_semitones).clamp(-1.0, 1.0);
        }
        for (index, kind) in EXPRESSION_KINDS.into_iter().enumerate() {
            if mask & driven & kind.mask() != 0
                && (midi
                    || before.is_none_or(|before| {
                        before.mask & kind.mask() == 0 || before.values[index] != values[index]
                    }))
            {
                events[count] = HostEvent::NoteExpression {
                    time: 0,
                    id: note.id,
                    key: note.key,
                    channel: note.channel,
                    kind,
                    value: values[index],
                };
                count += 1;
            }
        }
        count
    }
    pub fn note_on_instance(
        &mut self,
        id: u32,
        key: u8,
        channel: u8,
        velocity: f32,
        controls: PluginNoteControls,
    ) -> bool {
        if !controls.valid(key) {
            return false;
        }
        self.note_on_expression_instance(id, key, channel, velocity, controls.mapped(key))
    }
    pub fn note_on_expression_instance(
        &mut self,
        id: u32,
        key: u8,
        channel: u8,
        velocity: f32,
        controls: PluginNoteExpression,
    ) -> bool {
        if !self.supports_note_instances()
            || key > 127
            || channel > 15
            || !controls.valid()
            || self.instances.iter().any(|note| note.id == id)
        {
            return false;
        }
        let Some(slot) = self.instances.iter().position(|note| note.id == 0) else {
            return false;
        };
        let note = HeldInstance {
            id,
            key,
            channel,
            controls,
        };
        let mut events = [HostEvent::AllNotesOff { time: 0 }; 8];
        events[0] = HostEvent::NoteOnInstance {
            time: 0,
            id,
            key,
            channel,
            velocity,
        };
        let count = self.controls_events(note, None, controls, &mut events, 1);
        if self.processor.supports_midi_expression() {
            // Bend precedes onset, including a fractional note's first sample.
            events[0] = HostEvent::NoteOn {
                time: 0,
                key,
                channel,
                velocity,
            };
            events[..count].rotate_left(1);
        }
        if !self.processor.push_events(&events[..count]) {
            return false;
        }
        self.instances[slot] = note;
        true
    }
    pub fn set_note_controls(&mut self, id: u32, controls: PluginNoteControls) -> bool {
        let Some(note) = self.instances.iter().find(|note| note.id == id && id != 0) else {
            return false;
        };
        if !controls.valid(note.key) {
            return false;
        }
        self.set_note_expression(id, controls.mapped(note.key))
    }
    pub fn set_note_expression(&mut self, id: u32, controls: PluginNoteExpression) -> bool {
        let Some(slot) = self
            .instances
            .iter()
            .position(|note| note.id == id && id != 0)
        else {
            return false;
        };
        let note = self.instances[slot];
        if !controls.valid() {
            return false;
        }
        let mut events = [HostEvent::AllNotesOff { time: 0 }; 8];
        let count = self.controls_events(note, Some(note.controls), controls, &mut events, 0);
        if !self.processor.push_events(&events[..count]) {
            return false;
        }
        self.instances[slot].controls = controls;
        true
    }
    pub fn note_off_instance(&mut self, id: u32, velocity: f32) -> bool {
        let Some(slot) = self
            .instances
            .iter()
            .position(|note| note.id == id && id != 0)
        else {
            return true;
        };
        let note = self.instances[slot];
        if self.processor.supports_midi_expression() {
            self.instances[slot] = HeldInstance::EMPTY;
            if !self.instances.iter().any(|other| {
                other.id != 0 && other.channel == note.channel && other.key == note.key
            }) {
                self.processor
                    .release_note_on_channel(note.key, note.channel);
            }
            // Restore another owner on the shared MIDI channel, or neutralize
            // controls after its final release. The caller retries saturation.
            let other = self
                .instances
                .iter()
                .filter(|other| other.id != 0 && other.channel == note.channel)
                .max_by_key(|other| other.id)
                .copied();
            let controls = other.map_or(HeldInstance::EMPTY.controls, |other| other.controls);
            let mut events = [HostEvent::AllNotesOff { time: 0 }; 8];
            let count = self.controls_events(
                other.unwrap_or(note),
                Some(note.controls),
                controls,
                &mut events,
                0,
            );
            return self.processor.push_events(&events[..count]);
        }
        if !self
            .processor
            .release_note_instance(id, note.key, note.channel, velocity)
        {
            return false;
        }
        self.instances[slot] = HeldInstance::EMPTY;
        true
    }

    /// Lets go of a note on the first frame of the next block.
    pub fn note_off(&mut self, key: u8) -> bool {
        self.note_off_channel(key, 0)
    }

    pub fn note_off_channel(&mut self, key: u8, channel: u8) -> bool {
        if channel > 15 {
            return false;
        }
        let key = key.min(127);
        // Reserved immediate releases remain guaranteed under saturation.
        if channel == 0 {
            self.processor.release_note(key);
        } else {
            self.processor.release_note_on_channel(key, channel);
        }
        let slot = usize::from(channel) * 128 + usize::from(key);
        if self.held[slot] {
            self.held[slot] = false;
            self.held_count -= 1;
        }
        true
    }

    /// Stops every note without its release, as when the transport stops.
    pub fn all_notes_off(&mut self) -> bool {
        self.processor.release_all_notes();
        self.held = [false; KEYS];
        self.held_count = 0;
        self.instances.fill(HeldInstance::EMPTY);
        true
    }

    /// Writes the next block of output, replacing whatever the two slices
    /// held.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // A plugin with an audio input, such as a vocoder, would otherwise
        // be fed whatever the buffers held.
        if self.processor.has_audio_input() {
            left.fill(0.0);
            right.fill(0.0);
        }
        self.processor.process(left, right);
    }

    /// Notes that are held down. A plugin does not say how many voices are
    /// sounding, so a note in its release is not counted. The engine finds
    /// out when an instrument has gone quiet by listening to it, bounded by
    /// [`tail_samples`](Self::tail_samples).
    pub fn active_voices(&self) -> usize {
        self.held_count + self.instances.iter().filter(|note| note.id != 0).count()
    }

    /// True once the plugin has said that nothing more will come out until
    /// the next note.
    pub fn is_asleep(&self) -> bool {
        self.active_voices() == 0 && self.processor.status() == ProcessStatus::Sleep
    }

    /// Samples by which the output lags the notes.
    pub fn latency_samples(&self) -> usize {
        self.processor.latency_samples() as usize
    }

    /// Samples for which output can continue after the last note was let
    /// go, including the latency.
    pub fn tail_samples(&self) -> usize {
        tail_of(&self.processor)
    }

    /// See [`PluginEffect::health`].
    pub fn health(&self) -> PluginHealth {
        self.processor.health()
    }

    /// See [`PluginEffect::processor`].
    pub fn processor(&mut self) -> &mut PluginProcessor {
        &mut self.processor
    }
}

impl PluginInstance {
    /// Activates the plugin at `sample_rate` for blocks of up to
    /// `max_block` frames and returns it as an effect. This is the plugin's
    /// `prepare`: it allocates, so it belongs on the main thread.
    pub fn prepare_effect(
        &mut self,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<PluginEffect, PluginError> {
        self.prepare_effect_with_options(sample_rate, max_block, &Default::default())
    }

    pub fn prepare_effect_with_options(
        &mut self,
        sample_rate: f32,
        max_block: usize,
        options: &crate::wrapper::WrapperOptions,
    ) -> Result<PluginEffect, PluginError> {
        let params = slots(self);
        let processor = self.activate_with_options(f64::from(sample_rate), max_block, options)?;
        Ok(PluginEffect { processor, params })
    }

    /// Activates the plugin and returns it as an instrument. See
    /// [`prepare_effect`](Self::prepare_effect).
    pub fn prepare_instrument(
        &mut self,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<PluginInstrument, PluginError> {
        self.prepare_instrument_with_options(sample_rate, max_block, &Default::default())
    }

    pub fn prepare_instrument_with_options(
        &mut self,
        sample_rate: f32,
        max_block: usize,
        options: &crate::wrapper::WrapperOptions,
    ) -> Result<PluginInstrument, PluginError> {
        let params = slots(self);
        let processor = self.activate_with_options(f64::from(sample_rate), max_block, options)?;
        Ok(PluginInstrument {
            processor,
            params,
            held: [false; KEYS],
            held_count: 0,
            instances: vec![HeldInstance::EMPTY; MAX_NOTE_INSTANCES].into_boxed_slice(),
            bend_range_semitones: 2.0,
        })
    }

    /// Takes an effect back from the audio thread and deactivates the
    /// plugin.
    // Preserve the exact adapter on refusal, without boxing its ownership.
    #[allow(clippy::result_large_err)]
    pub fn release_effect(
        &mut self,
        effect: PluginEffect,
    ) -> Result<(), crate::DeactivationError<PluginEffect>> {
        let PluginEffect { processor, params } = effect;
        self.deactivate(processor)
            .map_err(|error| crate::DeactivationError {
                error: error.error,
                returned: PluginEffect {
                    processor: error.returned,
                    params,
                },
            })
    }

    /// Takes an instrument back from the audio thread and deactivates the
    /// plugin.
    #[allow(clippy::result_large_err)]
    pub fn release_instrument(
        &mut self,
        instrument: PluginInstrument,
    ) -> Result<(), crate::DeactivationError<PluginInstrument>> {
        let PluginInstrument {
            processor,
            params,
            held,
            held_count,
            instances,
            bend_range_semitones,
        } = instrument;
        self.deactivate(processor)
            .map_err(|error| crate::DeactivationError {
                error: error.error,
                returned: PluginInstrument {
                    processor: error.returned,
                    params,
                    held,
                    held_count,
                    instances,
                    bend_range_semitones,
                },
            })
    }
}
