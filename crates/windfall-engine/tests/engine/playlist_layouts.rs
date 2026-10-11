//! What the playlist's layouts do to the sound: independent arrangement
//! layouts, instrument tracks that play one channel, an automation clip's
//! own curve, and an LFO written into a curve, heard live and offline.

use windfall_engine::{RenderOptions, render};
use windfall_ipc::{AutomatedValue, PlayMode};
use windfall_project::arrangement::TrackKind;
use windfall_project::{
    AutomationId, AutomationPoint, AutomationTarget, ChannelId, ClipId, ClipPatch, ClipUpdate,
    Command, CurveLfo, CurveLfoWave, Document, PlaylistTrackId, Project, TrackId, curve_value,
};

use crate::realtime::allocator_calls;
use crate::support::{Rig, left, level};

const RATE: u32 = 48_000;

/// Frames in a tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// Frames of the fade at the start of an audio clip.
const DECLICK: usize = 144;

/// Frames between two looks at the automation.
const GRID: usize = 64;

fn offline(project: &Project, rig: &Rig, block: usize) -> Vec<f32> {
    let options = RenderOptions {
        mode: PlayMode::Song,
        sample_rate: RATE,
        block_frames: block,
        tail_secs: 0.0,
        ..Default::default()
    };
    render(project, &rig.pool, &options, &mut |_| true)
        .samples()
        .to_vec()
}

/// The same project as the callback plays it, for as long as the offline
/// render is.
fn live(project: &Project, rig: &Rig, frames: usize, block: usize) -> Vec<f32> {
    let playing = Rig {
        project: project.clone(),
        pool: rig.pool.clone(),
    };
    playing.play_song(RATE, frames, block)
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}

fn dispatch(doc: &mut Document, command: Command) -> Vec<u32> {
    let created = doc.dispatch(command, None).unwrap().created;
    doc.project().check().unwrap();
    created
}

fn update_clip(id: u32, patch: ClipPatch) -> Command {
    Command::UpdateClips {
        updates: vec![ClipUpdate {
            id: ClipId(id),
            patch,
        }],
    }
}

/// Replaces a curve by one flat point.
fn flat(automation: AutomationId, value: f32) -> Command {
    Command::SetAutomationPoints {
        id: automation,
        points: vec![AutomationPoint {
            tick: 0,
            value,
            curve: 0.0,
            hold: false,
        }],
    }
}

/// One lane holding a second of a steady quarter-scale level from tick 0.
fn one_clip() -> (Rig, PlaylistTrackId, ClipId) {
    let mut rig = Rig::new();
    let lane = rig.playlist_track();
    rig.project.playlist.tracks[0].name = "Lane".into();
    let clip = rig.audio_clip(lane, level(RATE, 0.25, 4.0), TrackId::MASTER, 0, 1_920);
    (rig, lane, clip)
}

#[test]
fn a_copied_layout_plays_its_own_clip_positions_live_and_offline() {
    let (rig, _, clip) = one_clip();
    let mut doc = Document::new(rig.project.clone());
    let created = dispatch(
        &mut doc,
        Command::AddArrangementLayout {
            name: "Late".into(),
            copy: true,
        },
    );
    let (late, copy) = (created[0], created[1]);
    // In the copied layout the clip starts a bar later and is half as long.
    let later = ClipPatch {
        start: Some(3_840),
        length: Some(960),
        ..Default::default()
    };
    dispatch(&mut doc, update_clip(copy, later));
    let late_project = doc.project().clone();
    let first = late_project.playlist.arrangement_book.arrangements[0].id;
    dispatch(&mut doc, Command::SwitchArrangement { id: first });
    let first_project = doc.project().clone();
    assert_ne!(first, late);
    // Both layouts are in the one saved project; only the active id differs.
    assert_eq!(first_project.playlist.clips, late_project.playlist.clips);

    let early = left(&offline(&first_project, &rig, 480));
    assert_eq!(early.len(), 1_920 * TICK);
    assert!(
        early[DECLICK..1_920 * TICK - DECLICK]
            .iter()
            .all(|sample| *sample == 0.25)
    );

    let moved = left(&offline(&late_project, &rig, 480));
    assert_eq!(
        moved.len(),
        (3_840 + 960) * TICK,
        "the layout is as long as its own clips"
    );
    assert!(moved[..3_840 * TICK].iter().all(|sample| *sample == 0.0));
    assert!(
        moved[3_840 * TICK + DECLICK..(3_840 + 960) * TICK - DECLICK]
            .iter()
            .all(|sample| *sample == 0.25)
    );

    // The original clip never moved, and each layout is what a project
    // holding only that layout's clips plays.
    let mut alone = rig.project.clone();
    assert_eq!(alone.playlist.clips[0].id, clip);
    assert!(same_bits(
        &offline(&alone, &rig, 480),
        &offline(&first_project, &rig, 480)
    ));
    alone.playlist.clips[0].start = 3_840;
    alone.playlist.clips[0].length = 960;
    assert!(same_bits(
        &offline(&alone, &rig, 480),
        &offline(&late_project, &rig, 480)
    ));

    // The callback plays each layout as the offline render does.
    for project in [&first_project, &late_project] {
        let rendered = offline(project, &rig, 256);
        let played = live(project, &rig, rendered.len() / 2, 256);
        assert!(same_bits(&played, &rendered));
    }

    // An empty layout is silent, and removing the copied layout brings the
    // first one back with its clip where it was.
    dispatch(
        &mut doc,
        Command::AddArrangementLayout {
            name: "Empty".into(),
            copy: false,
        },
    );
    assert!(offline(doc.project(), &rig, 480).is_empty());
    doc.undo().unwrap();
    dispatch(&mut doc, Command::RemoveArrangement { id: late });
    assert!(
        doc.project()
            .playlist
            .clips
            .iter()
            .all(|clip| clip.id.0 != copy)
    );
    assert!(same_bits(
        &offline(doc.project(), &rig, 480),
        &offline(&first_project, &rig, 480)
    ));
}

/// Two channels a note of each starts the first pattern, a twin pattern
/// that holds only the first channel's note, and three lanes.
struct Band {
    rig: Rig,
    channels: [ChannelId; 2],
    lanes: [PlaylistTrackId; 3],
    clip: ClipId,
}

fn band() -> Band {
    let mut rig = Rig::new();
    let quiet = rig.channel(level(RATE, 0.25, 0.25));
    let loud = rig.channel(level(RATE, 0.5, 0.25));
    rig.note(quiet, 0, 240);
    rig.note(loud, 0, 240);
    rig.note(loud, 480, 240);
    let lanes = [
        rig.playlist_track(),
        rig.playlist_track(),
        rig.playlist_track(),
    ];
    let pattern = rig.first_pattern();
    let clip = rig.clip(lanes[0], pattern, 0, 1_920).id;
    // The same pattern on a plain lane, two bars on.
    rig.clip(lanes[1], pattern, 7_680, 1_920);
    Band {
        rig,
        channels: [quiet, loud],
        lanes,
        clip,
    }
}

#[test]
fn an_instrument_track_plays_only_its_channel_of_a_pattern() {
    let Band {
        mut rig,
        channels: [quiet, loud],
        lanes,
        clip,
    } = band();
    let plain = offline(&rig.project, &rig, 480);
    let plain_left = left(&plain);
    assert_eq!(
        plain_left[1_000], 0.75,
        "unlinked, the pattern plays both channels"
    );

    rig.project
        .playlist
        .arrangement_book
        .linked_tracks
        .insert(lanes[0], TrackKind::Instrument { channel: quiet });
    rig.project.check().unwrap();
    let linked = offline(&rig.project, &rig, 480);
    let heard = left(&linked);
    assert_eq!(heard[1_000], 0.25, "only the linked channel's note");
    assert_eq!(
        heard[480 * TICK + 1_000],
        0.0,
        "the other channel's later note is not played"
    );
    // The same pattern on the plain lane still plays everything.
    assert_eq!(heard[7_680 * TICK + 1_000], 0.75);
    assert_eq!(heard[(7_680 + 480) * TICK + 1_000], 0.5);
    assert_eq!(linked.len(), plain.len(), "the song is as long as it was");

    // It is exactly the song with a pattern that holds only that channel.
    let mut reference = Rig {
        project: rig.project.clone(),
        pool: rig.pool.clone(),
    };
    reference.project.playlist.arrangement_book = Default::default();
    let twin = reference.pattern(16);
    reference.note_in(twin, quiet, 0, 240);
    reference.clip_mut(clip).content = windfall_project::ClipContent::Pattern { pattern: twin };
    assert!(same_bits(&offline(&reference.project, &rig, 480), &linked));

    // Live playback applies the same filter, without touching the allocator.
    let played = live(&rig.project, &rig, linked.len() / 2, 256);
    assert!(same_bits(&played, &offline(&rig.project, &rig, 256)));
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let mut out = vec![0.0; 256 * 2];
    for _ in 0..400 {
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    }

    // Linked to the other channel, the lane plays that one instead.
    rig.project
        .playlist
        .arrangement_book
        .linked_tracks
        .insert(lanes[0], TrackKind::Instrument { channel: loud });
    let other = left(&offline(&rig.project, &rig, 480));
    assert_eq!(other[1_000], 0.5);
    assert_eq!(other[480 * TICK + 1_000], 0.5);

    // An audio clip on an instrument track is not filtered away.
    let tape = rig.audio_clip(
        lanes[0],
        level(RATE, 0.125, 1.0),
        TrackId::MASTER,
        3_840,
        960,
    );
    let with_tape = left(&offline(&rig.project, &rig, 480));
    assert_eq!(with_tape[3_840 * TICK + 1_000], 0.125);
    rig.project.playlist.clips.retain(|clip| clip.id != tape);

    // A link to a channel that is gone plays nothing on that lane.
    rig.project.playlist.arrangement_book.linked_tracks.insert(
        lanes[0],
        TrackKind::Instrument {
            channel: ChannelId(9_999),
        },
    );
    let orphan = left(&offline(&rig.project, &rig, 480));
    assert_eq!(orphan[1_000], 0.0);
    assert_eq!(orphan[7_680 * TICK + 1_000], 0.75);
}

#[test]
fn an_audio_track_plays_its_clips_through_the_linked_mixer_track() {
    // The routing is a document rule: what the engine plays is the mixer
    // track the command left on the clip.
    let (mut rig, lane, clip) = one_clip();
    let quiet = rig.track();
    rig.track_mut(quiet).volume = 0.5;
    rig.track_mut(quiet).name = "Quiet".into();
    let mut doc = Document::new(rig.project.clone());
    let through_master = left(&offline(doc.project(), &rig, 480));
    assert_eq!(through_master[1_000], 0.25);
    dispatch(
        &mut doc,
        Command::LinkTrack {
            track: lane,
            kind: Some(TrackKind::AudioTrack { mixer: quiet }),
        },
    );
    let routed = left(&offline(doc.project(), &rig, 480));
    assert_eq!(
        routed[1_000], 0.125,
        "the clip now passes the linked track's fader"
    );
    let played = live(doc.project(), &rig, routed.len(), 256);
    assert!(same_bits(
        &left(&played),
        &left(&offline(doc.project(), &rig, 256))
    ));
    doc.undo().unwrap();
    assert!(same_bits(
        &left(&offline(doc.project(), &rig, 480)),
        &through_master
    ));
    assert_eq!(doc.project().playlist.clips[0].id, clip);
}

/// Four bars of a steady half-scale level on a mixer track of its own, and
/// a volume automation of that track on a lane of its own.
fn ridden() -> (Rig, TrackId, AutomationId, ClipId) {
    let mut rig = Rig::new();
    let track = rig.track();
    rig.track_mut(track).name = "Ridden".into();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.5, 10.0), track, 0, 15_360);
    let curves = rig.playlist_track();
    for (index, track) in rig.project.playlist.tracks.iter_mut().enumerate() {
        track.name = format!("Lane {index}");
    }
    let automation = rig.automation(AutomationTarget::TrackVolume { track }, &[(0, 0.5)]);
    rig.automation_mut(automation).name = "Ride".into();
    let clip = rig.automation_clip(curves, automation, 0, 15_360);
    (rig, track, automation, clip)
}

/// The gain a volume automation's value of `n` stands for.
fn gain(n: f64) -> f64 {
    2.0 * n * n
}

fn write_lfo(doc: &mut Document, automation: AutomationId, lfo: CurveLfo, range: (u32, u32)) {
    let expected = doc.project().automation(automation).unwrap().clone();
    dispatch(
        doc,
        Command::GenerateAutomationLfo {
            expected,
            start: range.0,
            end: range.1,
            resolution: 60,
            lfo,
        },
    );
}

#[test]
fn an_lfo_written_into_a_curve_moves_the_linked_fader_live_and_offline() {
    let (rig, _, automation, _) = ridden();
    let mut doc = Document::new(rig.project.clone());
    // One cycle a bar, around the middle of the fader, a quarter each way.
    let lfo = CurveLfo {
        wave: CurveLfoWave::Sine,
        period: 3_840,
        phase: 0.0,
        center: 0.5,
        depth: 0.25,
        width: 0.5,
        seed: 1,
    };
    write_lfo(&mut doc, automation, lfo, (0, 15_360));
    let points = doc.project().automation(automation).unwrap().points.clone();
    // One point every 60 ticks, both ends included, plus the retained
    // boundary values of the curve that was there.
    assert!(
        (257..=260).contains(&points.len()),
        "{} points",
        points.len()
    );
    let ideal = |tick: f64| 0.5 + 0.25 * (tick / 3_840.0 * std::f64::consts::TAU).sin();

    let out = left(&offline(doc.project(), &rig, 480));
    assert_eq!(out.len(), 15_360 * TICK);
    let mut largest = 0.0_f64;
    for frame in (DECLICK..out.len() - DECLICK).step_by(397) {
        let tick = frame as f64 / TICK as f64;
        // The engine follows the written points...
        let written = f64::from(curve_value(&points, tick));
        let expected = 0.5 * gain(written);
        assert!(
            (f64::from(out[frame]) - expected).abs() < 2e-4,
            "frame {frame}: {} not {expected}",
            out[frame]
        );
        // ...and the written points are the LFO that was asked for.
        largest = largest.max((written - ideal(tick)).abs());
    }
    assert!(largest < 2e-3, "the curve strays {largest} from the sine");
    // Peaks and troughs land a quarter and three quarters into each bar.
    for bar in 0..4 {
        let top = (bar * 3_840 + 960) * TICK;
        let bottom = (bar * 3_840 + 2_880) * TICK;
        assert!((f64::from(out[top]) - 0.5 * gain(0.75)).abs() < 1e-3);
        assert!((f64::from(out[bottom]) - 0.5 * gain(0.25)).abs() < 1e-3);
    }

    // The callback plays the same fader ride, and reports the value the
    // automation holds while it plays, at each place it is asked.
    let playing = Rig {
        project: doc.project().clone(),
        pool: rig.pool.clone(),
    };
    let (mut processor, controller) = playing.song_processor(RATE);
    assert!(controller.frame().automated.is_empty());
    controller.play();
    let mut played = Vec::new();
    let mut calls = 0;
    for stop in [960_usize, 2_880, 4_800, 9_000] {
        let frames = stop * TICK - played.len() / 2;
        let mut chunk = vec![0.0; frames * 2];
        for block in chunk.chunks_mut(256 * 2) {
            calls += allocator_calls(|| processor.process(block));
        }
        played.extend_from_slice(&chunk);
        let reported = controller.frame().automated;
        let [
            AutomatedValue {
                automation: id,
                value,
            },
        ] = reported[..]
        else {
            panic!("one automation is playing: {reported:?}");
        };
        assert_eq!(id, automation);
        // The report is of the last look at the curve, at most a grid back.
        let tolerance =
            0.25 * std::f64::consts::TAU * (2 * GRID) as f64 / (3_840 * TICK) as f64 + 2e-3;
        assert!(
            (f64::from(value) - ideal(stop as f64)).abs() < tolerance,
            "tick {stop}: reported {value}, the LFO is at {}",
            ideal(stop as f64)
        );
    }
    assert_eq!(calls, 0, "playing an LFO curve never allocates");
    let played = left(&played);
    for frame in (DECLICK..played.len()).step_by(397) {
        assert!((played[frame] - out[frame]).abs() < 1e-6, "frame {frame}");
    }

    // Undo takes the LFO out again: the fader sits at the one stored point.
    doc.undo().unwrap();
    let flat = left(&offline(doc.project(), &rig, 480));
    assert!(
        flat[DECLICK..15_000 * TICK]
            .iter()
            .all(|sample| *sample == 0.25)
    );
}

#[test]
fn held_lfo_shapes_step_and_a_ranged_write_keeps_the_curve_outside() {
    let (rig, _, automation, _) = ridden();
    let mut doc = Document::new(rig.project.clone());
    // A pulse over the second and third bars only: full for the first
    // quarter of each beat-long cycle, shut for the rest.
    let lfo = CurveLfo {
        wave: CurveLfoWave::Square,
        period: 960,
        phase: 0.0,
        center: 0.5,
        depth: 0.5,
        width: 0.25,
        seed: 1,
    };
    write_lfo(&mut doc, automation, lfo, (3_840, 11_520));
    let out = left(&offline(doc.project(), &rig, 480));
    // Before and after the written range the stored value still holds.
    assert!(
        out[DECLICK..3_840 * TICK - GRID]
            .iter()
            .all(|sample| *sample == 0.25)
    );
    assert!(
        out[11_520 * TICK + 2 * GRID..15_000 * TICK]
            .iter()
            .all(|sample| *sample == 0.25)
    );
    for cycle in 4..12 {
        let start = cycle * 960 * TICK;
        let high = &out[start + 2 * GRID..start + 240 * TICK - 2 * GRID];
        let low = &out[start + 240 * TICK + 2 * GRID..start + 960 * TICK - 2 * GRID];
        assert!(
            high.iter().all(|sample| *sample == 1.0),
            "cycle {cycle} is open"
        );
        assert!(
            low.iter().all(|sample| *sample == 0.0),
            "cycle {cycle} is shut"
        );
    }
    let rendered = offline(doc.project(), &rig, 256);
    let played = live(doc.project(), &rig, rendered.len() / 2, 256);
    for (frame, (a, b)) in left(&played).iter().zip(left(&rendered)).enumerate() {
        assert!((a - b).abs() < 1e-6, "frame {frame}");
    }

    // Sample and hold is the same curve for the same seed, another for another.
    let held = |seed: u32| {
        let mut doc = Document::new(rig.project.clone());
        let lfo = CurveLfo {
            wave: CurveLfoWave::SampleHold,
            period: 480,
            seed,
            ..lfo
        };
        write_lfo(&mut doc, automation, lfo, (0, 7_680));
        left(&offline(doc.project(), &rig, 480))
    };
    let (first, again, other) = (held(7), held(7), held(8));
    assert!(same_bits(&first, &again));
    assert!(!same_bits(&first, &other));
    // Within one cycle the value is held.
    let cycle = &first[3 * 480 * TICK + 2 * GRID..4 * 480 * TICK - 2 * GRID];
    assert!(cycle.iter().all(|sample| *sample == cycle[0]));
}

#[test]
fn a_unique_automation_clip_plays_its_own_curve() {
    let (mut rig, _, automation, first) = ridden();
    // The same curve shown twice: over the first bar and over the third.
    rig.clip_mut(first).length = 3_840;
    let curves = rig.project.playlist.tracks[1].id;
    let second = rig.automation_clip(curves, automation, 7_680, 3_840);
    let mut doc = Document::new(rig.project.clone());
    let shared = left(&offline(doc.project(), &rig, 480));
    assert_eq!(shared[1_000 * TICK], 0.25);
    assert_eq!(shared[9_000 * TICK], 0.25);

    let copy = AutomationId(dispatch(&mut doc, Command::MakeUnique { clip: second })[0]);
    assert!(
        same_bits(&left(&offline(doc.project(), &rig, 480)), &shared),
        "a copy sounds the same"
    );
    // Open the copy's fader fully: only the third bar changes.
    dispatch(&mut doc, flat(copy, 1.0));
    let edited = left(&offline(doc.project(), &rig, 480));
    assert_eq!(
        edited[1_000 * TICK],
        0.25,
        "the clip that kept the original curve"
    );
    assert_eq!(edited[9_000 * TICK], 1.0, "the clip that was made unique");
    let rendered = offline(doc.project(), &rig, 256);
    let played = live(doc.project(), &rig, rendered.len() / 2, 256);
    for (frame, (a, b)) in left(&played).iter().zip(left(&rendered)).enumerate() {
        assert!((a - b).abs() < 1e-6, "frame {frame}");
    }
    // The original curve still drives the first clip when it is edited.
    dispatch(&mut doc, flat(automation, 0.25));
    let both = left(&offline(doc.project(), &rig, 480));
    assert_eq!(both[1_000 * TICK], 0.0625);
    assert_eq!(both[9_000 * TICK], 1.0);
}
