//! Saved, bounded note modules. Occurrence ownership belongs to the rack host.
use crate::notemap::{
    ColorMap, ColorMapParams, KeyMap, KeyMapParams, KeySplit, KeySplitParams, LevelScale,
    LevelScaleParams, MappedNote, NoteTransform, StepGridParams,
};
use crate::{
    NoteExpression, ParamInfo,
    param::{ParamSet, param_set},
};
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
param_set!(NoteEnvelope, "Note envelope", {
    choice [destination] "destination" "Destination" { EnvelopeDestination, Gain, [
        Gain "gain" "Gain", Pan "pan" "Pan", Pitch "pitch" "Pitch",
        ModulationX "modulationX" "Modulation X", ModulationY "modulationY" "Modulation Y"
    ] }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Linear, 0.0, 10000.0, 10.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Linear, 0.0, 10000.0, 100.0 }
    float [sustain] "sustain" "Sustain" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Linear, 0.0, 10000.0, 100.0 }
    float [amount] "amount" "Amount" { Fraction, Linear, -1.0, 1.0, 1.0 }
});
param_set!(PropertyScale, "Property scaler", {
    choice [property] "property" "Property" { NoteProperty, Velocity, [
        Velocity "velocity" "Velocity", Pan "pan" "Pan", Release "release" "Release",
        Pitch "pitch" "Pitch", ModulationX "modulationX" "Modulation X", ModulationY "modulationY" "Modulation Y"
    ] }
    float [curve] "curve" "Curve" { None, Linear, 0.125, 8.0, 1.0 }
    float [levels.percent] "levels.percent" "Level" { None, Linear, 0.0, 200.0, 100.0 }
    float [levels.floor] "levels.floor" "Floor" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [levels.ceiling] "levels.ceiling" "Ceiling" { Fraction, Linear, 0.0, 1.0, 1.0 }
});

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SequenceMode {
    #[default]
    Sequence,
    Up,
    Down,
    AsPlayed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteSequence {
    pub mode: SequenceMode,
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
            mode: SequenceMode::Sequence,
            grid: Default::default(),
            length: 16,
            gate: 0.5,
            octaves: 1,
        }
    }
}
param_set!(NoteSequence, "Note sequencer", {
    choice [mode] "mode" "Mode" { SequenceMode, Sequence, [
        Sequence "sequence" "Sequence", Up "up" "Up", Down "down" "Down", AsPlayed "asPlayed" "As played"
    ] }
    int [length] "length" "Length" { None, 1, 16, 16 }
    float [gate] "gate" "Gate" { Fraction, Linear, 0.01, 1.0, 0.5 }
    int [octaves] "octaves" "Octaves" { Octaves, 1, 4, 1 }
});

// Extra wrapper controls follow the existing foundation descriptors. These
// indices are persisted; extend each list only at its end.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct SplitControls {
    preserve_key: bool,
}
param_set!(SplitControls, "Split controls", {
    toggle [preserve_key] "preserveKey" "Preserve incoming key" { true }
});
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChordControls {
    voices: u8,
    chord: [i8; 4],
}
impl Default for ChordControls {
    fn default() -> Self {
        Self {
            voices: 1,
            chord: [0; 4],
        }
    }
}
param_set!(ChordControls, "Chord controls", {
    int [voices] "voices" "Chord voices" { None, 1, 4, 1 }
    int [chord[0]] "chord.0" "Chord 1" { Semitones, -127, 127, 0 }
    int [chord[1]] "chord.1" "Chord 2" { Semitones, -127, 127, 0 }
    int [chord[2]] "chord.2" "Chord 3" { Semitones, -127, 127, 0 }
    int [chord[3]] "chord.3" "Chord 4" { Semitones, -127, 127, 0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
        #[serde(rename = "preserveKey")]
        #[ts(rename = "preserveKey")]
        preserve_key: bool,
    },
    KeyMapper {
        params: KeyMapParams,
        #[ts(as = "Vec<i8>")]
        chord: [i8; 4],
        voices: u8,
    },
    Sequencer {
        params: NoteSequence,
    },
}
impl RackNoteModule {
    pub fn kind_id(&self) -> &'static str {
        match self {
            Self::ColorRouter { .. } => "colorRouter",
            Self::Envelope { .. } => "envelope",
            Self::PropertyScaler { .. } => "propertyScaler",
            Self::KeySplitter { .. } => "keySplitter",
            Self::KeyMapper { .. } => "keyMapper",
            Self::Sequencer { .. } => "sequencer",
        }
    }
    pub fn parameter_info(&self, index: usize) -> Option<ParamInfo> {
        let (first, extra) = match self {
            Self::ColorRouter { .. } => (ColorMapParams::descriptors(), &[][..]),
            Self::Envelope { .. } => (NoteEnvelope::descriptors(), &[][..]),
            Self::PropertyScaler { .. } => (PropertyScale::descriptors(), &[][..]),
            Self::KeySplitter { .. } => {
                (KeySplitParams::descriptors(), SplitControls::descriptors())
            }
            Self::KeyMapper { .. } => (KeyMapParams::descriptors(), ChordControls::descriptors()),
            Self::Sequencer { .. } => (NoteSequence::descriptors(), StepGridParams::descriptors()),
        };
        first
            .get(index)
            .or_else(|| index.checked_sub(first.len()).and_then(|i| extra.get(i)))
            .copied()
    }
    pub fn parameter(&self, index: usize) -> Option<f32> {
        match self {
            Self::ColorRouter { params } => params.get(index),
            Self::Envelope { params } => params.get(index),
            Self::PropertyScaler { params } => params.get(index),
            Self::KeySplitter {
                params,
                preserve_key,
            } => params.get(index).or_else(|| {
                index
                    .checked_sub(KeySplitParams::descriptors().len())
                    .and_then(|i| {
                        SplitControls {
                            preserve_key: *preserve_key,
                        }
                        .get(i)
                    })
            }),
            Self::KeyMapper {
                params,
                chord,
                voices,
            } => params.get(index).or_else(|| {
                index
                    .checked_sub(KeyMapParams::descriptors().len())
                    .and_then(|i| {
                        ChordControls {
                            chord: *chord,
                            voices: *voices,
                        }
                        .get(i)
                    })
            }),
            Self::Sequencer { params } => params.get(index).or_else(|| {
                index
                    .checked_sub(NoteSequence::descriptors().len())
                    .and_then(|i| params.grid.get(i))
            }),
        }
    }
    /// Copy-only atomic parameter changes, safe on the automation grid.
    pub fn set_parameter(&mut self, index: usize, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let mut next = *self;
        let found = match &mut next {
            Self::ColorRouter { params } => params.set(index, value),
            Self::Envelope { params } => params.set(index, value),
            Self::PropertyScaler { params } => params.set(index, value),
            Self::KeySplitter {
                params,
                preserve_key,
            } => {
                if index < KeySplitParams::descriptors().len() {
                    params.set(index, value)
                } else {
                    let mut extra = SplitControls {
                        preserve_key: *preserve_key,
                    };
                    let found = extra.set(index - KeySplitParams::descriptors().len(), value);
                    *preserve_key = extra.preserve_key;
                    found
                }
            }
            Self::KeyMapper {
                params,
                chord,
                voices,
            } => {
                if index < KeyMapParams::descriptors().len() {
                    params.set(index, value)
                } else {
                    let mut extra = ChordControls {
                        chord: *chord,
                        voices: *voices,
                    };
                    let found = extra.set(index - KeyMapParams::descriptors().len(), value);
                    *chord = extra.chord;
                    *voices = extra.voices;
                    found
                }
            }
            Self::Sequencer { params } => {
                if index < NoteSequence::descriptors().len() {
                    params.set(index, value)
                } else {
                    params
                        .grid
                        .set(index - NoteSequence::descriptors().len(), value)
                }
            }
        };
        if !found || !next.valid() {
            return false;
        }
        *self = next;
        true
    }
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
            Self::Envelope { params } => {
                let Some(slot) = note.envelopes.iter_mut().find(|slot| slot.is_none()) else {
                    return 0;
                };
                *slot = Some(*params);
            }
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
    /// Per-instance expression-map gain, preserved by note transforms.
    pub gain: f32,
    pub pan: f32,
    pub expression: NoteExpression,
    pub channel: u8,
    pub envelopes: [Option<NoteEnvelope>; MAX_NOTE_ENVELOPES],
}
pub const MAX_NOTE_ENVELOPES: usize = 8;
impl Default for RackNote {
    fn default() -> Self {
        Self {
            key: 60,
            velocity: 1.0,
            gain: 1.0,
            pan: 0.0,
            expression: NoteExpression::NEUTRAL,
            channel: 0,
            envelopes: [None; MAX_NOTE_ENVELOPES],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_note_module_parameter_has_stable_measured_range_and_atomic_nonfinite_refusal() {
        let modules = [
            (
                RackNoteModule::ColorRouter {
                    params: Default::default(),
                },
                16,
            ),
            (
                RackNoteModule::Envelope {
                    params: Default::default(),
                },
                6,
            ),
            (
                RackNoteModule::PropertyScaler {
                    params: Default::default(),
                },
                5,
            ),
            (
                RackNoteModule::KeySplitter {
                    params: Default::default(),
                    preserve_key: true,
                },
                8,
            ),
            (
                RackNoteModule::KeyMapper {
                    params: Default::default(),
                    chord: [0; 4],
                    voices: 1,
                },
                133,
            ),
            (
                RackNoteModule::Sequencer {
                    params: Default::default(),
                },
                53,
            ),
        ];
        for (module, count) in modules {
            for index in 0..count {
                let info = module.parameter_info(index).unwrap();
                for value in [info.min, info.max] {
                    let mut next = module;
                    assert!(
                        next.set_parameter(index, value),
                        "{} {index}",
                        module.kind_id()
                    );
                    assert_eq!(next.parameter(index), Some(value));
                    assert!(next.valid());
                }
                let mut next = module;
                for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                    assert!(!next.set_parameter(index, invalid));
                    assert_eq!(next, module);
                }
            }
            assert!(module.parameter_info(count).is_none());
            assert!(module.parameter(count).is_none());
        }
    }

    #[test]
    fn automated_controls_change_routes_keys_properties_and_envelope_shape() {
        let mut color = RackNoteModule::ColorRouter {
            params: Default::default(),
        };
        assert!(color.set_parameter(0, 9.0));
        assert_eq!(mapped(color, RackNote::default())[0].channel, 9);
        let mut splitter = RackNoteModule::KeySplitter {
            params: Default::default(),
            preserve_key: true,
        };
        assert!(splitter.set_parameter(6, 8.0));
        assert!(splitter.set_parameter(7, 0.0));
        assert_eq!(
            (
                mapped(splitter, RackNote::default())[0].channel,
                mapped(splitter, RackNote::default())[0].key
            ),
            (8, 72)
        );
        let mut mapper = RackNoteModule::KeyMapper {
            params: Default::default(),
            chord: [0; 4],
            voices: 1,
        };
        assert!(mapper.set_parameter(60, 65.0));
        assert!(mapper.set_parameter(128, 2.0));
        assert!(mapper.set_parameter(130, 7.0));
        assert_eq!(
            mapped(mapper, RackNote::default())
                .iter()
                .map(|n| n.key)
                .collect::<Vec<_>>(),
            [65, 72]
        );
        let mut scaler = RackNoteModule::PropertyScaler {
            params: Default::default(),
        };
        assert!(scaler.set_parameter(2, 50.0));
        assert_eq!(mapped(scaler, RackNote::default())[0].velocity, 0.5);
        let mut envelope = RackNoteModule::Envelope {
            params: Default::default(),
        };
        assert!(envelope.set_parameter(1, 100.0));
        assert_eq!(
            mapped(envelope, RackNote::default())[0].envelopes[0]
                .unwrap()
                .level(0.05),
            0.5
        );
    }

    #[test]
    fn serial_envelope_modules_preserve_independent_shapes_and_refuse_ninth() {
        let a = NoteEnvelope {
            attack_ms: 100.0,
            ..Default::default()
        };
        let b = NoteEnvelope {
            destination: EnvelopeDestination::Pitch,
            attack_ms: 200.0,
            ..Default::default()
        };
        let first = mapped(RackNoteModule::Envelope { params: a }, RackNote::default())[0];
        let second = mapped(RackNoteModule::Envelope { params: b }, first)[0];
        assert_eq!(second.envelopes[0], Some(a));
        assert_eq!(second.envelopes[1], Some(b));
        assert_eq!(second.envelopes[0].unwrap().level(0.05), 0.5);
        assert_eq!(second.envelopes[1].unwrap().level(0.05), 0.25);
        let full = RackNote {
            envelopes: [Some(a); MAX_NOTE_ENVELOPES],
            ..Default::default()
        };
        assert!(mapped(RackNoteModule::Envelope { params: b }, full).is_empty());
    }

    fn mapped(module: RackNoteModule, note: RackNote) -> Vec<RackNote> {
        assert!(module.valid());
        let mut out = [RackNote::default(); 4];
        let count = module.map(note, &mut out);
        out[..count].to_vec()
    }

    #[test]
    fn color_mapping_routes_channels_and_keeps_source_color_and_expression_gain() {
        let mut params = ColorMapParams::default();
        params.table[3] = 9;
        let note = RackNote {
            gain: 0.4,
            expression: NoteExpression {
                color_group: Some(3),
                ..NoteExpression::NEUTRAL
            },
            ..Default::default()
        };
        let out = mapped(RackNoteModule::ColorRouter { params }, note);
        assert_eq!(out[0].channel, 9);
        assert_eq!(out[0].expression.color_group, Some(3));
        assert_eq!(out[0].gain, 0.4);
    }

    #[test]
    fn splitter_boundaries_drop_outside_zones_and_optionally_preserve_pitch() {
        let params = KeySplitParams {
            low_min: 24,
            boundary: 60,
            high_max: 96,
            ..Default::default()
        };
        for key in [23, 97] {
            assert!(
                mapped(
                    RackNoteModule::KeySplitter {
                        params,
                        preserve_key: true
                    },
                    RackNote {
                        key,
                        ..Default::default()
                    }
                )
                .is_empty()
            );
        }
        for (key, channel) in [(24, 0), (59, 0), (60, 1), (96, 1)] {
            let out = mapped(
                RackNoteModule::KeySplitter {
                    params,
                    preserve_key: true,
                },
                RackNote {
                    key,
                    ..Default::default()
                },
            );
            assert_eq!((out[0].key, out[0].channel), (key, channel));
        }
        assert_eq!(
            mapped(
                RackNoteModule::KeySplitter {
                    params,
                    preserve_key: false
                },
                RackNote::default()
            )[0]
            .key,
            72
        );
    }

    #[test]
    fn key_mapping_expands_a_bounded_chord_and_clamps_at_keyboard_limits() {
        let mut params = KeyMapParams::default();
        params.table[60] = 126;
        let out = mapped(
            RackNoteModule::KeyMapper {
                params,
                chord: [-12, 0, 4, 7],
                voices: 4,
            },
            RackNote::default(),
        );
        assert_eq!(
            out.iter().map(|note| note.key).collect::<Vec<_>>(),
            [114, 126, 127, 127]
        );
        assert!(
            !RackNoteModule::KeyMapper {
                params,
                chord: [0; 4],
                voices: 5
            }
            .valid()
        );
    }

    #[test]
    fn scaler_applies_the_power_curve_to_each_property_in_its_normalized_space() {
        for property in [
            NoteProperty::Velocity,
            NoteProperty::Pan,
            NoteProperty::Release,
            NoteProperty::Pitch,
            NoteProperty::ModulationX,
            NoteProperty::ModulationY,
        ] {
            let note = RackNote {
                velocity: 0.5,
                pan: 0.0,
                expression: NoteExpression {
                    release: 0.5,
                    modulation_x: 0.5,
                    modulation_y: 0.5,
                    fine_pitch_cents: 0.0,
                    ..NoteExpression::NEUTRAL
                },
                ..Default::default()
            };
            let out = mapped(
                RackNoteModule::PropertyScaler {
                    params: PropertyScale {
                        property,
                        curve: 2.0,
                        ..Default::default()
                    },
                },
                note,
            )[0];
            let result = match property {
                NoteProperty::Velocity => out.velocity,
                NoteProperty::Pan => (out.pan + 1.0) / 2.0,
                NoteProperty::Release => out.expression.release,
                NoteProperty::Pitch => (out.expression.fine_pitch_cents + 1200.0) / 2400.0,
                NoteProperty::ModulationX => out.expression.modulation_x,
                NoteProperty::ModulationY => out.expression.modulation_y,
            };
            assert!((result - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn envelope_attaches_an_independent_per_note_shape_with_exact_linear_segments() {
        let params = NoteEnvelope {
            attack_ms: 100.0,
            decay_ms: 100.0,
            sustain: 0.25,
            ..Default::default()
        };
        assert_eq!(params.level(0.0), 0.0);
        assert!((params.level(0.05) - 0.5).abs() < 1e-6);
        assert!((params.level(0.15) - 0.625).abs() < 1e-6);
        assert_eq!(params.level(0.25), 0.25);
        assert_eq!(
            mapped(RackNoteModule::Envelope { params }, RackNote::default())[0].envelopes[0],
            Some(params)
        );
    }

    #[test]
    fn sequencer_modes_and_all_note_modules_round_trip_saved_state() {
        let modules = [
            RackNoteModule::ColorRouter {
                params: Default::default(),
            },
            RackNoteModule::Envelope {
                params: Default::default(),
            },
            RackNoteModule::PropertyScaler {
                params: Default::default(),
            },
            RackNoteModule::KeySplitter {
                params: Default::default(),
                preserve_key: true,
            },
            RackNoteModule::KeyMapper {
                params: Default::default(),
                chord: [0; 4],
                voices: 1,
            },
            RackNoteModule::Sequencer {
                params: NoteSequence {
                    mode: SequenceMode::AsPlayed,
                    ..Default::default()
                },
            },
        ];
        for module in modules {
            assert!(module.valid());
            let json = serde_json::to_string(&module).unwrap();
            assert_eq!(
                serde_json::from_str::<RackNoteModule>(&json).unwrap(),
                module
            );
        }
        let legacy: NoteSequence = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.mode, SequenceMode::Sequence);
    }
}
