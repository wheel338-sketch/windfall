//! MIDI capture payloads use the existing saved expression/curve model.
use windfall_engine::midi_hardware::MidiMessage;
use windfall_project::{NoteCurveInsert, NoteCurveParameter, NoteCurvePoint, NoteExpression};

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct ExpressionCapture {
    pub initial: NoteExpression,
    pub source: Option<(u8, u8)>,
    pub samples: Vec<(NoteCurveParameter, f64, f32)>,
}

pub(super) fn movement(message: MidiMessage) -> Option<(u8, NoteCurveParameter, f32)> {
    Some(match message {
        MidiMessage::PitchBend { channel, value } => (
            channel,
            NoteCurveParameter::FinePitchCents,
            (f32::from(value) - 8192.0) / if value < 8192 { 8192.0 } else { 8191.0 } * 200.0,
        ),
        MidiMessage::Controller {
            channel,
            number: 74,
            value,
        } => (
            channel,
            NoteCurveParameter::ModulationX,
            f32::from(value) / 127.0,
        ),
        MidiMessage::Controller {
            channel,
            number: 1,
            value,
        }
        | MidiMessage::ChannelPressure { channel, value } => (
            channel,
            NoteCurveParameter::ModulationY,
            f32::from(value) / 127.0,
        ),
        _ => return None,
    })
}

pub(super) fn set(expression: &mut NoteExpression, parameter: NoteCurveParameter, value: f32) {
    match parameter {
        NoteCurveParameter::FinePitchCents => expression.fine_pitch_cents = value,
        NoteCurveParameter::ModulationX => expression.modulation_x = value,
        NoteCurveParameter::ModulationY => expression.modulation_y = value,
        _ => {}
    }
}

impl ExpressionCapture {
    pub fn curves(&self, start: f64, length: f64, note_index: u32) -> Vec<NoteCurveInsert> {
        let mut curves = Vec::new();
        for (parameter, initial) in [
            (
                NoteCurveParameter::FinePitchCents,
                self.initial.fine_pitch_cents,
            ),
            (NoteCurveParameter::ModulationX, self.initial.modulation_x),
            (NoteCurveParameter::ModulationY, self.initial.modulation_y),
        ] {
            let mut points = vec![NoteCurvePoint {
                position: 0.0,
                value: initial,
                curve: 0.0,
                hold: true,
            }];
            for &(_, tick, value) in self.samples.iter().filter(|(p, _, _)| *p == parameter) {
                let position = ((tick - start) / length.max(1.0)).clamp(0.0, 1.0) as f32;
                let point = NoteCurvePoint {
                    position,
                    value,
                    curve: 0.0,
                    hold: true,
                };
                if points.last().is_some_and(|p| p.position == position) {
                    *points.last_mut().unwrap() = point;
                } else if points.len() < windfall_project::note_curves::MAX_NOTE_CURVE_POINTS {
                    points.push(point);
                }
            }
            if points.len() > 1 || points[0].value != initial {
                curves.push(NoteCurveInsert {
                    note_index,
                    parameter,
                    points,
                });
            }
        }
        curves
    }
}
