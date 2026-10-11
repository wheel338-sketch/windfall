//! Normalized per-note expression curves. Musical duration is owned by the note.
use crate::{NoteId, Pattern};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_NOTE_CURVE_POINTS: usize = 256;
pub const MAX_PATTERN_CURVE_POINTS: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteCurveParameter {
    Pan,
    Release,
    FinePitchCents,
    ModulationX,
    ModulationY,
}
impl NoteCurveParameter {
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Pan => (-1.0, 1.0),
            Self::FinePitchCents => (-1200.0, 1200.0),
            _ => (0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCurvePoint {
    /// Fraction of the sounding note's musical duration, from zero through one.
    pub position: f32,
    pub value: f32,
    pub curve: f32,
    pub hold: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteExpressionCurve {
    pub note: NoteId,
    pub parameter: NoteCurveParameter,
    pub points: Vec<NoteCurvePoint>,
}

/// Curve payload indexed to an input note, before native ids are allocated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCurveInsert {
    pub note_index: u32,
    pub parameter: NoteCurveParameter,
    pub points: Vec<NoteCurvePoint>,
}

pub(crate) fn classes(
    curves: &[NoteExpressionCurve],
    notes: &[crate::Note],
) -> std::collections::BTreeMap<NoteId, u32> {
    let mut groups = std::collections::BTreeMap::new();
    let mut result = std::collections::BTreeMap::new();
    let mut by_note: std::collections::BTreeMap<NoteId, Vec<_>> = std::collections::BTreeMap::new();
    for curve in curves {
        by_note.entry(curve.note).or_default().push(curve);
    }
    for note in notes {
        let mut signature: Vec<_> = by_note
            .get(&note.id)
            .into_iter()
            .flatten()
            .map(|curve| {
                (
                    curve.parameter,
                    curve
                        .points
                        .iter()
                        .map(|point| {
                            (
                                point.position.to_bits(),
                                point.value.to_bits(),
                                point.curve.to_bits(),
                                point.hold,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        signature.sort();
        let next = groups.len() as u32;
        let class = *groups.entry(signature).or_insert(next);
        result.insert(note.id, class);
    }
    result
}

impl NoteExpressionCurve {
    /// Rebase a temporal slice without restarting the original contour.
    pub(crate) fn cropped(&self, from: f64, to: f64, note: NoteId) -> Self {
        let from = from.clamp(0.0, 1.0);
        let to = to.clamp(from, 1.0);
        if from == 0.0 && to == 1.0 {
            let mut copy = self.clone();
            copy.note = note;
            return copy;
        }
        let span = (to - from).max(f64::EPSILON);
        let mut positions: Vec<f64> = self
            .points
            .iter()
            .map(|point| f64::from(point.position))
            .filter(|position| *position >= from && *position <= to)
            .collect();
        if from > f64::from(self.points[0].position)
            && from < f64::from(self.points.last().unwrap().position)
            && positions.first().copied() != Some(from)
        {
            positions.insert(0, from);
        }
        if to > f64::from(self.points[0].position)
            && to < f64::from(self.points.last().unwrap().position)
            && positions.last().copied() != Some(to)
        {
            positions.push(to);
        }
        if positions.is_empty() {
            positions.push(from);
        }
        let points = positions
            .iter()
            .enumerate()
            .map(|(index, &position)| {
                let upper = self
                    .points
                    .partition_point(|point| f64::from(point.position) <= position);
                let origin = &self.points[upper.saturating_sub(1)];
                let next = self.points.get(upper);
                let bend = next
                    .zip(positions.get(index + 1))
                    .map_or(0.0, |(next, &end)| {
                        origin.curve
                            * ((end - position) / f64::from(next.position - origin.position)) as f32
                    });
                NoteCurvePoint {
                    position: ((position - from) / span).clamp(0.0, 1.0) as f32,
                    value: self.value_at(position).unwrap_or(origin.value),
                    curve: bend.clamp(-1.0, 1.0),
                    hold: next.is_some() && origin.hold,
                }
            })
            .collect();
        Self {
            note,
            parameter: self.parameter,
            points,
        }
    }
    pub fn value_at(&self, position: f64) -> Option<f32> {
        if !position.is_finite() {
            return None;
        }
        let first = self.points.first()?;
        let at = position.clamp(0.0, 1.0);
        if at <= f64::from(first.position) {
            return Some(first.value);
        }
        let upper = self
            .points
            .partition_point(|point| f64::from(point.position) <= at);
        let from = &self.points[upper.saturating_sub(1)];
        let Some(to) = self.points.get(upper) else {
            return Some(from.value);
        };
        if from.hold {
            return Some(from.value);
        }
        let phase = (at - f64::from(from.position)) / f64::from(to.position - from.position);
        let shaped = crate::curve_shape(phase, from.curve);
        Some(from.value + (to.value - from.value) * shaped as f32)
    }
}

pub fn check(pattern: &Pattern) -> Result<(), String> {
    let ids: std::collections::HashSet<_> = pattern
        .lanes
        .iter()
        .flat_map(|lane| &lane.notes)
        .map(|note| note.id)
        .collect();
    let mut targets = std::collections::HashSet::new();
    let mut total = 0usize;
    for curve in &pattern.note_curves {
        if !ids.contains(&curve.note) || !targets.insert((curve.note, curve.parameter)) {
            return Err("note curves need unique existing source notes and parameters".into());
        }
        if curve.points.is_empty() || curve.points.len() > MAX_NOTE_CURVE_POINTS {
            return Err("a note curve needs 1 to 256 points".into());
        }
        total = total.saturating_add(curve.points.len());
        if total > MAX_PATTERN_CURVE_POINTS {
            return Err("the pattern note-curve point budget was exceeded".into());
        }
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
                return Err("note-curve positions, values or segment shapes are invalid".into());
            }
            previous = Some(point.position);
        }
    }
    Ok(())
}
