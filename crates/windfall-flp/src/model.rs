//! What an FL Studio project file says, as plain data.
//!
//! [`parse`](crate::parse) fills these types from the events of a file.
//! Values are kept in the file's own units: ticks at the file's time base,
//! levels as the integers FL Studio stores. [`convert`](crate::convert)
//! turns them into Windfall's units, and the functions in
//! [`units`](crate::units) say how.
//!
//! A field that is `None` had no event in the file. Nothing is filled in
//! with a guess here.
//!
//! # How sure each part is
//!
//! Every type says where its layout comes from. Three levels are used:
//!
//! - **Agreed**: at least two of the sources read it the same way, and the
//!   files this crate was run over fit.
//! - **One source**: one source reads it, and nothing contradicts it.
//! - **Disputed**: the sources disagree. The comment says which reading
//!   was taken and why.
//!
//! The sources are PyFLP, DawVert, LMMS's importer and FLParser. The crate
//! documentation gives their links and licenses.

use std::collections::BTreeMap;

pub use crate::event::{FileFormat, Header};
use crate::target::ControlTarget;

/// A whole file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlpProject {
    /// Bounded original event stream (except licensee metadata). This is archival
    /// data, never executed, and includes controls whose semantics are unknown.
    pub source_event_stream: Vec<u8>,
    pub header: Option<Header>,
    /// The version of FL Studio that saved the file, as the file spells it:
    /// "20.8.4.2576". Event 199. Agreed.
    pub version_text: Option<String>,
    pub settings: Settings,
    /// The channel rack, in the order the file gives the channels.
    pub channels: Vec<Channel>,
    /// Names of the channel rack's groups, in order. Event 231. Agreed.
    pub channel_groups: Vec<String>,
    /// The patterns, in order of their numbers.
    pub patterns: Vec<Pattern>,
    pub mixer: Mixer,
    /// The arrangements, in the order the file gives them. Files from
    /// before FL Studio 12.9 have one, which the file does not announce.
    pub arrangements: Vec<Arrangement>,
    /// The arrangement that was selected. Event 100. One source (PyFLP).
    pub current_arrangement: Option<u16>,
    /// Links from a controller, such as an automation clip, to what it
    /// moves. Event 227.
    pub remote_controllers: Vec<RemoteController>,
    /// The values controls of channels start at. Event 216. One source
    /// (DawVert), and the layout is that of the mixer's event 225.
    pub initial_controls: Vec<ControlValue>,
    /// Events this crate does not interpret, in file order, with their
    /// values. No more than [`MAX_KEPT_EVENTS`](crate::parse::MAX_KEPT_EVENTS)
    /// are kept; `uninterpreted_counts` counts all of them.
    pub uninterpreted: Vec<RawEvent>,
    /// How often each uninterpreted event id occurred.
    pub uninterpreted_counts: BTreeMap<u8, u32>,
    /// What went wrong while reading, in file order.
    pub diagnostics: Vec<Diagnostic>,
}

impl FlpProject {
    /// The version as numbers. `None` when the file has no version or it
    /// does not start with a number.
    pub fn version(&self) -> Option<FlVersion> {
        FlVersion::parse(self.version_text.as_deref()?)
    }

    /// The time base of the file in ticks per quarter note. 96 when the
    /// project was built without a header.
    pub fn ppq(&self) -> u16 {
        self.header.map_or(96, |header| header.ppq)
    }

    pub fn channel(&self, iid: u16) -> Option<&Channel> {
        self.channels.iter().find(|channel| channel.iid == iid)
    }

    pub fn pattern(&self, iid: u16) -> Option<&Pattern> {
        self.patterns.iter().find(|pattern| pattern.iid == iid)
    }

    /// The arrangement a song is made of: the selected one, or the first.
    pub fn main_arrangement(&self) -> Option<&Arrangement> {
        self.current_arrangement
            .and_then(|index| self.arrangements.iter().find(|a| a.index == index))
            .or(self.arrangements.first())
    }

    /// The ids of the uninterpreted events that none of the sources has a
    /// name for.
    pub fn unknown_ids(&self) -> Vec<u8> {
        self.uninterpreted_counts
            .keys()
            .copied()
            .filter(|&id| crate::names::event_name(id).is_none())
            .collect()
    }
}

/// A version of FL Studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: u32,
}

impl FlVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            build: 0,
        }
    }

    /// Reads "20.8.4.2576". Parts that are missing count as 0, and a part
    /// that is not a number ends the version there.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.').map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        });
        let major = parts.next()??;
        let mut rest = [0_u32; 3];
        for slot in &mut rest {
            match parts.next() {
                Some(Some(number)) => *slot = number,
                _ => break,
            }
        }
        Some(Self {
            major,
            minor: rest[0],
            patch: rest[1],
            build: rest[2],
        })
    }
}

impl std::fmt::Display for FlVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if self.build != 0 {
            write!(formatter, ".{}", self.build)?;
        }
        Ok(())
    }
}

/// Settings of the whole project.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    /// Tempo in thousandths of a beat per minute. Event 156, or the whole
    /// beats of event 66 with the thousandths of event 93 in files from
    /// before it. Agreed (PyFLP, DawVert, FLParser).
    pub tempo_millibpm: Option<u32>,
    /// Beats in a bar. Event 17. Agreed.
    pub numerator: Option<u8>,
    /// Event 18. From FL Studio 20 on it is the beat unit of the time
    /// signature. Before that it is the number of steps in a beat, which
    /// FL Studio let the user set. One source (PyFLP) for both meanings;
    /// the files this crate was run over fit.
    pub denominator: Option<u8>,
    /// Event 194. Agreed.
    pub title: Option<String>,
    /// Event 207. Agreed.
    pub author: Option<String>,
    /// Event 206. Agreed.
    pub genre: Option<String>,
    /// Event 195, or event 198 in older files, which hold Rich Text there.
    /// Agreed.
    pub comments: Option<String>,
    /// The comments were Rich Text (event 198) and are given as written.
    pub comments_are_rtf: bool,
    /// Event 197. Agreed.
    pub url: Option<String>,
    /// The folder FL Studio keeps the project's data in. Event 202. Agreed.
    pub data_path: Option<String>,
    /// Master pitch in cents. Event 80. Agreed.
    pub main_pitch: Option<i16>,
    /// Main volume as files before the mixer's control values stored it.
    /// Event 12. One source (LMMS: 128 is full).
    pub main_volume: Option<u8>,
    /// Channel rack swing, 0 to 128. Event 11. Agreed (PyFLP, DawVert).
    pub swing: Option<u8>,
    /// 0 is circular, 2 is triangular. Event 23. One source (PyFLP).
    pub pan_law: Option<u8>,
    /// A part of the playlist was selected to loop. Event 9. Agreed.
    pub loop_active: Option<bool>,
    /// Notes that a clip cuts into still play. Event 30. Agreed.
    pub play_truncated_notes: Option<bool>,
    /// The pattern that was selected. Event 67. Agreed.
    pub current_pattern: Option<u16>,
}

impl Settings {
    /// The tempo in beats per minute.
    pub fn tempo_bpm(&self) -> Option<f64> {
        self.tempo_millibpm
            .map(|millibpm| f64::from(millibpm) / 1000.0)
    }
}

/// What kind of thing a channel is. Event 21.
///
/// Disputed: PyFLP calls 4 an instrument and 2 "native"; DawVert reads 2 as
/// a generator plugin and 4 as an audio clip. DawVert's reading is taken:
/// in the files this crate was run over, every channel of type 4 has a
/// sample and no plugin, and sits on the playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelKind {
    /// The built-in sampler.
    #[default]
    Sampler,
    /// A generator plugin: one of FL Studio's own, or a hosted one.
    Generator,
    /// A layer, which plays other channels.
    Layer,
    /// An audio clip.
    AudioClip,
    /// An automation clip.
    AutomationClip,
    /// A value the sources do not name. 1 was the TS404 bass synth of early
    /// versions (LMMS).
    Other(u8),
}

impl ChannelKind {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => ChannelKind::Sampler,
            2 => ChannelKind::Generator,
            3 => ChannelKind::Layer,
            4 => ChannelKind::AudioClip,
            5 => ChannelKind::AutomationClip,
            other => ChannelKind::Other(other),
        }
    }

    pub fn raw(self) -> u8 {
        match self {
            ChannelKind::Sampler => 0,
            ChannelKind::Generator => 2,
            ChannelKind::Layer => 3,
            ChannelKind::AudioClip => 4,
            ChannelKind::AutomationClip => 5,
            ChannelKind::Other(raw) => raw,
        }
    }
}

/// One channel of the channel rack.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Channel {
    /// The channel's number, which notes, playlist items and links use.
    /// Event 64. Agreed.
    pub iid: u16,
    pub kind: ChannelKind,
    /// Event 203, or event 192 in older files. Agreed.
    pub name: Option<String>,
    /// As 0xRRGGBB. Event 128. Agreed.
    pub color: Option<u32>,
    /// Off is muted. Event 0. Agreed.
    pub enabled: Option<bool>,
    /// The plugin of a generator channel.
    pub plugin: Option<Plugin>,
    /// Where the sample of a sampler or audio clip is, as FL Studio wrote
    /// it. Event 196. Agreed.
    pub sample_path: Option<String>,
    /// 0 to 12800. Event 219, bytes 4 to 8. Agreed (PyFLP, DawVert).
    pub volume: Option<u32>,
    /// 0 is left, 6400 the middle, 12800 right. Event 219, bytes 0 to 4.
    /// Agreed (PyFLP, DawVert).
    pub pan: Option<u32>,
    /// Pitch in cents. Event 219, bytes 8 to 12. Agreed (PyFLP, DawVert).
    pub pitch: Option<i32>,
    /// The mixer insert the channel plays into: 0 is the master. Event 22.
    /// Agreed (DawVert, FLParser; PyFLP adds that -1 means the selected
    /// insert).
    pub insert: Option<i8>,
    /// The cut groups. Event 132. One source (PyFLP).
    pub cut: Option<CutGroups>,
    /// Per-channel share of rack swing, 0..128. Event 97 (PyFLP).
    pub swing: Option<u16>,
    /// Nonlinear fine shift. Event 89 (PyFLP); retained, not guessed.
    pub time_shift: Option<u16>,
    /// Semitone range for channel pitch control. Event 83 (PyFLP).
    pub pitch_range: Option<u16>,
    /// Note echo, event 209 (PyFLP DelayEvent and DawVert).
    pub echo: Option<ChannelEcho>,
    /// The key that plays a sample at its own pitch, where 60 is the key
    /// FL Studio calls C5. Event 135. Agreed (PyFLP, DawVert).
    pub root_note: Option<u32>,
    /// Bit 1 plays the sample backwards and bit 8 swaps left and right.
    /// Event 70. Agreed (PyFLP, DawVert, LMMS).
    pub fx_flags: Option<u16>,
    /// Bit 3 loops the sample between its loop points. Event 143. Agreed
    /// (PyFLP, DawVert).
    pub sampler_flags: Option<u32>,
    /// Gain before the sampler's effects, 0 to 256. Event 74. One source
    /// (PyFLP).
    pub preamp: Option<u16>,
    /// Event 215.
    pub params: Option<ChannelParams>,
    /// The envelopes and LFOs of the sampler, in the order FL Studio
    /// writes them: panning, volume, mod X, mod Y, pitch. Event 218, once
    /// for each. Agreed on the layout (PyFLP, DawVert, LMMS); which is
    /// which comes from PyFLP and DawVert.
    pub envelopes: Vec<EnvelopeLfo>,
    /// Event 221. Agreed (PyFLP, DawVert).
    pub polyphony: Option<Polyphony>,
    /// The channels a layer plays. Event 94, once for each. Agreed.
    pub layer_children: Vec<u16>,
    /// The curve of an automation clip. Event 234.
    pub automation: Option<AutomationCurve>,
    /// The channel group the channel is in. Event 145. One source (PyFLP).
    pub group: Option<i32>,
}

impl Channel {
    /// The name to show: the given one, or else the plugin's.
    pub fn display_name(&self) -> Option<&str> {
        let named = |name: &&str| !name.trim().is_empty();
        self.name.as_deref().filter(named).or_else(|| {
            self.plugin
                .as_ref()
                .map(|plugin| plugin.internal_name.as_str())
                .filter(named)
        })
    }

    /// The sample plays backwards.
    pub fn reversed(&self) -> bool {
        self.fx_flags.is_some_and(|flags| flags & 0x2 != 0)
    }

    /// The envelope that shapes the volume, if the file has one.
    pub fn volume_envelope(&self) -> Option<&EnvelopeLfo> {
        self.envelopes.get(1)
    }
}

/// The two cut group numbers of a channel. A channel stops the channels
/// that are cut by the group it cuts. Both numbers the same makes the
/// channel cut itself too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CutGroups {
    /// The group this channel cuts. The low half of the event.
    pub cuts: u16,
    /// The group that cuts this channel. The high half of the event.
    pub cut_by: u16,
}

/// The fields of event 215 that this crate reads. The event has grown from
/// version to version, and fields past the end of a shorter one are `None`.
///
/// Agreed (PyFLP, DawVert) up to the stretch mode. The three 64-bit numbers
/// from byte 128 are DawVert's reading; PyFLP reads four of the same bytes
/// as a pair of 16-bit numbers and decodes them to the same value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChannelParams {
    /// PyFLP ParametersEvent / DawVert channel.py, bytes 40..64 and 92..96.
    pub arp_direction: Option<u32>,
    pub arp_range: Option<u32>,
    pub arp_chord: Option<u32>,
    /// Disputed: DawVert reads u32, PyFLP float32. Preserve the raw word.
    pub arp_time: Option<u32>,
    pub arp_gate: Option<u32>,
    pub arp_slide: Option<bool>,
    pub arp_repeat: Option<u32>,
    pub full_porta: Option<bool>,
    pub gate: Option<u16>,
    pub delay_flags: Option<u8>,
    /// How long the stretched sample is to last. Bytes 96 to 100.
    pub stretch_time: Option<u32>,
    /// Pitch shift of the time stretcher in cents. Bytes 100 to 104.
    pub stretch_pitch: Option<i32>,
    /// Speed of the time stretcher: 2 to the power of this over 10000.
    /// Bytes 104 to 108.
    pub stretch_multiplier: Option<i32>,
    /// Which time stretcher. 0 resamples, which changes pitch and length
    /// together. Bytes 108 to 112.
    pub stretch_mode: Option<i32>,
    /// Where in the sample playback starts, 0 to 1. Bytes 128 to 136.
    pub sample_start: Option<f64>,
    /// How much of what follows the start plays, 0 to 1. Bytes 136 to 144.
    pub sample_length: Option<f64>,
}

/// One envelope and LFO of the sampler. Event 218, 68 bytes of 32-bit
/// numbers. Only the envelope is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvelopeLfo {
    /// Bit 0 follows the tempo.
    pub flags: u32,
    pub enabled: bool,
    /// The times run from 100 to 65536 on FL Studio's own scale.
    pub predelay: u32,
    pub attack: u32,
    pub hold: u32,
    pub decay: u32,
    /// 0 to 128.
    pub sustain: u32,
    pub release: u32,
    /// -128 to 128.
    pub amount: i32,
    /// Optional tails of event 218; PyFLP EnvelopeLFOEvent, bytes 36..68.
    pub lfo_predelay: Option<u32>,
    pub lfo_attack: Option<u32>,
    pub lfo_amount: Option<i32>,
    pub lfo_speed: Option<u32>,
    pub lfo_shape: Option<i32>,
    pub attack_tension: Option<i32>,
    pub decay_tension: Option<i32>,
    pub release_tension: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelEcho {
    pub feedback: u32,
    pub pan: i32,
    pub pitch_cents: i32,
    pub repeats: u32,
    pub time: u32,
}

/// Event 221.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Polyphony {
    /// Most voices at once. 0 is no limit.
    pub max: u32,
    /// Glide time on FL Studio's own scale.
    pub slide: u32,
    /// Bit 0 is mono and bit 1 is portamento.
    pub flags: u8,
}

/// A plugin on a channel or in an effect slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plugin {
    /// The name FL Studio knows the plugin by, such as "Fruity Wrapper"
    /// for every hosted plugin. Event 201. Agreed.
    pub internal_name: String,
    /// The plugin makes sound instead of processing it. Event 212, bit 4
    /// of the 32-bit number at byte 16. Agreed (PyFLP, DawVert).
    pub generator: Option<bool>,
    /// Everything the plugin saved. Event 213. Agreed. The functions in
    /// [`plugin`](crate::plugin) read the states this crate understands.
    pub state: Vec<u8>,
}

/// The curve of an automation clip. Event 234.
///
/// Agreed on the layout (PyFLP, DawVert, FLParser): 17 bytes, a count, and
/// 24 bytes a point. What follows the points is not read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutomationCurve {
    pub points: Vec<AutomationPoint>,
}

/// One point of an automation clip.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AutomationPoint {
    /// How far after the point before it this one lies, in quarter notes.
    /// Agreed (PyFLP, DawVert, FLParser).
    pub offset: f64,
    /// 0 to 1 across the range of what the clip moves. Agreed.
    pub value: f64,
    /// The bend of the stretch that arrives at this point. One source
    /// (DawVert, which reads the same record in FL Studio's envelopes and
    /// attaches both this and the mode to the stretch before the point).
    pub tension: f32,
    /// The shape of the stretch that arrives at this point. 0 is a single
    /// curve and 2 holds the value of the point before. One source
    /// (DawVert).
    pub mode: u16,
}

/// A pattern.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pattern {
    /// The pattern's number, from 1. Event 65. Agreed.
    pub iid: u16,
    /// Event 193. Agreed.
    pub name: Option<String>,
    /// As 0xRRGGBB. Event 150. Agreed.
    pub color: Option<u32>,
    /// A length that was set by hand, in ticks. 0 or absent leaves the
    /// length to the notes. Event 164. One source (PyFLP).
    pub length: Option<u32>,
    /// Event 224. Agreed (PyFLP, DawVert).
    pub notes: Vec<Note>,
    /// Steps of the step sequencer as versions before 3.3 stored them.
    /// Event 91. One source (LMMS).
    pub legacy_steps: Vec<LegacyStep>,
    /// Changes of controls recorded into the pattern. Event 223. Agreed on
    /// the 12-byte layout (PyFLP, DawVert, LMMS); they are kept and not
    /// converted.
    pub control_events: Vec<ControlEvent>,
    /// Markers on the pattern's own timeline.
    pub markers: Vec<TimeMarker>,
}

/// A note. 24 bytes of event 224.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Note {
    /// Start in ticks from the start of the pattern.
    pub position: u32,
    /// Bit 3 makes the note a slide.
    pub flags: u16,
    /// The number of the channel that plays it.
    pub channel: u16,
    /// Length in ticks. 0 for a step of the step sequencer.
    pub length: u32,
    /// 0 to 131, where 60 is the key FL Studio calls C5.
    pub key: u16,
    pub group: u16,
    /// 0 to 240 in steps of 10 cents, with 120 in tune.
    pub fine_pitch: u8,
    /// 0 to 128.
    pub release: u8,
    pub midi_channel: u8,
    /// 0 is left, 64 the middle, 128 right.
    pub pan: u8,
    /// 0 to 128, 100 when drawn.
    pub velocity: u8,
    pub mod_x: u8,
    pub mod_y: u8,
}

impl Note {
    /// The note was made in the step sequencer.
    pub fn is_step(&self) -> bool {
        self.length == 0
    }

    pub fn is_slide(&self) -> bool {
        self.flags & 0x8 != 0
    }
}

/// A step of the step sequencer in the oldest files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyStep {
    pub channel: u16,
    /// The value of the event. Its low byte is the step.
    pub raw: u16,
}

impl LegacyStep {
    pub fn step(&self) -> u8 {
        (self.raw & 0xFF) as u8
    }
}

/// A recorded change of a control. 12 bytes of event 223.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlEvent {
    pub position: u32,
    /// What changes. [`ControlTarget::decode`] reads it.
    pub location: u32,
    pub value: i32,
}

/// A value a control starts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlValue {
    /// What it is the value of. [`ControlTarget::decode`] reads it.
    pub location: u32,
    pub value: i32,
}

impl ControlValue {
    pub fn target(&self) -> ControlTarget {
        ControlTarget::decode(self.location)
    }
}

/// A link from a controller to what it moves. Event 227, 20 bytes.
///
/// Disputed: PyFLP puts the target at bytes 4 to 8. DawVert and FLParser
/// put the controller at bytes 2 to 6 and the target at bytes 8 to 12, and
/// in the files this crate was run over only that reading gives targets
/// that exist. It is the one taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteController {
    /// The controller. Bytes 2 to 6. For an automation clip it is the
    /// channel number; a controller plugin in the mixer is written the way
    /// [`ControlTarget`] writes a slot.
    pub source: u32,
    /// What it moves. Bytes 8 to 12.
    pub location: u32,
    /// Bytes 12 to 16.
    pub flags: u32,
    /// Bytes 16 to 20.
    pub smoothing: u32,
}

impl RemoteController {
    /// The channel of the automation clip that is the controller, if it is
    /// one.
    pub fn source_channel(&self) -> Option<u16> {
        u16::try_from(self.source).ok().filter(|&iid| iid < 0x2000)
    }

    pub fn target(&self) -> ControlTarget {
        ControlTarget::decode(self.location)
    }
}

/// The mixer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mixer {
    /// The inserts in the order the file gives them. The first is the
    /// master. Agreed (DawVert, FLParser, and the files this crate was run
    /// over; PyFLP numbers them one lower).
    pub inserts: Vec<Insert>,
    /// Plugin delay compensation is on. Event 29. One source (PyFLP).
    pub delay_compensation: Option<bool>,
    /// Records of the mixer's control values that are not about an insert
    /// this crate knows: the main volume among them. Event 225.
    pub other_controls: Vec<ControlValue>,
}

impl Mixer {
    /// The main volume, where 12800 is full. One source (DawVert).
    pub fn main_volume(&self) -> Option<i32> {
        self.other_controls
            .iter()
            .find(|control| {
                control.target() == ControlTarget::Main(crate::target::MainParam::Volume)
            })
            .map(|control| control.value)
    }
}

/// One mixer insert.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Insert {
    /// Event 204. Agreed.
    pub name: Option<String>,
    /// As 0xRRGGBB. Event 149. Agreed.
    pub color: Option<u32>,
    /// Event 236, bytes 4 to 8, or event 27 in files that have it. See
    /// the methods for the bits. Agreed (PyFLP, DawVert, FLParser; LMMS for
    /// event 27).
    pub flags: Option<u32>,
    /// The fader: 12800 is 100% and 16000 the top. Event 225. Agreed.
    pub volume: Option<i32>,
    /// -6400 is left and 6400 right. Event 225. Agreed.
    pub pan: Option<i32>,
    /// -64 to 64. Event 225. Agreed (PyFLP, FLParser).
    pub stereo_separation: Option<i32>,
    /// The three bands of the insert's own equaliser. Event 225.
    pub eq: [InsertEqBand; 3],
    /// The inserts this one feeds. Event 235 says which, and event 225 at
    /// what level. Agreed (PyFLP, DawVert, FLParser).
    pub routes: Vec<Route>,
    /// The effect slots that hold a plugin, in order.
    pub slots: Vec<Slot>,
    /// The audio input. Event 154. One source (PyFLP).
    pub input: Option<i32>,
    /// The audio output. Event 147. One source (PyFLP).
    pub output: Option<i32>,
}

impl Insert {
    /// Off is muted. Bit 3 of the flags; on when the file has no flags.
    pub fn enabled(&self) -> bool {
        self.flags.is_none_or(|flags| flags & 0x8 != 0)
    }

    /// Bit 12 of the flags.
    pub fn solo(&self) -> bool {
        self.flags.is_some_and(|flags| flags & 0x1000 != 0)
    }

    /// The effect slots are switched on. Bit 2 of the flags.
    pub fn effects_enabled(&self) -> bool {
        self.flags.is_none_or(|flags| flags & 0x4 != 0)
    }

    /// Bit 0 of the flags.
    pub fn polarity_reversed(&self) -> bool {
        self.flags.is_some_and(|flags| flags & 0x1 != 0)
    }

    /// Bit 1 of the flags.
    pub fn channels_swapped(&self) -> bool {
        self.flags.is_some_and(|flags| flags & 0x2 != 0)
    }
}

/// One band of an insert's equaliser. All three values come from event
/// 225. Agreed (PyFLP, DawVert, FLParser).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InsertEqBand {
    /// -1800 to 1800, in hundredths of a dB.
    pub gain: Option<i32>,
    /// 0 to 65536 on FL Studio's own scale.
    pub frequency: Option<i32>,
    /// 0 to 65536.
    pub width: Option<i32>,
}

/// A connection from one insert to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    /// The insert that is fed. 0 is the master.
    pub target: u16,
    /// 12800 is 100%. `None` when the file gives no level, which FL Studio
    /// takes as 100%.
    pub level: Option<i32>,
}

/// An effect slot with a plugin in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slot {
    /// 0 to 9. Event 98, or bytes 4 to 8 of event 212 in files without
    /// it. Agreed (DawVert, FLParser, and the files this crate was run
    /// over).
    pub index: u8,
    pub plugin: Plugin,
    /// Event 203. Agreed.
    pub name: Option<String>,
    /// As 0xRRGGBB. Event 128. Agreed.
    pub color: Option<u32>,
    /// Event 225. Agreed (PyFLP, DawVert, FLParser).
    pub enabled: Option<bool>,
    /// 12800 is fully wet. Event 225. Agreed (DawVert, FLParser).
    pub mix: Option<i32>,
}

/// One arrangement: a playlist with its tracks and markers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arrangement {
    /// Event 99. Agreed (PyFLP, DawVert).
    pub index: u16,
    /// Event 241. Agreed.
    pub name: Option<String>,
    /// Event 233. Agreed on the first 32 bytes of an item (PyFLP, DawVert,
    /// FLParser).
    pub items: Vec<PlaylistItem>,
    /// Pattern blocks as versions before 4 stored them. Event 129. One
    /// source (LMMS).
    pub legacy_items: Vec<LegacyPlaylistItem>,
    /// Event 238, with the name from event 239. Agreed (PyFLP, DawVert).
    pub tracks: Vec<Track>,
    pub markers: Vec<TimeMarker>,
}

/// A clip on the playlist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaylistItem {
    /// Start in ticks.
    pub position: u32,
    /// Length in ticks.
    pub length: u32,
    /// The track, counted from the top, from 0. The file counts from the
    /// bottom: 499 is the top track from FL Studio 12.9.1 on, and 198
    /// before. Agreed (PyFLP, DawVert).
    pub track: u16,
    pub group: u16,
    /// Bit 12 mutes the clip.
    ///
    /// Disputed: FLParser takes bit 13 for it. DawVert takes bit 12, and
    /// a song this crate was run over has bit 13 on every clip, so bit 12
    /// it is.
    pub flags: u16,
    pub source: PlaylistSource,
    /// What only FL Studio 21 and later store.
    pub extra: Option<PlaylistItemExtra>,
}

impl PlaylistItem {
    pub fn muted(&self) -> bool {
        self.flags & 0x1000 != 0
    }
}

/// What a playlist item plays, and the window it shows of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaylistSource {
    /// A pattern. The window is in ticks from the start of the pattern.
    /// Agreed (DawVert, FLParser).
    Pattern {
        pattern: u16,
        start: Option<u32>,
        end: Option<u32>,
    },
    /// A channel: an audio clip or an automation clip. The window is
    /// stored as 32-bit floats. For an automation clip they are quarter
    /// notes (DawVert, FLParser). For an audio clip DawVert takes them as
    /// four units per quarter note before stretching, and FLParser as
    /// quarter notes; DawVert's reading is converted approximately and reported.
    Channel {
        channel: u16,
        start: Option<f32>,
        end: Option<f32>,
    },
}

/// The 28 bytes FL Studio 21 added to a playlist item. One source
/// (DawVert).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaylistItemExtra {
    pub id: u32,
    /// Fade in, in milliseconds.
    pub fade_in: f32,
    pub fade_in_tension: f32,
    /// Fade out, in milliseconds.
    pub fade_out: f32,
    pub fade_out_tension: f32,
    /// Gain of the clip, 1 for unchanged.
    pub gain: f32,
    pub fade_flags: u32,
}

/// A pattern block of the oldest playlists: one bar of one pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPlaylistItem {
    /// The bar it sits on, from 0.
    pub bar: u16,
    /// The pattern, from 1.
    pub pattern: u16,
}

/// A track of the playlist. Event 238.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    /// The track's number, from 1 at the top. Bytes 0 to 4.
    pub iid: u32,
    /// Event 239, which follows the track's data.
    pub name: Option<String>,
    /// As 0xRRGGBB. Bytes 4 to 8.
    pub color: Option<u32>,
    /// Off is muted. Byte 12.
    pub enabled: bool,
    /// 1 is the usual height. Bytes 13 to 17.
    pub height: f32,
}

/// A marker on a timeline. Event 148, with its name in event 205 and, for
/// a time signature, its two numbers in events 33 and 34.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeMarker {
    /// Position in ticks: the low 24 bits of the event. One source
    /// (DawVert; PyFLP subtracts the same top byte for a time signature).
    pub position: u32,
    /// The top 8 bits of the event. 0 is a plain marker, 8 a change of
    /// time signature (PyFLP, DawVert); DawVert names 1 to 5, 9 and 10 as
    /// well.
    pub kind: u8,
    pub name: Option<String>,
    pub numerator: Option<u8>,
    pub denominator: Option<u8>,
}

impl TimeMarker {
    pub fn is_time_signature(&self) -> bool {
        self.kind == 8
    }
}

/// An event that was kept as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub id: u8,
    pub value: RawValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    Byte(u8),
    Word(u16),
    DWord(u32),
    Data(Vec<u8>),
}

/// Something that went wrong while reading, which cost a part of the file
/// and not all of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where in the file, when that is known.
    pub offset: Option<usize>,
    /// What happened, in plain words.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_read_with_and_without_a_build() {
        assert_eq!(
            FlVersion::parse("20.8.4.2576"),
            Some(FlVersion {
                major: 20,
                minor: 8,
                patch: 4,
                build: 2576
            })
        );
        assert_eq!(FlVersion::parse("9.0.3"), Some(FlVersion::new(9, 0, 3)));
        assert_eq!(FlVersion::parse("12"), Some(FlVersion::new(12, 0, 0)));
        assert_eq!(
            FlVersion::parse("21.2 beta"),
            Some(FlVersion::new(21, 2, 0))
        );
        assert_eq!(FlVersion::parse(""), None);
        assert_eq!(FlVersion::parse("new"), None);
    }

    #[test]
    fn versions_order_by_their_parts() {
        assert!(FlVersion::new(12, 9, 1) > FlVersion::new(12, 9, 0));
        assert!(FlVersion::new(11, 5, 0) < FlVersion::new(20, 0, 0));
        assert_eq!(FlVersion::new(20, 8, 4).to_string(), "20.8.4");
    }
}
