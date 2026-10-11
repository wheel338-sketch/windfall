//! The project's panning law and timebase, measured in what plays and in
//! what an export renders.

use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{PanLaw, TrackId};

use crate::realtime::allocator_calls;
use crate::support::{Rig, impulse, level, peak, run, sounding_frames};

const RATE: u32 = 48_000;
/// The level of the steady signal the pan tests play.
const LEVEL: f32 = 0.5;
const PANS: [f32; 7] = [-1.0, -0.6, -0.25, 0.0, 0.25, 0.6, 1.0];

/// Left and right of an interleaved buffer well after anything has settled.
fn settled(pcm: &[f32]) -> (f32, f32) {
    let frame = 6_000;
    (pcm[frame * 2], pcm[frame * 2 + 1])
}

fn assert_sides(actual: (f32, f32), expected: (f32, f32), what: &str) {
    assert!(
        (actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6,
        "{what}: {actual:?}, not {expected:?}"
    );
}

fn scaled(law: PanLaw, pan: f32) -> (f32, f32) {
    let (left, right) = law.gains(pan);
    (LEVEL * left, LEVEL * right)
}

/// A rig that holds a steady signal on one sampler channel for two seconds.
fn steady(law: PanLaw) -> (Rig, windfall_project::ChannelId) {
    let mut rig = Rig::new();
    rig.project.settings.pan_law = law;
    let channel = rig.channel(level(RATE, LEVEL, 1.0));
    rig.note(channel, 0, 3_840);
    (rig, channel)
}

#[test]
fn every_pan_the_engine_applies_follows_the_projects_law() {
    for law in PanLaw::ALL {
        for pan in PANS {
            let what = format!("{law:?} at {pan}");

            // A channel's pan.
            let (mut rig, channel) = steady(law);
            rig.channel_mut(channel).pan = pan;
            assert_sides(
                settled(&rig.play(RATE, 8_000, 64)),
                scaled(law, pan),
                &format!("channel pan, {what}"),
            );

            // A note's pan on a sampler, on top of a centered channel.
            let (mut rig, _) = steady(law);
            rig.project.patterns[0].lanes[0].notes[0].pan = pan;
            assert_sides(
                settled(&rig.play(RATE, 8_000, 64)),
                scaled(law, pan),
                &format!("note pan, {what}"),
            );

            // A mixer track's pan.
            let mut rig = Rig::new();
            rig.project.settings.pan_law = law;
            let track = rig.track();
            let channel = rig.channel_on(level(RATE, LEVEL, 1.0), track);
            rig.note(channel, 0, 3_840);
            rig.track_mut(track).pan = pan;
            assert_sides(
                settled(&rig.play(RATE, 8_000, 64)),
                scaled(law, pan),
                &format!("track pan, {what}"),
            );

            // An audio clip's pan.
            let mut rig = Rig::new();
            rig.project.settings.pan_law = law;
            let lane = rig.playlist_track();
            let clip = rig.audio_clip(lane, level(RATE, LEVEL, 1.0), TrackId::MASTER, 0, 1_920);
            rig.audio_mut(clip, |audio| audio.pan = pan);
            assert_sides(
                settled(&rig.play_song(RATE, 8_000, 64)),
                scaled(law, pan),
                &format!("clip pan, {what}"),
            );
        }
    }
}

#[test]
fn pan_stages_in_series_multiply_and_a_centered_project_sounds_the_same_under_every_law() {
    for law in PanLaw::ALL {
        // Channel and track both panned: each stage applies the law once.
        let mut rig = Rig::new();
        rig.project.settings.pan_law = law;
        let track = rig.track();
        let channel = rig.channel_on(level(RATE, LEVEL, 1.0), track);
        rig.note(channel, 0, 3_840);
        rig.channel_mut(channel).pan = 0.5;
        rig.track_mut(track).pan = -0.25;
        let (channel_left, channel_right) = law.gains(0.5);
        let (track_left, track_right) = law.gains(-0.25);
        assert_sides(
            settled(&rig.play(RATE, 8_000, 64)),
            (
                LEVEL * channel_left * track_left,
                LEVEL * channel_right * track_right,
            ),
            &format!("{law:?} in series"),
        );

        // Nothing panned: every law is unity at the center, through the
        // channel, the track and the master.
        rig.channel_mut(channel).pan = 0.0;
        rig.track_mut(track).pan = 0.0;
        let centered = rig.play(RATE, 8_000, 64);
        rig.project.settings.pan_law = PanLaw::Balance;
        assert_eq!(centered, rig.play(RATE, 8_000, 64), "{law:?} at the center");
    }
}

#[test]
fn the_balance_law_is_what_projects_played_with_before_the_setting() {
    let (mut rig, channel) = steady(PanLaw::default());
    assert_eq!(rig.project.settings.pan_law, PanLaw::Balance);
    rig.channel_mut(channel).pan = 0.5;
    // The far side is halved and the near side is untouched: the balance
    // law, to the bit.
    assert_eq!(settled(&rig.play(RATE, 8_000, 64)), (LEVEL * 0.5, LEVEL));
    rig.channel_mut(channel).pan = -1.0;
    assert_eq!(settled(&rig.play(RATE, 8_000, 64)), (LEVEL, 0.0));
}

#[test]
fn changing_the_law_while_playing_takes_effect_and_never_touches_the_allocator() {
    let (mut rig, channel) = steady(PanLaw::Balance);
    rig.channel_mut(channel).pan = 0.6;
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let before = run(&mut processor, 8_000, 64);
    assert_sides(settled(&before), scaled(PanLaw::Balance, 0.6), "before");

    rig.project.settings.pan_law = PanLaw::ConstantPower;
    controller.set_project(&rig.project, &rig.pool);
    let mut block = vec![0.0; 256 * 2];
    let mut after = Vec::new();
    for _ in 0..40 {
        // The plan with the new law is taken over and played on the audio
        // path without an allocation or a free.
        assert_eq!(allocator_calls(|| processor.process(&mut block)), 0);
        after.extend_from_slice(&block);
    }
    assert_sides(
        settled(&after),
        scaled(PanLaw::ConstantPower, 0.6),
        "after the change",
    );
}

/// A project that puts both settings to work in song mode: panned channel,
/// panned track, panned clip and notes off the grid.
fn busy(law: PanLaw, timebase_ppq: u16) -> Rig {
    let mut rig = Rig::new();
    rig.project.settings.pan_law = law;
    rig.project.settings.timebase_ppq = timebase_ppq;
    let track = rig.track();
    let channel = rig.channel_on(level(RATE, LEVEL, 0.05), track);
    rig.channel_mut(channel).pan = -0.4;
    rig.track_mut(track).pan = 0.7;
    for (start, pan) in [(3, 0.0), (244, 0.5), (485, -1.0), (1_196, 0.25)] {
        rig.note(channel, start, 230).pan = pan;
    }
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 7, 3_833);
    let audio = rig.playlist_track();
    let clip = rig.audio_clip(audio, level(RATE, 0.25, 0.2), TrackId::MASTER, 1_203, 480);
    rig.audio_mut(clip, |audio| audio.pan = -0.8);
    rig
}

#[test]
fn an_export_renders_exactly_what_plays_under_every_law_and_timebase() {
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Song,
        tail_secs: 0.0,
        ..RenderOptions::default()
    };
    let mut seen = Vec::new();
    for (law, timebase_ppq) in [
        (PanLaw::Balance, 960),
        (PanLaw::ConstantPower, 960),
        (PanLaw::Linear, 96),
        (PanLaw::Compromise, 24),
    ] {
        let rig = busy(law, timebase_ppq);
        let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
        assert!(peak(rendered.samples()) > 0.01);
        for block in [64, 127, 1_000] {
            let (mut processor, controller) = rig.song_processor(RATE);
            let latency = controller.latency_frames() as usize;
            controller.play();
            let played = run(&mut processor, rendered.frames() + latency, block);
            assert_eq!(
                rendered.samples(),
                &played[latency * 2..],
                "{law:?} at {timebase_ppq}, blocks of {block}"
            );
        }
        seen.push(rendered.samples().to_vec());
    }
    // The settings are heard: no two of the four renders are the same.
    for (index, one) in seen.iter().enumerate() {
        for other in &seen[index + 1..] {
            assert_ne!(one, other);
        }
    }
}

#[test]
fn notes_and_clips_sound_on_the_grid_of_the_timebase() {
    // At 48 kHz and 120 bpm one stored tick is exactly 25 frames.
    let notes = [3, 244, 485, 1_196];
    for (timebase_ppq, ticks) in [
        (960, [3, 244, 485, 1_196]),
        (480, [4, 244, 486, 1_196]),
        (96, [0, 240, 490, 1_200]),
        (24, [0, 240, 480, 1_200]),
    ] {
        let mut rig = Rig::new();
        rig.project.settings.tempo_bpm = 120.0;
        rig.project.settings.timebase_ppq = timebase_ppq;
        let channel = rig.channel(impulse(RATE));
        for start in notes {
            rig.note(channel, start, 7);
        }
        let stored = rig.project.clone();
        for block in [7, 64, 1_000] {
            assert_eq!(
                sounding_frames(&rig.play(RATE, 40_000, block)),
                ticks.map(|tick| tick * 25),
                "{timebase_ppq} ticks per quarter note, blocks of {block}"
            );
        }
        // Playing rounds a copy: the project keeps the timing it was given.
        assert_eq!(rig.project, stored);

        // A pattern clip that starts off the grid starts on it.
        let lane = rig.playlist_track();
        let pattern = rig.first_pattern();
        rig.clip(lane, pattern, 3_847, 3_840);
        let grid = 960 / u32::from(timebase_ppq);
        let clip_start = (3_847 + grid / 2) / grid * grid;
        let first = sounding_frames(&rig.play_song(RATE, 140_000, 64))[0];
        assert_eq!(
            first,
            ((clip_start + ticks[0] as u32) * 25) as usize,
            "{timebase_ppq} ticks per quarter note, clip"
        );
    }
}
