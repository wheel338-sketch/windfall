//! The host gates docs/DELAY-FAMILY.md kept open for the echo bank and the
//! sixteen-band frequency delay: tails without end must not overflow the
//! host's sums, and their memory is admitted before anything is prepared.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use support::{Rig, sine};
use windfall_dsp::echo_bank::EchoBankParams;
use windfall_dsp::frequency_delay::FrequencyDelayParams;
use windfall_engine::{Processor, ProjectPreparationError, RenderOptions, render};
use windfall_project::*;

/// A bank whose first unit repeats every 30 ms and loses a tenth each time.
/// Any feedback above zero makes its reported tail endless.
fn bank(feedback: f32) -> EffectParams {
    let mut params = EchoBankParams {
        dry: 1.0,
        wet: 0.5,
        feedback: 1.0,
        ..Default::default()
    };
    params.units[0].sync = false;
    params.units[0].time_ms = 30.0;
    params.units[0].feedback = feedback;
    EffectParams::EchoBank(params)
}

fn bands(feedback: f32) -> EffectParams {
    let mut params = FrequencyDelayParams {
        dry: 1.0,
        wet: 0.5,
        feedback,
        ..Default::default()
    };
    for band in &mut params.bands {
        band.delay_ms = 20.0;
    }
    EffectParams::FrequencyDelay(params)
}

/// One quarter-second note through `effects` on the master.
fn rig(effects: &[EffectParams]) -> Rig {
    let mut rig = Rig::new();
    let channel = rig.channel(sine(48_000, 440.0, 0.25));
    rig.note(channel, 0, 240);
    for params in effects {
        rig.effect(TrackId::MASTER, *params);
    }
    rig
}

fn frames(rig: &Rig, tail_secs: f32, auto_tail: bool) -> usize {
    let options = RenderOptions {
        tail_secs,
        auto_tail,
        ..Default::default()
    };
    let audio = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(audio.samples().iter().all(|sample| sample.is_finite()));
    assert!(audio.samples().iter().any(|sample| sample.abs() > 0.001));
    audio.samples().len() / 2
}

#[test]
fn two_endless_tails_on_one_track_end_at_the_chosen_horizon() {
    // Both effects report a tail without end. Their sum used to overflow.
    let sustaining = rig(&[bank(0.9), bands(0.5)]);
    let body = frames(&sustaining, 0.0, false);
    // Half a second is too short for these echoes to die away, so an
    // automatic tail runs to the horizon the options give it, and stops.
    let fixed = frames(&sustaining, 0.5, false);
    assert_eq!(fixed, body + 24_000);
    assert_eq!(frames(&sustaining, 0.5, true), fixed);
}

#[test]
fn endless_tails_that_have_gone_quiet_are_asked_about_without_overflow() {
    // These echoes are below the silence level well inside the two-second
    // pattern, so the automatic tail asks the engine whether the track is
    // done. That question adds the two endless tails up. The bank's own
    // answer for how long silence must last is conservative (every unit's
    // whole 25 second history), so the render still runs its one allowed
    // second, and ends there with nothing wrapped round.
    let quiet = rig(&[bank(0.5), bands(0.3)]);
    let body = frames(&quiet, 0.0, false);
    assert_eq!(frames(&quiet, 1.0, true), body + 48_000);
    // Three endless tails in series, on a track that feeds the master.
    let mut routed = Rig::new();
    let track = routed.track();
    let channel = routed.channel_on(sine(48_000, 440.0, 0.25), track);
    routed.note(channel, 0, 240);
    for params in [bank(0.5), bands(0.3), bank(0.25)] {
        routed.effect(track, params);
    }
    routed.effect(TrackId::MASTER, bands(0.2));
    let body = frames(&routed, 0.0, false);
    assert_eq!(frames(&routed, 1.0, true), body + 48_000);
}

#[test]
fn a_plan_past_the_echo_history_limit_is_refused_and_the_old_one_plays_on() {
    let admitted = rig(&[bank(0.5)]);
    let play = |frames: usize, processor: &mut Processor| {
        let mut audio = vec![0.0_f32; frames * 2];
        for chunk in audio.chunks_mut(128) {
            processor.process(chunk);
        }
        audio
    };
    let (mut processor, controller) = Processor::new(48_000);
    controller.set_project(&admitted.project, &admitted.pool);
    assert_eq!(controller.project_preparation_error(), None);
    controller.play();
    let (mut twin, twin_controller) = Processor::new(48_000);
    twin_controller.set_project(&admitted.project, &admitted.pool);
    twin_controller.play();
    assert_eq!(play(4_800, &mut processor), play(4_800, &mut twin));

    // Fourteen banks at 48 kHz keep a little over 1 GiB between them.
    // A track admits only ten slots, so put seven on each of two tracks:
    // an overfull single-track fixture would prepare only its first ten.
    let mut refused = rig(&[bank(0.5); 7]);
    let track = refused.track();
    for _ in 0..7 {
        refused.effect(track, bank(0.5));
    }
    controller.set_project(&refused.project, &refused.pool);
    let Some(ProjectPreparationError::MemoryLimit { required, limit }) =
        controller.project_preparation_error()
    else {
        panic!(
            "expected a memory refusal, got {:?}",
            controller.project_preparation_error()
        );
    };
    assert_eq!(limit, 1 << 30);
    let one = EffectKind::EchoBank.history_bytes(48_000.0).unwrap();
    assert_eq!(required, 14 * one);
    // Nothing of the refused plan reached the audio thread.
    let after = play(9_600, &mut processor);
    assert!(after.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(after, play(9_600, &mut twin));

    // A plan that fits is taken again, and clears the refusal.
    let fits = rig(&[bank(0.5), bands(0.3)]);
    controller.set_project(&fits.project, &fits.pool);
    assert_eq!(controller.project_preparation_error(), None);
    assert!(
        play(4_800, &mut processor)
            .iter()
            .all(|sample| sample.is_finite())
    );
}
