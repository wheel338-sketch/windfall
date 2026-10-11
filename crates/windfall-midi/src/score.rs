//! Scores and Standard MIDI Files.
//!
//! A [`Score`] is the native file for a lane's notes. These two functions
//! let the same notes travel as a MIDI file, for programs that read nothing
//! else, and let any MIDI file be loaded as a score.
//!
//! A MIDI file holds less than a score does, and other things. Each
//! function returns one line for everything it had to leave behind, so that
//! nothing is dropped without a word.

use windfall_project::score::MAX_SCORE_NOTES;
use windfall_project::{MAX_PATTERN_TICKS, NoteExpression, Score, ScoreNote, TimeSignature};

use crate::mix::{velocity_from_midi, velocity_to_midi};
use crate::song::{MidiNote, MidiSong, MidiTrack, TempoChange, TimeSignatureChange};

/// A score as a one-track song at `tempo_bpm`, and what a MIDI file cannot
/// hold of it.
///
/// Every note keeps its start, length and key. Velocity becomes 1 to 127
/// and a note's release its release velocity. A note's pan, the rest of its
/// expression and every expression curve are not written.
pub fn score_to_song(score: &Score, tempo_bpm: f64) -> (MidiSong, Vec<String>) {
    let neutral = NoteExpression::default();
    let mut panned = 0_usize;
    let mut expressive = 0_usize;
    let mut notes = Vec::with_capacity(score.notes.len());
    for note in &score.notes {
        let expression = note.expression.clamped();
        if note.pan != 0.0 {
            panned += 1;
        }
        if (NoteExpression {
            release: neutral.release,
            ..expression
        }) != neutral
        {
            expressive += 1;
        }
        notes.push(MidiNote {
            start: note.start,
            length: note.length.max(1),
            key: note.key.min(127),
            velocity: velocity_to_midi(note.velocity),
            release: (expression.release * 128.0).round().min(127.0) as u8,
            channel: 0,
        });
    }
    let mut left_out = Vec::new();
    if panned > 0 {
        left_out.push(format!(
            "{panned} of the notes are panned. A MIDI file has no pan for a single note, so they are centered in it."
        ));
    }
    if expressive > 0 {
        left_out.push(format!(
            "{expressive} of the notes have expression beyond their release (fine pitch, modulation, articulation or color), which this MIDI file does not hold."
        ));
    }
    if !score.curves.is_empty() {
        left_out.push(format!(
            "{} expression curves were left out: a MIDI file does not hold them.",
            score.curves.len()
        ));
    }
    let song = MidiSong {
        name: None,
        tracks: vec![MidiTrack {
            name: (!score.name.is_empty()).then(|| score.name.clone()),
            instrument: None,
            notes,
            programs: Vec::new(),
            controls: Vec::new(),
        }],
        tempos: vec![TempoChange::from_bpm(0, tempo_bpm)],
        time_signatures: score
            .time_signature
            .map(|signature| TimeSignatureChange {
                tick: 0,
                numerator: signature.numerator,
                denominator: signature.denominator,
            })
            .into_iter()
            .collect(),
        key_signatures: Vec::new(),
        markers: Vec::new(),
        length: score.length_ticks,
    }
    .normalized();
    (song, left_out)
}

/// The notes of a song as a score, and what a score cannot hold of it.
/// `fallback_name` names the score when the file names nothing.
///
/// The notes of every track and channel go into the one lane a score is,
/// at the ticks the file gives them. Velocity 1 to 127 becomes
/// `velocity / 127` and a release velocity the note's release. Controllers,
/// program changes, pitch bend, pressure, tempo and markers are not notes
/// and are left out. A song with no notes, or with more than a score
/// holds, is refused.
pub fn score_from_song(
    song: &MidiSong,
    fallback_name: &str,
) -> Result<(Score, Vec<String>), String> {
    let total: usize = song.tracks.iter().map(|track| track.notes.len()).sum();
    if total == 0 {
        return Err("The MIDI file has no notes".to_owned());
    }
    if total > MAX_SCORE_NOTES {
        return Err(format!(
            "The MIDI file has {total} notes, and a score holds at most {MAX_SCORE_NOTES}. Import it as MIDI instead."
        ));
    }
    let mut left_out = Vec::new();
    let voiced = song
        .tracks
        .iter()
        .filter(|track| !track.notes.is_empty())
        .count();
    if voiced > 1 {
        left_out.push(format!(
            "The notes of {voiced} tracks were put into one lane."
        ));
    }
    let mut cut = 0_usize;
    let mut dropped = 0_usize;
    let mut notes = Vec::with_capacity(total);
    for note in song.tracks.iter().flat_map(|track| &track.notes) {
        if note.start >= MAX_PATTERN_TICKS {
            dropped += 1;
            continue;
        }
        let end = note.end().min(MAX_PATTERN_TICKS);
        if end < note.end() {
            cut += 1;
        }
        notes.push(ScoreNote {
            start: note.start,
            length: (end - note.start).max(1),
            key: note.key.min(127),
            velocity: velocity_from_midi(note.velocity),
            pan: 0.0,
            expression: NoteExpression {
                release: f32::from(note.release.min(127)) / 128.0,
                ..NoteExpression::default()
            },
        });
    }
    if dropped > 0 {
        left_out.push(format!(
            "{dropped} notes start past the end of the longest pattern and were left out."
        ));
    }
    if cut > 0 {
        left_out.push(format!(
            "{cut} notes were cut at the end of the longest pattern."
        ));
    }
    if notes.is_empty() {
        return Err("Every note of the MIDI file starts past the end of the longest pattern".to_owned());
    }
    notes.sort_by(|a, b| (a.start, a.key).cmp(&(b.start, b.key)));
    if song
        .tracks
        .iter()
        .any(|track| !track.controls.is_empty() || !track.programs.is_empty())
    {
        left_out.push(
            "Controllers, program changes, pitch bend and pressure are not notes and were left out."
                .to_owned(),
        );
    }
    if song.tempos.len() > 1 {
        left_out.push(
            "The file's tempo changes were left out: a score plays at the project's tempo."
                .to_owned(),
        );
    }
    if !song.markers.is_empty() || !song.key_signatures.is_empty() {
        left_out.push("Markers and key signatures were left out.".to_owned());
    }

    let last_end = notes
        .iter()
        .map(|note| note.start + note.length)
        .max()
        .unwrap_or(0);
    let name = song
        .name
        .clone()
        .or_else(|| song.tracks.iter().find_map(|track| track.name.clone()))
        .unwrap_or_else(|| fallback_name.to_owned());
    let mut score = Score::new(clean(&name), song.length.min(MAX_PATTERN_TICKS).max(last_end));
    score.time_signature = song
        .time_signatures
        .first()
        .filter(|signature| signature.tick == 0)
        .map(|signature| TimeSignature {
            numerator: signature.numerator,
            denominator: signature.denominator,
        })
        .filter(|signature| {
            (1..=16).contains(&signature.numerator)
                && [2, 4, 8, 16].contains(&signature.denominator)
        });
    if score.time_signature.is_none() && !song.time_signatures.is_empty() {
        left_out.push(
            "The file's time signature is not one a score can hold and was left out.".to_owned(),
        );
    } else if song.time_signatures.len() > 1 {
        left_out.push("Only the file's first time signature was kept.".to_owned());
    }
    score.notes = notes;
    score.validate()?;
    Ok((score, left_out))
}

/// A name a score accepts: no control characters, at most 256 bytes.
fn clean(name: &str) -> String {
    let mut name: String = name.chars().filter(|c| !c.is_control()).collect();
    while name.len() > windfall_project::score::MAX_SCORE_NAME_BYTES {
        name.pop();
    }
    name.trim().to_owned()
}
