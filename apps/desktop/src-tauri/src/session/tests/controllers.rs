//! Controllers against the real session: links moving the project through
//! checked commands, learning, multi-link, pickup, note recording, the note
//! log and the commands that write recorded takes. Hardware messages are
//! handed in as the MIDI worker would hand them; no port is opened.
use std::time::{Duration, Instant};

use super::Rig;
use crate::session::controller_capture::{AutomationLane, TakenNote};
use crate::session::controllers::{recorded_automation_command, recorded_notes_command};
use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
use windfall_ipc::{ControllerSettings, PlayMode};
use windfall_project::{
    AutomationTarget, ChannelId, ChannelPatch, ClipContent, Command, ControlMap, ControlSource,
    ControllerLink, Document, LinkTarget, MAX_GAIN, MidiControl, PatternId,
};

fn cc_source(number: u8) -> ControlSource {
    ControlSource::Midi {
        device: None,
        channel: Some(1),
        control: MidiControl::Cc { number },
    }
}

fn link(number: u8, target: AutomationTarget) -> ControllerLink {
    ControllerLink {
        source: cc_source(number),
        target: LinkTarget::Param { target },
        map: ControlMap::default(),
    }
}

/// A controller message on MIDI channel 1 of the main input. Answers
/// whether the session took it away from note audition.
fn cc(rig: &Rig, number: u8, value: u8) -> bool {
    rig.session.controller_message(InputMessage {
        device: 0,
        message: MidiMessage::Controller {
            channel: 0,
            number,
            value,
        },
        at: Instant::now(),
        stamp_micros: 0,
    })
}

fn pump(rig: &Rig) {
    rig.session.controllers_pump(Instant::now());
}

fn volume(rig: &Rig, channel: ChannelId) -> f32 {
    rig.project().channel(channel).unwrap().volume
}

fn links(rig: &Rig) -> Vec<ControllerLink> {
    rig.project().settings.controller_links
}

fn history(rig: &Rig) -> usize {
    rig.session.document_snapshot().history.entries.len()
}

fn set_links(rig: &Rig, links: Vec<ControllerLink>) {
    rig.session
        .dispatch(Command::SetControllerLinks { links }, None)
        .unwrap();
    // The hub reads the project's links on its next turn.
    pump(rig);
}

#[test]
fn a_linked_controller_moves_its_target_as_one_undoable_gesture() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let before = volume(&rig, channel);
    // Not linked yet: the message is left to note audition and moves nothing.
    assert!(!cc(&rig, 7, 0));
    pump(&rig);
    assert_eq!(volume(&rig, channel), before);

    set_links(
        &rig,
        vec![link(7, AutomationTarget::ChannelVolume { channel })],
    );
    let entries = history(&rig);
    assert!(cc(&rig, 7, 0));
    pump(&rig);
    assert_eq!(volume(&rig, channel), 0.0);
    assert!(cc(&rig, 7, 127));
    pump(&rig);
    assert_eq!(volume(&rig, channel), MAX_GAIN);
    // Several messages between two turns land as the last one.
    cc(&rig, 7, 10);
    cc(&rig, 7, 64);
    pump(&rig);
    let half = volume(&rig, channel);
    assert!(half > 0.0 && half < MAX_GAIN);
    // The whole movement is one step, and undo takes it back.
    assert_eq!(history(&rig), entries + 1);
    rig.session.undo().unwrap();
    assert_eq!(volume(&rig, channel), before);
    let state = rig.session.controller_state();
    assert_eq!(state.last_input, Some(cc_source(7)));
    assert_eq!(state.error, None);
}

#[test]
fn pickup_holds_a_linked_target_until_the_hardware_reaches_it() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let target = AutomationTarget::ChannelVolume { channel };
    rig.session
        .controller_settings_set(ControllerSettings {
            pickup: true,
            ..ControllerSettings::default()
        })
        .unwrap();
    set_links(&rig, vec![link(7, target)]);
    let before = volume(&rig, channel);
    assert!(before > 0.0 && before < MAX_GAIN);
    cc(&rig, 7, 0);
    pump(&rig);
    assert_eq!(volume(&rig, channel), before, "held below the value");
    assert_eq!(rig.session.controller_state().waiting, vec![target]);
    cc(&rig, 7, 127);
    pump(&rig);
    assert_eq!(volume(&rig, channel), MAX_GAIN, "crossing takes hold");
    assert!(rig.session.controller_state().waiting.is_empty());
    // The mouse moves the control away: the hardware has to come to it again.
    rig.session
        .dispatch(
            Command::UpdateChannel {
                id: channel,
                patch: ChannelPatch {
                    volume: Some(0.1),
                    ..ChannelPatch::default()
                },
            },
            None,
        )
        .unwrap();
    cc(&rig, 7, 126);
    pump(&rig);
    assert_eq!(volume(&rig, channel), 0.1);
    // The setting is remembered across a restart.
    let rig = rig.restart();
    assert!(rig.session.controller_state().settings.pickup);
}

#[test]
fn learning_links_the_next_control_that_moves_and_undo_removes_the_link() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let target = AutomationTarget::ChannelPan { channel };
    let generation = rig.session.controller_state().generation;
    assert!(
        rig.session
            .controller_learn(Vec::new(), false, false, generation)
            .is_err()
    );
    assert!(
        rig.session
            .controller_learn(
                vec![LinkTarget::Param { target }],
                false,
                false,
                generation + 1
            )
            .is_err(),
        "a request for another document is refused"
    );
    let missing = AutomationTarget::ChannelPan {
        channel: ChannelId(999_999),
    };
    assert!(
        rig.session
            .controller_learn(
                vec![LinkTarget::Param { target: missing }],
                false,
                false,
                generation
            )
            .is_err()
    );
    let state = rig
        .session
        .controller_learn(vec![LinkTarget::Param { target }], false, false, generation)
        .unwrap();
    assert_eq!(state.learn.unwrap().linked, 0);
    let entries = history(&rig);
    // While learning, the control is taken and moves nothing.
    assert!(cc(&rig, 20, 64));
    pump(&rig);
    assert_eq!(links(&rig), vec![link(20, target)]);
    assert_eq!(rig.session.controller_state().learn, None);
    assert_eq!(history(&rig), entries + 1);
    assert_eq!(rig.project().channel(channel).unwrap().pan, 0.0);
    // The link works from the next message on.
    cc(&rig, 20, 127);
    pump(&rig);
    assert_eq!(rig.project().channel(channel).unwrap().pan, 1.0);
    rig.session.undo().unwrap();
    rig.session.undo().unwrap();
    assert!(links(&rig).is_empty());
    // A link kept for every project cannot name this project's channel.
    assert!(
        rig.session
            .controller_learn(vec![LinkTarget::Param { target }], true, false, generation)
            .is_err()
    );
}

#[test]
fn multi_link_gives_tweaked_controls_to_hardware_in_the_order_they_were_tweaked() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let generation = rig.session.controller_state().generation;
    rig.session
        .controller_learn(Vec::new(), false, true, generation)
        .unwrap();
    let tweak = |patch: ChannelPatch| {
        rig.session
            .dispatch(Command::UpdateChannel { id: channel, patch }, None)
            .unwrap();
    };
    tweak(ChannelPatch {
        pan: Some(0.25),
        ..ChannelPatch::default()
    });
    tweak(ChannelPatch {
        volume: Some(0.5),
        ..ChannelPatch::default()
    });
    // Tweaking one control twice does not list it twice.
    tweak(ChannelPatch {
        pan: Some(0.5),
        ..ChannelPatch::default()
    });
    let pan = AutomationTarget::ChannelPan { channel };
    let level = AutomationTarget::ChannelVolume { channel };
    let state = rig.session.controller_state();
    assert_eq!(
        state.learn.unwrap().targets,
        vec![
            LinkTarget::Param { target: pan },
            LinkTarget::Param { target: level }
        ]
    );
    assert_eq!(state.last_tweaked, Some(pan));
    let entries = history(&rig);
    cc(&rig, 30, 1);
    // The same hardware control again is not a second control.
    cc(&rig, 30, 2);
    cc(&rig, 31, 1);
    cc(&rig, 32, 1);
    pump(&rig);
    assert_eq!(links(&rig), vec![link(30, pan), link(31, level)]);
    // The pass is one undo step, and stays open until it is ended.
    assert_eq!(history(&rig), entries + 1);
    assert_eq!(rig.session.controller_state().learn.unwrap().linked, 2);
    assert_eq!(rig.session.controller_learn_cancel().learn, None);
    rig.session.undo().unwrap();
    assert!(links(&rig).is_empty());
}

#[test]
fn a_link_leaves_with_its_channel_and_the_hub_stops_taking_its_control() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    set_links(
        &rig,
        vec![link(7, AutomationTarget::ChannelVolume { channel })],
    );
    assert!(cc(&rig, 7, 5));
    pump(&rig);
    rig.session
        .dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert!(links(&rig).is_empty());
    pump(&rig);
    assert!(!cc(&rig, 7, 9), "no link, so the message is not taken");
    rig.session.undo().unwrap();
    pump(&rig);
    assert_eq!(links(&rig).len(), 1);
    assert!(cc(&rig, 7, 9));
    // A new project forgets what the old one's hub held.
    rig.session.project_new().unwrap();
    pump(&rig);
    assert!(!cc(&rig, 7, 9));
    assert_eq!(rig.session.controller_state().score_log.notes, 0);
}

#[test]
fn notes_played_while_armed_and_playing_are_recorded_into_the_pattern_in_one_step() {
    let mut rig = Rig::new();
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    rig.session
        .controller_settings_set(ControllerSettings {
            record_quantize_ticks: 240,
            ..ControllerSettings::default()
        })
        .unwrap();
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(PlayMode::Pattern),
            pattern: Some(pattern),
            ..Default::default()
        })
        .unwrap();
    let notes_of = |rig: &Rig| -> Vec<(u32, u32, u8)> {
        let project = rig.project();
        let found = project.pattern(pattern).unwrap();
        found
            .lanes
            .iter()
            .filter(|lane| lane.channel == channel)
            .flat_map(|lane| &lane.notes)
            .map(|note| (note.start, note.length, note.key))
            .collect()
    };
    let before = notes_of(&rig);
    let ms = Duration::from_millis;
    let start = Instant::now();
    let idle = |rig: &Rig, at: Instant| {
        let playhead = rig.session.controller().playhead();
        rig.session.controller_idle(at, &playhead);
    };

    // Not armed: playing along records nothing.
    rig.session.transport_play().unwrap();
    rig.run(480);
    idle(&rig, start);
    rig.session
        .controller_played(channel, 72, Some(0.5), start + ms(10));
    rig.session
        .controller_played(channel, 72, None, start + ms(20));
    assert_eq!(rig.session.controller_state().note_record.pending, 0);

    assert!(rig.session.note_record_arm(true).note_record.armed);
    let entries = history(&rig);
    // The playhead reads about 19 ticks at `start`; 120 bpm is 1920 ticks a
    // second, so 100 ms later is near tick 211 and snaps to the step at 240.
    rig.session
        .controller_played(channel, 101, Some(0.8), start + ms(100));
    rig.session
        .controller_played(channel, 101, None, start + ms(225));
    // Held when playback stops: ended there.
    rig.session
        .controller_played(channel, 103, Some(1.0), start + ms(250));
    assert_eq!(rig.session.controller_state().note_record.pending, 2);
    idle(&rig, start + ms(290));
    rig.session.transport_stop();
    rig.run(480);
    idle(&rig, start + ms(300));
    pump(&rig);

    let mut recorded = notes_of(&rig);
    recorded.retain(|note| !before.contains(note));
    let tempo = rig.project().settings.tempo_bpm;
    let rate = tempo * 960.0 / 60.0;
    let first = recorded
        .iter()
        .find(|note| note.2 == 101)
        .expect("the note");
    assert_eq!(first.0 % 240, 0, "snapped to the grid");
    assert_eq!(first.1, (0.125 * rate).round() as u32);
    let held = recorded
        .iter()
        .find(|note| note.2 == 103)
        .expect("the held note");
    assert!(held.1 >= 1);
    assert_eq!(recorded.len(), 2);
    assert_eq!(history(&rig), entries + 1);
    assert_eq!(rig.session.controller_state().note_record.pending, 0);
    rig.session.undo().unwrap();
    assert_eq!(notes_of(&rig), before);
}

#[test]
fn the_note_log_is_always_on_and_a_dump_can_be_undone_and_repeated() {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    let generation = rig.session.controller_state().generation;
    assert!(
        rig.session
            .score_log_dump(pattern, None, generation)
            .is_err(),
        "nothing played yet"
    );
    let start = Instant::now() - Duration::from_secs(2);
    let ms = Duration::from_millis;
    // Keys the default pattern does not use.
    rig.session
        .controller_played(channel, 101, Some(0.6), start);
    rig.session
        .controller_played(channel, 101, None, start + ms(250));
    rig.session
        .controller_played(channel, 103, Some(0.6), start + ms(500));
    rig.session
        .controller_played(channel, 103, None, start + ms(600));
    // The audition path logs too.
    rig.session.audition_note_on(channel, 105, 0.7);
    rig.session.audition_note_off(channel, 105);
    assert_eq!(rig.session.controller_state().score_log.notes, 3);

    let notes_of = |rig: &Rig, key: u8| -> Vec<(u32, u32)> {
        let project = rig.project();
        let found = project.pattern(pattern).unwrap();
        found
            .lanes
            .iter()
            .filter(|lane| lane.channel == channel)
            .flat_map(|lane| &lane.notes)
            .filter(|note| note.key == key)
            .map(|note| (note.start, note.length))
            .collect()
    };
    let rate = rig.project().settings.tempo_bpm * 960.0 / 60.0;
    let entries = history(&rig);
    rig.session
        .score_log_dump(pattern, None, generation)
        .unwrap();
    assert_eq!(history(&rig), entries + 1);
    let low = notes_of(&rig, 101);
    let next = notes_of(&rig, 103);
    assert_eq!(
        (low.len(), next.len(), notes_of(&rig, 105).len()),
        (1, 1, 1)
    );
    assert_eq!(low[0], (0, (0.25 * rate).round() as u32));
    assert_eq!(next[0].0, (0.5 * rate).round() as u32);

    // Undo takes the notes out and leaves the log, so it can be dumped again.
    rig.session.undo().unwrap();
    assert!(notes_of(&rig, 101).is_empty() && notes_of(&rig, 105).is_empty());
    assert_eq!(rig.session.controller_state().score_log.notes, 3);
    rig.session
        .score_log_dump(pattern, None, generation)
        .unwrap();
    assert_eq!(notes_of(&rig, 101), low);
    // A window keeps only what was played inside it.
    rig.session.undo().unwrap();
    rig.session
        .score_log_dump(pattern, Some(1.0), generation)
        .unwrap();
    assert!(notes_of(&rig, 101).is_empty() && notes_of(&rig, 103).is_empty());
    assert_eq!(notes_of(&rig, 105).len(), 1);
    assert_eq!(
        notes_of(&rig, 105)[0].0,
        0,
        "the window starts at its first note"
    );
    assert!(
        rig.session
            .score_log_dump(PatternId(u32::MAX), None, generation)
            .is_err()
    );
    assert!(
        rig.session
            .score_log_dump(pattern, None, generation + 1)
            .is_err()
    );
    assert_eq!(rig.session.score_log_clear().score_log.notes, 0);
    assert!(
        rig.session
            .score_log_dump(pattern, None, generation)
            .is_err()
    );
}

#[test]
fn recorded_movements_become_a_new_automation_clip_then_overdub_its_curve() {
    let rig = Rig::new();
    let mut doc = Document::new(rig.project());
    let original = doc.project().clone();
    let lanes = vec![AutomationLane {
        target: AutomationTarget::Tempo,
        before: 0.2,
        samples: vec![(960.0, 0.3), (1_920.0, 0.4)],
    }];
    let command = recorded_automation_command(doc.project(), &lanes).unwrap();
    let applied = doc.dispatch(command, None).unwrap();
    assert_eq!(applied.label, "Record automation");
    let automation = doc.project().automations.last().unwrap().clone();
    assert_eq!(automation.target, AutomationTarget::Tempo);
    let points: Vec<(u32, f32)> = automation
        .points
        .iter()
        .map(|point| (point.tick, point.value))
        .collect();
    // From what the control held, a jump where the movement began, and the
    // last movement carried on.
    assert_eq!(points, [(0, 0.2), (960, 0.2), (960, 0.3), (1_920, 0.4)]);
    // Its clip is on a track that exists, from the start of the song.
    let clip = doc
        .project()
        .playlist
        .clips
        .iter()
        .find(|clip| {
            clip.content
                == ClipContent::Automation {
                    automation: automation.id,
                }
        })
        .expect("the clip of the new automation");
    assert_eq!(clip.start, 0);
    assert!(clip.length >= 1_920);
    assert!(
        doc.project()
            .playlist
            .tracks
            .iter()
            .any(|track| track.id == clip.track)
    );
    doc.project().check().unwrap();

    // A second pass under that clip writes into the same curve and leaves
    // the rest of it alone.
    let again = vec![AutomationLane {
        target: AutomationTarget::Tempo,
        before: 0.4,
        samples: vec![(1_200.0, 0.9), (1_500.0, 0.8)],
    }];
    let command = recorded_automation_command(doc.project(), &again).unwrap();
    doc.dispatch(command, None).unwrap();
    assert_eq!(
        doc.project().automations.len(),
        original.automations.len() + 1
    );
    let curve = &doc.project().automations.last().unwrap().points;
    let at = |tick: f64| windfall_project::curve_value(curve, tick);
    assert_eq!(at(500.0), 0.2);
    assert!((at(1_200.0) - 0.9).abs() < 1e-5);
    assert!((at(1_500.0) - (0.3 + 0.1 * (1_500.0 - 960.0) / 960.0)).abs() < 1e-4);
    assert_eq!(at(5_000.0), 0.4);
    doc.undo().unwrap();
    doc.undo().unwrap();
    // Undo restores everything but the id counter, which never goes back.
    let mut undone = doc.project().clone();
    undone.next_id = original.next_id;
    assert_eq!(undone, original);
    // Nothing recorded, or a control that is gone, writes nothing.
    assert!(recorded_automation_command(doc.project(), &[]).is_none());
    let gone = vec![AutomationLane {
        target: AutomationTarget::ChannelPan {
            channel: ChannelId(999_999),
        },
        before: 0.5,
        samples: vec![(0.0, 0.1)],
    }];
    assert!(recorded_automation_command(doc.project(), &gone).is_none());
}

#[test]
fn notes_recorded_in_song_mode_get_a_clip_of_their_pattern_when_none_is_playing() {
    let rig = Rig::new();
    let mut doc = Document::new(rig.project());
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    // Take the pattern off the playlist, so no clip of it is under the notes.
    let placed: Vec<_> = doc
        .project()
        .playlist
        .clips
        .iter()
        .filter(|clip| clip.content == ClipContent::Pattern { pattern })
        .map(|clip| clip.id)
        .collect();
    if !placed.is_empty() {
        doc.dispatch(Command::RemoveClips { clips: placed }, None)
            .unwrap();
    }
    let bar = doc.project().settings.time_signature.ticks_per_bar();
    let note = |start: f64, key: u8| TakenNote {
        channel,
        pattern,
        mode: PlayMode::Song,
        start,
        length: 240.0,
        key,
        velocity: 0.9,
        expression: Default::default(),
    };
    let taken = [
        note(f64::from(bar) * 2.0 + 480.0, 61),
        note(f64::from(bar) * 2.0 + 960.0, 63),
    ];
    let count = |doc: &Document, key: u8| {
        let found = doc.project().pattern(pattern).unwrap();
        found
            .lanes
            .iter()
            .flat_map(|lane| &lane.notes)
            .filter(|note| note.key == key)
            .map(|note| note.start)
            .collect::<Vec<u32>>()
    };
    let (had_61, had_63) = (count(&doc, 61).len(), count(&doc, 63).len());
    let command = recorded_notes_command(doc.project(), &taken, 0).unwrap();
    let applied = doc.dispatch(command, None).unwrap();
    assert_eq!(applied.label, "Record notes");
    doc.project().check().unwrap();
    // The new clip starts on the bar the first note is in and covers both.
    let clip = doc
        .project()
        .playlist
        .clips
        .iter()
        .find(|clip| clip.content == ClipContent::Pattern { pattern })
        .expect("a clip of the pattern");
    assert_eq!(clip.start, bar * 2);
    assert!(clip.length >= bar);
    assert!(
        doc.project()
            .playlist
            .tracks
            .iter()
            .any(|track| track.id == clip.track)
    );
    // The notes sit in the pattern where the clip plays them.
    assert_eq!(count(&doc, 61).len(), had_61 + 1);
    assert!(count(&doc, 61).contains(&480));
    assert_eq!(count(&doc, 63).len(), had_63 + 1);
    assert!(count(&doc, 63).contains(&960));

    // With the clip there, a later pass goes through it: no second clip.
    let clips = doc.project().playlist.clips.len();
    let more = [note(f64::from(bar) * 2.0 + 240.0, 65)];
    let command = recorded_notes_command(doc.project(), &more, 0).unwrap();
    doc.dispatch(command, None).unwrap();
    assert_eq!(doc.project().playlist.clips.len(), clips);
    assert!(count(&doc, 65).contains(&240));
    // A channel or a pattern that is gone leaves nothing to write.
    let stray = [TakenNote {
        channel: ChannelId(999_999),
        ..note(0.0, 60)
    }];
    assert!(recorded_notes_command(doc.project(), &stray, 0).is_none());
}

#[test]
fn wheel_pressure_and_cc_capture_attach_curves_to_native_note_ids_and_survive_history_and_file() {
    use crate::session::capture_expression::{ExpressionCapture, movement, set};
    use crate::session::controller_capture::{NoteTake, Position};
    use windfall_project::{NoteCurveParameter, NoteExpression};
    let rig = Rig::new();
    let channel = rig.channel(0);
    let pattern = rig.pattern();
    let position = |tick| Position {
        tick,
        mode: PlayMode::Pattern,
        pattern,
        pattern_ticks: Some(3840),
    };
    let mut take = NoteTake::default();
    let mut initial = NoteExpression::default();
    let (_, parameter, value) = movement(MidiMessage::PitchBend {
        channel: 2,
        value: 12288,
    })
    .unwrap();
    set(&mut initial, parameter, value);
    take.note_on_expressed(
        channel,
        110,
        0.8,
        position(100.0),
        ExpressionCapture {
            initial,
            source: Some((1, 2)),
            samples: Vec::new(),
        },
    );
    // Other input/channel movements never change the note's contour.
    take.expression((0, 2), NoteCurveParameter::ModulationX, 120.0, 0.9);
    for (tick, message) in [
        (
            125.0,
            MidiMessage::Controller {
                channel: 2,
                number: 74,
                value: 127,
            },
        ),
        (
            150.0,
            MidiMessage::Controller {
                channel: 2,
                number: 1,
                value: 0,
            },
        ),
        (
            175.0,
            MidiMessage::ChannelPressure {
                channel: 2,
                value: 127,
            },
        ),
        (
            190.0,
            MidiMessage::PitchBend {
                channel: 2,
                value: 0,
            },
        ),
    ] {
        let (_, parameter, value) = movement(message).unwrap();
        take.expression((1, 2), parameter, tick, value);
    }
    take.note_off(channel, 110, position(200.0));
    let notes = take.take();
    let original = rig.project();
    let mut doc = Document::new(original.clone());
    doc.dispatch(
        recorded_notes_command(doc.project(), &notes, 240).unwrap(),
        None,
    )
    .unwrap();
    let recorded = doc.project().pattern(pattern).unwrap();
    let note = recorded
        .lanes
        .iter()
        .flat_map(|lane| &lane.notes)
        .find(|note| note.key == 110)
        .unwrap();
    assert_eq!(note.start, 0); // Quantization translates the note and its relative contours together.
    assert!((note.expression.fine_pitch_cents - 100.0122).abs() < 0.001);
    let curves: Vec<_> = recorded
        .note_curves
        .iter()
        .filter(|curve| curve.note == note.id)
        .collect();
    assert_eq!(curves.len(), 3);
    let x = curves
        .iter()
        .find(|curve| curve.parameter == NoteCurveParameter::ModulationX)
        .unwrap();
    assert_eq!((x.points[1].position, x.points[1].value), (0.25, 1.0));
    let y = curves
        .iter()
        .find(|curve| curve.parameter == NoteCurveParameter::ModulationY)
        .unwrap();
    assert_eq!(
        y.points
            .iter()
            .map(|p| (p.position, p.value))
            .collect::<Vec<_>>(),
        [(0.0, 0.5), (0.5, 0.0), (0.75, 1.0)]
    );
    doc.project().check().unwrap();
    let saved = serde_json::to_string(doc.project()).unwrap();
    let reopened: windfall_project::Project = serde_json::from_str(&saved).unwrap();
    reopened.check().unwrap();
    assert_eq!(&reopened, doc.project());
    doc.undo().unwrap();
    assert_eq!(doc.project().pattern(pattern), original.pattern(pattern));
    doc.redo().unwrap();
    assert_eq!(doc.project().pattern(pattern), reopened.pattern(pattern));
}
