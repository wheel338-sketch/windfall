//! Routed effect outputs through the real mixer graph: Send Tap
//! (`fx-fruity-send`) and Transient Split (`fx-transmitter`).
//!
//! Every case renders the public [`Processor`](windfall_engine::Processor)
//! from a saved project, so what is measured is what playback and export
//! produce, not a route flag or the DSP block on its own.

#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use support::{
    Rig, counted, counting, idle_limiter, impulse, largest_step, left, level, right, run,
};
use windfall_core::AudioBuffer;
use windfall_dsp::sendtap::SendTapParams;
use windfall_dsp::{Effect, EffectKind, PolarityParams, TransientSplit, TransientSplitParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{
    AutomationTarget, Command, Document, EffectId, EffectOutputRoute, EffectParams, TrackId, file,
};

const RATE: u32 = 8_000;
const FRAMES: usize = 3_000;

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every call a thread makes while it is
/// inside [`allocator_calls`].
struct CountingAllocator;

fn count() {
    if WATCHING.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}

// SAFETY: every method forwards to the system allocator unchanged. Counting
// reads and writes thread-local cells and nothing else.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocator_calls(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    WATCHING.set(true);
    work();
    WATCHING.set(false);
    CALLS.get()
}

fn send_tap(level: f32) -> EffectParams {
    EffectParams::SendTap(SendTapParams { level })
}

fn flip() -> EffectParams {
    EffectParams::Polarity(PolarityParams {
        left: true,
        right: true,
    })
}

fn route(rig: &mut Rig, effect: EffectId, output: u8, target: TrackId) {
    rig.project.mixer.effect_outputs.push(EffectOutputRoute {
        effect,
        output,
        target,
    });
}

/// A source track with nothing but its effects' own outputs leaving it, a
/// destination that plays into the master, and a counting sample on the
/// source's first step.
fn tap_rig() -> (Rig, TrackId, TrackId) {
    let mut rig = Rig::new();
    let source = rig.track();
    let destination = rig.track();
    let channel = rig.channel_on(counting(RATE, 2_000), source);
    rig.steps(channel, &[0]);
    rig.track_mut(source).output = None;
    (rig, source, destination)
}

/// Frame `n` of the output must be `gain * counted(n)` on both sides while
/// the sample lasts, and silence after it.
fn assert_counted(audio: &[f32], gain: f32, what: &str) {
    for (frame, pair) in audio.chunks_exact(2).enumerate() {
        let expected = if frame < 2_000 {
            gain * counted(frame)
        } else {
            0.0
        };
        assert_eq!(pair, [expected, expected], "{what}: frame {frame}");
    }
}

#[test]
fn qa_send_tap_sends_the_signal_at_its_chain_position_before_the_fader() {
    let (mut rig, source, destination) = tap_rig();
    let tap = rig.effect(source, send_tap(0.5));
    let after = rig.effect(source, flip());
    route(&mut rig, tap, 0, destination);

    // Ahead of the inverter the tap carries the signal as it entered.
    let ahead = rig.play(RATE, FRAMES, 127);
    assert_counted(&ahead, 0.5, "tap before the inverter");
    assert_eq!(ahead, rig.play(RATE, FRAMES, 1));
    assert_eq!(ahead, rig.play(RATE, FRAMES, 64));

    // After it, the same tap carries the inverted signal: position counts.
    rig.track_mut(source).effects.swap(0, 1);
    assert_eq!(rig.track_mut(source).effects[1].id, tap);
    assert_counted(&rig.play(RATE, FRAMES, 127), -0.5, "tap after the inverter");
    rig.track_mut(source).effects.swap(0, 1);

    // The source's fader and pan come after the chain, so they do not touch
    // what the tap already sent.
    rig.track_mut(source).volume = 0.25;
    rig.track_mut(source).pan = 1.0;
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "pre-fader tap");
    rig.track_mut(source).volume = 1.0;
    rig.track_mut(source).pan = 0.0;

    // The destination's own fader does shape it, like anything else there.
    rig.track_mut(destination).volume = 0.5;
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.25, "destination fader");
    rig.track_mut(destination).volume = 1.0;

    // Without the route the tap sends nowhere and the track is silent.
    rig.project.mixer.effect_outputs.clear();
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));
    let _ = after;
}

#[test]
fn qa_send_tap_leaves_the_dry_chain_bit_identical() {
    let (mut rig, source, destination) = tap_rig();
    rig.track_mut(source).output = Some(TrackId::MASTER);
    rig.effect(source, flip());
    let untapped = rig.play(RATE, FRAMES, 127);
    assert_counted(&untapped, -1.0, "the chain alone");

    let tap = rig.effect(source, send_tap(1.0));
    rig.track_mut(source).effects.swap(0, 1);
    route(&mut rig, tap, 0, destination);
    // The destination is muted, so only the source's own output is heard.
    rig.track_mut(destination).muted = true;
    assert_eq!(untapped, rig.play(RATE, FRAMES, 127));

    // Heard together they sum: the dry path inverted, the tap as it entered.
    rig.track_mut(destination).muted = false;
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));
}

#[test]
fn qa_send_tap_level_switch_and_mix_scale_what_is_sent() {
    let (mut rig, source, destination) = tap_rig();
    let tap = rig.effect(source, send_tap(1.0));
    route(&mut rig, tap, 0, destination);
    assert_counted(&rig.play(RATE, FRAMES, 127), 1.0, "full level");

    rig.effect_mut(tap).params = send_tap(0.25);
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.25, "quarter level");
    rig.effect_mut(tap).params = send_tap(0.0);
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));

    rig.effect_mut(tap).params = send_tap(1.0);
    rig.effect_mut(tap).mix = 0.5;
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "half mix");
    rig.effect_mut(tap).mix = 1.0;
    rig.effect_mut(tap).enabled = false;
    assert!(
        rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0),
        "a slot that is off sends nothing"
    );
}

#[test]
fn qa_send_tap_follows_source_mute_and_solo() {
    let (mut rig, source, destination) = tap_rig();
    let other = rig.track();
    let tap = rig.effect(source, send_tap(0.5));
    route(&mut rig, tap, 0, destination);

    rig.track_mut(source).muted = true;
    assert!(
        rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0),
        "muting the source silences its tap"
    );
    rig.track_mut(source).muted = false;

    // Soloing the destination keeps the track that feeds it.
    rig.track_mut(destination).solo = true;
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "destination solo");
    rig.track_mut(destination).solo = false;

    // Soloing the source keeps the path its tap takes to the master.
    rig.track_mut(source).solo = true;
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "source solo");
    rig.track_mut(source).solo = false;

    // Soloing an unrelated track silences both.
    rig.track_mut(other).solo = true;
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));
}

/// The frames that are not silent, with their left value.
fn marks(audio: &[f32]) -> Vec<(usize, f32)> {
    left(audio)
        .into_iter()
        .enumerate()
        .filter(|(_, value)| value.abs() > 1e-6)
        .collect()
}

#[test]
fn qa_send_tap_is_sample_aligned_with_its_track_around_latent_effects() {
    const FAST: u32 = 48_000;
    // 5 ms of look-ahead is 240 frames at 48 kHz.
    for tap_first in [true, false] {
        for destination_latency in [None, Some(2.5_f32)] {
            let mut rig = Rig::new();
            let source = rig.track();
            let destination = rig.track();
            let channel = rig.channel_on(impulse(FAST), source);
            rig.steps(channel, &[0]);
            let (tap, limiter) = if tap_first {
                let tap = rig.effect(source, send_tap(0.5));
                (tap, rig.effect(source, idle_limiter(5.0)))
            } else {
                let limiter = rig.effect(source, idle_limiter(5.0));
                (rig.effect(source, send_tap(0.5)), limiter)
            };
            let _ = limiter;
            if let Some(lookahead) = destination_latency {
                rig.effect(destination, idle_limiter(lookahead));
            }
            route(&mut rig, tap, 0, destination);

            let case = format!("tap_first={tap_first} destination={destination_latency:?}");
            let heard = rig.play(FAST, 2_000, 127);
            let found = marks(&heard);
            assert_eq!(
                found.len(),
                1,
                "{case}: both copies on one frame, got {found:?}"
            );
            assert!(
                (found[0].1 - 1.5).abs() < 1e-5,
                "{case}: direct plus tap, got {found:?}"
            );
            assert_eq!(left(&heard), right(&heard), "{case}");
            assert_eq!(heard, rig.play(FAST, 2_000, 1), "{case}: one-frame blocks");
            assert_eq!(heard, rig.play(FAST, 2_000, 64), "{case}: 64-frame blocks");
        }
    }
}

#[test]
fn qa_send_tap_route_command_history_file_and_loop_refusal_reach_the_audio() {
    let (mut rig, source, destination) = tap_rig();
    let tap = rig.effect(source, send_tap(0.5));
    let mut document = Document::new(rig.project.clone());
    let routed = Command::SetEffectOutput {
        effect: tap,
        output: 0,
        target: Some(destination),
    };
    document.dispatch(routed.clone(), None).unwrap();

    // Saved and reopened, the route plays.
    let saved = file::to_json(document.project()).unwrap();
    rig.project = file::from_json(&saved).unwrap();
    assert_eq!(rig.project.mixer.effect_outputs.len(), 1);
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "reopened route");

    // One undo step removes it from the audio, redo brings it back.
    document.undo().expect("the route is one undo step");
    rig.project = document.project().clone();
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));
    document.redo().expect("redo");
    rig.project = document.project().clone();
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "redone route");

    // A route that would feed the destination back into the source is
    // refused and changes nothing.
    let before = document.project().clone();
    let looped = document.dispatch(
        Command::SetTrackOutput {
            id: destination,
            output: Some(source),
        },
        None,
    );
    assert!(
        looped.is_err(),
        "output back into the tapped track must be refused"
    );
    assert_eq!(document.project(), &before);

    // Deleting the destination retires the route in the same step, and one
    // undo restores track, route and sound together.
    document
        .dispatch(Command::RemoveMixerTrack { id: destination }, None)
        .unwrap();
    assert!(document.project().mixer.effect_outputs.is_empty());
    rig.project = document.project().clone();
    assert!(rig.play(RATE, FRAMES, 127).iter().all(|v| *v == 0.0));
    document.undo().expect("undo the delete");
    rig.project = document.project().clone();
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "restored destination");

    // Deleting the effect does the same.
    document
        .dispatch(
            Command::RemoveEffect {
                track: source,
                effect: tap,
            },
            None,
        )
        .unwrap();
    assert!(document.project().mixer.effect_outputs.is_empty());
    document.undo().expect("undo the effect delete");
    assert_eq!(document.project().mixer.effect_outputs.len(), 1);
    rig.project = document.project().clone();
    assert_counted(&rig.play(RATE, FRAMES, 127), 0.5, "restored effect");
}

#[test]
fn qa_send_tap_offline_render_equals_live_playback() {
    let (mut rig, source, destination) = tap_rig();
    rig.track_mut(source).output = Some(TrackId::MASTER);
    let tap = rig.effect(source, send_tap(0.5));
    rig.effect(source, idle_limiter(2.0));
    rig.effect(destination, flip());
    route(&mut rig, tap, 0, destination);

    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        ..RenderOptions::default()
    };
    let offline = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let offline = offline.samples();
    // Playback lags the notes by the graph's latency, which export leaves
    // off its front: 2 ms is 16 frames at 8 kHz.
    let lag = 16;
    let live = rig.play(RATE, FRAMES + lag, 127);
    assert_eq!(&live[lag * 2..], &offline[..FRAMES * 2]);
    // And the signal is the limited source minus half of it, not silence.
    assert!((offline[2 * 1_000] - 0.5 * counted(1_000)).abs() < 1e-6);
}

#[test]
fn qa_send_tap_level_automation_moves_the_send_without_steps() {
    let mut rig = Rig::new();
    let source = rig.track();
    let destination = rig.track();
    let channel = rig.channel_on(level(RATE, 0.5, 3.0), source);
    rig.steps(channel, &[0]);
    rig.track_mut(source).output = None;
    let tap = rig.effect(source, send_tap(0.0));
    route(&mut rig, tap, 0, destination);
    let lane = rig.playlist_track();
    let other = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 3_840);
    let sweep = rig.automation(
        AutomationTarget::EffectParam {
            track: source,
            effect: tap,
            param: 0,
        },
        &[(0, 0.0), (960, 1.0)],
    );
    rig.automation_clip(other, sweep, 0, 3_840);

    // One beat at 120 BPM is 4000 frames at 8 kHz.
    let heard = left(&rig.play_song(RATE, 6_000, 64));
    // Automation is read once a block, so the first block already holds
    // the curve's value 64 frames in: 64/4000 of the 0.5 signal.
    assert!(
        heard[0].abs() <= 0.5 * 64.0 / 4_000.0 + 1e-6,
        "starts closed, got {}",
        heard[0]
    );
    assert!(
        (heard[5_000] - 0.5).abs() < 1e-3,
        "ends open, got {}",
        heard[5_000]
    );
    assert!(
        heard.windows(2).all(|pair| pair[1] >= pair[0] - 1e-6),
        "a rising level never dips"
    );
    // The level travels its whole range in 5 ms at the fastest: 1/40 of the
    // 0.5 signal per frame at 8 kHz.
    assert!(
        largest_step(&heard) <= 0.5 / 40.0 + 1e-6,
        "{}",
        largest_step(&heard)
    );
    assert_eq!(heard, left(&rig.play_song(RATE, 6_000, 64)));
}

#[test]
fn qa_send_tap_route_edits_while_playing_fade_instead_of_clicking() {
    let mut rig = Rig::new();
    let source = rig.track();
    let destination = rig.track();
    let channel = rig.channel_on(level(RATE, 0.5, 3.0), source);
    rig.steps(channel, &[0]);
    rig.track_mut(source).output = None;
    let tap = rig.effect(source, send_tap(1.0));

    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let silent = run(&mut processor, 1_000, 127);
    assert!(silent.iter().all(|v| *v == 0.0), "no route yet");

    // The route appears while sound is passing: it fades in.
    route(&mut rig, tap, 0, destination);
    controller.set_project(&rig.project, &rig.pool);
    let joined = left(&run(&mut processor, 1_000, 127));
    assert!(
        (joined[999] - 0.5).abs() < 1e-6,
        "route settles at the send level"
    );
    assert!(
        largest_step(&joined) < 0.05,
        "fade in, got {}",
        largest_step(&joined)
    );

    // The level is turned down while it is sending: the send glides there.
    rig.effect_mut(tap).params = send_tap(0.25);
    controller.set_project(&rig.project, &rig.pool);
    let turned = left(&run(&mut processor, 1_000, 127));
    assert!(
        (turned[999] - 0.125).abs() < 1e-6,
        "the new level is reached"
    );
    assert!(
        largest_step(&turned) < 0.05,
        "level glide, got {}",
        largest_step(&turned)
    );

    // The tap effect is deleted: its route goes with it, as a removed send
    // does, and nothing of it is left.
    rig.remove_effect(tap);
    rig.project.mixer.effect_outputs.clear();
    controller.set_project(&rig.project, &rig.pool);
    let gone = left(&run(&mut processor, 1_000, 127));
    assert!(
        gone[1..].iter().all(|v| *v == 0.0),
        "a deleted tap is silent"
    );
}

#[test]
fn qa_routed_outputs_never_allocate_on_the_audio_thread() {
    let mut rig = Rig::new();
    let source = rig.track();
    let (first, second) = (rig.track(), rig.track());
    let channel = rig.channel_on(level(RATE, 0.5, 3.0), source);
    rig.steps(channel, &[0]);
    let tap = rig.effect(source, send_tap(1.0));
    rig.effect(source, idle_limiter(2.0));
    let split = rig.effect(source, EffectKind::TransientSplit.default_params());
    route(&mut rig, tap, 0, first);

    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut out = vec![0.0_f32; 2 * 4_000];
    let mut calls = allocator_calls(|| {
        for chunk in out.chunks_mut(2 * 127) {
            processor.process(chunk);
        }
    });
    assert_eq!(calls, 0, "steady routed playback");

    // Routes are added, retargeted and removed, and the tap deleted, with
    // the callback running across each change.
    route(&mut rig, split, 0, first);
    route(&mut rig, split, 1, second);
    controller.set_project(&rig.project, &rig.pool);
    calls += allocator_calls(|| {
        for chunk in out.chunks_mut(2 * 64) {
            processor.process(chunk);
        }
    });
    rig.project.mixer.effect_outputs[0].target = second;
    rig.project.mixer.effect_outputs.remove(1);
    controller.set_project(&rig.project, &rig.pool);
    calls += allocator_calls(|| {
        for chunk in out.chunks_mut(2) {
            processor.process(chunk);
        }
    });
    rig.remove_effect(tap);
    rig.project
        .mixer
        .effect_outputs
        .retain(|route| route.effect != tap);
    controller.set_project(&rig.project, &rig.pool);
    calls += allocator_calls(|| {
        for chunk in out.chunks_mut(2 * 127) {
            processor.process(chunk);
        }
    });
    assert_eq!(calls, 0, "route edits while playing");
    assert!(out.iter().any(|v| *v != 0.0), "the rig is audible");
}

// --- Transient Split -------------------------------------------------------

const HIT: usize = 2_000;

/// Three decaying 200 Hz hits with hard onsets, 2000 frames apart.
fn hits() -> Vec<f32> {
    (0..3 * HIT)
        .map(|frame| {
            let at = (frame % HIT) as f32;
            let phase = at * 200.0 / RATE as f32 * std::f32::consts::TAU;
            0.8 * phase.cos() * (-at / 600.0).exp()
        })
        .collect()
}

/// What the processor itself separates `input` into: transient, sustain.
fn reference_split(input: &[f32], params: TransientSplitParams) -> (Vec<f32>, Vec<f32>) {
    let mut split = TransientSplit::default();
    split.prepare(RATE as f32, 256);
    split.set_params(&params);
    input
        .iter()
        .map(|sample| {
            let [transient, sustain] = split.split_frame([*sample, *sample]);
            (transient[0], sustain[0])
        })
        .unzip()
}

struct SplitRig {
    rig: Rig,
    source: TrackId,
    transient: TrackId,
    sustain: TrackId,
    split: EffectId,
}

fn split_rig() -> SplitRig {
    let mut rig = Rig::new();
    let source = rig.track();
    let transient = rig.track();
    let sustain = rig.track();
    let channel = rig.channel_on(AudioBuffer::from_interleaved(RATE, 1, hits()), source);
    rig.steps(channel, &[0]);
    let split = rig.effect(source, EffectKind::TransientSplit.default_params());
    route(&mut rig, split, 0, transient);
    route(&mut rig, split, 1, sustain);
    SplitRig {
        rig,
        source,
        transient,
        sustain,
        split,
    }
}

fn energy(signal: &[f32]) -> f64 {
    signal.iter().map(|v| f64::from(*v).powi(2)).sum()
}

#[test]
fn qa_transient_split_sends_each_part_to_its_own_track() {
    let SplitRig {
        mut rig,
        transient,
        sustain,
        ..
    } = split_rig();
    let input = hits();
    let (attack, body) = reference_split(&input, TransientSplitParams::default());
    let frames = input.len();

    // Only the transient track is heard: exactly the transient part.
    rig.track_mut(sustain).muted = true;
    let heard = rig.play(RATE, frames, 127);
    assert_eq!(
        left(&heard),
        attack,
        "the transient track carries the transient part"
    );
    assert_eq!(left(&heard), right(&heard));
    assert_eq!(heard, rig.play(RATE, frames, 1));
    assert_eq!(heard, rig.play(RATE, frames, 64));

    // Only the sustain track: exactly the sustained part.
    rig.track_mut(sustain).muted = false;
    rig.track_mut(transient).muted = true;
    let heard = rig.play(RATE, frames, 127);
    assert_eq!(
        left(&heard),
        body,
        "the sustain track carries the sustained part"
    );
    assert_eq!(heard, rig.play(RATE, frames, 1));

    // The separation is a real one. The transient part holds the onsets
    // and little else; the sustained part holds the ring and not the onset.
    let onset = |signal: &[f32]| {
        (0..3)
            .map(|hit| energy(&signal[hit * HIT..][..40]))
            .sum::<f64>()
    };
    let ring = |signal: &[f32]| {
        (0..3)
            .map(|hit| energy(&signal[hit * HIT + 400..][..1_200]))
            .sum::<f64>()
    };
    let shares = [
        onset(&attack) / onset(&input),
        ring(&attack) / ring(&input),
        ring(&body) / ring(&input),
        onset(&body) / onset(&input),
    ];
    // Measured at the default settings: about 0.53, 0.0, 1.0 and 0.10. The
    // parts are amplitudes that add up to the input, so their energies in
    // a window where both are present do not.
    assert!(
        shares[0] > 0.4,
        "onsets go to the transient part: {shares:?}"
    );
    assert!(shares[1] < 0.01, "the ring does not: {shares:?}");
    assert!(
        shares[2] > 0.99,
        "the ring goes to the sustained part: {shares:?}"
    );
    assert!(shares[3] < 0.2, "the onset does not: {shares:?}");
}

#[test]
fn qa_transient_split_routed_parts_leave_the_chain_and_sum_back_to_the_input() {
    let SplitRig {
        mut rig,
        source,
        transient,
        sustain,
        split,
    } = split_rig();
    let input = hits();
    let frames = input.len();
    let (attack, body) = reference_split(&input, TransientSplitParams::default());

    // Both parts are routed away, so the source's own output is empty.
    rig.track_mut(transient).muted = true;
    rig.track_mut(sustain).muted = true;
    assert!(
        rig.play(RATE, frames, 127).iter().all(|v| *v == 0.0),
        "nothing is left in the chain once both parts are routed"
    );

    // All three heard together are the input again, nothing doubled.
    rig.track_mut(transient).muted = false;
    rig.track_mut(sustain).muted = false;
    let together = left(&rig.play(RATE, frames, 127));
    for (frame, (heard, dry)) in together.iter().zip(&input).enumerate() {
        assert!(
            (heard - dry).abs() < 1e-6,
            "frame {frame}: {heard} against {dry}"
        );
    }

    // With only the transient routed, the chain keeps the sustained part.
    rig.project
        .mixer
        .effect_outputs
        .retain(|route| route.output == 0);
    rig.track_mut(transient).muted = true;
    assert_eq!(left(&rig.play(RATE, frames, 127)), body);
    // And the other way round.
    rig.project.mixer.effect_outputs[0].output = 1;
    rig.project.mixer.effect_outputs[0].target = sustain;
    rig.track_mut(sustain).muted = true;
    assert_eq!(left(&rig.play(RATE, frames, 127)), attack);

    // Unrouted, the effect is the plain recombining processor.
    rig.project.mixer.effect_outputs.clear();
    let whole = left(&rig.play(RATE, frames, 127));
    for (frame, (heard, dry)) in whole.iter().zip(&input).enumerate() {
        assert!((heard - dry).abs() < 1e-6, "frame {frame}");
    }
    let _ = (source, split);
}

#[test]
fn qa_transient_split_parts_can_be_processed_separately() {
    let SplitRig {
        mut rig,
        transient,
        sustain,
        ..
    } = split_rig();
    let input = hits();
    let frames = input.len();
    let (attack, body) = reference_split(&input, TransientSplitParams::default());

    // Invert only the transients and halve only the sustain: the mix is
    // what those two operations give on the separate parts, which no
    // processing of the whole signal could.
    rig.effect(transient, flip());
    rig.track_mut(sustain).volume = 0.5;
    let heard = left(&rig.play(RATE, frames, 127));
    for (frame, value) in heard.iter().enumerate() {
        let expected = 0.5 * body[frame] - attack[frame];
        assert!(
            (value - expected).abs() < 1e-6,
            "frame {frame}: {value} against {expected}"
        );
    }
    assert!(
        heard
            .iter()
            .zip(&input)
            .any(|(heard, dry)| (heard - dry).abs() > 0.1),
        "the separate processing is audible"
    );
}

#[test]
fn qa_transient_split_routes_follow_commands_history_file_and_offline_render() {
    let SplitRig {
        mut rig,
        transient,
        sustain,
        split,
        ..
    } = split_rig();
    rig.project.mixer.effect_outputs.clear();
    rig.track_mut(sustain).muted = true;
    let input = hits();
    let frames = input.len();
    let (attack, _) = reference_split(&input, TransientSplitParams::default());

    let mut document = Document::new(rig.project.clone());
    for (output, target) in [(0, transient), (1, sustain)] {
        document
            .dispatch(
                Command::SetEffectOutput {
                    effect: split,
                    output,
                    target: Some(target),
                },
                None,
            )
            .unwrap();
    }
    // A third output does not exist.
    assert!(
        document
            .dispatch(
                Command::SetEffectOutput {
                    effect: split,
                    output: 2,
                    target: Some(transient),
                },
                None,
            )
            .is_err()
    );

    rig.project = file::from_json(&file::to_json(document.project()).unwrap()).unwrap();
    assert_eq!(
        left(&rig.play(RATE, frames, 127)),
        attack,
        "reopened routes"
    );

    // Export is the same audio as playback.
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        ..RenderOptions::default()
    };
    let offline = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert_eq!(
        left(&offline.samples()[..frames * 2]),
        attack,
        "offline render"
    );

    // Undo both routes: the plain effect again, the whole signal on the
    // source track.
    document.undo().expect("undo sustain route");
    document.undo().expect("undo transient route");
    rig.project = document.project().clone();
    let whole = left(&rig.play(RATE, frames, 127));
    for (frame, (heard, dry)) in whole.iter().zip(&input).enumerate() {
        assert!((heard - dry).abs() < 1e-6, "frame {frame}");
    }
    document.redo().expect("redo transient route");
    document.redo().expect("redo sustain route");
    rig.project = document.project().clone();
    assert_eq!(left(&rig.play(RATE, frames, 127)), attack, "redone routes");
}
