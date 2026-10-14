//! Event automation: curves a pattern stores for itself.
//!
//! An automation clip moves a control along the song. A pattern's event lane
//! moves it along the pattern instead: wherever the pattern plays, alone or
//! as a clip on the playlist, the control follows the lane read at the place
//! in the pattern. A lane is one curve of [`AutomationPoint`]s in pattern
//! ticks for one [`AutomationTarget`], and a pattern has at most one lane per
//! target. The tempo is not a target: it belongs to the song.
//!
//! Everything here runs on the document thread.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    AutomationId, AutomationPoint, AutomationTarget, CommandError, MAX_AUTOMATION_POINTS,
    MAX_PATTERN_TICKS, Pattern, Project,
};

/// Lanes one pattern can hold.
pub const MAX_PATTERN_EVENT_LANES: usize = 64;

/// One control's curve inside a pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PatternEventLane {
    /// Names the lane where the engine reports the value it gave the
    /// control. Unique among the project's automations and event lanes.
    pub id: AutomationId,
    pub target: AutomationTarget,
    /// At least one point and at most [`MAX_AUTOMATION_POINTS`], in order of
    /// their ticks, none past [`MAX_PATTERN_TICKS`]. Read with
    /// [`curve_value`](crate::curve_value), like an automation's.
    pub points: Vec<AutomationPoint>,
}

impl Pattern {
    /// The lane this pattern has for `target`.
    pub fn event_lane(&self, target: &AutomationTarget) -> Option<&PatternEventLane> {
        self.events.iter().find(|lane| lane.target == *target)
    }
}

/// The points of a lane, brought into range. An empty list is allowed here:
/// it means the lane is to go.
pub(crate) fn checked_points(
    points: Vec<AutomationPoint>,
) -> Result<Vec<AutomationPoint>, CommandError> {
    if points.len() > MAX_AUTOMATION_POINTS {
        return Err(CommandError::invalid(
            windfall_core::notice!(
                "project.events.tooManyPoints",
                max = MAX_AUTOMATION_POINTS,
                count = points.len()
            )
            .to_string(),
        ));
    }
    if !points.is_sorted_by_key(|point| point.tick) {
        return Err(CommandError::invalid(
            "the points of an event lane must be in order of time",
        ));
    }
    if points
        .last()
        .is_some_and(|point| point.tick > MAX_PATTERN_TICKS)
    {
        return Err(CommandError::invalid(
            "a point of the event lane is past the end of the longest pattern",
        ));
    }
    points
        .into_iter()
        .map(|point| {
            if !point.value.is_finite() || !point.curve.is_finite() {
                return Err(CommandError::invalid(
                    "the value and curve of an event point must be numbers",
                ));
            }
            Ok(AutomationPoint {
                value: point.value.clamp(0.0, 1.0),
                curve: point.curve.clamp(-1.0, 1.0),
                ..point
            })
        })
        .collect()
}

/// The lanes a pattern has once the lane of `target` is replaced by
/// `points`. No points takes the lane out. `fresh` is the id a new lane gets.
pub(crate) fn replaced(
    lanes: &[PatternEventLane],
    target: AutomationTarget,
    points: Vec<AutomationPoint>,
    fresh: Option<AutomationId>,
) -> Result<Vec<PatternEventLane>, CommandError> {
    if target == AutomationTarget::Tempo {
        return Err(CommandError::invalid(
            "the tempo belongs to the song: automate it with a clip on the playlist",
        ));
    }
    let mut next = lanes.to_vec();
    let at = next.iter().position(|lane| lane.target == target);
    match (at, points.is_empty()) {
        (Some(at), true) => {
            next.remove(at);
        }
        (Some(at), false) => next[at].points = points,
        (None, true) => {}
        (None, false) => {
            if next.len() >= MAX_PATTERN_EVENT_LANES {
                return Err(CommandError::invalid(
                    windfall_core::notice!(
                        "project.events.tooManyLanes",
                        max = MAX_PATTERN_EVENT_LANES
                    )
                    .to_string(),
                ));
            }
            let id = fresh.ok_or_else(|| CommandError::invalid("the event lane has no id"))?;
            next.push(PatternEventLane { id, target, points });
        }
    }
    Ok(next)
}

/// The points a pattern holds for `target`, none when it has no lane.
pub(crate) fn captured(pattern: &Pattern, target: &AutomationTarget) -> Vec<AutomationPoint> {
    pattern
        .event_lane(target)
        .map(|lane| lane.points.clone())
        .unwrap_or_default()
}

/// The lanes of a pattern without those `gone` picks out.
pub(crate) fn without(
    lanes: &[PatternEventLane],
    gone: impl Fn(&AutomationTarget) -> bool,
) -> Vec<PatternEventLane> {
    lanes
        .iter()
        .filter(|lane| !gone(&lane.target))
        .cloned()
        .collect()
}

/// What a saved project must satisfy.
pub(crate) fn check(project: &Project) -> Result<(), String> {
    let mut ids: HashSet<AutomationId> = project.automations.iter().map(|a| a.id).collect();
    for pattern in &project.patterns {
        let owner = windfall_core::notice!("project.owner.pattern", id = pattern.id.0).to_string();
        if pattern.events.len() > MAX_PATTERN_EVENT_LANES {
            return Err(windfall_core::notice!(
                "project.events.check.tooManyLanes",
                owner = owner,
                count = pattern.events.len(),
                max = MAX_PATTERN_EVENT_LANES
            )
            .to_string());
        }
        let mut targets = HashSet::new();
        for lane in &pattern.events {
            let raw = lane.id.0;
            if raw == 0 || raw >= project.next_id {
                return Err(windfall_core::notice!(
                    "project.events.check.laneId",
                    owner = owner,
                    id = raw
                )
                .to_string());
            }
            if !ids.insert(lane.id) {
                return Err(windfall_core::notice!(
                    "project.events.check.laneIdRepeated",
                    owner = owner,
                    id = raw
                )
                .to_string());
            }
            if lane.target == AutomationTarget::Tempo {
                return Err(windfall_core::notice!(
                    "project.events.check.tempoLane",
                    owner = owner
                )
                .to_string());
            }
            if project.automation_range(&lane.target).is_none() {
                return Err(windfall_core::notice!(
                    "project.events.check.target",
                    owner = owner,
                    target = format!("{:?}", lane.target)
                )
                .to_string());
            }
            if !targets.insert(lane.target) {
                return Err(windfall_core::notice!(
                    "project.events.check.repeatedTarget",
                    owner = owner,
                    target = format!("{:?}", lane.target)
                )
                .to_string());
            }
            let points = &lane.points;
            if points.is_empty() || points.len() > MAX_AUTOMATION_POINTS {
                return Err(windfall_core::notice!(
                    "project.events.check.points",
                    owner = owner,
                    count = points.len(),
                    max = MAX_AUTOMATION_POINTS
                )
                .to_string());
            }
            if !points.is_sorted_by_key(|point| point.tick) {
                return Err(
                    windfall_core::notice!("project.events.check.unsorted", owner = owner)
                        .to_string(),
                );
            }
            for point in points {
                let value = point.value >= 0.0 && point.value <= 1.0;
                let curve = point.curve >= -1.0 && point.curve <= 1.0;
                if point.tick > MAX_PATTERN_TICKS || !value || !curve {
                    return Err(windfall_core::notice!(
                        "project.events.check.pointRange",
                        owner = owner
                    )
                    .to_string());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelId;

    fn point(tick: u32, value: f32) -> AutomationPoint {
        AutomationPoint {
            tick,
            value,
            curve: 0.0,
            hold: false,
        }
    }

    const VOLUME: AutomationTarget = AutomationTarget::ChannelVolume {
        channel: ChannelId(1),
    };
    const PAN: AutomationTarget = AutomationTarget::ChannelPan {
        channel: ChannelId(1),
    };

    #[test]
    fn points_are_clamped_and_must_be_ordered_finite_and_inside_the_pattern() {
        let clamped = checked_points(vec![
            AutomationPoint {
                tick: 0,
                value: -3.0,
                curve: 9.0,
                hold: true,
            },
            point(MAX_PATTERN_TICKS, 2.0),
        ])
        .unwrap();
        assert_eq!(clamped[0].value, 0.0);
        assert_eq!(clamped[0].curve, 1.0);
        assert!(clamped[0].hold);
        assert_eq!(clamped[1].value, 1.0);
        assert!(checked_points(Vec::new()).unwrap().is_empty());
        assert!(checked_points(vec![point(10, 0.5), point(9, 0.5)]).is_err());
        assert!(checked_points(vec![point(MAX_PATTERN_TICKS + 1, 0.5)]).is_err());
        assert!(checked_points(vec![point(0, f32::NAN)]).is_err());
        assert!(checked_points(vec![point(0, 0.5); MAX_AUTOMATION_POINTS + 1]).is_err());
    }

    #[test]
    fn replacing_adds_changes_and_removes_one_lane_per_target() {
        let id = AutomationId(7);
        let added = replaced(&[], VOLUME, vec![point(0, 0.25)], Some(id)).unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].id, id);
        // A lane that exists keeps its id, whatever is offered.
        let changed =
            replaced(&added, VOLUME, vec![point(0, 0.75)], Some(AutomationId(8))).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].id, id);
        assert_eq!(changed[0].points[0].value, 0.75);
        let two = replaced(&changed, PAN, vec![point(0, 0.5)], Some(AutomationId(9))).unwrap();
        assert_eq!(two.len(), 2);
        let removed = replaced(&two, VOLUME, Vec::new(), None).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].target, PAN);
        // Removing what is not there changes nothing.
        assert_eq!(
            replaced(&removed, VOLUME, Vec::new(), None).unwrap(),
            removed
        );
    }

    #[test]
    fn the_tempo_a_missing_id_and_a_full_pattern_are_refused() {
        assert!(
            replaced(
                &[],
                AutomationTarget::Tempo,
                vec![point(0, 0.5)],
                Some(AutomationId(1))
            )
            .is_err()
        );
        assert!(replaced(&[], VOLUME, vec![point(0, 0.5)], None).is_err());
        let full: Vec<_> = (0..MAX_PATTERN_EVENT_LANES as u32)
            .map(|index| PatternEventLane {
                id: AutomationId(index + 1),
                target: AutomationTarget::ChannelVolume {
                    channel: ChannelId(index + 100),
                },
                points: vec![point(0, 0.5)],
            })
            .collect();
        assert!(replaced(&full, VOLUME, vec![point(0, 0.5)], Some(AutomationId(999))).is_err());
        // Changing a lane of a full pattern is still fine.
        let target = full[3].target;
        assert!(replaced(&full, target, vec![point(0, 0.1)], None).is_ok());
    }

    #[test]
    fn without_drops_only_the_lanes_picked_out() {
        let lanes = vec![
            PatternEventLane {
                id: AutomationId(1),
                target: VOLUME,
                points: vec![point(0, 0.5)],
            },
            PatternEventLane {
                id: AutomationId(2),
                target: PAN,
                points: vec![point(0, 0.5)],
            },
        ];
        let kept = without(&lanes, |target| *target == VOLUME);
        assert_eq!(kept, lanes[1..]);
    }
}
