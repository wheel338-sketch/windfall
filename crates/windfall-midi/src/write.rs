//! Writing a [`MidiSong`] as a Standard MIDI File.

use crate::song::{
    ControlKind, DEFAULT_RELEASE, MidiSong, MidiTrack, TICKS_PER_QUARTER, merged, rescale,
};

/// The largest number a file can write as a time or a length: 28 bits.
const MAX_NUMBER: u32 = 0x0FFF_FFFF;

/// The most track chunks a header can count.
const MAX_CHUNKS: usize = u16::MAX as usize;

/// How the tracks of a song are laid out in the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SmfFormat {
    /// Format 0: everything in one track. Some hardware reads nothing
    /// else.
    Single,
    /// Format 1: a first track with the tempo, signatures and markers, and
    /// then one track for each of the song's.
    #[default]
    Multi,
}

/// How finely the file counts time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Resolution {
    /// 960 ticks to the quarter note, Windfall's own: nothing is rounded.
    #[default]
    Ppq960,
    /// 480 ticks to the quarter note, for programs that read nothing
    /// finer. Every time is halved, and an odd tick rounds up.
    Ppq480,
}

impl Resolution {
    pub fn ticks_per_quarter(self) -> u16 {
        match self {
            Resolution::Ppq960 => 960,
            Resolution::Ppq480 => 480,
        }
    }

    /// Where a Windfall tick falls in a file of this resolution.
    fn tick(self, tick: u32) -> u32 {
        let per_quarter = u64::from(self.ticks_per_quarter());
        let scaled = rescale(u64::from(tick), per_quarter, TICKS_PER_QUARTER);
        u32::try_from(scaled).unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    pub format: SmfFormat,
    pub resolution: Resolution,
    /// Leaves out the status byte of an event that has the same status as
    /// the one before it, and writes a "note off" as a "note on" with
    /// velocity 0 so that runs of notes share one status. Every reader
    /// understands it and files come out about a quarter smaller. Turn it
    /// off for a file a person is going to read byte by byte.
    pub running_status: bool,
}

impl Default for WriteOptions {
    /// Format 1 at 960 ticks to the quarter note, with running status.
    fn default() -> Self {
        Self {
            format: SmfFormat::default(),
            resolution: Resolution::default(),
            running_status: true,
        }
    }
}

/// Writes a song as a Standard MIDI File. The same song and options always
/// give the same bytes.
///
/// What is written is the song's normal form ([`MidiSong::normalized`]), or
/// for [`SmfFormat::Single`] the normal form of the flattened song
/// ([`MidiSong::flattened`]). At [`Resolution::Ppq960`], reading the file
/// back gives exactly that.
///
/// # Order
///
/// Events on one tick are written in this order: names, time signatures,
/// key signatures, tempo changes, markers, the ends of notes, program
/// changes, controllers and bends, and last the starts of notes. A note
/// that ends where another begins is therefore let go first, and a
/// channel's sound is set before a note plays on it. Among the controllers
/// of one tick, those that choose a registered parameter come first and
/// those that set its value next, so that a bend range written on the tick
/// of a bend is in force for it.
///
/// # Limits of the format
///
/// - A note is at least one tick long in the file. At
///   [`Resolution::Ppq480`] a note that the halving would leave with no
///   length keeps one tick.
/// - A release velocity is written only when it differs from
///   [`DEFAULT_RELEASE`], or when running status is off.
/// - A file counts at most 65,535 tracks. In format 1 the first is the
///   tempo track, and a song with more than 65,534 tracks has those past
///   the 65,533rd merged into the last one.
/// - No single wait can be longer than 268,435,455 ticks. A longer gap is
///   bridged with empty text events, which readers pass over.
/// - Every track ends where the song does, at `length`, so the file is as
///   long as the song.
pub fn write(song: &MidiSong, options: &WriteOptions) -> Vec<u8> {
    let song = match options.format {
        SmfFormat::Single => song.flattened(),
        SmfFormat::Multi => song.clone(),
    }
    .normalized();
    let writer = Writer {
        resolution: options.resolution,
        running_status: options.running_status,
        end: options.resolution.tick(song.length),
    };

    let mut conductor = Vec::new();
    writer.song_events(&song, &mut conductor);
    let mut chunks = Vec::new();
    match options.format {
        SmfFormat::Single => {
            if let Some(track) = song.tracks.first() {
                writer.track_events(track, &mut conductor);
            }
            chunks.push(writer.chunk(conductor));
        }
        SmfFormat::Multi => {
            if let Some(name) = &song.name {
                conductor.push(Event::name(META_TRACK_NAME, RANK_NAME, name));
            }
            chunks.push(writer.chunk(conductor));
            let room = MAX_CHUNKS - 1;
            let (kept, rest) = if song.tracks.len() > room {
                song.tracks.split_at(room - 1)
            } else {
                (song.tracks.as_slice(), [].as_slice())
            };
            for track in kept {
                let mut events = Vec::new();
                writer.track_events(track, &mut events);
                chunks.push(writer.chunk(events));
            }
            if let Some(first) = rest.first() {
                let mut last = merged(rest);
                last.name = first.name.clone();
                last.normalize();
                let mut events = Vec::new();
                writer.track_events(&last, &mut events);
                chunks.push(writer.chunk(events));
            }
        }
    }

    let format: u16 = match options.format {
        SmfFormat::Single => 0,
        SmfFormat::Multi => 1,
    };
    let count = u16::try_from(chunks.len()).unwrap_or(u16::MAX);
    let size: usize = chunks.iter().map(|chunk| chunk.len() + 8).sum();
    let mut file = Vec::with_capacity(14 + size);
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6_u32.to_be_bytes());
    file.extend_from_slice(&format.to_be_bytes());
    file.extend_from_slice(&count.to_be_bytes());
    file.extend_from_slice(&options.resolution.ticks_per_quarter().to_be_bytes());
    for chunk in chunks {
        let length = u32::try_from(chunk.len()).unwrap_or(u32::MAX);
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&length.to_be_bytes());
        file.extend_from_slice(&chunk);
    }
    file
}

const META_TEXT: u8 = 0x01;
const META_TRACK_NAME: u8 = 0x03;
const META_INSTRUMENT_NAME: u8 = 0x04;
const META_MARKER: u8 = 0x06;
const META_END_OF_TRACK: u8 = 0x2F;
const META_TEMPO: u8 = 0x51;
const META_TIME_SIGNATURE: u8 = 0x58;
const META_KEY_SIGNATURE: u8 = 0x59;

// Where a kind of event stands among the events of one tick.
const RANK_NAME: u8 = 0;
const RANK_INSTRUMENT: u8 = 1;
const RANK_TIME_SIGNATURE: u8 = 2;
const RANK_KEY_SIGNATURE: u8 = 3;
const RANK_TEMPO: u8 = 4;
const RANK_MARKER: u8 = 5;
const RANK_NOTE_OFF: u8 = 6;
const RANK_PROGRAM: u8 = 7;
const RANK_CONTROL: u8 = 8;
const RANK_NOTE_ON: u8 = 9;

/// MIDI clocks between two clicks of the metronome, and thirty-second
/// notes to the quarter note: the two values every time signature event
/// carries, at what they are in all but the rarest file.
const CLOCKS_PER_CLICK: u8 = 24;
const THIRTY_SECONDS_PER_QUARTER: u8 = 8;

struct Event {
    tick: u32,
    rank: u8,
    /// What settles the order of two events of one rank on one tick.
    order: (u32, u32),
    message: Message,
}

enum Message {
    Meta {
        kind: u8,
        data: Vec<u8>,
    },
    Channel {
        status: u8,
        first: u8,
        second: Option<u8>,
    },
}

impl Event {
    fn name(kind: u8, rank: u8, text: &str) -> Self {
        Self::meta(0, rank, 0, kind, text.as_bytes().to_vec())
    }

    fn meta(tick: u32, rank: u8, index: usize, kind: u8, data: Vec<u8>) -> Self {
        Self {
            tick,
            rank,
            order: (index as u32, 0),
            message: Message::Meta { kind, data },
        }
    }
}

struct Writer {
    resolution: Resolution,
    running_status: bool,
    /// The tick of the file the song ends on.
    end: u32,
}

impl Writer {
    /// The events that belong to the song and to no track.
    fn song_events(&self, song: &MidiSong, events: &mut Vec<Event>) {
        let tick = |tick| self.resolution.tick(tick);
        for (index, signature) in song.time_signatures.iter().enumerate() {
            let data = vec![
                signature.numerator,
                signature.denominator.trailing_zeros() as u8,
                CLOCKS_PER_CLICK,
                THIRTY_SECONDS_PER_QUARTER,
            ];
            let kind = META_TIME_SIGNATURE;
            let at = tick(signature.tick);
            events.push(Event::meta(at, RANK_TIME_SIGNATURE, index, kind, data));
        }
        for (index, key) in song.key_signatures.iter().enumerate() {
            let data = vec![key.sharps as u8, u8::from(key.minor)];
            let kind = META_KEY_SIGNATURE;
            let at = tick(key.tick);
            events.push(Event::meta(at, RANK_KEY_SIGNATURE, index, kind, data));
        }
        for (index, tempo) in song.tempos.iter().enumerate() {
            let [_, a, b, c] = tempo.micros_per_quarter.to_be_bytes();
            let at = tick(tempo.tick);
            events.push(Event::meta(
                at,
                RANK_TEMPO,
                index,
                META_TEMPO,
                vec![a, b, c],
            ));
        }
        for (index, marker) in song.markers.iter().enumerate() {
            let data = marker.text.as_bytes().to_vec();
            let at = tick(marker.tick);
            events.push(Event::meta(at, RANK_MARKER, index, META_MARKER, data));
        }
    }

    fn track_events(&self, track: &MidiTrack, events: &mut Vec<Event>) {
        let tick = |tick| self.resolution.tick(tick);
        if let Some(name) = &track.name {
            events.push(Event::name(META_TRACK_NAME, RANK_NAME, name));
        }
        if let Some(name) = &track.instrument {
            events.push(Event::name(META_INSTRUMENT_NAME, RANK_INSTRUMENT, name));
        }
        for (index, note) in track.notes.iter().enumerate() {
            let start = tick(note.start);
            let end = tick(note.end()).max(start.saturating_add(1));
            events.push(Event {
                tick: start,
                rank: RANK_NOTE_ON,
                order: (index as u32, 0),
                message: Message::Channel {
                    status: 0x90 | note.channel,
                    first: note.key,
                    second: Some(note.velocity),
                },
            });
            // With running status, a "note on" with velocity 0 ends the
            // note without a new status byte. It cannot carry a release
            // velocity, so a note that has one of its own keeps a real
            // "note off".
            let plain = self.running_status && note.release == DEFAULT_RELEASE;
            let (status, release) = if plain {
                (0x90, 0)
            } else {
                (0x80, note.release)
            };
            events.push(Event {
                tick: end,
                rank: RANK_NOTE_OFF,
                // Of the notes of one key that end on one tick, the one
                // that began first ends first, as a reader pairs them.
                order: (start, index as u32),
                message: Message::Channel {
                    status: status | note.channel,
                    first: note.key,
                    second: Some(release),
                },
            });
        }
        for (index, program) in track.programs.iter().enumerate() {
            events.push(Event {
                tick: tick(program.tick),
                rank: RANK_PROGRAM,
                order: (index as u32, 0),
                message: Message::Channel {
                    status: 0xC0 | program.channel,
                    first: program.program,
                    second: None,
                },
            });
        }
        for (curve_index, curve) in track.controls.iter().enumerate() {
            for (index, point) in curve.points.iter().enumerate() {
                let value = (point.value & 0x7F) as u8;
                let (status, first, second) = match curve.kind {
                    ControlKind::Controller(number) => (0xB0, number, Some(value)),
                    ControlKind::PitchBend => (0xE0, value, Some((point.value >> 7) as u8 & 0x7F)),
                    ControlKind::ChannelPressure => (0xD0, value, None),
                    ControlKind::KeyPressure(key) => (0xA0, key, Some(value)),
                };
                events.push(Event {
                    tick: tick(point.tick),
                    rank: RANK_CONTROL,
                    order: (
                        (parameter_step(curve.kind) << 16) | (curve_index as u32 & 0xFFFF),
                        index as u32,
                    ),
                    message: Message::Channel {
                        status: status | curve.channel,
                        first,
                        second,
                    },
                });
            }
        }
    }

    /// The body of a track chunk that holds these events and ends where
    /// the song does.
    fn chunk(&self, mut events: Vec<Event>) -> Vec<u8> {
        events.sort_by_key(|event| (event.tick, event.rank, event.order));
        let mut out = Vec::new();
        let mut now = 0;
        // The status the next channel event may leave out.
        let mut running = None;
        for event in events {
            wait(&mut out, &mut now, event.tick, &mut running);
            match event.message {
                Message::Meta { kind, data } => {
                    meta(&mut out, kind, &data);
                    running = None;
                }
                Message::Channel {
                    status,
                    first,
                    second,
                } => {
                    if !(self.running_status && running == Some(status)) {
                        out.push(status);
                    }
                    running = Some(status);
                    out.push(first);
                    out.extend(second);
                }
            }
        }
        let end = self.end.max(now);
        wait(&mut out, &mut now, end, &mut running);
        meta(&mut out, META_END_OF_TRACK, &[]);
        out
    }
}

/// Where a controller stands among the controllers of one tick. A
/// registered or non-registered parameter is set by four controllers in a
/// row: which parameter (101 and 100, or 99 and 98), then its value (6 and
/// 38). A device that hears the value first sets whatever parameter was
/// chosen last, so on one tick they are written in that order, ahead of
/// every other controller and of the bend a bend range is for.
fn parameter_step(kind: ControlKind) -> u32 {
    match kind {
        ControlKind::Controller(101 | 99) => 0,
        ControlKind::Controller(100 | 98) => 1,
        ControlKind::Controller(6) => 2,
        ControlKind::Controller(38) => 3,
        _ => 4,
    }
}

/// Writes the wait from `now` until the tick `to`, which the next event
/// then happens on.
fn wait(out: &mut Vec<u8>, now: &mut u32, to: u32, running: &mut Option<u8>) {
    let mut delta = to - *now;
    while delta > MAX_NUMBER {
        number(out, MAX_NUMBER);
        meta(out, META_TEXT, &[]);
        *running = None;
        delta -= MAX_NUMBER;
    }
    number(out, delta);
    *now = to;
}

fn meta(out: &mut Vec<u8>, kind: u8, data: &[u8]) {
    let data = &data[..data.len().min(MAX_NUMBER as usize)];
    out.push(0xFF);
    out.push(kind);
    number(out, data.len() as u32);
    out.extend_from_slice(data);
}

/// Writes a variable-length number: seven bits to the byte, most
/// significant first, with the top bit set on every byte but the last.
fn number(out: &mut Vec<u8>, value: u32) {
    let value = value.min(MAX_NUMBER);
    for shift in [21, 14, 7] {
        if value >> shift != 0 {
            out.push((value >> shift) as u8 & 0x7F | 0x80);
        }
    }
    out.push(value as u8 & 0x7F);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_the_specification_lists_them() {
        let cases: [(u32, &[u8]); 9] = [
            (0x00, &[0x00]),
            (0x40, &[0x40]),
            (0x7F, &[0x7F]),
            (0x80, &[0x81, 0x00]),
            (0x2000, &[0xC0, 0x00]),
            (0x3FFF, &[0xFF, 0x7F]),
            (0x4000, &[0x81, 0x80, 0x00]),
            (0x20_0000, &[0x81, 0x80, 0x80, 0x00]),
            (0x0FFF_FFFF, &[0xFF, 0xFF, 0xFF, 0x7F]),
        ];
        for (value, bytes) in cases {
            let mut out = Vec::new();
            number(&mut out, value);
            assert_eq!(out, bytes, "{value:#x}");
        }
    }

    #[test]
    fn halving_rounds_an_odd_tick_up() {
        assert_eq!(Resolution::Ppq960.tick(961), 961);
        assert_eq!(Resolution::Ppq480.tick(960), 480);
        assert_eq!(Resolution::Ppq480.tick(961), 481);
        assert_eq!(Resolution::Ppq480.tick(1), 1);
        assert_eq!(Resolution::Ppq480.tick(u32::MAX), u32::MAX / 2 + 1);
    }
}
