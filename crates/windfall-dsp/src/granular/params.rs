//! Compact scalar parameters. Legacy table decoding is control-side only.
use super::{MAX_SAMPLE_FRAMES, SLICE_COUNT};
use crate::param::{ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit, approach_value};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Mono source audio. Only `data[..len]` plays; empty tables are silent.
/// `sample_rate` is the source rate, independently of the output rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SampleTable {
    #[serde(with = "table_serde")]
    #[ts(type = "number[]")]
    pub data: [f32; MAX_SAMPLE_FRAMES],
    pub len: u16,
    pub sample_rate: f32,
}
impl Default for SampleTable {
    fn default() -> Self {
        Self {
            data: [0.0; MAX_SAMPLE_FRAMES],
            len: 0,
            sample_rate: 48_000.0,
        }
    }
}
impl SampleTable {
    /// Copies source frames; rejects oversized sources rather than truncating.
    pub fn from_slice(data: &[f32], sample_rate: f32) -> Result<Self, &'static str> {
        if data.len() > MAX_SAMPLE_FRAMES {
            return Err("sample table exceeds 4096 frames");
        }
        let mut table = Self::default();
        table.data[..data.len()].copy_from_slice(data);
        table.len = data.len() as u16;
        table.sample_rate = sample_rate;
        table.sanitize();
        Ok(table)
    }
    fn sanitize(&mut self) {
        self.len = self.len.min(MAX_SAMPLE_FRAMES as u16);
        self.sample_rate = clean(self.sample_rate, 1000.0, 384000.0, 48000.0);
        for sample in &mut self.data {
            *sample = clean(*sample, -1.0, 1.0, 0.0);
        }
    }
}
mod table_serde {
    use super::MAX_SAMPLE_FRAMES;
    use serde::{
        Deserializer, Serializer,
        de::{Error, SeqAccess, Visitor},
        ser::SerializeSeq,
    };
    pub fn serialize<S: Serializer>(
        data: &[f32; MAX_SAMPLE_FRAMES],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(MAX_SAMPLE_FRAMES))?;
        for value in data {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[f32; MAX_SAMPLE_FRAMES], D::Error> {
        struct TableVisitor;
        impl<'de> Visitor<'de> for TableVisitor {
            type Value = [f32; MAX_SAMPLE_FRAMES];
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("at most 4096 mono samples")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut data = [0.0; MAX_SAMPLE_FRAMES];
                let mut count = 0;
                while let Some(value) = seq.next_element::<f32>()? {
                    if count == MAX_SAMPLE_FRAMES {
                        return Err(A::Error::custom("sample table exceeds 4096 frames"));
                    }
                    data[count] = value;
                    count += 1;
                }
                Ok(data)
            }
        }
        deserializer.deserialize_seq(TableVisitor)
    }
}
/// Slice Deck's additional controls. Pitch is semitones, gain is linear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SliceSettings {
    pub pitch: f32,
    pub gain: f32,
    pub reverse: bool,
}
impl Default for SliceSettings {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            gain: 1.0,
            reverse: false,
        }
    }
}
pub(super) fn clean(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        default
    }
}
fn starts() -> [f32; SLICE_COUNT] {
    std::array::from_fn(|i| i as f32 / SLICE_COUNT as f32)
}

/// Slice Map parameters. Table frames are source data, not automatable controls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SliceMapParams {
    /// Compatibility activation bits for the original scalar automation indices.
    #[serde(default, skip_serializing_if = "no_legacy_edits")]
    #[ts(as = "Option<[u16; 4]>", optional)]
    pub legacy_edits: [u16; 4],
    pub base_key: u8,
    pub level: f32,
    pub release_ms: f32,
    /// Ordered normalized slice starts. Each end is the next start, or 1.
    pub slice_starts: [f32; SLICE_COUNT],
}
impl Default for SliceMapParams {
    fn default() -> Self {
        Self {
            legacy_edits: [0; 4],
            base_key: 60,
            level: 0.5,
            release_ms: 30.0,
            slice_starts: starts(),
        }
    }
}
impl ParamSet for SliceMapParams {
    const NAME: &'static str = "Slice Map";
    fn descriptors() -> &'static [ParamInfo] {
        static INFO: &[ParamInfo] = &[
            ParamInfo {
                id: "baseKey",
                name: "Base key",
                kind: ParamKind::Integer,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 127.0,
                default: 60.0,
                choices: &[],
            },
            ParamInfo {
                id: "level",
                name: "Level",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "releaseMs",
                name: "Release",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 1000.0,
                default: 30.0,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.0",
                name: "Slice 1 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.1",
                name: "Slice 2 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.2",
                name: "Slice 3 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.3",
                name: "Slice 4 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.1875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.4",
                name: "Slice 5 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.25,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.5",
                name: "Slice 6 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.3125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.6",
                name: "Slice 7 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.375,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.7",
                name: "Slice 8 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.4375,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.8",
                name: "Slice 9 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.9",
                name: "Slice 10 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.10",
                name: "Slice 11 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.11",
                name: "Slice 12 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.6875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.12",
                name: "Slice 13 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.75,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.13",
                name: "Slice 14 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.8125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.14",
                name: "Slice 15 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.15",
                name: "Slice 16 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.9375,
                choices: &[],
            },
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.base_key as f32,
            1 => self.level,
            2 => self.release_ms,
            3 => self.slice_starts[0],
            4 => self.slice_starts[1],
            5 => self.slice_starts[2],
            6 => self.slice_starts[3],
            7 => self.slice_starts[4],
            8 => self.slice_starts[5],
            9 => self.slice_starts[6],
            10 => self.slice_starts[7],
            11 => self.slice_starts[8],
            12 => self.slice_starts[9],
            13 => self.slice_starts[10],
            14 => self.slice_starts[11],
            15 => self.slice_starts[12],
            16 => self.slice_starts[13],
            17 => self.slice_starts[14],
            18 => self.slice_starts[15],
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.base_key = value.round() as _,
            1 => self.level = value,
            2 => self.release_ms = value,
            3 => self.slice_starts[0] = value,
            4 => self.slice_starts[1] = value,
            5 => self.slice_starts[2] = value,
            6 => self.slice_starts[3] = value,
            7 => self.slice_starts[4] = value,
            8 => self.slice_starts[5] = value,
            9 => self.slice_starts[6] = value,
            10 => self.slice_starts[7] = value,
            11 => self.slice_starts[8] = value,
            12 => self.slice_starts[9] = value,
            13 => self.slice_starts[10] = value,
            14 => self.slice_starts[11] = value,
            15 => self.slice_starts[12] = value,
            16 => self.slice_starts[13] = value,
            17 => self.slice_starts[14] = value,
            18 => self.slice_starts[15] = value,
            _ => return false,
        }
        if index >= 3 {
            let bit = index - 3;
            self.legacy_edits[bit / 16] |= 1 << (bit % 16);
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut clean = *self;
        for index in 0..Self::descriptors().len() {
            clean.set(index, self.get(index).unwrap());
        }
        clean.legacy_edits = self.legacy_edits;
        clean
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let amount = clean(amount, 0.0, 1.0, 1.0);
        let mut moving = false;
        for (index, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(index).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) = approach_value(
                    self.get(index).unwrap(),
                    target,
                    amount,
                    info.max - info.min,
                );
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(index, value);
        }
        // Source replacement is discrete and happens in set_params, never here.
        self.legacy_edits = target.legacy_edits;
        moving
    }
}

/// Slice Deck parameters. Table frames are source data, not automatable controls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SliceDeckParams {
    /// Compatibility activation bits for the original scalar automation indices.
    #[serde(default, skip_serializing_if = "no_legacy_edits")]
    #[ts(as = "Option<[u16; 4]>", optional)]
    pub legacy_edits: [u16; 4],
    pub base_key: u8,
    pub level: f32,
    pub release_ms: f32,
    /// Ordered normalized slice starts. Each end is the next start, or 1.
    pub slice_starts: [f32; SLICE_COUNT],
    pub slices: [SliceSettings; SLICE_COUNT],
}
impl Default for SliceDeckParams {
    fn default() -> Self {
        Self {
            legacy_edits: [0; 4],
            base_key: 60,
            level: 0.5,
            release_ms: 30.0,
            slice_starts: starts(),
            slices: [SliceSettings::default(); SLICE_COUNT],
        }
    }
}
impl ParamSet for SliceDeckParams {
    const NAME: &'static str = "Slice Deck";
    fn descriptors() -> &'static [ParamInfo] {
        static INFO: &[ParamInfo] = &[
            ParamInfo {
                id: "baseKey",
                name: "Base key",
                kind: ParamKind::Integer,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 127.0,
                default: 60.0,
                choices: &[],
            },
            ParamInfo {
                id: "level",
                name: "Level",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "releaseMs",
                name: "Release",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 1000.0,
                default: 30.0,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.0",
                name: "Slice 1 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.1",
                name: "Slice 2 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.2",
                name: "Slice 3 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.3",
                name: "Slice 4 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.1875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.4",
                name: "Slice 5 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.25,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.5",
                name: "Slice 6 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.3125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.6",
                name: "Slice 7 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.375,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.7",
                name: "Slice 8 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.4375,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.8",
                name: "Slice 9 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.9",
                name: "Slice 10 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.10",
                name: "Slice 11 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.625,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.11",
                name: "Slice 12 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.6875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.12",
                name: "Slice 13 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.75,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.13",
                name: "Slice 14 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.8125,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.14",
                name: "Slice 15 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.875,
                choices: &[],
            },
            ParamInfo {
                id: "sliceStarts.15",
                name: "Slice 16 start",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.9375,
                choices: &[],
            },
            ParamInfo {
                id: "slices.0.pitch",
                name: "Slice 1 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.0.gain",
                name: "Slice 1 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.0.reverse",
                name: "Slice 1 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.1.pitch",
                name: "Slice 2 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.1.gain",
                name: "Slice 2 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.1.reverse",
                name: "Slice 2 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.2.pitch",
                name: "Slice 3 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.2.gain",
                name: "Slice 3 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.2.reverse",
                name: "Slice 3 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.3.pitch",
                name: "Slice 4 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.3.gain",
                name: "Slice 4 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.3.reverse",
                name: "Slice 4 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.4.pitch",
                name: "Slice 5 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.4.gain",
                name: "Slice 5 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.4.reverse",
                name: "Slice 5 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.5.pitch",
                name: "Slice 6 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.5.gain",
                name: "Slice 6 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.5.reverse",
                name: "Slice 6 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.6.pitch",
                name: "Slice 7 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.6.gain",
                name: "Slice 7 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.6.reverse",
                name: "Slice 7 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.7.pitch",
                name: "Slice 8 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.7.gain",
                name: "Slice 8 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.7.reverse",
                name: "Slice 8 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.8.pitch",
                name: "Slice 9 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.8.gain",
                name: "Slice 9 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.8.reverse",
                name: "Slice 9 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.9.pitch",
                name: "Slice 10 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.9.gain",
                name: "Slice 10 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.9.reverse",
                name: "Slice 10 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.10.pitch",
                name: "Slice 11 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.10.gain",
                name: "Slice 11 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.10.reverse",
                name: "Slice 11 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.11.pitch",
                name: "Slice 12 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.11.gain",
                name: "Slice 12 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.11.reverse",
                name: "Slice 12 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.12.pitch",
                name: "Slice 13 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.12.gain",
                name: "Slice 13 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.12.reverse",
                name: "Slice 13 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.13.pitch",
                name: "Slice 14 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.13.gain",
                name: "Slice 14 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.13.reverse",
                name: "Slice 14 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.14.pitch",
                name: "Slice 15 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.14.gain",
                name: "Slice 15 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.14.reverse",
                name: "Slice 15 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.15.pitch",
                name: "Slice 16 pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.15.gain",
                name: "Slice 16 gain",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 2.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "slices.15.reverse",
                name: "Slice 16 reverse",
                kind: ParamKind::Toggle,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.base_key as f32,
            1 => self.level,
            2 => self.release_ms,
            3 => self.slice_starts[0],
            4 => self.slice_starts[1],
            5 => self.slice_starts[2],
            6 => self.slice_starts[3],
            7 => self.slice_starts[4],
            8 => self.slice_starts[5],
            9 => self.slice_starts[6],
            10 => self.slice_starts[7],
            11 => self.slice_starts[8],
            12 => self.slice_starts[9],
            13 => self.slice_starts[10],
            14 => self.slice_starts[11],
            15 => self.slice_starts[12],
            16 => self.slice_starts[13],
            17 => self.slice_starts[14],
            18 => self.slice_starts[15],
            19 => self.slices[0].pitch,
            20 => self.slices[0].gain,
            21 => {
                if self.slices[0].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            22 => self.slices[1].pitch,
            23 => self.slices[1].gain,
            24 => {
                if self.slices[1].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            25 => self.slices[2].pitch,
            26 => self.slices[2].gain,
            27 => {
                if self.slices[2].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            28 => self.slices[3].pitch,
            29 => self.slices[3].gain,
            30 => {
                if self.slices[3].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            31 => self.slices[4].pitch,
            32 => self.slices[4].gain,
            33 => {
                if self.slices[4].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            34 => self.slices[5].pitch,
            35 => self.slices[5].gain,
            36 => {
                if self.slices[5].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            37 => self.slices[6].pitch,
            38 => self.slices[6].gain,
            39 => {
                if self.slices[6].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            40 => self.slices[7].pitch,
            41 => self.slices[7].gain,
            42 => {
                if self.slices[7].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            43 => self.slices[8].pitch,
            44 => self.slices[8].gain,
            45 => {
                if self.slices[8].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            46 => self.slices[9].pitch,
            47 => self.slices[9].gain,
            48 => {
                if self.slices[9].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            49 => self.slices[10].pitch,
            50 => self.slices[10].gain,
            51 => {
                if self.slices[10].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            52 => self.slices[11].pitch,
            53 => self.slices[11].gain,
            54 => {
                if self.slices[11].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            55 => self.slices[12].pitch,
            56 => self.slices[12].gain,
            57 => {
                if self.slices[12].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            58 => self.slices[13].pitch,
            59 => self.slices[13].gain,
            60 => {
                if self.slices[13].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            61 => self.slices[14].pitch,
            62 => self.slices[14].gain,
            63 => {
                if self.slices[14].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            64 => self.slices[15].pitch,
            65 => self.slices[15].gain,
            66 => {
                if self.slices[15].reverse {
                    1.0
                } else {
                    0.0
                }
            }
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.base_key = value.round() as _,
            1 => self.level = value,
            2 => self.release_ms = value,
            3 => self.slice_starts[0] = value,
            4 => self.slice_starts[1] = value,
            5 => self.slice_starts[2] = value,
            6 => self.slice_starts[3] = value,
            7 => self.slice_starts[4] = value,
            8 => self.slice_starts[5] = value,
            9 => self.slice_starts[6] = value,
            10 => self.slice_starts[7] = value,
            11 => self.slice_starts[8] = value,
            12 => self.slice_starts[9] = value,
            13 => self.slice_starts[10] = value,
            14 => self.slice_starts[11] = value,
            15 => self.slice_starts[12] = value,
            16 => self.slice_starts[13] = value,
            17 => self.slice_starts[14] = value,
            18 => self.slice_starts[15] = value,
            19 => self.slices[0].pitch = value,
            20 => self.slices[0].gain = value,
            21 => self.slices[0].reverse = value >= 0.5,
            22 => self.slices[1].pitch = value,
            23 => self.slices[1].gain = value,
            24 => self.slices[1].reverse = value >= 0.5,
            25 => self.slices[2].pitch = value,
            26 => self.slices[2].gain = value,
            27 => self.slices[2].reverse = value >= 0.5,
            28 => self.slices[3].pitch = value,
            29 => self.slices[3].gain = value,
            30 => self.slices[3].reverse = value >= 0.5,
            31 => self.slices[4].pitch = value,
            32 => self.slices[4].gain = value,
            33 => self.slices[4].reverse = value >= 0.5,
            34 => self.slices[5].pitch = value,
            35 => self.slices[5].gain = value,
            36 => self.slices[5].reverse = value >= 0.5,
            37 => self.slices[6].pitch = value,
            38 => self.slices[6].gain = value,
            39 => self.slices[6].reverse = value >= 0.5,
            40 => self.slices[7].pitch = value,
            41 => self.slices[7].gain = value,
            42 => self.slices[7].reverse = value >= 0.5,
            43 => self.slices[8].pitch = value,
            44 => self.slices[8].gain = value,
            45 => self.slices[8].reverse = value >= 0.5,
            46 => self.slices[9].pitch = value,
            47 => self.slices[9].gain = value,
            48 => self.slices[9].reverse = value >= 0.5,
            49 => self.slices[10].pitch = value,
            50 => self.slices[10].gain = value,
            51 => self.slices[10].reverse = value >= 0.5,
            52 => self.slices[11].pitch = value,
            53 => self.slices[11].gain = value,
            54 => self.slices[11].reverse = value >= 0.5,
            55 => self.slices[12].pitch = value,
            56 => self.slices[12].gain = value,
            57 => self.slices[12].reverse = value >= 0.5,
            58 => self.slices[13].pitch = value,
            59 => self.slices[13].gain = value,
            60 => self.slices[13].reverse = value >= 0.5,
            61 => self.slices[14].pitch = value,
            62 => self.slices[14].gain = value,
            63 => self.slices[14].reverse = value >= 0.5,
            64 => self.slices[15].pitch = value,
            65 => self.slices[15].gain = value,
            66 => self.slices[15].reverse = value >= 0.5,
            _ => return false,
        }
        if index >= 3 {
            let bit = index - 3;
            self.legacy_edits[bit / 16] |= 1 << (bit % 16);
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut clean = *self;
        for index in 0..Self::descriptors().len() {
            clean.set(index, self.get(index).unwrap());
        }
        clean.legacy_edits = self.legacy_edits;
        clean
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let amount = clean(amount, 0.0, 1.0, 1.0);
        let mut moving = false;
        for (index, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(index).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) = approach_value(
                    self.get(index).unwrap(),
                    target,
                    amount,
                    info.max - info.min,
                );
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(index, value);
        }
        // Source replacement is discrete and happens in set_params, never here.
        self.legacy_edits = target.legacy_edits;
        moving
    }
}

/// Grain Cloud parameters. Table frames are source data, not automatable controls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct GrainCloudParams {
    pub root_key: u8,
    pub grain_size_ms: f32,
    pub density: f32,
    pub position: f32,
    pub spray: f32,
    pub pitch: f32,
    pub seed: u32,
    pub level: f32,
    pub release_ms: f32,
}
impl Default for GrainCloudParams {
    fn default() -> Self {
        Self {
            root_key: 60,
            grain_size_ms: 80.0,
            density: 24.0,
            position: 0.5,
            spray: 1.0,
            pitch: 0.0,
            seed: 1,
            level: 0.5,
            release_ms: 80.0,
        }
    }
}
impl ParamSet for GrainCloudParams {
    const NAME: &'static str = "Grain Cloud";
    fn descriptors() -> &'static [ParamInfo] {
        static INFO: &[ParamInfo] = &[
            ParamInfo {
                id: "rootKey",
                name: "Root key",
                kind: ParamKind::Integer,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 127.0,
                default: 60.0,
                choices: &[],
            },
            ParamInfo {
                id: "grainSizeMs",
                name: "Grain size",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 250.0,
                default: 80.0,
                choices: &[],
            },
            ParamInfo {
                id: "density",
                name: "Density",
                kind: ParamKind::Float,
                unit: ParamUnit::Hertz,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 128.0,
                default: 24.0,
                choices: &[],
            },
            ParamInfo {
                id: "position",
                name: "Position",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "spray",
                name: "Spray",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "pitch",
                name: "Pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "seed",
                name: "Seed",
                kind: ParamKind::Integer,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 16777215.0,
                default: 1.0,
                choices: &[],
            },
            ParamInfo {
                id: "level",
                name: "Level",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "releaseMs",
                name: "Release",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 1000.0,
                default: 80.0,
                choices: &[],
            },
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.root_key as f32,
            1 => self.grain_size_ms,
            2 => self.density,
            3 => self.position,
            4 => self.spray,
            5 => self.pitch,
            6 => self.seed as f32,
            7 => self.level,
            8 => self.release_ms,
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.root_key = value.round() as _,
            1 => self.grain_size_ms = value,
            2 => self.density = value,
            3 => self.position = value,
            4 => self.spray = value,
            5 => self.pitch = value,
            6 => self.seed = value.round() as _,
            7 => self.level = value,
            8 => self.release_ms = value,
            _ => return false,
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut clean = *self;
        for index in 0..Self::descriptors().len() {
            clean.set(index, self.get(index).unwrap());
        }

        clean
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let amount = clean(amount, 0.0, 1.0, 1.0);
        let mut moving = false;
        for (index, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(index).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) = approach_value(
                    self.get(index).unwrap(),
                    target,
                    amount,
                    info.max - info.min,
                );
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(index, value);
        }
        // Source replacement is discrete and happens in set_params, never here.
        moving
    }
}

/// Wave Ride parameters. Table frames are source data, not automatable controls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct WaveRideParams {
    /// Compatibility activation bits for the original scalar automation indices.
    #[serde(default, skip_serializing_if = "no_legacy_edits")]
    #[ts(as = "Option<[u16; 4]>", optional)]
    pub legacy_edits: [u16; 4],
    pub root_key: u8,
    pub duration_ms: f32,
    pub pitch: f32,
    pub level: f32,
    pub release_ms: f32,

    /// Eight normalized source positions at equally spaced envelope times.
    pub positions: [f32; 8],
}
impl Default for WaveRideParams {
    fn default() -> Self {
        Self {
            legacy_edits: [0; 4],
            root_key: 60,
            duration_ms: 1000.0,
            pitch: 0.0,
            level: 0.5,
            release_ms: 30.0,

            positions: std::array::from_fn(|i| i as f32 / 7.0),
        }
    }
}
impl ParamSet for WaveRideParams {
    const NAME: &'static str = "Wave Ride";
    fn descriptors() -> &'static [ParamInfo] {
        static INFO: &[ParamInfo] = &[
            ParamInfo {
                id: "rootKey",
                name: "Root key",
                kind: ParamKind::Integer,
                unit: ParamUnit::None,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 127.0,
                default: 60.0,
                choices: &[],
            },
            ParamInfo {
                id: "durationMs",
                name: "Scan duration",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 10000.0,
                default: 1000.0,
                choices: &[],
            },
            ParamInfo {
                id: "pitch",
                name: "Pitch",
                kind: ParamKind::Float,
                unit: ParamUnit::Semitones,
                scale: ParamScale::Linear,
                min: -48.0,
                max: 48.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "level",
                name: "Level",
                kind: ParamKind::Float,
                unit: ParamUnit::Gain,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.5,
                choices: &[],
            },
            ParamInfo {
                id: "releaseMs",
                name: "Release",
                kind: ParamKind::Float,
                unit: ParamUnit::Milliseconds,
                scale: ParamScale::Linear,
                min: 1.0,
                max: 1000.0,
                default: 30.0,
                choices: &[],
            },
            ParamInfo {
                id: "positions.0",
                name: "Position 1",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.0,
                choices: &[],
            },
            ParamInfo {
                id: "positions.1",
                name: "Position 2",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.142_857_15,
                choices: &[],
            },
            ParamInfo {
                id: "positions.2",
                name: "Position 3",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.285_714_3,
                choices: &[],
            },
            ParamInfo {
                id: "positions.3",
                name: "Position 4",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.428_571_43,
                choices: &[],
            },
            ParamInfo {
                id: "positions.4",
                name: "Position 5",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.571_428_6,
                choices: &[],
            },
            ParamInfo {
                id: "positions.5",
                name: "Position 6",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.714_285_73,
                choices: &[],
            },
            ParamInfo {
                id: "positions.6",
                name: "Position 7",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 0.857_142_87,
                choices: &[],
            },
            ParamInfo {
                id: "positions.7",
                name: "Position 8",
                kind: ParamKind::Float,
                unit: ParamUnit::Fraction,
                scale: ParamScale::Linear,
                min: 0.0,
                max: 1.0,
                default: 1.0,
                choices: &[],
            },
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.root_key as f32,
            1 => self.duration_ms,
            2 => self.pitch,
            3 => self.level,
            4 => self.release_ms,
            5 => self.positions[0],
            6 => self.positions[1],
            7 => self.positions[2],
            8 => self.positions[3],
            9 => self.positions[4],
            10 => self.positions[5],
            11 => self.positions[6],
            12 => self.positions[7],
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.root_key = value.round() as _,
            1 => self.duration_ms = value,
            2 => self.pitch = value,
            3 => self.level = value,
            4 => self.release_ms = value,
            5 => self.positions[0] = value,
            6 => self.positions[1] = value,
            7 => self.positions[2] = value,
            8 => self.positions[3] = value,
            9 => self.positions[4] = value,
            10 => self.positions[5] = value,
            11 => self.positions[6] = value,
            12 => self.positions[7] = value,
            _ => return false,
        }
        if index >= 5 {
            let bit = index - 5;
            self.legacy_edits[bit / 16] |= 1 << (bit % 16);
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut clean = *self;
        for index in 0..Self::descriptors().len() {
            clean.set(index, self.get(index).unwrap());
        }

        clean.legacy_edits = self.legacy_edits;
        clean
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let amount = clean(amount, 0.0, 1.0, 1.0);
        let mut moving = false;
        for (index, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(index).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) = approach_value(
                    self.get(index).unwrap(),
                    target,
                    amount,
                    info.max - info.min,
                );
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(index, value);
        }
        // Source replacement is discrete and happens in set_params, never here.
        self.legacy_edits = target.legacy_edits;
        moving
    }
}

fn no_legacy_edits(bits: &[u16; 4]) -> bool {
    bits.iter().all(|&word| word == 0)
}
macro_rules! legacy_changes {
    ($ty:ty, $prefix:expr) => {
        impl $ty {
            pub fn with_legacy_changes(mut self, previous: &Self) -> Self {
                for index in $prefix..Self::descriptors().len() {
                    let bit = index - $prefix;
                    if self.get(index) != previous.get(index) {
                        self.legacy_edits[bit / 16] |= 1 << (bit % 16);
                    }
                }
                for (word, old) in self.legacy_edits.iter_mut().zip(previous.legacy_edits) {
                    *word |= old;
                }
                self
            }
        }
    };
}
legacy_changes!(SliceMapParams, 3);
legacy_changes!(SliceDeckParams, 3);
legacy_changes!(WaveRideParams, 5);
impl GrainCloudParams {
    pub fn with_legacy_changes(self, _previous: &Self) -> Self {
        self
    }
}
