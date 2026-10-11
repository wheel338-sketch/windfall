//! Turning the host's events into CLAP's and back.
//!
//! The lists here have a fixed size. Clack offers growing ones, and a
//! growing list is an allocation on the audio thread the first time a block
//! carries more events than any block before it.

use clack_host::events::event_types::{
    MidiEvent, NoteChokeEvent, NoteExpressionEvent, NoteExpressionType, NoteOffEvent, NoteOnEvent,
    ParamValueEvent,
};
use clack_host::events::io::{InputEventBuffer, OutputEventBuffer, TryPushError};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{Event, Match, Pckn, UnknownEvent};
use clack_host::utils::ClapId;

use crate::events::{HostEvent, PluginEvent};
use crate::processor::{EVENT_CAPACITY, IMMEDIATE_RELEASE_CAPACITY};

/// The language a plugin's note port understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// CLAP's own note events.
    Clap,
    /// Three-byte MIDI messages.
    Midi,
    /// The plugin has no note input, so notes are dropped.
    None,
}

/// One CLAP event, whichever kind.
enum Slot {
    NoteOn(NoteOnEvent),
    NoteOff(NoteOffEvent),
    Choke(NoteChokeEvent),
    Param(ParamValueEvent),
    Midi(MidiEvent),
    Expression(NoteExpressionEvent),
}

impl Slot {
    fn as_unknown(&self) -> &UnknownEvent {
        match self {
            Self::NoteOn(event) => event.as_unknown(),
            Self::NoteOff(event) => event.as_unknown(),
            Self::Choke(event) => event.as_unknown(),
            Self::Param(event) => event.as_unknown(),
            Self::Midi(event) => event.as_unknown(),
            Self::Expression(event) => event.as_unknown(),
        }
    }
}

/// Panic also neutralizes bend and controllers on each MIDI channel.
const ALL_NOTES_OFF_MESSAGES: usize = 16 * 9;

/// The CLAP events of one block, in a list that never grows.
pub(crate) struct EventList {
    slots: Vec<Slot>,
    dropped: u32,
}

fn midi_velocity(velocity: f32) -> u8 {
    (velocity.clamp(0.0, 1.0) * 127.0).round() as u8
}

impl EventList {
    /// Allocates room for every event a block can carry.
    pub fn new(dialect: Dialect) -> Self {
        let capacity = match dialect {
            Dialect::None => EVENT_CAPACITY,
            Dialect::Clap => EVENT_CAPACITY + IMMEDIATE_RELEASE_CAPACITY,
            // Every ordinary event could be a panic. The adapter reserve
            // adds at most 2048 channel/key note-offs and one further panic.
            // Translation must not silently drop an admitted release.
            Dialect::Midi => {
                EVENT_CAPACITY * ALL_NOTES_OFF_MESSAGES + IMMEDIATE_RELEASE_CAPACITY - 1
                    + ALL_NOTES_OFF_MESSAGES
            }
        };
        Self {
            slots: Vec::with_capacity(capacity),
            dropped: 0,
        }
    }

    fn push(&mut self, slot: Slot) {
        if self.slots.len() < self.slots.capacity() {
            self.slots.push(slot);
        } else {
            self.dropped += 1;
        }
    }
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Replaces the list with `events`, in the plugin's dialect.
    pub fn fill(&mut self, events: &[HostEvent], dialect: Dialect) {
        self.slots.clear();
        self.dropped = 0;
        for event in events {
            match (*event, dialect) {
                (HostEvent::Param { time, id, value }, _) => {
                    if let Some(id) = ClapId::from_raw(id) {
                        let event = ParamValueEvent::new(time, id, Pckn::match_all(), value);
                        self.push(Slot::Param(event));
                    }
                }
                (_, Dialect::None) => {}
                (
                    HostEvent::NoteOnInstance {
                        time,
                        id,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Clap,
                ) => {
                    let target = Pckn::new(0_u16, u16::from(channel), u16::from(key), id);
                    self.push(Slot::NoteOn(NoteOnEvent::new(
                        time,
                        target,
                        f64::from(velocity),
                    )));
                }
                (
                    HostEvent::NoteOffInstance {
                        time,
                        id,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Clap,
                ) => {
                    let target = Pckn::new(0_u16, u16::from(channel), u16::from(key), id);
                    self.push(Slot::NoteOff(NoteOffEvent::new(
                        time,
                        target,
                        f64::from(velocity),
                    )));
                }
                (
                    HostEvent::NoteExpression {
                        time,
                        id,
                        key,
                        channel,
                        kind,
                        value,
                    },
                    Dialect::Clap,
                ) => {
                    use crate::events::NoteExpressionKind as Kind;
                    let expression = match kind {
                        Kind::Volume => NoteExpressionType::Volume,
                        Kind::Pan => NoteExpressionType::Pan,
                        Kind::Tuning => NoteExpressionType::Tuning,
                        Kind::Vibrato => NoteExpressionType::Vibrato,
                        Kind::Expression => NoteExpressionType::Expression,
                        Kind::Brightness => NoteExpressionType::Brightness,
                        Kind::Pressure => NoteExpressionType::Pressure,
                    };
                    let target = Pckn::new(0_u16, u16::from(channel), u16::from(key), id);
                    self.push(Slot::Expression(NoteExpressionEvent::new(
                        time, target, expression, value,
                    )));
                }
                (HostEvent::NoteExpression { time, channel, kind, value, .. }, Dialect::Midi) => {
                    use crate::events::NoteExpressionKind as Kind;
                    let channel = channel & 0x0f;
                    let unit = |value: f64| (value.clamp(0.0, 1.0) * 127.0).round() as u8;
                    // Adapter scales relative semitones by the declared channel
                    // bend range. MIDI shares one expression state per channel.
                    let data = match kind {
                        Kind::Tuning => {
                            let bend = (8192.0 + value.clamp(-1.0, 1.0) * 8192.0).round().clamp(0.0, 16383.0) as u16;
                            [0xe0 | channel, (bend & 127) as u8, (bend >> 7) as u8]
                        }
                        Kind::Pressure => [0xd0 | channel, unit(value), 0],
                        Kind::Volume => [0xb0 | channel, 7, unit(value)],
                        Kind::Pan => [0xb0 | channel, 10, unit(value)],
                        Kind::Vibrato => [0xb0 | channel, 1, unit(value)],
                        Kind::Expression => [0xb0 | channel, 11, unit(value)],
                        Kind::Brightness => [0xb0 | channel, 74, unit(value)],
                    };
                    self.push(Slot::Midi(MidiEvent::new(time, 0, data)));
                }
                (
                    HostEvent::NoteOnInstance { .. } | HostEvent::NoteOffInstance { .. },
                    Dialect::Midi,
                ) => {
                    self.dropped = self.dropped.saturating_add(1);
                }
                (
                    HostEvent::NoteOn {
                        time,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Clap,
                ) => {
                    let note = Pckn::new(0_u16, u16::from(channel), u16::from(key), Match::All);
                    let event = NoteOnEvent::new(time, note, f64::from(velocity));
                    self.push(Slot::NoteOn(event));
                }
                (
                    HostEvent::NoteOff {
                        time,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Clap,
                ) => {
                    let note = Pckn::new(0_u16, u16::from(channel), u16::from(key), Match::All);
                    let event = NoteOffEvent::new(time, note, f64::from(velocity));
                    self.push(Slot::NoteOff(event));
                }
                (HostEvent::AllNotesOff { time }, Dialect::Clap) => {
                    let everything = Pckn::new(0_u16, Match::All, Match::All, Match::All);
                    self.push(Slot::Choke(NoteChokeEvent::new(time, everything)));
                }
                (
                    HostEvent::NoteOn {
                        time,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Midi,
                ) => {
                    // A MIDI note-on with no velocity means note-off.
                    let data = [0x90 | (channel & 0x0f), key, midi_velocity(velocity).max(1)];
                    self.push(Slot::Midi(MidiEvent::new(time, 0, data)));
                }
                (
                    HostEvent::NoteOff {
                        time,
                        key,
                        channel,
                        velocity,
                    },
                    Dialect::Midi,
                ) => {
                    let data = [0x80 | (channel & 0x0f), key, midi_velocity(velocity)];
                    self.push(Slot::Midi(MidiEvent::new(time, 0, data)));
                }
                (HostEvent::AllNotesOff { time }, Dialect::Midi) => {
                    for channel in 0..16_u8 {
                        // All sound off, then all notes off.
                        for controller in [120, 123] {
                            let data = [0xb0 | channel, controller, 0];
                            self.push(Slot::Midi(MidiEvent::new(time, 0, data)));
                        }
                        self.push(Slot::Midi(MidiEvent::new(time, 0, [0xe0 | channel, 0, 64])));
                        for (controller, value) in [(7, 127), (10, 64), (1, 0), (11, 127), (74, 64)] {
                            self.push(Slot::Midi(MidiEvent::new(time, 0, [0xb0 | channel, controller, value])));
                        }
                        self.push(Slot::Midi(MidiEvent::new(time, 0, [0xd0 | channel, 0, 0])));
                    }
                }
            }
        }
    }
}

impl InputEventBuffer for EventList {
    fn len(&self) -> u32 {
        self.slots.len() as u32
    }

    fn get(&self, index: u32) -> Option<&UnknownEvent> {
        self.slots.get(index as usize).map(Slot::as_unknown)
    }
}

/// Where a plugin's own events go: straight to a function, with nothing
/// stored.
pub(crate) struct Outgoing<'a> {
    pub sink: &'a mut dyn FnMut(PluginEvent),
}

impl OutputEventBuffer for Outgoing<'_> {
    fn try_push(&mut self, event: &UnknownEvent) -> Result<(), TryPushError> {
        let event = match event.as_core_event() {
            Some(CoreEventSpace::ParamValue(event)) => {
                event.param_id().map(|id| PluginEvent::ParamValue {
                    id: id.get(),
                    value: event.value(),
                })
            }
            Some(CoreEventSpace::ParamGestureBegin(event)) => event
                .param_id()
                .map(|id| PluginEvent::GestureBegin { id: id.get() }),
            Some(CoreEventSpace::ParamGestureEnd(event)) => event
                .param_id()
                .map(|id| PluginEvent::GestureEnd { id: id.get() }),
            Some(CoreEventSpace::NoteEnd(event)) => {
                let note = event.pckn();
                match (note.key, note.channel) {
                    (Match::Specific(key), channel) => Some(PluginEvent::NoteEnd {
                        key: key.min(127) as u8,
                        channel: channel.into_specific().unwrap_or(0).min(15) as u8,
                    }),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(event) = event {
            (self.sink)(event);
        }
        // An event the host has no use for still counts as taken.
        Ok(())
    }
}
