//! Public playback acceptance for event automation stored inside a pattern:
//! measured audio in pattern and song mode, live against offline, every
//! block size, and no allocation on the callback.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;
#[allow(dead_code)]
#[path = "../src/test_alloc.rs"]
mod test_alloc;

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

use support::{Rig, left, level, right, run};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{
    AutomationId, AutomationPoint, AutomationTarget, ChannelId, PatternEventLane, PatternId,
    TrackId, curve_value,
};

const RATE: u32 = 48_000;
/// Frames in a tick at 120 bpm and 48 kHz.
const TICK: usize = 25;
/// Frames between two looks at the automation.
const GRID: usize = 64;
/// One pass of a sixteen-step pattern, in ticks and in frames.
const PASS_TICKS: u32 = 3_840;
const PASS: usize = PASS_TICKS as usize * TICK;

/// The gain a volume lane's value of `n` stands for.
fn gain(n: f32) -> f32 {
    2.0 * n * n
}

fn points(points: &[(u32, f32)]) -> Vec<AutomationPoint> {
    points
        .iter()
        .map(|&(tick, value)| AutomationPoint {
            tick,
            value,
            curve: 0.0,
            hold: false,
        })
        .collect()
}

/// Gives a pattern an event lane and returns the lane's id.
fn lane(
    rig: &mut Rig,
    pattern: PatternId,
    target: AutomationTarget,
    curve: &[(u32, f32)],
) -> AutomationId {
    let id = AutomationId(rig.project.next_id);
    rig.project.next_id += 1;
    rig.pattern_mut(pattern).events.push(PatternEventLane {
        id,
        target,
        points: points(curve),
    });
    rig.project.check().expect("the project is one a file could hold");
    id
}

/// A triangle of a volume curve: half gain, up to +6 dB, down to -18 dB.
const SHAPE: [(u32, f32); 3] = [(0, 0.5), (1_920, 1.0), (3_840, 0.25)];

/// A channel that holds a steady 0.5 for exactly one pass of the first
/// pattern, and a lane that moves its volume.
fn held_pattern() -> (Rig, ChannelId, AutomationId) {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let channel = rig.channel(level(RATE, 0.5, 2.0));
    rig.note(channel, 0, PASS_TICKS);
    let pattern = rig.first_pattern();
    let id = lane(
        &mut rig,
        pattern,
        AutomationTarget::ChannelVolume { channel },
        &SHAPE,
    );
    (rig, channel, id)
}

/// What a channel holding 0.5 comes out at on `frame` of a pattern that
/// plays around and around with [`SHAPE`] on its volume.
fn shaped(frame: usize) -> f32 {
    let tick = (frame % PASS) as f64 / TICK as f64;
    0.5 * gain(curve_value(&points(&SHAPE), tick))
}

/// Frames a pass can be measured on: clear of the grid step either side of
/// where the pattern goes around, on which the curve's jump is spread.
fn measured() -> impl Iterator<Item = usize> {
    (0..PASS * 2)
        .step_by(997)
        .filter(|frame| (GRID * 2..PASS - GRID * 2).contains(&(frame % PASS)))
}

#[test]
fn a_pattern_playing_by_itself_follows_its_event_lane_around_every_pass() {
    let (rig, _, id) = held_pattern();
    let stored = rig.project.clone();
    let (mut processor, controller) = rig.processor(RATE);
    assert!(controller.frame().automated.is_empty());
    controller.play();
    let out = run(&mut processor, PASS * 2, 480);
    let (l, r) = (left(&out), right(&out));
    for frame in measured() {
        let wanted = shaped(frame);
        assert!(
            (l[frame] - wanted).abs() < 2e-3 && (r[frame] - wanted).abs() < 2e-3,
            "frame {frame}: {} and {} are not {wanted}",
            l[frame],
            r[frame]
        );
    }
    // It really moved: half gain at the top, +6 dB in the middle, -18 dB at the end.
    assert!((l[GRID * 4] - 0.25).abs() < 0.01);
    assert!((l[PASS / 2] - 1.0).abs() < 0.01);
    assert!((l[PASS - GRID * 4] - 0.0625).abs() < 0.01);
    // The second pass starts over from the top of the curve.
    assert!((l[PASS + GRID * 4] - 0.25).abs() < 0.01);

    // The UI is told which lane has the control and what it gave it.
    let automated = controller.frame().automated;
    assert_eq!(automated.len(), 1);
    assert_eq!(automated[0].automation, id);
    assert!((0.0..=1.0).contains(&automated[0].value));
    // Nothing a lane does is written to the project.
    assert_eq!(rig.project, stored);
}

#[test]
fn a_pattern_without_a_lane_leaves_the_control_alone_and_stopping_lets_go() {
    let (mut rig, channel, id) = held_pattern();
    let plain = rig.pattern(16);
    rig.note_in(plain, channel, 0, PASS_TICKS);
    let (mut processor, controller) = rig.processor(RATE);

    // The pattern with no lane plays the channel at its stored volume.
    controller.set_transport(TransportPatch {
        pattern: Some(plain),
        ..TransportPatch::default()
    });
    controller.play();
    let out = left(&run(&mut processor, PASS / 2, 480));
    assert!(out[GRID * 4..].iter().all(|sample| *sample == 0.5));
    assert!(controller.frame().automated.is_empty());
    controller.stop();
    run(&mut processor, RATE as usize * 3 / 2, 480);

    // The pattern with the lane takes the volume in hand from its first frame.
    controller.set_transport(TransportPatch {
        pattern: Some(rig.first_pattern()),
        ..TransportPatch::default()
    });
    controller.play();
    let out = left(&run(&mut processor, PASS / 2, 480));
    assert!((out[0] - 0.25).abs() < 0.01, "{}", out[0]);
    assert!((out[PASS / 4] - shaped(PASS / 4)).abs() < 2e-3);
    assert_eq!(controller.frame().automated[0].automation, id);

    // Stopped, the lane lets go: a note played by hand comes out at the
    // stored volume once the 100 ms way back is over.
    controller.stop();
    run(&mut processor, RATE as usize * 3 / 2, 480);
    assert!(controller.frame().automated.is_empty());
    controller.note_on(channel, 60, 1.0);
    let out = left(&run(&mut processor, 4_800, 480));
    assert!(out[GRID * 4..].iter().all(|sample| *sample == 0.5));
}

/// A track holding a steady 0.5 for the whole song, and a pattern whose
/// lane moves that track's fader.
fn held_song() -> (Rig, TrackId, PatternId, AutomationId) {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let track = rig.track();
    let audio = rig.playlist_track();
    rig.audio_clip(audio, level(RATE, 0.5, 30.0), track, 0, 30_720);
    let pattern = rig.first_pattern();
    let id = lane(
        &mut rig,
        pattern,
        AutomationTarget::TrackVolume { track },
        &SHAPE,
    );
    (rig, track, pattern, id)
}

/// What the held track comes out at when its fader has the lane's value `n`.
fn faded(n: f32) -> f32 {
    0.5 * gain(n)
}

fn near(out: &[f32], tick: u32, wanted: f32) {
    let frame = tick as usize * TICK;
    assert!(
        (out[frame] - wanted).abs() < 2e-3,
        "tick {tick}: {} is not {wanted}",
        out[frame]
    );
}

#[test]
fn every_clip_of_the_pattern_plays_the_lane_around_from_its_own_offset() {
    let (mut rig, _, pattern, id) = held_song();
    let row = rig.playlist_track();
    // Two and a half passes, starting a quarter of the way into the pattern.
    rig.clip(row, pattern, 7_680, 9_600).offset = 960;
    let curve = points(&SHAPE);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let out = left(&run(&mut processor, 24_000 * TICK, 480));

    // Before the clip nothing has the fader: the track plays as stored.
    near(&out, 3_840, 0.5);
    near(&out, 7_600, 0.5);
    // Inside the clip the place in the pattern is the clip's offset plus
    // the time since its start, around the pattern's length.
    for (tick, place) in [
        (7_800, 1_080.0),
        (8_640, 1_920.0),
        (10_000, 3_280.0),
        // Second time around.
        (10_800, 240.0),
        (12_480, 1_920.0),
        (14_000, 3_440.0),
        // Third, cut off by the clip's end at 17,280.
        (14_600, 200.0),
        (17_000, 2_600.0),
    ] {
        near(&out, tick, faded(curve_value(&curve, place)));
    }
    // After the clip the fader stays where the clip left it.
    let left_at = faded(curve_value(&curve, 2_880.0));
    near(&out, 17_600, left_at);
    near(&out, 23_000, left_at);
    assert_eq!(controller.frame().automated[0].automation, id);
}

#[test]
fn a_clip_ending_on_a_pass_leaves_the_end_of_the_curve_and_muted_clips_count_for_nothing() {
    let (mut rig, _, pattern, _) = held_song();
    let row = rig.playlist_track();
    rig.clip(row, pattern, 3_840, 7_680);
    let muted = rig.clip(row, pattern, 15_360, 3_840).id;
    rig.clip_mut(muted).muted = true;
    let out = left(&rig.play_song(RATE, 24_000 * TICK, 480));
    near(&out, 5_760, faded(1.0));
    near(&out, 9_600, faded(1.0));
    // Two whole passes end on the curve's last value, not back on its first.
    near(&out, 12_000, faded(0.25));
    // The muted clip does not take the fader up again.
    near(&out, 17_280, faded(0.25));
    near(&out, 20_000, faded(0.25));
}

#[test]
fn an_automation_clip_on_a_higher_row_wins_over_the_pattern_and_hands_back() {
    let (mut rig, track, pattern, lane_id) = held_song();
    // Rows are ranked from the top: the automation clip sits above.
    let top = rig.playlist_track();
    let row = rig.playlist_track();
    rig.clip(row, pattern, 0, 11_520);
    let target = AutomationTarget::TrackVolume { track };
    let fixed = rig.automation(target, &[(0, 0.75)]);
    rig.automation_clip(top, fixed, 3_840, 3_840);
    let curve = points(&SHAPE);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let out = left(&run(&mut processor, 2_000 * TICK, 480));
    near(&out, 960, faded(curve_value(&curve, 960.0)));
    assert_eq!(controller.frame().automated[0].automation, lane_id);
    let out = [out, left(&run(&mut processor, 10_000 * TICK, 480))].concat();
    // The automation clip has the fader while it plays...
    near(&out, 4_800, faded(0.75));
    near(&out, 7_000, faded(0.75));
    // ...and the pattern has it again afterwards, at its place in the third pass.
    near(&out, 8_640, faded(curve_value(&curve, 960.0)));
    near(&out, 9_600, faded(1.0));
}

#[test]
fn live_playback_is_the_same_for_every_block_size_and_matches_offline_render() {
    let (pattern_rig, _, _) = held_pattern();
    let reference = pattern_rig.play(RATE, PASS * 2, 64);
    for block in [1, 7, 480, 1_024] {
        assert_eq!(pattern_rig.play(RATE, PASS * 2, block), reference, "block {block}");
    }
    let options = RenderOptions {
        mode: PlayMode::Pattern,
        sample_rate: RATE,
        pattern_loops: 2,
        tail_secs: 0.0,
        block_frames: 37,
        ..Default::default()
    };
    let audio = render(&pattern_rig.project, &pattern_rig.pool, &options, &mut |_| true);
    assert_eq!(audio.frames(), PASS * 2);
    assert_eq!(audio.samples(), &reference[..]);

    let (mut song_rig, _, pattern, _) = held_song();
    let row = song_rig.playlist_track();
    song_rig.clip(row, pattern, 7_680, 9_600).offset = 960;
    let frames = 30_720 * TICK;
    let reference = song_rig.play_song(RATE, frames, 64);
    for block in [7, 1_024] {
        assert_eq!(song_rig.play_song(RATE, frames, block), reference, "block {block}");
    }
    let options = RenderOptions {
        mode: PlayMode::Song,
        sample_rate: RATE,
        tail_secs: 0.0,
        block_frames: 333,
        ..Default::default()
    };
    let audio = render(&song_rig.project, &song_rig.pool, &options, &mut |_| true);
    assert_eq!(audio.frames(), frames);
    assert_eq!(audio.samples(), &reference[..]);
}

#[test]
fn every_kind_of_target_is_moved_and_the_callback_never_allocates() {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let track = rig.track();
    let bus = rig.track();
    rig.track_mut(track).sends.push(windfall_project::Send {
        target: bus,
        gain: 0.5,
    });
    let channel = rig.channel_on(level(RATE, 0.5, 2.0), track);
    let synth = rig.synth_on(support::plain_synth(), track);
    let effect = rig.effect(
        track,
        windfall_project::EffectParams::Delay(Default::default()),
    );
    rig.note(channel, 0, PASS_TICKS);
    rig.note(synth, 0, PASS_TICKS);
    let pattern = rig.first_pattern();
    let ramp = [(0, 0.2), (1_920, 0.9), (3_840, 0.4)];
    let targets = [
        AutomationTarget::ChannelVolume { channel },
        AutomationTarget::ChannelPan { channel },
        AutomationTarget::TrackVolume { track },
        AutomationTarget::TrackPan { track },
        AutomationTarget::TrackParam { track, param: 0 },
        AutomationTarget::SendGain { track, target: bus },
        AutomationTarget::EffectParam {
            track,
            effect,
            param: 0,
        },
        AutomationTarget::EffectMix { track, effect },
        AutomationTarget::InstrumentParam {
            channel: synth,
            param: 0,
        },
    ];
    let ids: Vec<AutomationId> = targets
        .iter()
        .map(|target| lane(&mut rig, pattern, *target, &ramp))
        .collect();

    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    // Past the first block, in which the plan arrives.
    run(&mut processor, 4_800, 480);
    let mut out = vec![0.0; PASS * 2 * 2];
    let calls = test_alloc::allocator_calls(|| {
        for chunk in out.chunks_mut(441 * 2) {
            processor.process(chunk);
        }
    });
    assert_eq!(calls, 0, "the audio callback allocated or freed");
    assert!(out.iter().all(|sample| sample.is_finite()));
    assert!(out.iter().any(|sample| *sample != 0.0));

    // Every lane has its control in hand, each under its own id.
    let automated = controller.frame().automated;
    let mut reported: Vec<AutomationId> = automated.iter().map(|value| value.automation).collect();
    reported.sort();
    let mut wanted = ids.clone();
    wanted.sort();
    assert_eq!(reported, wanted);

    // The same in the song, with the pattern on the playlist.
    let row = rig.playlist_track();
    rig.clip(row, pattern, 0, PASS_TICKS * 3);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 4_800, 480);
    let calls = test_alloc::allocator_calls(|| {
        for chunk in out.chunks_mut(441 * 2) {
            processor.process(chunk);
        }
    });
    assert_eq!(calls, 0, "the audio callback allocated or freed in the song");
    assert_eq!(controller.frame().automated.len(), ids.len());
}

#[test]
fn a_lane_for_something_the_plan_does_not_have_is_left_out() {
    let (mut rig, channel, _) = held_pattern();
    // A lane whose channel is gone, as a plan compiled mid-edit could see.
    let id = AutomationId(rig.project.next_id);
    rig.project.next_id += 1;
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).events.push(PatternEventLane {
        id,
        target: AutomationTarget::ChannelPan {
            channel: ChannelId(9_999),
        },
        points: points(&[(0, 0.0)]),
    });
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let out = run(&mut processor, PASS / 2, 480);
    let (l, r) = (left(&out), right(&out));
    // The lane that is there still plays, and nothing pans.
    assert!((l[PASS / 4] - shaped(PASS / 4)).abs() < 2e-3);
    assert_eq!(l[PASS / 4], r[PASS / 4]);
    assert_eq!(controller.frame().automated.len(), 1);
    let _ = channel;
}
