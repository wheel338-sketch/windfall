//! Note expression as MIDI channel messages.
//!
//! A project note carries a fine pitch, two modulation controls, a pan and
//! an articulation. A MIDI note carries none of them: they are properties of
//! the channel the note plays on. This module holds the one mapping between
//! the two that export, the live hardware output and the live hardware input
//! share, and the pass that turns the notes of an export into controller
//! curves.
//!
//! # The mapping
//!
//! | Project | MIDI | Neutral |
//! | --- | --- | --- |
//! | Fine pitch, slide and portamento | Pitch bend, scaled by the bend range | 8192 |
//! | Modulation X | Controller 74 | 64 |
//! | Modulation Y | Controller 1, or channel pressure in MPE | 0 |
//! | Note pan | Controller 10, added to the channel's pan | the channel's pan |
//!
//! Modulation X is bipolar and controller 74 rests at 64 on every device, so
//! 0, 64 and 127 are 0, 0.5 and 1. Modulation Y goes to the modulation wheel
//! because that is the controller every keyboard has, and to channel pressure
//! in MPE because that is the third MPE dimension. Both rest at 0 on a real
//! device, so 0 is the neutral 0.5 and 127 is 1. A value below neutral has no
//! MIDI value: it is written as 0 and counted in the report.
//!
//! # Channel-voice mode
//!
//! Every message goes to the MIDI channel its note plays on. A channel has
//! one pitch bend and one value for each controller, so of the notes that
//! sound together on one channel the one that started last sets them, and
//! when it ends the one before it does again. Every other note that wanted
//! something else is counted in [`ExportReport::overlap_notes`]. When the
//! last note of a channel ends, everything it moved goes back to neutral.
//! A release tail that is still sounding moves with it: that is what one
//! bend for a whole channel means, and MPE is the way around it.
//!
//! Pan is different, because a pan that jumps under a held note is heard as
//! a fault and not as an approximation. A note's pan is written only when
//! no note with another pan sounds on its channel while it does. Otherwise
//! it is left out and counted in [`ExportReport::pan_omitted_notes`].
//!
//! # MPE mode
//!
//! The lower zone: channel 1 is the master and channels 2 to 16 each play
//! one note at a time, taken in turn so that the channel that has been
//! silent longest is used next. A colour group cannot choose the channel
//! then, and the notes that had one are counted. More than fifteen notes at
//! once share channels, and those are counted too.

use std::collections::BTreeMap;

use windfall_project::{NoteArticulation, NoteExpression, NoteExpressionCurve};

use crate::mix::pan_to_midi;
use crate::song::{CC_PAN, ControlCurve, ControlKind, ControlPoint, PITCH_BEND_CENTER};

/// Controller number of the modulation wheel, which carries modulation Y.
pub const CC_MODULATION: u8 = 1;
/// Controller number of "brightness", MPE's timbre, which carries
/// modulation X.
pub const CC_TIMBRE: u8 = 74;
/// Controller numbers that select and set a registered parameter.
pub const CC_DATA_ENTRY: u8 = 6;
pub const CC_DATA_ENTRY_FINE: u8 = 38;
pub const CC_RPN_FINE: u8 = 100;
pub const CC_RPN_COARSE: u8 = 101;
/// Registered parameter 0: how far a full pitch bend goes, in semitones.
pub const RPN_BEND_RANGE: u8 = 0;
/// Registered parameter 6: the MPE configuration message.
pub const RPN_MPE_CONFIGURATION: u8 = 6;

/// The bend range a device has until it is told otherwise.
pub const DEVICE_BEND_RANGE: u8 = 2;
/// The bend range channel-voice export sets: an octave either way, which
/// is the longest slide a piano roll usually draws.
pub const DEFAULT_BEND_RANGE: u8 = 12;
/// The bend range of an MPE member channel.
pub const MPE_BEND_RANGE: u8 = 48;
/// The widest bend range that can be asked for.
pub const MAX_BEND_RANGE: u8 = 96;
/// How often, in ticks, a moving value gets a new message.
pub const DEFAULT_EXPRESSION_STEP: u32 = 10;
/// The member channels of the lower MPE zone, counted from 0.
pub const MPE_MEMBERS: std::ops::RangeInclusive<u8> = 1..=15;
/// The most messages one note's movement is written in. A longer movement
/// gets longer steps.
const MAX_STEPS_PER_NOTE: u32 = 2_048;

/// How note expression is laid out on MIDI channels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExpressionMode {
    /// On the channel each note already plays on.
    #[default]
    ChannelVoice,
    /// A channel for each note, in the lower MPE zone.
    Mpe,
}

/// Choices for writing note expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionOptions {
    pub mode: ExpressionMode,
    /// Semitones a full pitch bend moves, 1 to [`MAX_BEND_RANGE`]. It is
    /// written to every channel that bends, as registered parameter 0. A
    /// note that moves further is held at the end of the range and counted.
    pub bend_range: u8,
    /// Ticks between two messages of a value that is moving: a slide, a
    /// glide or a saved curve. 0 counts as 1.
    pub step: u32,
}

impl ExpressionOptions {
    /// Channel-voice mode with a bend range of an octave.
    pub fn channel_voice() -> Self {
        Self {
            mode: ExpressionMode::ChannelVoice,
            bend_range: DEFAULT_BEND_RANGE,
            step: DEFAULT_EXPRESSION_STEP,
        }
    }

    /// MPE with the bend range MPE gives its member channels.
    pub fn mpe() -> Self {
        Self {
            mode: ExpressionMode::Mpe,
            bend_range: MPE_BEND_RANGE,
            step: DEFAULT_EXPRESSION_STEP,
        }
    }

    /// The bend range that is written.
    pub fn range(&self) -> u8 {
        self.bend_range.clamp(1, MAX_BEND_RANGE)
    }
}

impl Default for ExpressionOptions {
    fn default() -> Self {
        Self::channel_voice()
    }
}

/// What an export did with the expression of the project's notes. Nothing
/// in here is an error: each count is something a reader of the file will
/// not hear the way the project plays it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// How expression was written. `None` when the export left it out.
    pub mode: Option<ExpressionMode>,
    /// The bend range written to the channels that bend. 0 when expression
    /// was left out.
    pub bend_range: u8,
    /// Stored notes that have expression a plain export leaves out: a fine
    /// pitch, a modulation, a pan, a slide, a portamento or a curve. Only
    /// counted when expression was left out.
    pub unexported_notes: usize,
    /// Slides written as a bend of the notes they slide.
    pub slides: usize,
    /// Slides that had no held note to slide, which do nothing when the
    /// project plays either.
    pub slides_without_target: usize,
    /// Portamento notes written with a glide from the note before them.
    pub glides: usize,
    /// Notes that sound while another note on their MIDI channel sets the
    /// bend or a controller to something else. The latest note wins.
    pub overlap_notes: usize,
    /// Notes whose pitch moves further than the bend range reaches.
    pub bend_clamped_notes: usize,
    /// Notes whose modulation Y is below neutral, which a wheel or a
    /// pressure that rests at 0 cannot say.
    pub modulation_y_clamped_notes: usize,
    /// Notes whose pan was written as controller 10.
    pub pan_notes: usize,
    /// Notes whose pan was left out: another pan sounds on the channel at
    /// the same time, or the export is MPE.
    pub pan_omitted_notes: usize,
    /// MPE: notes whose colour group could not choose their channel.
    pub mpe_group_notes: usize,
    /// MPE: notes that share a member channel with a note still sounding.
    pub mpe_shared_notes: usize,
    /// Messages written for expression, the bend range included.
    pub messages: usize,
}

impl ExportReport {
    /// True when the file says everything the project's notes do.
    pub fn is_clean(&self) -> bool {
        self.lines().is_empty()
    }

    /// One sentence for each thing the file does not say, for a dialog.
    pub fn lines(&self) -> Vec<String> {
        let notes = |count: usize| {
            if count == 1 {
                "1 note".to_owned()
            } else {
                format!("{count} notes")
            }
        };
        let mut lines = Vec::new();
        let mut line = |count: usize, text: String| {
            if count > 0 {
                lines.push(text);
            }
        };
        line(
            self.unexported_notes,
            format!(
                "{} with fine pitch, modulation, pan, slide or portamento: not written without note expression",
                notes(self.unexported_notes)
            ),
        );
        line(
            self.overlap_notes,
            format!(
                "{} overlapping another on the same MIDI channel with a different bend or controller: the latest note wins",
                notes(self.overlap_notes)
            ),
        );
        line(
            self.bend_clamped_notes,
            format!(
                "{} bending further than the {}-semitone bend range: held at its end",
                notes(self.bend_clamped_notes),
                self.bend_range
            ),
        );
        line(
            self.modulation_y_clamped_notes,
            format!(
                "{} with modulation Y below neutral: written as 0",
                notes(self.modulation_y_clamped_notes)
            ),
        );
        line(
            self.pan_omitted_notes,
            match self.mode {
                Some(ExpressionMode::Mpe) => format!(
                    "{} with a pan of their own: MPE has no pan for a single note",
                    notes(self.pan_omitted_notes)
                ),
                _ => format!(
                    "{} with a pan of their own left at the channel's pan: another pan sounds on the channel at the same time",
                    notes(self.pan_omitted_notes)
                ),
            },
        );
        line(
            self.slides_without_target,
            format!(
                "{} with no held note to slide: nothing written",
                if self.slides_without_target == 1 {
                    "1 slide".to_owned()
                } else {
                    format!("{} slides", self.slides_without_target)
                }
            ),
        );
        line(
            self.mpe_group_notes,
            format!(
                "{} with a colour group: MPE gives every note a channel of its own instead",
                notes(self.mpe_group_notes)
            ),
        );
        line(
            self.mpe_shared_notes,
            format!(
                "{} sharing an MPE channel: more than 15 notes sound at once",
                notes(self.mpe_shared_notes)
            ),
        );
        lines
    }
}

/// The pitch bend for `semitones` when a full bend is `range` semitones,
/// and whether the range was too small for it.
pub fn bend_from_semitones(semitones: f64, range: u8) -> (u16, bool) {
    let range = f64::from(range.clamp(1, MAX_BEND_RANGE));
    let center = f64::from(PITCH_BEND_CENTER);
    let raw = if semitones.is_finite() {
        (center + semitones / range * center).round()
    } else {
        center
    };
    (
        raw.clamp(0.0, 16_383.0) as u16,
        !(0.0..=16_383.0).contains(&raw),
    )
}

/// The semitones a pitch bend moves a note when a full bend is `range`.
pub fn semitones_from_bend(value: u16, range: u8) -> f64 {
    let range = f64::from(range.clamp(1, MAX_BEND_RANGE));
    let center = f64::from(PITCH_BEND_CENTER);
    (f64::from(value.min(16_383)) - center) / center * range
}

/// Controller 74 for a modulation X of 0 to 1. Each half is scaled by its
/// own count of steps, so 0.5 is exactly 64.
pub fn timbre_to_midi(x: f32) -> u8 {
    if !x.is_finite() {
        return 64;
    }
    let x = x.clamp(0.0, 1.0);
    let value = if x < 0.5 {
        x * 128.0
    } else {
        64.0 + (x - 0.5) * 126.0
    };
    (value.round() as u8).min(127)
}

/// The modulation X controller 74 stands for.
pub fn timbre_from_midi(value: u8) -> f32 {
    let value = f32::from(value.min(127));
    if value < 64.0 {
        value / 128.0
    } else {
        0.5 + (value - 64.0) / 126.0
    }
}

/// The modulation wheel or channel pressure for a modulation Y of 0 to 1,
/// and whether the value was below neutral and had to be written as 0.
pub fn pressure_to_midi(y: f32) -> (u8, bool) {
    if !y.is_finite() {
        return (0, false);
    }
    let y = y.clamp(0.0, 1.0);
    if y < 0.5 {
        return (0, true);
    }
    ((((y - 0.5) * 254.0).round() as u8).min(127), false)
}

/// The modulation Y a modulation wheel or channel pressure stands for.
pub fn pressure_from_midi(value: u8) -> f32 {
    0.5 + f32::from(value.min(127)) / 254.0
}

/// The saved curves of one note.
#[derive(Clone, Copy, Default)]
pub(crate) struct Curves<'a> {
    pub pan: Option<&'a NoteExpressionCurve>,
    pub fine: Option<&'a NoteExpressionCurve>,
    pub x: Option<&'a NoteExpressionCurve>,
    pub y: Option<&'a NoteExpressionCurve>,
}

/// One note of an export, or one slide, as the expression pass sees it.
#[derive(Clone, Copy)]
pub(crate) struct Source<'a> {
    /// The track the note is written to.
    pub track: usize,
    /// Where the note is in that track's notes. A slide has no note.
    pub note: Option<usize>,
    pub start: u32,
    pub end: u32,
    pub key: u8,
    /// The MIDI channel the note plays on in channel-voice mode.
    pub channel: u8,
    pub pan: f32,
    /// The pan of the note's channel, which its own pan is added to.
    pub base_pan: f32,
    pub expression: NoteExpression,
    pub curves: Curves<'a>,
}

/// What the expression pass adds to the tracks of an export.
pub(crate) struct Rendered {
    /// Controller curves for each track, in track order.
    pub controls: Vec<Vec<ControlCurve>>,
    /// MPE: the member channel of each note, as (track, note, channel).
    pub channels: Vec<(usize, usize, u8)>,
    pub report: ExportReport,
    /// True when the pass stopped because it would write too much.
    pub overflow: bool,
}

/// A note that sounds, with what it wants its channel to say.
struct Voice<'a> {
    source: Source<'a>,
    channel: u8,
    /// Semitones away from the key, by tick, with straight lines between.
    pitch: Vec<(u32, f64)>,
    /// The ticks inside the note on which something it wants changes.
    breaks: Vec<u32>,
    /// The fine pitch the note starts with, in cents.
    fine: f32,
    pan_ambiguous: bool,
    overlapped: bool,
    bend_clamped: bool,
    y_clamped: bool,
    pan_written: bool,
}

impl Voice<'_> {
    /// How far through the note `tick` is, 0 to 1.
    fn position(&self, tick: u32) -> f64 {
        let length = f64::from(self.source.end.saturating_sub(self.source.start).max(1));
        (f64::from(tick.saturating_sub(self.source.start)) / length).clamp(0.0, 1.0)
    }

    fn curve(&self, curve: Option<&NoteExpressionCurve>, tick: u32, fallback: f32) -> f32 {
        curve
            .and_then(|curve| curve.value_at(self.position(tick)))
            .unwrap_or(fallback)
    }

    /// Semitones from the key at `tick`: glide, slide and fine pitch.
    fn offset(&self, tick: u32) -> f64 {
        let moved = self.source.curves.fine.map_or(0.0, |curve| {
            f64::from(curve.value_at(self.position(tick)).unwrap_or(self.fine) - self.fine) / 100.0
        });
        piecewise(&self.pitch, tick) + moved
    }

    /// The note's pitch at `tick`, as a fractional key.
    fn pitch_at(&self, tick: u32) -> f64 {
        f64::from(self.source.key) + self.offset(tick)
    }

    fn wants_pan(&self) -> bool {
        self.source.pan != 0.0 || self.source.curves.pan.is_some()
    }

    /// What the note wants its channel to say on `tick`.
    fn wants(&self, tick: u32, range: u8) -> Wanted {
        let (bend, bend_clamped) = bend_from_semitones(self.offset(tick), range);
        let x = self.curve(
            self.source.curves.x,
            tick,
            self.source.expression.modulation_x,
        );
        let y = self.curve(
            self.source.curves.y,
            tick,
            self.source.expression.modulation_y,
        );
        let (y, y_clamped) = pressure_to_midi(y);
        let pan = (self.wants_pan() && !self.pan_ambiguous).then(|| {
            let own = self.curve(self.source.curves.pan, tick, self.source.pan);
            pan_to_midi((self.source.base_pan + own).clamp(-1.0, 1.0))
        });
        Wanted {
            bend,
            x: timbre_to_midi(x),
            y,
            pan,
            bend_clamped,
            y_clamped,
        }
    }
}

#[derive(Clone, Copy)]
struct Wanted {
    bend: u16,
    x: u8,
    y: u8,
    pan: Option<u8>,
    bend_clamped: bool,
    y_clamped: bool,
}

/// The value of a line through `points` at `tick`: the first value before
/// them and the last after.
fn piecewise(points: &[(u32, f64)], tick: u32) -> f64 {
    let after = points.partition_point(|point| point.0 <= tick);
    let Some(from) = after.checked_sub(1).and_then(|at| points.get(at)) else {
        return points.first().map_or(0.0, |point| point.1);
    };
    let Some(to) = points.get(after) else {
        return from.1;
    };
    let span = f64::from(to.0 - from.0);
    from.1 + (to.1 - from.1) * f64::from(tick - from.0) / span
}

/// Makes the line through `points` go from where it is on `from` to
/// `target` on `to`, and stay there.
fn bend_to(points: &mut Vec<(u32, f64)>, from: u32, to: u32, target: f64) {
    let now = piecewise(points, from);
    points.retain(|point| point.0 < from);
    if to > from {
        points.push((from, now));
        points.push((to, target));
    } else {
        points.push((from, target));
    }
}

/// What a channel last said.
#[derive(Clone, Copy)]
struct ChannelState {
    bend: u16,
    x: u8,
    y: u8,
    /// The pan a note set and the pan to go back to when it ends.
    pan: Option<(u8, u8)>,
    /// The track of the note that last set anything.
    track: usize,
}

struct Writer {
    points: BTreeMap<(usize, u8, ControlKind), Vec<ControlPoint>>,
    count: usize,
    limit: usize,
}

impl Writer {
    fn put(&mut self, track: usize, channel: u8, kind: ControlKind, tick: u32, value: u16) {
        self.count += 1;
        if self.count > self.limit {
            return;
        }
        let points = self.points.entry((track, channel, kind)).or_default();
        // Of several values on one tick only the last would be heard.
        match points.last_mut() {
            Some(last) if last.tick == tick => last.value = value,
            _ => points.push(ControlPoint { tick, value }),
        }
    }
}

/// Works out the controller curves that say what `sources` play, and in
/// MPE the channel of every note. `tracks` is how many tracks there are and
/// `limit` the most messages the pass may write.
pub(crate) fn render(
    sources: &[Source<'_>],
    tracks: usize,
    options: &ExpressionOptions,
    limit: usize,
) -> Rendered {
    let range = options.range();
    let mpe = options.mode == ExpressionMode::Mpe;
    let step = options.step.max(1);
    let mut report = ExportReport {
        mode: Some(options.mode),
        bend_range: range,
        ..ExportReport::default()
    };

    // Notes before slides on one tick, so that a slide drawn from the very
    // start of a note has that note to slide.
    let mut order: Vec<usize> = (0..sources.len()).collect();
    let slide = |source: &Source<'_>| source.expression.articulation == NoteArticulation::Slide;
    order.sort_by_key(|&index| {
        let source = &sources[index];
        (source.start, slide(source), source.track, index)
    });

    // The pitch of every note, worked out the way the engine plays it:
    // within one track and one colour group, a portamento glides from the
    // note that started last and a slide moves every note that is held.
    let mut voices: Vec<Voice<'_>> = Vec::new();
    let mut held: BTreeMap<(usize, u8), Vec<usize>> = BTreeMap::new();
    for &index in &order {
        let source = sources[index];
        let group = (
            source.track,
            source.expression.color_group.unwrap_or(0).min(15),
        );
        let fine = source
            .curves
            .fine
            .and_then(|curve| curve.value_at(0.0))
            .unwrap_or(source.expression.fine_pitch_cents);
        let own = f64::from(fine) / 100.0;
        let group_voices = held.entry(group).or_default();
        group_voices.retain(|&voice| voices[voice].source.end > source.start);
        let latest = group_voices.last().copied();
        if slide(&source) {
            let Some(anchor) = latest else {
                report.slides_without_target += 1;
                continue;
            };
            report.slides += 1;
            let delta = f64::from(source.key) + own - voices[anchor].pitch_at(source.start);
            for &voice in group_voices.iter() {
                let voice = &mut voices[voice];
                // The fine-pitch curve keeps moving on top of the slide.
                let key = f64::from(voice.source.key);
                let now = key + piecewise(&voice.pitch, source.start);
                let target = (now + delta).clamp(-12.0, 139.0) - key;
                bend_to(&mut voice.pitch, source.start, source.end, target);
            }
            continue;
        }
        let mut pitch = vec![(source.start, own)];
        if source.expression.articulation == NoteArticulation::Portamento
            && let Some(before) = latest
        {
            let from = voices[before].pitch_at(source.start) - f64::from(source.key);
            let glide = source.expression.glide_ticks.clamp(1, 245_760);
            pitch = vec![
                (source.start, from),
                (source.start.saturating_add(glide), own),
            ];
            report.glides += 1;
        }
        group_voices.push(voices.len());
        voices.push(Voice {
            source,
            channel: source.channel.min(15),
            pitch,
            breaks: Vec::new(),
            fine,
            pan_ambiguous: mpe,
            overlapped: false,
            bend_clamped: false,
            y_clamped: false,
            pan_written: false,
        });
    }

    let mut channels = Vec::new();
    if mpe {
        // The channel that has been silent longest plays the next note.
        let mut busy_until = [0_u32; 16];
        let mut used = [false; 16];
        for voice in &mut voices {
            let start = voice.source.start;
            let free = MPE_MEMBERS
                .filter(|&channel| busy_until[usize::from(channel)] <= start)
                .min_by_key(|&channel| {
                    (used[usize::from(channel)], busy_until[usize::from(channel)])
                });
            let channel = free.unwrap_or_else(|| {
                report.mpe_shared_notes += 1;
                MPE_MEMBERS
                    .min_by_key(|&channel| busy_until[usize::from(channel)])
                    .unwrap_or(1)
            });
            let slot = usize::from(channel);
            busy_until[slot] = busy_until[slot].max(voice.source.end);
            used[slot] = true;
            voice.channel = channel;
            report.mpe_group_notes += usize::from(voice.source.expression.color_group.is_some());
            report.pan_omitted_notes += usize::from(voice.wants_pan());
            if let Some(note) = voice.source.note {
                channels.push((voice.source.track, note, channel));
            }
        }
    }

    for voice in &mut voices {
        voice.breaks = breaks(voice, step);
    }

    let mut by_channel: [Vec<usize>; 16] = Default::default();
    for (index, voice) in voices.iter().enumerate() {
        by_channel[usize::from(voice.channel)].push(index);
    }

    if !mpe {
        mark_ambiguous_pans(&mut voices, &by_channel);
    }

    let mut writer = Writer {
        points: BTreeMap::new(),
        count: 0,
        limit,
    };
    let y_kind = if mpe {
        ControlKind::ChannelPressure
    } else {
        ControlKind::Controller(CC_MODULATION)
    };
    for (channel, members) in by_channel.iter().enumerate() {
        let channel = channel as u8;
        if members.is_empty() {
            continue;
        }
        let mut edges: Vec<u32> = members
            .iter()
            .flat_map(|&voice| [voices[voice].source.start, voices[voice].source.end])
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let mut state = ChannelState {
            bend: PITCH_BEND_CENTER,
            x: 64,
            y: 0,
            pan: None,
            track: voices[members[0]].source.track,
        };
        // `members` is in starting order, so the notes that have started
        // are a prefix of it.
        let mut started = 0;
        let mut active: Vec<usize> = Vec::new();
        let mut ticks: Vec<u32> = Vec::new();
        for (at, &edge) in edges.iter().enumerate() {
            active.retain(|&voice| voices[voice].source.end > edge);
            while let Some(&voice) = members.get(started) {
                if voices[voice].source.start > edge {
                    break;
                }
                started += 1;
                if voices[voice].source.end > edge {
                    active.push(voice);
                }
            }
            let Some(&owner) = active.last() else {
                // Nothing sounds: everything a note moved goes back.
                let track = state.track;
                if state.bend != PITCH_BEND_CENTER {
                    let center = PITCH_BEND_CENTER;
                    writer.put(track, channel, ControlKind::PitchBend, edge, center);
                    state.bend = center;
                }
                if state.x != 64 {
                    writer.put(track, channel, ControlKind::Controller(CC_TIMBRE), edge, 64);
                    state.x = 64;
                }
                if state.y != 0 {
                    writer.put(track, channel, y_kind, edge, 0);
                    state.y = 0;
                }
                if let Some((_, base)) = state.pan.take() {
                    let kind = ControlKind::Controller(CC_PAN);
                    writer.put(track, channel, kind, edge, u16::from(base));
                }
                continue;
            };
            let until = edges.get(at + 1).copied().unwrap_or(u32::MAX);
            ticks.clear();
            ticks.push(edge);
            for &voice in &active {
                let breaks = &voices[voice].breaks;
                let from = breaks.partition_point(|&tick| tick <= edge);
                let to = breaks.partition_point(|&tick| tick < until);
                ticks.extend_from_slice(&breaks[from..to.max(from)]);
            }
            ticks.sort_unstable();
            ticks.dedup();
            let track = voices[owner].source.track;
            state.track = track;
            for &tick in &ticks {
                let wanted = voices[owner].wants(tick, range);
                voices[owner].bend_clamped |= wanted.bend_clamped;
                voices[owner].y_clamped |= wanted.y_clamped;
                if wanted.bend != state.bend {
                    writer.put(track, channel, ControlKind::PitchBend, tick, wanted.bend);
                    state.bend = wanted.bend;
                }
                if wanted.x != state.x {
                    let kind = ControlKind::Controller(CC_TIMBRE);
                    writer.put(track, channel, kind, tick, u16::from(wanted.x));
                    state.x = wanted.x;
                }
                if wanted.y != state.y {
                    writer.put(track, channel, y_kind, tick, u16::from(wanted.y));
                    state.y = wanted.y;
                }
                let base = pan_to_midi(voices[owner].source.base_pan);
                match (wanted.pan, state.pan) {
                    (Some(pan), set) if set.map(|set| set.0) != Some(pan) => {
                        let kind = ControlKind::Controller(CC_PAN);
                        if set.is_some() || pan != base {
                            writer.put(track, channel, kind, tick, u16::from(pan));
                        }
                        state.pan = Some((pan, base));
                        voices[owner].pan_written = true;
                    }
                    (Some(_), _) => voices[owner].pan_written = true,
                    (None, Some((pan, _))) => {
                        if pan != base {
                            let kind = ControlKind::Controller(CC_PAN);
                            writer.put(track, channel, kind, tick, u16::from(base));
                        }
                        state.pan = None;
                    }
                    (None, None) => {}
                }
                for &other in &active[..active.len() - 1] {
                    let theirs = voices[other].wants(tick, range);
                    if theirs.bend != wanted.bend || theirs.x != wanted.x || theirs.y != wanted.y {
                        voices[other].overlapped = true;
                    }
                }
                if writer.count > writer.limit {
                    break;
                }
            }
            if writer.count > writer.limit {
                break;
            }
        }
    }

    for voice in &voices {
        report.overlap_notes += usize::from(voice.overlapped);
        report.bend_clamped_notes += usize::from(voice.bend_clamped);
        report.modulation_y_clamped_notes += usize::from(voice.y_clamped);
        report.pan_notes += usize::from(voice.pan_written);
        if !mpe {
            report.pan_omitted_notes += usize::from(voice.wants_pan() && voice.pan_ambiguous);
        }
    }

    // The bend range, on every channel of every track that bends there,
    // and in MPE the zone itself on the master channel of the first track.
    let mut setup = Vec::new();
    for &(track, channel, kind) in writer.points.keys() {
        if kind == ControlKind::PitchBend {
            setup.push((track, channel, RPN_BEND_RANGE, range));
        }
    }
    if mpe && let Some(first) = voices.iter().map(|voice| voice.source.track).min() {
        let members = *MPE_MEMBERS.end() - *MPE_MEMBERS.start() + 1;
        setup.push((first, 0, RPN_MPE_CONFIGURATION, members));
    }
    for (track, channel, parameter, value) in setup {
        for (controller, value) in [
            (CC_RPN_COARSE, 0),
            (CC_RPN_FINE, parameter),
            (CC_DATA_ENTRY, value),
            (CC_DATA_ENTRY_FINE, 0),
        ] {
            // The fine part of the zone size does not exist.
            if parameter == RPN_MPE_CONFIGURATION && controller == CC_DATA_ENTRY_FINE {
                continue;
            }
            let kind = ControlKind::Controller(controller);
            writer.put(track, channel, kind, 0, u16::from(value));
        }
    }

    let overflow = writer.count > writer.limit;
    report.messages = writer.count.min(writer.limit);
    let mut controls: Vec<Vec<ControlCurve>> = vec![Vec::new(); tracks];
    for ((track, channel, kind), points) in writer.points {
        if let Some(curves) = controls.get_mut(track) {
            curves.push(ControlCurve {
                channel,
                kind,
                points,
            });
        }
    }
    Rendered {
        controls,
        channels,
        report,
        overflow,
    }
}

/// The ticks inside a note on which what it wants may change: the corners
/// of its pitch, and a grid over everything that moves.
fn breaks(voice: &Voice<'_>, step: u32) -> Vec<u32> {
    let (start, end) = (voice.source.start, voice.source.end);
    let length = end.saturating_sub(start);
    let step = step.max(length / MAX_STEPS_PER_NOTE).max(1);
    let mut ticks = Vec::new();
    let inside = |tick: u32| tick > start && tick < end;
    let grid = |from: u32, to: u32, ticks: &mut Vec<u32>| {
        let mut tick = from.max(start).saturating_add(step);
        let to = to.min(end);
        while tick < to {
            ticks.push(tick);
            tick = tick.saturating_add(step);
        }
    };
    for pair in voice.pitch.windows(2) {
        if pair[0].1 != pair[1].1 {
            grid(pair[0].0, pair[1].0, &mut ticks);
        }
    }
    ticks.extend(
        voice
            .pitch
            .iter()
            .map(|point| point.0)
            .filter(|&tick| inside(tick)),
    );
    let curves = voice.source.curves;
    let mut moving = false;
    for curve in [curves.pan, curves.fine, curves.x, curves.y]
        .into_iter()
        .flatten()
    {
        if curve.points.len() < 2 {
            continue;
        }
        moving = true;
        ticks.extend(
            curve
                .points
                .iter()
                .map(|point| start + (f64::from(point.position) * f64::from(length)).round() as u32)
                .filter(|&tick| inside(tick)),
        );
    }
    if moving {
        grid(start, end, &mut ticks);
    }
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

/// A note's pan is only written when no other pan sounds on its channel
/// while it does. Marks every note for which that is not so.
fn mark_ambiguous_pans(voices: &mut [Voice<'_>], by_channel: &[Vec<usize>; 16]) {
    let pan_of = |voice: &Voice<'_>| {
        let curve = voice
            .source
            .curves
            .pan
            .map(|curve| curve as *const NoteExpressionCurve);
        (
            voice.source.pan.to_bits(),
            voice.source.base_pan.to_bits(),
            curve,
            // A pan that moves is tied to its own note's length.
            curve.map(|_| (voice.source.start, voice.source.end)),
        )
    };
    for members in by_channel {
        if !members.iter().any(|&voice| voices[voice].wants_pan()) {
            continue;
        }
        let mut active: Vec<usize> = Vec::new();
        for &voice in members {
            let start = voices[voice].source.start;
            active.retain(|&other| voices[other].source.end > start);
            let pan = pan_of(&voices[voice]);
            let mut clash = false;
            for &other in &active {
                if pan_of(&voices[other]) != pan {
                    voices[other].pan_ambiguous = true;
                    clash = true;
                }
            }
            voices[voice].pan_ambiguous |= clash;
            active.push(voice);
        }
    }
}

/// Whether a stored note has anything a plain export leaves out.
pub(crate) fn has_expression(pan: f32, expression: &NoteExpression) -> bool {
    pan != 0.0
        || expression.fine_pitch_cents != 0.0
        || expression.modulation_x != 0.5
        || expression.modulation_y != 0.5
        || expression.articulation != NoteArticulation::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bend_is_exact_at_the_center_and_the_ends_of_the_range() {
        assert_eq!(bend_from_semitones(0.0, 12), (8192, false));
        assert_eq!(bend_from_semitones(-12.0, 12), (0, false));
        assert_eq!(bend_from_semitones(6.0, 12), (12_288, false));
        // A full bend up is one step short of the range, as on a wheel.
        assert_eq!(bend_from_semitones(12.0, 12), (16_383, true));
        assert_eq!(bend_from_semitones(-13.0, 12), (0, true));
        assert_eq!(bend_from_semitones(f64::NAN, 12), (8192, false));
        assert_eq!(semitones_from_bend(8192, 2), 0.0);
        assert_eq!(semitones_from_bend(0, 2), -2.0);
        assert_eq!(semitones_from_bend(12_288, 48), 24.0);
    }

    #[test]
    fn every_controller_value_comes_back_unchanged() {
        for value in 0..=127_u8 {
            assert_eq!(timbre_to_midi(timbre_from_midi(value)), value);
            assert_eq!(pressure_to_midi(pressure_from_midi(value)), (value, false));
        }
        assert_eq!(timbre_from_midi(64), 0.5);
        assert_eq!(timbre_to_midi(0.5), 64);
        assert_eq!(pressure_from_midi(0), 0.5);
        assert_eq!(pressure_from_midi(127), 1.0);
        assert_eq!(pressure_to_midi(0.5), (0, false));
        assert_eq!(pressure_to_midi(0.25), (0, true));
        assert_eq!(timbre_to_midi(f32::NAN), 64);
    }

    #[test]
    fn a_line_holds_its_ends_and_can_be_bent_part_way() {
        let mut points = vec![(100, 0.0), (200, 2.0)];
        assert_eq!(piecewise(&points, 0), 0.0);
        assert_eq!(piecewise(&points, 150), 1.0);
        assert_eq!(piecewise(&points, 900), 2.0);
        bend_to(&mut points, 150, 250, 5.0);
        assert_eq!(points, [(100, 0.0), (150, 1.0), (250, 5.0)]);
        assert_eq!(piecewise(&points, 200), 3.0);
        bend_to(&mut points, 300, 300, -1.0);
        assert_eq!(piecewise(&points, 300), -1.0);
    }
}
