//! Immutable worker-prepared sample assets. PCM never belongs in Copy params.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::AudioBuffer;

pub const MAX_BANK_ZONES: usize = 256;
pub const MAX_BANK_BYTES: usize = 256 * 1024 * 1024;
pub const BANK_VOICES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BankLoop {
    #[default]
    Off,
    Continuous,
    UntilRelease,
}

/// Musical mapping only. `sample` is a project SampleId, never a PCM payload.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SampleZone {
    pub sample: u32,
    pub key_low: u8,
    pub key_high: u8,
    pub velocity_low: u8,
    pub velocity_high: u8,
    pub root_key: u8,
    pub tune_cents: f32,
    pub gain: f32,
    pub pan: f32,
    /// Independent fades inside the inclusive key/velocity ranges.
    pub key_fade: u8,
    pub velocity_fade: u8,
    /// Zero layers unconditionally; nonzero groups select one member per strike.
    pub round_robin: u8,
    pub choke_group: u8,
    pub loop_mode: BankLoop,
    pub loop_start: u32,
    pub loop_end: u32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub scale_tuning: f32,
    #[serde(default)]
    #[ts(as = "Option<u8>", optional)]
    pub modulation_group: u8,
    #[serde(default)]
    #[ts(as = "Option<super::ZoneModulation>", optional)]
    pub modulation: super::ZoneModulation,
    #[serde(default)]
    #[ts(as = "Option<f32>", optional)]
    pub delay_ms: f32,
}
impl Default for SampleZone {
    fn default() -> Self {
        Self {
            sample: 0,
            key_low: 0,
            key_high: 127,
            velocity_low: 0,
            velocity_high: 127,
            root_key: 60,
            tune_cents: 0.0,
            gain: 1.0,
            pan: 0.0,
            key_fade: 0,
            velocity_fade: 0,
            round_robin: 0,
            choke_group: 0,
            loop_mode: BankLoop::Off,
            loop_start: 0,
            loop_end: 0,
            attack_ms: 0.0,
            hold_ms: 0.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 100.0,
            scale_tuning: 100.0,
            modulation_group: 0, modulation: super::ZoneModulation::default(), delay_ms: 0.0,
        }
    }
}
impl SampleZone {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.modulation.validate()?;
        if self.key_low > self.key_high
            || self.key_high > 127
            || self.root_key > 127
            || self.velocity_low > self.velocity_high
            || self.velocity_high > 127
        {
            return Err("zone key and velocity ranges must be ordered within 0..127");
        }
        if !self.gain.is_finite()
            || !(0.0..=4.0).contains(&self.gain)
            || !self.pan.is_finite()
            || !(-1.0..=1.0).contains(&self.pan)
            || !self.tune_cents.is_finite()
            || self.tune_cents.abs() > 12000.0
            || !self.scale_tuning.is_finite()
            || !(0.0..=1200.0).contains(&self.scale_tuning)
            || !self.sustain.is_finite()
            || !(0.0..=1.0).contains(&self.sustain)
        {
            return Err("zone gain, pan, tuning or sustain is invalid");
        }
        if [self.delay_ms, self.attack_ms, self.hold_ms, self.decay_ms, self.release_ms]
            .iter()
            .any(|x| !x.is_finite() || !(0.0..=100000.0).contains(x))
        {
            return Err("zone envelope times must be finite within 0..100000 ms");
        }
        if self.loop_mode != BankLoop::Off && self.loop_start >= self.loop_end {
            return Err("zone loop start must precede its end");
        }
        Ok(())
    }
    pub fn weight(&self, key: u8, velocity: u8) -> f32 {
        if key < self.key_low
            || key > self.key_high
            || velocity < self.velocity_low
            || velocity > self.velocity_high
        {
            return 0.0;
        }
        let fade = |value: u8, low: u8, high: u8, width: u8| {
            if width == 0 {
                1.0
            } else {
                (f32::from(value - low + 1) / f32::from(width))
                    .min(1.0)
                    .min(f32::from(high - value + 1) / f32::from(width))
            }
        };
        fade(key, self.key_low, self.key_high, self.key_fade)
            * fade(
                velocity,
                self.velocity_low,
                self.velocity_high,
                self.velocity_fade,
            )
    }
}

#[derive(Debug, Clone)]
pub struct LoadedZone {
    pub mapping: SampleZone,
    pub audio: AudioBuffer,
}

#[derive(Debug)]
pub struct SampleBank {
    zones: Box<[LoadedZone]>,
    bytes: usize,
    // Leases are owned by the immutable bank, including unpublished work.
    reservations: Vec<super::BankReservation>,
    unavailable: Option<String>,
}
impl SampleBank {
    /// Worker/control side only. Missing sources are errors, never dropped zones.
    pub fn new(mut zones: Vec<LoadedZone>) -> Result<Self, &'static str> {
        if zones.len() > MAX_BANK_ZONES {
            return Err("sample bank exceeds 256 zones");
        }
        let mut bytes = 0usize;
        for zone in &zones {
            zone.mapping.validate()?;
            if zone.audio.frames() == 0 {
                return Err("sample zone source is empty");
            }
            if zone.audio.channels() > 2 {
                return Err("sample zones require mono or stereo PCM");
            }
            if zone.audio.samples().iter().any(|x| !x.is_finite()) {
                return Err("sample PCM is not finite");
            }
            if zone.mapping.loop_mode != BankLoop::Off
                && zone.mapping.loop_end as usize > zone.audio.frames()
            {
                return Err("zone loop exceeds source");
            }
            // Conservative bound charges shared sources per zone; never undercounts.
            bytes = bytes
                .checked_add(zone.audio.samples().len() * 4)
                .ok_or("sample bank byte overflow")?;
            if bytes > MAX_BANK_BYTES {
                return Err("sample bank exceeds 256 MiB");
            }
        }
        let mut groups = [None; 256];
        for zone in &mut zones {
            let group = usize::from(zone.mapping.modulation_group);
            if group != 0 {
                zone.mapping.modulation = *groups[group].get_or_insert(zone.mapping.modulation);
            }
        }
        Ok(Self {
            zones: zones.into_boxed_slice(),
            bytes,
            reservations: Vec::new(),
            unavailable: None,
        })
    }
    /// Preserve the document's mapping when sources are absent. The bank is
    /// silent as a whole until the worker can prepare every referenced source.
    pub fn unavailable(reason: String) -> Self {
        Self { zones: Box::new([]), bytes: 0, reservations: Vec::new(), unavailable: Some(reason) }
    }
    pub fn unavailable_reason(&self) -> Option<&str> { self.unavailable.as_deref() }
    /// Off audio only. The reservation must be taken before preparing PCM.
    pub fn retain_reservation(&mut self, reservation: super::BankReservation) {
        self.reservations.push(reservation);
    }
    pub fn zones(&self) -> &[LoadedZone] {
        &self.zones
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
