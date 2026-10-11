//! Saved, bounded note modules. Occurrence ownership belongs to the rack host.
use crate::notemap::{
    ColorMap, ColorMapParams, KeyMap, KeyMapParams, KeySplit, KeySplitParams, LevelScale,
    LevelScaleParams, MappedNote, NoteTransform, StepGridParams,
};
use crate::{NoteExpression, param::ParamSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteProperty {
    Velocity,
    Pan,
    Release,
    Pitch,
    ModulationX,
    ModulationY,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PropertyScale {
    pub property: NoteProperty,
    pub levels: LevelScaleParams,
    /// Power curve in normalized property space, 0.125..8.
    pub curve: f32,
}
impl Default for PropertyScale {
    fn default() -> Self {
        Self {
            property: NoteProperty::Velocity,
            levels: Default::default(),
            curve: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EnvelopeDestination {
    Gain,
    Pan,
    Pitch,
    ModulationX,
    ModulationY,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteEnvelope {
    pub destination: EnvelopeDestination,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub amount: f32,
}
impl Default for NoteEnvelope {
    fn default() -> Self {
        Self {
            destination: EnvelopeDestination::Gain,
            attack_ms: 10.0,
            decay_ms: 100.0,
            sustain: 0.7,
            release_ms: 100.0,
            amount: 1.0,
        }
    }
}
impl NoteEnvelope {
    pub fn valid(self) -> bool {
        [self.attack_ms, self.decay_ms, self.release_ms]
            .iter()
            .all(|v| v.is_finite() && (0.0..=10_000.0).contains(v))
            && self.sustain.is_finite()
            && (0.0..=1.0).contains(&self.sustain)
            && self.amount.is_finite()
            && (-1.0..=1.0).contains(&self.amount)
    }
    pub fn level(self, seconds: f64) -> f32 {
        let attack = f64::from(self.attack_ms) / 1000.0;
        let decay = f64::from(self.decay_ms) / 1000.0;
        if attack > 0.0 && seconds < attack {
            (seconds / attack) as f32
        } else if decay > 0.0 && seconds < attack + decay {
            1.0 + (self.sustain - 1.0) * ((seconds - attack) / decay) as f32
        } else {
            self.sustain
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteSequence {
    pub grid: StepGridParams,
    /// Number of active steps, 1..16. Repeats while the input owner is held.
    pub length: u8,
    pub gate: f32,
    /// Each cycle moves up through this many octaves, 1..4.
    pub octaves: u8,
}
impl Default for NoteSequence {
    fn default() -> Self {
        Self {
            grid: Default::default(),
            length: 16,
            gate: 0.5,
            octaves: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum RackNoteModule {
    ColorRouter {
        params: ColorMapParams,
    },
    Envelope {
        params: NoteEnvelope,
    },
    PropertyScaler {
        params: PropertyScale,
    },
    KeySplitter {
        params: KeySplitParams,
        preserve_key: bool,
    },
    KeyMapper {
        params: KeyMapParams,
        chord: [i8; 4],
        voices: u8,
    },
    Sequencer {
        params: NoteSequence,
    },
}
impl RackNoteModule {
    pub fn valid(&self) -> bool {
        match self {
            Self::ColorRouter { params } => *params == params.sanitized(),
            Self::Envelope { params } => params.valid(),
            Self::PropertyScaler { params } => {
                params.levels == params.levels.sanitized()
                    && params.curve.is_finite()
                    && (0.125..=8.0).contains(&params.curve)
            }
            Self::KeySplitter { params, .. } => *params == params.sanitized(),
            Self::KeyMapper { params, voices, .. } => {
                *params == params.sanitized() && (1..=4).contains(voices)
            }
            Self::Sequencer { params } => {
                params.grid == params.grid.sanitized()
                    && (1..=16).contains(&params.length)
                    && (1..=4).contains(&params.octaves)
                    && params.gate.is_finite()
                    && (0.01..=1.0).contains(&params.gate)
            }
        }
    }
    /// At most four chord voices. Envelopes and scheduling are host-side.
    pub fn map(&self, input: RackNote, out: &mut [RackNote; 4]) -> usize {
        let mut note = input;
        match self {
            Self::ColorRouter { params } => {
                let mut map = ColorMap::default();
                map.set_params(params);
                // The foundation maps a color; this host interprets that
                // result as a routing channel, preserving the source color.
                note.channel = map.map(note.mapped()).unwrap().color;
            }
            Self::PropertyScaler { params } => {
                let value = match params.property {
                    NoteProperty::Velocity => note.velocity,
                    NoteProperty::Pan => (note.pan + 1.0) * 0.5,
                    NoteProperty::Release => note.expression.release,
                    NoteProperty::Pitch => (note.expression.fine_pitch_cents + 1200.0) / 2400.0,
                    NoteProperty::ModulationX => note.expression.modulation_x,
                    NoteProperty::ModulationY => note.expression.modulation_y,
                };
                let mut map = LevelScale::default();
                map.set_params(&params.levels);
                let value = map
                    .map(MappedNote {
                        velocity: value.clamp(0.0, 1.0).powf(params.curve),
                        ..note.mapped()
                    })
                    .unwrap()
                    .velocity;
                match params.property {
                    NoteProperty::Velocity => note.velocity = value,
                    NoteProperty::Pan => note.pan = value * 2.0 - 1.0,
                    NoteProperty::Release => note.expression.release = value,
                    NoteProperty::Pitch => {
                        note.expression.fine_pitch_cents = value * 2400.0 - 1200.0
                    }
                    NoteProperty::ModulationX => note.expression.modulation_x = value,
                    NoteProperty::ModulationY => note.expression.modulation_y = value,
                }
            }
            Self::KeySplitter {
                params,
                preserve_key,
            } => {
                let mut map = KeySplit::default();
                map.set_params(params);
                let Some(mapped) = map.map(note.mapped()) else {
                    return 0;
                };
                if !preserve_key {
                    note.key = mapped.key;
                }
                note.channel = mapped.channel;
            }
            Self::KeyMapper {
                params,
                chord,
                voices,
            } => {
                let mut map = KeyMap::default();
                map.set_params(params);
                note.key = map.map(note.mapped()).unwrap().key;
                for index in 0..usize::from(*voices).min(4) {
                    out[index] = RackNote {
                        key: (i16::from(note.key) + i16::from(chord[index])).clamp(0, 127) as u8,
                        ..note
                    };
                }
                return usize::from(*voices).min(4);
            }
            Self::Envelope { params } => note.envelope = Some(*params),
            Self::Sequencer { .. } => {}
        }
        out[0] = note;
        1
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RackNote {
    pub key: u8,
    pub velocity: f32,
    pub pan: f32,
    pub expression: NoteExpression,
    pub channel: u8,
    pub envelope: Option<NoteEnvelope>,
}
impl Default for RackNote {
    fn default() -> Self {
        Self {
            key: 60,
            velocity: 1.0,
            pan: 0.0,
            expression: NoteExpression::NEUTRAL,
            channel: 0,
            envelope: None,
        }
    }
}
impl RackNote {
    fn mapped(self) -> MappedNote {
        MappedNote {
            key: self.key,
            velocity: self.velocity,
            channel: self.channel,
            color: self.expression.color_group.unwrap_or(0),
        }
    }
}
