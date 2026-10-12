//! Value-only source identity and musical clock for one runtime occurrence.
use crate::plan::Plan;
use windfall_dsp::NoteExpression;
use windfall_project::{NoteId, PatternId};

#[derive(Debug, Clone, Copy)]
pub(crate) struct CurveSource {
    pub performance: bool,
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
        let tick = if self.performance { f64::from_bits(plan.performance_tick.load(std::sync::atomic::Ordering::Relaxed)) } else { tick };
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
        if self.performance { return; }
        self.start += ticks;
        self.end += ticks;
        if let Some(origin) = &mut self.song_origin {
            *origin += ticks;
        }
    }
    pub fn move_clock(&mut self, moved: &impl Fn(f64) -> f64) {
        if self.performance { return; }
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

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    fn every_performance_curve_parameter_reads_musical_phase_instead_of_stopped_song_clock() {
        use windfall_project::{NoteCurveParameter as Parameter, NoteCurvePoint, NoteExpressionCurve};
        let mut project = windfall_project::Project::new("curve clock");
        let pattern = project.patterns[0].id;
        let note = NoteId(999);
        for (parameter, a, b) in [(Parameter::Pan, -1.0, 1.0), (Parameter::Release, 0.0, 1.0), (Parameter::FinePitchCents, -1200.0, 1200.0), (Parameter::ModulationX, 0.0, 1.0), (Parameter::ModulationY, 1.0, 0.0)] {
            project.patterns[0].note_curves.push(NoteExpressionCurve { note, parameter, points: vec![
                NoteCurvePoint { position: 0.0, value: a, curve: 0.0, hold: false },
                NoteCurvePoint { position: 1.0, value: b, curve: 0.0, hold: false },
            ] });
        }
        // Compile copies curves verbatim; no sounding note or device required.
        let plan = crate::plan::compile(&project, &crate::SamplePool::new());
        plan.performance_tick.store(340.0_f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
        let source = CurveSource { performance: true, pattern, note, start: 100.0, end: 1060.0, song_origin: None, base_pan: 0.0, base_expression: Default::default(), active: true };
        let (pan, expression) = source.controls(&plan, 999999.0);
        assert_eq!(pan, -0.5);
        assert_eq!(expression.release, 0.25);
        assert_eq!(expression.fine_pitch_cents, -600.0);
        assert_eq!(expression.modulation_x, 0.25);
        assert_eq!(expression.modulation_y, 0.75);
    }
}
