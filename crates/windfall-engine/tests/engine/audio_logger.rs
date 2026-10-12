//! The audio logger through the public API: what its histories hold, how
//! they are bounded, and that the audio thread stays realtime-safe.

use windfall_engine::audio_logger::{AudioLogger, LoggerSpec, MAX_SOURCE_BYTES, QUEUE_SECONDS};
use windfall_engine::{Controller, Processor, ProjectRetirement, RenderOptions, render};
use windfall_ipc::{AudioLoggerSource, PlayMode};
use windfall_project::{ChannelId, TrackId};

use crate::realtime::allocator_calls;
use crate::support::{Rig, idle_limiter, plain_synth, rms, run};

/// Low enough that half a minute of history is quick to fill and to wrap.
const RATE: u32 = 8_000;
/// The transfer queue at [`RATE`].
const QUEUE: usize = (RATE * QUEUE_SECONDS) as usize;
/// The shortest history at [`RATE`].
const HISTORY: usize = RATE as usize * 30;

const MASTER: AudioLoggerSource = AudioLoggerSource::Master;

#[test]
fn effect_slot_tap_measures_its_chain_position_and_never_allocates() {
    use windfall_dsp::{BalanceParams, EffectParams};
    use windfall_dsp::sendtap::SendTapParams;
    let mut rig = Rig::new();
    let track = rig.track();
    let channel = rig.channel_on(crate::support::level(RATE, 0.4, 4.0), track);
    rig.effect(
        track,
        EffectParams::Balance(BalanceParams {
            gain: 0.5,
            ..Default::default()
        }),
    );
    let slot = rig.effect(track, EffectParams::SendTap(SendTapParams::default()));
    rig.effect(
        track,
        EffectParams::Balance(BalanceParams {
            gain: 0.25,
            ..Default::default()
        }),
    );
    let (mut processor, controller) = rig.processor(RATE);
    let logger = controller
        .start_audio_logger(&LoggerSpec {
            effects: vec![slot],
            ..spec(true, &[])
        })
        .unwrap();
    controller.note_on(channel, 60, 1.0);
    let played = run_logged(&mut processor, &logger, 4000);
    let tapped = logger
        .tail(AudioLoggerSource::Effect { effect: slot }, None)
        .unwrap();
    assert_eq!(tapped.samples.len(), played.len());
    for (tap, output) in tapped.samples[4000..].iter().zip(&played[4000..]) {
        assert!((*tap - 0.2).abs() < 1e-5);
        assert!((*output - *tap * 0.25).abs() < 1e-5);
    }
    let mut out = [0.0; 512];
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    controller.stop_audio_logger();
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
}

fn track(track: TrackId) -> AudioLoggerSource {
    AudioLoggerSource::Track { track }
}

fn spec(master: bool, tracks: &[TrackId]) -> LoggerSpec {
    LoggerSpec {
        seconds: 30,
        master,
        tracks: tracks.to_vec(),
        // The tests drain by hand, so what a history holds never depends on
        // when a worker thread happened to run.
        effects: Vec::new(),
        worker: false,
    }
}

/// Runs the processor and drains the logger often enough that nothing is
/// dropped, however many frames are asked for.
fn run_logged(processor: &mut Processor, logger: &AudioLogger, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    let mut left = frames;
    while left > 0 {
        let step = left.min(QUEUE / 2);
        out.extend(run(processor, step, 256));
        logger.pump();
        left -= step;
    }
    out
}

fn all(logger: &AudioLogger, source: AudioLoggerSource) -> Vec<f32> {
    logger.tail(source, None).unwrap().samples
}

fn held(logger: &AudioLogger, source: AudioLoggerSource) -> u64 {
    let stats = logger.stats();
    stats
        .iter()
        .find(|stats| stats.source == source)
        .unwrap()
        .available_frames
}

/// A synth on a mixer track of its own, held on one key so there is always
/// sound to log.
fn held_synth(rig: &mut Rig) -> (TrackId, ChannelId) {
    let track = rig.track();
    let synth = rig.synth_on(plain_synth(), track);
    (track, synth)
}

#[test]
fn the_master_history_is_exactly_what_was_played_and_wraps_to_the_newest() {
    let mut rig = Rig::new();
    let (_, synth) = held_synth(&mut rig);
    let (mut processor, controller) = rig.processor(RATE);
    let before = run(&mut processor, 1_000, 256);
    controller.note_on(synth, 60, 1.0);
    let logger = controller.start_audio_logger(&spec(true, &[])).unwrap();
    assert_eq!(logger.sample_rate(), RATE);
    assert_eq!(held(&logger, MASTER), 0);

    let first = run_logged(&mut processor, &logger, 50_000);
    assert!(rms(&first) > 0.05, "the synth sounds");
    assert!(rms(&before) == 0.0);
    assert!(
        all(&logger, MASTER) == first,
        "nothing from before it started"
    );
    let newest = logger.tail(MASTER, Some(1_234)).unwrap().samples;
    assert!(newest == first[(50_000 - 1_234) * 2..]);

    // Past its length the history holds the newest half minute, in order.
    let mut played = first;
    played.extend(run_logged(&mut processor, &logger, HISTORY + 12_345));
    let stats = logger.stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].capacity_frames, HISTORY as u64);
    assert_eq!(stats[0].available_frames, HISTORY as u64);
    assert_eq!(stats[0].dropped_frames, 0);
    assert_eq!(stats[0].sample_rate, RATE);
    let newest = &played[played.len() - HISTORY * 2..];
    assert!(all(&logger, MASTER) == newest);
    let audio = logger.tail(MASTER, Some(HISTORY as u64 * 9)).unwrap();
    assert_eq!(audio.sample_rate, RATE);
    assert!(
        audio.samples == newest,
        "more than there is gives all there is"
    );
    assert!(logger.tail(track(TrackId(9)), None).is_err());
}

#[test]
fn memory_is_the_history_plus_the_queue_and_never_more_than_the_bound() {
    let (_processor, controller) = Processor::new(RATE);
    let logger = controller
        .start_audio_logger(&LoggerSpec {
            seconds: 600,
            master: true,
            tracks: vec![TrackId::MASTER],
            effects: Vec::new(),
            worker: true,
        })
        .unwrap();
    for stats in logger.stats() {
        assert_eq!(stats.capacity_frames, u64::from(RATE) * 600);
        assert_eq!(
            stats.memory_bytes,
            (u64::from(RATE) * 600 + QUEUE as u64) * 8
        );
    }
    drop(logger);

    // A long history at a high rate is cut to the bound, and says so.
    let (_processor, controller) = Processor::new(192_000);
    let logger = controller
        .start_audio_logger(&LoggerSpec {
            seconds: 600,
            master: true,
            tracks: Vec::new(),
            effects: Vec::new(),
            worker: false,
        })
        .unwrap();
    let stats = logger.stats()[0];
    assert_eq!(stats.capacity_frames, MAX_SOURCE_BYTES / 8);
    assert!(stats.capacity_frames < 192_000 * 600);
    assert_eq!(
        stats.memory_bytes,
        MAX_SOURCE_BYTES + u64::from(192_000 * QUEUE_SECONDS) * 8
    );
}

#[test]
fn a_queue_nobody_drains_drops_frames_and_counts_every_one() {
    let mut rig = Rig::new();
    let (_, synth) = held_synth(&mut rig);
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(synth, 60, 1.0);
    let logger = controller.start_audio_logger(&spec(true, &[])).unwrap();

    // More than the queue holds, with no drain in between.
    let total = QUEUE + 7_777;
    let played = run(&mut processor, total, 256);
    let stats = logger.stats()[0];
    assert_eq!(stats.dropped_frames, 7_777);
    assert_eq!(stats.available_frames, QUEUE as u64);
    // What fitted is intact and in order: the callback dropped the newest.
    assert!(all(&logger, MASTER) == played[..QUEUE * 2]);

    // Draining makes room again, and the count stays as a record.
    let more = run_logged(&mut processor, &logger, 3_000);
    let stats = logger.stats()[0];
    assert_eq!(stats.dropped_frames, 7_777);
    assert_eq!(stats.available_frames, (QUEUE + 3_000) as u64);
    assert!(logger.tail(MASTER, Some(3_000)).unwrap().samples == more);
}

#[test]
fn a_track_is_logged_after_its_effects_and_before_its_fader() {
    // 40 frames of look-ahead at this rate: what leaves the effect is late.
    const LATE: usize = 40;
    let logged_start = |with_effect: bool| {
        let mut rig = Rig::new();
        let (logged, synth) = held_synth(&mut rig);
        if with_effect {
            rig.effect(logged, idle_limiter(5.0));
        }
        rig.track_mut(logged).volume = 0.5;
        let (mut processor, controller) = rig.processor(RATE);
        let logger = controller
            .start_audio_logger(&spec(true, &[logged]))
            .unwrap();
        controller.note_on(synth, 60, 1.0);
        let played = run_logged(&mut processor, &logger, 4_000);
        let logged = all(&logger, track(logged));
        assert!(rms(&logged) > 0.05);
        // The fader comes after the log: the master is the log at half level.
        let faded: Vec<f32> = logged.iter().map(|sample| sample * 0.5).collect();
        assert!(faded == played);
        logged[..LATE * 2].to_vec()
    };
    // Without the effect the note is there at once; with it the log starts
    // with the effect's delay, so the log is taken after the effect.
    assert!(rms(&logged_start(false)) > 0.01);
    assert_eq!(rms(&logged_start(true)), 0.0);
}

#[test]
fn a_logged_track_is_followed_across_plans_and_stops_when_it_is_removed() {
    let mut rig = Rig::new();
    let first = rig.track();
    let (logged, synth) = held_synth(&mut rig);
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(synth, 60, 1.0);
    let logger = controller
        .start_audio_logger(&spec(true, &[logged]))
        .unwrap();
    let mut played = run_logged(&mut processor, &logger, 3_000);

    // Another plan, in which the logged track sits in another place.
    rig.project.mixer.tracks.retain(|track| track.id != first);
    controller.set_project(&rig.project, &rig.pool);
    played.extend(run_logged(&mut processor, &logger, 3_000));
    let later = rig.track();
    rig.project.mixer.tracks.swap(1, 2);
    assert_eq!(rig.project.mixer.tracks[1].id, later);
    controller.set_project(&rig.project, &rig.pool);
    played.extend(run_logged(&mut processor, &logger, 3_000));
    assert!(rms(&played[played.len() - 2_000..]) > 0.05);
    assert_eq!(held(&logger, track(logged)), 9_000);
    assert!(all(&logger, track(logged)) == played);
    assert!(all(&logger, MASTER) == played);

    // Without the track nothing more arrives for it; the master goes on.
    rig.channel_mut(synth).mixer_track = TrackId::MASTER;
    rig.project.mixer.tracks.retain(|track| track.id != logged);
    controller.set_project(&rig.project, &rig.pool);
    run_logged(&mut processor, &logger, 3_000);
    assert_eq!(held(&logger, track(logged)), 9_000);
    assert_eq!(held(&logger, MASTER), 12_000);
    assert!(
        all(&logger, track(logged)) == played,
        "what it held is kept"
    );
    assert_eq!(logger.stats()[1].dropped_frames, 0);
}

#[test]
fn starting_again_resizes_and_every_old_tap_is_retired_to_the_control_side() {
    let mut rig = Rig::new();
    let (_, synth) = held_synth(&mut rig);
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(synth, 60, 1.0);
    let mut retirement = ProjectRetirement::default();
    let mut collect = |controller: &Controller| {
        controller.take_retired(&mut retirement);
        retirement.clear();
    };

    let short = controller.start_audio_logger(&spec(true, &[])).unwrap();
    run_logged(&mut processor, &short, 2_000);
    assert!(!short.detached());

    // A longer history is a new logger: empty, and of the new size.
    let long = controller
        .start_audio_logger(&LoggerSpec {
            seconds: 60,
            ..spec(true, &[])
        })
        .unwrap();
    let played = run_logged(&mut processor, &long, 2_000);
    assert_eq!(long.stats()[0].capacity_frames, u64::from(RATE) * 60);
    assert!(all(&long, MASTER) == played);
    // The audio thread sent the first tap back instead of dropping it.
    assert!(!short.detached());
    collect(&controller);
    assert!(short.detached());
    assert_eq!(held(&short, MASTER), 2_000, "its history is still readable");
    drop(short);

    // Disabling takes the tap away the same way, and logging stops.
    controller.stop_audio_logger();
    run(&mut processor, 2_000, 256);
    assert!(!long.detached());
    collect(&controller);
    assert!(long.detached());
    assert_eq!(held(&long, MASTER), 2_000);
}

#[test]
fn processing_with_the_logger_on_never_touches_the_allocator() {
    let mut rig = Rig::new();
    let (logged, synth) = held_synth(&mut rig);
    rig.effect(logged, idle_limiter(5.0));
    let (mut processor, controller) = rig.processor(48_000);
    controller.note_on(synth, 60, 1.0);
    let mut out = vec![0.0_f32; 2 * 480];
    processor.process(&mut out);

    let logger = controller
        .start_audio_logger(&spec(true, &[logged, TrackId::MASTER, TrackId(404)]))
        .unwrap();
    // Taking the tap up, then copying into the queues.
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    let calls = allocator_calls(|| {
        for _ in 0..100 {
            processor.process(&mut out);
        }
    });
    assert_eq!(calls, 0);
    assert_eq!(held(&logger, track(logged)), 101 * 480);

    // Overflowing: two seconds of queue, three seconds of audio, no drain.
    let calls = allocator_calls(|| {
        for _ in 0..300 {
            processor.process(&mut out);
        }
    });
    assert_eq!(calls, 0);
    assert_eq!(logger.stats()[0].dropped_frames, 300 * 480 - 96_000);

    // Replacing the tap and taking it away retire, and do not free.
    let again = controller
        .start_audio_logger(&spec(true, &[logged]))
        .unwrap();
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    controller.stop_audio_logger();
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    assert!(!logger.detached() && !again.detached());
}

#[test]
fn an_offline_render_carries_no_logger_and_is_not_changed_by_one() {
    let mut rig = Rig::new();
    let (logged, synth) = held_synth(&mut rig);
    rig.steps(synth, &[0, 4, 8, 12]);
    let options = RenderOptions {
        region: None,
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern: None,
        pattern_loops: 2,
        tail_secs: 0.25,
        auto_tail: false,
        block_frames: 256,
    };
    let alone = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(rms(alone.samples()) > 0.01);

    // A live engine on the same project, logging, while the render runs.
    let (mut processor, controller) = rig.processor(RATE);
    let logger = controller
        .start_audio_logger(&spec(true, &[logged]))
        .unwrap();
    run_logged(&mut processor, &logger, 1_000);
    let beside = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(beside.samples() == alone.samples());
    // The render fed nothing to the live logger's histories.
    assert_eq!(held(&logger, MASTER), 1_000);
    assert_eq!(held(&logger, track(logged)), 1_000);
    assert_eq!(rms(&all(&logger, MASTER)), 0.0);
}
