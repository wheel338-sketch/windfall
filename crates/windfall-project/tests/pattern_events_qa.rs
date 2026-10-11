//! Public document acceptance for event automation stored inside a pattern.
use windfall_project::file::{from_json, to_json};
use windfall_project::*;

fn point(tick: u32, value: f32) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: 0.0,
        hold: false,
    }
}

/// A synth channel, a second mixer track and a delay on the first insert.
struct Fixture {
    doc: Document,
    channel: ChannelId,
    track: TrackId,
    other_track: TrackId,
    effect: EffectId,
}

fn created(doc: &mut Document, command: Command) -> u32 {
    doc.dispatch(command, None).unwrap().created[0]
}

fn fixture() -> Fixture {
    let mut doc = Document::new(Project::new("Event QA"));
    let channel = ChannelId(created(
        &mut doc,
        Command::AddChannel {
            name: Some("Lead".into()),
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            mixer_track: None,
            index: None,
        },
    ));
    let track = doc.project().channel(channel).unwrap().mixer_track;
    let other_track = TrackId(created(&mut doc, Command::AddMixerTrack { name: None }));
    let effect = EffectId(created(
        &mut doc,
        Command::AddEffect {
            track,
            kind: EffectKind::Delay,
            index: None,
        },
    ));
    Fixture {
        doc,
        channel,
        track,
        other_track,
        effect,
    }
}

/// Through the JSON boundary the UI and the native bridge use.
fn json(command: &Command) -> Command {
    serde_json::from_str(&serde_json::to_string(command).unwrap()).unwrap()
}

fn set(
    doc: &mut Document,
    target: AutomationTarget,
    expected: Vec<AutomationPoint>,
    points: Vec<AutomationPoint>,
) -> Result<Applied, CommandError> {
    doc.dispatch(
        json(&Command::SetPatternEvents {
            pattern: PatternId(1),
            target,
            expected,
            points,
        }),
        None,
    )
}

fn lanes(doc: &Document) -> Vec<PatternEventLane> {
    doc.project().pattern(PatternId(1)).unwrap().events.clone()
}

fn entries(doc: &Document) -> usize {
    doc.history().entries.len()
}

#[test]
fn a_lane_is_added_changed_and_cleared_as_single_undoable_edits() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let volume = AutomationTarget::ChannelVolume {
        channel: fixture.channel,
    };
    let before = entries(doc);
    let curve = vec![
        point(0, 0.25),
        AutomationPoint {
            tick: 960,
            value: 1.0,
            curve: 0.5,
            hold: true,
        },
    ];

    let added = set(doc, volume, Vec::new(), curve.clone()).unwrap();
    assert_eq!(added.label, "Add event automation");
    assert_eq!(added.created.len(), 1);
    assert_eq!(added.touched.patterns, vec![PatternId(1)]);
    let lane = lanes(doc);
    assert_eq!(lane.len(), 1);
    assert_eq!(lane[0].id, AutomationId(added.created[0]));
    assert_eq!(lane[0].target, volume);
    assert_eq!(lane[0].points, curve);
    assert_eq!(entries(doc), before + 1);

    let moved = vec![point(0, 0.5), point(480, 0.75)];
    let changed = set(doc, volume, curve.clone(), moved.clone()).unwrap();
    assert_eq!(changed.label, "Change event automation");
    assert!(
        changed.created.is_empty(),
        "a lane that exists keeps its id"
    );
    assert_eq!(lanes(doc)[0].id, lane[0].id);
    assert_eq!(lanes(doc)[0].points, moved);
    assert_eq!(entries(doc), before + 2);

    let cleared = set(doc, volume, moved.clone(), Vec::new()).unwrap();
    assert_eq!(cleared.label, "Clear event automation");
    assert!(lanes(doc).is_empty());
    assert_eq!(entries(doc), before + 3);

    // Each step undoes and redoes exactly, the lane's id included.
    doc.undo().unwrap();
    assert_eq!(lanes(doc)[0].points, moved);
    doc.undo().unwrap();
    assert_eq!(lanes(doc), lane);
    doc.undo().unwrap();
    assert!(lanes(doc).is_empty());
    doc.redo().unwrap();
    assert_eq!(lanes(doc), lane);

    // Clearing a lane the pattern does not have changes nothing and adds no history.
    let pan = AutomationTarget::ChannelPan {
        channel: fixture.channel,
    };
    let entries_before = entries(doc);
    let nothing = set(doc, pan, Vec::new(), Vec::new()).unwrap();
    assert!(nothing.touched.patterns.is_empty());
    assert_eq!(entries(doc), entries_before);
}

#[test]
fn values_are_clamped_and_a_drag_with_a_gesture_is_one_history_entry() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let pan = AutomationTarget::ChannelPan {
        channel: fixture.channel,
    };
    set(
        doc,
        pan,
        Vec::new(),
        vec![AutomationPoint {
            tick: 0,
            value: 7.0,
            curve: -9.0,
            hold: false,
        }],
    )
    .unwrap();
    let stored = lanes(doc)[0].points[0];
    assert_eq!((stored.value, stored.curve), (1.0, -1.0));

    let before = entries(doc);
    let mut held = lanes(doc)[0].points.clone();
    for step in 1..=5u32 {
        let next = vec![point(0, 1.0 - step as f32 * 0.1)];
        doc.dispatch(
            Command::SetPatternEvents {
                pattern: PatternId(1),
                target: pan,
                expected: held,
                points: next.clone(),
            },
            Some(42),
        )
        .unwrap();
        held = next;
    }
    assert_eq!(entries(doc), before + 1, "the whole drag is one undo step");
    assert_eq!(lanes(doc)[0].points, vec![point(0, 0.5)]);
    doc.undo().unwrap();
    assert_eq!(lanes(doc)[0].points, vec![stored]);
}

#[test]
fn stale_and_invalid_requests_leave_project_history_and_ids_untouched() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let channel = fixture.channel;
    let volume = AutomationTarget::ChannelVolume { channel };
    set(doc, volume, Vec::new(), vec![point(0, 0.5)]).unwrap();

    let project = doc.project().clone();
    let history = entries(doc);
    let revision = doc.revision();
    let refuse = |doc: &mut Document, command: Command| {
        assert!(doc.dispatch(json(&command), None).is_err(), "{command:?}");
        assert_eq!(doc.project(), &project);
        assert_eq!(entries(doc), history);
        assert_eq!(doc.revision(), revision);
    };
    let events = |target, expected: Vec<AutomationPoint>, points: Vec<AutomationPoint>| {
        Command::SetPatternEvents {
            pattern: PatternId(1),
            target,
            expected,
            points,
        }
    };
    let pan = AutomationTarget::ChannelPan { channel };

    // The editor captured something else than the pattern now holds.
    refuse(doc, events(volume, Vec::new(), vec![point(0, 0.1)]));
    refuse(
        doc,
        events(volume, vec![point(0, 0.4)], vec![point(0, 0.1)]),
    );
    refuse(doc, events(pan, vec![point(0, 0.5)], vec![point(0, 0.1)]));
    // The tempo belongs to the song.
    refuse(
        doc,
        events(AutomationTarget::Tempo, Vec::new(), vec![point(0, 0.5)]),
    );
    // What the lane would move is not in the project.
    let ghost = AutomationTarget::ChannelVolume {
        channel: ChannelId(9999),
    };
    refuse(doc, events(ghost, Vec::new(), vec![point(0, 0.5)]));
    let no_setting = AutomationTarget::InstrumentParam {
        channel,
        param: 9999,
    };
    refuse(doc, events(no_setting, Vec::new(), vec![point(0, 0.5)]));
    // Points out of order, past the longest pattern, not numbers, or too many.
    refuse(
        doc,
        events(pan, Vec::new(), vec![point(10, 0.5), point(9, 0.5)]),
    );
    refuse(
        doc,
        events(pan, Vec::new(), vec![point(MAX_PATTERN_TICKS + 1, 0.5)]),
    );
    // JSON has no NaN, so the boundary itself turns one away; so does the command.
    let nan = events(pan, Vec::new(), vec![point(0, f32::NAN)]);
    assert!(serde_json::from_str::<Command>(&serde_json::to_string(&nan).unwrap()).is_err());
    assert!(doc.dispatch(nan, None).is_err());
    assert_eq!(doc.project(), &project);
    assert_eq!(entries(doc), history);
    refuse(
        doc,
        events(
            pan,
            Vec::new(),
            vec![point(0, 0.5); MAX_AUTOMATION_POINTS + 1],
        ),
    );
    // A pattern that is not there.
    refuse(
        doc,
        Command::SetPatternEvents {
            pattern: PatternId(9999),
            target: pan,
            expected: Vec::new(),
            points: vec![point(0, 0.5)],
        },
    );
}

#[test]
fn a_pattern_holds_sixty_four_lanes_and_refuses_the_next() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    // The synth has plenty of settings to give each lane its own target.
    let descriptors = InstrumentKind::SubtractiveSynth.descriptors().len() as u32;
    let mut targets: Vec<AutomationTarget> = (0..descriptors)
        .map(|param| AutomationTarget::InstrumentParam {
            channel: fixture.channel,
            param,
        })
        .collect();
    let delay = EffectKind::Delay.descriptors().len() as u32;
    targets.extend((0..delay).map(|param| AutomationTarget::EffectParam {
        track: fixture.track,
        effect: fixture.effect,
        param,
    }));
    let mut extra = Vec::new();
    while targets.len() + extra.len() * 2 <= MAX_PATTERN_EVENT_LANES {
        extra.push(TrackId(created(doc, Command::AddMixerTrack { name: None })));
    }
    for track in extra {
        targets.push(AutomationTarget::TrackVolume { track });
        targets.push(AutomationTarget::TrackPan { track });
    }
    assert!(targets.len() > MAX_PATTERN_EVENT_LANES);
    for target in &targets[..MAX_PATTERN_EVENT_LANES] {
        set(doc, *target, Vec::new(), vec![point(0, 0.5)]).unwrap();
    }
    assert_eq!(lanes(doc).len(), MAX_PATTERN_EVENT_LANES);
    let project = doc.project().clone();
    let error = set(
        doc,
        targets[MAX_PATTERN_EVENT_LANES],
        Vec::new(),
        vec![point(0, 0.5)],
    )
    .unwrap_err();
    assert!(error.to_string().contains("64"), "{error}");
    assert_eq!(doc.project(), &project, "a refused lane hands out no id");
    // A lane that is there can still be changed.
    set(doc, targets[0], vec![point(0, 0.5)], vec![point(0, 0.25)]).unwrap();
}

#[test]
fn lanes_survive_the_file_and_old_files_load_without_them() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let legacy = to_json(doc.project()).unwrap();
    assert!(
        !legacy.contains("\"events\""),
        "a pattern without event automation writes no key"
    );
    assert!(from_json(&legacy).unwrap().patterns[0].events.is_empty());

    let mix = AutomationTarget::EffectMix {
        track: fixture.track,
        effect: fixture.effect,
    };
    let curve = vec![
        point(0, 0.0),
        AutomationPoint {
            tick: 1920,
            value: 1.0,
            curve: -0.25,
            hold: false,
        },
    ];
    set(doc, mix, Vec::new(), curve.clone()).unwrap();
    let saved = to_json(doc.project()).unwrap();
    let loaded = from_json(&saved).unwrap();
    assert_eq!(&loaded, doc.project());
    assert_eq!(loaded.patterns[0].events[0].points, curve);
    assert_eq!(loaded.patterns[0].events[0].target, mix);
}

#[test]
fn files_with_broken_lanes_are_refused() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let volume = AutomationTarget::ChannelVolume {
        channel: fixture.channel,
    };
    set(
        doc,
        volume,
        Vec::new(),
        vec![point(0, 0.5), point(960, 1.0)],
    )
    .unwrap();
    doc.dispatch(
        Command::AddAutomation {
            name: None,
            target: volume,
            points: None,
        },
        None,
    )
    .unwrap();
    let good = doc.project().clone();
    assert!(from_json(&to_json(&good).unwrap()).is_ok());

    let refused = |change: &dyn Fn(&mut Project)| {
        let mut project = good.clone();
        change(&mut project);
        let text = to_json(&project).unwrap();
        assert!(from_json(&text).is_err());
    };
    // An id shared with an automation, or one never handed out.
    refused(&|project| project.patterns[0].events[0].id = project.automations[0].id);
    refused(&|project| project.patterns[0].events[0].id = AutomationId(project.next_id));
    refused(&|project| project.patterns[0].events[0].id = AutomationId(0));
    // The tempo, a control that is not there, and one control twice.
    refused(&|project| project.patterns[0].events[0].target = AutomationTarget::Tempo);
    refused(&|project| {
        project.patterns[0].events[0].target = AutomationTarget::ChannelPan {
            channel: ChannelId(9999),
        }
    });
    refused(&|project| {
        let mut twin = project.patterns[0].events[0].clone();
        twin.id = AutomationId(project.next_id);
        project.next_id += 1;
        project.patterns[0].events.push(twin);
    });
    // No points, points out of order, out of range or past the pattern.
    refused(&|project| project.patterns[0].events[0].points.clear());
    refused(&|project| project.patterns[0].events[0].points.reverse());
    refused(&|project| project.patterns[0].events[0].points[0].value = 1.5);
    refused(&|project| project.patterns[0].events[0].points[1].tick = MAX_PATTERN_TICKS + 1);
}

#[test]
fn removing_what_a_lane_moves_removes_the_lane_and_undo_brings_it_back() {
    let mut fixture = fixture();
    let (channel, track, effect) = (fixture.channel, fixture.track, fixture.effect);
    let doc = &mut fixture.doc;
    let targets = [
        AutomationTarget::ChannelVolume { channel },
        AutomationTarget::InstrumentParam { channel, param: 0 },
        AutomationTarget::EffectParam {
            track,
            effect,
            param: 0,
        },
        AutomationTarget::EffectMix { track, effect },
        AutomationTarget::TrackPan { track },
    ];
    for target in targets {
        set(doc, target, Vec::new(), vec![point(0, 0.5)]).unwrap();
    }
    let all = lanes(doc);
    assert_eq!(all.len(), 5);

    doc.dispatch(Command::RemoveEffect { track, effect }, None)
        .unwrap();
    let left: Vec<_> = lanes(doc).iter().map(|lane| lane.target).collect();
    assert_eq!(left, vec![targets[0], targets[1], targets[4]]);
    doc.undo().unwrap();
    assert_eq!(
        lanes(doc),
        all,
        "one undo restores the effect and its lanes"
    );

    doc.dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    let left: Vec<_> = lanes(doc).iter().map(|lane| lane.target).collect();
    assert_eq!(left, vec![targets[2], targets[3], targets[4]]);
    // The file is still one the loader accepts.
    assert!(from_json(&to_json(doc.project()).unwrap()).is_ok());
    doc.undo().unwrap();
    assert_eq!(lanes(doc), all);
}

#[test]
fn a_lane_follows_its_effect_to_another_track() {
    let mut fixture = fixture();
    let (track, other, effect) = (fixture.track, fixture.other_track, fixture.effect);
    let doc = &mut fixture.doc;
    let param = AutomationTarget::EffectParam {
        track,
        effect,
        param: 1,
    };
    set(
        doc,
        param,
        Vec::new(),
        vec![point(0, 0.5), point(240, 0.75)],
    )
    .unwrap();
    let before = lanes(doc);

    doc.dispatch(
        Command::MoveEffect {
            track,
            effect,
            to_track: Some(other),
            index: 0,
        },
        None,
    )
    .unwrap();
    let moved = lanes(doc);
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].id, before[0].id);
    assert_eq!(moved[0].points, before[0].points);
    assert_eq!(
        moved[0].target,
        AutomationTarget::EffectParam {
            track: other,
            effect,
            param: 1,
        }
    );
    assert!(from_json(&to_json(doc.project()).unwrap()).is_ok());
    doc.undo().unwrap();
    assert_eq!(lanes(doc), before);
}

#[test]
fn copies_of_a_pattern_get_lanes_with_ids_of_their_own() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let volume = AutomationTarget::ChannelVolume {
        channel: fixture.channel,
    };
    let pan = AutomationTarget::ChannelPan {
        channel: fixture.channel,
    };
    set(
        doc,
        volume,
        Vec::new(),
        vec![point(0, 0.2), point(480, 0.9)],
    )
    .unwrap();
    set(doc, pan, Vec::new(), vec![point(0, 0.5)]).unwrap();
    let source = lanes(doc);

    let copy = PatternId(created(doc, Command::DuplicatePattern { id: PatternId(1) }));
    let copied = doc.project().pattern(copy).unwrap().events.clone();
    assert_eq!(copied.len(), 2);
    for (ours, theirs) in copied.iter().zip(&source) {
        assert_eq!(ours.target, theirs.target);
        assert_eq!(ours.points, theirs.points);
        assert_ne!(ours.id, theirs.id);
    }
    assert_ne!(copied[0].id, copied[1].id);
    assert!(from_json(&to_json(doc.project()).unwrap()).is_ok());

    // Editing the copy leaves the original alone.
    doc.dispatch(
        Command::SetPatternEvents {
            pattern: copy,
            target: pan,
            expected: vec![point(0, 0.5)],
            points: vec![point(0, 0.0)],
        },
        None,
    )
    .unwrap();
    assert_eq!(lanes(doc), source);

    // Make unique copies a clip's pattern, lanes and all.
    let track = PlaylistTrackId(created(
        doc,
        Command::AddPlaylistTrack {
            name: None,
            index: None,
        },
    ));
    let clip = ClipId(created(
        doc,
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 0,
                length: None,
                offset: None,
                muted: None,
                content: ClipContent::Pattern {
                    pattern: PatternId(1),
                },
            }],
        },
    ));
    doc.dispatch(Command::MakeUnique { clip }, None).unwrap();
    let unique = doc.project().patterns.last().unwrap();
    assert_ne!(unique.id, PatternId(1));
    assert_eq!(unique.events.len(), 2);
    assert!(unique.events.iter().all(|lane| {
        source.iter().all(|old| old.id != lane.id) && copied.iter().all(|old| old.id != lane.id)
    }));
    assert!(from_json(&to_json(doc.project()).unwrap()).is_ok());
}

#[test]
fn an_lfo_is_written_into_a_lane_in_one_edit() {
    let mut fixture = fixture();
    let doc = &mut fixture.doc;
    let pan = AutomationTarget::ChannelPan {
        channel: fixture.channel,
    };
    let lfo = CurveLfo {
        wave: CurveLfoWave::Sine,
        period: 960,
        phase: 0.0,
        center: 0.5,
        depth: 0.25,
        width: 0.5,
        seed: 0,
    };
    let generate =
        |expected: Vec<AutomationPoint>, start: u32, end: u32| Command::GeneratePatternEventLfo {
            pattern: PatternId(1),
            target: pan,
            expected,
            start,
            end,
            resolution: 120,
            lfo,
        };
    let before = entries(doc);

    // With no lane yet, the curve starts from what the pan is stored at.
    let applied = doc
        .dispatch(json(&generate(Vec::new(), 0, 960)), None)
        .unwrap();
    assert_eq!(applied.label, "Write event LFO");
    assert_eq!(applied.created.len(), 1);
    assert_eq!(entries(doc), before + 1);
    let lane = lanes(doc);
    let points = &lane[0].points;
    for tick in (0..=960).step_by(120) {
        let wanted = lfo.value_at(f64::from(tick));
        let got = curve_value(points, f64::from(tick));
        assert!(
            (got - wanted).abs() < 1e-5,
            "tick {tick}: {got} != {wanted}"
        );
    }
    assert!((curve_value(points, 240.0) - 0.75).abs() < 1e-5);
    assert!((curve_value(points, 720.0) - 0.25).abs() < 1e-5);

    // A second range leaves what lies before it alone.
    let captured = points.clone();
    doc.dispatch(json(&generate(captured.clone(), 960, 1920)), None)
        .unwrap();
    let longer = lanes(doc)[0].points.clone();
    assert_eq!(lanes(doc)[0].id, lane[0].id);
    for tick in (0..960).step_by(60) {
        let old = curve_value(&captured, f64::from(tick));
        assert!((curve_value(&longer, f64::from(tick)) - old).abs() < 1e-5);
    }
    assert!((curve_value(&longer, 1200.0) - 0.75).abs() < 1e-5);

    // A stale capture, a backward range and a range past the pattern are refused.
    let project = doc.project().clone();
    assert!(doc.dispatch(generate(captured, 0, 960), None).is_err());
    assert!(
        doc.dispatch(generate(longer.clone(), 960, 960), None)
            .is_err()
    );
    assert!(
        doc.dispatch(generate(longer.clone(), 0, MAX_PATTERN_TICKS + 960), None)
            .is_err()
    );
    assert_eq!(doc.project(), &project);

    doc.undo().unwrap();
    assert_eq!(lanes(doc), lane);
    doc.undo().unwrap();
    assert!(lanes(doc).is_empty());
}
