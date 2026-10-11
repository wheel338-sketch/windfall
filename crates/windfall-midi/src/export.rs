//! Turning a project, or one pattern of it, into a [`MidiSong`].
//!
//! The song says what the engine would play: the same notes, cut where a
//! clip cuts them, moved where swing moves them, at the tempo the song has
//! on each tick. [`write`](crate::write) then makes a file of it.

use std::collections::{HashMap, HashSet};
use std::fmt;

use windfall_project::{
    AutomationPoint, AutomationRange, AutomationTarget, Channel, ChannelId, ClipContent, ClipId,
    DEFAULT_CHANNEL_VOLUME, NoteCurveParameter, NoteExpression, NoteId, Pattern, PatternId,
    Project, TICKS_PER_STEP, curve_value,
};

use crate::expression::{self, Curves, ExportReport, ExpressionMode, ExpressionOptions, Source};
use crate::mix::{pan_to_midi, velocity_to_midi, volume_to_midi};
use crate::song::{
    CC_PAN, CC_VOLUME, ControlCurve, ControlKind, ControlPoint, DRUM_CHANNEL,
    MidiNote, MidiSong, MidiTrack, TempoChange, TimeSignatureChange, micros_per_quarter,
};

/// The most notes one export holds. A clip that loops a short pattern for
/// hours could ask for more memory than there is; such a song is refused.
pub const MAX_EXPORTED_NOTES: usize = 4_000_000;

/// The most messages note expression is written in. A song that would
/// need more is refused.
pub const MAX_EXPRESSION_MESSAGES: usize = 8_000_000;

/// The most steps a gliding tempo is written in. A song long enough to
/// need more at [`ExportOptions::tempo_step`] gets longer steps.
const MAX_TEMPO_STEPS: u32 = 100_000;

/// Choices for representing project playback in MIDI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    /// Moves the notes the way the project's swing moves them when it
    /// plays, to the nearest tick, so that the file sounds like the
    /// project. Off, the notes are written where the piano roll shows them.
    pub swing: bool,
    /// How often, in ticks, a tempo that glides gets a new tempo event. A
    /// MIDI file can only set a tempo, not glide to one, so a glide is
    /// written as steps of this length, each at the tempo the glide has in
    /// its middle. 0 counts as 1.
    pub tempo_step: u32,
    /// Writes each channel's volume and pan as channel volume (controller
    /// 7) and pan (controller 10), where they are not the defaults a MIDI
    /// device has anyway.
    pub channel_mix: bool,
    /// Writes what MIDI notes cannot say themselves as pitch bend and
    /// controllers: fine pitch, slides, portamento, modulation X and Y, a
    /// note's own pan and its saved curves. See
    /// [`expression`](crate::expression) for where each goes. `None`, the
    /// default, writes none of it and every file is what it was before the
    /// option existed.
    pub expression: Option<ExpressionOptions>,
}

impl Default for ExportOptions {
    /// Swing as it plays, a tempo step of a sixteenth note, the channels'
    /// volumes and pans, and no note expression.
    fn default() -> Self {
        Self {
            swing: true,
            tempo_step: TICKS_PER_STEP,
            channel_mix: true,
            expression: None,
        }
    }
}

/// A song and what could not be said in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub song: MidiSong,
    pub report: ExportReport,
}

/// Why a project could not be exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportError {
    /// The project has no pattern with this id.
    PatternNotFound(PatternId),
    /// The song plays more notes than [`MAX_EXPORTED_NOTES`].
    TooManyNotes,
    /// The song's note expression needs more messages than
    /// [`MAX_EXPRESSION_MESSAGES`].
    TooManyMessages,
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::PatternNotFound(id) => write!(f, "pattern {} does not exist", id.0),
            ExportError::TooManyNotes => write!(
                f,
                "the song plays more than {MAX_EXPORTED_NOTES} notes, which is more than one MIDI export holds"
            ),
            ExportError::TooManyMessages => write!(
                f,
                "the song's note expression needs more than {MAX_EXPRESSION_MESSAGES} MIDI messages; export it without note expression or with a longer step"
            ),
        }
    }
}

impl std::error::Error for ExportError {}

/// One pass of a pattern as a song: every channel that has notes in it is
/// a track, the song is as long as the pattern, and its tempo and time
/// signature are the project's.
///
/// A note that starts at or past the end of the pattern does not play and
/// is left out, and one that lasts past the end is cut there, as it is
/// when a clip plays the pattern once. See [`export_song`] for how a
/// channel becomes a track.
pub fn export_pattern(
    project: &Project,
    pattern: PatternId,
    options: &ExportOptions,
) -> Result<MidiSong, ExportError> {
    export_pattern_reported(project, pattern, options).map(|exported| exported.song)
}

/// [`export_pattern`], with the report of what the file does not say.
pub fn export_pattern_reported(
    project: &Project,
    pattern: PatternId,
    options: &ExportOptions,
) -> Result<Exported, ExportError> {
    let pattern = project
        .pattern(pattern)
        .ok_or(ExportError::PatternNotFound(pattern))?;
    let length = pattern_length(pattern);
    let expressive = options.expression.is_some();
    let mut notes = Notes::new(expressive);
    for played in played_notes(project, pattern, swing(project, options), expressive) {
        let end = played.end.min(length);
        notes.add(played, played.start, end)?;
    }
    let tempo = TempoChange::from_bpm(0, project.settings.tempo_bpm);
    let (mut result, mut report) = song(
        project,
        &pattern.name,
        notes,
        vec![tempo],
        length,
        options,
    )?;
    if !expressive {
        report.unexported_notes = unexported(pattern);
    }
    let signature = pattern.effective_signature(project.settings.time_signature);
    result.time_signatures = vec![crate::TimeSignatureChange { tick: 0, numerator: signature.numerator, denominator: signature.denominator }];
    for meter in pattern.timeline.meters.iter().filter(|meter| meter.tick < length) {
        if meter.tick == 0 { result.time_signatures.clear(); }
        result.time_signatures.push(crate::TimeSignatureChange { tick: meter.tick, numerator: meter.signature.numerator, denominator: meter.signature.denominator });
    }
    result.markers = pattern.timeline.markers.iter().filter(|marker| marker.tick <= length)
        .map(|marker| crate::Marker { tick: marker.tick, text: marker.name.clone() }).collect();
    Ok(Exported { song: result, report })
}

/// The playlist as a song, the way song mode plays it.
///
/// - **Clips.** Every pattern clip is laid out note by note: from its
///   offset into the pattern, around the pattern's end as often as the
///   clip is long, with the notes that are sounding at its end cut there.
///   Muted clips and clips on muted playlist tracks are left out. Audio
///   clips have no notes, and automation other than the tempo's has no
///   place in the file.
/// - **Tracks.** Every channel that plays a note is a track, in rack
///   order, named after the channel, whether it is muted or not: a mute
///   is a mixing decision and the notes are still the song. Each track has
///   a MIDI channel to itself for as long as there are enough, from 1 up
///   and past 10, which General MIDI keeps for drums; from the sixteenth
///   track on they are shared. A velocity of 0 to 1 becomes 1 to 127.
/// - **Tempo.** The project's tempo and the tempo automation, read the way
///   the engine reads it: before the first tempo clip the project's
///   tempo, inside a clip its curve, after a clip the value it ended on,
///   and where clips overlap the one on the upper playlist track. Steps
///   and jumps of the curve are exact. A glide is written as steps
///   ([`ExportOptions::tempo_step`]). Each tempo is rounded to the whole
///   microsecond per quarter note that a file stores.
/// - **Length.** The song ends where its last clip of any kind ends, muted
///   ones included, as it does when it plays.
///
/// A note's pan, a sampler's tuning, the mixer and every effect are not
/// part of a MIDI file.
pub fn export_song(project: &Project, options: &ExportOptions) -> Result<MidiSong, ExportError> {
    export_song_reported(project, options).map(|exported| exported.song)
}

/// [`export_song`], with the report of what the file does not say.
pub fn export_song_reported(
    project: &Project,
    options: &ExportOptions,
) -> Result<Exported, ExportError> {
    let playlist = &project.playlist;
    let muted: HashSet<_> = playlist
        .tracks
        .iter()
        .filter(|track| track.muted)
        .map(|track| track.id)
        .collect();
    let swing = swing(project, options);
    let expressive = options.expression.is_some();
    let mut patterns: HashMap<PatternId, (u32, Vec<Played<'_>>)> = HashMap::new();
    let mut notes = Notes::new(expressive);
    let mut length = 0;
    for clip in &playlist.clips {
        let clip_end = clip.start.saturating_add(clip.length);
        length = length.max(clip_end);
        let ClipContent::Pattern { pattern } = clip.content else {
            continue;
        };
        if clip.muted || muted.contains(&clip.track) || clip.length == 0 {
            continue;
        }
        let Some(pattern) = project.pattern(pattern) else {
            continue;
        };
        let (pattern_length, played) = patterns
            .entry(pattern.id)
            .or_insert_with(|| {
                (
                    pattern_length(pattern),
                    played_notes(project, pattern, swing, expressive),
                )
            });
        if played.is_empty() {
            continue;
        }
        // The tick the clip's first pass of the pattern would begin on,
        // had the clip not cut its beginning off.
        let period = u64::from(*pattern_length);
        let mut pass = u64::from(clip.start) + period - u64::from(clip.offset) % period;
        // Ticks are counted one pattern length late, so that the pass that
        // begins before the clip does not need a negative number.
        let (clip_start, clip_end) = (u64::from(clip.start) + period, u64::from(clip_end) + period);
        while pass < clip_end {
            for &note in played.iter() {
                let start = pass + u64::from(note.start);
                if start >= clip_end {
                    break;
                }
                if start < clip_start {
                    continue;
                }
                let end = (pass + u64::from(note.end)).min(clip_end);
                notes.add(note, (start - period) as u32, (end - period) as u32)?;
            }
            pass += period;
        }
    }
    let tempos = tempo_changes(project, length, options.tempo_step);
    let (mut result, mut report) = song(
        project,
        &project.settings.name,
        notes,
        tempos,
        length,
        options,
    )?;
    if !expressive {
        report.unexported_notes = patterns
            .keys()
            .filter_map(|&id| project.pattern(id))
            .map(unexported)
            .sum();
    }
    for meter in &project.playlist.timeline.meters {
        if meter.tick == 0 {
            result.time_signatures.clear();
        }
        result.time_signatures.push(crate::TimeSignatureChange {
            tick: meter.tick,
            numerator: meter.signature.numerator,
            denominator: meter.signature.denominator,
        });
    }
    result.markers = project.playlist.timeline.markers.iter()
        .filter(|marker| marker.tick <= length && marker.kind == windfall_project::MarkerKind::Named)
        .map(|marker| crate::Marker { tick: marker.tick, text: marker.name.clone() }).collect();
    Ok(Exported { song: result, report })
}

/// How many notes of a pattern that play have expression a plain export
/// leaves out.
fn unexported(pattern: &Pattern) -> usize {
    let length = pattern_length(pattern);
    let curved: HashSet<NoteId> = pattern
        .note_curves
        .iter()
        .filter(|curve| curve.parameter != NoteCurveParameter::Release)
        .map(|curve| curve.note)
        .collect();
    let notes = pattern.lanes.iter().flat_map(|lane| &lane.notes);
    notes
        .filter(|note| note.start < length)
        .filter(|note| {
            expression::has_expression(note.pan, &note.expression) || curved.contains(&note.id)
        })
        .count()
}

/// The project's swing as the engine applies it, or none when the export
/// leaves swing out.
fn swing(project: &Project, options: &ExportOptions) -> f64 {
    let swing = f64::from(project.settings.swing);
    if options.swing && swing.is_finite() {
        swing.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// How long a pattern is when it plays, in ticks.
fn pattern_length(pattern: &Pattern) -> u32 {
    pattern.length_steps.max(1).saturating_mul(TICKS_PER_STEP)
}

/// Moves a tick to where swing puts it, exactly as the engine does: time is
/// stretched inside each pair of steps, the first step growing by the
/// swing delay and the second shrinking by it. Ticks at or past
/// `swung_length` are left alone.
#[cfg(test)]
fn swing_warp(tick: f64, swing: f64, swung_length: f64) -> f64 {
    windfall_project::note_timing::swing_warp(tick, swing, swung_length)
}

/// A note of a pattern as it plays in one pass of the pattern.
#[derive(Clone, Copy)]
struct Played<'a> {
    channel: ChannelId,
    /// Ticks from the start of the pass, with swing applied. `end` is
    /// after `start`, and may lie past the end of the pattern.
    start: u32,
    end: u32,
    key: u8,
    velocity: u8,
    release: u8,
    midi_channel: Option<u8>,
    /// A slide plays no note. It is only kept when expression is written.
    slide: bool,
    pan: f32,
    expression: NoteExpression,
    curves: Curves<'a>,
}

/// The notes of a pattern that play, in order of their starts. With
/// `expressive`, slides are among them and every note has its curves.
fn played_notes<'a>(
    project: &Project,
    pattern: &'a Pattern,
    swing: f64,
    expressive: bool,
) -> Vec<Played<'a>> {
    let length = pattern_length(pattern);
    let mut curves: HashMap<NoteId, Curves<'a>> = HashMap::new();
    if expressive {
        for curve in &pattern.note_curves {
            let of_note = curves.entry(curve.note).or_default();
            match curve.parameter {
                NoteCurveParameter::Pan => of_note.pan = Some(curve),
                NoteCurveParameter::FinePitchCents => of_note.fine = Some(curve),
                NoteCurveParameter::ModulationX => of_note.x = Some(curve),
                NoteCurveParameter::ModulationY => of_note.y = Some(curve),
                // Release is the note-off velocity, which has no curve.
                NoteCurveParameter::Release => {}
            }
        }
    }
    let mut played = Vec::new();
    for lane in &pattern.lanes {
        let Some(channel) = project.channel(lane.channel) else { continue; };
        for note in lane.notes.iter().filter(|note| note.start < length) {
            // A slide is a control event, never an additional MIDI note-on.
            let slide = note.expression.articulation == windfall_project::NoteArticulation::Slide;
            if slide && !expressive { continue; }
            let Some((tick, duration)) = channel.timing.place(note.start, note.length, length, swing) else { continue; };
            let start = tick.round() as u32;
            let end = (tick + duration).round() as u32;
            played.push(Played {
                channel: lane.channel,
                start,
                end: end.max(start.saturating_add(1)),
                key: note.key.min(127),
                velocity: velocity_to_midi(note.velocity),
                release: (note.expression.clamped().release * 128.0).round().min(127.0) as u8,
                midi_channel: note.expression.color_group,
                slide,
                pan: if note.pan.is_finite() { note.pan.clamp(-1.0, 1.0) } else { 0.0 },
                expression: note.expression.clamped(),
                curves: curves.get(&note.id).copied().unwrap_or_default(),
            });
        }
    }
    played.sort_by_key(|note| note.start);
    played
}

/// A note or a slide of an export that the expression pass has yet to see.
struct Pending<'a> {
    /// Where the note is in its channel's notes. A slide has none.
    note: Option<usize>,
    start: u32,
    end: u32,
    played: Played<'a>,
}

/// The notes of an export, by the channel that plays them.
struct Notes<'a> {
    by_channel: HashMap<ChannelId, Vec<MidiNote>>,
    /// The same notes once more, and the slides, when expression is
    /// written.
    expressive: Option<HashMap<ChannelId, Vec<Pending<'a>>>>,
    count: usize,
}

impl<'a> Notes<'a> {
    fn new(expressive: bool) -> Self {
        Self {
            by_channel: HashMap::new(),
            expressive: expressive.then(HashMap::new),
            count: 0,
        }
    }

    fn add(&mut self, played: Played<'a>, start: u32, end: u32) -> Result<(), ExportError> {
        if self.count == MAX_EXPORTED_NOTES {
            return Err(ExportError::TooManyNotes);
        }
        self.count += 1;
        if let Some(expressive) = &mut self.expressive {
            let note = (!played.slide)
                .then(|| self.by_channel.get(&played.channel).map_or(0, Vec::len));
            let pending = Pending {
                note,
                start,
                end: end.max(start.saturating_add(1)),
                played,
            };
            expressive.entry(played.channel).or_default().push(pending);
        }
        if played.slide {
            return Ok(());
        }
        let notes = self.by_channel.entry(played.channel).or_default();
        notes.push(MidiNote {
            start,
            length: end.saturating_sub(start).max(1),
            key: played.key,
            velocity: played.velocity,
            release: played.release,
            // Untagged notes receive the track's automatic route in song().
            channel: played.midi_channel.unwrap_or(u8::MAX),
        });
        Ok(())
    }
}

/// The MIDI channel of the track at `index` of a file: 0 to 15 without
/// the drum channel, around again from the sixteenth track on.
fn midi_channel(index: usize) -> u8 {
    let slot = (index % 15) as u8;
    if slot >= DRUM_CHANNEL { slot + 1 } else { slot }
}

/// Puts the song together: a track for each channel that has notes, in
/// rack order.
fn song(
    project: &Project,
    name: &str,
    mut all: Notes<'_>,
    tempos: Vec<TempoChange>,
    length: u32,
    options: &ExportOptions,
) -> Result<(MidiSong, ExportReport), ExportError> {
    let mut tracks: Vec<MidiTrack> = Vec::new();
    let mut sources: Vec<Source<'_>> = Vec::new();
    let mpe = options
        .expression
        .is_some_and(|expression| expression.mode == ExpressionMode::Mpe);
    for channel in &project.channels {
        let Some(mut notes) = all.by_channel.remove(&channel.id) else {
            continue;
        };
        let midi_channel = midi_channel(tracks.len());
        for note in &mut notes {
            if note.channel == u8::MAX { note.channel = midi_channel; }
        }
        let pending = all
            .expressive
            .as_mut()
            .and_then(|expressive| expressive.remove(&channel.id));
        for pending in pending.into_iter().flatten() {
            let played = pending.played;
            sources.push(Source {
                track: tracks.len(),
                note: pending.note,
                start: pending.start,
                end: pending.end,
                key: played.key,
                channel: played.midi_channel.unwrap_or(midi_channel),
                pan: played.pan,
                base_pan: if options.channel_mix { channel.pan } else { 0.0 },
                expression: played.expression,
                curves: played.curves,
            });
        }
        let controls = if mpe && options.channel_mix {
            // A zone has one volume and one pan, on its master channel.
            mix_controls(channel, 0)
        } else if options.channel_mix {
            let mut used = [false; 16];
            for note in &notes { used[usize::from(note.channel.min(15))] = true; }
            used.into_iter().enumerate().filter(|(_, used)| *used)
                .flat_map(|(route, _)| mix_controls(channel, route as u8)).collect()
        } else {
            Vec::new()
        };
        tracks.push(MidiTrack {
            name: Some(channel.name.clone()),
            instrument: None,
            notes,
            programs: Vec::new(),
            controls,
        });
    }
    let mut report = ExportReport::default();
    if let Some(expression) = &options.expression {
        let rendered = expression::render(
            &sources,
            tracks.len(),
            expression,
            MAX_EXPRESSION_MESSAGES,
        );
        if rendered.overflow {
            return Err(ExportError::TooManyMessages);
        }
        report = rendered.report;
        // A slide on a channel that plays no note has nothing to slide.
        let stranded = all.expressive.iter().flat_map(HashMap::values).flatten();
        report.slides_without_target += stranded.filter(|pending| pending.played.slide).count();
        for (track, note, channel) in rendered.channels {
            tracks[track].notes[note].channel = channel;
        }
        for (track, controls) in tracks.iter_mut().zip(rendered.controls) {
            track.controls.extend(controls);
        }
    }
    let signature = project.settings.time_signature;
    let song = MidiSong {
        name: Some(name.to_owned()),
        tracks,
        tempos,
        time_signatures: vec![TimeSignatureChange {
            tick: 0,
            numerator: signature.numerator,
            denominator: signature.denominator,
        }],
        key_signatures: Vec::new(),
        markers: Vec::new(),
        length,
    }
    .normalized();
    Ok((song, report))
}

/// A channel's volume and pan as controller messages at the start of the
/// song. A value that a MIDI device has anyway is not written.
fn mix_controls(channel: &Channel, midi_channel: u8) -> Vec<ControlCurve> {
    let at_start = |controller, value: u8| ControlCurve {
        channel: midi_channel,
        kind: ControlKind::Controller(controller),
        points: vec![ControlPoint {
            tick: 0,
            value: u16::from(value),
        }],
    };
    let mut controls = Vec::new();
    if channel.volume != DEFAULT_CHANNEL_VOLUME {
        controls.push(at_start(CC_VOLUME, volume_to_midi(channel.volume)));
    }
    if channel.pan != 0.0 {
        controls.push(at_start(CC_PAN, pan_to_midi(channel.pan)));
    }
    controls
}

/// One tempo automation clip that plays.
struct Piece<'a> {
    start: u32,
    end: u32,
    offset: u32,
    /// Position of the clip's playlist track, from the top.
    row: usize,
    id: ClipId,
    points: &'a [AutomationPoint],
}

impl Piece<'_> {
    /// Whether this clip has its way over `other` where both play: the one
    /// on the playlist track nearer the top, then the one that starts
    /// later, then the one with the lower id.
    fn beats(&self, other: &Piece<'_>) -> bool {
        let rank = |piece: &Piece<'_>| (piece.row, u32::MAX - piece.start, piece.id);
        rank(self) < rank(other)
    }

    /// The value the curve has where the clip ends, which the tempo keeps
    /// until another clip begins.
    fn last_value(&self) -> f32 {
        let end = f64::from(self.offset) + f64::from(self.end - self.start);
        curve_value(self.points, end)
    }
}

/// What the tempo does from one tick of the song until the next span
/// begins.
struct Span<'a> {
    start: u32,
    source: Source<'a>,
}

enum Source<'a> {
    /// A clip is playing: its curve, read `shift` ticks ahead of the song,
    /// modulo 2^32.
    Curve {
        points: &'a [AutomationPoint],
        shift: u32,
    },
    /// No clip is playing: the value the last one ended on.
    Hold(f32),
}

/// Settles what the tempo does along the song, the way the engine does: at
/// every tick the clip that wins among those playing or, when none is, the
/// value left by the clip that ended last.
fn tempo_spans<'a>(pieces: &[Piece<'a>]) -> Vec<Span<'a>> {
    let mut edges: Vec<u32> = pieces
        .iter()
        .flat_map(|piece| [piece.start, piece.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();

    let mut spans = Vec::new();
    // The clip the last span was made of, so that one clip that wins over
    // several edges in a row stays one span.
    let mut playing = None;
    for edge in edges {
        let covering = pieces
            .iter()
            .filter(|piece| piece.start <= edge && edge < piece.end);
        let winner = covering.reduce(|best, piece| if piece.beats(best) { piece } else { best });
        let source = match winner {
            Some(piece) if playing == Some(piece.id) => continue,
            Some(piece) => {
                playing = Some(piece.id);
                Source::Curve {
                    points: piece.points,
                    // Tick `start` of the song is tick `offset` of the
                    // curve. The difference may be negative, so it is kept
                    // modulo 2^32: added to a tick of the song, which is
                    // never before the clip's start, it gives the tick of
                    // the curve.
                    shift: piece.offset.wrapping_sub(piece.start),
                }
            }
            None => {
                playing = None;
                let ended = pieces.iter().filter(|piece| piece.end <= edge);
                let last = ended.reduce(|best, piece| {
                    let later = piece.end > best.end;
                    let wins = piece.end == best.end && piece.beats(best);
                    if later || wins { piece } else { best }
                });
                match last {
                    Some(last) => Source::Hold(last.last_value()),
                    None => continue,
                }
            }
        };
        spans.push(Span {
            start: edge,
            source,
        });
    }
    spans
}

/// The tempo of the song as the events of a file: the project's tempo, and
/// what the tempo automation makes of it up to tick `end`.
fn tempo_changes(project: &Project, end: u32, step: u32) -> Vec<TempoChange> {
    let playlist = &project.playlist;
    let mut pieces = Vec::new();
    for clip in &playlist.clips {
        let ClipContent::Automation { automation } = clip.content else {
            continue;
        };
        let row = playlist
            .tracks
            .iter()
            .position(|track| track.id == clip.track);
        let Some(row) = row.filter(|&row| !clip.muted && !playlist.tracks[row].muted) else {
            continue;
        };
        let automation = project.automation(automation);
        let Some(automation) = automation.filter(|a| a.target == AutomationTarget::Tempo) else {
            continue;
        };
        if automation.points.is_empty() || clip.length == 0 {
            continue;
        }
        pieces.push(Piece {
            start: clip.start,
            end: clip.start.saturating_add(clip.length),
            offset: clip.offset,
            row,
            id: clip.id,
            points: &automation.points,
        });
    }
    let spans = tempo_spans(&pieces);
    let step = step.max(1).max(end / MAX_TEMPO_STEPS);

    let mut changes = vec![TempoChange::from_bpm(0, project.settings.tempo_bpm)];
    let mut set = |tick: u32, value: f32| {
        let tempo = f64::from(AutomationRange::TEMPO.value(value));
        let micros_per_quarter = micros_per_quarter(tempo);
        match changes.last_mut() {
            Some(last) if last.tick == tick => last.micros_per_quarter = micros_per_quarter,
            Some(last) if last.micros_per_quarter == micros_per_quarter => {}
            _ => changes.push(TempoChange {
                tick,
                micros_per_quarter,
            }),
        }
    };
    for (index, span) in spans.iter().enumerate() {
        let until = spans.get(index + 1).map_or(end, |next| next.start).min(end);
        if span.start >= until {
            continue;
        }
        match span.source {
            Source::Hold(value) => set(span.start, value),
            Source::Curve { points, shift } => {
                let mut tick = span.start;
                while tick < until {
                    // Where the song is on the curve, and the points on
                    // either side of that place.
                    let place = tick.wrapping_add(shift);
                    let after = points.partition_point(|point| point.tick <= place);
                    let from = after.checked_sub(1).and_then(|at| points.get(at));
                    let to = points.get(after);
                    let corner = to.map_or(until, |to| to.tick.wrapping_sub(shift).min(until));
                    let glides = from
                        .zip(to)
                        .is_some_and(|(from, to)| !from.hold && from.value != to.value);
                    if glides {
                        let stop = corner.min(tick.saturating_add(step));
                        let middle = (f64::from(tick) + f64::from(stop)) / 2.0;
                        set(
                            tick,
                            curve_value(points, middle + f64::from(place) - f64::from(tick)),
                        );
                        tick = stop;
                    } else {
                        set(tick, curve_value(points, f64::from(place)));
                        tick = corner;
                    }
                }
            }
        }
    }

    // A change that leads back to the tempo before it on the same tick
    // leaves two events alike in a row.
    changes.dedup_by(|next, kept| next.micros_per_quarter == kept.micros_per_quarter);
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_delays_the_second_step_of_each_pair_as_the_engine_does() {
        let length = 3_840.0;
        assert_eq!(swing_warp(0.0, 1.0, length), 0.0);
        assert_eq!(swing_warp(240.0, 1.0, length), 320.0);
        assert_eq!(swing_warp(480.0, 1.0, length), 480.0);
        assert_eq!(swing_warp(720.0, 0.5, length), 760.0);
        assert_eq!(swing_warp(240.0, 0.0, length), 240.0);
        // Past the swung part of the pattern nothing moves.
        assert_eq!(swing_warp(3_840.0 + 240.0, 1.0, length), 4_080.0);
    }

    #[test]
    fn tracks_take_every_channel_but_the_drum_channel_in_turn() {
        let channels: Vec<u8> = (0..17).map(midi_channel).collect();
        assert_eq!(
            channels,
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 0, 1]
        );
    }
}
