//! The MIDI messages Windfall reads from hardware, as fixed-size values.

/// One channel or realtime message. Channels count from 0 here, as on the
/// wire. System exclusive, time code and active sensing are not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMessage {
    /// A key going down. The velocity is 1 to 127.
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    /// A key coming up, however the device said so.
    NoteOff {
        channel: u8,
        key: u8,
    },
    Controller {
        channel: u8,
        number: u8,
        value: u8,
    },
    /// The pitch wheel, 0 to 16383 with 8192 the center.
    PitchBend {
        channel: u8,
        value: u16,
    },
    ChannelPressure {
        channel: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    /// One of the twenty-four clock pulses of a quarter note.
    Clock,
    Start,
    Continue,
    Stop,
    PolyPressure {channel:u8,key:u8,value:u8},
}

impl MidiMessage {
    /// Reads one complete message. Anything else, running status included,
    /// is `None`: the backends hand over whole messages.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let (&status, data) = bytes.split_first()?;
        if data.iter().any(|byte| *byte > 127) {
            return None;
        }
        match (status, data) {
            (0xf8, []) => return Some(Self::Clock),
            (0xfa, []) => return Some(Self::Start),
            (0xfb, []) => return Some(Self::Continue),
            (0xfc, []) => return Some(Self::Stop),
            _ => {}
        }
        let channel = status & 15;
        match (status >> 4, data) {
            (8, &[key, _]) | (9, &[key, 0]) => Some(Self::NoteOff { channel, key }),
            (9, &[key, velocity]) => Some(Self::NoteOn {
                channel,
                key,
                velocity,
            }),
            (10, &[key,value])=>Some(Self::PolyPressure {channel,key,value}),
            (11, &[number, value]) => Some(Self::Controller {
                channel,
                number,
                value,
            }),
            (12, &[program]) => Some(Self::ProgramChange { channel, program }),
            (13, &[value]) => Some(Self::ChannelPressure { channel, value }),
            (14, &[low, high]) => Some(Self::PitchBend {
                channel,
                value: (u16::from(high) << 7) | u16::from(low),
            }),
            _ => None,
        }
    }

    /// The channel of a channel message, counted from 0.
    pub fn channel(self) -> Option<u8> {
        match self {
            Self::NoteOn { channel, .. }
            | Self::NoteOff { channel, .. }
            | Self::Controller { channel, .. }
            | Self::PitchBend { channel, .. }
            | Self::ChannelPressure { channel, .. }
            | Self::PolyPressure {channel,..}
            | Self::ProgramChange { channel, .. } => Some(channel),
            Self::Clock | Self::Start | Self::Continue | Self::Stop => None,
        }
    }

    /// The message as bytes for the wire, and how many of them count.
    pub fn bytes(self) -> ([u8; 3], usize) {
        match self {
            Self::NoteOn {
                channel,
                key,
                velocity,
            } => ([0x90 | (channel & 15), key & 127, velocity & 127], 3),
            Self::NoteOff { channel, key } => ([0x80 | (channel & 15), key & 127, 0], 3),
            Self::Controller {
                channel,
                number,
                value,
            } => ([0xb0 | (channel & 15), number & 127, value & 127], 3),
            Self::PitchBend { channel, value } => (
                [
                    0xe0 | (channel & 15),
                    (value & 127) as u8,
                    ((value >> 7) & 127) as u8,
                ],
                3,
            ),
            Self::ChannelPressure { channel, value } => {
                ([0xd0 | (channel & 15), value & 127, 0], 2)
            }
            Self::ProgramChange { channel, program } => {
                ([0xc0 | (channel & 15), program & 127, 0], 2)
            }
            Self::PolyPressure{channel,key,value}=>([0xa0|(channel&15),key&127,value&127],3),
            Self::Clock => ([0xf8, 0, 0], 1),
            Self::Start => ([0xfa, 0, 0], 1),
            Self::Continue => ([0xfb, 0, 0], 1),
            Self::Stop => ([0xfc, 0, 0], 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_message_parses_and_writes_back_the_same_bytes() {
        for bytes in [
            &[0x93, 60, 100][..],
            &[0x83, 60, 0],
            &[0xb5, 74, 127],
            &[0xe0, 0x7f, 0x7f],
            &[0xe9, 0, 0x40],
            &[0xd2, 99],
            &[0xa2, 60, 100],
            &[0xcf, 5],
            &[0xf8],
            &[0xfa],
            &[0xfb],
            &[0xfc],
        ] {
            let message = MidiMessage::parse(bytes).unwrap();
            let (written, len) = message.bytes();
            assert_eq!(&written[..len], bytes, "{message:?}");
        }
        assert_eq!(
            MidiMessage::parse(&[0xe1, 0, 0x40]),
            Some(MidiMessage::PitchBend {
                channel: 1,
                value: 8192
            })
        );
        assert_eq!(
            MidiMessage::parse(&[0xe1, 0x7f, 0x7f]),
            Some(MidiMessage::PitchBend {
                channel: 1,
                value: 16_383
            })
        );
    }

    #[test]
    fn a_note_on_with_no_velocity_and_a_note_off_with_any_are_both_a_release() {
        let off = Some(MidiMessage::NoteOff {
            channel: 4,
            key: 61,
        });
        assert_eq!(MidiMessage::parse(&[0x94, 61, 0]), off);
        assert_eq!(MidiMessage::parse(&[0x84, 61, 77]), off);
        assert_eq!(MidiMessage::parse(&[0x84, 61, 0]), off);
    }

    #[test]
    fn incomplete_oversized_and_unread_messages_are_refused() {
        for bytes in [
            &[][..],
            &[0x90],
            &[0x90, 60],
            &[0x90, 60, 100, 1],
            &[0x90, 128, 5],
            &[0xb0, 1, 128],
            &[0xc0],
            &[0xc0, 1, 2],
            &[0xf0, 1, 2, 0xf7],
            &[0xf1, 0],
            &[0xf2, 0, 0],
            &[0xfe],
            &[0xff],
            &[0xf8, 0],
            &[60, 100],
        ] {
            assert_eq!(MidiMessage::parse(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn only_channel_messages_have_a_channel() {
        assert_eq!(
            MidiMessage::parse(&[0xbf, 1, 2]).unwrap().channel(),
            Some(15)
        );
        assert_eq!(MidiMessage::Clock.channel(), None);
        assert_eq!(MidiMessage::Stop.channel(), None);
    }
}
