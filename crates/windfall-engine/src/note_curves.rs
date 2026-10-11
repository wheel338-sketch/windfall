//! Value-only source identity and musical clock for one runtime occurrence.
use crate::plan::Plan;
use windfall_dsp::NoteExpression;
use windfall_project::{NoteId, PatternId};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CurveSource {
    pub pattern: PatternId,
    pub note: NoteId,
    /// Clock-domain endpoints; a song clock uses warped ticks.
    pub start: f64,
    pub end: f64,
    pub song_origin: Option<f64>,
    pub base_pan: f32,
    pub base_expression: NoteExpression,
    pub active: bool,
}
impl CurveSource {
    pub fn controls(self, plan: &Plan, tick: f64) -> (f32, NoteExpression) {
        let musical = |tick| {
            self.song_origin
                .map_or(tick, |origin| plan.unwarp(tick - origin))
        };
        let start = musical(self.start);
        let duration = musical(self.end) - start;
        let position = if duration.is_finite() && duration > 0.0 {
            ((musical(tick) - start) / duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        plan.note_curve_controls(
            self.pattern,
            self.note,
            position,
            self.base_pan,
            self.base_expression,
        )
    }
    pub fn shift(&mut self, ticks: f64) {
        self.start += ticks;
        self.end += ticks;
        if let Some(origin) = &mut self.song_origin {
            *origin += ticks;
        }
    }
    pub fn move_clock(&mut self, moved: &impl Fn(f64) -> f64) {
        self.start = moved(self.start);
        self.end = moved(self.end);
        if let Some(origin) = &mut self.song_origin {
            *origin = moved(*origin);
        }
    }
    pub fn refresh(&mut self, plan: &Plan) {
        // A source which lost its curves still samples the neutral/base values,
        // restoring them rather than leaving its final curve value latched.
        self.active |= plan
            .pattern_ids
            .get(self.pattern.0)
            .is_some_and(|index| !plan.patterns[index].curves_for(self.note).is_empty());
    }
}
