//! Recorded pattern controls reuse the playlist target resolver and checked
//! event-lane command. Unsupported locations remain explicit import losses.
use super::Builder;
use crate::{
    model::ControlEvent,
    report::{Outcome, ReportSection},
    target::{ChannelParam, ControlTarget, InsertParam, MainParam, SlotParam},
    units,
};
use windfall_project::{AutomationPoint, AutomationTarget, Command, PatternId};

impl Builder<'_> {
    pub(super) fn pattern_events(&mut self, pattern: PatternId, events: &[ControlEvent]) {
        let mut lanes: Vec<(AutomationTarget, Vec<AutomationPoint>)> = Vec::new();
        let length = self.project().pattern(pattern).map_or(0, |pattern| {
            pattern.length_steps * windfall_core::TICKS_PER_STEP
        });
        for event in events {
            let source = ControlTarget::decode(event.location);
            let target = self
                .automation_target(source)
                .filter(|target| *target != AutomationTarget::Tempo);
            let tick = units::rescale(event.position, self.ppq);
            let Some(target) = target.filter(|_| tick.ticks <= u64::from(length)) else {
                self.report.count(ReportSection::Other, Outcome::Dropped, 1);
                self.report.say(ReportSection::Other, Outcome::Dropped, "A recorded pattern control has an unsupported target (including pattern tempo) or exceeds the pattern length.");
                continue;
            };
            // Event 223 contains native integer control values, unlike the
            // normalized floating-point values of automation clips.
            let actual = match source {
                ControlTarget::Channel {
                    param: ChannelParam::Volume,
                    ..
                } => Some(units::channel_gain(event.value.max(0) as u32)),
                ControlTarget::Channel {
                    param: ChannelParam::Pan,
                    ..
                } => Some(units::channel_pan(event.value.max(0) as u32)),
                ControlTarget::Insert {
                    param: InsertParam::Volume,
                    ..
                }
                | ControlTarget::Route { .. }
                | ControlTarget::Main(MainParam::Volume) => Some(units::fader_gain(event.value)),
                ControlTarget::Insert {
                    param: InsertParam::Pan,
                    ..
                } => Some(units::insert_pan(event.value)),
                ControlTarget::Slot {
                    param: SlotParam::Mix,
                    ..
                } => Some((event.value as f32 / 12800.0).clamp(0.0, 1.0)),
                _ => None,
            };
            let Some(value) = actual.and_then(|actual| {
                self.project()
                    .automation_range(&target)
                    .map(|range| range.normalized(actual))
            }) else {
                self.report.count(ReportSection::Other, Outcome::Dropped, 1);
                self.report.say(
                    ReportSection::Other,
                    Outcome::Dropped,
                    "A recorded plugin control uses an unsupported native value encoding.",
                );
                continue;
            };
            let index = match lanes.iter().position(|(other, _)| *other == target) {
                Some(index) => index,
                None => {
                    lanes.push((target, Vec::new()));
                    lanes.len() - 1
                }
            };
            lanes[index].1.push(AutomationPoint {
                tick: tick.ticks as u32,
                value,
                curve: 0.0,
                hold: true,
            });
            self.report
                .count(ReportSection::Other, Outcome::Approximated, 1);
        }
        for (target, mut points) in lanes {
            points.sort_by_key(|point| point.tick);
            // The final recorded value at each tick wins.
            let mut unique: Vec<AutomationPoint> = Vec::new();
            for point in points {
                if unique.last().is_some_and(|last| last.tick == point.tick) {
                    unique.pop();
                }
                unique.push(point);
            }
            self.apply(
                ReportSection::Other,
                "Recorded pattern events",
                Command::SetPatternEvents {
                    pattern,
                    target,
                    expected: Vec::new(),
                    points: unique,
                },
            );
        }
        if !events.is_empty() {
            self.report.say(ReportSection::Other, Outcome::Approximated, "Supported recorded controls were converted to held pattern event points; source interpolation and gain laws can differ.");
        }
    }
}
