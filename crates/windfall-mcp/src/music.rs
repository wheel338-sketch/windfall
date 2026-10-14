//! Musical positions, lengths, note names and keys, written the way an
//! agent writes them, and turned into the ticks a [`Command`] takes.
//!
//! A position is `bar:beat:tick`: the bar and the beat count from one and
//! the tick from zero inside the beat, as the playlist and the piano roll
//! show them. A beat is the meter's notated beat, so in 6/8 it is an eighth
//! note. Meter changes are followed through [`MeterMap`]. A position may
//! also be given as plain ticks: [`PPQ`] ticks are a quarter note.
//!
//! A length is a note value, `1/4`, `1/8.` (dotted), `1/8t` (triplet) or
//! `3/16`; or `bars:beats:ticks` counted from zero in the base meter; or
//! plain ticks.
//!
//! [`Command`]: windfall_project::Command

use serde_json::Value;
use windfall_core::PPQ;
use windfall_project::{MeterChange, MeterMap, MusicalPosition, TimeSignature};

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The ticks of a whole note.
const WHOLE: u32 = PPQ * 4;

/// The MIDI key of a note name. Scientific pitch: `C4` is 60 and `A4` is
/// 69. Sharps are `#` or `♯`, flats `b` or `♭`, at most two of them; the
/// octave runs from -1 to 9.
pub fn parse_pitch(text: &str) -> Result<u8, String> {
    let text = text.trim();
    let mut chars = text.chars();
    let letter = chars.next().map(|c| c.to_ascii_uppercase());
    let class: i32 = match letter {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(not_a_pitch(text)),
    };
    let rest: String = chars.collect();
    let mut accidental = 0i32;
    let mut octave = rest.as_str();
    for (index, c) in rest.char_indices() {
        let step = match c {
            '#' | '♯' => 1,
            'b' | '♭' => -1,
            _ => {
                octave = &rest[index..];
                break;
            }
        };
        accidental += step;
        octave = &rest[index + c.len_utf8()..];
        if accidental.abs() > 2 {
            return Err(not_a_pitch(text));
        }
    }
    let octave: i32 = octave.parse().map_err(|_| not_a_pitch(text))?;
    if !(-1..=9).contains(&octave) {
        return Err(not_a_pitch(text));
    }
    let key = (octave + 1) * 12 + class + accidental;
    u8::try_from(key)
        .ok()
        .filter(|key| *key <= 127)
        .ok_or_else(|| not_a_pitch(text))
}

fn not_a_pitch(text: &str) -> String {
    format!(
        "{text:?} is not a note name between C-1 and G9; write a name such as C4, F#3 or Bb2, or a MIDI number from 0 to 127"
    )
}

/// The name of a MIDI key, with sharps: 60 is `C4`.
pub fn pitch_name(key: u8) -> String {
    let octave = i32::from(key / 12) - 1;
    format!("{}{octave}", NOTE_NAMES[usize::from(key % 12)])
}

/// A pitch given as a MIDI number or a note name.
pub fn pitch_value(value: &Value) -> Result<u8, String> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .filter(|key| *key <= 127)
            .map(|key| key as u8)
            .ok_or_else(|| format!("pitch {number} is outside the MIDI range 0 to 127")),
        Value::String(text) => parse_pitch(text),
        other => Err(format!(
            "pitch {other} must be a note name such as C4 or a MIDI number"
        )),
    }
}

/// Converts between ticks and positions in one meter context: a pattern's
/// own, or the song's.
pub struct Grid {
    map: MeterMap,
    base: TimeSignature,
    /// The positions this grid holds: a pattern's ticks or the song's.
    limit: u32,
}

impl Grid {
    pub fn new(base: TimeSignature, changes: &[MeterChange], limit: u32) -> Result<Self, String> {
        Ok(Self {
            map: MeterMap::new(base, changes)?,
            base,
            limit,
        })
    }

    pub fn base(&self) -> TimeSignature {
        self.base
    }

    /// `bar:beat:tick`, `bar:beat` or `bar`.
    pub fn parse_position(&self, text: &str) -> Result<u32, String> {
        let parts: Vec<&str> = text.trim().split(':').collect();
        let number = |index: usize, default: u32| -> Result<u32, String> {
            match parts.get(index) {
                None => Ok(default),
                Some(part) => part.trim().parse().map_err(|_| bad_position(text)),
            }
        };
        if parts.is_empty() || parts.len() > 3 {
            return Err(bad_position(text));
        }
        let position = MusicalPosition {
            bar: number(0, 1)?,
            beat: number(1, 1)?,
            tick: number(2, 0)?,
        };
        let tick = self
            .map
            .position_to_tick(position)
            .map_err(|error| format!("position {text:?} cannot be used: {error}"))?;
        self.inside(tick)
    }

    pub fn format_position(&self, tick: u32) -> String {
        match self.map.tick_to_position(tick) {
            Ok(position) => format!("{}:{}:{}", position.bar, position.beat, position.tick),
            Err(_) => tick.to_string(),
        }
    }

    /// A position given as ticks or as `bar:beat:tick` text.
    pub fn position_value(&self, value: &Value) -> Result<u32, String> {
        match value {
            Value::Number(number) => {
                let tick = number
                    .as_u64()
                    .and_then(|tick| u32::try_from(tick).ok())
                    .ok_or_else(|| format!("position {number} must be a whole number of ticks"))?;
                self.inside(tick)
            }
            Value::String(text) => self.parse_position(text),
            other => Err(format!(
                "position {other} must be text such as \"2:1:0\" or a number of ticks"
            )),
        }
    }

    fn inside(&self, tick: u32) -> Result<u32, String> {
        if tick >= self.limit {
            return Err(format!(
                "position {} is past the end this grid holds ({} ticks)",
                self.format_position(tick),
                self.limit
            ));
        }
        Ok(tick)
    }

    /// A note value (`1/4`, `1/8.`, `1/8t`, `3/16`), a `bars:beats:ticks`
    /// duration in the base meter, or ticks. Always at least one tick.
    pub fn parse_length(&self, text: &str) -> Result<u32, String> {
        let text = text.trim();
        let ticks = if let Some((top, bottom)) = text.split_once('/') {
            let (bottom, factor) = if let Some(bottom) = bottom.strip_suffix('.') {
                (bottom, (3, 2))
            } else if let Some(bottom) = bottom
                .strip_suffix('t')
                .or_else(|| bottom.strip_suffix('T'))
            {
                (bottom, (2, 3))
            } else {
                (bottom, (1, 1))
            };
            let top: u64 = top.trim().parse().map_err(|_| bad_length(text))?;
            let bottom: u64 = bottom.trim().parse().map_err(|_| bad_length(text))?;
            if top == 0 || bottom == 0 || !bottom.is_power_of_two() || bottom > 128 {
                return Err(bad_length(text));
            }
            let numerator = u64::from(WHOLE) * top * factor.0;
            let denominator = bottom * factor.1;
            if !numerator.is_multiple_of(denominator) {
                return Err(bad_length(text));
            }
            numerator / denominator
        } else if text.contains(':') {
            let parts: Vec<&str> = text.split(':').collect();
            if parts.len() > 3 {
                return Err(bad_length(text));
            }
            let mut values = [0u64; 3];
            for (slot, part) in values.iter_mut().zip(&parts) {
                *slot = part.trim().parse().map_err(|_| bad_length(text))?;
            }
            u64::from(self.base.ticks_per_bar()) * values[0]
                + u64::from(self.base.ticks_per_beat()) * values[1]
                + values[2]
        } else {
            text.parse().map_err(|_| bad_length(text))?
        };
        u32::try_from(ticks)
            .ok()
            .filter(|ticks| *ticks >= 1 && *ticks <= self.limit)
            .ok_or_else(|| bad_length(text))
    }

    /// A length given as ticks or as text.
    pub fn length_value(&self, value: &Value) -> Result<u32, String> {
        match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|ticks| u32::try_from(ticks).ok())
                .filter(|ticks| *ticks >= 1 && *ticks <= self.limit)
                .ok_or_else(|| {
                    format!("length {number} must be a whole number of ticks, at least 1")
                }),
            Value::String(text) => self.parse_length(text),
            other => Err(format!(
                "length {other} must be a note value such as \"1/8\" or a number of ticks"
            )),
        }
    }
}

fn bad_position(text: &str) -> String {
    format!(
        "{text:?} is not a position; write bar:beat:tick counting bars and beats from 1, such as \"3:2:0\""
    )
}

fn bad_length(text: &str) -> String {
    format!(
        "{text:?} is not a length; write a note value such as \"1/4\", \"1/8.\" or \"1/16t\", a duration bars:beats:ticks such as \"0:2:0\", or ticks"
    )
}

/// `#RRGGBB` or `RRGGBB`.
pub fn parse_color(text: &str) -> Result<u32, String> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return Err(bad_color(text));
    }
    u32::from_str_radix(hex, 16).map_err(|_| bad_color(text))
}

fn bad_color(text: &str) -> String {
    format!("{text:?} is not a colour; write #RRGGBB, such as \"#3FA7D6\"")
}

pub fn format_color(color: u32) -> String {
    format!("#{color:06X}")
}

/// `3/4`, `6/8`: beats per bar from 1 to 16 over 2, 4, 8 or 16.
pub fn parse_meter(text: &str) -> Result<TimeSignature, String> {
    let bad = || {
        format!(
            "{text:?} is not a meter; write beats/unit with 1 to 16 beats over 2, 4, 8 or 16, such as \"3/4\""
        )
    };
    let (top, bottom) = text.trim().split_once('/').ok_or_else(bad)?;
    let numerator: u8 = top.trim().parse().map_err(|_| bad())?;
    let denominator: u8 = bottom.trim().parse().map_err(|_| bad())?;
    if !(1..=16).contains(&numerator) || ![2, 4, 8, 16].contains(&denominator) {
        return Err(bad());
    }
    Ok(TimeSignature {
        numerator,
        denominator,
    })
}

pub fn format_meter(meter: TimeSignature) -> String {
    format!("{}/{}", meter.numerator, meter.denominator)
}

/// A key found in a set of pitch-class weights.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEstimate {
    /// Such as "A minor".
    pub name: String,
    pub tonic: &'static str,
    pub mode: &'static str,
    /// The correlation of the weights with the key's profile, -1 to 1.
    /// Above about 0.7 the key is clear; near the runner-up it is not.
    pub confidence: f64,
    pub runner_up: String,
}

// Krumhansl and Kessler's probe-tone profiles (1982).
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// The major or minor key whose profile the weights follow best, or `None`
/// for weights with no pitch in them. Weight 0 is C.
pub fn estimate_key(weights: &[f64; 12]) -> Option<KeyEstimate> {
    let total: f64 = weights.iter().filter(|w| w.is_finite()).sum();
    if total.is_nan() || total <= 0.0 {
        return None;
    }
    let mut scores = Vec::with_capacity(24);
    for tonic in 0..12 {
        for (mode, profile) in [("major", &MAJOR), ("minor", &MINOR)] {
            let rotated: Vec<f64> = (0..12).map(|pc| profile[(pc + 12 - tonic) % 12]).collect();
            scores.push((correlation(weights, &rotated), tonic, mode));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (confidence, tonic, mode) = scores[0];
    let (_, second_tonic, second_mode) = scores[1];
    Some(KeyEstimate {
        name: format!("{} {mode}", NOTE_NAMES[tonic]),
        tonic: NOTE_NAMES[tonic],
        mode,
        confidence: (confidence * 1000.0).round() / 1000.0,
        runner_up: format!("{} {second_mode}", NOTE_NAMES[second_tonic]),
    })
}

fn correlation(a: &[f64; 12], b: &[f64]) -> f64 {
    let mean_a = a.iter().sum::<f64>() / 12.0;
    let mean_b = b.iter().sum::<f64>() / 12.0;
    let (mut cross, mut left, mut right) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        cross += (x - mean_a) * (y - mean_b);
        left += (x - mean_a).powi(2);
        right += (y - mean_b).powi(2);
    }
    if left <= 0.0 || right <= 0.0 {
        0.0
    } else {
        cross / (left * right).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use windfall_project::{MAX_PATTERN_TICKS, MeterChangeId};

    const FOUR: TimeSignature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };

    #[test]
    fn note_names_and_midi_numbers_read_both_ways() {
        assert_eq!(parse_pitch("C4").unwrap(), 60);
        assert_eq!(parse_pitch("a4").unwrap(), 69);
        assert_eq!(parse_pitch("F#3").unwrap(), 54);
        assert_eq!(parse_pitch("Bb2").unwrap(), 46);
        assert_eq!(parse_pitch("E♭5").unwrap(), 75);
        assert_eq!(parse_pitch("C-1").unwrap(), 0);
        assert_eq!(parse_pitch("G9").unwrap(), 127);
        for bad in ["H2", "C", "C10", "G#9", "Cb-1", "C###4", "", "4C", "C 4x"] {
            assert!(parse_pitch(bad).is_err(), "{bad}");
        }
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(0), "C-1");
        assert_eq!(pitch_name(127), "G9");
        assert_eq!(pitch_value(&json!(64)).unwrap(), 64);
        assert_eq!(pitch_value(&json!("E4")).unwrap(), 64);
        assert!(pitch_value(&json!(128)).is_err());
        assert!(pitch_value(&json!(-1)).is_err());
        assert!(pitch_value(&json!(true)).is_err());
    }

    #[test]
    fn positions_count_bars_and_beats_from_one_and_follow_meter_changes() {
        let grid = Grid::new(FOUR, &[], MAX_PATTERN_TICKS).unwrap();
        assert_eq!(grid.parse_position("1:1:0").unwrap(), 0);
        assert_eq!(grid.parse_position("1:2").unwrap(), PPQ);
        assert_eq!(grid.parse_position("2").unwrap(), 4 * PPQ);
        assert_eq!(grid.parse_position("2:3:120").unwrap(), 6 * PPQ + 120);
        assert_eq!(grid.format_position(6 * PPQ + 120), "2:3:120");
        assert_eq!(grid.position_value(&json!(480)).unwrap(), 480);
        for bad in ["0:1:0", "1:5:0", "1:1:960", "x", "1:1:1:1"] {
            assert!(grid.parse_position(bad).is_err(), "{bad}");
        }
        assert!(grid.position_value(&json!(MAX_PATTERN_TICKS)).is_err());
        // 3/4 from bar 2 of a 4/4 song.
        let song = Grid::new(
            FOUR,
            &[MeterChange {
                id: MeterChangeId(9),
                tick: 4 * PPQ,
                signature: TimeSignature {
                    numerator: 3,
                    denominator: 4,
                },
            }],
            u32::MAX,
        )
        .unwrap();
        assert_eq!(song.parse_position("3:1:0").unwrap(), 7 * PPQ);
        assert!(song.parse_position("2:4:0").is_err());
    }

    #[test]
    fn lengths_take_note_values_durations_and_ticks() {
        let grid = Grid::new(FOUR, &[], MAX_PATTERN_TICKS).unwrap();
        assert_eq!(grid.parse_length("1/4").unwrap(), PPQ);
        assert_eq!(grid.parse_length("1/8.").unwrap(), PPQ * 3 / 4);
        assert_eq!(grid.parse_length("1/8t").unwrap(), PPQ / 3);
        assert_eq!(grid.parse_length("3/16").unwrap(), PPQ * 3 / 4);
        assert_eq!(grid.parse_length("1:0:0").unwrap(), 4 * PPQ);
        assert_eq!(grid.parse_length("0:2").unwrap(), 2 * PPQ);
        assert_eq!(grid.parse_length("240").unwrap(), 240);
        assert_eq!(grid.length_value(&json!(10)).unwrap(), 10);
        for bad in ["0/4", "1/3", "1/0", "0", "1/256", "-1", "1/4x"] {
            assert!(grid.parse_length(bad).is_err(), "{bad}");
        }
        assert!(grid.length_value(&json!(0)).is_err());
    }

    #[test]
    fn colours_and_meters_are_checked() {
        assert_eq!(parse_color("#3FA7D6").unwrap(), 0x3FA7D6);
        assert_eq!(parse_color("00ff00").unwrap(), 0x00FF00);
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("#GG0000").is_err());
        assert_eq!(format_color(0x0000FF), "#0000FF");
        assert_eq!(
            parse_meter("6/8").unwrap(),
            TimeSignature {
                numerator: 6,
                denominator: 8
            }
        );
        for bad in ["0/4", "17/4", "4/3", "4", "a/b"] {
            assert!(parse_meter(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_scale_s_pitch_classes_give_its_key() {
        // C major scale with the tonic triad weighted up.
        let mut weights = [0.0; 12];
        for (pc, weight) in [
            (0, 3.0),
            (2, 1.0),
            (4, 2.0),
            (5, 1.0),
            (7, 2.0),
            (9, 1.0),
            (11, 1.0),
        ] {
            weights[pc] = weight;
        }
        let key = estimate_key(&weights).unwrap();
        assert_eq!(key.name, "C major");
        assert!(key.confidence > 0.7, "{key:?}");
        // A natural minor with the A minor triad weighted up.
        let mut weights = [0.0; 12];
        for (pc, weight) in [
            (9, 3.0),
            (11, 1.0),
            (0, 2.0),
            (2, 1.0),
            (4, 2.0),
            (5, 1.0),
            (7, 1.0),
        ] {
            weights[pc] = weight;
        }
        assert_eq!(estimate_key(&weights).unwrap().name, "A minor");
        assert_eq!(estimate_key(&[0.0; 12]), None);
    }
}
