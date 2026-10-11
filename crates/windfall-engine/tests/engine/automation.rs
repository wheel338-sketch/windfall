//! Automation: curves on the playlist that move faders, knobs and the
//! tempo while the song plays.

use windfall_core::{db_to_gain, gain_to_db};
use windfall_dsp::{EqParams, LimiterParams, ParamSet, ReverbParams, SynthParams, Waveform};
use windfall_engine::{RenderOptions, TAIL_SILENCE_DB, render};
use windfall_ipc::{AutomatedValue, PlayMode, TransportPatch};
use windfall_project::{
    AutomationId, AutomationTarget, ChannelId, ClipId, EffectParams, Envelope, Send, TrackId,
};

use crate::support::{
    Rig, frequency, impulse, largest_step, left, level, peak, plain_synth, right, rms, run, sine,
    sounding_frames,
};

const RATE: u32 = 48_000;

/// Frames in a tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// Frames between two looks at the automation. A corner or a jump of a
/// curve takes effect within this many frames of its tick.
const GRID: usize = 64;

/// Frames of the fade at the start of an audio clip.
const DECLICK: usize = 144;

/// The gain a volume automation's value of `n` stands for.
fn gain(n: f64) -> f64 {
    2.0 * n * n
}

/// A project whose song is four bars of a steady 0.5 on a track of its
/// own, so that what comes out is half the gains it went through.
fn steady() -> (Rig, TrackId) {
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.5, 10.0), track, 0, 15_360);
    (rig, track)
}

/// Puts an automation of `target` with these points on a playlist track of
/// its own, from tick `start` for `length` ticks.
fn automate(
    rig: &mut Rig,
    target: AutomationTarget,
    points: &[(u32, f32)],
    start: u32,
    length: u32,
) -> AutomationId {
    let lane = rig.playlist_track();
    let automation = rig.automation(target, points);
    rig.automation_clip(lane, automation, start, length);
    automation
}

fn close(actual: f32, expected: f64, tolerance: f64) -> bool {
    (f64::from(actual) - expected).abs() <= tolerance
}

#[test]
fn a_volume_ride_follows_its_curve_in_decibels() {
    // From a quarter of the way up to the top of the fader over two bars,
    // 192000 frames, on a track whose fader is stored at 0 dB.
    let (mut rig, track) = steady();
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.25), (7_680, 1.0)], 0, 7_680);
    let out = left(&rig.play_song(RATE, 300_000, 480));
    let value = |frame: usize| 0.25 + 0.75 * (frame as f64 / 192_000.0).min(1.0);

    // It opens where the curve does, 18 dB down, not at the stored 0 dB,
    // and climbs to +6 dB in a line that is straight in the fader's travel.
    let decibels = |frame: usize| f64::from(gain_to_db(out[frame] / 0.5));
    assert!(
        (decibels(DECLICK) + 18.0).abs() < 0.1,
        "{}",
        decibels(DECLICK)
    );
    for frame in [DECLICK, 1_000, 48_000, 96_000, 150_000, 191_999] {
        let expected = 20.0 * gain(value(frame)).log10();
        assert!(
            (decibels(frame) - expected).abs() < 0.001,
            "frame {frame}: {} dB, not {expected}",
            decibels(frame)
        );
    }
    assert!((decibels(96_000) - 20.0 * gain(0.625).log10()).abs() < 0.001);
    // Sample for sample it is the curve, read every 64 frames and joined
    // by straight lines, which for a curve this gentle is the curve.
    for frame in (DECLICK..192_000).step_by(97) {
        let expected = 0.5 * gain(value(frame));
        assert!(close(out[frame], expected, 2e-6), "frame {frame}");
    }
    assert!(out[DECLICK..192_000].is_sorted());

    // After the clip the fader holds where the curve left it, at +6 dB,
    // for as long as the song plays.
    assert!(out[192_000 + GRID..].iter().all(|sample| *sample == 1.0));
}

#[test]
fn stopping_returns_a_target_to_its_stored_value_without_a_click() {
    // The fader is held 18 dB down for the whole song. A sampler on the
    // same track is played by hand.
    let (mut rig, track) = steady();
    let key = rig.channel_on(level(RATE, 0.5, 10.0), track);
    let target = AutomationTarget::TrackVolume { track };
    let automation = automate(&mut rig, target, &[(0, 0.25)], 0, 15_360);
    let stored = rig.project.clone();
    let (mut processor, controller) = rig.song_processor(RATE);
    assert!(controller.frame().automated.is_empty());
    controller.play();
    let out = left(&run(&mut processor, 4_800, 480));
    assert!(out[DECLICK..].iter().all(|sample| *sample == 0.0625));

    // While the song plays, the UI is told what the automation is doing.
    assert_eq!(
        controller.frame().automated,
        [AutomatedValue {
            automation,
            value: 0.25
        }]
    );
    // A note played by hand goes through the automated fader too.
    controller.note_on(key, 60, 1.0);
    let out = left(&run(&mut processor, 4_800, 480));
    assert!(out.iter().all(|sample| *sample == 0.125));

    // Stopped, with a note played by hand at once: the fader glides back
    // to its stored 0 dB over 100 ms, under the note, which comes out at
    // that from then on.
    controller.stop();
    controller.note_on(key, 60, 1.0);
    let out = left(&run(&mut processor, 9_600, 480));
    assert!(out[4_800..].iter().all(|sample| *sample == 0.5));
    // Once what the song was playing has faded, it is the note alone
    // under a fader on its way up in a straight line.
    for frame in (240..4_800).step_by(97) {
        let fader = 0.125 + 0.875 * frame as f64 / 4_800.0;
        assert!(close(out[frame], 0.5 * fader, 1e-6), "frame {frame}");
    }
    assert!(largest_step(&out) < 0.001, "{}", largest_step(&out));
    assert!(controller.frame().automated.is_empty());

    // Nothing of it was written anywhere: the project is as it was, and
    // playing it again gives the same sound.
    assert_eq!(rig.project, stored);
    controller.stop();
    run(&mut processor, 4_800, 480);
    controller.play();
    let again = left(&run(&mut processor, 4_800, 480));
    assert!(again[DECLICK..].iter().all(|sample| *sample == 0.0625));
}

/// A reverb that takes a second to fade by 60 dB, as much of it in the
/// signal as of the sound that went in.
fn reverb() -> EffectParams {
    EffectParams::Reverb(ReverbParams {
        decay_s: 1.0,
        mix: 0.5,
        ..ReverbParams::default()
    })
}

/// A song of two bars, 192000 frames: a tone through a reverb on the
/// master, whose fader stays at 0 dB for the first bar and comes down to
/// silence over the second. Returns the automation clip as well.
fn faded_out() -> (Rig, ClipId) {
    let mut rig = Rig::new();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, sine(RATE, 440.0, 4.0), TrackId::MASTER, 0, 7_680);
    rig.effect(TrackId::MASTER, reverb());
    let target = AutomationTarget::TrackVolume {
        track: TrackId::MASTER,
    };
    let unity = std::f32::consts::FRAC_1_SQRT_2;
    let fade = rig.automation(target, &[(0, unity), (3_840, unity), (7_680, 0.0)]);
    let lane = rig.playlist_track();
    let clip = rig.automation_clip(lane, fade, 0, 7_680);
    (rig, clip)
}

#[test]
fn a_fade_out_at_the_end_of_a_song_keeps_the_tail_silent_in_a_render() {
    let (mut rig, fade) = faded_out();
    let options = |auto_tail, block_frames| RenderOptions {
        mode: PlayMode::Song,
        tail_secs: 2.0,
        auto_tail,
        block_frames,
        ..RenderOptions::default()
    };
    let silence = db_to_gain(TAIL_SILENCE_DB);
    let end = 192_000 * 2;

    // The fader is still at silence when the song is over, and stays
    // there: the reverb rings on unheard.
    let rendered = render(&rig.project, &rig.pool, &options(false, 1_024), &mut |_| {
        true
    });
    assert_eq!(rendered.frames(), 192_000 + 96_000);
    let out = rendered.samples();
    assert!(peak(&out[96_000 * 2..120_000 * 2]) > 0.3);
    assert!(peak(&out[end - 9_600..end]) < 0.01);
    let tail = peak(&out[end..]);
    assert!(tail < silence, "the tail reaches {tail}");

    // An automatic tail finds nothing to wait for, whatever the block
    // size: the file ends with the song.
    for block_frames in [7, 64, 1_024, 100_000] {
        let short = render(
            &rig.project,
            &rig.pool,
            &options(true, block_frames),
            &mut |_| true,
        );
        assert_eq!(short.frames(), 192_000, "blocks of {block_frames}");
        assert!(short.samples() == &out[..end], "blocks of {block_frames}");
    }

    // Without the fade the same tail is plain to hear.
    rig.clip_mut(fade).muted = true;
    let plain = render(&rig.project, &rig.pool, &options(false, 1_024), &mut |_| {
        true
    });
    assert!(peak(&plain.samples()[end..]) > 0.01);
}

#[test]
fn a_fade_out_at_the_end_of_a_song_keeps_the_tail_silent_in_playback() {
    let (rig, _) = faded_out();
    let automation = rig.project.automations[0].id;
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 192_000 + 480, 480);
    assert!(!controller.frame().playing);

    // The song is over and its reverb is not. Nothing of it comes out, the
    // master's meter stays down with it, and the UI is still told where
    // the fader is.
    let silence = db_to_gain(TAIL_SILENCE_DB);
    let tail = run(&mut processor, 24_000, 480);
    assert!(peak(&tail) < silence, "the tail reaches {}", peak(&tail));
    let frame = controller.frame();
    assert!(frame.meters[0] < silence && frame.meters[1] < silence);
    assert_eq!(
        frame.automated,
        [AutomatedValue {
            automation,
            value: 0.0
        }]
    );

    // Once the reverb has rung out, the fader goes back to where it is
    // stored, and still nothing is heard of what is left.
    let mut waited = 0;
    while !controller.frame().automated.is_empty() {
        let more = run(&mut processor, 4_800, 480);
        assert!(peak(&more) < silence);
        waited += 4_800;
        assert!(waited < 10 * 48_000, "the fader never went back");
    }
    assert!(waited > 24_000, "the fader went back after {waited} frames");
    let more = run(&mut processor, 48_000, 480);
    assert!(peak(&more) < silence, "{}", peak(&more));

    // Played again, the song opens as loud as it did the first time.
    let first = rig.play_song(RATE, 9_600, 480);
    controller.play();
    let again = run(&mut processor, 9_600, 480);
    assert!(rms(&first) > 0.1);
    let ratio = rms(&again) / rms(&first);
    assert!((ratio - 1.0).abs() < 0.02, "{ratio}");
}

#[test]
fn the_mix_of_an_effect_stays_where_the_song_left_it_through_the_tail() {
    // A click on the last step of a one-bar song, into a reverb that is
    // all reverb. A curve holds the mix of its slot at a half; it is
    // stored at 1.
    let mut rig = Rig::new();
    let track = rig.track();
    let click = rig.channel_on(impulse(RATE), track);
    rig.steps(click, &[15]);
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 3_840);
    let effect = rig.effect(
        track,
        EffectParams::Reverb(ReverbParams {
            decay_s: 1.0,
            mix: 1.0,
            ..ReverbParams::default()
        }),
    );
    let target = AutomationTarget::EffectMix { track, effect };
    automate(&mut rig, target, &[(0, 0.5)], 0, 3_840);
    let options = RenderOptions {
        mode: PlayMode::Song,
        tail_secs: 1.0,
        ..RenderOptions::default()
    };
    let held = render(&rig.project, &rig.pool, &options, &mut |_| true);
    rig.project.playlist.clips[1].muted = true;
    let plain = render(&rig.project, &rig.pool, &options, &mut |_| true);

    // The tail after the song's end is half of what it is without the
    // curve, all the way: nothing of the click itself is left in it, only
    // reverb.
    let (held, plain) = (left(held.samples()), left(plain.samples()));
    assert!(rms(&plain[96_000..120_000]) > 1e-3);
    for from in [96_000, 96_960, 100_000, 120_000, 139_200] {
        let window = from..from + 4_800;
        let ratio = rms(&held[window.clone()]) / rms(&plain[window]);
        assert!((ratio - 0.5).abs() < 0.01, "from {from}: {ratio}");
    }
}

/// A song of one bar: a tone on a track of its own with a reverb, under a
/// fader that a curve holds 18 dB down and that is stored at `stored`.
/// With `automated` off the curve's clip is muted and the fader is simply
/// where it is stored. Returns a sampler on the same track as well, for
/// playing by hand.
fn ringing(stored: f32, automated: bool) -> (Rig, ChannelId) {
    let mut rig = Rig::new();
    let track = rig.track();
    rig.track_mut(track).volume = stored;
    rig.effect(track, reverb());
    let lane = rig.playlist_track();
    rig.audio_clip(lane, sine(RATE, 440.0, 4.0), track, 0, 3_840);
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.25)], 0, 3_840);
    rig.project.playlist.clips[1].muted = !automated;
    let key = rig.channel_on(level(RATE, 0.25, 0.1), track);
    (rig, key)
}

#[test]
fn a_song_that_ends_by_itself_holds_its_automation_while_it_rings_out() {
    // What comes out is, to the bit, what a fader stored 18 dB down gives:
    // through the song, across its end and on through the reverb's tail.
    let (rig, key) = ringing(1.0, true);
    let automation = rig.project.automations[0].id;
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let out = run(&mut processor, 96_000 + 48_000, 480);
    let (down, _) = ringing(0.125, false);
    assert!(out == down.play_song(RATE, 96_000 + 48_000, 480));
    // A tail that is there to be heard, and no louder across the end.
    let out = left(&out);
    assert!(rms(&out[96_480..100_800]) > 1e-3);
    assert!(peak(&out[96_000..97_000]) <= peak(&out[95_000..96_000]));

    // The transport has stopped, and the fader is still the song's.
    let frame = controller.frame();
    assert!(!frame.playing);
    assert_eq!(
        frame.automated,
        [AutomatedValue {
            automation,
            value: 0.25
        }]
    );

    // It goes back when the tail has died away.
    let mut waited = 0;
    while !controller.frame().automated.is_empty() {
        run(&mut processor, 4_800, 480);
        waited += 4_800;
        assert!(waited < 10 * 48_000, "the fader never went back");
    }
    // A tenth of a second later the fader is at its stored 0 dB: a note
    // played by hand is as loud as in a project that was never played,
    // not 18 dB under it.
    run(&mut processor, 4_800, 480);
    controller.note_on(key, 60, 1.0);
    let out = run(&mut processor, 4_800, 480);
    let (fresh, key) = ringing(1.0, false);
    let (mut other, hands) = fresh.song_processor(RATE);
    hands.note_on(key, 60, 1.0);
    let expected = run(&mut other, 4_800, 480);
    assert!(rms(&expected) > 0.1);
    let ratio = rms(&out) / rms(&expected);
    assert!((ratio - 1.0).abs() < 0.02, "{ratio}");
}

#[test]
fn stopping_holds_the_automation_while_a_tail_rings_and_stopping_again_lets_go() {
    // Half a second into the song the transport is stopped. The tone is
    // faded out and the reverb rings on, under a fader that stays where
    // the song had it: to the bit what a fader stored 18 dB down gives.
    let stopped = |rig: &Rig| {
        let (mut processor, controller) = rig.song_processor(RATE);
        controller.play();
        run(&mut processor, 24_000, 480);
        controller.stop();
        let out = run(&mut processor, 12_000, 480);
        (processor, controller, out)
    };
    let (rig, _) = ringing(1.0, true);
    let automation = rig.project.automations[0].id;
    let (mut processor, controller, out) = stopped(&rig);
    let (down, _) = ringing(0.125, false);
    let (_, _, expected) = stopped(&down);
    assert!(out == expected);
    assert!(rms(&out[9_600..]) > 1e-4, "no tail to hear");
    assert_eq!(controller.frame().automated.len(), 1);
    assert_eq!(controller.frame().automated[0].automation, automation);

    // Stop again while it rings: the fader goes back to its stored 0 dB in
    // a straight line over 100 ms. The tail swells with it and never
    // jumps: it is the tail of a track stored at 0 dB under that line.
    let (full, _) = ringing(1.0, false);
    let (mut reference, other, _) = stopped(&full);
    controller.stop();
    other.stop();
    let out = left(&run(&mut processor, 9_600, 480));
    let full = left(&run(&mut reference, 9_600, 480));
    for (frame, (out, full)) in out.iter().zip(&full).enumerate() {
        let fader = 0.125 + 0.875 * (frame as f64 / 4_800.0).min(1.0);
        assert!(close(*out, f64::from(*full) * fader, 1e-6), "frame {frame}");
    }
    assert!(rms(&full[..4_800]) > 1e-3);
    // No step in it is larger than the tail's own.
    assert!(largest_step(&out) <= largest_step(&full) + 1e-6);
    assert!(controller.frame().automated.is_empty());
}

#[test]
fn a_setting_that_is_let_go_of_while_stopped_walks_back_over_a_tenth_of_a_second() {
    // The equaliser's output gain is held 12 dB down by a curve and
    // stored at 0 dB. The song is stopped and a note of the same sine is
    // played by hand in the same breath, which lets the setting go.
    let (mut rig, track, effect) = toned();
    let key = rig.channel_on(sine(RATE, 1_000.0, 2.0), track);
    let param = EqParams::index_of("outputGainDb").unwrap() as u32;
    let target = AutomationTarget::EffectParam {
        track,
        effect,
        param,
    };
    automate(&mut rig, target, &[(0, 0.25)], 0, 15_360);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 9_600, 480);
    controller.stop();
    controller.note_on(key, 60, 1.0);
    let out = left(&run(&mut processor, 14_400, 480));

    // Ten milliseconds of the note at a time, against a sine at half
    // scale: from 12 dB down it climbs a decibel every 8.3 ms, where the
    // equaliser left to itself would be back in a fraction of that. The
    // equaliser's own glide keeps it a little behind the line.
    let decibels = |from: usize| {
        let level = rms(&out[from..from + 480]) / (0.5 * std::f32::consts::FRAC_1_SQRT_2);
        f64::from(gain_to_db(level))
    };
    for (from, expected) in [(960, -9.0), (2_160, -6.0), (3_360, -3.0)] {
        let behind = expected - decibels(from);
        assert!(
            (0.0..2.5).contains(&behind),
            "from frame {from}: {behind} dB under {expected}"
        );
    }
    assert!(decibels(7_200).abs() < 0.05, "{}", decibels(7_200));
    assert!(decibels(13_000).abs() < 0.01);
    assert!(controller.frame().automated.is_empty());
}

#[test]
fn a_looping_song_lets_go_of_a_target_before_its_first_clip_each_time_around() {
    // Two bars. The fader is the curve's, 18 dB down, in the second bar
    // only; in the first it is where it is stored.
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.5, 10.0), track, 0, 7_680);
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.25)], 3_840, 3_840);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..TransportPatch::default()
    });
    controller.play();
    let out = left(&run(&mut processor, 3 * 192_000, 480));
    let bar = 96_000;
    for pass in 0..3 {
        let start = pass * 2 * bar;
        // Around again is from the top again: back at the stored 0 dB
        // within the 5 ms a fader move takes, with the song playing on.
        let first = &out[start + 240..start + bar - GRID];
        assert!(first.iter().all(|sample| *sample == 0.5), "pass {pass}");
        let second = &out[start + bar + GRID..start + 2 * bar - DECLICK];
        assert!(second.iter().all(|s| *s == 0.0625), "pass {pass}");
    }
    assert!(controller.frame().playing);
}

#[test]
fn a_channels_volume_and_pan_follow_their_curves() {
    // A long steady sample on a sampler channel, played by a pattern clip.
    let mut rig = Rig::new();
    let channel = rig.channel(level(RATE, 0.5, 10.0));
    rig.note(channel, 0, 240);
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 3_840);
    let volume = AutomationTarget::ChannelVolume { channel };
    automate(&mut rig, volume, &[(0, 1.0), (3_840, 0.5)], 0, 3_840);
    let pan = AutomationTarget::ChannelPan { channel };
    automate(&mut rig, pan, &[(0, 0.5), (3_840, 1.0)], 0, 3_840);

    let out = rig.play_song(RATE, 96_000, 480);
    let (left, right) = (left(&out), right(&out));
    for frame in (0..96_000).step_by(1_013) {
        let part = frame as f64 / 96_000.0;
        // From +6 dB down to 6 dB under unity, and from the center to
        // hard right, which takes the left side away.
        let level = 0.5 * gain(1.0 - 0.5 * part);
        assert!(close(right[frame], level, 2e-6), "frame {frame}");
        assert!(
            close(left[frame], level * (1.0 - part), 2e-6),
            "frame {frame}"
        );
    }

    // A muted channel stays silent whatever its curve says.
    rig.channel_mut(channel).muted = true;
    assert!(rig.play_song(RATE, 9_600, 480).iter().all(|s| *s == 0.0));
    // In pattern mode the curves do nothing: the channel is at its stored
    // volume, in the center.
    rig.channel_mut(channel).muted = false;
    let out = rig.play(RATE, 9_600, 480);
    assert!(out.iter().all(|sample| *sample == 0.5));
}

#[test]
fn an_instruments_channel_volume_is_automated_like_a_samplers() {
    let mut rig = Rig::new();
    let track = rig.track();
    let synth = rig.synth_on(plain_synth(), track);
    rig.note(synth, 0, 3_000).key = 57;
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 3_840);
    let plain = left(&rig.play_song(RATE, 48_000, 480));
    assert!(rms(&plain) > 0.1);

    // Held 6 dB under unity for the whole song.
    let target = AutomationTarget::ChannelVolume { channel: synth };
    automate(&mut rig, target, &[(0, 0.5)], 0, 3_840);
    let quiet = left(&rig.play_song(RATE, 48_000, 480));
    for (frame, (quiet, plain)) in quiet.iter().zip(&plain).enumerate() {
        assert!((quiet - plain * 0.5).abs() < 1e-7, "frame {frame}");
    }
}

#[test]
fn a_send_opens_and_closes_with_its_curve() {
    let (mut rig, track) = steady();
    let bus = rig.track();
    rig.track_mut(track).sends.push(Send {
        target: bus,
        gain: 0.0,
    });
    let target = AutomationTarget::SendGain { track, target: bus };
    // Shut for the first bar, then open to 0 dB over the second.
    let n = std::f32::consts::FRAC_1_SQRT_2;
    automate(
        &mut rig,
        target,
        &[(0, 0.0), (3_840, 0.0), (7_680, n)],
        0,
        7_680,
    );
    let out = left(&rig.play_song(RATE, 250_000, 480));
    assert!(out[DECLICK..96_000].iter().all(|sample| *sample == 0.5));
    for frame in (96_000..192_000).step_by(1_009) {
        let part = (frame - 96_000) as f64 / 96_000.0;
        let send = gain(f64::from(n) * part);
        assert!(close(out[frame], 0.5 + 0.5 * send, 2e-6), "frame {frame}");
    }
    // Held open after the clip: the track and its send, twice the level.
    assert!(out[192_000 + GRID..].iter().all(|s| (s - 1.0).abs() < 1e-6));
}

/// The level of `signal` around `frame`, in dB relative to a sine at half
/// scale.
fn sine_db(signal: &[f32], frame: usize) -> f64 {
    let window = &signal[frame - 2_400..frame + 2_400];
    f64::from(gain_to_db(
        rms(window) / (0.5 * std::f32::consts::FRAC_1_SQRT_2),
    ))
}

/// A project whose song is four bars of a 1 kHz sine at half scale through
/// an equaliser that does nothing yet.
fn toned() -> (Rig, TrackId, windfall_project::EffectId) {
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, sine(RATE, 1_000.0, 10.0), track, 0, 15_360);
    let eq = rig.effect(track, EffectParams::Eq(EqParams::default()));
    (rig, track, eq)
}

#[test]
fn a_setting_of_an_effect_follows_its_curve() {
    let (mut rig, track, effect) = toned();
    let param = EqParams::index_of("outputGainDb").unwrap() as u32;
    let target = AutomationTarget::EffectParam {
        track,
        effect,
        param,
    };
    // The output gain runs from -24 to 24 dB: a quarter of the way up is
    // -12 dB and three quarters +12, two bars apart.
    automate(&mut rig, target, &[(0, 0.25), (7_680, 0.75)], 0, 7_680);
    let out = left(&rig.play_song(RATE, 300_000, 480));
    for frame in [4_800, 48_000, 96_000, 144_000, 187_000] {
        let expected = -12.0 + 24.0 * frame as f64 / 192_000.0;
        let measured = sine_db(&out, frame);
        assert!(
            (measured - expected).abs() < 0.25,
            "frame {frame}: {measured} dB, not {expected}"
        );
    }
    // Held at +12 dB after the clip.
    assert!((sine_db(&out, 250_000) - 12.0).abs() < 0.01);

    // The stored setting is untouched: without the clip the equaliser
    // does nothing.
    rig.project.playlist.clips[1].muted = true;
    let out = left(&rig.play_song(RATE, 48_000, 480));
    assert!(sine_db(&out, 24_000).abs() < 0.01);
}

#[test]
fn the_mix_of_an_effect_follows_its_curve() {
    // The equaliser turns its signal down by 24 dB. Its mix goes from all
    // of it to none of it over two bars.
    let (mut rig, track, effect) = toned();
    let EffectParams::Eq(eq) = &mut rig.effect_mut(effect).params else {
        panic!("not an equaliser");
    };
    eq.output_gain_db = -24.0;
    let target = AutomationTarget::EffectMix { track, effect };
    automate(&mut rig, target, &[(0, 1.0), (7_680, 0.0)], 0, 7_680);
    let out = left(&rig.play_song(RATE, 300_000, 480));
    let wet = 10.0_f64.powf(-24.0 / 20.0);
    for frame in [4_800, 48_000, 96_000, 144_000, 187_000] {
        let mix = 1.0 - frame as f64 / 192_000.0;
        let expected = 20.0 * (1.0 - mix + mix * wet).log10();
        let measured = sine_db(&out, frame);
        assert!(
            (measured - expected).abs() < 0.5,
            "frame {frame}: {measured} dB, not {expected}"
        );
    }
    assert!(sine_db(&out, 250_000).abs() < 0.01);
}

/// How bright a signal is: the level of the change from sample to sample
/// against the level of the signal. A low-pass filter that opens lets
/// more of the fast changes through.
fn brightness(signal: &[f32]) -> f32 {
    let changes: Vec<f32> = signal.windows(2).map(|pair| pair[1] - pair[0]).collect();
    rms(&changes) / rms(signal)
}

#[test]
fn a_filter_sweep_on_the_synth_opens_up_the_sound() {
    // A low saw held for two bars.
    let mut rig = Rig::new();
    let track = rig.track();
    let mut sound = plain_synth();
    sound.oscillators[0].waveform = Waveform::Saw;
    let synth = rig.synth_on(sound, track);
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).length_steps = 32;
    rig.note(synth, 0, 7_680).key = 36;
    let lane = rig.playlist_track();
    rig.clip(lane, pattern, 0, 7_680);
    let windows = |out: &[f32]| -> Vec<f32> {
        let frames = out[24_000..192_000 - 24_000].chunks(24_000);
        frames.map(brightness).collect()
    };

    // With the cutoff where it is stored, wide open, the sound is the
    // same all the way through.
    let open = windows(&left(&rig.play_song(RATE, 192_000, 480)));
    let (least, most) = open.iter().fold((f32::MAX, 0.0_f32), |(least, most), b| {
        (least.min(*b), most.max(*b))
    });
    assert!(most / least < 1.05, "{open:?}");

    // The cutoff runs from 20 Hz to 20 kHz in equal ratios: 0.2 of the way
    // is 80 Hz, just over the note, and 0.95 is 14 kHz.
    let param = SynthParams::index_of("filter.cutoffHz").unwrap() as u32;
    let target = AutomationTarget::InstrumentParam {
        channel: synth,
        param,
    };
    automate(&mut rig, target, &[(0, 0.2), (7_680, 0.95)], 0, 7_680);
    let swept = windows(&left(&rig.play_song(RATE, 192_000, 480)));
    assert!(swept.is_sorted(), "{swept:?}");
    assert!(swept[5] > swept[0] * 4.0, "{swept:?}");
    // It starts out dull and ends up on its way to the open sound.
    assert!(swept[0] < open[0] * 0.2, "{swept:?} against {open:?}");
    assert!(swept[5] > open[0] * 0.5, "{swept:?} against {open:?}");
    // The pitch is the note's throughout: the filter moved, nothing else.
    let out = left(&rig.play_song(RATE, 192_000, 480));
    let pitch = frequency(&out[9_600..19_200], RATE);
    assert!((pitch - 65.41).abs() < 0.5, "{pitch} Hz");
}

#[test]
fn where_clips_overlap_the_one_nearest_the_top_decides() {
    let (mut rig, track) = steady();
    let (above, below) = (rig.playlist_track(), rig.playlist_track());
    let target = AutomationTarget::TrackVolume { track };
    // A long clip that holds the fader 12 dB down, and above it a short
    // one of another automation that pushes it to +6 dB for a beat.
    let low = rig.automation(target, &[(0, 0.5)]);
    let high = rig.automation(target, &[(0, 1.0)]);
    rig.automation_clip(below, low, 0, 3_840);
    rig.automation_clip(above, high, 960, 960);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let out = left(&run(&mut processor, 150_000, 480));

    let beat = 960 * TICK;
    let steady_at = |from: usize, to: usize, level: f32| {
        let part = &out[from + GRID..to - GRID];
        assert!(
            part.iter().all(|sample| *sample == level),
            "{from} to {to} is not at {level}"
        );
    };
    steady_at(DECLICK, beat, 0.25);
    steady_at(beat, 2 * beat, 1.0);
    // The long clip has the fader again once the short one is over, and
    // when it ends itself the fader stays where it left it.
    steady_at(2 * beat, 4 * beat, 0.25);
    steady_at(4 * beat, 150_000, 0.25);
    // Each change takes one step of the grid at most, and is a straight
    // line, not a jump.
    assert!(largest_step(&out[DECLICK..]) <= 0.75 / GRID as f32 + 1e-6);
    // The UI is told which of the two is in charge: the one that ended
    // last, holding.
    let automated = controller.frame().automated;
    assert_eq!(automated.len(), 1);
    assert_eq!(automated[0].automation, low);

    // Swapped, the long clip is on top and the short one is never heard.
    for clip in &mut rig.project.playlist.clips {
        if let windfall_project::ClipContent::Automation { automation } = clip.content {
            clip.track = if automation == low { above } else { below };
        }
    }
    let out = left(&rig.play_song(RATE, 100_000, 480));
    assert!(out[DECLICK..].iter().all(|sample| *sample == 0.25));
    // A muted clip decides nothing.
    for clip in &mut rig.project.playlist.clips {
        clip.muted = clip.content == windfall_project::ClipContent::Automation { automation: low };
    }
    let out = left(&rig.play_song(RATE, 100_000, 480));
    assert!(out[DECLICK..beat - GRID].iter().all(|s| *s == 0.5));
    assert!(out[beat + GRID..2 * beat - GRID].iter().all(|s| *s == 1.0));
    assert!(out[2 * beat + GRID..].iter().all(|s| *s == 1.0));
}

#[test]
fn a_target_is_where_the_song_has_it_wherever_playback_begins() {
    // The fader climbs over the second and third bar of four.
    let (mut rig, track) = steady();
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.25), (7_680, 1.0)], 3_840, 7_680);
    let value = |tick: f64| 0.25 + 0.75 * ((tick - 3_840.0) / 7_680.0).clamp(0.0, 1.0);
    let from = |tick: f64| {
        let (mut processor, controller) = rig.song_processor(RATE);
        controller.seek(tick);
        controller.play();
        left(&run(&mut processor, 9_600, 480))
    };

    // Before the first clip: the stored 0 dB.
    let out = from(960.0);
    assert!(out[DECLICK..].iter().all(|sample| *sample == 0.5));
    // In the middle of the climb: where the curve is, from the first
    // frame on, and on up from there.
    let out = from(5_760.0);
    for frame in [0, 1, GRID, 1_000, 9_000] {
        let tick = 5_760.0 + frame as f64 / TICK as f64;
        let expected = 0.5 * gain(value(tick)) * ((frame + 1) as f64 / 144.0).min(1.0);
        assert!(close(out[frame], expected, 2e-6), "frame {frame}");
    }
    // After the clip: where it left the fader.
    let out = from(13_000.0);
    assert!(out[DECLICK..].iter().all(|sample| *sample == 1.0));

    // Moved back to before the clip while it plays, the fader returns to
    // its stored value.
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.seek(13_000.0);
    controller.play();
    run(&mut processor, 4_800, 480);
    controller.seek(0.0);
    let out = left(&run(&mut processor, 9_600, 480));
    assert!(out[240..].iter().all(|sample| *sample == 0.5));
}

#[test]
fn a_looping_song_starts_each_time_around_from_its_first_values() {
    // One bar, over which the fader climbs. At the end it is at +6 dB,
    // and the next time around it starts 18 dB down again.
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.5, 10.0), track, 0, 3_840);
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.25), (3_840, 1.0)], 0, 3_840);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..TransportPatch::default()
    });
    controller.play();
    let out = left(&run(&mut processor, 300_000, 480));
    let value = |frame: usize| 0.25 + 0.75 * (frame % 96_000) as f64 / 96_000.0;
    for pass in 0..3 {
        for frame in (DECLICK..96_000 - 2 * DECLICK).step_by(997) {
            let at = pass * 96_000 + frame;
            let expected = 0.5 * gain(value(at));
            assert!(close(out[at], expected, 2e-6), "frame {at}");
        }
    }
    // Around the loop point the fader comes down within a step of the
    // grid, under the 3 ms the clip itself takes to fade out and in.
    assert!(out[96_000 - 1] < 0.02 && out[96_000 + GRID] < 0.1);
}

/// The level of one frequency in a signal, by correlating it with a sine
/// and a cosine through a window that tapers to nothing at both ends.
fn level_at(signal: &[f32], hertz: f64) -> f64 {
    let frames = signal.len() as f64;
    let (mut real, mut imaginary, mut weight) = (0.0, 0.0, 0.0);
    for (frame, sample) in signal.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * frame as f64 / frames).cos();
        let phase = std::f64::consts::TAU * hertz * frame as f64 / f64::from(RATE);
        real += f64::from(*sample) * window * phase.cos();
        imaginary += f64::from(*sample) * window * phase.sin();
        weight += window;
    }
    2.0 * (real * real + imaginary * imaginary).sqrt() / weight
}

#[test]
fn a_fast_sweep_has_no_steps_in_it() {
    // A 1 kHz sine whose fader goes from silence to +6 dB in a quarter of
    // a second. The automation is read 750 times a second, and steps at
    // that rate would put tones 750 Hz either side of the sine and at
    // every multiple of it.
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, sine(RATE, 1_000.0, 2.0), track, 0, 1_920);
    let target = AutomationTarget::TrackVolume { track };
    automate(&mut rig, target, &[(0, 0.0), (480, 1.0)], 0, 480);
    let out = left(&rig.play_song(RATE, 12_000, 480));

    // Against the same sine under the curve itself, worked out for every
    // frame: the two are the same to within a few millionths.
    let ideal: Vec<f32> = (0..12_000)
        .map(|frame| {
            let phase = frame as f64 * 1_000.0 / f64::from(RATE);
            let sine = ((phase * std::f64::consts::TAU).sin() * 0.5) as f32;
            sine * gain(frame as f64 / 12_000.0) as f32
        })
        .collect();
    // Up to the last step of the grid before the curve's end, where the
    // corner into the hold is rounded off.
    let whole = DECLICK..12_000 - 2 * GRID;
    let apart: Vec<f32> = out.iter().zip(&ideal).map(|(a, b)| a - b).collect();
    let apart = &apart[whole.clone()];
    let worst = apart.iter().fold(0.0_f32, |w, d| w.max(d.abs()));
    assert!(worst < 2e-5, "off by {worst}");
    // And nothing stands out where steps would: 90 dB and more under the
    // sine at 750 Hz to either side and at its multiples.
    let carrier = level_at(&out[whole], 1_000.0);
    assert!(carrier > 0.1);
    for side in [250.0, 1_750.0, 2_500.0, 3_250.0, 4_000.0] {
        let stepping = level_at(apart, side);
        let decibels = 20.0 * (stepping / carrier).log10();
        assert!(decibels < -90.0, "{side} Hz: {decibels} dB");
    }

    // The same for a setting of an effect, which is handed its new value
    // 750 times a second and glides to each by itself: the equaliser's
    // output gain, from -24 to 24 dB in the same quarter of a second.
    // There is no curve to hold it against, so what is measured is the
    // level where steps would put tones. It stays 80 dB and more under
    // the sine, which is below what 16 bits can hold.
    let (mut rig, track, effect) = toned();
    let param = EqParams::index_of("outputGainDb").unwrap() as u32;
    let target = AutomationTarget::EffectParam {
        track,
        effect,
        param,
    };
    automate(&mut rig, target, &[(0, 0.0), (480, 1.0)], 0, 480);
    let out = left(&rig.play_song(RATE, 12_000, 480));
    let swept = &out[DECLICK..12_000 - 2 * GRID];
    let carrier = level_at(swept, 1_000.0);
    assert!(carrier > 0.1);
    for side in [1_750.0, 2_500.0, 3_250.0, 4_000.0] {
        let decibels = 20.0 * (level_at(swept, side) / carrier).log10();
        assert!(decibels < -80.0, "{side} Hz: {decibels} dB");
    }
}

#[test]
fn a_setting_that_moves_an_effects_latency_is_not_automated() {
    // A click through a limiter with nothing to do, on a track of its own,
    // and the same click on a track beside it. Both land on one frame.
    let mut rig = Rig::new();
    let (limited, plain) = (rig.track(), rig.track());
    let effect = rig.effect(
        limited,
        EffectParams::Limiter(LimiterParams {
            ceiling_db: 0.0,
            input_gain_db: 0.0,
            release_ms: 100.0,
            lookahead_ms: 5.0,
            ..LimiterParams::default()
        }),
    );
    let pattern = rig.first_pattern();
    for track in [limited, plain] {
        let click = rig.channel_on(impulse(RATE), track);
        rig.channel_mut(click).volume = 0.5;
        rig.steps(click, &[0, 4, 8, 12]);
    }
    let lane = rig.playlist_track();
    rig.clip(lane, pattern, 0, 3_840);
    let options = RenderOptions {
        mode: PlayMode::Song,
        ..RenderOptions::default()
    };
    let reference = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert_eq!(
        sounding_frames(reference.samples()),
        [0, 24_000, 48_000, 72_000]
    );

    // A curve on the look-ahead would pull the two apart. It is ignored.
    let param = LimiterParams::index_of("lookaheadMs").unwrap() as u32;
    let target = AutomationTarget::EffectParam {
        track: limited,
        effect,
        param,
    };
    automate(&mut rig, target, &[(0, 0.0), (3_840, 1.0)], 0, 3_840);
    let automated = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(automated.samples() == reference.samples());
}

/// Seconds a tempo that moves in a straight line from `from` to `to` bpm
/// takes to get through `ticks` ticks: the sum of the length of every
/// tick on the way.
fn ramp_seconds(ticks: f64, from: f64, to: f64) -> f64 {
    60.0 / 960.0 * ticks * (to / from).ln() / (to - from)
}

/// The value of a tempo automation that stands for `bpm`.
fn tempo(bpm: f64) -> f32 {
    ((bpm - 10.0) / 512.0) as f32
}

/// A song of four bars with a click on every beat, at a stored 120 bpm.
fn clicks() -> (Rig, ChannelId) {
    let mut rig = Rig::new();
    let click = rig.channel(impulse(RATE));
    rig.steps(click, &[0, 4, 8, 12]);
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 15_360);
    (rig, click)
}

#[test]
fn a_tempo_ramp_takes_exactly_as_long_as_its_curve_says() {
    // From 60 to 180 bpm over the four bars.
    let (mut rig, _) = clicks();
    let points = [(0, tempo(60.0)), (15_360, tempo(180.0))];
    automate(&mut rig, AutomationTarget::Tempo, &points, 0, 15_360);
    let options = RenderOptions {
        mode: PlayMode::Song,
        ..RenderOptions::default()
    };
    let audio = render(&rig.project, &rig.pool, &options, &mut |_| true);

    // The whole song: 8.79 seconds, to the frame.
    let seconds = ramp_seconds(15_360.0, 60.0, 180.0);
    assert!((seconds - 8.788_9).abs() < 1e-3);
    let expected = (seconds * f64::from(RATE)).ceil() as usize;
    assert_eq!(audio.frames(), expected);

    // Every beat lands on the frame its place in the ramp gives it. The
    // tempo at tick t is 60 + 120 * t / 15360.
    let clicks = sounding_frames(audio.samples());
    assert_eq!(clicks.len(), 16);
    for (beat, frame) in clicks.iter().enumerate() {
        let ticks = beat as f64 * 960.0;
        let reached = 60.0 + 120.0 * ticks / 15_360.0;
        let seconds = if beat == 0 {
            0.0
        } else {
            ramp_seconds(ticks, 60.0, reached)
        };
        let expected = (seconds * f64::from(RATE) - 1e-6).ceil() as usize;
        assert_eq!(*frame, expected, "beat {beat}");
    }
    // The beats come faster and faster: the first takes a second at 60
    // bpm and a bit less, the last a third of one and a bit more.
    let first = clicks[1] - clicks[0];
    let last = clicks[15] - clicks[14];
    assert!((44_000..48_000).contains(&first), "{first}");
    assert!((16_500..17_500).contains(&last), "{last}");
}

#[test]
fn a_tempo_that_steps_and_holds_keeps_exact_time() {
    // A bar at the stored 120, a bar at 60 by a clip of one flat point, a
    // bar that holds that 60 after the clip has ended, and a last bar in
    // which a second clip steps to 240 half way through.
    let (mut rig, _) = clicks();
    let lane = rig.playlist_track();
    let slow = rig.automation(AutomationTarget::Tempo, &[(0, tempo(60.0))]);
    rig.automation_clip(lane, slow, 3_840, 3_840);
    let stepped = rig.automation(
        AutomationTarget::Tempo,
        &[(0, tempo(60.0)), (1_920, tempo(240.0))],
    );
    rig.automation_mut(stepped).points[0].hold = true;
    rig.automation_clip(lane, stepped, 11_520, 3_840);
    let options = RenderOptions {
        mode: PlayMode::Song,
        ..RenderOptions::default()
    };
    let audio = render(&rig.project, &rig.pool, &options, &mut |_| true);

    // A beat is 24000 frames at 120, 48000 at 60 and 12000 at 240.
    let mut expected = vec![0];
    let beats = [
        [24_000; 4],
        [48_000; 4],
        [48_000; 4],
        [48_000, 48_000, 12_000, 12_000],
    ];
    for beat in beats.iter().flatten() {
        expected.push(expected[expected.len() - 1] + beat);
    }
    assert_eq!(audio.frames(), expected[16]);
    assert_eq!(sounding_frames(audio.samples()), expected[..16]);

    // Played, the playhead reads the place in the song, not the time: a
    // bar and a half of frames into the slow second bar is its third beat.
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 96_000 + 96_000, 480);
    assert!((controller.frame().tick - (3_840.0 + 1_920.0)).abs() < 1e-6);
    // The tempo automation in charge is reported like any other.
    let automated = controller.frame().automated;
    assert_eq!(automated.len(), 1);
    assert_eq!(automated[0].automation, slow);
    assert!((automated[0].value - tempo(60.0)).abs() < 1e-6);

    // Playback that begins in the last bar is on that bar's time.
    controller.stop();
    run(&mut processor, 4_800, 480);
    controller.seek(11_520.0 + 960.0);
    controller.play();
    let out = run(&mut processor, 100_000, 480);
    assert_eq!(sounding_frames(&out), [0, 48_000, 60_000]);
}

#[test]
fn notes_and_clips_end_on_their_ticks_while_the_tempo_moves() {
    // A sampler with an envelope holds a steady sample for a bar, an audio
    // clip lasts from the second bar to the third, and the tempo climbs
    // from 60 to 180 over the four bars.
    let mut rig = Rig::new();
    let held = rig.channel(level(RATE, 0.25, 20.0));
    rig.sampler_mut(held).envelope = Some(Envelope {
        attack_ms: 0.0,
        decay_ms: 0.0,
        sustain: 1.0,
        release_ms: 1.0,
    });
    rig.note(held, 960, 3_840);
    rig.channel_mut(held).pan = -1.0;
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).length_steps = 64;
    rig.clip(lane, pattern, 0, 15_360);
    let track = rig.track();
    let audio = rig.playlist_track();
    rig.audio_clip(audio, level(RATE, 0.5, 20.0), track, 3_840, 7_680);
    rig.track_mut(track).pan = 1.0;
    let points = [(0, tempo(60.0)), (15_360, tempo(180.0))];
    automate(&mut rig, AutomationTarget::Tempo, &points, 0, 15_360);

    let out = rig.play_song(RATE, 430_000, 480);
    // The frame a tick of the song falls on, by the same sum as above.
    let frame_of = |tick: f64| {
        let reached = 60.0 + 120.0 * tick / 15_360.0;
        (ramp_seconds(tick, 60.0, reached) * f64::from(RATE) - 1e-6).ceil() as usize
    };
    // The note is on the left alone: it starts a beat in and is let go a
    // bar later, in ticks, wherever the tempo has put that.
    let left = left(&out);
    let sounding: Vec<usize> = (0..left.len()).filter(|f| left[*f] == 0.25).collect();
    assert_eq!(sounding[0], frame_of(960.0));
    assert_eq!(sounding[sounding.len() - 1], frame_of(4_800.0));
    assert_eq!(sounding.len(), frame_of(4_800.0) - frame_of(960.0) + 1);

    // The audio clip is on the right alone: it starts on its tick, plays
    // at its own speed, and is cut on the tick it ends on.
    let right = right(&out);
    let (start, end) = (frame_of(3_840.0), frame_of(11_520.0));
    assert!(right[..start].iter().all(|sample| *sample == 0.0));
    assert!(right[start] > 0.0);
    assert!(
        right[start + DECLICK..end - DECLICK]
            .iter()
            .all(|s| *s == 0.5)
    );
    assert!(right[end - 1] > 0.0);
    assert!(right[end..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn a_looping_song_keeps_its_tempo_map_every_time_around() {
    // Two bars: one at the stored tempo, one that speeds up to twice it.
    let (mut rig, _) = clicks();
    rig.project.playlist.clips[0].length = 7_680;
    let points = [(0, tempo(120.0)), (3_840, tempo(240.0))];
    automate(&mut rig, AutomationTarget::Tempo, &points, 3_840, 3_840);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..TransportPatch::default()
    });
    controller.play();
    let out = run(&mut processor, 500_000, 480);
    let clicks = sounding_frames(&out);
    // One time around: the first bar, and the second by the same sum.
    let bar = (ramp_seconds(3_840.0, 120.0, 240.0) * f64::from(RATE)).ceil() as usize;
    let around = 96_000 + bar;
    assert!((66_000..67_000).contains(&bar), "{bar}");
    let first: Vec<usize> = clicks.iter().copied().filter(|f| *f < around).collect();
    assert_eq!(first.len(), 8);
    assert_eq!(first[..5], [0, 24_000, 48_000, 72_000, 96_000]);
    // Every later time around is the first, to within the one frame the
    // loop point itself is rounded to.
    for (index, frame) in clicks.iter().enumerate() {
        let expected = first[index % 8] + index / 8 * around;
        assert!(frame.abs_diff(expected) <= 1 + index / 8, "click {index}");
    }
    assert!(clicks.len() >= 24);
}

#[test]
fn an_edit_to_the_tempo_curve_under_a_playing_song_keeps_its_place() {
    // Four bars at a stored 120 with a clip that holds 60 from the start.
    let (mut rig, _) = clicks();
    let lane = rig.playlist_track();
    let slow = rig.automation(AutomationTarget::Tempo, &[(0, tempo(60.0))]);
    rig.automation_clip(lane, slow, 0, 15_360);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    // Two and a half beats at 60: 120000 frames, tick 2400.
    let out = run(&mut processor, 120_000, 480);
    assert_eq!(sounding_frames(&out), [0, 48_000, 96_000]);
    assert!((controller.frame().tick - 2_400.0).abs() < 1e-6);

    // The curve is redrawn to 240 bpm. The song is where it was, and the
    // next beat, half a beat on, comes after 6000 frames.
    rig.automation_mut(slow).points[0].value = tempo(240.0);
    controller.set_project(&rig.project, &rig.pool);
    let out = run(&mut processor, 60_000, 480);
    assert_eq!(
        sounding_frames(&out),
        [6_000, 18_000, 30_000, 42_000, 54_000]
    );
    assert!((controller.frame().tick - (2_400.0 + 4_800.0)).abs() < 1e-6);

    // The clip is muted: the song carries on at its stored 120.
    rig.project.playlist.clips[1].muted = true;
    controller.set_project(&rig.project, &rig.pool);
    let out = run(&mut processor, 60_000, 480);
    assert_eq!(sounding_frames(&out), [12_000, 36_000]);
}
