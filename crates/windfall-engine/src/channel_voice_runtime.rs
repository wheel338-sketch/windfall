//! Prepared control-side storage for channel note routing. No callback allocation.
use windfall_dsp::NoteInstanceId;
use windfall_project::ChannelVoiceSettings;

use crate::channel_voice::*;
use crate::sequencer::Clock;
use crate::voice::{Note, Origin};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generator {
    Arp,
    Echo,
}
#[derive(Clone, Copy)]
pub(crate) struct Owner {
    pub note: Note,
    pub instance: NoteInstanceId,
    pub generated: Option<(Generator, u64)>,
    /// The note has an entry in the held table. False for an echo that
    /// sounds alongside the note it repeats.
    pub admitted: bool,
}

pub(crate) struct ChannelVoiceRuntime {
    pub settings: ChannelVoiceSettings,
    pub envelopes: PreparedChannelEnvelopes,
    pub glide: f64,
    pub arp: Arpeggiator,
    pub echo: NoteEcho,
    pub held: HeldNotes,
    pub owners: [Option<Owner>; MAX_HELD_NOTES],
    chord: [Option<Note>; MAX_CHORD_NOTES],
    echo_sources: [Option<(u64, Note)>; ECHO_EVENT_CAPACITY],
    /// Echoes of notes still held by hand, waiting for their release.
    held_echoes: [Option<(u8, Origin, HeldEcho)>; MAX_HELD_NOTES],
    pub arp_events: NoteEvents,
    pub echo_events: NoteEvents,
    pub rejected_echoes: u64,
}

impl ChannelVoiceRuntime {
    pub fn prepare(settings: ChannelVoiceSettings, rate: f64) -> Self {
        let settings = settings.sanitized();
        Self {
            settings,
            envelopes: PreparedChannelEnvelopes::prepare(settings.envelopes, rate),
            glide: settings.polyphony.glide_coefficient(rate),
            arp: Arpeggiator::prepare(settings.arpeggiator),
            echo: NoteEcho::prepare(settings.echo),
            held: HeldNotes::default(),
            owners: [None; MAX_HELD_NOTES],
            chord: [None; MAX_CHORD_NOTES],
            echo_sources: [None; ECHO_EVENT_CAPACITY],
            held_echoes: [None; MAX_HELD_NOTES],
            arp_events: NoteEvents::default(),
            echo_events: NoteEvents::default(),
            rejected_echoes: 0,
        }
    }

    pub fn chord_on(&mut self, note: Note) {
        let slot = self
            .chord
            .iter()
            .position(|n| n.is_some_and(|n| n.key == note.key && n.origin == note.origin))
            .or_else(|| self.chord.iter().position(Option::is_none));
        if let Some(slot) = slot {
            self.chord[slot] = Some(note);
            self.refresh_chord();
        }
    }

    fn refresh_chord(&mut self) {
        let mut used = 0;
        for index in 0..self.chord.len() {
            if let Some(note) = self.chord[index] {
                self.chord[used] = Some(note);
                used += 1;
            }
        }
        self.chord[used..].fill(None);
        let mut chord = [HeldNote::default(); MAX_CHORD_NOTES];
        let mut len = 0;
        for note in self.chord.iter().flatten() {
            chord[len] = HeldNote {
                key: note.key,
                velocity: note.velocity,
            };
            len += 1;
        }
        self.arp.set_chord(&chord[..len]);
    }

    pub fn chord_off(&mut self, key: u8, origin: Origin) {
        let mut changed = false;
        for slot in &mut self.chord {
            if slot.is_some_and(|n| n.key == key && n.origin == origin) {
                *slot = None;
                changed = true;
            }
        }
        if changed {
            self.refresh_chord();
        }
    }

    pub fn expire(&mut self, clock: Clock, now: u64) {
        let mut changed = false;
        for slot in &mut self.chord {
            if slot.is_some_and(|n| clock.frame_of(n.end) <= now) {
                *slot = None;
                changed = true;
            }
        }
        if changed {
            self.refresh_chord();
        }
        for owner in &mut self.owners {
            if owner.is_some_and(|o| o.generated.is_none() && clock.frame_of(o.note.end) <= now) {
                self.held.note_off(owner.unwrap().note.key);
                *owner = None;
            }
        }
    }

    pub fn schedule_echo(&mut self, note: Note, duration: usize, rate: f64, bpm: f64) {
        let first = self.echo.next_id();
        if !self.echo.schedule_note(
            HeldNote {
                key: note.key,
                velocity: note.velocity,
            },
            0,
            duration,
            rate,
            bpm,
        ) {
            self.rejected_echoes = self.rejected_echoes.saturating_add(1);
            return;
        }
        let count = self.echo.next_id().wrapping_sub(first);
        for offset in 0..count {
            // One metadata slot per queued pair. Released pairs clear their slot.
            if let Some(slot) = self.echo_sources.iter_mut().find(|s| s.is_none()) {
                *slot = Some((first.wrapping_add(offset), note));
            }
        }
    }

    /// Echoes a note that is still held and has no known length. Its
    /// repeats start now and end when [`Self::release_held_echo`] is called
    /// for the same key and origin. A full table or queue leaves the note
    /// dry and counts one rejected echo.
    pub fn schedule_held_echo(&mut self, note: Note, rate: f64, bpm: f64) {
        if !self.settings.echo.enabled {
            return;
        }
        // Playing a held key again releases the echoes of the earlier press.
        self.release_held_echo(note.key, note.origin);
        let Some(slot) = self.held_echoes.iter().position(Option::is_none) else {
            self.rejected_echoes = self.rejected_echoes.saturating_add(1);
            return;
        };
        let Some(held) = self.echo.schedule_held(
            HeldNote {
                key: note.key,
                velocity: note.velocity,
            },
            0,
            rate,
            bpm,
        ) else {
            self.rejected_echoes = self.rejected_echoes.saturating_add(1);
            return;
        };
        if held.repeats() == 0 {
            return;
        }
        for index in 0..held.repeats() {
            if let Some(source) = self.echo_sources.iter_mut().find(|s| s.is_none()) {
                *source = Some((held.id(index), note));
            }
        }
        self.held_echoes[slot] = Some((note.key, note.origin, held));
    }

    /// Lets go of the echoes of a held note; they end one by one, each as
    /// long after this as it started after the note.
    pub fn release_held_echo(&mut self, key: u8, origin: Origin) {
        for slot in &mut self.held_echoes {
            if let Some((held_key, held_origin, held)) = *slot
                && held_key == key
                && held_origin == origin
            {
                self.echo.release_held(held, 0);
                *slot = None;
            }
        }
    }

    /// Discards every queued echo of notes from one origin, as when those
    /// notes are cut. The caller releases the echoes already sounding.
    pub fn cancel_echoes_of(&mut self, origin: Origin) {
        for slot in &mut self.echo_sources {
            if let Some((id, note)) = *slot
                && note.origin == origin
            {
                self.echo.cancel(id);
                *slot = None;
            }
        }
        for slot in &mut self.held_echoes {
            if slot.is_some_and(|(_, held_origin, _)| held_origin == origin) {
                *slot = None;
            }
        }
    }

    /// The oldest echo that sounds alongside its note, as a slot of
    /// [`Self::owners`]. It is the first voice to give way when the channel
    /// is at its voice limit.
    pub fn oldest_echo(&self) -> Option<usize> {
        self.owners
            .iter()
            .enumerate()
            .filter_map(|(slot, owner)| {
                owner
                    .filter(|owner| !owner.admitted)
                    .map(|owner| (owner.instance.0, slot))
            })
            .min()
            .map(|(_, slot)| slot)
    }

    pub fn source(&self, generator: Generator, event: NoteEvent) -> Option<Note> {
        match generator {
            Generator::Arp => self
                .chord
                .iter()
                .flatten()
                .find(|note| {
                    event.note.velocity == note.velocity
                        && event.note.key >= note.key
                        && (event.note.key - note.key).is_multiple_of(12)
                        && event.note.key - note.key <= 36
                })
                .copied(),
            Generator::Echo => self
                .echo_sources
                .iter()
                .flatten()
                .find(|(id, _)| *id == event.id)
                .map(|(_, note)| *note),
        }
    }

    pub fn forget_echo(&mut self, id: u64) {
        for slot in &mut self.echo_sources {
            if slot.is_some_and(|(known, _)| known == id) {
                *slot = None;
            }
        }
    }

    pub fn rebind(&mut self, channel: usize) {
        for note in self.chord.iter_mut().flatten() {
            note.channel = channel;
        }
        for (_, note) in self.echo_sources.iter_mut().flatten() {
            note.channel = channel;
        }
        for owner in self.owners.iter_mut().flatten() {
            owner.note.channel = channel;
        }
    }

    pub fn move_ends(&mut self, moved: &impl Fn(f64) -> f64) {
        for note in self
            .chord
            .iter_mut()
            .flatten()
            .chain(self.echo_sources.iter_mut().flatten().map(|(_, note)| note))
        {
            note.end = moved(note.end);
            if let Some(source) = &mut note.source {
                source.move_clock(moved);
            }
        }
        for owner in self.owners.iter_mut().flatten() {
            owner.note.end = moved(owner.note.end);
            if let Some(source) = &mut owner.note.source {
                source.move_clock(moved);
            }
        }
    }

    /// Flush transport-owned scheduling without retiring notes held by hand.
    pub fn end_sequenced(&mut self) {
        self.arp.reset();
        self.cancel_echoes_of(Origin::Sequenced);
        for note in &mut self.chord {
            if note.is_some_and(|note| note.origin == Origin::Sequenced) {
                *note = None;
            }
        }
        self.refresh_chord();
        self.held.clear();
        for owner in self.owners.iter().flatten().filter(|owner| owner.admitted) {
            admit_note(&mut self.held, owner.note.key, self.settings.polyphony);
        }
    }

    pub fn reset(&mut self) {
        self.arp.reset();
        self.arp.set_chord(&[]);
        self.echo.reset();
        self.held.clear();
        self.chord.fill(None);
        self.owners.fill(None);
        self.echo_sources.fill(None);
        self.held_echoes.fill(None);
        self.envelopes.reset();
    }
}
