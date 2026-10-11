//! The project timebase: how finely playback and export count musical time.
//!
//! A project stores every position in ticks of [`PPQ`], 960 to the quarter
//! note, and that never changes: lowering the timebase does not rewrite or
//! lose what was drawn, and raising it again brings the stored timing back.
//!
//! What the setting changes is where things happen when the project plays.
//! With a timebase of `ppq` ticks per quarter note, time moves on a grid of
//! `960 / ppq` stored ticks, and [`playback_project`] puts every note and
//! every playlist clip on it:
//!
//! - **Playback and audio export.** The engine compiles its plan from the
//!   project [`playback_project`] returns, and an export compiles the same
//!   plan, so both sound the notes and clips at their grid positions.
//! - **MIDI export.** The file is written from the same project, at `ppq`
//!   ticks per quarter note unless another resolution is asked for, so it
//!   holds exactly what plays.
//! - **Import.** A file's own resolution is converted to 960 without loss
//!   where that is possible, and the imported material then plays on the
//!   project's grid like everything else. An FL Studio project brings its
//!   own timebase along when it is one Windfall has.
//!
//! Tempo and meter changes, markers and the points of automation curves are
//! not events on the grid: curves are read continuously and the others stay
//! where they were put.
//!
//! At the default of 960 the grid is one stored tick and nothing moves.

use std::borrow::Cow;

use windfall_core::{PPQ, TICKS_PER_STEP};

use crate::model::{Clip, Note, Project};

/// The timebases a project can have, in ticks per quarter note. Each
/// divides 960 and a sixteenth-note step, so bars, beats and steps are
/// always on the grid.
pub const TIMEBASES: [u16; 8] = [24, 48, 96, 120, 192, 240, 480, 960];

/// The timebase of a project whose file names none: no rounding at all.
pub const DEFAULT_TIMEBASE_PPQ: u16 = PPQ as u16;

/// For serde: the timebase of a file that has none.
pub fn default_ppq() -> u16 {
    DEFAULT_TIMEBASE_PPQ
}

/// For serde: whether the timebase is the one a file leaves out.
pub fn is_default_ppq(ppq: &u16) -> bool {
    *ppq == DEFAULT_TIMEBASE_PPQ
}

/// Whether `ppq` is one of [`TIMEBASES`].
pub fn is_timebase(ppq: u16) -> bool {
    TIMEBASES.contains(&ppq)
}

/// How many stored ticks one tick of the timebase lasts. A value that is
/// not a timebase counts as the default, whose grid is 1.
pub fn grid_ticks(ppq: u16) -> u32 {
    if is_timebase(ppq) {
        PPQ / u32::from(ppq)
    } else {
        1
    }
}

/// The grid position nearest to `tick`. A tick exactly between two goes to
/// the later one.
pub fn quantize_tick(tick: u32, grid: u32) -> u32 {
    if grid <= 1 {
        return tick;
    }
    let grid = u64::from(grid);
    let snapped = (u64::from(tick) + grid / 2) / grid * grid;
    u32::try_from(snapped).unwrap_or(u32::MAX / grid as u32 * grid as u32)
}

/// Where a note of `start` and `length` plays on the grid, in a pattern
/// `pattern_ticks` long. A note keeps at least one grid tick of length. A
/// note that starts inside the pattern stays inside it, so that rounding
/// never silences a note that would otherwise play.
pub fn quantize_note(start: u32, length: u32, pattern_ticks: u32, grid: u32) -> (u32, u32) {
    if grid <= 1 {
        return (start, length);
    }
    let mut snapped = quantize_tick(start, grid);
    if start < pattern_ticks && snapped >= pattern_ticks {
        snapped = pattern_ticks.saturating_sub(1) / grid * grid;
    }
    let end = quantize_tick(start.saturating_add(length), grid);
    (snapped, end.saturating_sub(snapped).max(grid))
}

/// Where a clip span plays on the grid. A clip keeps at least one grid
/// tick of length.
pub fn quantize_span(start: u32, length: u32, grid: u32) -> (u32, u32) {
    if grid <= 1 {
        return (start, length);
    }
    let snapped = quantize_tick(start, grid);
    let end = quantize_tick(start.saturating_add(length), grid);
    (snapped, end.saturating_sub(snapped).max(grid))
}

/// The project as it plays: every note and every playlist clip on the grid
/// of the project's timebase. Borrowed and untouched at the default
/// timebase.
///
/// The result is for playing and exporting, not for editing: it is not
/// stored, and its ids are those of the project it was made from.
pub fn playback_project(project: &Project) -> Cow<'_, Project> {
    let grid = grid_ticks(project.settings.timebase_ppq);
    if grid <= 1 {
        return Cow::Borrowed(project);
    }
    let mut played = project.clone();
    for pattern in &mut played.patterns {
        let pattern_ticks = pattern.length_steps.max(1).saturating_mul(TICKS_PER_STEP);
        for lane in &mut pattern.lanes {
            for note in &mut lane.notes {
                (note.start, note.length) =
                    quantize_note(note.start, note.length, pattern_ticks, grid);
            }
            lane.notes.sort_by_key(Note::sort_key);
        }
    }
    for clip in &mut played.playlist.clips {
        (clip.start, clip.length) = quantize_span(clip.start, clip.length, grid);
        clip.offset = quantize_tick(clip.offset, grid);
    }
    played.playlist.clips.sort_by_key(Clip::sort_key);
    Cow::Owned(played)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_timebase_divides_a_quarter_note_and_a_step() {
        for ppq in TIMEBASES {
            let grid = grid_ticks(ppq);
            assert_eq!(grid * u32::from(ppq), PPQ, "{ppq}");
            assert_eq!(TICKS_PER_STEP % grid, 0, "{ppq}");
        }
        assert_eq!(grid_ticks(DEFAULT_TIMEBASE_PPQ), 1);
        for invalid in [0, 1, 72, 100, 384, 961, u16::MAX] {
            assert!(!is_timebase(invalid));
            assert_eq!(grid_ticks(invalid), 1);
        }
    }

    #[test]
    fn ticks_round_to_the_nearest_grid_position_and_halves_go_later() {
        assert_eq!(quantize_tick(0, 10), 0);
        assert_eq!(quantize_tick(4, 10), 0);
        assert_eq!(quantize_tick(5, 10), 10);
        assert_eq!(quantize_tick(14, 10), 10);
        assert_eq!(quantize_tick(15, 10), 20);
        assert_eq!(quantize_tick(123, 1), 123);
        assert_eq!(quantize_tick(123, 0), 123);
        // The last position a u32 holds, not a wrapped one.
        assert_eq!(quantize_tick(u32::MAX, 40), u32::MAX / 40 * 40);
    }

    #[test]
    fn notes_keep_a_length_and_stay_inside_their_pattern() {
        // A one-tick note still lasts one grid tick.
        assert_eq!(quantize_note(12, 1, 3840, 10), (10, 10));
        // Start and end round on their own, so a note does not drift long.
        assert_eq!(quantize_note(14, 12, 3840, 10), (10, 20));
        // The last ticks of a pattern would round onto its end, where
        // nothing plays; they take the last grid position instead.
        assert_eq!(quantize_note(3838, 2, 3840, 40), (3800, 40));
        // A note past the end was silent already and is only rounded.
        assert_eq!(quantize_note(3850, 20, 3840, 40), (3840, 40));
        assert_eq!(quantize_note(7, 9, 3840, 1), (7, 9));
    }

    #[test]
    fn the_default_timebase_borrows_the_project_unchanged() {
        let project = Project::new("Fine");
        assert!(matches!(playback_project(&project), Cow::Borrowed(_)));
    }
}
