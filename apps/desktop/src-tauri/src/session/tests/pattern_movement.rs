use super::Rig;
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{AutomationPoint, AutomationTarget, ChannelPatch, Command};

fn move_volume(rig: &mut Rig, value: f32) {
    rig.run(12_000);
    let playhead = rig.session.controller().playhead();
    rig.session
        .controller_idle(std::time::Instant::now(), &playhead);
    rig.session
        .dispatch(
            Command::UpdateChannel {
                id: rig.channel(0),
                patch: ChannelPatch {
                    volume: Some(value),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
}
fn begin(rig: &mut Rig) {
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Pattern),
            pattern: Some(rig.pattern()),
            ..Default::default()
        })
        .unwrap();
    rig.session.automation_record_arm(true);
    rig.session.transport_play().unwrap();
}

#[test]
fn pattern_mode_movements_write_events_once_and_preserve_history_file_and_outside_overdub() {
    let mut rig = Rig::new();
    let target = AutomationTarget::ChannelVolume {
        channel: rig.channel(0),
    };
    let before = rig.session.document_snapshot().history.cursor;
    let initial_volume = rig.project().channel(rig.channel(0)).unwrap().volume;
    begin(&mut rig);
    move_volume(&mut rig, 0.25);
    move_volume(&mut rig, 0.75);
    assert_eq!(rig.session.controller_state().automation_record.targets, 1);
    rig.session.automation_record_arm(false);
    let snapshot = rig.session.document_snapshot();
    assert_eq!(snapshot.history.cursor, before + 1);
    let pattern = snapshot.project.pattern(rig.pattern()).unwrap();
    let lane = pattern
        .events
        .iter()
        .find(|lane| lane.target == target)
        .unwrap()
        .clone();
    assert!(lane.points.len() >= 3);
    assert!(
        lane.points
            .windows(2)
            .all(|pair| pair[0].tick < pair[1].tick
                || (pair[0].tick == pair[1].tick && pair[0].value != pair[1].value))
    );
    let seam = lane
        .points
        .windows(2)
        .find(|pair| pair[0].tick == pair[1].tick)
        .unwrap();
    assert_eq!(seam[0].value, initial_volume);
    assert_eq!(seam[1].value, 0.25);
    assert!(
        lane.points
            .iter()
            .all(|point| point.tick < pattern.length_steps * 240)
    );
    assert!(snapshot.project.automations.is_empty());
    let saved = rig
        .session
        .project_save(Some(&rig.file("pattern-movements.windfall")))
        .unwrap();
    rig.session.undo().unwrap();
    assert!(
        rig.project()
            .pattern(rig.pattern())
            .unwrap()
            .events
            .is_empty()
    );
    rig.session.redo().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(
        rig.project().pattern(rig.pattern()).unwrap().events,
        vec![lane]
    );
}

#[test]
fn pattern_mode_recording_refuses_a_concurrent_event_edit_and_an_obsolete_generation() {
    let mut rig = Rig::new();
    let target = AutomationTarget::ChannelVolume {
        channel: rig.channel(0),
    };
    begin(&mut rig);
    move_volume(&mut rig, 0.25);
    let points = vec![AutomationPoint {
        tick: 0,
        value: 0.9,
        curve: 0.0,
        hold: true,
    }];
    rig.session
        .dispatch(
            Command::SetPatternEvents {
                pattern: rig.pattern(),
                target,
                expected: vec![],
                points: points.clone(),
            },
            None,
        )
        .unwrap();
    let before = rig.session.document_snapshot().history.cursor;
    rig.session.automation_record_arm(false);
    assert_eq!(rig.session.document_snapshot().history.cursor, before);
    assert_eq!(
        rig.project().pattern(rig.pattern()).unwrap().events[0].points,
        points
    );
    assert!(rig.session.controller_state().error.is_some());
    rig.session.transport_stop();
    begin(&mut rig);
    move_volume(&mut rig, 0.5);
    rig.session.project_new().unwrap();
    let replacement = rig.session.document_snapshot();
    rig.session.automation_record_arm(false);
    assert_eq!(rig.session.document_snapshot(), replacement);
}
