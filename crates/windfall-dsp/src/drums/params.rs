use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::math::clean;
use crate::param::{
    approach_value, param_set, ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit,
};

/// Controls for a tunable two-mode drum.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MembraneParams {
    /// Fundamental at MIDI 60, Hz, 30..2000, default 160.
    pub pitch_hz: f32,
    /// Time from strike to silence, ms, 10..4000, default 600.
    pub decay_ms: f32,
    /// Second membrane mode and noise brightness, 0..1, default 0.5.
    pub tone: f32,
    /// Short noisy strike level, 0..1, default 0.2.
    pub snap: f32,
    /// Initial upward detuning, semitones, 0..24, default 7.
    pub pitch_drop_semitones: f32,
    /// Pitch sweep time constant, ms, 1..500, default 65.
    pub pitch_decay_ms: f32,
    /// Linear output gain, 0..1, default 0.8.
    pub level: f32,
    /// Radial strike position: center 0, near rim 0.95.
    pub strike_position: f32,
    /// Frequency-dependent loss of the membrane modes.
    pub damping: f32,
    /// Coupled low-frequency cavity resonance.
    pub body: f32,
}

impl Default for MembraneParams {
    fn default() -> Self {
        Self {
            pitch_hz: 160.0,
            decay_ms: 600.0,
            tone: 0.5,
            snap: 0.2,
            pitch_drop_semitones: 7.0,
            pitch_decay_ms: 65.0,
            level: 0.8,
            strike_position: 0.25,
            damping: 0.35,
            body: 0.4,
        }
    }
}

param_set!(MembraneParams, "Membrane", {
    float [pitch_hz] "pitchHz" "Pitch" { Hertz, Logarithmic, 30.0, 2000.0, 160.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Logarithmic, 10.0, 4000.0, 600.0 }
    float [tone] "tone" "Tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [snap] "snap" "Snap" { Fraction, Linear, 0.0, 1.0, 0.2 }
    float [pitch_drop_semitones] "pitchDropSemitones" "Pitch Drop" { Semitones, Linear, 0.0, 24.0, 7.0 }
    float [pitch_decay_ms] "pitchDecayMs" "Pitch Decay" { Milliseconds, Logarithmic, 1.0, 500.0, 65.0 }
    float [level] "level" "Level" { Gain, Linear, 0.0, 1.0, 0.8 }
    float [strike_position] "strikePosition" "Strike Position" { Fraction, Linear, 0.0, 0.95, 0.25 }
    float [damping] "damping" "Damping" { Fraction, Linear, 0.0, 1.0, 0.35 }
    float [body] "body" "Body" { Fraction, Linear, 0.0, 1.0, 0.4 }
});

/// Controls for the fast swept kick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct KickParams {
    /// Settled body pitch at MIDI 36, Hz, 20..250, default 52.
    pub pitch_hz: f32,
    /// Body duration, ms, 10..1500, default 420.
    pub decay_ms: f32,
    /// Initial pitch above the body, semitones, 0..60, default 36.
    pub pitch_drop_semitones: f32,
    /// Pitch sweep time constant, ms, 1..150, default 18.
    pub pitch_decay_ms: f32,
    /// Filtered 2 ms click level, 0..1, default 0.25.
    pub click: f32,
    /// Linear output gain, 0..1, default 0.8.
    pub level: f32,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            pitch_hz: 52.0,
            decay_ms: 420.0,
            pitch_drop_semitones: 36.0,
            pitch_decay_ms: 18.0,
            click: 0.25,
            level: 0.8,
        }
    }
}

param_set!(KickParams, "Kick", {
    float [pitch_hz] "pitchHz" "Body Pitch" { Hertz, Logarithmic, 20.0, 250.0, 52.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Logarithmic, 10.0, 1500.0, 420.0 }
    float [pitch_drop_semitones] "pitchDropSemitones" "Pitch Drop" { Semitones, Linear, 0.0, 60.0, 36.0 }
    float [pitch_decay_ms] "pitchDecayMs" "Pitch Decay" { Milliseconds, Logarithmic, 1.0, 150.0, 18.0 }
    float [click] "click" "Click" { Fraction, Linear, 0.0, 1.0, 0.25 }
    float [level] "level" "Level" { Gain, Linear, 0.0, 1.0, 0.8 }
});

/// A synthesis recipe, latched separately for each new hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DrumMode {
    #[default]
    Kick,
    Snare,
    Hat,
    Tom,
}

/// Controls shared by four distinct recipes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DrumVoiceParams {
    pub mode: DrumMode,
    /// Tuning at MIDI 36, Hz, 30..2000, default 120. Kick body is half this.
    pub pitch_hz: f32,
    /// Base duration, ms, 10..3000, default 500. Hat uses 25%, snare 60%.
    pub decay_ms: f32,
    /// Brightness and modal balance, 0..1, default 0.5.
    pub tone: f32,
    /// Strike/wire/metal level according to recipe, 0..1, default 0.5.
    pub snap: f32,
    /// Linear output gain, 0..1, default 0.8.
    pub level: f32,
}

impl Default for DrumVoiceParams {
    fn default() -> Self {
        Self {
            mode: DrumMode::Kick,
            pitch_hz: 120.0,
            decay_ms: 500.0,
            tone: 0.5,
            snap: 0.5,
            level: 0.8,
        }
    }
}

param_set!(DrumVoiceParams, "Drum Voice", {
    choice [mode] "mode" "Mode" { DrumMode, Kick, [Kick "kick" "Kick", Snare "snare" "Snare", Hat "hat" "Hat", Tom "tom" "Tom"] }
    float [pitch_hz] "pitchHz" "Pitch" { Hertz, Logarithmic, 30.0, 2000.0, 120.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Logarithmic, 10.0, 3000.0, 500.0 }
    float [tone] "tone" "Tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [snap] "snap" "Snap" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [level] "level" "Level" { Gain, Linear, 0.0, 1.0, 0.8 }
});

/// One synthesized pad's independently editable patch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DrumPadParams {
    /// Fundamental, Hz, 30..2000, default 120. Pads do not track MIDI pitch.
    pub pitch_hz: f32,
    /// Duration, ms, 10..4000, default 300.
    pub decay_ms: f32,
    /// Modal balance and noise brightness, 0..1, default 0.5.
    pub tone: f32,
    /// Share of noise versus membrane modes, 0..1, default 0.1.
    pub noise_mix: f32,
    pub strike_position: f32,
    pub damping: f32,
    pub body: f32,
}

impl Default for DrumPadParams {
    fn default() -> Self {
        Self {
            pitch_hz: 120.0,
            decay_ms: 300.0,
            tone: 0.5,
            noise_mix: 0.1,
            strike_position: 0.25,
            damping: 0.35,
            body: 0.4,
        }
    }
}

param_set!(DrumPadParams, "Drum Pad", {
    float [pitch_hz] "pitchHz" "Pitch" { Hertz, Logarithmic, 30.0, 2000.0, 120.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Logarithmic, 10.0, 4000.0, 300.0 }
    float [tone] "tone" "Tone" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [noise_mix] "noiseMix" "Noise / Tone" { Fraction, Linear, 0.0, 1.0, 0.1 }
    float [strike_position] "strikePosition" "Strike Position" { Fraction, Linear, 0.0, 0.95, 0.25 }
    float [damping] "damping" "Damping" { Fraction, Linear, 0.0, 1.0, 0.35 }
    float [body] "body" "Body" { Fraction, Linear, 0.0, 1.0, 0.4 }
});

/// Sixteen patches and a master level. Keys 36..51 address `pads[0..16]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DrumRackParams {
    pub pads: [DrumPadParams; 16],
    /// Linear master gain, 0..1, default 0.8.
    pub level: f32,
}

const fn rack_pad(index: usize) -> DrumPadParams {
    DrumPadParams {
        pitch_hz: 70.0 + index as f32 * 27.0,
        decay_ms: 160.0 + index as f32 * 20.0,
        tone: 0.2 + index as f32 * 0.04,
        noise_mix: (index % 4) as f32 * 0.2,
        strike_position: 0.25,
        damping: 0.35,
        body: 0.4,
    }
}

impl Default for DrumRackParams {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(rack_pad),
            level: 0.8,
        }
    }
}

// Literal paths are static so neither descriptor lookup nor automation allocates.
const PAD_IDS: [[&str; 4]; 16] = [
    [
        "pads.0.pitchHz",
        "pads.0.decayMs",
        "pads.0.tone",
        "pads.0.noiseMix",
    ],
    [
        "pads.1.pitchHz",
        "pads.1.decayMs",
        "pads.1.tone",
        "pads.1.noiseMix",
    ],
    [
        "pads.2.pitchHz",
        "pads.2.decayMs",
        "pads.2.tone",
        "pads.2.noiseMix",
    ],
    [
        "pads.3.pitchHz",
        "pads.3.decayMs",
        "pads.3.tone",
        "pads.3.noiseMix",
    ],
    [
        "pads.4.pitchHz",
        "pads.4.decayMs",
        "pads.4.tone",
        "pads.4.noiseMix",
    ],
    [
        "pads.5.pitchHz",
        "pads.5.decayMs",
        "pads.5.tone",
        "pads.5.noiseMix",
    ],
    [
        "pads.6.pitchHz",
        "pads.6.decayMs",
        "pads.6.tone",
        "pads.6.noiseMix",
    ],
    [
        "pads.7.pitchHz",
        "pads.7.decayMs",
        "pads.7.tone",
        "pads.7.noiseMix",
    ],
    [
        "pads.8.pitchHz",
        "pads.8.decayMs",
        "pads.8.tone",
        "pads.8.noiseMix",
    ],
    [
        "pads.9.pitchHz",
        "pads.9.decayMs",
        "pads.9.tone",
        "pads.9.noiseMix",
    ],
    [
        "pads.10.pitchHz",
        "pads.10.decayMs",
        "pads.10.tone",
        "pads.10.noiseMix",
    ],
    [
        "pads.11.pitchHz",
        "pads.11.decayMs",
        "pads.11.tone",
        "pads.11.noiseMix",
    ],
    [
        "pads.12.pitchHz",
        "pads.12.decayMs",
        "pads.12.tone",
        "pads.12.noiseMix",
    ],
    [
        "pads.13.pitchHz",
        "pads.13.decayMs",
        "pads.13.tone",
        "pads.13.noiseMix",
    ],
    [
        "pads.14.pitchHz",
        "pads.14.decayMs",
        "pads.14.tone",
        "pads.14.noiseMix",
    ],
    [
        "pads.15.pitchHz",
        "pads.15.decayMs",
        "pads.15.tone",
        "pads.15.noiseMix",
    ],
];

const MODEL_IDS: [[&str; 3]; 16] = [
    ["pads.0.strikePosition", "pads.0.damping", "pads.0.body"],
    ["pads.1.strikePosition", "pads.1.damping", "pads.1.body"],
    ["pads.2.strikePosition", "pads.2.damping", "pads.2.body"],
    ["pads.3.strikePosition", "pads.3.damping", "pads.3.body"],
    ["pads.4.strikePosition", "pads.4.damping", "pads.4.body"],
    ["pads.5.strikePosition", "pads.5.damping", "pads.5.body"],
    ["pads.6.strikePosition", "pads.6.damping", "pads.6.body"],
    ["pads.7.strikePosition", "pads.7.damping", "pads.7.body"],
    ["pads.8.strikePosition", "pads.8.damping", "pads.8.body"],
    ["pads.9.strikePosition", "pads.9.damping", "pads.9.body"],
    ["pads.10.strikePosition", "pads.10.damping", "pads.10.body"],
    ["pads.11.strikePosition", "pads.11.damping", "pads.11.body"],
    ["pads.12.strikePosition", "pads.12.damping", "pads.12.body"],
    ["pads.13.strikePosition", "pads.13.damping", "pads.13.body"],
    ["pads.14.strikePosition", "pads.14.damping", "pads.14.body"],
    ["pads.15.strikePosition", "pads.15.damping", "pads.15.body"],
];

const LEVEL_INFO: ParamInfo = ParamInfo {
    id: "level",
    name: "Level",
    kind: ParamKind::Float,
    unit: ParamUnit::Gain,
    scale: ParamScale::Linear,
    min: 0.0,
    max: 1.0,
    default: 0.8,
    choices: &[],
};

const fn rack_descriptors() -> [ParamInfo; 113] {
    let mut result = [LEVEL_INFO; 113];
    let mut pad = 0;
    while pad < 16 {
        let patch = rack_pad(pad);
        let values = [patch.pitch_hz, patch.decay_ms, patch.tone, patch.noise_mix];
        let mut field = 0;
        while field < 4 {
            result[pad * 4 + field] = ParamInfo {
                id: PAD_IDS[pad][field],
                name: ["Pitch", "Decay", "Tone", "Noise / Tone"][field],
                kind: ParamKind::Float,
                unit: [
                    ParamUnit::Hertz,
                    ParamUnit::Milliseconds,
                    ParamUnit::Fraction,
                    ParamUnit::Fraction,
                ][field],
                scale: if field < 2 {
                    ParamScale::Logarithmic
                } else {
                    ParamScale::Linear
                },
                min: [30.0, 10.0, 0.0, 0.0][field],
                max: [2000.0, 4000.0, 1.0, 1.0][field],
                default: values[field],
                choices: &[],
            };
            field += 1;
        }
        let mut model = 0;
        while model < 3 {
            result[65 + pad * 3 + model] = ParamInfo {
                id: MODEL_IDS[pad][model],
                name: ["Strike Position", "Damping", "Body"][model],
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: if model == 0 { 0.95 } else { 1.0 },
                default: [0.25, 0.35, 0.4][model],
                choices: &[],
            };
            model += 1;
        }
        pad += 1;
    }
    result
}

impl ParamSet for DrumRackParams {
    const NAME: &'static str = "Drum Rack";
    fn descriptors() -> &'static [ParamInfo] {
        static INFO: [ParamInfo; 113] = rack_descriptors();
        &INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        if index < 64 {
            self.pads[index / 4].get(index % 4)
        } else if index == 64 {
            Some(self.level)
        } else if index < 113 {
            self.pads[(index - 65) / 3].get(4 + (index - 65) % 3)
        } else {
            None
        }
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        if index < 64 {
            // Rack defaults differ from the standalone pad defaults.
            let info = &Self::descriptors()[index];
            self.pads[index / 4].set(index % 4, clean(value, info.min, info.max, info.default))
        } else if index == 64 {
            self.level = clean(value, 0.0, 1.0, 0.8);
            true
        } else if index < 113 {
            self.pads[(index - 65) / 3].set(4 + (index - 65) % 3, value)
        } else {
            false
        }
    }
    fn sanitized(&self) -> Self {
        let mut result = *self;
        for index in 0..113 {
            result.set(index, self.get(index).unwrap_or(0.0));
        }
        result
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let mut moving = false;
        for (pad, target) in self.pads.iter_mut().zip(&target.pads) {
            moving |= pad.approach(target, amount);
        }
        let (level, unsettled) = approach_value(self.level, target.level, amount, 1.0);
        self.level = level;
        moving | unsettled
    }
}
