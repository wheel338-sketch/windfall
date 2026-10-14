//! Explicit version-eleven words. These offsets, not Rust object layouts, are the ABI.
//!
//! Version eleven adds what a helper needs to take a sounding note over from
//! the helper it replaces: the ramps handed in with a block, the request to
//! fade out before leaving, and the ramps reported back while it does. It
//! also lets a plugin without note instances own its notes by id, so the
//! channel can apply their pan and volume. A version-ten peer is refused.

use crate::{HostEvent, Transport};

pub const VERSION: u32 = 11;
pub const DEFAULT_BLOCK: usize = 256;
pub const MAGIC: u32 = u32::from_le_bytes(*b"WFBR");
pub const SLOT_COUNT: usize = 4;
pub const MAX_BLOCK: usize = 512;
pub const ORDINARY_EVENTS: usize = 1024;
pub const EVENT_CAPACITY: usize = ORDINARY_EVENTS + 16 * 128 + 2 * crate::MAX_NOTE_INSTANCES + 1;
pub const PARAM_CAPACITY: usize = 4096;
pub const HEADER_WORDS: usize = 64;
pub const HELPER_FAILURE: usize = 26;
pub const OWNER_COMPLETIONS: usize = 27;
pub const TIMELINE_EPOCH: usize = 28;
pub const NATIVE_CHANGES: usize = 29;
pub const READMISSION: usize = 30;
pub const META_WORDS: usize = 64;
pub const INPUT: usize = META_WORDS;
pub const KEY_INPUT: usize = INPUT + MAX_BLOCK * 2;
pub const OUTPUT: usize = KEY_INPUT + MAX_BLOCK * 2;
pub const AUX_OUTPUT: usize = OUTPUT + MAX_BLOCK * 2;
pub const NOTES: usize = AUX_OUTPUT + MAX_BLOCK * 2 * crate::MAX_PORTS as usize;
pub const NOTE_KEYS: usize = 16 * 128;
pub const NOTE_SNAPSHOT_WORDS: usize = 25;
pub const INSTANCES: usize = NOTES + NOTE_KEYS;
/// One area for both directions: the host hands ramps in with a block, and
/// the helper reports the ramps it holds with its reply to the same block.
pub const RAMP_STAGE: usize = INSTANCES + crate::MAX_NOTE_INSTANCES * NOTE_SNAPSHOT_WORDS;
pub const RAMP_NOTES: usize = RAMP_STAGE + crate::project_expression::STAGE_WORDS;
pub const LAW_EVENTS: usize =
    RAMP_NOTES + crate::MAX_NOTE_INSTANCES * crate::project_expression::CARRIED_WORDS;
pub const PARAMETERS: usize = LAW_EVENTS + MAX_BLOCK * 2;
pub const PROJECT_LAW: usize = 56;
pub const PROJECT_LAW_COUNT: usize = 57;
pub const INSTANCE_COUNT: usize = 54;
/// Ramps handed in with this block; [`STAGE_PRESENT`] says a stage comes too.
pub const HANDED_COUNT: usize = 58;
/// Bit 0: the owner wants the plugin and the output is to fade out.
pub const DEPART: usize = 59;
/// The frame of this block from which [`DEPART`] holds; 0 when it held before.
pub const DEPART_AT: usize = 60;
/// Ramps reported with this reply; [`STAGE_PRESENT`] says a stage comes too.
pub const REPORT_COUNT: usize = 61;
/// Bit 0: the output has faded out for the requested departure.
pub const REPORT_FLAGS: usize = 62;
pub const STAGE_PRESENT: u32 = 1 << 31;
pub const OUTPUT_COUNT: usize = 55;
pub const EVENTS: usize = PARAMETERS + PARAM_CAPACITY * 3;
pub const EVENT_WORDS: usize = 8;
pub const SLOT_WORDS: usize = (EVENTS + EVENT_CAPACITY * EVENT_WORDS).next_multiple_of(16);
pub const REGION_WORDS: usize = HEADER_WORDS + SLOT_COUNT * SLOT_WORDS;
pub const REGION_BYTES: usize = REGION_WORDS * 4;
pub const STATE: usize = 0;
pub const IDENTITY: usize = 1;
pub const SEQUENCE: usize = 9;
pub const FRAMES: usize = 11;
pub const EVENT_COUNT: usize = 12;
pub const PARAM_COUNT: usize = 13;
pub const FLAGS: usize = 14;
pub const NOTE_CHANNEL: usize = 15;
pub const TRANSPORT: usize = 16;
pub const EPOCH: usize = 30;
pub const CONTROL_START: usize = 48;
pub const CONTROL_END: usize = 50;
pub const REPLY_IDENTITY: usize = 32;
pub const REPLY_SEQUENCE: usize = 40;
pub const REPLY_FRAMES: usize = 42;
pub const REPLY_STATUS: usize = 43;
pub const REPLY_EPOCH: usize = 44;
pub const PROCESSED_GENERATION: usize = 46;
pub const NATIVE_DROPS: usize = 52;
pub const SIDECHAIN_PORT: usize = 53;
pub const EMPTY: u32 = 0;
pub const HOST_WRITE: u32 = 1;
pub const READY: u32 = 2;
pub const HELPER_WRITE: u32 = 3;
pub const DONE: u32 = 4;
pub const HOST_READ: u32 = 5;
pub const OUTPUT_OK: u32 = 0;
pub const OUTPUT_FAILED: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub session: u64,
    pub token: u64,
    pub revision: u64,
    pub binding: u64,
}

impl Identity {
    pub fn valid(self) -> bool {
        self.session != 0 && self.token != 0
    }
    pub fn encode(self) -> [u32; 8] {
        let values = [self.session, self.token, self.revision, self.binding];
        std::array::from_fn(|index| (values[index / 2] >> (32 * (index % 2))) as u32)
    }
    pub fn decode(words: [u32; 8]) -> Result<Self, ProtocolError> {
        let value = Self {
            session: pair(words[0], words[1]),
            token: pair(words[2], words[3]),
            revision: pair(words[4], words[5]),
            binding: pair(words[6], words[7]),
        };
        if !value.valid() {
            return Err(ProtocolError::Identity);
        }
        Ok(value)
    }
}

pub const fn pair(low: u32, high: u32) -> u64 {
    u64::from_le_bytes([
        low as u8,
        (low >> 8) as u8,
        (low >> 16) as u8,
        (low >> 24) as u8,
        high as u8,
        (high >> 8) as u8,
        (high >> 16) as u8,
        (high >> 24) as u8,
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Effect,
    Instrument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub identity: Identity,
    pub sample_rate: u32,
    pub block: usize,
    pub native_latency: usize,
    pub kind: Kind,
}

impl Config {
    pub fn validate(self) -> Result<Self, ProtocolError> {
        if !self.identity.valid() {
            return Err(ProtocolError::Identity);
        }
        if !(8_000..=384_000).contains(&self.sample_rate)
            || !(1..=MAX_BLOCK).contains(&self.block)
            || self
                .native_latency
                .checked_add(self.block * 2)
                .is_none_or(|latency| latency > self.sample_rate as usize)
        {
            return Err(ProtocolError::Configuration);
        }
        Ok(self)
    }
    pub fn latency(self) -> usize {
        self.native_latency + self.block * 2
    }
    pub fn header(self) -> Result<[u32; HEADER_WORDS], ProtocolError> {
        self.validate()?;
        let mut words = [0; HEADER_WORDS];
        words[..16].copy_from_slice(&[
            MAGIC,
            VERSION,
            REGION_BYTES as u32,
            HEADER_WORDS as u32,
            SLOT_WORDS as u32,
            SLOT_COUNT as u32,
            MAX_BLOCK as u32,
            EVENT_CAPACITY as u32,
            PARAM_CAPACITY as u32,
            self.sample_rate,
            self.block as u32,
            self.native_latency as u32,
            u32::from(self.kind == Kind::Instrument),
            INPUT as u32,
            OUTPUT as u32,
            EVENTS as u32,
        ]);
        words[16..24].copy_from_slice(&self.identity.encode());
        words[24] = NOTES as u32;
        words[25] = PARAMETERS as u32;
        words[TIMELINE_EPOCH] = 1;
        Ok(words)
    }
    /// Startup only: exact v4 layout and initial epoch; never trust peer offsets.
    pub fn from_header(words: &[u32], mapped_bytes: usize) -> Result<Self, ProtocolError> {
        if words.len() != HEADER_WORDS || mapped_bytes != REGION_BYTES {
            return Err(ProtocolError::Layout);
        }
        let kind = match words[12] {
            0 => Kind::Effect,
            1 => Kind::Instrument,
            _ => return Err(ProtocolError::Configuration),
        };
        let config = Self {
            identity: Identity::decode(
                words[16..24]
                    .try_into()
                    .map_err(|_| ProtocolError::Layout)?,
            )?,
            sample_rate: words[9],
            block: words[10] as usize,
            native_latency: words[11] as usize,
            kind,
        }
        .validate()?;
        if words != config.header()? {
            return Err(ProtocolError::Layout);
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Layout,
    Identity,
    Configuration,
    Event,
    Transport,
    Parameter,
    Audio,
    Sequence,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let notice = windfall_core::notice!("pluginHost.protocol", detail = format!("{self:?}"));
        write!(formatter, "{notice}")
    }
}
impl std::error::Error for ProtocolError {}

pub fn encode_event(event: HostEvent, frames: usize) -> Result<[u32; EVENT_WORDS], ProtocolError> {
    if event.time() as usize >= frames || !event.valid() {
        return Err(ProtocolError::Event);
    }
    let mut words = [0; EVENT_WORDS];
    words[1] = event.time();
    match event {
        HostEvent::Midi { bytes, len, .. } => {
            words[0] = 8;
            words[2] = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]);
            words[3] = u32::from(len);
        }
        HostEvent::NoteOn {
            key,
            channel,
            velocity,
            ..
        }
        | HostEvent::NoteOff {
            key,
            channel,
            velocity,
            ..
        } => {
            words[0] = if matches!(event, HostEvent::NoteOn { .. }) {
                1
            } else {
                2
            };
            words[2] = u32::from(key);
            words[3] = u32::from(channel);
            words[4] = velocity.to_bits();
        }
        HostEvent::AllNotesOff { .. } => words[0] = 3,
        HostEvent::Param { id, value, .. } => {
            words[0] = 4;
            words[2] = id;
            words[4] = value.to_bits() as u32;
            words[5] = (value.to_bits() >> 32) as u32;
        }
        HostEvent::NoteOnInstance {
            id,
            key,
            channel,
            velocity,
            ..
        }
        | HostEvent::NoteOffInstance {
            id,
            key,
            channel,
            velocity,
            ..
        } => {
            words[0] = if matches!(event, HostEvent::NoteOnInstance { .. }) {
                5
            } else {
                6
            };
            words[2] = id;
            words[3] = u32::from(key);
            words[4] = u32::from(channel);
            words[5] = velocity.to_bits();
        }
        HostEvent::NoteExpression {
            id,
            key,
            channel,
            kind,
            value,
            ..
        } => {
            words[0] = 7;
            words[2] = id;
            words[3] = u32::from(key);
            words[4] = u32::from(channel);
            words[5] = kind as u32;
            words[6] = value.to_bits() as u32;
            words[7] = (value.to_bits() >> 32) as u32;
        }
    }
    Ok(words)
}

pub fn decode_event(words: [u32; EVENT_WORDS], frames: usize) -> Result<HostEvent, ProtocolError> {
    let event = match words[0] {
        1 | 2 if words[2] <= 127 && words[3] <= 15 && words[5..].iter().all(|word| *word == 0) => {
            let time = words[1];
            let key = words[2] as u8;
            let channel = words[3] as u8;
            let velocity = f32::from_bits(words[4]);
            if words[0] == 1 {
                HostEvent::NoteOn {
                    time,
                    key,
                    channel,
                    velocity,
                }
            } else {
                HostEvent::NoteOff {
                    time,
                    key,
                    channel,
                    velocity,
                }
            }
        }
        3 if words[2..].iter().all(|word| *word == 0) => HostEvent::AllNotesOff { time: words[1] },
        4 if words[3] == 0 && words[6..].iter().all(|word| *word == 0) => HostEvent::Param {
            time: words[1],
            id: words[2],
            value: f64::from_bits(pair(words[4], words[5])),
        },
        5 | 6 if words[3] <= 127 && words[4] <= 15 && words[6..].iter().all(|word| *word == 0) => {
            let time = words[1];
            let id = words[2];
            let key = words[3] as u8;
            let channel = words[4] as u8;
            let velocity = f32::from_bits(words[5]);
            if words[0] == 5 {
                HostEvent::NoteOnInstance {
                    time,
                    id,
                    key,
                    channel,
                    velocity,
                }
            } else {
                HostEvent::NoteOffInstance {
                    time,
                    id,
                    key,
                    channel,
                    velocity,
                }
            }
        }
        7 if words[3] <= 127 && words[4] <= 15 => HostEvent::NoteExpression {
            time: words[1],
            id: words[2],
            key: words[3] as u8,
            channel: words[4] as u8,
            kind: crate::NoteExpressionKind::from_raw(words[5]).ok_or(ProtocolError::Event)?,
            value: f64::from_bits(pair(words[6], words[7])),
        },
        8 if words[2] <= 0x00ff_ffff
            && words[3] <= 3
            && words[4..].iter().all(|word| *word == 0) =>
        {
            let packed = words[2].to_le_bytes();
            HostEvent::Midi {
                time: words[1],
                bytes: [packed[0], packed[1], packed[2]],
                len: words[3] as u8,
            }
        }
        _ => return Err(ProtocolError::Event),
    };
    encode_event(event, frames)?;
    Ok(event)
}

pub fn encode_transport(transport: Transport) -> Result<[u32; 14], ProtocolError> {
    if !transport.tempo_bpm.is_finite()
        || !(1.0..=1_000.0).contains(&transport.tempo_bpm)
        || !transport.position_beats.is_finite()
        || !transport.position_seconds.is_finite()
        || transport.position_beats.abs() > 1.0e9
        || transport.position_seconds.abs() > 1.0e9
        || !(1..=64).contains(&transport.numerator)
        || !(1..=64).contains(&transport.denominator)
        || !transport.denominator.is_power_of_two()
    {
        return Err(ProtocolError::Transport);
    }
    let mut words = [0; 14];
    words[0] = u32::from(transport.playing);
    words[1] = u32::from(transport.numerator);
    words[2] = u32::from(transport.denominator);
    for (index, value) in [
        transport.tempo_bpm,
        transport.position_beats,
        transport.position_seconds,
    ]
    .iter()
    .enumerate()
    {
        words[4 + index * 2] = value.to_bits() as u32;
        words[5 + index * 2] = (value.to_bits() >> 32) as u32;
    }
    if let Some(anchor) = transport.meter_anchor {
        if transport.bar_position().is_none() {
            return Err(ProtocolError::Transport);
        }
        words[3] = 1;
        words[10] = anchor.bar_origin_beats.to_bits() as u32;
        words[11] = (anchor.bar_origin_beats.to_bits() >> 32) as u32;
        words[12] = anchor.bar_origin_index;
    }
    Ok(words)
}

pub fn decode_transport(words: [u32; 14]) -> Result<Transport, ProtocolError> {
    if words[0] > 1
        || words[1] > u16::MAX as u32
        || words[2] > u16::MAX as u32
        || words[3] > 1
        || words[13] != 0
        || (words[3] == 0 && words[10..13].iter().any(|word| *word != 0))
    {
        return Err(ProtocolError::Transport);
    }
    let transport = Transport {
        playing: words[0] == 1,
        numerator: words[1] as u16,
        denominator: words[2] as u16,
        tempo_bpm: f64::from_bits(pair(words[4], words[5])),
        position_beats: f64::from_bits(pair(words[6], words[7])),
        position_seconds: f64::from_bits(pair(words[8], words[9])),
        meter_anchor: (words[3] == 1).then(|| crate::MeterAnchor {
            bar_origin_beats: f64::from_bits(pair(words[10], words[11])),
            bar_origin_index: words[12],
        }),
    };
    encode_transport(transport)?;
    Ok(transport)
}

#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    pub id: u32,
    pub value: f64,
}
impl Parameter {
    pub fn encode(self) -> Result<[u32; 3], ProtocolError> {
        if self.id == u32::MAX || !self.value.is_finite() {
            return Err(ProtocolError::Parameter);
        }
        Ok([
            self.id,
            self.value.to_bits() as u32,
            (self.value.to_bits() >> 32) as u32,
        ])
    }
    pub fn decode(words: [u32; 3]) -> Result<Self, ProtocolError> {
        let parameter = Self {
            id: words[0],
            value: f64::from_bits(pair(words[1], words[2])),
        };
        parameter.encode()?;
        Ok(parameter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            identity: Identity {
                session: 1,
                token: u64::MAX,
                revision: 7,
                binding: 99,
            },
            sample_rate: 48_000,
            block: 256,
            native_latency: 32,
            kind: Kind::Effect,
        }
    }
    #[test]
    fn every_header_word_and_mapping_length_is_checked() {
        let config = config();
        let header = config.header().unwrap();
        assert_eq!(Config::from_header(&header, REGION_BYTES), Ok(config));
        for index in 0..HEADER_WORDS {
            // Configuration/identity words legitimately vary; all layout/reserved words are exact.
            if (9..=12).contains(&index) || (16..24).contains(&index) {
                continue;
            }
            let mut corrupt = header;
            corrupt[index] ^= 1;
            assert!(
                Config::from_header(&corrupt, REGION_BYTES).is_err(),
                "word {index}"
            );
        }
        for length in [0, REGION_BYTES - 1, REGION_BYTES + 1, usize::MAX] {
            assert!(Config::from_header(&header, length).is_err());
        }
        assert!(Config::from_header(&header[..63], REGION_BYTES).is_err());
        assert_eq!(REGION_BYTES % 64, 0);
        assert_eq!(SLOT_WORDS * 4 % 64, 0);
    }
    #[test]
    fn earlier_mapping_versions_never_downgrade_the_v11_ramp_handover_contract() {
        let mut header = config().header().unwrap();
        assert_eq!(header[1], 11);
        assert_eq!(header[OWNER_COMPLETIONS], 0);
        assert_eq!(header[TIMELINE_EPOCH], 1);
        for version in 1..11 {
            header[1] = version;
            assert_eq!(
                Config::from_header(&header, REGION_BYTES),
                Err(ProtocolError::Layout)
            );
        }
    }
    #[test]
    fn event_roundtrip_preserves_native_precision_and_rejects_unchecked_bytes() {
        let events = [
            HostEvent::Midi {
                time: 1,
                bytes: [0xb9, 74, 99],
                len: 3,
            },
            HostEvent::Midi {
                time: 1,
                bytes: [0xc9, 11, 0],
                len: 2,
            },
            HostEvent::Midi {
                time: 1,
                bytes: [0xe9, 127, 127],
                len: 3,
            },
            HostEvent::NoteOn {
                time: 1,
                key: 127,
                channel: 15,
                velocity: 0.75,
            },
            HostEvent::NoteOff {
                time: 2,
                key: 0,
                channel: 0,
                velocity: 0.0,
            },
            HostEvent::AllNotesOff { time: 3 },
            HostEvent::Param {
                time: 4,
                id: 999,
                value: -123.123_456_789,
            },
        ];
        for event in events {
            let encoded = encode_event(event, 7).unwrap();
            assert_eq!(decode_event(encoded, 7), Ok(event));
            assert!(decode_event(encoded, event.time() as usize).is_err());
        }
        for words in [
            [99, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 128, 0, 0, 0, 0, 0],
            [1, 0, 0, 16, 0, 0, 0, 0],
            [1, 0, 0, 0, f32::NAN.to_bits(), 0, 0, 0],
            [3, 0, 1, 0, 0, 0, 0, 0],
            [4, 0, u32::MAX, 0, 0, 0, 0, 0],
            [4, 0, 1, 0, 0, (f64::INFINITY.to_bits() >> 32) as u32, 0, 0],
        ] {
            assert!(decode_event(words, 64).is_err());
        }
    }
    #[test]
    fn v3_transport_anchor_roundtrip_and_metadata_ranges_do_not_overlap() {
        assert_eq!(TRANSPORT + 14, EPOCH);
        assert_eq!(EPOCH + 2, REPLY_IDENTITY);
        let ranges = [
            (STATE, 1),
            (IDENTITY, 8),
            (SEQUENCE, 2),
            (FRAMES, 1),
            (EVENT_COUNT, 1),
            (PARAM_COUNT, 1),
            (FLAGS, 1),
            (NOTE_CHANNEL, 1),
            (TRANSPORT, 14),
            (EPOCH, 2),
            (REPLY_IDENTITY, 8),
            (REPLY_SEQUENCE, 2),
            (REPLY_FRAMES, 1),
            (REPLY_STATUS, 1),
            (REPLY_EPOCH, 2),
            (PROCESSED_GENERATION, 2),
            (CONTROL_START, 2),
            (CONTROL_END, 2),
            (NATIVE_DROPS, 1),
            (SIDECHAIN_PORT, 1),
            (INSTANCE_COUNT, 1),
            (OUTPUT_COUNT, 1),
            (PROJECT_LAW, 1),
            (PROJECT_LAW_COUNT, 1),
            (HANDED_COUNT, 1),
            (DEPART, 1),
            (DEPART_AT, 1),
            (REPORT_COUNT, 1),
            (REPORT_FLAGS, 1),
        ];
        let mut used = [false; META_WORDS];
        for (start, count) in ranges {
            for word in &mut used[start..start + count] {
                assert!(!*word);
                *word = true;
            }
        }
        let origin = 4001.0 / 960.0;
        let transport = Transport {
            playing: true,
            tempo_bpm: 137.0,
            position_beats: origin + 3.75,
            position_seconds: 9.25,
            numerator: 7,
            denominator: 8,
            meter_anchor: Some(crate::MeterAnchor {
                bar_origin_beats: origin,
                bar_origin_index: 2,
            }),
        };
        let words = encode_transport(transport).unwrap();
        assert_eq!(words.len(), 14);
        assert_eq!(words[3], 1);
        assert_eq!(words[12], 2);
        assert_eq!(words[13], 0);
        assert_eq!(decode_transport(words), Ok(transport));
        assert_eq!(
            decode_transport(words).unwrap().bar_position(),
            Some((origin + 3.5, 3))
        );
        for anchor in [
            crate::MeterAnchor {
                bar_origin_beats: f64::NAN,
                bar_origin_index: 2,
            },
            crate::MeterAnchor {
                bar_origin_beats: -1.0,
                bar_origin_index: 2,
            },
            crate::MeterAnchor {
                bar_origin_beats: transport.position_beats + 1.0,
                bar_origin_index: 2,
            },
            crate::MeterAnchor {
                bar_origin_beats: origin,
                bar_origin_index: i32::MAX as u32,
            },
        ] {
            assert!(
                encode_transport(Transport {
                    meter_anchor: Some(anchor),
                    ..transport
                })
                .is_err()
            );
        }
        for index in [3, 10, 11, 12, 13] {
            let mut none = encode_transport(Transport::default()).unwrap();
            none[index] = 2;
            assert!(decode_transport(none).is_err());
        }
        let mut malformed = words;
        malformed[13] = 1;
        assert!(decode_transport(malformed).is_err());
        let legacy = Transport {
            position_beats: -3.5,
            position_seconds: -2.0,
            ..Transport::default()
        };
        assert_eq!(
            decode_transport(encode_transport(legacy).unwrap()),
            Ok(legacy)
        );
    }
    #[test]
    fn transport_and_latency_are_bounded() {
        let mut transport = Transport {
            playing: true,
            position_beats: -3.5,
            ..Transport::default()
        };
        assert_eq!(
            decode_transport(encode_transport(transport).unwrap()),
            Ok(transport)
        );
        transport.denominator = 3;
        assert!(encode_transport(transport).is_err());
        transport.denominator = 4;
        transport.position_seconds = f64::INFINITY;
        assert!(encode_transport(transport).is_err());
        for block in [0, MAX_BLOCK + 1, usize::MAX] {
            assert!(Config { block, ..config() }.validate().is_err());
        }
        assert!(
            Config {
                native_latency: 48_000,
                ..config()
            }
            .validate()
            .is_err()
        );
        assert!(
            Config {
                native_latency: usize::MAX,
                ..config()
            }
            .validate()
            .is_err()
        );
        assert_eq!(config().latency(), 544);
    }
}
