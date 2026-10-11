//! Compact scalar parameters. Legacy table decoding is control-side only.
use super::MAX_SAMPLE_FRAMES;
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SliceMapParams {
    pub base_key: u8,
    pub level: f32,
    pub release_ms: f32,
}
impl Default for SliceMapParams {
    fn default() -> Self {
        Self {
            base_key: 60,
            level: 0.5,
            release_ms: 30.0,
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
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.base_key as f32,
            1 => self.level,
            2 => self.release_ms,
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.base_key = value.round() as u8,
            1 => self.level = value,
            2 => self.release_ms = value,
            _ => return false,
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut result = *self;
        for i in 0..Self::descriptors().len() {
            result.set(i, self.get(i).unwrap());
        }
        result
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let mut moving = false;
        let amount = clean(amount, 0.0, 1.0, 1.0);
        for (i, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(i).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) =
                    approach_value(self.get(i).unwrap(), target, amount, info.max - info.min);
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(i, value);
        }
        moving
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SliceDeckParams {
    pub base_key: u8,
    pub level: f32,
    pub release_ms: f32,
}
impl Default for SliceDeckParams {
    fn default() -> Self {
        Self {
            base_key: 60,
            level: 0.5,
            release_ms: 30.0,
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
        ];
        INFO
    }
    fn get(&self, index: usize) -> Option<f32> {
        Some(match index {
            0 => self.base_key as f32,
            1 => self.level,
            2 => self.release_ms,
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.base_key = value.round() as u8,
            1 => self.level = value,
            2 => self.release_ms = value,
            _ => return false,
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut result = *self;
        for i in 0..Self::descriptors().len() {
            result.set(i, self.get(i).unwrap());
        }
        result
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let mut moving = false;
        let amount = clean(amount, 0.0, 1.0, 1.0);
        for (i, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(i).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) =
                    approach_value(self.get(i).unwrap(), target, amount, info.max - info.min);
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(i, value);
        }
        moving
    }
}
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
            0 => self.root_key = value.round() as u8,
            1 => self.grain_size_ms = value,
            2 => self.density = value,
            3 => self.position = value,
            4 => self.spray = value,
            5 => self.pitch = value,
            6 => self.seed = value.round() as u32,
            7 => self.level = value,
            8 => self.release_ms = value,
            _ => return false,
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut result = *self;
        for i in 0..Self::descriptors().len() {
            result.set(i, self.get(i).unwrap());
        }
        result
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let mut moving = false;
        let amount = clean(amount, 0.0, 1.0, 1.0);
        for (i, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(i).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) =
                    approach_value(self.get(i).unwrap(), target, amount, info.max - info.min);
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(i, value);
        }
        moving
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct WaveRideParams {
    pub root_key: u8,
    pub duration_ms: f32,
    pub pitch: f32,
    pub level: f32,
    pub release_ms: f32,
}
impl Default for WaveRideParams {
    fn default() -> Self {
        Self {
            root_key: 60,
            duration_ms: 1000.0,
            pitch: 0.0,
            level: 0.5,
            release_ms: 30.0,
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
            _ => return None,
        })
    }
    fn set(&mut self, index: usize, value: f32) -> bool {
        let Some(info) = Self::descriptors().get(index) else {
            return false;
        };
        let value = clean(value, info.min, info.max, info.default);
        match index {
            0 => self.root_key = value.round() as u8,
            1 => self.duration_ms = value,
            2 => self.pitch = value,
            3 => self.level = value,
            4 => self.release_ms = value,
            _ => return false,
        }
        true
    }
    fn sanitized(&self) -> Self {
        let mut result = *self;
        for i in 0..Self::descriptors().len() {
            result.set(i, self.get(i).unwrap());
        }
        result
    }
    fn approach(&mut self, target: &Self, amount: f32) -> bool {
        let mut moving = false;
        let amount = clean(amount, 0.0, 1.0, 1.0);
        for (i, info) in Self::descriptors().iter().enumerate() {
            let target = target.get(i).unwrap();
            let value = if info.kind == ParamKind::Float {
                let (value, unsettled) =
                    approach_value(self.get(i).unwrap(), target, amount, info.max - info.min);
                moving |= unsettled;
                value
            } else {
                target
            };
            self.set(i, value);
        }
        moving
    }
}
