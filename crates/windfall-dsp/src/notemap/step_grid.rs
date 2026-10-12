use super::{MappedNote, NoteTransform, velocity};
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One gate plus a -127..127 semitone offset and a 0..2 velocity multiplier.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct GridStep {
    pub gate: bool,
    pub pitch_offset: i16,
    pub velocity_scale: f32,
}
impl Default for GridStep {
    fn default() -> Self {
        Self {
            gate: true,
            pitch_offset: 0,
            velocity_scale: 1.0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StepGridParams {
    /// Whole steps per quarter-note beat, 1..16, default 4.
    pub steps_per_beat: u8,
    #[ts(as = "Vec<GridStep>")]
    pub steps: [GridStep; 16],
}
impl Default for StepGridParams {
    fn default() -> Self {
        Self {
            steps_per_beat: 4,
            steps: [GridStep::default(); 16],
        }
    }
}
param_set!(StepGridParams, "Step grid", {
    int [steps_per_beat] "stepsPerBeat" "Steps per beat" { None, 1, 16, 4 }
    toggle [steps[0].gate] "steps.0.gate" "Step 1 gate" { true }
    int [steps[0].pitch_offset] "steps.0.pitchOffset" "Step 1 pitch" { Semitones, -127, 127, 0 }
    float [steps[0].velocity_scale] "steps.0.velocityScale" "Step 1 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[1].gate] "steps.1.gate" "Step 2 gate" { true }
    int [steps[1].pitch_offset] "steps.1.pitchOffset" "Step 2 pitch" { Semitones, -127, 127, 0 }
    float [steps[1].velocity_scale] "steps.1.velocityScale" "Step 2 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[2].gate] "steps.2.gate" "Step 3 gate" { true }
    int [steps[2].pitch_offset] "steps.2.pitchOffset" "Step 3 pitch" { Semitones, -127, 127, 0 }
    float [steps[2].velocity_scale] "steps.2.velocityScale" "Step 3 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[3].gate] "steps.3.gate" "Step 4 gate" { true }
    int [steps[3].pitch_offset] "steps.3.pitchOffset" "Step 4 pitch" { Semitones, -127, 127, 0 }
    float [steps[3].velocity_scale] "steps.3.velocityScale" "Step 4 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[4].gate] "steps.4.gate" "Step 5 gate" { true }
    int [steps[4].pitch_offset] "steps.4.pitchOffset" "Step 5 pitch" { Semitones, -127, 127, 0 }
    float [steps[4].velocity_scale] "steps.4.velocityScale" "Step 5 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[5].gate] "steps.5.gate" "Step 6 gate" { true }
    int [steps[5].pitch_offset] "steps.5.pitchOffset" "Step 6 pitch" { Semitones, -127, 127, 0 }
    float [steps[5].velocity_scale] "steps.5.velocityScale" "Step 6 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[6].gate] "steps.6.gate" "Step 7 gate" { true }
    int [steps[6].pitch_offset] "steps.6.pitchOffset" "Step 7 pitch" { Semitones, -127, 127, 0 }
    float [steps[6].velocity_scale] "steps.6.velocityScale" "Step 7 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[7].gate] "steps.7.gate" "Step 8 gate" { true }
    int [steps[7].pitch_offset] "steps.7.pitchOffset" "Step 8 pitch" { Semitones, -127, 127, 0 }
    float [steps[7].velocity_scale] "steps.7.velocityScale" "Step 8 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[8].gate] "steps.8.gate" "Step 9 gate" { true }
    int [steps[8].pitch_offset] "steps.8.pitchOffset" "Step 9 pitch" { Semitones, -127, 127, 0 }
    float [steps[8].velocity_scale] "steps.8.velocityScale" "Step 9 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[9].gate] "steps.9.gate" "Step 10 gate" { true }
    int [steps[9].pitch_offset] "steps.9.pitchOffset" "Step 10 pitch" { Semitones, -127, 127, 0 }
    float [steps[9].velocity_scale] "steps.9.velocityScale" "Step 10 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[10].gate] "steps.10.gate" "Step 11 gate" { true }
    int [steps[10].pitch_offset] "steps.10.pitchOffset" "Step 11 pitch" { Semitones, -127, 127, 0 }
    float [steps[10].velocity_scale] "steps.10.velocityScale" "Step 11 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[11].gate] "steps.11.gate" "Step 12 gate" { true }
    int [steps[11].pitch_offset] "steps.11.pitchOffset" "Step 12 pitch" { Semitones, -127, 127, 0 }
    float [steps[11].velocity_scale] "steps.11.velocityScale" "Step 12 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[12].gate] "steps.12.gate" "Step 13 gate" { true }
    int [steps[12].pitch_offset] "steps.12.pitchOffset" "Step 13 pitch" { Semitones, -127, 127, 0 }
    float [steps[12].velocity_scale] "steps.12.velocityScale" "Step 13 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[13].gate] "steps.13.gate" "Step 14 gate" { true }
    int [steps[13].pitch_offset] "steps.13.pitchOffset" "Step 14 pitch" { Semitones, -127, 127, 0 }
    float [steps[13].velocity_scale] "steps.13.velocityScale" "Step 14 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[14].gate] "steps.14.gate" "Step 15 gate" { true }
    int [steps[14].pitch_offset] "steps.14.pitchOffset" "Step 15 pitch" { Semitones, -127, 127, 0 }
    float [steps[14].velocity_scale] "steps.14.velocityScale" "Step 15 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
    toggle [steps[15].gate] "steps.15.gate" "Step 16 gate" { true }
    int [steps[15].pitch_offset] "steps.15.pitchOffset" "Step 16 pitch" { Semitones, -127, 127, 0 }
    float [steps[15].velocity_scale] "steps.15.velocityScale" "Step 16 velocity" { Gain, Linear, 0.0, 2.0, 1.0 }
});

/// A partial step transform: it never generates or schedules extra notes.
/// Advance the clock up to an event's sample before transforming that event.
#[derive(Debug, Clone, Copy)]
pub struct StepGrid {
    params: StepGridParams,
    tempo: f32,
    phase: f64,
}
impl Default for StepGrid {
    fn default() -> Self {
        Self {
            params: StepGridParams::default(),
            tempo: 120.0,
            phase: 0.0,
        }
    }
}
impl StepGrid {
    /// BPM 1..1000; non-finite values restore 120 BPM. Retains step phase.
    pub fn set_tempo(&mut self, bpm: f32) {
        self.tempo = if bpm.is_finite() {
            bpm.clamp(1.0, 1000.0)
        } else {
            120.0
        };
    }
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
    pub fn current_step(&self) -> usize {
        self.phase.floor() as usize
    }
    /// Advances by samples at a finite positive sample rate; invalid rates do nothing.
    /// Wraps every 16 steps, and retains fractional phase between calls.
    pub fn advance_samples(&mut self, samples: u64, sample_rate: f32) {
        if sample_rate.is_finite() && sample_rate > 0.0 {
            let steps =
                samples as f64 * f64::from(self.tempo) * f64::from(self.params.steps_per_beat)
                    / (60.0 * f64::from(sample_rate));
            self.phase = (self.phase + steps.rem_euclid(16.0)).rem_euclid(16.0);
        }
    }
}
impl NoteTransform for StepGrid {
    type Params = StepGridParams;
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
    }
    fn map(&self, note: MappedNote) -> Option<MappedNote> {
        let step = self.params.steps[self.current_step()];
        if !step.gate {
            return None;
        }
        let mut note = note.sanitized();
        note.key = (i16::from(note.key) + step.pitch_offset).clamp(0, 127) as u8;
        note.velocity = velocity(note.velocity * step.velocity_scale);
        Some(note)
    }
}
