//! Control-thread destination capture for pattern-mode automation recording.
use super::controller_capture::{AutomationLane, Position, merge_points, recorded_points};
use windfall_ipc::PlayMode;
use windfall_project::{
    AutomationPoint, AutomationTarget, Command, PatternEventLane, PatternId, Project,
    TICKS_PER_STEP,
};

pub(super) enum Destination {
    Song,
    Pattern {
        id: PatternId,
        ticks: u32,
        events: Vec<PatternEventLane>,
    },
}

impl Destination {
    pub fn capture(project: &Project, place: Position) -> Option<Self> {
        match place.mode {
            PlayMode::Song => Some(Self::Song),
            PlayMode::Pattern => project.pattern(place.pattern).map(|pattern| Self::Pattern {
                id: pattern.id,
                ticks: pattern.length_steps * TICKS_PER_STEP,
                events: pattern.events.clone(),
            }),
        }
    }
    pub fn matches(&self, place: Position) -> bool {
        match self {
            Self::Song => place.mode == PlayMode::Song,
            Self::Pattern { id, ticks, .. } => {
                place.mode == PlayMode::Pattern
                    && place.pattern == *id
                    && place.pattern_ticks == Some(*ticks)
            }
        }
    }
    pub fn command(
        &self,
        project: &Project,
        lanes: &[AutomationLane],
    ) -> Result<Option<Command>, String> {
        let Self::Pattern { id, ticks, events } = self else {
            return Ok(super::controllers::recorded_automation_command(
                project, lanes,
            ));
        };
        let pattern = project
            .pattern(*id)
            .ok_or("The recorded pattern no longer exists.")?;
        if pattern.length_steps * TICKS_PER_STEP != *ticks {
            return Err("The recorded pattern length changed.".into());
        }
        let mut commands = Vec::new();
        for lane in lanes {
            if lane.target == AutomationTarget::Tempo
                || project.automation_range(&lane.target).is_none()
            {
                continue;
            }
            let samples: Vec<_> = lane
                .samples
                .iter()
                .copied()
                .filter(|(tick, _)| *tick >= 0.0 && *tick < f64::from(*ticks))
                .collect();
            let recorded = recorded_points(&samples, windfall_project::MAX_AUTOMATION_POINTS);
            if recorded.is_empty() {
                continue;
            }
            let expected = events
                .iter()
                .find(|event| event.target == lane.target)
                .map_or_else(Vec::new, |event| event.points.clone());
            let seed = [AutomationPoint {
                tick: 0,
                value: lane.before,
                curve: 0.0,
                hold: false,
            }];
            let points = merge_points(
                if expected.is_empty() {
                    &seed
                } else {
                    &expected
                },
                &recorded,
                !expected.is_empty(),
            );
            if points != expected {
                commands.push(Command::SetPatternEvents {
                    pattern: *id,
                    target: lane.target,
                    expected,
                    points,
                });
            }
        }
        Ok((!commands.is_empty()).then_some(Command::Batch {
            label: Some("Record pattern events".into()),
            commands,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::{Channel, ChannelId, Document, PatternEventLane};
    #[test]
    fn looping_pattern_overdub_restores_outside_the_recorded_span_and_refuses_length_drift() {
        let mut project = Project::new("Pattern movement");
        let channel = ChannelId(2);
        let mut document = Document::new(project);
        document
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: None,
                    mixer_track: None,
                    index: None,
                },
                None,
            )
            .unwrap();
        project = document.project().clone();
        let channel = project
            .channels
            .first()
            .map_or(channel, |channel: &Channel| channel.id);
        let target = AutomationTarget::ChannelPan { channel };
        let point = |tick, value| AutomationPoint {
            tick,
            value,
            curve: 0.0,
            hold: false,
        };
        project.patterns[0].events.push(PatternEventLane {
            id: windfall_project::AutomationId(10),
            target,
            points: vec![point(0, 0.2), point(1500, 0.8)],
        });
        project.next_id = project.next_id.max(11);
        let place = Position {
            tick: 0.0,
            mode: PlayMode::Pattern,
            pattern: project.patterns[0].id,
            pattern_ticks: Some(project.patterns[0].length_steps * TICKS_PER_STEP),
        };
        let destination = Destination::capture(&project, place).unwrap();
        let lanes = [AutomationLane {
            pattern: Some(place.pattern),
            target,
            before: 0.2,
            samples: vec![(1000.0, 0.9), (1200.0, 0.9), (240.0, 0.1), (480.0, 0.3)],
        }];
        let command = destination.command(&project, &lanes).unwrap().unwrap();
        let mut document = Document::new(project.clone());
        document.dispatch(command, None).unwrap();
        let points = &document.project().patterns[0].events[0].points;
        assert!(
            points
                .iter()
                .any(|point| point.tick == 240 && point.value == 0.1)
        );
        assert!(
            points
                .iter()
                .any(|point| point.tick == 480 && point.value == 0.3)
        );
        assert!(
            points
                .iter()
                .any(|point| point.tick == 1500 && point.value == 0.8)
        );
        assert!(
            !points
                .iter()
                .any(|point| point.tick == 1000 && point.value == 0.9)
        );
        document.undo().unwrap();
        assert_eq!(document.project(), &project);
        project.patterns[0].length_steps += 1;
        assert!(destination.command(&project, &lanes).is_err());
    }
}
