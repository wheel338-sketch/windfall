//! Selectable analyzer captures through the public engine API: a processor
//! run by hand, real mixed audio, no device.
//!
//! Every expectation is worked out here from what the test put into the
//! project, or read from the audio the processor wrote, never from the
//! analyzer under test.

#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;
#[path = "../src/test_alloc.rs"]
mod test_alloc;

use std::sync::{Mutex, MutexGuard};

use support::{Rig, level, sine};
use windfall_core::AudioBuffer;
use windfall_dsp::tuner::TunerParams;
use windfall_dsp::{EffectObservation, EffectParams, LimiterMode, LimiterParams};
use windfall_engine::analyzers::{self, Dbfs, Source};
use windfall_engine::{
    CAPTURE_SEATS, CaptureRefusal, CaptureRequest, CaptureState, CaptureTicket, Controller,
    Processor,
};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{ChannelId, EffectId, TrackId};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

const RATE: u32 = 48_000;
/// One analysis window of the default analyzer.
const WINDOW: usize = 1024;

/// The analyzer's quotas are counted for the whole process, so the tests of
/// this file take turns.
static TURN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    let guard = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(
        analyzers::usage().taps,
        0,
        "an earlier test left an analyzer alive"
    );
    guard
}

fn request(source: Source) -> CaptureRequest {
    CaptureRequest {
        project: 1,
        source,
        effect: None,
    }
}

/// Runs `frames` frames in device buffers of `block` and returns the output.
/// Nothing in it may touch the allocator.
fn run(processor: &mut Processor, frames: usize, block: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    for chunk in out.chunks_mut(block * 2) {
        assert_eq!(
            test_alloc::allocator_calls(|| processor.process(chunk)),
            0,
            "the audio callback allocated or freed"
        );
    }
    out
}

/// What one look at a capture saw, copied out of the borrowed view.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    state: CaptureState,
    peak: [Option<f64>; 2],
    rms: [Option<f64>; 2],
    invalid: [u32; 2],
    spectrum_valid: [bool; 2],
    first_frame: u64,
    end_frame: u64,
    clock: u64,
    pdc: u32,
    rate: u32,
    resets: u64,
    dropped_packets: u64,
    reduction: Option<(EffectId, f32)>,
    observations: Vec<windfall_engine::CapturedObservation>,
    effect_present: bool,
    missing_blocks: u64,
    project: u64,
    source: Option<Source>,
}

fn look(controller: &Controller, seat: usize, ticket: CaptureTicket) -> Seen {
    controller.capture_poll(seat, ticket, |view| {
        let snapshot = view.snapshot;
        let levels = |side: usize| snapshot.and_then(|s| s.channels[side].levels);
        Seen {
            state: view.state,
            peak: [0, 1].map(|side| levels(side).map(|l| l.sample_peak)),
            rms: [0, 1].map(|side| levels(side).map(|l| l.rms)),
            invalid: snapshot.map_or([0; 2], |s| {
                [0, 1].map(|side| s.channels[side].invalid_samples)
            }),
            spectrum_valid: snapshot.map_or([false; 2], |s| s.spectrum_valid),
            first_frame: snapshot.map_or(0, |s| s.range.first_frame),
            end_frame: snapshot.map_or(0, |s| s.range.end_frame),
            clock: snapshot.map_or(0, |s| s.range.clock.0),
            pdc: snapshot.map_or(0, |s| s.range.pdc_frames),
            rate: snapshot.map_or(0, |s| s.range.sample_rate),
            resets: view.counters.resets,
            dropped_packets: view.counters.dropped_packets,
            reduction: snapshot
                .and_then(|s| s.gain_reduction)
                .map(|reduction| (reduction.effect, reduction.db)),
            observations: view.observations.to_vec(),
            effect_present: view.effect_present,
            missing_blocks: view.seat.missing_source_blocks,
            project: snapshot.map_or(0, |s| s.selection.project.0),
            source: snapshot.map(|s| s.selection.source),
        }
    })
}

/// Runs audio and looks often enough that the capture's queue never fills,
/// until a result newer than `after` arrives.
fn settle(
    processor: &mut Processor,
    controller: &Controller,
    seat: usize,
    ticket: CaptureTicket,
    after: u64,
) -> Seen {
    for _ in 0..40 {
        run(processor, 512, 256);
        let seen = look(controller, seat, ticket);
        if seen.state == CaptureState::Ready && seen.end_frame > after {
            return seen;
        }
    }
    panic!(
        "no fresh analysis arrived: {:?}",
        look(controller, seat, ticket)
    );
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1.0e-6)
}

/// Two tracks holding different steady levels, so every source is told
/// apart by its level alone: track A 0.25, track B 0.5 through a fader at
/// 0.8, the master their sum, and the mix output half the master.
struct Fixture {
    rig: Rig,
    a: TrackId,
    b: TrackId,
    channels: [ChannelId; 2],
}

const A: f64 = 0.25;
const B: f64 = 0.5 * 0.8;
const OUTPUT_GAIN: f32 = 0.5;

fn fixture() -> Fixture {
    let mut rig = Rig::new();
    let (a, b) = (rig.track(), rig.track());
    rig.track_mut(b).volume = 0.8;
    let channels = [
        rig.channel_on(level(RATE, 0.25, 30.0), a),
        rig.channel_on(level(RATE, 0.5, 30.0), b),
    ];
    Fixture {
        rig,
        a,
        b,
        channels,
    }
}

impl Fixture {
    fn start(&self) -> (Processor, Controller) {
        let (mut processor, controller) = self.rig.processor(RATE);
        controller.set_output_gain(OUTPUT_GAIN);
        for channel in self.channels {
            controller.note_on(channel, 60, 1.0);
        }
        // The output gain, the project and the notes are all in.
        run(&mut processor, 256, 256);
        (processor, controller)
    }
}

#[test]
fn each_source_reports_its_own_audio_and_a_new_selection_never_shows_the_old_one() {
    let _turn = turn();
    let fixture = fixture();
    let (mut processor, controller) = fixture.start();

    let capability = controller.capture_capability();
    assert_eq!(capability.seats, CAPTURE_SEATS);
    assert_eq!(capability.stream.map(|stream| stream.1), Some(RATE));
    assert_eq!(capability.config.fft_size, WINDOW);

    let on_a = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.a)))
        .unwrap();
    let on_mix = controller
        .capture_select(1, request(Source::MixOutput))
        .unwrap();
    assert_ne!(on_a.selection, on_mix.selection);
    assert_eq!(on_a.sample_rate, RATE);
    // Selected is not live: nothing is shown before real audio has arrived.
    assert_eq!(
        look(&controller, 0, on_a).state,
        CaptureState::AwaitingAudio
    );
    assert_eq!(
        look(&controller, 1, on_mix).state,
        CaptureState::AwaitingAudio
    );
    run(&mut processor, WINDOW - 256, 256);
    assert_eq!(
        look(&controller, 0, on_a).state,
        CaptureState::AwaitingAudio
    );

    let out = run(&mut processor, 512, 256);
    let track = look(&controller, 0, on_a);
    let mix = look(&controller, 1, on_mix);
    assert_eq!(track.state, CaptureState::Ready);
    assert_eq!(track.source, Some(Source::TrackPostFader(fixture.a)));
    assert_eq!(track.project, 1);
    assert!(
        close(track.peak[0], A) && close(track.peak[1], A),
        "{track:?}"
    );
    assert!(close(track.rms[0], A), "{track:?}");
    // The mix is what the device was given: the sum, after the output gain.
    let heard = f64::from(out[out.len() - 2]);
    assert!((heard - (A + B) * f64::from(OUTPUT_GAIN)).abs() < 1.0e-6);
    assert!(
        close(mix.peak[0], heard) && close(mix.peak[1], heard),
        "{mix:?}"
    );
    assert_eq!(mix.source, Some(Source::MixOutput));
    assert_eq!((track.rate, mix.rate), (RATE, RATE));
    assert_eq!(track.end_frame - track.first_frame, WINDOW as u64);

    // The master track after its fader is the sum without the output gain,
    // which is not the mix output.
    let on_master = controller
        .capture_select(1, request(Source::TrackPostFader(TrackId::MASTER)))
        .unwrap();
    assert_eq!(look(&controller, 1, on_mix).state, CaptureState::Stale);
    let master = settle(&mut processor, &controller, 1, on_master, 0);
    assert!(close(master.peak[0], A + B), "{master:?}");

    // Track A's seat moves to track B. The old ticket shows nothing from
    // then on, and the new one shows nothing until B's own audio has filled
    // a whole window: A's level never appears under B's ticket.
    let on_b = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.b)))
        .unwrap();
    assert_eq!(look(&controller, 0, on_a).state, CaptureState::Stale);
    let mut first = None;
    for _ in 0..12 {
        run(&mut processor, 256, 256);
        let seen = look(&controller, 0, on_b);
        if seen.state == CaptureState::Ready {
            first = Some(seen);
            break;
        }
        assert_eq!(seen.state, CaptureState::AwaitingAudio);
        assert_eq!(seen.peak, [None, None]);
    }
    let first = first.expect("track B reported");
    assert!(
        close(first.peak[0], B) && close(first.rms[1], B),
        "{first:?}"
    );
    assert_eq!(first.source, Some(Source::TrackPostFader(fixture.b)));
    assert_eq!(look(&controller, 0, on_a).state, CaptureState::Stale);

    // Clearing a seat ends its selection.
    assert!(controller.capture_clear(0));
    assert_eq!(look(&controller, 0, on_b).state, CaptureState::Stale);
    run(&mut processor, 512, 256);

    drop(processor);
    assert_eq!(
        look(&controller, 1, on_master).state,
        CaptureState::NoStream
    );
    assert_eq!(controller.capture_capability().stream, None);
}

#[test]
fn selections_are_checked_against_the_project_and_refusals_change_nothing() {
    let _turn = turn();
    let mut fixture = fixture();
    let limiter = fixture
        .rig
        .effect(fixture.a, EffectParams::Limiter(LimiterParams::default()));
    let (mut processor, controller) = fixture.start();
    let on_a = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.a)))
        .unwrap();
    let before = settle(&mut processor, &controller, 0, on_a, 0);

    let refuse = |seat, source, effect| {
        controller
            .capture_select(
                seat,
                CaptureRequest {
                    project: 1,
                    source,
                    effect,
                },
            )
            .unwrap_err()
    };
    assert_eq!(
        refuse(CAPTURE_SEATS, Source::MixOutput, None),
        CaptureRefusal::Seat
    );
    assert_eq!(
        refuse(0, Source::TrackPostFader(TrackId(9_999)), None),
        CaptureRefusal::SourceMissing
    );
    assert_eq!(
        refuse(0, Source::TrackPostFader(fixture.a), Some(EffectId(9_999))),
        CaptureRefusal::EffectMissing
    );
    // An effect's own readings come with its own track only.
    assert_eq!(
        refuse(0, Source::TrackPostFader(fixture.b), Some(limiter)),
        CaptureRefusal::EffectNotOnSource
    );
    assert_eq!(
        refuse(0, Source::MixOutput, Some(limiter)),
        CaptureRefusal::EffectNotOnSource
    );
    // None of that disturbed what the seat was capturing.
    let after = settle(&mut processor, &controller, 0, on_a, before.end_frame);
    assert_eq!(after.state, CaptureState::Ready);
    assert_eq!(after.resets, before.resets);

    // Two selections with no audio in between: the audio thread still holds
    // the first one's hand-over, so the second is told to come back.
    let next = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.b)))
        .unwrap();
    assert_eq!(
        refuse(0, Source::TrackPostFader(fixture.a), None),
        CaptureRefusal::Busy
    );
    assert_eq!(
        look(&controller, 0, next).state,
        CaptureState::AwaitingAudio
    );
    let seen = settle(&mut processor, &controller, 0, next, 0);
    assert!(close(seen.peak[0], B));
    drop(processor);
}

#[test]
fn a_seek_a_transport_change_and_a_new_plan_never_share_a_window_with_what_came_before() {
    let _turn = turn();
    let mut fixture = fixture();
    let (mut processor, controller) = fixture.start();
    let ticket = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.a)))
        .unwrap();
    let mut seen = settle(&mut processor, &controller, 0, ticket, 0);

    type Discontinuity = Box<dyn Fn(&Controller)>;
    let mut breaks: Vec<(&str, Discontinuity)> = vec![
        ("seek", Box::new(|controller| controller.seek(960.0))),
        ("play", Box::new(|controller| controller.play())),
        ("stop", Box::new(|controller| controller.stop())),
        (
            "song mode",
            Box::new(|controller| {
                controller.set_transport(TransportPatch {
                    mode: Some(PlayMode::Song),
                    ..TransportPatch::default()
                })
            }),
        ),
    ];
    for (what, act) in breaks.drain(..) {
        // Where the engine's clock stands when the break is made.
        let at = seen.end_frame.next_multiple_of(256);
        act(&controller);
        // The request is taken at the next buffer. From then on the old
        // evidence is gone at once, before any new audio is analyzed.
        run(&mut processor, 256, 256);
        let gone = look(&controller, 0, ticket);
        assert_eq!(gone.state, CaptureState::AwaitingAudio, "{what}");
        assert!(gone.resets > seen.resets, "{what}");
        let fresh = settle(&mut processor, &controller, 0, ticket, at);
        assert!(fresh.clock > seen.clock, "{what}: {fresh:?}");
        // Its whole window lies after the break.
        assert!(fresh.first_frame >= at, "{what}: {fresh:?} from {at}");
        seen = fresh;
    }

    // An ordinary loop is no break: the pattern wraps and the audio and its
    // analysis run on without a gap.
    controller.set_transport(TransportPatch {
        mode: Some(PlayMode::Pattern),
        ..TransportPatch::default()
    });
    run(&mut processor, 256, 256);
    controller.play();
    seen = settle(
        &mut processor,
        &controller,
        0,
        ticket,
        seen.end_frame + 2_048,
    );
    let (clock, resets) = (seen.clock, seen.resets);
    let started = controller.frame().tick;
    let mut wrapped = false;
    for _ in 0..400 {
        run(&mut processor, 1_024, 256);
        let next = look(&controller, 0, ticket);
        assert_eq!(next.state, CaptureState::Ready);
        assert_eq!((next.clock, next.resets), (clock, resets));
        assert!(next.end_frame >= seen.end_frame);
        seen = next;
        if controller.frame().tick < started {
            wrapped = true;
            break;
        }
    }
    assert!(wrapped, "the pattern never looped");

    // Stopping let go of the notes played by hand. Play track A's again.
    controller.stop();
    run(&mut processor, 256, 256);
    controller.note_on(fixture.channels[0], 60, 1.0);
    seen = settle(
        &mut processor,
        &controller,
        0,
        ticket,
        seen.end_frame + 4_096,
    );
    assert!(close(seen.peak[0], A), "{seen:?}");

    // An edit hands the audio thread a new plan: again a clean break.
    fixture.rig.track_mut(fixture.a).volume = 0.5;
    let at = seen.end_frame.next_multiple_of(256);
    controller.set_project(&fixture.rig.project, &fixture.rig.pool);
    run(&mut processor, 256, 256);
    assert_eq!(
        look(&controller, 0, ticket).state,
        CaptureState::AwaitingAudio
    );
    // The fader glides to its new place; once there the level is the new one.
    let mut edited = settle(&mut processor, &controller, 0, ticket, at);
    for _ in 0..40 {
        if close(edited.peak[0], A * 0.5) && close(edited.rms[0], A * 0.5) {
            break;
        }
        edited = settle(&mut processor, &controller, 0, ticket, edited.end_frame);
    }
    assert!(close(edited.rms[0], A * 0.5), "{edited:?}");
    assert!(edited.first_frame >= at);
    drop(processor);
}

#[test]
fn a_deleted_source_shows_nothing_and_a_new_stream_or_rate_makes_old_tickets_stale() {
    let _turn = turn();
    let mut fixture = fixture();
    let (mut processor, controller) = fixture.start();
    let on_b = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.b)))
        .unwrap();
    let on_mix = controller
        .capture_select(1, request(Source::MixOutput))
        .unwrap();
    let seen = settle(&mut processor, &controller, 0, on_b, 0);
    assert!(close(seen.peak[0], B));

    // Track B leaves the project.
    let track = fixture.b;
    fixture
        .rig
        .project
        .channels
        .retain(|c| c.mixer_track != track);
    fixture.rig.project.mixer.tracks.retain(|t| t.id != track);
    controller.set_project(&fixture.rig.project, &fixture.rig.pool);
    // Even before the audio thread has the new plan nothing of B is shown.
    let gone = look(&controller, 0, on_b);
    assert_eq!(gone.state, CaptureState::SourceMissing);
    assert_eq!(gone.peak, [None, None]);
    run(&mut processor, 2_048, 256);
    let gone = look(&controller, 0, on_b);
    assert_eq!(gone.state, CaptureState::SourceMissing);
    assert_eq!(gone.peak, [None, None]);
    assert!(gone.missing_blocks >= 7, "{gone:?}");
    // The mix goes on, now without B.
    let mix = settle(
        &mut processor,
        &controller,
        1,
        on_mix,
        seen.end_frame + 4_096,
    );
    assert!(close(mix.peak[0], A * f64::from(OUTPUT_GAIN)), "{mix:?}");
    assert_eq!(
        controller
            .capture_select(0, request(Source::TrackPostFader(track)))
            .unwrap_err(),
        CaptureRefusal::SourceMissing
    );

    // The stream is reopened at another rate. Tickets of the old stream
    // show nothing, and new ones carry the new rate.
    drop(processor);
    assert_eq!(look(&controller, 1, on_mix).state, CaptureState::NoStream);
    let mut processor = Processor::reopen(&controller, 44_100);
    assert_eq!(look(&controller, 1, on_mix).state, CaptureState::Stale);
    assert_eq!(look(&controller, 0, on_b).state, CaptureState::Stale);
    let reopened = controller
        .capture_select(1, request(Source::MixOutput))
        .unwrap();
    assert_ne!(reopened.stream, on_mix.stream);
    assert_eq!(reopened.sample_rate, 44_100);
    controller.note_on(fixture.channels[0], 60, 1.0);
    let mix = settle(&mut processor, &controller, 1, reopened, 0);
    assert_eq!(mix.rate, 44_100);
    assert_eq!(look(&controller, 1, on_mix).state, CaptureState::Stale);
    drop(processor);
}

#[test]
fn a_look_that_comes_too_late_loses_audio_openly_and_recovers_on_a_fresh_window() {
    let _turn = turn();
    let fixture = fixture();
    let (mut processor, controller) = fixture.start();
    let ticket = controller
        .capture_select(0, request(Source::TrackPostFader(fixture.a)))
        .unwrap();
    let seen = settle(&mut processor, &controller, 0, ticket, 0);
    assert_eq!(seen.dropped_packets, 0);

    // Nobody looks for 40 blocks, and the queue holds 32. Playback goes on,
    // and the device output is untouched by the overflow.
    let out = run(&mut processor, 40 * 256, 256);
    assert!((f64::from(out[out.len() - 1]) - (A + B) * f64::from(OUTPUT_GAIN)).abs() < 1.0e-6);
    let late = look(&controller, 0, ticket);
    // What was in the queue is thrown away with the overflow: no window is
    // pieced together across the hole.
    assert_eq!(late.state, CaptureState::AwaitingAudio);
    assert!(late.dropped_packets >= 1, "{late:?}");
    assert!(late.resets > seen.resets);

    let back = settle(&mut processor, &controller, 0, ticket, seen.end_frame);
    assert!(close(back.peak[0], A));
    assert!(back.first_frame >= seen.end_frame + 32 * 256, "{back:?}");
    assert!(
        back.clock == seen.clock,
        "an overflow is not a transport break"
    );
    drop(processor);
}

/// A stereo sample with every kind of bad number in it.
fn broken(seconds: f64) -> AudioBuffer {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut data = vec![0.125_f32; frames * 2];
    for frame in (0..frames).step_by(97) {
        data[frame * 2] = f32::NAN;
    }
    for frame in (0..frames).step_by(211) {
        data[frame * 2 + 1] = f32::INFINITY;
    }
    for frame in (0..frames).step_by(307) {
        data[frame * 2] = f32::NEG_INFINITY;
    }
    for frame in (0..frames).step_by(401) {
        data[frame * 2 + 1] = 3.0e38;
    }
    AudioBuffer::from_interleaved(RATE, 2, data)
}

#[test]
fn listening_to_the_mix_changes_no_bit_of_the_device_output_and_reports_bad_samples_as_bad() {
    let _turn = turn();
    let mut rig = Rig::new();
    let track = rig.track();
    let bad = rig.channel_on(broken(4.0), track);
    let good = rig.channel(sine(RATE, 440.0, 4.0));

    let start = |capture: bool| {
        let (mut processor, controller) = rig.processor(RATE);
        controller.set_output_gain(0.75);
        let ticket = capture.then(|| {
            controller
                .capture_select(0, request(Source::MixOutput))
                .unwrap()
        });
        run(&mut processor, 256, 256);
        controller.note_on(bad, 60, 1.0);
        controller.note_on(good, 60, 1.0);
        (processor, controller, ticket)
    };
    let (mut plain, plain_controller, _) = start(false);
    let (mut heard, heard_controller, ticket) = start(true);
    let ticket = ticket.unwrap();

    let mut invalid_seen = 0;
    let mut frames = 0;
    // Device buffers of every awkward size, a gain ramp part way through,
    // and a gain above one that turns a huge finite sample into infinity.
    for (round, block) in [1, 7, 64, 256, 300, 1_000, 96, 513, 2_048, 33]
        .into_iter()
        .cycle()
        .take(60)
        .enumerate()
    {
        let gain = match round {
            20 => Some(0.1),
            35 => Some(1.9),
            _ => None,
        };
        if let Some(gain) = gain {
            plain_controller.set_output_gain(gain);
            heard_controller.set_output_gain(gain);
        }
        let a = run(&mut plain, block, block);
        let b = run(&mut heard, block, block);
        for (index, (a, b)) in a.iter().zip(&b).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "round {round}, sample {index}: {a} vs {b}"
            );
            assert!(!a.is_nan(), "the device was handed a NaN");
        }
        frames += block;
        let seen = look(&heard_controller, 0, ticket);
        invalid_seen += seen.invalid[0] + seen.invalid[1];
        if seen.state == CaptureState::Ready && seen.invalid[0] > 0 {
            // A side with a bad sample has no levels and no spectrum: it is
            // never shown as healthy audio.
            assert_eq!(seen.peak[0], None);
            assert!(!seen.spectrum_valid[0]);
        }
    }
    assert!(frames > 4 * WINDOW);
    assert!(
        invalid_seen > 0,
        "the mix capture never saw the bad samples"
    );
    drop((plain, heard));
}

#[test]
fn a_limiter_sends_its_own_levels_and_reduction_block_by_block_and_keeps_its_meter() {
    let _turn = turn();
    let mut rig = Rig::new();
    let track = rig.track();
    let other = rig.track();
    // A full-scale-and-a-half square wave against a ceiling of -6 dB.
    let loud = AudioBuffer::from_interleaved(
        RATE,
        1,
        (0..RATE as usize * 8)
            .map(|n| if (n / 48) % 2 == 0 { 0.75 } else { -0.75 })
            .collect(),
    );
    let channel = rig.channel_on(loud, track);
    let limiter = rig.effect(
        track,
        EffectParams::Limiter(LimiterParams {
            ceiling_db: -6.0,
            input_gain_db: 6.0,
            lookahead_ms: 5.0,
            ..LimiterParams::default()
        }),
    );
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let bound = CaptureRequest {
        project: 1,
        source: Source::TrackPostFader(track),
        effect: Some(limiter),
    };
    let ticket = controller.capture_select(0, bound).unwrap();
    let on_other = controller
        .capture_select(1, request(Source::TrackPostFader(other)))
        .unwrap();

    // 0.75 driven up 6 dB is 1.4963 against a ceiling of 0.5012: 9.5 dB.
    let expected_db = 20.0 * (0.75_f64 * 10.0_f64.powf(0.3) / 10.0_f64.powf(-0.3)).log10();
    let mut observations = Vec::new();
    let mut last = None;
    let mut metered = 0.0_f32;
    for _ in 0..40 {
        run(&mut processor, 512, 256);
        let seen = look(&controller, 0, ticket);
        assert!(seen.effect_present);
        observations.extend(seen.observations.iter().copied());
        if seen.state == CaptureState::Ready {
            last = Some(seen);
        }
        // The meter the mixer strip reads is fed as before.
        metered = metered.max(
            controller
                .frame()
                .gain_reductions
                .iter()
                .find(|reading| reading.effect == limiter)
                .map_or(0.0, |reading| reading.db),
        );
    }
    let last = last.expect("the limiter's track reported");
    // One observation per 256-frame block, end to end on the engine's clock.
    assert!(observations.len() >= 70, "{}", observations.len());
    for pair in observations.windows(2) {
        assert_eq!(
            pair[0].first_frame + u64::from(pair[0].frames),
            pair[1].first_frame
        );
        assert_eq!(pair[0].effect, limiter);
        assert_eq!(pair[0].selection, ticket.selection);
    }
    let mut deepest = 0.0_f32;
    for packet in &observations[20..] {
        let EffectObservation::Dynamics(dynamics) = packet.observation else {
            panic!("a limiter reports dynamics");
        };
        assert!((f64::from(dynamics.input_peak) - 0.75 * 10.0_f64.powf(0.3)).abs() < 1.0e-3);
        assert!((f64::from(dynamics.output_peak) - 10.0_f64.powf(-0.3)).abs() < 1.0e-3);
        assert!((f64::from(dynamics.reduction_db) - expected_db).abs() < 0.05);
        assert_eq!(dynamics.gate_db, 0.0);
        deepest = deepest.max(dynamics.reduction_db);
    }
    // The same figure labels the analysis window, tagged with the effect.
    let (effect, db) = last
        .reduction
        .expect("a window with one effect's reduction");
    assert_eq!(effect, limiter);
    assert!((f64::from(db) - expected_db).abs() < 0.05);
    assert!((f64::from(metered) - expected_db).abs() < 0.05, "{metered}");
    // The track comes out at the ceiling, and its look-ahead is its latency.
    assert!(
        close(last.peak[0], f64::from(10.0_f32.powf(-0.3))) || {
            (last.peak[0].unwrap() - 10.0_f64.powf(-0.3)).abs() < 1.0e-3
        }
    );
    assert_eq!(last.pdc, 240);
    // A track without the limiter has no reduction and no latency of its own.
    let quiet = look(&controller, 1, on_other);
    assert_eq!(quiet.reduction, None);
    assert!(quiet.observations.is_empty());
    assert_eq!(quiet.pdc, 0);

    // A longer look-ahead is a new latency: the label changes with a break,
    // never inside a window.
    let resets = last.resets;
    rig.effect_mut(limiter).params = EffectParams::Limiter(LimiterParams {
        ceiling_db: -6.0,
        input_gain_db: 6.0,
        lookahead_ms: 10.0,
        ..LimiterParams::default()
    });
    controller.set_project(&rig.project, &rig.pool);
    let mut later = settle(&mut processor, &controller, 0, ticket, last.end_frame + 512);
    for _ in 0..80 {
        if later.pdc == 480 {
            break;
        }
        later = settle(&mut processor, &controller, 0, ticket, later.end_frame);
    }
    assert_eq!(later.pdc, 480, "{later:?}");
    assert!(later.resets > resets);

    // Switched off, the limiter stops reporting: no stale figure is resent.
    rig.effect_mut(limiter).enabled = false;
    controller.set_project(&rig.project, &rig.pool);
    for _ in 0..20 {
        run(&mut processor, 512, 256);
        look(&controller, 0, ticket);
    }
    run(&mut processor, 2_048, 256);
    assert!(look(&controller, 0, ticket).observations.is_empty());

    // Removed, the binding says so.
    rig.remove_effect(limiter);
    controller.set_project(&rig.project, &rig.pool);
    run(&mut processor, 512, 256);
    assert!(!look(&controller, 0, ticket).effect_present);
    drop(processor);
}

#[test]
fn compression_and_the_gate_are_heard_and_reported_through_the_engine() {
    let _turn = turn();
    let mut rig = Rig::new();
    let track = rig.track();
    // -6 dB square wave.
    let tone = AudioBuffer::from_interleaved(
        RATE,
        1,
        (0..RATE as usize * 8)
            .map(|n| if (n / 48) % 2 == 0 { 0.5 } else { -0.5 })
            .collect(),
    );
    let channel = rig.channel_on(tone, track);
    let limiter = rig.effect(
        track,
        EffectParams::Limiter(LimiterParams {
            ceiling_db: 0.0,
            mode: LimiterMode::Compress,
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 0.0,
            attack_ms: 0.05,
            release_ms: 1000.0,
            ..LimiterParams::default()
        }),
    );
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let ticket = controller
        .capture_select(
            0,
            CaptureRequest {
                project: 1,
                source: Source::TrackPostFader(track),
                effect: Some(limiter),
            },
        )
        .unwrap();
    // 12 dB over the threshold at 4:1 leaves 3 dB over it: -15 dB out.
    let input_db = 20.0 * 0.5_f64.log10();
    let expected = 10.0_f64.powf((-18.0 + (input_db + 18.0) / 4.0) / 20.0);
    let mut seen = settle(&mut processor, &controller, 0, ticket, 0);
    for _ in 0..20 {
        seen = settle(&mut processor, &controller, 0, ticket, seen.end_frame);
    }
    assert!(
        (seen.peak[0].unwrap() - expected).abs() < 2.0e-3,
        "{seen:?}"
    );
    let (_, db) = seen.reduction.unwrap();
    assert!(
        (f64::from(db) - (input_db + 18.0) * 0.75).abs() < 0.05,
        "{db}"
    );

    // A gate above the signal shuts it: the track goes silent, and the
    // limiter says its gate is what did it.
    rig.effect_mut(limiter).params = EffectParams::Limiter(LimiterParams {
        ceiling_db: 0.0,
        gate_threshold_db: -3.0,
        gate_release_ms: 5.0,
        ..LimiterParams::default()
    });
    controller.set_project(&rig.project, &rig.pool);
    let mut shut = None;
    for _ in 0..200 {
        run(&mut processor, 512, 256);
        let look = look(&controller, 0, ticket);
        let gate = look
            .observations
            .iter()
            .rev()
            .find_map(|packet| match packet.observation {
                EffectObservation::Dynamics(dynamics) => Some(dynamics.gate_db),
                _ => None,
            });
        if look.state == CaptureState::Ready
            && gate == Some(windfall_dsp::GATE_FLOOR_DB)
            && look.peak[0] == Some(0.0)
        {
            shut = Some(look);
            break;
        }
    }
    let shut = shut.expect("the gate shut the track");
    assert_eq!(shut.rms, [Some(0.0), Some(0.0)]);
    assert!(matches!(
        controller.capture_poll(0, ticket, |view| view
            .snapshot
            .map(|s| s.channels[0].levels.map(|l| l.peak_dbfs))),
        Some(Some(Dbfs::Silence))
    ));
    drop(processor);
}

#[test]
fn a_tuner_on_the_captured_track_reads_the_note_against_its_reference() {
    let _turn = turn();
    let mut rig = Rig::new();
    let track = rig.track();
    let channel = rig.channel_on(sine(RATE, 440.0, 6.0), track);
    let tuner = rig.effect(
        track,
        EffectParams::Tuner(TunerParams {
            reference_hz: 432.0,
        }),
    );
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let ticket = controller
        .capture_select(
            0,
            CaptureRequest {
                project: 1,
                source: Source::TrackPostFader(track),
                effect: Some(tuner),
            },
        )
        .unwrap();
    let read = |processor: &mut Processor| {
        let mut newest = None;
        for _ in 0..60 {
            run(processor, 512, 256);
            for packet in look(&controller, 0, ticket).observations {
                assert_eq!(packet.effect, tuner);
                if let EffectObservation::Tuner { readout, .. } = packet.observation {
                    newest = Some(readout);
                }
            }
        }
        newest.expect("the tuner reported")
    };
    let sharp = read(&mut processor);
    // 440 Hz against A4 = 432 Hz: 1200 * log2(440 / 432) cents sharp of A4.
    let cents = 1200.0 * (440.0_f64 / 432.0).log2();
    assert_eq!(sharp.midi_note, Some(69));
    assert!(
        (f64::from(sharp.frequency_hz) - 440.0).abs() < 0.3,
        "{sharp:?}"
    );
    assert!(
        (f64::from(sharp.cents_error) - cents).abs() < 1.5,
        "{sharp:?}"
    );
    assert!(sharp.confidence > 0.9);

    // Recalibrated to 440 the same sound is in tune.
    rig.effect_mut(tuner).params = EffectParams::Tuner(TunerParams {
        reference_hz: 440.0,
    });
    controller.set_project(&rig.project, &rig.pool);
    let tuned = read(&mut processor);
    assert_eq!(tuned.midi_note, Some(69));
    assert!(tuned.cents_error.abs() < 1.5, "{tuned:?}");
    drop(processor);
}
