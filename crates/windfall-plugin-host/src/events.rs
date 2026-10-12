//! What crosses between the host and a plugin with each block besides
//! audio: notes and parameter changes going in, the plugin's own changes
//! coming out, and where the song is.
//!
//! These are plain values with no pointers in them, the same for every
//! plugin format. That is what lets a plugin move into another process
//! later: they can be copied into shared memory as they are.

pub const MAX_NOTE_INSTANCES: usize = 1024;
/// Native ids below this value belong to the legacy channel/key API.
pub const FIRST_NOTE_INSTANCE_ID: u32 = 16 * 128;

/// Format-independent values. Tuning is a relative semitone offset; other
/// controls are normalized, except volume which uses CLAP's linear 0..4 range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NoteExpressionKind {
    Volume,
    Pan,
    Tuning,
    Vibrato,
    Expression,
    Brightness,
    Pressure,
}
impl NoteExpressionKind {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Volume),
            1 => Some(Self::Pan),
            2 => Some(Self::Tuning),
            3 => Some(Self::Vibrato),
            4 => Some(Self::Expression),
            5 => Some(Self::Brightness),
            6 => Some(Self::Pressure),
            _ => None,
        }
    }
    pub const fn mask(self) -> u32 {
        1 << self as u32
    }
    pub fn valid(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::Tuning => (-120.0..=120.0).contains(&value),
                Self::Volume => (0.0..=4.0).contains(&value),
                _ => (0.0..=1.0).contains(&value),
            }
    }
}

/// Something the host tells a plugin at a frame of the next block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostEvent {
    /// Channel-voice MIDI other than notes; notes retain the ownership API.
    Midi { time: u32, bytes: [u8; 3], len: u8 },
    /// Start a note. `velocity` runs from 0 to 1.
    NoteOn {
        time: u32,
        key: u8,
        channel: u8,
        velocity: f32,
    },
    /// Let go of a note, which starts its release.
    NoteOff {
        time: u32,
        key: u8,
        channel: u8,
        velocity: f32,
    },
    /// Stop every note at once, without releases.
    AllNotesOff { time: u32 },
    /// Set a parameter, by the plugin's own id and in its own units.
    Param { time: u32, id: u32, value: f64 },
    /// One sounding occurrence, independent of other notes on its key.
    NoteOnInstance {
        time: u32,
        id: u32,
        key: u8,
        channel: u8,
        velocity: f32,
    },
    NoteOffInstance {
        time: u32,
        id: u32,
        key: u8,
        channel: u8,
        velocity: f32,
    },
    NoteExpression {
        time: u32,
        id: u32,
        key: u8,
        channel: u8,
        kind: NoteExpressionKind,
        value: f64,
    },
}

impl HostEvent {
    pub(crate) fn valid(&self) -> bool {
        match *self {
            Self::Midi { bytes, len, .. } => {
                let expected = match bytes[0] & 0xf0 {
                    0xa0 | 0xb0 | 0xe0 => 3,
                    0xc0 | 0xd0 => 2,
                    _ => 0,
                };
                len == expected
                    && expected != 0
                    && bytes[1..usize::from(len.min(3))]
                        .iter()
                        .all(|byte| *byte < 128)
            }
            Self::NoteOnInstance {
                id,
                key,
                channel,
                velocity,
                ..
            }
            | Self::NoteOffInstance {
                id,
                key,
                channel,
                velocity,
                ..
            } => {
                (FIRST_NOTE_INSTANCE_ID..=i32::MAX as u32).contains(&id)
                    && key <= 127
                    && channel <= 15
                    && velocity.is_finite()
                    && (0.0..=1.0).contains(&velocity)
            }
            Self::NoteExpression {
                id,
                key,
                channel,
                kind,
                value,
                ..
            } => {
                (FIRST_NOTE_INSTANCE_ID..=i32::MAX as u32).contains(&id)
                    && key <= 127
                    && channel <= 15
                    && kind.valid(value)
            }
            Self::NoteOn {
                key,
                channel,
                velocity,
                ..
            }
            | Self::NoteOff {
                key,
                channel,
                velocity,
                ..
            } => {
                key <= 127
                    && channel <= 15
                    && velocity.is_finite()
                    && (0.0..=1.0).contains(&velocity)
            }
            Self::Param { id, value, .. } => id != u32::MAX && value.is_finite(),
            Self::AllNotesOff { .. } => true,
        }
    }
    /// The frame of the block the event takes effect on.
    pub const fn time(&self) -> u32 {
        match *self {
            Self::Midi { time, .. }
            | Self::NoteOn { time, .. }
            | Self::NoteOff { time, .. }
            | Self::AllNotesOff { time }
            | Self::Param { time, .. } => time,
            Self::NoteOnInstance { time, .. }
            | Self::NoteOffInstance { time, .. }
            | Self::NoteExpression { time, .. } => time,
        }
    }

    pub(crate) const fn set_time(&mut self, new: u32) {
        match self {
            Self::Midi { time, .. }
            | Self::NoteOn { time, .. }
            | Self::NoteOff { time, .. }
            | Self::AllNotesOff { time }
            | Self::Param { time, .. } => *time = new,
            Self::NoteOnInstance { time, .. }
            | Self::NoteOffInstance { time, .. }
            | Self::NoteExpression { time, .. } => *time = new,
        }
    }
}

/// Something a plugin tells the host while it processes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginEvent {
    /// The user is about to move this parameter in the plugin's editor.
    /// Everything up to the matching [`PluginEvent::GestureEnd`] is one edit.
    GestureBegin { id: u32 },
    /// The plugin changed a parameter itself, in its own units.
    ParamValue { id: u32, value: f64 },
    /// The user let go of the parameter.
    GestureEnd { id: u32 },
    /// A note has finished sounding, release included.
    NoteEnd { key: u8, channel: u8 },
}

/// A song meter segment's first downbeat and zero-based bar index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterAnchor {
    pub bar_origin_beats: f64,
    pub bar_origin_index: u32,
}

/// Where the song is at the first frame of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    pub playing: bool,
    pub tempo_bpm: f64,
    /// Position in quarter notes from the start of the song.
    pub position_beats: f64,
    /// Position in seconds from the start of the song.
    pub position_seconds: f64,
    /// The time signature, as in 4/4.
    pub numerator: u16,
    pub denominator: u16,
    /// `None` retains the scalar signature's origin at beat zero.
    pub meter_anchor: Option<MeterAnchor>,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            tempo_bpm: 120.0,
            position_beats: 0.0,
            position_seconds: 0.0,
            numerator: 4,
            denominator: 4,
            meter_anchor: None,
        }
    }
}

impl Transport {
    /// Derives the native bar downbeat and zero-based index without changing
    /// the absolute beat/seconds clock. Invalid anchors or native index
    /// overflow refuse transport instead of silently reverting to beat zero.
    pub fn bar_position(&self) -> Option<(f64, i32)> {
        if !self.position_beats.is_finite() || self.numerator == 0 || self.denominator == 0 {
            return None;
        }
        let (origin, index) = match self.meter_anchor {
            Some(anchor)
                if anchor.bar_origin_beats.is_finite()
                    && anchor.bar_origin_beats >= 0.0
                    && self.position_beats >= anchor.bar_origin_beats
                    && anchor.bar_origin_index <= i32::MAX as u32 =>
            {
                (anchor.bar_origin_beats, f64::from(anchor.bar_origin_index))
            }
            Some(_) => return None,
            None => (0.0, 0.0),
        };
        let width = f64::from(self.numerator) * 4.0 / f64::from(self.denominator);
        let mut relative_bar = ((self.position_beats - origin) / width).floor();
        let bar_start = if self.meter_anchor.is_some() {
            if !relative_bar.is_finite()
                || !(0.0..=f64::from(i32::MAX) + 1.0).contains(&relative_bar)
            {
                return None;
            }
            // Song anchors originate on the 960-PPQ grid. Reconstruct that
            // grid only when division reproduces the supplied anchor exactly.
            // Comparing absolute downbeats avoids cancellation in (pos-origin),
            // without an epsilon that would promote a genuinely earlier frame.
            const PPQ: f64 = 960.0;
            const MAX_EXACT_TICK: f64 = (1_u64 << 53) as f64;
            let origin_tick = (origin * PPQ).round();
            let width_ticks = width * PPQ;
            let tick_grid = origin_tick <= MAX_EXACT_TICK
                && origin_tick / PPQ == origin
                && width_ticks >= 1.0
                && width_ticks == width_ticks.round();
            let downbeat = |bar: f64| {
                if tick_grid {
                    let tick = origin_tick + bar * width_ticks;
                    (0.0..=MAX_EXACT_TICK).contains(&tick).then_some(tick / PPQ)
                } else {
                    Some(bar.mul_add(width, origin))
                }
            };
            if self.position_beats < downbeat(relative_bar)? {
                relative_bar -= 1.0;
            }
            if self.position_beats >= downbeat(relative_bar + 1.0)? {
                relative_bar += 1.0;
            }
            let start = downbeat(relative_bar)?;
            let end = downbeat(relative_bar + 1.0)?;
            // At most one correction in either direction; unrepresentable
            // boundaries refuse rather than spin or manufacture a valid bar.
            if relative_bar < 0.0 || !(start <= self.position_beats && self.position_beats < end) {
                return None;
            }
            start
        } else {
            origin + relative_bar * width
        };
        let bar = index + relative_bar;
        if !bar.is_finite() || bar < f64::from(i32::MIN) || bar > f64::from(i32::MAX) {
            return None;
        }
        Some((bar_start, bar as i32))
    }

    /// Moves the position on by `frames` at `sample_rate`, if the song is
    /// playing. The processor does this after every block, so a host that
    /// sets the position once gets a running clock.
    pub(crate) fn advance(&mut self, frames: usize, sample_rate: f64) {
        if self.playing && sample_rate > 0.0 {
            let seconds = frames as f64 / sample_rate;
            self.position_seconds += seconds;
            self.position_beats += seconds * self.tempo_bpm / 60.0;
        }
    }
}

#[cfg(test)]
mod meter_tests {
    use super::*;

    #[test]
    fn tick_grid_boundaries_do_not_round_preceding_positions_forward() {
        for origin_tick in [1_u32, 485, 4_001, 1_000_001, u32::MAX - 20_000] {
            for (numerator, denominator, width_ticks) in [
                (1, 16, 240),
                (7, 16, 1_680),
                (7, 8, 3_360),
                (4, 4, 3_840),
                (3, 2, 5_760),
            ] {
                for relative_bar in [1_u32, 2, 3] {
                    let tick = origin_tick + relative_bar * width_ticks;
                    let boundary = f64::from(tick) / 960.0;
                    let preceding = f64::from(tick - width_ticks) / 960.0;
                    let transport = Transport {
                        position_beats: boundary,
                        numerator,
                        denominator,
                        meter_anchor: Some(MeterAnchor {
                            bar_origin_beats: f64::from(origin_tick) / 960.0,
                            bar_origin_index: 2,
                        }),
                        ..Transport::default()
                    };
                    assert_eq!(
                        transport.bar_position(),
                        Some((boundary, 2 + relative_bar as i32))
                    );
                    assert_eq!(
                        Transport {
                            position_beats: boundary.next_down(),
                            ..transport
                        }
                        .bar_position(),
                        Some((preceding, 1 + relative_bar as i32))
                    );
                    assert_eq!(
                        Transport {
                            position_beats: boundary.next_up(),
                            ..transport
                        }
                        .bar_position(),
                        Some((boundary, 2 + relative_bar as i32))
                    );
                }
            }
        }
    }

    #[test]
    fn non_grid_anchors_and_native_index_bounds_remain_checked() {
        let anchor = MeterAnchor {
            bar_origin_beats: 0.123,
            bar_origin_index: 1,
        };
        let boundary = 3.5_f64 + anchor.bar_origin_beats;
        let mut transport = Transport {
            position_beats: boundary.next_down(),
            numerator: 7,
            denominator: 8,
            meter_anchor: Some(anchor),
            ..Transport::default()
        };
        assert_eq!(transport.bar_position(), Some((anchor.bar_origin_beats, 1)));
        transport.position_beats = boundary;
        assert_eq!(transport.bar_position(), Some((boundary, 2)));
        transport.meter_anchor = Some(MeterAnchor {
            bar_origin_beats: 485.0 / 960.0,
            bar_origin_index: i32::MAX as u32,
        });
        transport.position_beats = (3845.0_f64 / 960.0).next_down();
        assert_eq!(transport.bar_position(), Some((485.0 / 960.0, i32::MAX)));
        transport.position_beats = 3845.0 / 960.0;
        assert_eq!(transport.bar_position(), None);
        transport.position_beats = (3845.0_f64 / 960.0).next_up();
        assert_eq!(transport.bar_position(), None);
        transport.meter_anchor = Some(MeterAnchor {
            bar_origin_beats: f64::MAX,
            bar_origin_index: 0,
        });
        transport.position_beats = f64::MAX;
        assert_eq!(transport.bar_position(), None);
    }

    #[test]
    fn checked_bar_bounds_and_advance_preserve_the_segment_anchor() {
        let anchor = MeterAnchor {
            bar_origin_beats: 4.0,
            bar_origin_index: 1,
        };
        let mut transport = Transport {
            playing: true,
            position_beats: 4.0,
            position_seconds: 2.0,
            numerator: 7,
            denominator: 8,
            meter_anchor: Some(anchor),
            ..Transport::default()
        };
        assert_eq!(transport.bar_position(), Some((4.0, 1)));
        transport.advance(84_000, 48_000.0);
        assert_eq!(transport.position_beats, 7.5);
        assert_eq!(transport.position_seconds, 3.75);
        assert_eq!(transport.meter_anchor, Some(anchor));
        assert_eq!(transport.bar_position(), Some((7.5, 2)));
        transport.playing = false;
        let stopped = transport;
        transport.advance(48_000, 48_000.0);
        assert_eq!(transport, stopped);

        transport.meter_anchor = Some(MeterAnchor {
            bar_origin_index: i32::MAX as u32,
            ..anchor
        });
        transport.position_beats = 4.0;
        assert_eq!(transport.bar_position(), Some((4.0, i32::MAX)));
        transport.position_beats = 7.5;
        assert_eq!(transport.bar_position(), None);
        transport.meter_anchor = None;
        transport.position_beats = -0.25;
        assert_eq!(transport.bar_position(), Some((-3.5, -1)));
        for position in [f64::NAN, f64::INFINITY, f64::MAX] {
            transport.position_beats = position;
            assert_eq!(transport.bar_position(), None);
        }
        transport.position_beats = 0.0;
        transport.denominator = 0;
        assert_eq!(transport.bar_position(), None);
        transport.denominator = 4;
        transport.numerator = 0;
        assert_eq!(transport.bar_position(), None);
    }
}
