//! Deterministic selected-note tools, shared by desktop and the WASM document.
//! These run on the document thread; they never participate in audio callbacks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CommandError, MAX_KEY, MAX_PATTERN_TICKS, Note, NoteId};

/// A bound on both the input selection and the output of a tool. In particular,
/// a one-tick chop cannot accidentally create hundreds of thousands of notes.
pub const MAX_TOOL_NOTES: usize = 16_384;
pub const MAX_RHYTHM_STEPS: usize = 64;

fn one() -> f64 {
    1.0
}

/// A boundary within one repeating Chop period. Gate scales the retained
/// piece (including partial edges); velocity multiplies the source dynamic.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ChopStep {
    pub tick: u32,
    #[serde(default = "one")]
    pub gate: f64,
    #[serde(default = "one")]
    pub velocity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ArpDirection {
    Ascending,
    Descending,
    Alternating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FlamPosition {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RhythmMode {
    Remove,
    Add,
    Shift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteEdge {
    Start,
    End,
}

/// Original, repeating timing patterns. Offsets are fractions of the grid:
/// Swing delays odd divisions by 1/6, LatePairs by 1/4, and PushFour advances
/// every fourth division by 1/6. Straight has no offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteGroove {
    Straight,
    Swing,
    LatePairs,
    PushFour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteLfoProperty { Velocity, Pan, Release, FinePitchCents, ModulationX, ModulationY }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(export)]
pub enum NoteTransform {
    /// Extend/trim to the next distinct selected onset; keep the last chord.
    Legato,
    /// Multiply lengths by a finite factor in (0, 1]. Round, minimum one tick.
    Staccato {
        factor: f64,
    },
    /// Legato lengths plus `overlap` ticks, and every note after the first
    /// selected onset becomes a portamento note gliding for `glide` ticks.
    /// The first onset and slide notes keep their articulation.
    Portamento {
        glide: u32,
        overlap: u32,
    },
    /// Split at absolute grid boundaries. Keep the first segment's id.
    Chop {
        grid: u32,
    },
    /// Repeat boundaries in both directions from origin. First step is zero,
    /// ticks strictly increase below period. Never discard partial edges.
    ChopPattern {
        origin: u32,
        period: u32,
        steps: Vec<ChopStep>,
    },
    /// Exact-onset chords, ordered by key/id across ascending octave copies.
    /// Repetitions 0 fills the original longest voice span, requiring exact
    /// rate divisibility; 1..64 produces that many complete traversals.
    Arpeggiate {
        rate: u32,
        gate: f64,
        octaves: u8,
        repetitions: u32,
        direction: ArpDirection,
    },
    Flam {
        interval: u32,
        velocity: f64,
        position: FlamPosition,
    },
    /// Match onset cells floor((start-origin)/step) mod period == phase.
    /// Remove matches, add shifted copies, or shift the originals.
    RhythmReshape {
        origin: u32,
        step: u32,
        period: u32,
        phase: u32,
        offset: i32,
        mode: RhythmMode,
    },
    /// Union touching/overlapping notes with identical key, velocity and pan.
    Glue,
    /// Each exact-onset chord, ordered by pitch then id (or the reverse pitch
    /// order), gets successive delays and velocity offsets. Lengths stay put.
    Strum {
        spacing: u32,
        velocity_step: f64,
        descending: bool,
    },
    FlipTime,
    FlipPitch,
    /// Transpose, then clamp to the inclusive MIDI range. With octaves, choose
    /// the nearest octave-equivalent key in range, falling back to clamping
    /// when the chosen range has no representative of the pitch class.
    KeyRange {
        low: u8,
        high: u8,
        transpose: i32,
        octaves: bool,
    },
    ScaleVelocity {
        factor: f64,
    },
    /// Seeded offsets preserve each source note's identity and expression.
    Randomize {
        seed: u32,
        pitch: u8,
        velocity: f64,
        pan: f64,
        timing: u32,
        length: f64,
    },
    /// Replace the selection with seeded grid hits from a relative chord map.
    GenerateRandom {
        seed: u32,
        grid: u32,
        density: f64,
        gate: f64,
        root: u8,
        pitch_classes: u16,
        low: u8,
        high: u8,
        velocity_low: f64,
        velocity_high: f64,
    },
    /// Sample a musical LFO at selected onsets into an event property.
    Lfo {
        property: NoteLfoProperty,
        origin: u32,
        strength: f64,
        lfo: crate::CurveLfo,
    },
    Quantize {
        grid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        musical: Option<crate::NoteMusicalGrid>,
        strength: f64,
        edge: NoteEdge,
        groove: NoteGroove,
    },
}

impl NoteTransform {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Legato => "Legato notes",
            Self::Staccato { .. } => "Staccato notes",
            Self::Portamento { .. } => "Portamento notes",
            Self::Chop { .. } | Self::ChopPattern { .. } => "Chop notes",
            Self::Arpeggiate { .. } => "Arpeggiate notes",
            Self::Flam { .. } => "Flam notes",
            Self::RhythmReshape { .. } => "Reshape note rhythm",
            Self::Glue => "Glue notes",
            Self::Strum { .. } => "Strum notes",
            Self::FlipTime => "Flip note time",
            Self::FlipPitch => "Flip note pitch",
            Self::KeyRange { .. } => "Limit and transpose notes",
            Self::ScaleVelocity { .. } => "Scale note velocities",
            Self::Randomize { .. } => "Randomize notes",
            Self::GenerateRandom { .. } => "Generate random notes",
            Self::Lfo { .. } => "Write note-event LFO",
            Self::Quantize {
                edge: NoteEdge::Start,
                ..
            } => "Quantize note starts",
            Self::Quantize {
                edge: NoteEdge::End,
                ..
            } => "Quantize note ends",
        }
    }

    fn check(&self) -> Result<(), CommandError> {
        if let Self::ChopPattern { steps, .. } = self
            && steps.len() > MAX_RHYTHM_STEPS
        {
            return Err(CommandError::invalid("Chop allows at most 64 boundaries"));
        }
        let tool = self.clone();
        match tool {
            Self::ChopPattern {
                origin,
                period,
                steps,
            } => {
                grid(period)?;
                if origin > MAX_PATTERN_TICKS
                    || steps.is_empty()
                    || steps.len() > MAX_RHYTHM_STEPS
                    || steps[0].tick != 0
                    || steps.iter().any(|s| s.tick >= period)
                    || steps.windows(2).any(|s| s[0].tick >= s[1].tick)
                {
                    return Err(CommandError::invalid(
                        "use 1 to 64 increasing Chop boundaries starting at zero, below the period",
                    ));
                }
                for step in steps {
                    positive_gate("Chop gate", step.gate)?;
                    number("Chop velocity", step.velocity, 0.0, 4.0)?;
                }
                Ok(())
            }
            Self::Arpeggiate {
                rate,
                gate,
                octaves,
                repetitions,
                ..
            } => {
                grid(rate)?;
                positive_gate("arpeggio gate", gate)?;
                if !(1..=8).contains(&octaves) || repetitions > 64 {
                    return Err(CommandError::invalid(
                        "arpeggio needs 1 to 8 octaves and 0 to 64 repetitions",
                    ));
                }
                Ok(())
            }
            Self::Flam {
                interval, velocity, ..
            } => {
                if !(1..=960).contains(&interval) {
                    return Err(CommandError::invalid(
                        "Flam interval must be 1 to 960 ticks",
                    ));
                }
                number("Flam velocity", velocity, 0.0, 1.0)
            }
            Self::RhythmReshape {
                origin,
                step,
                period,
                phase,
                offset,
                mode,
            } => {
                grid(step)?;
                if origin > MAX_PATTERN_TICKS
                    || !(1..=64).contains(&period)
                    || phase >= period
                    || offset.unsigned_abs() > MAX_PATTERN_TICKS
                    || (mode == RhythmMode::Add && offset == 0)
                {
                    return Err(CommandError::invalid(
                        "invalid rhythm origin, period, phase or offset (Add needs a nonzero offset)",
                    ));
                }
                Ok(())
            }
            Self::Staccato { factor } => number("length factor", factor, f64::MIN_POSITIVE, 1.0),
            Self::Portamento { glide, overlap } => {
                if !(1..=MAX_PATTERN_TICKS).contains(&glide) || overlap > MAX_PATTERN_TICKS {
                    return Err(CommandError::invalid(
                        "portamento needs a glide of 1 to 245760 ticks and an overlap inside the pattern",
                    ));
                }
                Ok(())
            }
            Self::ScaleVelocity { factor } => number("velocity factor", factor, 0.0, 4.0),
            Self::Randomize { pitch, velocity, pan, timing, length, .. } => {
                if pitch > MAX_KEY || timing > MAX_PATTERN_TICKS {
                    return Err(CommandError::invalid("random pitch or timing range is too large"));
                }
                number("random velocity range", velocity, 0.0, 1.0)?;
                number("random pan range", pan, 0.0, 2.0)?;
                number("random length range", length, 0.0, 1.0)
            }
            Self::Lfo { origin, strength, lfo, .. } => {
                if origin > MAX_PATTERN_TICKS { return Err(CommandError::invalid("LFO origin is past the pattern limit")); }
                lfo.check()?;
                number("LFO strength", strength, 0.0, 1.0)
            }
            Self::GenerateRandom { grid: value, density, gate, root, pitch_classes, low, high, velocity_low, velocity_high, .. } => {
                grid(value)?;
                number("random density", density, 0.0, 1.0)?;
                positive_gate("random gate", gate)?;
                number("random minimum velocity", velocity_low, 0.0, 1.0)?;
                number("random maximum velocity", velocity_high, velocity_low, 1.0)?;
                if root > 11 || pitch_classes == 0 || pitch_classes > 0x0fff || low > high || high > MAX_KEY {
                    return Err(CommandError::invalid("invalid chord map, root or MIDI key range"));
                }
                Ok(())
            }
            Self::Chop { grid: value } => grid(value),
            Self::Quantize {
                grid: value,
                strength,
                musical,
                ..
            } => {
                grid(value)?;
                if let Some(musical) = musical { musical.segments()?; }
                number("quantize strength", strength, 0.0, 1.0)
            }
            Self::Strum {
                spacing,
                velocity_step,
                ..
            } => {
                if spacing > MAX_PATTERN_TICKS {
                    return Err(CommandError::invalid("strum spacing is too large"));
                }
                number("strum velocity step", velocity_step, -1.0, 1.0)
            }
            Self::KeyRange {
                low,
                high,
                transpose,
                ..
            } => {
                if low > high || high > MAX_KEY || !(-127..=127).contains(&transpose) {
                    return Err(CommandError::invalid("invalid MIDI key range or transpose"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn number(name: &str, value: f64, low: f64, high: f64) -> Result<(), CommandError> {
    if !value.is_finite() || !(low..=high).contains(&value) {
        return Err(CommandError::invalid(format!("invalid {name}")));
    }
    Ok(())
}

fn positive_gate(name: &str, value: f64) -> Result<(), CommandError> {
    number(name, value, 0.0, 1.0)?;
    if value == 0.0 {
        return Err(CommandError::invalid(format!("invalid {name}")));
    }
    Ok(())
}

fn grid(value: u32) -> Result<(), CommandError> {
    if value == 0 || value > MAX_PATTERN_TICKS {
        return Err(CommandError::invalid("grid must be 1 to 245760 ticks"));
    }
    Ok(())
}

fn valid_note(note: &Note) -> Result<(), CommandError> {
    if note.id.0 == 0
        || note.key > MAX_KEY
        || note.length == 0
        || u64::from(note.start) + u64::from(note.length) > u64::from(MAX_PATTERN_TICKS)
        || !note.velocity.is_finite()
        || !(0.0..=1.0).contains(&note.velocity)
        || !note.pan.is_finite()
        || !(-1.0..=1.0).contains(&note.pan)
        || !note.expression.valid()
    {
        return Err(CommandError::invalid(
            "selected note is invalid or outside the pattern limit",
        ));
    }
    Ok(())
}

/// Normalize a captured selection. Identical duplicate ids are harmless;
/// conflicting snapshots of one id are rejected. Result is in lane order.
pub(crate) fn selection(notes: &[Note]) -> Result<Vec<Note>, CommandError> {
    if notes.is_empty() || notes.len() > MAX_TOOL_NOTES {
        return Err(CommandError::invalid(
            "select 1 to 16384 notes for this tool",
        ));
    }
    let mut unique = BTreeMap::new();
    for note in notes {
        valid_note(note)?;
        if unique
            .insert(note.id, *note)
            .is_some_and(|old| old != *note)
        {
            return Err(CommandError::invalid("conflicting selected note snapshots"));
        }
    }
    let mut notes: Vec<_> = unique.into_values().collect();
    notes.sort_by_key(Note::sort_key);
    Ok(notes)
}

/// Pure tool calculation. New pieces use id 0 until the transaction allocates
/// ids. The output is bounded before any document or ID mutation.
pub fn transform_selected_notes(notes: &[Note], tool: NoteTransform) -> Result<Vec<Note>, CommandError> {
    let (mut notes, sources) = transform_mapped(notes, tool, &[])?;
    for note in &mut notes { if sources.contains_key(&note.id) { note.id = NoteId(0); } }
    Ok(notes)
}

struct GeneratedSources {
    next: u32,
    used: std::collections::BTreeSet<NoteId>,
    origins: BTreeMap<NoteId, NoteId>,
}
impl GeneratedSources {
    fn new(notes: &[Note]) -> Self {
        Self { next: u32::MAX, used: notes.iter().map(|note| note.id).collect(), origins: BTreeMap::new() }
    }
    fn copy(&mut self, source: NoteId) -> NoteId {
        // Input/output cardinalities are bounded to 16,384. At most twice that
        // many reserved candidates can be skipped; these ids never leave here.
        while self.used.contains(&NoteId(self.next)) { self.next -= 1; }
        let id = NoteId(self.next); self.next -= 1;
        self.used.insert(id); self.origins.insert(id, source); id
    }
}

pub(crate) fn transform_mapped(
    notes: &[Note],
    tool: NoteTransform,
    curves: &[crate::NoteExpressionCurve],
) -> Result<(Vec<Note>, BTreeMap<NoteId, NoteId>), CommandError> {
    tool.check()?;
    let mut notes = selection(notes)?;
    let curve_classes = crate::note_curves::classes(curves, &notes);
    let mut generated_sources = GeneratedSources::new(&notes);
    match tool {
        NoteTransform::ChopPattern {
            origin,
            period,
            steps,
        } => {
            let mut pieces = Vec::new();
            for note in notes {
                let end = note.start + note.length;
                let mut start = note.start;
                while start < end {
                    let relative = i64::from(start) - i64::from(origin);
                    let phase = relative.rem_euclid(i64::from(period)) as u32;
                    let index = steps.partition_point(|s| s.tick <= phase) - 1;
                    let next_phase = steps.get(index + 1).map_or(period, |s| s.tick);
                    let next = end.min(start + next_phase - phase);
                    let accent = steps[index];
                    push_piece(
                        &mut pieces,
                        Note {
                            id: if start == note.start {
                                note.id
                            } else {
                                generated_sources.copy(note.id)
                            },
                            start,
                            length: gated(next - start, accent.gate),
                            velocity: (f64::from(note.velocity) * accent.velocity).min(1.0) as f32,
                            ..note
                        },
                    )?;
                    start = next;
                }
            }
            notes = pieces;
        }
        NoteTransform::Arpeggiate {
            rate,
            gate,
            octaves,
            repetitions,
            direction,
        } => {
            let mut pieces = Vec::new();
            let mut first = 0;
            while first < notes.len() {
                let onset = notes[first].start;
                let end = first + notes[first..].partition_point(|n| n.start == onset);
                let chord = &notes[first..end];
                // Construct the pitch ladder only after its bound is checked.
                let voices = chord.len() * usize::from(octaves);
                if voices > MAX_TOOL_NOTES {
                    return Err(output_limit());
                }
                let mut ladder = Vec::with_capacity(voices);
                for octave in 0..octaves {
                    for note in chord {
                        let key = u16::from(note.key) + u16::from(octave) * 12;
                        if key > u16::from(MAX_KEY) {
                            return Err(CommandError::invalid(
                                "arpeggio octave exceeds MIDI key 127",
                            ));
                        }
                        ladder.push(Note {
                            key: key as u8,
                            ..*note
                        });
                    }
                }
                ladder.sort_by_key(|n| (n.key, n.id));
                if direction == ArpDirection::Descending {
                    ladder.sort_by(|a, b| b.key.cmp(&a.key).then(a.id.cmp(&b.id)));
                }
                let traversal = if direction == ArpDirection::Alternating && voices > 1 {
                    voices * 2 - 2
                } else {
                    voices
                };
                let slots = if repetitions == 0 {
                    let span = chord.iter().map(|n| n.length).max().unwrap();
                    if span % rate != 0 {
                        return Err(CommandError::invalid(
                            "original chord span must contain complete arpeggio rate slots; choose fixed repetitions or another rate",
                        ));
                    }
                    (span / rate) as usize
                } else {
                    traversal * repetitions as usize
                };
                if slots > MAX_TOOL_NOTES - pieces.len() {
                    return Err(output_limit());
                }
                if u64::from(onset) + slots as u64 * u64::from(rate) > u64::from(MAX_PATTERN_TICKS)
                {
                    return Err(CommandError::invalid(
                        "complete arpeggio slots exceed the pattern limit",
                    ));
                }
                let mut retained = std::collections::BTreeSet::new();
                for slot in 0..slots {
                    let rank = slot % traversal;
                    let index = if rank < voices {
                        rank
                    } else {
                        traversal - rank
                    };
                    let voice = ladder[index];
                    push_piece(
                        &mut pieces,
                        Note {
                            id: if retained.insert(voice.id) {
                                voice.id
                            } else {
                                generated_sources.copy(voice.id)
                            },
                            start: onset + slot as u32 * rate,
                            length: gated(rate, gate),
                            ..voice
                        },
                    )?;
                }
                first = end;
            }
            notes = pieces;
        }
        NoteTransform::Flam {
            interval,
            velocity,
            position,
        } => {
            if notes.len() > MAX_TOOL_NOTES / 2 {
                return Err(output_limit());
            }
            let mut pieces = Vec::with_capacity(notes.len() * 2);
            for note in notes {
                let start = match position {
                    FlamPosition::Before => note.start.checked_sub(interval),
                    FlamPosition::After => note.start.checked_add(interval),
                }
                .ok_or_else(|| {
                    CommandError::invalid("Flam grace hit would start before tick zero")
                })?;
                push_piece(
                    &mut pieces,
                    Note {
                        id: generated_sources.copy(note.id),
                        start,
                        length: interval.min(note.length),
                        velocity: (f64::from(note.velocity) * velocity) as f32,
                        ..note
                    },
                )?;
                pieces.push(note);
            }
            notes = pieces;
        }
        NoteTransform::RhythmReshape {
            origin,
            step,
            period,
            phase,
            offset,
            mode,
        } => {
            let matches = |n: &Note| {
                (i64::from(n.start) - i64::from(origin))
                    .div_euclid(i64::from(step))
                    .rem_euclid(i64::from(period))
                    == i64::from(phase)
            };
            match mode {
                RhythmMode::Remove => notes.retain(|n| !matches(n)),
                RhythmMode::Shift => {
                    for note in notes.iter_mut().filter(|n| matches(n)) {
                        note.start = shifted(note, offset)?;
                    }
                }
                RhythmMode::Add => {
                    let mut existing: std::collections::BTreeSet<_> =
                        notes.iter().map(|note| (identity(note), curve_classes.get(&note.id).copied())).collect();
                    let mut additions = Vec::new();
                    for note in notes.iter().filter(|n| matches(n)) {
                        let added = Note {
                            id: generated_sources.copy(note.id),
                            start: shifted(note, offset)?,
                            ..*note
                        };
                        if existing.insert((identity(&added), curve_classes.get(&note.id).copied())) {
                            if notes.len() + additions.len() == MAX_TOOL_NOTES {
                                return Err(output_limit());
                            }
                            additions.push(added);
                        }
                    }
                    notes.extend(additions);
                }
            }
        }
        NoteTransform::Legato => {
            let mut starts: Vec<_> = notes.iter().map(|n| n.start).collect();
            starts.dedup();
            for note in &mut notes {
                if let Some(next) = starts.get(starts.partition_point(|s| *s <= note.start)) {
                    note.length = next - note.start;
                }
            }
        }
        NoteTransform::Portamento { glide, overlap } => {
            let mut starts: Vec<_> = notes.iter().map(|n| n.start).collect();
            starts.dedup();
            let first = starts[0];
            for note in &mut notes {
                if let Some(next) = starts.get(starts.partition_point(|s| *s <= note.start)) {
                    // Reach into the next onset, never past the longest pattern.
                    let room = MAX_PATTERN_TICKS - note.start;
                    note.length = (next - note.start).saturating_add(overlap).min(room);
                }
                let slide = note.expression.articulation == crate::NoteArticulation::Slide;
                if note.start != first && !slide {
                    note.expression.articulation = crate::NoteArticulation::Portamento;
                    note.expression.glide_ticks = glide;
                }
            }
        }
        NoteTransform::Staccato { factor } => {
            for note in &mut notes {
                note.length = (f64::from(note.length) * factor).round().max(1.0) as u32;
            }
        }
        NoteTransform::Chop { grid } => {
            let count: usize = notes
                .iter()
                .map(|n| ((n.start + n.length - 1) / grid - n.start / grid + 1) as usize)
                .sum();
            if count > MAX_TOOL_NOTES {
                return Err(CommandError::invalid(
                    "chop would exceed 16384 notes; choose a coarser grid",
                ));
            }
            let mut pieces = Vec::with_capacity(count);
            for note in notes {
                let end = note.start + note.length;
                let mut start = note.start;
                while start < end {
                    let next = ((start / grid + 1) * grid).min(end);
                    pieces.push(Note {
                        id: if start == note.start {
                            note.id
                        } else {
                            generated_sources.copy(note.id)
                        },
                        start,
                        length: next - start,
                        ..note
                    });
                    start = next;
                }
            }
            notes = pieces;
        }
        NoteTransform::Glue => {
            notes.sort_by(|a, b| {
                a.key
                    .cmp(&b.key)
                    // Inputs are finite. Numeric ordering keeps -0 and +0
                    // together, matching the compatibility comparison below.
                    .then(a.velocity.partial_cmp(&b.velocity).unwrap())
                    .then(a.pan.partial_cmp(&b.pan).unwrap())
                    .then(a.expression.release.partial_cmp(&b.expression.release).unwrap())
                    .then(a.expression.fine_pitch_cents.partial_cmp(&b.expression.fine_pitch_cents).unwrap())
                    .then(a.expression.modulation_x.partial_cmp(&b.expression.modulation_x).unwrap())
                    .then(a.expression.modulation_y.partial_cmp(&b.expression.modulation_y).unwrap())
                    .then((a.expression.articulation as u8).cmp(&(b.expression.articulation as u8)))
                    .then(a.expression.glide_ticks.cmp(&b.expression.glide_ticks))
                    .then(a.expression.color_group.cmp(&b.expression.color_group))
                    .then(curve_classes.get(&a.id).cmp(&curve_classes.get(&b.id)))
                    .then(a.start.cmp(&b.start))
                    .then(a.id.cmp(&b.id))
            });
            let mut glued: Vec<Note> = Vec::with_capacity(notes.len());
            for note in notes {
                if let Some(last) = glued.last_mut()
                    && last.key == note.key
                    && last.velocity == note.velocity
                    && last.pan == note.pan
                    && last.expression == note.expression
                    && curve_classes.get(&last.id) == curve_classes.get(&note.id)
                    && note.start <= last.start + last.length
                {
                    last.length =
                        (last.start + last.length).max(note.start + note.length) - last.start;
                } else {
                    glued.push(note);
                }
            }
            notes = glued;
        }
        NoteTransform::Strum {
            spacing,
            velocity_step,
            descending,
        } => {
            let mut first = 0;
            while first < notes.len() {
                let onset = notes[first].start;
                let end = first + notes[first..].partition_point(|n| n.start == onset);
                if descending {
                    notes[first..end].sort_by(|a, b| b.key.cmp(&a.key).then(a.id.cmp(&b.id)));
                }
                for (rank, note) in notes[first..end].iter_mut().enumerate() {
                    let start = u64::from(onset) + rank as u64 * u64::from(spacing);
                    if start + u64::from(note.length) > u64::from(MAX_PATTERN_TICKS) {
                        return Err(CommandError::invalid(
                            "strum would end past the longest pattern",
                        ));
                    }
                    note.start = start as u32;
                    note.velocity = (f64::from(note.velocity) + rank as f64 * velocity_step)
                        .clamp(0.0, 1.0) as f32;
                }
                first = end;
            }
        }
        NoteTransform::FlipTime => {
            let left = notes.iter().map(|n| n.start).min().unwrap();
            let right = notes.iter().map(|n| n.start + n.length).max().unwrap();
            for note in &mut notes {
                note.start = left + right - note.start - note.length;
            }
        }
        NoteTransform::FlipPitch => {
            let low = notes.iter().map(|n| n.key).min().unwrap();
            let high = notes.iter().map(|n| n.key).max().unwrap();
            for note in &mut notes {
                note.key = (u16::from(low) + u16::from(high) - u16::from(note.key)) as u8;
            }
        }
        NoteTransform::KeyRange {
            low,
            high,
            transpose,
            octaves,
        } => {
            for note in &mut notes {
                let key = i32::from(note.key) + transpose;
                let target = if octaves {
                    (i32::from(low)..=i32::from(high))
                        .filter(|k| (k - key).rem_euclid(12) == 0)
                        .min_by_key(|k| ((k - key).abs(), *k))
                } else {
                    None
                };
                note.key =
                    target.unwrap_or_else(|| key.clamp(i32::from(low), i32::from(high))) as u8;
            }
        }
        NoteTransform::ScaleVelocity { factor } => {
            for note in &mut notes {
                note.velocity = (f64::from(note.velocity) * factor).clamp(0.0, 1.0) as f32;
            }
        }
        NoteTransform::Randomize { seed, pitch, velocity, pan, timing, length } => {
            let mut random = NoteRandom::new(seed);
            for note in &mut notes {
                note.key = (i32::from(note.key) + random.integer(i32::from(pitch))).clamp(0, i32::from(MAX_KEY)) as u8;
                note.velocity = (f64::from(note.velocity) + random.signed() * velocity).clamp(0.0, 1.0) as f32;
                note.pan = (f64::from(note.pan) + random.signed() * pan).clamp(-1.0, 1.0) as f32;
                note.start = (i64::from(note.start) + i64::from(random.integer(timing as i32)))
                    .clamp(0, i64::from(MAX_PATTERN_TICKS - note.length)) as u32;
                note.length = (f64::from(note.length) * (1.0 + random.signed() * length)).round()
                    .clamp(1.0, f64::from(MAX_PATTERN_TICKS - note.start)) as u32;
            }
        }
        NoteTransform::GenerateRandom { seed, grid, density, gate, root, pitch_classes, low, high, velocity_low, velocity_high } => {
            let first = notes.iter().map(|note| note.start).min().unwrap_or(0);
            let end = notes.iter().map(|note| note.start + note.length).max().unwrap_or(first);
            let cells = (end - first).div_ceil(grid) as usize;
            if cells > MAX_TOOL_NOTES { return Err(output_limit()); }
            let keys: Vec<u8> = (low..=high).filter(|key| {
                let relative = (u16::from(*key) + 12 - u16::from(root)) % 12;
                pitch_classes & (1_u16 << relative) != 0
            }).collect();
            if keys.is_empty() { return Err(CommandError::invalid("the chord map has no key in this MIDI range")); }
            let mut random = NoteRandom::new(seed);
            let mut generated = Vec::with_capacity(cells);
            for cell in 0..cells {
                if random.unit() >= density { continue; }
                let source = notes[random.index(notes.len())];
                let start = first + cell as u32 * grid;
                let length = gated(grid.min(end - start), gate).min(end - start);
                generated.push(Note {
                    id: generated_sources.copy(source.id), start, length, key: keys[random.index(keys.len())],
                    velocity: (velocity_low + random.unit() * (velocity_high - velocity_low)) as f32,
                    ..source
                });
            }
            notes = generated;
        }
        NoteTransform::Lfo { property, origin, strength, lfo } => {
            for note in &mut notes {
                let normalized = f64::from(lfo.value_at(f64::from(note.start) - f64::from(origin)));
                let mix = |old: f32, target: f64| (f64::from(old) + (target - f64::from(old)) * strength) as f32;
                match property {
                    NoteLfoProperty::Velocity => note.velocity = mix(note.velocity, normalized),
                    NoteLfoProperty::Pan => note.pan = mix(note.pan, normalized * 2.0 - 1.0),
                    NoteLfoProperty::Release => note.expression.release = mix(note.expression.release, normalized),
                    NoteLfoProperty::FinePitchCents => note.expression.fine_pitch_cents = mix(note.expression.fine_pitch_cents, normalized * 2400.0 - 1200.0),
                    NoteLfoProperty::ModulationX => note.expression.modulation_x = mix(note.expression.modulation_x, normalized),
                    NoteLfoProperty::ModulationY => note.expression.modulation_y = mix(note.expression.modulation_y, normalized),
                }
            }
        }
        NoteTransform::Quantize {
            grid,
            musical,
            strength,
            edge,
            groove,
        } => {
            let segments = musical.map(|musical| musical.segments()).transpose()?;
            for note in &mut notes {
                let old = match edge {
                    NoteEdge::Start => note.start,
                    NoteEdge::End => note.start + note.length,
                };
                let target = if let Some(segments) = &segments {
                    let &(start, end, spacing) = segments.iter().rev().find(|(start, _, _)| *start <= old).unwrap_or(&segments[0]);
                    start.saturating_add(nearest(old.saturating_sub(start), spacing, groove)).min(end)
                } else { nearest(old, grid, groove) };
                let tick = (f64::from(old) + (f64::from(target) - f64::from(old)) * strength)
                    .round() as u32;
                match edge {
                    NoteEdge::Start => note.start = tick.min(MAX_PATTERN_TICKS - note.length),
                    NoteEdge::End => {
                        note.length = tick.clamp(note.start + 1, MAX_PATTERN_TICKS) - note.start
                    }
                }
            }
        }
    }
    notes.sort_by_key(|note| (note.start, note.key, if generated_sources.origins.contains_key(&note.id) { NoteId(0) } else { note.id }));
    Ok((notes, generated_sources.origins))
}

fn gated(length: u32, gate: f64) -> u32 {
    (f64::from(length) * gate).round().max(1.0) as u32
}

/// Platform-independent, project-thread RNG; a seed never depends on the clock.
struct NoteRandom(u32);
impl NoteRandom {
    fn new(seed: u32) -> Self { Self((seed ^ 0xa3c5_9ac3).max(1)) }
    fn next(&mut self) -> u32 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.0 = value;
        value
    }
    fn unit(&mut self) -> f64 { f64::from(self.next()) / 4_294_967_296.0 }
    fn signed(&mut self) -> f64 { self.unit() * 2.0 - 1.0 }
    fn index(&mut self, count: usize) -> usize { ((u64::from(self.next()) * count as u64) >> 32) as usize }
    fn integer(&mut self, spread: i32) -> i32 { self.index((spread * 2 + 1) as usize) as i32 - spread }
}

fn output_limit() -> CommandError {
    CommandError::invalid("rhythm tool would exceed 16384 notes")
}

fn push_piece(pieces: &mut Vec<Note>, note: Note) -> Result<(), CommandError> {
    if pieces.len() == MAX_TOOL_NOTES {
        return Err(output_limit());
    }
    // ID zero is a placeholder, but every other output property is checked.
    valid_note(&Note {
        id: NoteId(1),
        ..note
    })?;
    pieces.push(note);
    Ok(())
}

fn shifted(note: &Note, offset: i32) -> Result<u32, CommandError> {
    let start = i64::from(note.start) + i64::from(offset);
    if start < 0 || start + i64::from(note.length) > i64::from(MAX_PATTERN_TICKS) {
        return Err(CommandError::invalid(
            "rhythm offset moves a complete note outside the pattern",
        ));
    }
    Ok(start as u32)
}

type NoteIdentity = (u32, u32, u8, u32, u32, u32, u32, u32, u32, u8, u32, Option<u8>);

fn identity(note: &Note) -> NoteIdentity {
    let bits = |n: f32| if n == 0.0 { 0 } else { n.to_bits() };
    (
        note.start,
        note.length,
        note.key,
        bits(note.velocity),
        bits(note.pan),
        bits(note.expression.release),
        bits(note.expression.fine_pitch_cents),
        bits(note.expression.modulation_x),
        bits(note.expression.modulation_y),
        note.expression.articulation as u8,
        note.expression.glide_ticks,
        note.expression.color_group,
    )
}

fn nearest(tick: u32, grid: u32, groove: NoteGroove) -> u32 {
    let division = i64::from(tick / grid);
    ((division - 2).max(0)..=division + 2)
        .map(|i| {
            let offset = match groove {
                NoteGroove::Swing if i % 2 == 1 => i64::from(grid) / 6,
                NoteGroove::LatePairs if i % 2 == 1 => i64::from(grid) / 4,
                NoteGroove::PushFour if i % 4 == 3 => -i64::from(grid) / 6,
                _ => 0,
            };
            (i * i64::from(grid) + offset).clamp(0, i64::from(MAX_PATTERN_TICKS)) as u32
        })
        .min_by_key(|target| (target.abs_diff(tick), std::cmp::Reverse(*target)))
        .unwrap()
}
