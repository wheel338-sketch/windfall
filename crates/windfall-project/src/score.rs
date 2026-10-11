//! Score files: the notes and note events of one piano-roll lane, kept as a
//! reusable file.
//!
//! A score is Windfall's own format. It holds what a lane holds, in the
//! project's own units: notes at 960 ticks to the quarter note with their
//! velocity, pan and expression, and the expression curves drawn on them.
//! Nothing in it names a project, a pattern or a channel, so a score loads
//! into any lane. `windfall-midi` converts a score to and from a Standard
//! MIDI File for other programs.
//!
//! Loading is one undo step built from ordinary commands, so everything a
//! command checks is checked for a score too. Every loaded note gets a new
//! id from the destination project, and the curves follow their notes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::{PPQ, TICKS_PER_STEP};

use crate::command::{Command, NoteInit, PatternPatch};
use crate::model::{
    ChannelId, MAX_KEY, MAX_PATTERN_STEPS, MAX_PATTERN_TICKS, NoteId, PatternId, Project,
    TimeSignature,
};
use crate::note_curves::{MAX_NOTE_CURVE_POINTS, MAX_PATTERN_CURVE_POINTS};
use crate::{NoteCurveInsert, NoteExpression};

/// What the `schema` field of a score file says.
pub const SCORE_SCHEMA: &str = "windfall.score";
/// The newest score version this build reads and the one it writes.
pub const SCORE_VERSION: u32 = 1;
/// The most notes one score holds: what one command adds.
pub const MAX_SCORE_NOTES: usize = crate::piano_tools::MAX_TOOL_NOTES;
/// The longest score name, in bytes.
pub const MAX_SCORE_NAME_BYTES: usize = 256;
/// The largest score file a reader takes in.
pub const MAX_SCORE_FILE_BYTES: usize = 32 * 1024 * 1024;

/// One note of a score: a [`Note`](crate::Note) without an id.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreNote {
    /// Ticks from the start of the score.
    pub start: u32,
    /// Ticks, at least 1.
    pub length: u32,
    /// MIDI key, 0 to 127.
    pub key: u8,
    /// 0 to 1.
    pub velocity: f32,
    /// -1 to 1.
    #[serde(default)]
    pub pan: f32,
    #[serde(default, skip_serializing_if = "NoteExpression::is_default")]
    #[ts(as = "Option<NoteExpression>", optional)]
    pub expression: NoteExpression,
}

/// A score file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Score {
    /// Always [`SCORE_SCHEMA`].
    pub schema: String,
    pub version: u32,
    pub name: String,
    /// Ticks per quarter note of every time in the file. Always 960.
    pub ppq: u32,
    /// How long the score is, in ticks: the length of the pattern it was
    /// taken from, or of the file it was converted from. Never shorter than
    /// the end of its last note.
    pub length_ticks: u32,
    /// The meter the score was written in, when it had one of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub time_signature: Option<TimeSignature>,
    /// Sorted by start, then key.
    pub notes: Vec<ScoreNote>,
    /// Expression curves, each on the note at `note_index` of `notes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<NoteCurveInsert>>", optional)]
    pub curves: Vec<NoteCurveInsert>,
}

/// What loading a score does with the notes the lane already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScoreLoadMode {
    /// The lane ends up holding the score and nothing else.
    Replace,
    /// The score is added to what the lane holds.
    Merge,
}

/// Only what tells whether a file is a score this build can read.
#[derive(Deserialize)]
struct Header {
    schema: Option<String>,
    version: Option<u32>,
}

impl Score {
    /// An empty score of the given length.
    pub fn new(name: impl Into<String>, length_ticks: u32) -> Self {
        Self {
            schema: SCORE_SCHEMA.to_owned(),
            version: SCORE_VERSION,
            name: name.into(),
            ppq: PPQ,
            length_ticks,
            time_signature: None,
            notes: Vec::new(),
            curves: Vec::new(),
        }
    }

    /// The notes `channel` plays in `pattern`, with their curves. With
    /// `only`, just the notes it names, at the positions they have. The
    /// score is named after the pattern and the channel.
    pub fn capture(
        project: &Project,
        pattern: PatternId,
        channel: ChannelId,
        only: Option<&[NoteId]>,
    ) -> Result<Self, String> {
        let source = project
            .pattern(pattern)
            .ok_or("The pattern of the score no longer exists")?;
        let rack = project
            .channel(channel)
            .ok_or("The channel of the score no longer exists")?;
        let notes: Vec<_> = source
            .lane(channel)
            .map(|lane| lane.notes.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|note| only.is_none_or(|ids| ids.contains(&note.id)))
            .collect();
        if notes.is_empty() {
            return Err("There are no notes to save as a score".to_owned());
        }
        if notes.len() > MAX_SCORE_NOTES {
            return Err(format!(
                "A score holds at most {MAX_SCORE_NOTES} notes; this selection has {}",
                notes.len()
            ));
        }
        let mut score = Self::new(
            format!("{} - {}", source.name, rack.name),
            source.length_ticks(),
        );
        score.time_signature = source.time_signature;
        for (index, note) in notes.iter().enumerate() {
            score.notes.push(ScoreNote {
                start: note.start,
                length: note.length,
                key: note.key,
                velocity: note.velocity,
                pan: note.pan,
                expression: note.expression,
            });
            score.length_ticks = score
                .length_ticks
                .max(note.start.saturating_add(note.length));
            for curve in source
                .note_curves
                .iter()
                .filter(|curve| curve.note == note.id)
            {
                score.curves.push(NoteCurveInsert {
                    note_index: index as u32,
                    parameter: curve.parameter,
                    points: curve.points.clone(),
                });
            }
        }
        score.normalize_name();
        score.validate()?;
        Ok(score)
    }

    fn normalize_name(&mut self) {
        let mut name: String = self.name.chars().filter(|c| !c.is_control()).collect();
        while name.len() > MAX_SCORE_NAME_BYTES {
            name.pop();
        }
        self.name = name.trim().to_owned();
    }

    /// Says what stops this score being loaded, if anything does.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCORE_SCHEMA {
            return Err("This file is not a Windfall score".to_owned());
        }
        if self.version == 0 {
            return Err("The score has no version".to_owned());
        }
        if self.version > SCORE_VERSION {
            return Err(format!(
                "This score was saved by a newer Windfall (score version {}); this build reads up to version {SCORE_VERSION}",
                self.version
            ));
        }
        if self.ppq != PPQ {
            return Err(format!(
                "The score counts {} ticks per quarter note, and a score always counts {PPQ}",
                self.ppq
            ));
        }
        if self.name.len() > MAX_SCORE_NAME_BYTES || self.name.chars().any(char::is_control) {
            return Err("The score name is too long or has control characters".to_owned());
        }
        if self.notes.len() > MAX_SCORE_NOTES {
            return Err(format!("A score holds at most {MAX_SCORE_NOTES} notes"));
        }
        if self.length_ticks > MAX_PATTERN_TICKS {
            return Err("The score is longer than the longest pattern".to_owned());
        }
        if let Some(signature) = self.time_signature
            && (!(1..=16).contains(&signature.numerator)
                || ![2, 4, 8, 16].contains(&signature.denominator))
        {
            return Err("The score has a time signature Windfall does not have".to_owned());
        }
        for note in &self.notes {
            let end = note.start.checked_add(note.length);
            if note.length == 0
                || note.key > MAX_KEY
                || note.start >= MAX_PATTERN_TICKS
                || end.is_none_or(|end| end > self.length_ticks)
                || !note.velocity.is_finite()
                || !(0.0..=1.0).contains(&note.velocity)
                || !note.pan.is_finite()
                || !(-1.0..=1.0).contains(&note.pan)
                || note.expression.clamped() != note.expression
            {
                return Err(
                    "The score has a note outside its length or outside a note's ranges".to_owned(),
                );
            }
        }
        let mut targets = std::collections::HashSet::new();
        let mut points = 0_usize;
        for curve in &self.curves {
            if curve.note_index as usize >= self.notes.len()
                || !targets.insert((curve.note_index, curve.parameter))
                || curve.points.is_empty()
                || curve.points.len() > MAX_NOTE_CURVE_POINTS
            {
                return Err(
                    "The score has an expression curve without a note of its own".to_owned(),
                );
            }
            points = points.saturating_add(curve.points.len());
            let (min, max) = curve.parameter.range();
            let mut previous = None;
            for point in &curve.points {
                if !point.position.is_finite()
                    || !(0.0..=1.0).contains(&point.position)
                    || previous.is_some_and(|previous| point.position <= previous)
                    || !point.value.is_finite()
                    || !(min..=max).contains(&point.value)
                    || !point.curve.is_finite()
                    || !(-1.0..=1.0).contains(&point.curve)
                {
                    return Err("The score has an invalid expression curve".to_owned());
                }
                previous = Some(point.position);
            }
        }
        if points > MAX_PATTERN_CURVE_POINTS {
            return Err(
                "The score has more expression curve points than a pattern holds".to_owned(),
            );
        }
        Ok(())
    }

    /// The file a score is stored as. The same score always gives the same
    /// text.
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }

    /// Reads a score file. A file of a newer version is refused by name
    /// before anything else about it is looked at.
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_SCORE_FILE_BYTES {
            return Err("The score file is larger than 32 MiB".to_owned());
        }
        let header: Header = serde_json::from_slice(bytes)
            .map_err(|error| format!("This file is not a Windfall score: {error}"))?;
        if header.schema.as_deref() != Some(SCORE_SCHEMA) {
            return Err("This file is not a Windfall score".to_owned());
        }
        match header.version {
            Some(version) if version > SCORE_VERSION => {
                return Err(format!(
                    "This score was saved by a newer Windfall (score version {version}); this build reads up to version {SCORE_VERSION}"
                ));
            }
            Some(0) | None => return Err("The score has no version".to_owned()),
            Some(_) => {}
        }
        let score: Score = serde_json::from_slice(bytes)
            .map_err(|error| format!("The score file is damaged: {error}"))?;
        score.validate()?;
        Ok(score)
    }

    /// The one undo step that loads this score into the lane `channel` has
    /// in `pattern`.
    ///
    /// A pattern shorter than the score is made long enough to play all of
    /// it, up to the longest pattern there is. With
    /// [`ScoreLoadMode::Replace`] the lane's notes are removed first, which
    /// takes their curves with them. Dispatch the command to the document
    /// `project` belongs to straight away: it is built for the project as it
    /// is now.
    pub fn load_command(
        &self,
        project: &Project,
        pattern: PatternId,
        channel: ChannelId,
        mode: ScoreLoadMode,
    ) -> Result<Command, String> {
        self.validate()?;
        let target = project
            .pattern(pattern)
            .ok_or("The pattern to load the score into no longer exists")?;
        if project.channel(channel).is_none() {
            return Err("The channel to load the score into no longer exists".to_owned());
        }
        if self.notes.is_empty() {
            return Err("The score has no notes".to_owned());
        }
        let mut commands = Vec::new();
        let steps = self
            .length_ticks
            .div_ceil(TICKS_PER_STEP)
            .min(MAX_PATTERN_STEPS);
        if steps > target.length_steps {
            commands.push(Command::UpdatePattern {
                id: pattern,
                patch: PatternPatch {
                    length_steps: Some(steps),
                    ..PatternPatch::default()
                },
            });
        }
        let has_notes = target
            .lane(channel)
            .is_some_and(|lane| !lane.notes.is_empty());
        if mode == ScoreLoadMode::Replace && has_notes {
            commands.push(Command::ClearLane { pattern, channel });
        }
        commands.push(Command::AddNotesWithCurves {
            pattern,
            channel,
            notes: self
                .notes
                .iter()
                .map(|note| NoteInit {
                    start: note.start,
                    length: note.length,
                    key: note.key,
                    velocity: Some(note.velocity),
                    pan: Some(note.pan),
                    expression: Some(note.expression),
                })
                .collect(),
            curves: self.curves.clone(),
        });
        Ok(Command::Batch {
            label: Some("Load score".to_owned()),
            commands,
        })
    }
}
