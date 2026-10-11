//! A musical LFO written into the expression curves of notes.
//!
//! Where the note tool's LFO gives each note one value, read at its start,
//! this gives each note a curve: the LFO read along the whole note, so a
//! held note moves. The LFO is the same generator automation uses, read in
//! pattern ticks from `origin`, and its 0 to 1 covers the whole range of the
//! property.

use crate::curve_lfo::{CurveLfo, CurveLfoWave};
use crate::note_curves::MAX_NOTE_CURVE_POINTS;
use crate::piano_tools::MAX_TOOL_NOTES;
use crate::{
    CommandError, MAX_PATTERN_TICKS, Note, NoteCurveParameter, NoteCurvePoint, NoteExpressionCurve,
};

/// The value a note has for `parameter` where no curve moves it.
fn stored(note: &Note, parameter: NoteCurveParameter) -> f32 {
    match parameter {
        NoteCurveParameter::Pan => note.pan,
        NoteCurveParameter::Release => note.expression.release,
        NoteCurveParameter::FinePitchCents => note.expression.fine_pitch_cents,
        NoteCurveParameter::ModulationX => note.expression.modulation_x,
        NoteCurveParameter::ModulationY => note.expression.modulation_y,
    }
}

/// The ticks, counted from the start of a note `length` long, a curve is to
/// have points on: every `resolution` ticks and the note's end and, where
/// the LFO jumps, the tick before the jump and the tick of it. A note too
/// long for 256 points at `resolution` is read on a coarser grid.
fn offsets(start: u32, length: u32, origin: u32, resolution: u32, lfo: CurveLfo) -> Vec<u32> {
    let most = MAX_NOTE_CURVE_POINTS as u32 - 1;
    let step = resolution.max(length.div_ceil(most)).max(1);
    let mut offsets: Vec<u32> = (0..length).step_by(step as usize).collect();
    offsets.push(length);
    let jumps = matches!(
        lfo.wave,
        CurveLfoWave::SawUp | CurveLfoWave::SawDown | CurveLfoWave::Square | CurveLfoWave::SampleHold
    );
    if jumps {
        // Phase of the LFO, in cycles, at the note's start and end.
        let period = f64::from(lfo.period);
        let begin = lfo.phase + (f64::from(start) - f64::from(origin)) / period;
        let end = begin + f64::from(length) / period;
        let mut edges = Vec::new();
        let mut cycle = begin.floor();
        while cycle <= end.ceil() {
            edges.push(cycle);
            if lfo.wave == CurveLfoWave::Square && lfo.width > 0.0 && lfo.width < 1.0 {
                edges.push(cycle + lfo.width);
            }
            cycle += 1.0;
        }
        let mut sharp = Vec::new();
        for edge in edges {
            let offset = ((edge - begin) * period).ceil();
            if offset < 1.0 || offset >= f64::from(length) {
                continue;
            }
            let offset = offset as u32;
            sharp.push(offset - 1);
            sharp.push(offset);
        }
        // Jumps are drawn sharp only while the curve has room for them.
        if offsets.len() + sharp.len() <= MAX_NOTE_CURVE_POINTS {
            offsets.extend(sharp);
        }
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

/// The curves the captured notes have once the LFO is written into
/// `parameter`: their curves for other properties as they were, and one new
/// curve per note for this one.
pub(crate) fn generate(
    notes: &[Note],
    curves: &[NoteExpressionCurve],
    parameter: NoteCurveParameter,
    origin: u32,
    resolution: u32,
    strength: f64,
    lfo: CurveLfo,
) -> Result<Vec<NoteExpressionCurve>, CommandError> {
    lfo.check()?;
    if notes.is_empty() || notes.len() > MAX_TOOL_NOTES {
        return Err(CommandError::invalid(
            "select 1 to 16384 notes for the curve LFO",
        ));
    }
    if origin > MAX_PATTERN_TICKS || resolution == 0 || resolution > MAX_PATTERN_TICKS {
        return Err(CommandError::invalid(
            "use an LFO origin inside the pattern and a positive tick resolution",
        ));
    }
    if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
        return Err(CommandError::invalid("invalid LFO strength"));
    }
    let (min, max) = parameter.range();
    let hold = matches!(lfo.wave, CurveLfoWave::Square | CurveLfoWave::SampleHold);
    let mut result: Vec<NoteExpressionCurve> = curves
        .iter()
        .filter(|curve| curve.parameter != parameter)
        .cloned()
        .collect();
    for note in notes {
        if note.length == 0 {
            return Err(CommandError::invalid("a selected note has no length"));
        }
        let before = curves
            .iter()
            .find(|curve| curve.note == note.id && curve.parameter == parameter);
        let offsets = offsets(note.start, note.length, origin, resolution, lfo);
        let last = offsets.len() - 1;
        let points = offsets
            .iter()
            .enumerate()
            .map(|(index, &offset)| {
                let position = f64::from(offset) / f64::from(note.length);
                let tick = f64::from(note.start) + f64::from(offset) - f64::from(origin);
                let wave = min + (max - min) * lfo.value_at(tick);
                let base = before
                    .and_then(|curve| curve.value_at(position))
                    .unwrap_or_else(|| stored(note, parameter));
                let value = f64::from(base) + (f64::from(wave) - f64::from(base)) * strength;
                NoteCurvePoint {
                    position: position as f32,
                    value: (value as f32).clamp(min, max),
                    curve: 0.0,
                    hold: hold && index != last,
                }
            })
            .collect();
        result.push(NoteExpressionCurve {
            note: note.id,
            parameter,
            points,
        });
    }
    result.sort_by_key(|curve| (curve.note, curve.parameter));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoteExpression, NoteId};

    fn note(id: u32, start: u32, length: u32) -> Note {
        Note {
            id: NoteId(id),
            start,
            length,
            key: 60,
            velocity: 0.8,
            pan: 0.0,
            expression: NoteExpression::default(),
        }
    }

    fn lfo(wave: CurveLfoWave, period: u32) -> CurveLfo {
        CurveLfo {
            wave,
            period,
            phase: 0.0,
            center: 0.5,
            depth: 0.5,
            width: 0.5,
            seed: 1,
        }
    }

    #[test]
    fn a_sine_is_read_along_the_whole_note_in_the_property_range() {
        let notes = [note(1, 0, 960)];
        let curves = generate(
            &notes,
            &[],
            NoteCurveParameter::FinePitchCents,
            0,
            120,
            1.0,
            lfo(CurveLfoWave::Sine, 960),
        )
        .unwrap();
        assert_eq!(curves.len(), 1);
        let points = &curves[0].points;
        assert_eq!(points.len(), 9);
        assert_eq!(points[0].position, 0.0);
        assert_eq!(points[8].position, 1.0);
        // Quarter of the cycle is the crest, three quarters the trough.
        assert!((points[2].value - 1200.0).abs() < 1e-3);
        assert!((points[6].value + 1200.0).abs() < 1e-3);
        assert!(points[0].value.abs() < 1e-3);
        assert!(points.iter().all(|point| !point.hold));
        assert!(points.windows(2).all(|pair| pair[0].position < pair[1].position));
    }

    #[test]
    fn the_origin_sets_the_phase_each_note_starts_in() {
        let notes = [note(1, 0, 240), note(2, 480, 240)];
        let wave = lfo(CurveLfoWave::Triangle, 960);
        let curves = generate(&notes, &[], NoteCurveParameter::Pan, 0, 240, 1.0, wave).unwrap();
        // The triangle starts at its bottom, is at the middle a quarter in and at its top a half in.
        assert!((curves[0].points[0].value - -1.0).abs() < 1e-6);
        assert!((curves[0].points[1].value - 0.0).abs() < 1e-6);
        assert!((curves[1].points[0].value - 1.0).abs() < 1e-6);
        let moved = generate(&notes, &[], NoteCurveParameter::Pan, 480, 240, 1.0, wave).unwrap();
        assert!((moved[1].points[0].value - -1.0).abs() < 1e-6);
    }

    #[test]
    fn strength_blends_with_the_stored_value_or_the_curve_that_was_there() {
        let mut plain = note(1, 0, 480);
        plain.expression.modulation_x = 0.2;
        let high = CurveLfo {
            center: 1.0,
            depth: 0.0,
            ..lfo(CurveLfoWave::Sine, 480)
        };
        let half = generate(&[plain], &[], NoteCurveParameter::ModulationX, 0, 480, 0.5, high).unwrap();
        assert!(half[0].points.iter().all(|point| (point.value - 0.6).abs() < 1e-6));
        let none = generate(&[plain], &[], NoteCurveParameter::ModulationX, 0, 480, 0.0, high).unwrap();
        assert!(none[0].points.iter().all(|point| (point.value - 0.2).abs() < 1e-6));

        let was = NoteExpressionCurve {
            note: NoteId(1),
            parameter: NoteCurveParameter::ModulationX,
            points: vec![
                NoteCurvePoint { position: 0.0, value: 0.0, curve: 0.0, hold: false },
                NoteCurvePoint { position: 1.0, value: 1.0, curve: 0.0, hold: false },
            ],
        };
        let other = NoteExpressionCurve {
            note: NoteId(1),
            parameter: NoteCurveParameter::Pan,
            points: vec![NoteCurvePoint { position: 0.0, value: 0.25, curve: 0.0, hold: false }],
        };
        let kept = generate(
            &[plain],
            &[other.clone(), was],
            NoteCurveParameter::ModulationX,
            0,
            240,
            0.0,
            high,
        )
        .unwrap();
        // Another property's curve is untouched, and no strength keeps the old contour.
        assert_eq!(kept[0], other);
        let values: Vec<f32> = kept[1].points.iter().map(|point| point.value).collect();
        assert_eq!(values, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn stepped_waves_hold_and_jumps_stay_sharp() {
        let notes = [note(1, 0, 960)];
        let square = generate(
            &notes,
            &[],
            NoteCurveParameter::ModulationY,
            0,
            480,
            1.0,
            lfo(CurveLfoWave::Square, 480),
        )
        .unwrap();
        let points = &square[0].points;
        assert!(points[..points.len() - 1].iter().all(|point| point.hold));
        assert!(!points.last().unwrap().hold);
        // High for the first half of each cycle, low for the second.
        let at = |offset: u32| {
            square[0]
                .value_at(f64::from(offset) / 960.0)
                .unwrap()
        };
        assert_eq!(at(0), 1.0);
        assert_eq!(at(239), 1.0);
        assert_eq!(at(240), 0.0);
        assert_eq!(at(479), 0.0);
        assert_eq!(at(480), 1.0);

        let saw = generate(
            &notes,
            &[],
            NoteCurveParameter::ModulationY,
            0,
            240,
            1.0,
            lfo(CurveLfoWave::SawUp, 480),
        )
        .unwrap();
        let before = saw[0].value_at(479.0 / 960.0).unwrap();
        let after = saw[0].value_at(480.0 / 960.0).unwrap();
        assert!(before > 0.99 && after < 0.01, "{before} {after}");
    }

    #[test]
    fn long_notes_fit_the_point_limit_and_bad_input_is_refused() {
        let long = [note(1, 0, MAX_PATTERN_TICKS)];
        let wave = lfo(CurveLfoWave::SawUp, 7);
        let curves = generate(&long, &[], NoteCurveParameter::Pan, 0, 1, 1.0, wave).unwrap();
        assert!(curves[0].points.len() <= MAX_NOTE_CURVE_POINTS);
        assert_eq!(curves[0].points.last().unwrap().position, 1.0);
        assert!(curves[0].points.windows(2).all(|pair| pair[0].position < pair[1].position));

        let one = [note(1, 0, 240)];
        let sine = lfo(CurveLfoWave::Sine, 240);
        assert!(generate(&[], &[], NoteCurveParameter::Pan, 0, 60, 1.0, sine).is_err());
        assert!(generate(&one, &[], NoteCurveParameter::Pan, 0, 0, 1.0, sine).is_err());
        assert!(generate(&one, &[], NoteCurveParameter::Pan, MAX_PATTERN_TICKS + 1, 60, 1.0, sine).is_err());
        assert!(generate(&one, &[], NoteCurveParameter::Pan, 0, 60, 1.5, sine).is_err());
        assert!(generate(&one, &[], NoteCurveParameter::Pan, 0, 60, f64::NAN, sine).is_err());
        let broken = CurveLfo { period: 0, ..sine };
        assert!(generate(&one, &[], NoteCurveParameter::Pan, 0, 60, 1.0, broken).is_err());
    }
}
