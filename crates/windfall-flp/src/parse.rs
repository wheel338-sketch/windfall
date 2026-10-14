//! From the events of a file to an [`FlpProject`].
//!
//! The events of a file come in one row, and what an event belongs to
//! follows from the events before it: a channel's events follow the event
//! that names the channel, and so on. A handful of ids are shared: a
//! colour, a plugin's name and a plugin's state belong to the channel
//! being described until the mixer starts, and to an effect slot after
//! that. The mixer starts with the first event only an insert has.
//!
//! An insert's events end with its output (event 147), and its name and
//! colour come before its flags. An effect slot's events end with its
//! number (event 98); files from before that event number the slot in its
//! wrapper (event 212), and a slot ends where the next one's name begins.
//! Both orders were read off files saved by FL Studio 9, 11 and 20, and
//! match how DawVert and FLParser walk them.
//!
//! # Limits
//!
//! Nothing here allocates more than a small multiple of the size of the
//! file, and the file is at most
//! [`MAX_FILE_BYTES`](crate::event::MAX_FILE_BYTES). Things that a few
//! bytes of a file can ask for in bulk are counted as well, so a hostile
//! file cannot turn a megabyte into a gigabyte: channels, patterns,
//! inserts, slots, arrangements, tracks and kept events all have a cap,
//! and what is over it is left out with a note in the diagnostics.

use std::collections::BTreeMap;

use crate::error::FlpError;
use crate::event::{self, Event, EventValue, u16_at, u32_at};
use crate::model::{
    Arrangement, AutomationCurve, AutomationPoint, Channel, ChannelEcho, ChannelKind,
    ChannelParams, ControlEvent, ControlValue, CutGroups, Diagnostic, EnvelopeLfo, FlVersion,
    FlpProject, Insert, LegacyPlaylistItem, LegacyStep, Note, Pattern, PlaylistItem,
    PlaylistItemExtra, PlaylistSource, Polyphony, RawEvent, RawValue, RemoteController, Route,
    Slot, TimeMarker, Track,
};
use crate::target::{ControlTarget, InsertParam, SlotParam};
use crate::text::{self, TextEncoding};

/// Most uninterpreted events that are kept with their values.
pub const MAX_KEPT_EVENTS: usize = 16_384;
/// Most channels read from one file.
pub const MAX_CHANNELS: usize = 8_192;
/// Most patterns read from one file.
pub const MAX_PATTERNS: usize = 8_192;
/// Most mixer inserts read from one file; includes modern 500-insert projects.
pub const MAX_INSERTS: usize = 512;
/// Most effect slots read for one insert. FL Studio has 10.
pub const MAX_SLOTS: usize = 32;
/// Most arrangements read from one file.
pub const MAX_ARRANGEMENTS: usize = 256;
/// Most playlist tracks read for one arrangement. FL Studio has 500.
pub const MAX_TRACKS: usize = 2_048;
/// Most markers read from one file.
pub const MAX_MARKERS: usize = 16_384;
/// Most links from controllers read from one file.
pub const MAX_REMOTE_CONTROLLERS: usize = 65_536;
/// Most channel groups read from one file.
pub const MAX_CHANNEL_GROUPS: usize = 4_096;
/// Most layer children read for one channel.
pub const MAX_LAYER_CHILDREN: usize = 4_096;
/// Global event-223 record budget, across every pattern and repeated event.
pub const MAX_RECORDED_CONTROL_EVENTS: usize = 262_144;
/// Complete event-prefix retention budget, including understood source controls.
pub const MAX_SOURCE_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 200;

/// Bytes of one note in event 224.
const NOTE_BYTES: usize = 24;
/// Bytes of one note in files from before FL Studio 9, which have no
/// group and no release velocity. Agreed (LMMS, and a layout DawVert keeps
/// for version 8 and older).
const OLD_NOTE_BYTES: usize = 20;
/// Bytes of one playlist item.
const ITEM_BYTES: usize = 32;
/// Bytes of one playlist item from FL Studio 21 on.
const NEW_ITEM_BYTES: usize = 60;
/// Bytes before the count of points in event 234, and of one point.
const CURVE_HEADER_BYTES: usize = 21;
const POINT_BYTES: usize = 24;
/// Bytes of one record of control values.
const CONTROL_BYTES: usize = 12;

/// Reads a file.
///
/// A file that is cut off, or damaged somewhere in the middle, still gives
/// a project: everything up to the damage, and a line in
/// [`FlpProject::diagnostics`] that says where it was. Only a file that is
/// not an FL Studio file at all, or has no events, is an error.
pub fn parse(bytes: &[u8]) -> Result<FlpProject, FlpError> {
    let container = event::open(bytes)?;
    let mut parser = Parser::new();
    parser.project.header = Some(container.header);
    if container.cut_short {
        parser.note(
            None,
            windfall_core::notice!("flp.read.cutShort").to_string(),
        );
    }
    for event in container.events {
        match event {
            Ok(event) => parser.event(&event),
            Err(error) => {
                parser.note(
                    None,
                    windfall_core::notice!("flp.read.stopped", error = error.to_string())
                        .to_string(),
                );
            }
        }
    }
    Ok(parser.finish())
}

/// Who owns the markers that come next.
#[derive(Clone, Copy)]
enum MarkerOwner {
    Nobody,
    Pattern(usize),
    Arrangement(usize),
}

#[derive(Default)]
struct PendingSlot {
    slot: Slot,
    named: bool,
    /// The slot's number from its wrapper, for files without event 98.
    wrapper_index: Option<u32>,
}

#[derive(Default)]
struct PendingInsert {
    insert: Insert,
    /// Anything of this insert has been read.
    started: bool,
    /// Its flags event has been read, so a name or colour that follows
    /// belongs to the next insert.
    flagged: bool,
    slot: Option<PendingSlot>,
}

struct Parser {
    project: FlpProject,
    version: Option<FlVersion>,
    encoding: Option<TextEncoding>,
    channel: Option<usize>,
    channel_index: BTreeMap<u16, usize>,
    pattern: Option<usize>,
    pattern_index: BTreeMap<u16, usize>,
    arrangement: Option<usize>,
    /// The mixer has started: shared ids now belong to effect slots.
    in_mixer: bool,
    insert: PendingInsert,
    mixer_controls: Vec<ControlValue>,
    marker_owner: MarkerOwner,
    marker_count: usize,
    recorded_control_count: usize,
    source_archive_full: bool,
    legacy_tempo: (Option<u16>, Option<u16>),
    /// Diagnostics that were not kept because there were too many.
    dropped_diagnostics: usize,
    /// Things left out because a cap was reached, by what they are.
    over_cap: BTreeMap<&'static str, usize>,
}

impl Parser {
    fn new() -> Self {
        Self {
            project: FlpProject::default(),
            version: None,
            encoding: None,
            channel: None,
            channel_index: BTreeMap::new(),
            pattern: None,
            pattern_index: BTreeMap::new(),
            arrangement: None,
            in_mixer: false,
            insert: PendingInsert::default(),
            mixer_controls: Vec::new(),
            marker_owner: MarkerOwner::Nobody,
            marker_count: 0,
            recorded_control_count: 0,
            source_archive_full: false,
            legacy_tempo: (None, None),
            dropped_diagnostics: 0,
            over_cap: BTreeMap::new(),
        }
    }

    fn note(&mut self, offset: Option<usize>, message: impl Into<String>) {
        if self.project.diagnostics.len() < MAX_DIAGNOSTICS {
            self.project.diagnostics.push(Diagnostic {
                offset,
                message: message.into(),
            });
        } else {
            self.dropped_diagnostics += 1;
        }
    }

    fn over(&mut self, what: &'static str) {
        *self.over_cap.entry(what).or_default() += 1;
    }

    fn text(&self, data: &[u8]) -> String {
        let encoding = self.encoding.unwrap_or_else(|| TextEncoding::guess(data));
        text::decode(data, encoding)
    }

    fn event(&mut self, event: &Event<'_>) {
        self.archive(event);
        let handled = match event.value {
            EventValue::Byte(value) => self.byte(event.id, value),
            EventValue::Word(value) => self.word(event.id, value),
            EventValue::DWord(value) => self.dword(event.id, value),
            EventValue::Data(data) => self.data(event, data),
        };
        if !handled {
            self.keep(event);
        }
    }

    fn archive(&mut self, event: &Event<'_>) {
        // Deliberately omit the personal licensee event, as keep() already does.
        if event.id == 200 || self.source_archive_full {
            return;
        }
        let payload = match event.value {
            EventValue::Byte(_) => 1,
            EventValue::Word(_) => 2,
            EventValue::DWord(_) => 4,
            EventValue::Data(data) => data.len(),
        };
        let mut header = vec![event.id];
        if matches!(event.value, EventValue::Data(_)) {
            let mut size = payload;
            loop {
                let low = (size & 127) as u8;
                size >>= 7;
                header.push(low | if size == 0 { 0 } else { 128 });
                if size == 0 {
                    break;
                }
            }
        }
        if payload + header.len() > MAX_SOURCE_EVENT_BYTES - self.project.source_event_stream.len()
        {
            self.source_archive_full = true;
            self.note(
                None,
                windfall_core::notice!("flp.read.archiveFull").to_string(),
            );
            return;
        }
        let out = &mut self.project.source_event_stream;
        out.extend(header);
        match event.value {
            EventValue::Byte(value) => out.push(value),
            EventValue::Word(value) => out.extend(value.to_le_bytes()),
            EventValue::DWord(value) => out.extend(value.to_le_bytes()),
            EventValue::Data(data) => out.extend_from_slice(data),
        }
    }

    fn keep(&mut self, event: &Event<'_>) {
        *self
            .project
            .uninterpreted_counts
            .entry(event.id)
            .or_default() += 1;
        // The licensee is the name of a person, and nothing here needs it.
        const LICENSEE: u8 = 200;
        if event.id == LICENSEE || self.project.uninterpreted.len() >= MAX_KEPT_EVENTS {
            return;
        }
        let value = match event.value {
            EventValue::Byte(value) => RawValue::Byte(value),
            EventValue::Word(value) => RawValue::Word(value),
            EventValue::DWord(value) => RawValue::DWord(value),
            EventValue::Data(data) => RawValue::Data(data.to_vec()),
        };
        self.project.uninterpreted.push(RawEvent {
            id: event.id,
            value,
        });
    }

    fn current_channel(&mut self) -> Option<&mut Channel> {
        if self.in_mixer {
            return None;
        }
        self.project.channels.get_mut(self.channel?)
    }

    fn current_pattern(&mut self) -> Option<&mut Pattern> {
        self.project.patterns.get_mut(self.pattern?)
    }

    fn byte(&mut self, id: u8, value: u8) -> bool {
        match id {
            9 => self.project.settings.loop_active = Some(value != 0),
            11 => self.project.settings.swing = Some(value),
            12 => self.project.settings.main_volume = Some(value),
            17 => self.project.settings.numerator = Some(value),
            18 => self.project.settings.denominator = Some(value),
            23 => self.project.settings.pan_law = Some(value),
            30 => self.project.settings.play_truncated_notes = Some(value != 0),
            29 => self.project.mixer.delay_compensation = Some(value != 0),
            0 => return self.on_channel(|channel| channel.enabled = Some(value != 0)),
            21 => {
                return self.on_channel(|channel| channel.kind = ChannelKind::from_raw(value));
            }
            22 => return self.on_channel(|channel| channel.insert = Some(value as i8)),
            27 => {
                self.insert_event(false);
                self.insert.insert.flags = Some(u32::from(value));
            }
            33 | 34 => {
                let Some(marker) = self.last_marker() else {
                    return false;
                };
                if id == 33 {
                    marker.numerator = Some(value);
                } else {
                    marker.denominator = Some(value);
                }
            }
            _ => return false,
        }
        true
    }

    fn word(&mut self, id: u8, value: u16) -> bool {
        match id {
            64 => self.select_channel(value),
            65 => self.select_pattern(value),
            66 => self.legacy_tempo.0 = Some(value),
            93 => self.legacy_tempo.1 = Some(value),
            67 => self.project.settings.current_pattern = Some(value),
            80 => self.project.settings.main_pitch = Some(value as i16),
            70 => return self.on_channel(|channel| channel.fx_flags = Some(value)),
            74 => return self.on_channel(|channel| channel.preamp = Some(value)),
            97 => return self.on_channel(|channel| channel.swing = Some(value)),
            89 => return self.on_channel(|channel| channel.time_shift = Some(value)),
            71 => return self.on_channel(|channel| channel.cutoff = Some(value)),
            83 => return self.on_channel(|channel| channel.resonance = Some(value)),
            94 => {
                let mut full = false;
                let handled = self.on_channel(|channel| {
                    if channel.layer_children.len() < MAX_LAYER_CHILDREN {
                        channel.layer_children.push(value);
                    } else {
                        full = true;
                    }
                });
                if full {
                    self.over("layer children");
                }
                return handled;
            }
            91 => {
                let (Some(channel), false) = (self.channel, self.in_mixer) else {
                    return false;
                };
                let iid = self.project.channels[channel].iid;
                let Some(pattern) = self.current_pattern() else {
                    return false;
                };
                pattern.legacy_steps.push(LegacyStep {
                    channel: iid,
                    raw: value,
                });
            }
            98 => {
                self.insert_event(false);
                self.finish_slot(Some(u32::from(value & 0xFF)));
            }
            99 => self.select_arrangement(value),
            100 => self.project.current_arrangement = Some(value),
            _ => return false,
        }
        true
    }

    fn dword(&mut self, id: u8, value: u32) -> bool {
        match id {
            156 => self.project.settings.tempo_millibpm = Some(value),
            128 => {
                let color = Some(color(value));
                if self.in_mixer {
                    self.slot().slot.color = color;
                } else {
                    return self.on_channel(|channel| channel.color = color);
                }
            }
            129 => {
                let index = self.arrangement_or_first();
                let Some(arrangement) = self.project.arrangements.get_mut(index) else {
                    return false;
                };
                arrangement.legacy_items.push(LegacyPlaylistItem {
                    bar: (value & 0xFFFF) as u16,
                    pattern: (value >> 16) as u16,
                });
            }
            132 => {
                return self.on_channel(|channel| {
                    channel.cut = Some(CutGroups {
                        cuts: (value & 0xFFFF) as u16,
                        cut_by: (value >> 16) as u16,
                    });
                });
            }
            135 => return self.on_channel(|channel| channel.root_note = Some(value)),
            142 => return self.on_channel(|channel| channel.fine_tune = Some(value as i32)),
            143 => return self.on_channel(|channel| channel.sampler_flags = Some(value)),
            145 => return self.on_channel(|channel| channel.group = Some(value as i32)),
            147 => {
                self.insert_event(false);
                self.insert.insert.output = Some(value as i32);
                self.finish_insert();
            }
            148 => self.marker(value),
            149 => {
                self.insert_event(true);
                self.insert.insert.color = Some(color(value));
            }
            150 => {
                let Some(pattern) = self.current_pattern() else {
                    return false;
                };
                pattern.color = Some(color(value));
            }
            154 => {
                self.insert_event(false);
                self.insert.insert.input = Some(value as i32);
            }
            164 => {
                let Some(pattern) = self.current_pattern() else {
                    return false;
                };
                pattern.length = Some(value);
            }
            _ => return false,
        }
        true
    }

    fn data(&mut self, event: &Event<'_>, data: &[u8]) -> bool {
        let offset = Some(event.offset);
        match event.id {
            199 => {
                let version = text::decode_bytes(data);
                self.version = FlVersion::parse(&version);
                self.encoding = self.version.map(TextEncoding::of);
                self.project.version_text = Some(version);
            }
            192 => {
                let name = self.text(data);
                return self.on_channel(|channel| {
                    // The newer event 203 wins over this one.
                    channel.name.get_or_insert(name);
                });
            }
            193 => {
                let name = self.text(data);
                let Some(pattern) = self.current_pattern() else {
                    return false;
                };
                pattern.name = Some(name);
            }
            194 => self.project.settings.title = Some(self.text(data)),
            195 => {
                self.project.settings.comments = Some(self.text(data));
                self.project.settings.comments_are_rtf = false;
            }
            198 => {
                if self.project.settings.comments.is_none() {
                    self.project.settings.comments = Some(self.text(data));
                    self.project.settings.comments_are_rtf = true;
                }
            }
            196 => {
                let path = self.text(data);
                return self.on_channel(|channel| channel.sample_path = Some(path));
            }
            197 => self.project.settings.url = Some(self.text(data)),
            202 => self.project.settings.data_path = Some(self.text(data)),
            206 => self.project.settings.genre = Some(self.text(data)),
            207 => self.project.settings.author = Some(self.text(data)),
            201 => {
                let name = self.text(data);
                if self.in_mixer {
                    if self.insert.slot.as_ref().is_some_and(|slot| slot.named) {
                        self.finish_slot(None);
                    }
                    let slot = self.slot();
                    slot.slot.plugin.internal_name = name;
                    slot.named = true;
                } else if !name.is_empty() {
                    return self.on_channel(|channel| {
                        channel.plugin.get_or_insert_default().internal_name = name;
                    });
                } else if self.current_channel().is_none() {
                    return false;
                }
            }
            203 => {
                let name = self.text(data);
                if self.in_mixer {
                    self.slot().slot.name = Some(name);
                } else {
                    return self.on_channel(|channel| channel.name = Some(name));
                }
            }
            204 => {
                self.insert_event(true);
                self.insert.insert.name = Some(self.text(data));
            }
            205 => {
                let name = self.text(data);
                let Some(marker) = self.last_marker() else {
                    return false;
                };
                marker.name = Some(name);
            }
            212 => {
                let generator = (data.len() >= 20).then(|| u32_at(data, 16) & 0x10 != 0);
                if self.in_mixer {
                    let index = (data.len() >= 8).then(|| u32_at(data, 4));
                    let slot = self.slot();
                    slot.wrapper_index = index;
                    slot.slot.plugin.generator = generator;
                } else {
                    return self.on_channel(|channel| {
                        if let Some(plugin) = &mut channel.plugin {
                            plugin.generator = generator;
                        }
                    });
                }
            }
            213 => {
                if self.in_mixer {
                    self.slot().slot.plugin.state = data.to_vec();
                } else {
                    return self.on_channel(|channel| {
                        channel.plugin.get_or_insert_default().state = data.to_vec();
                    });
                }
            }
            215 => {
                let params = channel_params(data);
                return self.on_channel(|channel| channel.params = Some(params));
            }
            209 => {
                if data.len() < 20 {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.echoShort").to_string(),
                    );
                    return true;
                }
                return self.on_channel(|channel| {
                    channel.echo = Some(ChannelEcho {
                        feedback: u32_at(data, 0),
                        pan: u32_at(data, 4) as i32,
                        pitch_cents: u32_at(data, 8) as i32,
                        repeats: u32_at(data, 12),
                        time: u32_at(data, 16),
                    })
                });
            }
            216 => self.project.initial_controls.extend(control_values(data)),
            218 => {
                let Some(envelope) = envelope(data) else {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.envelopeShort").to_string(),
                    );
                    return true;
                };
                return self.on_channel(|channel| {
                    // FL Studio writes five. More than that is not a file
                    // FL Studio wrote.
                    if channel.envelopes.len() < 16 {
                        channel.envelopes.push(envelope);
                    }
                });
            }
            219 => {
                if data.len() < 12 {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.levelsShort").to_string(),
                    );
                    return true;
                }
                return self.on_channel(|channel| {
                    channel.pan = Some(u32_at(data, 0));
                    channel.volume = Some(u32_at(data, 4));
                    channel.pitch = Some(u32_at(data, 8) as i32);
                    channel.filter_mod_x = (data.len() >= 16).then(|| u32_at(data, 12));
                    channel.filter_mod_y = (data.len() >= 20).then(|| u32_at(data, 16));
                    channel.filter_type = (data.len() >= 24).then(|| u32_at(data, 20));
                });
            }
            221 => {
                if data.len() < 9 {
                    return false;
                }
                return self.on_channel(|channel| {
                    channel.polyphony = Some(Polyphony {
                        max: u32_at(data, 0),
                        slide: u32_at(data, 4),
                        flags: data[8],
                    });
                });
            }
            223 => {
                if self.pattern.is_none() {
                    return false;
                }
                let records = data.as_chunks::<CONTROL_BYTES>().0;
                let allowed = records
                    .len()
                    .min(MAX_RECORDED_CONTROL_EVENTS - self.recorded_control_count);
                self.recorded_control_count += allowed;
                let Some(pattern) = self.current_pattern() else {
                    return false;
                };
                pattern
                    .control_events
                    .extend(records.iter().take(allowed).map(|record| ControlEvent {
                        position: u32_at(record, 0),
                        location: u32_at(record, 4),
                        value: u32_at(record, 8) as i32,
                    }));
                if records.len() > allowed {
                    *self.over_cap.entry("recorded control events").or_default() +=
                        records.len() - allowed;
                }
                if !data.len().is_multiple_of(CONTROL_BYTES) {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.recordedControlShort").to_string(),
                    );
                }
            }
            224 => self.notes(event, data),
            225 => self.mixer_controls.extend(control_values(data)),
            227 => {
                if data.len() < 20 {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.controllerLinkShort").to_string(),
                    );
                    return true;
                }
                if self.project.remote_controllers.len() >= MAX_REMOTE_CONTROLLERS {
                    self.over("links from controllers");
                    return true;
                }
                self.project.remote_controllers.push(RemoteController {
                    source: u32_at(data, 2),
                    location: u32_at(data, 8),
                    flags: u32_at(data, 12),
                    smoothing: u32_at(data, 16),
                });
            }
            231 => {
                let name = self.text(data);
                if self.project.channel_groups.len() < MAX_CHANNEL_GROUPS {
                    self.project.channel_groups.push(name);
                } else {
                    self.over("channel groups");
                }
            }
            233 => self.playlist(event, data),
            234 => {
                let Some(curve) = automation_curve(data) else {
                    self.note(
                        offset,
                        windfall_core::notice!("flp.read.curveShort").to_string(),
                    );
                    return true;
                };
                return self.on_channel(|channel| channel.automation = Some(curve));
            }
            235 => {
                self.insert_event(false);
                self.insert.insert.routes = data
                    .iter()
                    .enumerate()
                    .filter(|&(_, &on)| on != 0)
                    .take(MAX_INSERTS)
                    .map(|(target, _)| Route {
                        target: target as u16,
                        level: None,
                    })
                    .collect();
            }
            236 => {
                self.insert_event(false);
                self.insert.flagged = true;
                // Files from before FL Studio 12 hold only a latency here
                // and keep the flags in event 27.
                if data.len() >= 8 {
                    self.insert.insert.flags = Some(u32_at(data, 4));
                }
            }
            238 => self.track(event, data),
            239 => {
                let name = self.text(data);
                let track = self
                    .arrangement
                    .and_then(|index| self.project.arrangements.get_mut(index))
                    .and_then(|arrangement| arrangement.tracks.last_mut());
                let Some(track) = track else {
                    return false;
                };
                track.name = Some(name);
            }
            241 => {
                let name = self.text(data);
                let Some(arrangement) = self
                    .arrangement
                    .and_then(|index| self.project.arrangements.get_mut(index))
                else {
                    return false;
                };
                arrangement.name = Some(name);
            }
            _ => return false,
        }
        true
    }

    /// Runs `edit` on the channel being described. False when there is
    /// none, which leaves the event uninterpreted.
    fn on_channel(&mut self, edit: impl FnOnce(&mut Channel)) -> bool {
        match self.current_channel() {
            Some(channel) => {
                edit(channel);
                true
            }
            None => false,
        }
    }

    fn select_channel(&mut self, iid: u16) {
        if let Some(&index) = self.channel_index.get(&iid) {
            self.channel = Some(index);
        } else if self.project.channels.len() < MAX_CHANNELS {
            self.channel_index.insert(iid, self.project.channels.len());
            self.channel = Some(self.project.channels.len());
            self.project.channels.push(Channel {
                iid,
                ..Channel::default()
            });
        } else {
            self.channel = None;
            self.over("channels");
        }
    }

    fn select_pattern(&mut self, iid: u16) {
        if let Some(&index) = self.pattern_index.get(&iid) {
            self.pattern = Some(index);
        } else if self.project.patterns.len() < MAX_PATTERNS {
            self.pattern_index.insert(iid, self.project.patterns.len());
            self.pattern = Some(self.project.patterns.len());
            self.project.patterns.push(Pattern {
                iid,
                ..Pattern::default()
            });
        } else {
            self.pattern = None;
            self.over("patterns");
        }
        self.marker_owner = self
            .pattern
            .map_or(MarkerOwner::Nobody, MarkerOwner::Pattern);
    }

    fn select_arrangement(&mut self, index: u16) {
        let arrangements = &mut self.project.arrangements;
        let found = arrangements.iter().position(|a| a.index == index);
        self.arrangement = match found {
            Some(position) => Some(position),
            None if arrangements.len() < MAX_ARRANGEMENTS => {
                arrangements.push(Arrangement {
                    index,
                    ..Arrangement::default()
                });
                Some(arrangements.len() - 1)
            }
            None => {
                self.over("arrangements");
                None
            }
        };
        self.marker_owner = self
            .arrangement
            .map_or(MarkerOwner::Nobody, MarkerOwner::Arrangement);
    }

    /// The arrangement being described. Files from before FL Studio 12.9
    /// do not announce their one arrangement, so the first playlist event
    /// makes it.
    fn arrangement_or_first(&mut self) -> usize {
        if self.arrangement.is_none() {
            self.select_arrangement(0);
        }
        self.arrangement.unwrap_or(usize::MAX)
    }

    fn marker(&mut self, value: u32) {
        if self.marker_count >= MAX_MARKERS {
            self.over("markers");
            return;
        }
        let marker = TimeMarker {
            position: value & 0x00FF_FFFF,
            kind: (value >> 24) as u8,
            ..TimeMarker::default()
        };
        if let MarkerOwner::Nobody = self.marker_owner {
            self.arrangement_or_first();
            self.marker_owner = self
                .arrangement
                .map_or(MarkerOwner::Nobody, MarkerOwner::Arrangement);
        }
        let markers = match self.marker_owner {
            MarkerOwner::Pattern(index) => self
                .project
                .patterns
                .get_mut(index)
                .map(|pattern| &mut pattern.markers),
            MarkerOwner::Arrangement(index) => self
                .project
                .arrangements
                .get_mut(index)
                .map(|arrangement| &mut arrangement.markers),
            MarkerOwner::Nobody => None,
        };
        if let Some(markers) = markers {
            markers.push(marker);
            self.marker_count += 1;
        }
    }

    fn last_marker(&mut self) -> Option<&mut TimeMarker> {
        match self.marker_owner {
            MarkerOwner::Pattern(index) => self.project.patterns.get_mut(index)?.markers.last_mut(),
            MarkerOwner::Arrangement(index) => {
                self.project.arrangements.get_mut(index)?.markers.last_mut()
            }
            MarkerOwner::Nobody => None,
        }
    }

    /// Notes that an event of an insert has come. `opens` is true for the
    /// events that come before an insert's flags, its name and its colour:
    /// after the flags they belong to the next insert.
    fn insert_event(&mut self, opens: bool) {
        self.in_mixer = true;
        if opens && self.insert.flagged {
            self.finish_insert();
        }
        self.insert.started = true;
    }

    fn slot(&mut self) -> &mut PendingSlot {
        self.insert.started = true;
        self.insert.slot.get_or_insert_default()
    }

    /// Ends the effect slot being described. `index` is its number from
    /// event 98; without one the wrapper's number is used, or the next
    /// free one.
    fn finish_slot(&mut self, index: Option<u32>) {
        let Some(pending) = self.insert.slot.take() else {
            return;
        };
        let slots = &mut self.insert.insert.slots;
        if slots.len() >= MAX_SLOTS {
            self.over("effect slots");
            return;
        }
        let index = index
            .or(pending.wrapper_index)
            .unwrap_or(slots.len() as u32)
            .min(255) as u8;
        slots.push(Slot {
            index,
            ..pending.slot
        });
    }

    fn finish_insert(&mut self) {
        self.finish_slot(None);
        let pending = std::mem::take(&mut self.insert);
        if !pending.started {
            return;
        }
        if self.project.mixer.inserts.len() < MAX_INSERTS {
            self.project.mixer.inserts.push(pending.insert);
        } else {
            self.over("mixer inserts");
        }
    }

    fn notes(&mut self, event: &Event<'_>, data: &[u8]) {
        let old = self.version.is_some_and(|version| version.major < 9);
        let (first, second) = if old {
            (OLD_NOTE_BYTES, NOTE_BYTES)
        } else {
            (NOTE_BYTES, OLD_NOTE_BYTES)
        };
        let size = if data.len().is_multiple_of(first) || !data.len().is_multiple_of(second) {
            first
        } else {
            second
        };
        if !data.len().is_multiple_of(size) {
            self.note(
                Some(event.offset),
                windfall_core::notice!("flp.read.notesCutShort").to_string(),
            );
        }
        let Some(pattern) = self.current_pattern() else {
            self.note(
                Some(event.offset),
                windfall_core::notice!("flp.read.notesBeforePattern").to_string(),
            );
            return;
        };
        let read = |record: &[u8]| {
            let common = Note {
                position: u32_at(record, 0),
                flags: u16_at(record, 4),
                channel: u16_at(record, 6),
                length: u32_at(record, 8),
                ..Note::default()
            };
            if size == NOTE_BYTES {
                Note {
                    key: u16_at(record, 12),
                    group: u16_at(record, 14),
                    fine_pitch: record[16],
                    release: record[18],
                    midi_channel: record[19],
                    pan: record[20],
                    velocity: record[21],
                    mod_x: record[22],
                    mod_y: record[23],
                    ..common
                }
            } else {
                Note {
                    key: u16::from(record[12]),
                    fine_pitch: record[13],
                    release: 64,
                    midi_channel: record[15],
                    pan: record[16],
                    velocity: record[17],
                    mod_x: record[18],
                    mod_y: record[19],
                    ..common
                }
            }
        };
        pattern.notes.extend(data.chunks_exact(size).map(read));
    }

    fn playlist(&mut self, event: &Event<'_>, data: &[u8]) {
        let index = self.arrangement_or_first();
        self.marker_owner = self
            .arrangement
            .map_or(MarkerOwner::Nobody, MarkerOwner::Arrangement);
        // FL Studio 21 made an item longer. DawVert takes the late betas
        // of 20, numbered 20.99, for 21 as well.
        let new = self
            .version
            .is_some_and(|version| (version.major, version.minor) >= (20, 99));
        let (first, second) = if new {
            (NEW_ITEM_BYTES, ITEM_BYTES)
        } else {
            (ITEM_BYTES, NEW_ITEM_BYTES)
        };
        let size = if data.len().is_multiple_of(first) || !data.len().is_multiple_of(second) {
            first
        } else {
            second
        };
        if !data.len().is_multiple_of(size) {
            self.note(
                Some(event.offset),
                windfall_core::notice!("flp.read.playlistCutShort").to_string(),
            );
        }
        // The file counts tracks from the bottom. Without a version, a
        // number above 198 says which of the two heights it is.
        let tall = match self.version {
            Some(version) => version >= FlVersion::new(12, 9, 1),
            None => data
                .chunks_exact(size)
                .any(|record| u16_at(record, 12) > 198),
        };
        let top: u16 = if tall { 499 } else { 198 };
        let mut off_the_playlist = 0_usize;
        let mut items = Vec::new();
        for record in data.chunks_exact(size) {
            let (base, item) = (u16_at(record, 4), u16_at(record, 6));
            let row = u16_at(record, 12);
            let block = legacy_block(tall, base, item, row);
            let track = match (top.checked_sub(row), block) {
                (Some(track), _) => track,
                // Blocks get rows of their own below the clip tracks, one
                // for each pattern, as FL Studio showed them.
                (None, Some(pattern)) => top + pattern,
                (None, None) => {
                    off_the_playlist += 1;
                    continue;
                }
            };
            let source = if let Some(pattern) = block {
                PlaylistSource::Pattern {
                    pattern,
                    start: None,
                    end: None,
                }
            } else if item > base {
                // -1 is "none", and some versions write it as the float.
                let ticks = |at: usize| {
                    let raw = u32_at(record, at);
                    (raw != u32::MAX && raw != (-1.0_f32).to_bits()).then_some(raw)
                };
                PlaylistSource::Pattern {
                    pattern: item - base,
                    start: ticks(24),
                    end: ticks(28),
                }
            } else {
                let amount = |at: usize| {
                    let value = f32::from_bits(u32_at(record, at));
                    (value.is_finite() && value >= 0.0).then_some(value)
                };
                PlaylistSource::Channel {
                    channel: item,
                    start: amount(24),
                    end: amount(28),
                }
            };
            let float = |at: usize| f32::from_bits(u32_at(record, at));
            items.push(PlaylistItem {
                position: u32_at(record, 0),
                length: u32_at(record, 8),
                track,
                group: u16_at(record, 14),
                flags: u16_at(record, 18),
                source,
                extra: (size == NEW_ITEM_BYTES).then(|| PlaylistItemExtra {
                    id: u32_at(record, 32),
                    fade_in: float(36),
                    fade_in_tension: float(40),
                    fade_out: float(44),
                    fade_out_tension: float(48),
                    gain: float(52),
                    fade_flags: u32_at(record, 56),
                }),
            });
        }
        if off_the_playlist > 0 {
            self.note(
                Some(event.offset),
                windfall_core::notice!("flp.read.clipsOffPlaylist", count = off_the_playlist)
                    .to_string(),
            );
        }
        if let Some(arrangement) = self.project.arrangements.get_mut(index) {
            arrangement.items.extend(items);
        }
    }

    fn track(&mut self, event: &Event<'_>, data: &[u8]) {
        let index = self.arrangement_or_first();
        if data.len() < 17 {
            self.note(
                Some(event.offset),
                windfall_core::notice!("flp.read.playlistTrackShort").to_string(),
            );
            return;
        }
        let Some(arrangement) = self.project.arrangements.get_mut(index) else {
            return;
        };
        if arrangement.tracks.len() >= MAX_TRACKS {
            self.over("playlist tracks");
            return;
        }
        arrangement.tracks.push(Track {
            iid: u32_at(data, 0),
            name: None,
            color: Some(color(u32_at(data, 4))),
            enabled: data[12] != 0,
            height: f32::from_bits(u32_at(data, 13)),
        });
    }

    fn finish(mut self) -> FlpProject {
        self.finish_insert();
        self.apply_mixer_controls();
        let settings = &mut self.project.settings;
        if settings.tempo_millibpm.is_none()
            && let (Some(whole), fraction) = self.legacy_tempo
        {
            settings.tempo_millibpm =
                Some(u32::from(whole) * 1000 + u32::from(fraction.unwrap_or(0)));
        }
        self.project.patterns.sort_by_key(|pattern| pattern.iid);
        for (what, count) in std::mem::take(&mut self.over_cap) {
            self.note(
                None,
                windfall_core::notice!(
                    "flp.read.overCap",
                    what = windfall_core::notice::term(what),
                    count = count
                )
                .to_string(),
            );
        }
        if self.dropped_diagnostics > 0 {
            let count = self.dropped_diagnostics;
            self.project.diagnostics.push(Diagnostic {
                offset: None,
                message: windfall_core::notice!("flp.read.moreProblems", count = count).to_string(),
            });
        }
        self.project
    }

    /// Hands the mixer's control values to the inserts, slots and routes
    /// they are about.
    fn apply_mixer_controls(&mut self) {
        let mixer = &mut self.project.mixer;
        for control in std::mem::take(&mut self.mixer_controls) {
            let value = control.value;
            match control.target() {
                ControlTarget::Insert { insert, param } => {
                    let Some(insert) = mixer.inserts.get_mut(usize::from(insert)) else {
                        continue;
                    };
                    let band = |band: u8| usize::from(band.min(2));
                    match param {
                        InsertParam::Volume => insert.volume = Some(value),
                        InsertParam::Pan => insert.pan = Some(value),
                        InsertParam::StereoSeparation => insert.stereo_separation = Some(value),
                        InsertParam::EqGain(index) => insert.eq[band(index)].gain = Some(value),
                        InsertParam::EqFrequency(index) => {
                            insert.eq[band(index)].frequency = Some(value);
                        }
                        InsertParam::EqWidth(index) => insert.eq[band(index)].width = Some(value),
                        InsertParam::Other(_) => {}
                    }
                }
                ControlTarget::Slot {
                    insert,
                    slot,
                    param,
                } => {
                    let slot = mixer
                        .inserts
                        .get_mut(usize::from(insert))
                        .and_then(|insert| {
                            insert
                                .slots
                                .iter_mut()
                                .find(|filled| u16::from(filled.index) == slot)
                        });
                    match (slot, param) {
                        (Some(slot), SlotParam::Enabled) => slot.enabled = Some(value != 0),
                        (Some(slot), SlotParam::Mix) => slot.mix = Some(value),
                        _ => {}
                    }
                }
                ControlTarget::Route { insert, target } => {
                    let route = mixer
                        .inserts
                        .get_mut(usize::from(insert))
                        .and_then(|insert| {
                            insert
                                .routes
                                .iter_mut()
                                .find(|route| route.target == target)
                        });
                    if let Some(route) = route {
                        route.level = Some(value);
                    }
                }
                _ => mixer.other_controls.push(control),
            }
        }
    }
}

/// The pattern of a pattern block, if a playlist item is one.
///
/// Up to version 11 the playlist had, besides its clip tracks, a block area
/// with one row for each pattern. A block is stored like a clip, with a
/// row number that no clip track has: 999 less the pattern's number.
///
/// Observed, in three projects saved by FL Studio 9: the rows of the blocks
/// run from 998 down, the lowest row is 999 less the highest pattern
/// number, and a new project's one block of pattern 9 has row 990. LMMS's
/// importer reads an older layout the same way, with another constant.
fn legacy_block(tall: bool, base: u16, item: u16, row: u16) -> Option<u16> {
    const ROWS: u16 = 999;
    (!tall && item > base && (199..ROWS).contains(&row)).then(|| ROWS - row)
}

/// A colour as the file stores it, red in the lowest byte, to 0xRRGGBB.
/// The top byte, which some events use for something else, is dropped.
///
/// Agreed (PyFLP and DawVert read the bytes as red, green, blue).
fn color(raw: u32) -> u32 {
    let [red, green, blue, _] = raw.to_le_bytes();
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

fn control_values(data: &[u8]) -> impl Iterator<Item = ControlValue> + '_ {
    data.as_chunks::<CONTROL_BYTES>()
        .0
        .iter()
        .map(|record| ControlValue {
            location: u32_at(record, 4),
            value: u32_at(record, 8) as i32,
        })
}

fn channel_params(data: &[u8]) -> ChannelParams {
    let dword = |at: usize| (data.len() >= at + 4).then(|| u32_at(data, at));
    let double = |at: usize| {
        let bytes: [u8; 8] = data.get(at..at + 8)?.try_into().ok()?;
        Some(f64::from_le_bytes(bytes))
    };
    ChannelParams {
        main_pitch: data.get(11).map(|b| *b != 0),
        add_root: data.get(63).map(|b| *b != 0),
        key_region: dword(68).zip(dword(72)),
        remove_dc: data.get(9).map(|b| *b != 0),
        normalize: data.get(80).map(|b| *b != 0),
        inverted: data.get(81).map(|b| *b != 0),
        declick_mode: data.get(83).copied(),
        crossfade: dword(84),
        trim: dword(88),
        midi_channel_through: dword(112),
        sample_offset: double(144),
        stretch_formant: double(160).map(|v| (v - 0.5) * 24.0),
        fix_trim: data.get(157).map(|b| *b != 0),
        drum_stretch: [20, 24, 28, 32, 36].map(|at| dword(at).map(f32::from_bits)),
        arp_direction: dword(40),
        arp_range: dword(44),
        arp_chord: dword(48),
        arp_time: dword(52),
        arp_gate: dword(56),
        arp_slide: data.get(60).map(|b| *b != 0),
        full_porta: data.get(62).map(|b| *b != 0),
        gate: data
            .get(64..66)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap())),
        arp_repeat: dword(92),
        delay_flags: data.get(10).copied(),
        stretch_time: dword(96),
        stretch_pitch: dword(100).map(|value| value as i32),
        stretch_multiplier: dword(104).map(|value| value as i32),
        stretch_mode: dword(108).map(|value| value as i32),
        sample_start: double(128),
        sample_length: double(136),
    }
}

fn envelope(data: &[u8]) -> Option<EnvelopeLfo> {
    if data.len() < 36 {
        return None;
    }
    let optional = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    Some(EnvelopeLfo {
        flags: u32_at(data, 0),
        enabled: u32_at(data, 4) != 0,
        predelay: u32_at(data, 8),
        attack: u32_at(data, 12),
        hold: u32_at(data, 16),
        decay: u32_at(data, 20),
        sustain: u32_at(data, 24),
        release: u32_at(data, 28),
        amount: u32_at(data, 32) as i32,
        lfo_predelay: optional(36),
        lfo_attack: optional(40),
        lfo_amount: optional(44).map(|v| v as i32),
        lfo_speed: optional(48),
        lfo_shape: optional(52).map(|v| v as i32),
        attack_tension: optional(56).map(|v| v as i32),
        decay_tension: optional(60).map(|v| v as i32),
        release_tension: optional(64).map(|v| v as i32),
    })
}

fn automation_curve(data: &[u8]) -> Option<AutomationCurve> {
    if data.len() < CURVE_HEADER_BYTES {
        return None;
    }
    let points = &data[CURVE_HEADER_BYTES..];
    // The count is the file's word, so no more points are read than there
    // are bytes for.
    let count = (u32_at(data, 17) as usize).min(points.len() / POINT_BYTES);
    let double = |record: &[u8], at: usize| {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&record[at..at + 8]);
        f64::from_le_bytes(bytes)
    };
    Some(AutomationCurve {
        points: points
            .as_chunks::<POINT_BYTES>()
            .0
            .iter()
            .take(count)
            .map(|record| AutomationPoint {
                offset: double(record, 0),
                value: double(record, 8),
                tension: f32::from_bits(u32_at(record, 16)),
                mode: u16_at(record, 20),
            })
            .collect(),
    })
}
