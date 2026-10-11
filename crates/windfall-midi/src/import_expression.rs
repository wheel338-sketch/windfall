//! Reading a part's pitch bend and controllers as the expression of its
//! notes.
//!
//! A project note has one fine pitch and one value for each modulation, so
//! what a note takes from its channel is what the channel says when the
//! note starts: the last pitch bend, controller 74 and modulation wheel at
//! or before that tick. A message that moves one of them while a note is
//! sounding has no place in the note and is counted as left behind. A
//! message that arrives while the channel is silent changes nothing that is
//! heard and is not counted.
//!
//! The bend is scaled by the bend range the channel has at that tick:
//! registered parameter 0 where the file sets it, else two semitones, or 48
//! on a member channel of an MPE zone.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::expression::{
    CC_DATA_ENTRY, CC_DATA_ENTRY_FINE, CC_MODULATION, CC_RPN_COARSE, CC_RPN_FINE, CC_TIMBRE,
    DEVICE_BEND_RANGE, MAX_BEND_RANGE, MPE_BEND_RANGE, RPN_BEND_RANGE, RPN_MPE_CONFIGURATION,
    semitones_from_bend,
};
use crate::song::{ControlKind, ControlPoint, MidiTrack, PITCH_BEND_CENTER};

/// What one note takes from its channel, in MIDI's own values so that two
/// patterns can be told apart exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NoteValues {
    pub bend: u16,
    /// The bend range the bend is scaled by, in semitones.
    pub range: u8,
    /// Controller 74.
    pub x: u8,
    /// The modulation wheel, or channel pressure in an MPE zone.
    pub y: u8,
}

impl NoteValues {
    pub const NEUTRAL: Self = Self {
        bend: PITCH_BEND_CENTER,
        range: DEVICE_BEND_RANGE,
        x: 64,
        y: 0,
    };

    /// The fine pitch in cents, and whether it had to be brought into the
    /// octave either way that a note can hold.
    pub fn fine_pitch_cents(&self) -> (f32, bool) {
        let cents = semitones_from_bend(self.bend, self.range) * 100.0;
        (cents.clamp(-1200.0, 1200.0) as f32, cents.abs() > 1200.0)
    }
}

/// What a part's notes take from its channel, and how many messages that
/// accounts for.
#[derive(Debug, Default)]
pub(crate) struct PartExpression {
    /// One for each note that was asked about, in the same order.
    pub notes: Vec<NoteValues>,
    /// Pitch bend messages that are not left behind.
    pub bends_kept: usize,
    /// Controller messages that are not left behind: controller 74, the
    /// modulation wheel and the bend range.
    pub controllers_kept: usize,
    /// Channel pressure messages that are not left behind.
    pub pressure_kept: usize,
    /// Notes that take a fine pitch or a modulation from the channel.
    pub expressive: usize,
    /// Notes whose bend reaches further than an octave.
    pub clamped: usize,
}

/// How many member channels the lower MPE zone of a song has: what the MPE
/// configuration message on channel 1 last says, or 0 without one.
pub(crate) fn mpe_members(tracks: &[MidiTrack]) -> u8 {
    let mut members = 0;
    for track in tracks {
        if let Some(&(_, value)) = parameters(track, 0)
            .get(&RPN_MPE_CONFIGURATION)
            .and_then(|values| values.last())
        {
            members = value.min(15);
        }
    }
    members
}

/// How many controller messages on channel 1 of a track set up an MPE
/// zone, which an import of that zone has put to use.
pub(crate) fn mpe_setup_messages(track: &MidiTrack) -> usize {
    if !parameters(track, 0).contains_key(&RPN_MPE_CONFIGURATION) {
        return 0;
    }
    [CC_RPN_COARSE, CC_RPN_FINE, CC_DATA_ENTRY]
        .into_iter()
        .filter_map(|controller| track.control(0, ControlKind::Controller(controller)))
        .map(|curve| curve.points.len())
        .sum()
}

/// Reads what the notes of `channel` in `track` take from it. `notes` is
/// each note's start and end; `member` says that the channel is a member
/// of an MPE zone.
pub(crate) fn read(
    track: &MidiTrack,
    channel: u8,
    notes: &[(u32, u32)],
    member: bool,
) -> PartExpression {
    let points = |kind| {
        track
            .control(channel, kind)
            .map_or(&[][..], |curve| curve.points.as_slice())
    };
    let bends = points(ControlKind::PitchBend);
    let xs = points(ControlKind::Controller(CC_TIMBRE));
    // An MPE controller says its third dimension with pressure. A file
    // that has both on one member channel keeps the wheel.
    let wheel = points(ControlKind::Controller(CC_MODULATION));
    let pressure = points(ControlKind::ChannelPressure);
    let by_pressure = member && wheel.is_empty();
    let ys = if by_pressure { pressure } else { wheel };

    let parameters = parameters(track, channel);
    let ranges = parameters.get(&RPN_BEND_RANGE);
    let range_at = |tick: u32| {
        let set = ranges.and_then(|ranges| {
            let after = ranges.partition_point(|&(at, _)| at <= tick);
            after.checked_sub(1).map(|index| ranges[index].1)
        });
        let default = if member {
            MPE_BEND_RANGE
        } else {
            DEVICE_BEND_RANGE
        };
        set.unwrap_or(default).clamp(1, MAX_BEND_RANGE)
    };

    let mut part = PartExpression::default();
    for &(start, _) in notes {
        let values = NoteValues {
            bend: at(bends, start).unwrap_or(PITCH_BEND_CENTER),
            range: range_at(start),
            x: at(xs, start).map_or(64, |value| value.min(127) as u8),
            y: at(ys, start).map_or(0, |value| value.min(127) as u8),
        };
        let bent = values.bend != PITCH_BEND_CENTER;
        part.expressive += usize::from(bent || values.x != 64 || values.y != 0);
        part.clamped += usize::from(values.fine_pitch_cents().1);
        // A note that bends nothing does not need its channel's range to
        // tell it from a note of another pattern.
        part.notes.push(if bent {
            values
        } else {
            NoteValues {
                range: DEVICE_BEND_RANGE,
                ..values
            }
        });
    }

    part.bends_kept = bends.len() - moved(bends, notes);
    part.controllers_kept = xs.len() - moved(xs, notes);
    if by_pressure {
        part.pressure_kept = pressure.len() - moved(pressure, notes);
    } else {
        part.controllers_kept += wheel.len() - moved(wheel, notes);
    }
    if ranges.is_some() {
        // The messages that set the bend range were put to use.
        let setup = [
            CC_RPN_COARSE,
            CC_RPN_FINE,
            CC_DATA_ENTRY,
            CC_DATA_ENTRY_FINE,
        ];
        part.controllers_kept += setup
            .into_iter()
            .map(|controller| points(ControlKind::Controller(controller)).len())
            .sum::<usize>();
    }
    part
}

/// The value the last of `points` at or before `tick` sets.
fn at(points: &[ControlPoint], tick: u32) -> Option<u16> {
    let after = points.partition_point(|point| point.tick <= tick);
    after.checked_sub(1).map(|index| points[index].value)
}

/// How many of `points` move a value while a note is sounding: they lie
/// strictly inside a note and say something other than what that note
/// started with.
fn moved(points: &[ControlPoint], notes: &[(u32, u32)]) -> usize {
    if points.is_empty() || notes.is_empty() {
        return 0;
    }
    let mut starts: Vec<(u32, u32, u16)> = notes
        .iter()
        .map(|&(start, end)| (start, end, at(points, start).unwrap_or(u16::MAX)))
        .collect();
    starts.sort_unstable();
    let mut next = 0;
    // The notes sounding at the point in hand: when each ends, and how
    // many of them started with each value.
    let mut ends: BinaryHeap<Reverse<(u32, u16)>> = BinaryHeap::new();
    let mut sounding: BTreeMap<u16, usize> = BTreeMap::new();
    let mut total = 0_usize;
    let mut count = 0;
    for point in points {
        while let Some(&(start, end, value)) = starts.get(next) {
            if start >= point.tick {
                break;
            }
            next += 1;
            ends.push(Reverse((end, value)));
            *sounding.entry(value).or_default() += 1;
            total += 1;
        }
        while let Some(&Reverse((end, value))) = ends.peek() {
            if end > point.tick {
                break;
            }
            ends.pop();
            total -= 1;
            if let Some(left) = sounding.get_mut(&value) {
                *left -= 1;
            }
        }
        let same = sounding.get(&point.value).copied().unwrap_or(0);
        count += usize::from(total > same);
    }
    count
}

/// The registered parameters a channel of a track sets: for each parameter
/// number below 128, the ticks it is given a value on and the values.
///
/// A [`MidiTrack`] keeps each controller's messages in order, but not the
/// order of two controllers on one tick. A value is therefore given to the
/// parameter chosen on its own tick, or else the one chosen last before it.
fn parameters(track: &MidiTrack, channel: u8) -> BTreeMap<u8, Vec<(u32, u8)>> {
    let points = |controller| {
        track
            .control(channel, ControlKind::Controller(controller))
            .map_or(&[][..], |curve| curve.points.as_slice())
    };
    let (coarse, fine, data) = (
        points(CC_RPN_COARSE),
        points(CC_RPN_FINE),
        points(CC_DATA_ENTRY),
    );
    let mut set: BTreeMap<u8, Vec<(u32, u8)>> = BTreeMap::new();
    // What a controller chooses for a value on `tick`: the first it says on
    // that tick, which is what came before the value, else the last before.
    let chosen = |points: &[ControlPoint], tick: u32| {
        let from = points.partition_point(|point| point.tick < tick);
        match points.get(from) {
            Some(point) if point.tick == tick => Some(point.value),
            _ => from.checked_sub(1).map(|index| points[index].value),
        }
    };
    for point in data {
        if chosen(coarse, point.tick) != Some(0) {
            continue;
        }
        let Some(parameter) = chosen(fine, point.tick).filter(|&number| number < 127) else {
            continue;
        };
        let values = set.entry(parameter as u8).or_default();
        values.push((point.tick, point.value.min(127) as u8));
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::ControlCurve;

    fn points(values: &[(u32, u16)]) -> Vec<ControlPoint> {
        values
            .iter()
            .map(|&(tick, value)| ControlPoint { tick, value })
            .collect()
    }

    #[test]
    fn a_message_is_only_left_behind_when_it_moves_a_sounding_note() {
        let bends = points(&[(0, 9000), (50, 9000), (60, 9100), (100, 8192), (150, 7000)]);
        // One note from 0 to 100: the point at 60 moves it, and the points
        // at its start, at its end and after it do not.
        assert_eq!(moved(&bends, &[(0, 100)]), 1);
        // A second note that is still sounding hears the points at 100 and
        // 150 as changes too.
        assert_eq!(moved(&bends, &[(0, 100), (55, 200)]), 3);
        // Two notes that started with different values: a point that takes
        // the channel back to one of them still moves the other.
        let back = points(&[(0, 9000), (40, 8192), (60, 9000)]);
        assert_eq!(moved(&back, &[(0, 100), (50, 100)]), 2);
        assert_eq!(moved(&bends, &[]), 0);
        assert_eq!(moved(&[], &[(0, 100)]), 0);
    }

    #[test]
    fn a_bend_range_is_read_from_its_registered_parameter() {
        let curve = |controller, values: &[(u32, u16)]| ControlCurve {
            channel: 2,
            kind: ControlKind::Controller(controller),
            points: points(values),
        };
        let track = MidiTrack {
            controls: vec![
                curve(CC_RPN_COARSE, &[(0, 0), (0, 127), (500, 0)]),
                curve(CC_RPN_FINE, &[(0, 0), (0, 127), (500, 0)]),
                // The value at 900 belongs to the parameter chosen at 500.
                curve(CC_DATA_ENTRY, &[(0, 12), (500, 24), (900, 7)]),
                curve(CC_DATA_ENTRY_FINE, &[(0, 0)]),
            ],
            ..MidiTrack::default()
        };
        let set = parameters(&track, 2);
        assert_eq!(set[&RPN_BEND_RANGE], [(0, 12), (500, 24), (900, 7)]);
        assert!(parameters(&track, 3).is_empty());

        let part = read(&track, 2, &[(10, 20), (600, 700)], false);
        assert_eq!(part.notes, [NoteValues::NEUTRAL; 2]);
        assert_eq!(part.controllers_kept, 10);
    }
}
