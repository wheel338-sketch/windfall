use super::{FIRST_PAD_KEY, MAX_SAMPLE_FRAMES, MAX_ZONES, PAD_COUNT};
use crate::param::{ParamChoice, ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn clean(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        default
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CrossfadeAxis {
    #[default]
    Velocity,
    Key,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LoopMode {
    #[default]
    Off,
    Continuous,
    UntilRelease,
}

/// Mono PCM. Only `data[..len]` plays; an empty source is silent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Sample {
    #[serde(with = "sample_serde")]
    #[ts(type = "number[]")]
    pub data: [f32; MAX_SAMPLE_FRAMES],
    pub len: u16,
    pub sample_rate: f32,
}
impl Default for Sample {
    fn default() -> Self {
        Self {
            data: [0.0; MAX_SAMPLE_FRAMES],
            len: 0,
            sample_rate: 48_000.0,
        }
    }
}
impl Sample {
    /// Rejects oversized sources; never truncates imported audio silently.
    pub fn from_slice(data: &[f32], sample_rate: f32) -> Result<Self, ZoneTableError> {
        if data.len() > MAX_SAMPLE_FRAMES {
            return Err(ZoneTableError::TooManyFrames { frames: data.len() });
        }
        let mut sample = Self {
            len: data.len() as u16,
            sample_rate,
            ..Self::default()
        };
        sample.data[..data.len()].copy_from_slice(data);
        sample.sanitize();
        Ok(sample)
    }
    fn sanitize(&mut self) {
        self.len = self.len.min(MAX_SAMPLE_FRAMES as u16);
        self.sample_rate = clean(self.sample_rate, 1000.0, 384_000.0, 48_000.0);
        for frame in &mut self.data {
            *frame = clean(*frame, -1.0, 1.0, 0.0);
        }
    }
}
mod sample_serde {
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
        struct Frames;
        impl<'de> Visitor<'de> for Frames {
            type Value = [f32; MAX_SAMPLE_FRAMES];
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("at most 1024 mono frames")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut data = [0.0; MAX_SAMPLE_FRAMES];
                let mut count = 0;
                while let Some(value) = seq.next_element()? {
                    if count == MAX_SAMPLE_FRAMES {
                        return Err(A::Error::custom("sample exceeds 1024 frames"));
                    }
                    data[count] = value;
                    count += 1;
                }
                Ok(data)
            }
        }
        deserializer.deserialize_seq(Frames)
    }
}

/// Inclusive MIDI key and normalized velocity ranges. Reversed ranges are empty.
/// Loop endpoints are source frame indices, with an exclusive end.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Zone {
    pub sample: Sample,
    pub key_low: u8,
    pub key_high: u8,
    pub velocity_low: f32,
    pub velocity_high: f32,
    pub root_key: u8,
    pub gain: f32,
    pub pan: f32,
    pub tune_cents: f32,
    pub loop_mode: LoopMode,
    pub loop_start: u16,
    pub loop_end: u16,
}
impl Default for Zone {
    fn default() -> Self {
        Self {
            sample: Sample::default(),
            key_low: 0,
            key_high: 127,
            velocity_low: 0.0,
            velocity_high: 1.0,
            root_key: 60,
            gain: 1.0,
            pan: 0.0,
            tune_cents: 0.0,
            loop_mode: LoopMode::Off,
            loop_start: 0,
            loop_end: 0,
        }
    }
}
impl Zone {
    pub fn sanitized(&self) -> Self {
        let mut zone = *self;
        zone.sample.sanitize();
        zone.key_low = zone.key_low.min(127);
        zone.key_high = zone.key_high.min(127);
        zone.root_key = zone.root_key.min(127);
        zone.velocity_low = clean(zone.velocity_low, 0.0, 1.0, 0.0);
        zone.velocity_high = clean(zone.velocity_high, 0.0, 1.0, 1.0);
        zone.gain = clean(zone.gain, 0.0, 4.0, 1.0);
        zone.pan = clean(zone.pan, -1.0, 1.0, 0.0);
        zone.tune_cents = clean(zone.tune_cents, -12_000.0, 12_000.0, 0.0);
        zone.loop_start = zone.loop_start.min(zone.sample.len);
        zone.loop_end = zone.loop_end.min(zone.sample.len);
        if zone.loop_start >= zone.loop_end {
            zone.loop_mode = LoopMode::Off;
        }
        zone
    }
    pub(super) fn matches(&self, key: u8, velocity: f32) -> bool {
        self.sample.len != 0
            && key >= self.key_low
            && key <= self.key_high
            && velocity >= self.velocity_low
            && velocity <= self.velocity_high
    }
    pub(super) fn weight(&self, key: u8, velocity: f32, axis: CrossfadeAxis) -> f32 {
        let (value, low, high) = match axis {
            CrossfadeAxis::Velocity => (velocity, self.velocity_low, self.velocity_high),
            CrossfadeAxis::Key => (key as f32, self.key_low as f32, self.key_high as f32),
        };
        if high <= low {
            1.0
        } else {
            (1.0 - (2.0 * (value - low) / (high - low) - 1.0).abs()).max(0.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ZoneTable {
    pub zones: [Zone; MAX_ZONES],
    pub len: u8,
}
impl Default for ZoneTable {
    fn default() -> Self {
        Self {
            zones: [Zone::default(); MAX_ZONES],
            len: 0,
        }
    }
}
impl ZoneTable {
    pub fn from_slice(zones: &[Zone]) -> Result<Self, ZoneTableError> {
        if zones.len() > MAX_ZONES {
            return Err(ZoneTableError::TooManyZones { zones: zones.len() });
        }
        let mut table = Self {
            len: zones.len() as u8,
            ..Self::default()
        };
        for (target, zone) in table.zones.iter_mut().zip(zones) {
            *target = zone.sanitized();
        }
        Ok(table)
    }
    pub fn sanitized(&self) -> Self {
        let mut table = *self;
        table.len = table.len.min(MAX_ZONES as u8);
        for zone in &mut table.zones {
            *zone = zone.sanitized();
        }
        table
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneTableError {
    TooManyZones { zones: usize },
    TooManyFrames { frames: usize },
}
impl std::fmt::Display for ZoneTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let notice = windfall_core::notice!("dsp.zoneTable", detail = format!("{self:?}"));
        write!(f, "{notice}")
    }
}
impl std::error::Error for ZoneTableError {}

const CONTROLS: &[ParamInfo] = &[
    ParamInfo {
        id: "level",
        name: "Level",
        kind: ParamKind::Float,
        unit: ParamUnit::Gain,
        scale: ParamScale::Linear,
        min: 0.0,
        max: 2.0,
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
        max: 10_000.0,
        default: 100.0,
        choices: &[],
    },
    ParamInfo {
        id: "crossfade",
        name: "Crossfade",
        kind: ParamKind::Choice,
        unit: ParamUnit::None,
        scale: ParamScale::Linear,
        min: 0.0,
        max: 1.0,
        default: 0.0,
        choices: &[
            ParamChoice {
                value: "velocity",
                label: "Velocity",
            },
            ParamChoice {
                value: "key",
                label: "Key",
            },
        ],
    },
];

macro_rules! parameters {
    ($ty:ident, $name:literal, $locked:expr, $layout:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", default)]
        pub struct $ty {
            pub zones: ZoneTable,
            /// Linear global gain, 0..2; default 0.5.
            pub level: f32,
            /// Linear note-off fade in milliseconds, 1..10000; default 100.
            pub release_ms: f32,
            pub crossfade: CrossfadeAxis,
            /// Prevents zone replacement through set_params. Always true for ZonePlayer.
            pub zones_locked: bool,
        }
        impl Default for $ty {
            fn default() -> Self {
                let mut zones = ZoneTable::default();
                $layout(&mut zones);
                Self {
                    zones,
                    level: 0.5,
                    release_ms: 100.0,
                    crossfade: CrossfadeAxis::Velocity,
                    zones_locked: $locked,
                }
            }
        }
        impl ParamSet for $ty {
            const NAME: &'static str = $name;
            fn descriptors() -> &'static [ParamInfo] {
                CONTROLS
            }
            fn get(&self, index: usize) -> Option<f32> {
                match index {
                    0 => Some(self.level),
                    1 => Some(self.release_ms),
                    2 => Some(if self.crossfade == CrossfadeAxis::Key {
                        1.0
                    } else {
                        0.0
                    }),
                    _ => None,
                }
            }
            fn set(&mut self, index: usize, value: f32) -> bool {
                match index {
                    0 => self.level = clean(value, 0.0, 2.0, 0.5),
                    1 => self.release_ms = clean(value, 1.0, 10_000.0, 100.0),
                    2 => {
                        if value.is_finite() {
                            self.crossfade = if value >= 0.5 {
                                CrossfadeAxis::Key
                            } else {
                                CrossfadeAxis::Velocity
                            };
                        }
                    }
                    _ => return false,
                }
                true
            }
            fn sanitized(&self) -> Self {
                let mut params = *self;
                params.level = clean(params.level, 0.0, 2.0, 0.5);
                params.release_ms = clean(params.release_ms, 1.0, 10_000.0, 100.0);
                params.zones = params.zones.sanitized();
                params.zones_locked |= $locked;
                $layout(&mut params.zones);
                params
            }
            fn approach(&mut self, target: &Self, amount: f32) -> bool {
                let amount = clean(amount, 0.0, 1.0, 1.0);
                let mut moving = false;
                for (current, target, span) in [
                    (&mut self.level, target.level, 2.0),
                    (&mut self.release_ms, target.release_ms, 9999.0),
                ] {
                    let gap = target - *current;
                    if gap.abs() <= span * 1.0e-4 {
                        *current = target;
                    } else {
                        *current += gap * amount;
                        moving = true;
                    }
                }
                self.crossfade = target.crossfade;
                self.zones_locked = target.zones_locked || $locked;
                // Sample payloads and mappings are discrete and applied by set_params.
                moving
            }
        }
    };
}
fn unchanged(_zones: &mut ZoneTable) {}
fn pads(zones: &mut ZoneTable) {
    zones.len = zones.len.min(PAD_COUNT as u8);
    for (index, zone) in zones.zones.iter_mut().take(PAD_COUNT).enumerate() {
        let key = FIRST_PAD_KEY + index as u8;
        zone.key_low = key;
        zone.key_high = key;
        zone.root_key = key;
    }
}
fn keyboard(zones: &mut ZoneTable) {
    zones.len = zones.len.min(1);
    zones.zones[0].key_low = 0;
    zones.zones[0].key_high = 127;
}
parameters!(ZoneSamplerParams, "Zone Sampler", false, unchanged);
parameters!(ZonePlayerParams, "Zone Player", true, unchanged);
parameters!(PadSamplerParams, "Pad Sampler", false, pads);
parameters!(KeyBedParams, "Key Bed", false, keyboard);
