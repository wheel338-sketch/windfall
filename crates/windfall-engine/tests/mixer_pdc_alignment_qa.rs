//! Measured plugin delay compensation (`win-mixer-pdc`).
//!
//! Each case plays impulses through the public
//! [`Processor`](windfall_engine::Processor) and finds the frame they land
//! on. Automatic compensation must put every path on one frame whatever the
//! graph: parallel tracks, buses, sends, sidechains, bypassed, removed and
//! moved effects, live and in an offline render. The typed manual
//! correction must move the other paths by exactly what was typed.

#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use support::{Rig, idle_limiter, impulse, left, right, run};
use windfall_core::AudioBuffer;
use windfall_dsp::{EffectKind, StereoMatrixParams};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory};
use windfall_engine::{Processor, RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{
    ClipAudioOutput, ClipContent, Command, Document, EffectId, EffectParams, MixerTrackPatch,
    PluginBinding, PluginTarget, Send, TrackId, file,
};

const RATE: u32 = 48_000;
/// Frames of 5 ms of look-ahead at 48 kHz.
const FIVE_MS: usize = 240;
/// Frames between sixteenth-note steps at 120 BPM and 48 kHz.
const STEP: usize = 6_000;

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

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

/// The frames that are not silent, with their left value.
fn marks(audio: &[f32]) -> Vec<(usize, f32)> {
    left(audio)
        .into_iter()
        .enumerate()
        .filter(|(_, value)| value.abs() > 1e-6)
        .collect()
}

/// One impulse of `sum` on `frame` and silence everywhere else.
#[track_caller]
fn assert_one_mark(audio: &[f32], frame: usize, sum: f32, what: &str) {
    let found = marks(audio);
    assert_eq!(found.len(), 1, "{what}: one frame expected, got {found:?}");
    assert_eq!(found[0].0, frame, "{what}: wrong frame, got {found:?}");
    assert!(
        (found[0].1 - sum).abs() < 1e-5,
        "{what}: wrong sum, got {found:?}"
    );
    assert_eq!(left(audio), right(audio), "{what}: sides differ");
}

/// A track with an impulse on the first step.
fn impulse_track(rig: &mut Rig) -> TrackId {
    let track = rig.track();
    let channel = rig.channel_on(impulse(RATE), track);
    rig.steps(channel, &[0]);
    track
}

/// Plays live at three block sizes, which must agree, and returns the audio.
fn play_checked(rig: &Rig, frames: usize) -> Vec<f32> {
    let heard = rig.play(RATE, frames, 127);
    assert_eq!(heard, rig.play(RATE, frames, 1), "one-frame blocks");
    assert_eq!(heard, rig.play(RATE, frames, 256), "256-frame blocks");
    heard
}

fn offline(rig: &Rig) -> Vec<f32> {
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        ..RenderOptions::default()
    };
    render(&rig.project, &rig.pool, &options, &mut |_| true)
        .samples()
        .to_vec()
}

/// Live playback puts every path on `latency`; export leaves the latency
/// off its front and puts them on frame 0.
#[track_caller]
fn assert_aligned(rig: &Rig, latency: usize, sum: f32, what: &str) {
    assert_one_mark(&play_checked(rig, 2_000), latency, sum, what);
    let rendered = offline(rig);
    assert_one_mark(&rendered[..2 * 2_000], 0, sum, &format!("{what}, exported"));
}

#[test]
fn qa_parallel_tracks_buses_and_sends_of_different_latency_land_on_one_frame() {
    // Three parallel tracks: 5 ms, none, 2.5 ms.
    let mut rig = Rig::new();
    let slow = impulse_track(&mut rig);
    let _plain = impulse_track(&mut rig);
    let middle = impulse_track(&mut rig);
    rig.effect(slow, idle_limiter(5.0));
    rig.effect(middle, idle_limiter(2.5));
    assert_aligned(&rig, FIVE_MS, 3.0, "parallel tracks");

    // A bus with latency of its own, fed by a latent track; a plain track
    // beside it. 5 ms on the track, 2 ms on the bus.
    let mut rig = Rig::new();
    let fed = impulse_track(&mut rig);
    let bus = rig.track();
    let _beside = impulse_track(&mut rig);
    rig.effect(fed, idle_limiter(5.0));
    rig.effect(bus, idle_limiter(2.0));
    rig.track_mut(fed).output = Some(bus);
    assert_aligned(&rig, FIVE_MS + 96, 2.0, "track into a latent bus");

    // A bus fed by two tracks of different latency, with an instrument-free
    // impulse straight on the bus as well: three arrivals, one frame.
    let quick = impulse_track(&mut rig);
    rig.track_mut(quick).output = Some(bus);
    let direct = rig.channel_on(impulse(RATE), bus);
    rig.steps(direct, &[0]);
    // A quarter each, so the three together stay under the bus limiter's
    // ceiling and it has nothing to do but delay them.
    rig.track_mut(fed).volume = 0.25;
    rig.track_mut(quick).volume = 0.25;
    rig.channel_mut(direct).volume = 0.25;
    assert_aligned(
        &rig,
        FIVE_MS + 96,
        1.75,
        "two feeders and a direct voice on the bus",
    );

    // A send from a latent track into a return that has its own latency,
    // while the same track also plays to the master.
    let mut rig = Rig::new();
    let source = impulse_track(&mut rig);
    let back = rig.track();
    rig.effect(source, idle_limiter(5.0));
    rig.effect(back, idle_limiter(2.5));
    rig.track_mut(source).sends.push(Send {
        target: back,
        gain: 0.5,
    });
    assert_aligned(&rig, FIVE_MS + 120, 1.5, "send into a latent return");

    // The return also takes a send from a track with no latency at all.
    let dry = impulse_track(&mut rig);
    rig.track_mut(dry).sends.push(Send {
        target: back,
        gain: 0.25,
    });
    assert_aligned(&rig, FIVE_MS + 120, 2.75, "two sends of different latency");
}

#[test]
fn qa_a_bypassed_effect_keeps_its_place_in_time() {
    let mut rig = Rig::new();
    let slow = impulse_track(&mut rig);
    let _plain = impulse_track(&mut rig);
    let limiter = rig.effect(slow, idle_limiter(5.0));
    assert_aligned(&rig, FIVE_MS, 2.0, "limiter on");

    // Switched off, the slot still delays by the effect's latency, so
    // nothing else has to move when the lamp is clicked.
    rig.effect_mut(limiter).enabled = false;
    assert_aligned(&rig, FIVE_MS, 2.0, "limiter bypassed");

    // Half mixed, the dry half is delayed to match the wet half.
    rig.effect_mut(limiter).enabled = true;
    rig.effect_mut(limiter).mix = 0.5;
    assert_aligned(&rig, FIVE_MS, 2.0, "limiter half mixed");
}

/// Four impulses a bar on two tracks, the first of which has the effects.
fn pulse_rig() -> (Rig, TrackId, TrackId) {
    let mut rig = Rig::new();
    let (first, second) = (rig.track(), rig.track());
    for track in [first, second] {
        let channel = rig.channel_on(impulse(RATE), track);
        rig.steps(
            channel,
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        );
    }
    (rig, first, second)
}

/// Runs to the next multiple of `STEP` plus `settle` steps, then checks
/// that every impulse of the following three steps is a single frame of
/// `sum`, `latency` frames after its step.
#[track_caller]
fn assert_pulses_aligned(
    processor: &mut Processor,
    played: &mut usize,
    latency: usize,
    sum: f32,
    what: &str,
) {
    // Let the change of plan settle: its fades and the crossfade of the
    // compensation delays take a few milliseconds.
    let settle = 2 * STEP - (*played % STEP);
    run(processor, settle, 127);
    *played += settle;
    let audio = run(processor, 3 * STEP, 127);
    *played += 3 * STEP;
    let found = marks(&audio);
    let expected: Vec<usize> = (0..3).map(|step| step * STEP + latency).collect();
    let frames: Vec<usize> = found.iter().map(|mark| mark.0).collect();
    assert_eq!(frames, expected, "{what}: impulse frames, got {found:?}");
    for mark in &found {
        assert!((mark.1 - sum).abs() < 1e-5, "{what}: sum, got {found:?}");
    }
}

#[test]
fn qa_adding_removing_moving_and_retiming_effects_while_playing_stays_aligned() {
    let (mut rig, first, second) = pulse_rig();
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut played = 0;
    assert_pulses_aligned(&mut processor, &mut played, 0, 2.0, "no latency");

    // Add a latent effect while playing.
    let limiter = rig.effect(first, idle_limiter(5.0));
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, FIVE_MS, 2.0, "limiter added");

    // Shorten its look-ahead.
    rig.effect_mut(limiter).params = idle_limiter(2.5);
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 120, 2.0, "look-ahead halved");

    // Bypass it: nothing moves.
    rig.effect_mut(limiter).enabled = false;
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 120, 2.0, "bypassed");
    rig.effect_mut(limiter).enabled = true;
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 120, 2.0, "switched back on");

    // Move it to the other track through the real command.
    let mut document = Document::new(rig.project.clone());
    document
        .dispatch(
            Command::MoveEffect {
                track: first,
                effect: limiter,
                to_track: Some(second),
                index: 0,
            },
            None,
        )
        .unwrap();
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(
        &mut processor,
        &mut played,
        120,
        2.0,
        "moved to the other track",
    );

    // A second latent effect behind it, then reorder the two.
    let longer = rig.effect(second, idle_limiter(5.0));
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(
        &mut processor,
        &mut played,
        360,
        2.0,
        "second limiter added",
    );
    rig.track_mut(second).effects.swap(0, 1);
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 360, 2.0, "chain reordered");

    // Remove both: back to no latency at all.
    rig.remove_effect(limiter);
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, FIVE_MS, 2.0, "one removed");
    rig.remove_effect(longer);
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 0, 2.0, "all removed");

    // Undo of the move puts the effect, and the alignment, back.
    document.undo().expect("undo the move");
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 120, 2.0, "move undone");
}

#[test]
fn qa_compensation_changes_while_playing_never_allocate() {
    let (mut rig, first, second) = pulse_rig();
    rig.track_mut(first).sends.push(Send {
        target: second,
        gain: 0.5,
    });
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut out = vec![0.0_f32; 2 * 3 * STEP];
    let mut calls = 0;
    let mut spin = |processor: &mut Processor, block: usize| {
        allocator_calls(|| {
            for chunk in out.chunks_mut(2 * block) {
                processor.process(chunk);
            }
        })
    };
    calls += spin(&mut processor, 127);
    let limiter = rig.effect(first, idle_limiter(5.0));
    controller.set_project(&rig.project, &rig.pool);
    calls += spin(&mut processor, 64);
    rig.track_mut(first).latency_offset_ms = 3.0;
    controller.set_project(&rig.project, &rig.pool);
    calls += spin(&mut processor, 1);
    rig.track_mut(first).latency_offset_ms = -2.0;
    rig.effect_mut(limiter).enabled = false;
    controller.set_project(&rig.project, &rig.pool);
    calls += spin(&mut processor, 256);
    rig.remove_effect(limiter);
    rig.track_mut(first).latency_offset_ms = 0.0;
    controller.set_project(&rig.project, &rig.pool);
    calls += spin(&mut processor, 127);
    assert_eq!(
        calls, 0,
        "compensation changes must not touch the allocator"
    );
}

// A hosted effect that reports latency and delays by it, to compensate a
// native plugin's reported delay the way a built-in's is. It also exposes
// the key it is given, for the sidechain case.
#[derive(Debug, Default)]
struct LatentFactory;

struct Latent {
    line: Vec<[f32; 2]>,
    at: usize,
    difference: bool,
}

impl HostedEffect for Latent {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        for (index, (left, right)) in left.iter_mut().zip(right).enumerate() {
            if self.difference {
                let key = key.map_or([0.0; 2], |key| key[index]);
                *left -= key[0];
                *right -= key[1];
            }
            if !self.line.is_empty() {
                let delayed = std::mem::replace(&mut self.line[self.at], [*left, *right]);
                self.at = (self.at + 1) % self.line.len();
                [*left, *right] = delayed;
            }
        }
    }
    fn set_param(&mut self, _id: u32, _value: f32) {}
    fn set_tempo(&mut self, _bpm: f32) {}
    fn latency(&self) -> usize {
        self.line.len()
    }
    fn tail(&self) -> usize {
        self.line.len()
    }
}

impl PluginFactory for LatentFactory {
    fn effect(
        &self,
        binding: &PluginBinding,
        _rate: u32,
        _block: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        let frames: usize = binding
            .id
            .trim_start_matches("difference-")
            .parse()
            .unwrap_or(0);
        Ok(Box::new(Latent {
            line: vec![[0.0; 2]; frames],
            at: 0,
            difference: binding.id.starts_with("difference-"),
        }))
    }
    fn instrument(
        &self,
        _binding: &PluginBinding,
        _rate: u32,
        _block: usize,
    ) -> Result<Box<dyn HostedInstrument>, String> {
        Err("effect fixture only".into())
    }
}

fn hosted(rig: &mut Rig, track: TrackId, id: &str) -> EffectId {
    let effect = rig.effect(track, EffectKind::Balance.default_params());
    rig.project.plugins.push(PluginBinding {
        target: PluginTarget::Effect { effect },
        format: "clap".into(),
        path: "qa-latent-fixture.clap".into(),
        id: id.into(),
        name: id.into(),
        state: Vec::new(),
        parameters: Vec::new(),
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
    });
    effect
}

#[test]
fn qa_hosted_plugin_latency_is_compensated_like_a_built_in() {
    let mut rig = Rig::new();
    let slow = impulse_track(&mut rig);
    let _plain = impulse_track(&mut rig);
    hosted(&mut rig, slow, "333");
    rig.effect(slow, idle_limiter(5.0));
    rig.pool.set_plugin_factory(Arc::new(LatentFactory));
    assert_one_mark(
        &play_checked(&rig, 2_000),
        333 + FIVE_MS,
        2.0,
        "hosted plus built-in latency",
    );
}

#[test]
fn qa_sidechain_keys_arrive_aligned_with_the_signal_they_control() {
    // The key track and the main track play the same pulses. The key comes
    // through 5 ms of latency, and the main track's first slot is latent
    // too, so the detector slot behind it sees its key later still. The
    // probe subtracts the key from the signal: silence means they met.
    let pulses: Vec<f32> = (0..24_000)
        .map(|i| if i % 37 == 0 { 0.5 } else { 0.0 })
        .collect();
    let build = |probe: &str| {
        let mut rig = Rig::new();
        let main = rig.track();
        let key = rig.track();
        rig.track_mut(key).output = None;
        for track in [main, key] {
            let channel = rig.channel_on(
                AudioBuffer::from_interleaved(RATE, 1, pulses.clone()),
                track,
            );
            rig.steps(channel, &[0]);
        }
        rig.effect(key, idle_limiter(5.0));
        rig.track_mut(key).sidechains.push(Send {
            target: main,
            gain: 1.0,
        });
        rig.effect(main, idle_limiter(2.5));
        rig.effect(
            main,
            EffectParams::StereoMatrix(StereoMatrixParams {
                left_delay_ms: 1.0,
                right_delay_ms: 1.0,
                ..Default::default()
            }),
        );
        hosted(&mut rig, main, probe);
        rig.pool.set_plugin_factory(Arc::new(LatentFactory));
        rig
    };
    let nulled = build("difference-0").play(RATE, 6_000, 127);
    assert!(
        nulled.iter().all(|v| v.abs() < 1e-6),
        "the key did not meet the signal at the detector slot"
    );
    assert_eq!(nulled, build("difference-0").play(RATE, 6_000, 1));
    // Positive control: without the subtraction the same graph is audible.
    assert!(
        build("0")
            .play(RATE, 6_000, 127)
            .iter()
            .any(|v| v.abs() > 0.1)
    );
}

#[test]
fn qa_direct_printed_clips_stay_aligned_with_a_latent_mix() {
    let mut rig = Rig::new();
    let slow = rig.track();
    rig.effect(slow, idle_limiter(5.0));
    let lane = rig.playlist_track();
    let other = rig.playlist_track();
    // One impulse through the latent mixer track, one printed past the
    // mixer, both on the first tick of the song.
    rig.audio_clip(lane, clip_impulse(0), slow, 0, 960);
    let printed = rig.audio_clip(other, clip_impulse(0), TrackId::MASTER, 0, 960);
    if let ClipContent::Audio { output, .. } = &mut rig.clip_mut(printed).content {
        *output = ClipAudioOutput::Direct;
    }
    let heard = rig.play_song(RATE, 2_000, 127);
    assert_one_mark(
        &heard,
        FIVE_MS + CLIP_LEAD,
        2.0,
        "printed clip beside a latent track",
    );
    assert_eq!(heard, rig.play_song(RATE, 2_000, 1));

    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Song,
        ..RenderOptions::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert_one_mark(
        &rendered.samples()[..2 * 2_000],
        CLIP_LEAD,
        2.0,
        "printed clip, exported",
    );
}

// --- Manual correction -----------------------------------------------------

/// Frames before the impulse in a [`clip_impulse`] source, and after it:
/// longer than the 3 ms ramp the engine puts on both ends of an audio clip,
/// so the impulse is heard at full level.
const CLIP_LEAD: usize = 200;

/// An impulse for an audio clip, `late` frames after [`CLIP_LEAD`].
fn clip_impulse(late: usize) -> AudioBuffer {
    let mut data = vec![0.0; CLIP_LEAD + late + 1 + CLIP_LEAD];
    data[CLIP_LEAD + late] = 1.0;
    AudioBuffer::from_interleaved(RATE, 1, data)
}

/// An impulse that comes `late` frames into its sample, as it would out of
/// a processor that delays without saying so.
fn late_impulse(late: usize) -> AudioBuffer {
    let mut data = vec![0.0; late + 1];
    data[late] = 1.0;
    AudioBuffer::from_interleaved(RATE, 1, data)
}

fn ms(frames: f64) -> f64 {
    frames * 1_000.0 / f64::from(RATE)
}

#[test]
fn qa_a_positive_correction_delays_everything_else_by_the_typed_amount() {
    // The first track's sound is 100 frames late and nothing reports it.
    let mut rig = Rig::new();
    let late = rig.track();
    let channel = rig.channel_on(late_impulse(100), late);
    rig.steps(channel, &[0]);
    let _plain = impulse_track(&mut rig);
    let found = marks(&rig.play(RATE, 2_000, 127));
    assert_eq!(
        found.iter().map(|mark| mark.0).collect::<Vec<_>>(),
        [0, 100],
        "uncorrected"
    );

    // Typing the 100 frames in as a correction lines the other track up.
    rig.track_mut(late).latency_offset_ms = ms(100.0);
    assert_aligned(&rig, 100, 2.0, "corrected by 100 frames");

    // The correction does not delay the track itself: its own impulse is
    // where it was.
    let alone = {
        let mut rig = Rig::new();
        let late = rig.track();
        let channel = rig.channel_on(late_impulse(100), late);
        rig.steps(channel, &[0]);
        rig.track_mut(late).latency_offset_ms = ms(100.0);
        rig.play(RATE, 2_000, 127)
    };
    assert_one_mark(&alone, 100, 1.0, "the corrected track alone");

    // It adds to latency the track's effects report.
    rig.effect(late, idle_limiter(5.0));
    assert_aligned(
        &rig,
        100 + FIVE_MS,
        2.0,
        "correction on top of reported latency",
    );

    // Too little or too much correction is audible as exactly that error.
    rig.track_mut(late).latency_offset_ms = ms(60.0);
    let found = marks(&rig.play(RATE, 2_000, 127));
    assert_eq!(
        found.iter().map(|mark| mark.0).collect::<Vec<_>>(),
        [60 + FIVE_MS, 100 + FIVE_MS],
        "undercorrected by 40 frames"
    );
}

#[test]
fn qa_a_negative_correction_takes_back_overstated_latency_and_stops_at_zero() {
    // The limiter reports 240 frames. Its track's sample is 240 frames
    // early in effect once -5 ms says the report is overstated.
    let mut rig = Rig::new();
    let slow = impulse_track(&mut rig);
    let _plain = impulse_track(&mut rig);
    rig.effect(slow, idle_limiter(5.0));
    assert_one_mark(&play_checked(&rig, 2_000), FIVE_MS, 2.0, "uncorrected");

    let frames = |rig: &Rig| {
        marks(&rig.play(RATE, 2_000, 127))
            .iter()
            .map(|mark| mark.0)
            .collect::<Vec<_>>()
    };
    rig.track_mut(slow).latency_offset_ms = -ms(120.0);
    assert_eq!(frames(&rig), [120, FIVE_MS], "half the report taken back");
    rig.track_mut(slow).latency_offset_ms = -ms(240.0);
    assert_eq!(frames(&rig), [0, FIVE_MS], "the whole report taken back");
    // More than the report cannot make latency negative.
    rig.track_mut(slow).latency_offset_ms = -1_000.0;
    assert_eq!(frames(&rig), [0, FIVE_MS], "clamped at zero");
    assert_eq!(rig.play(RATE, 2_000, 127), rig.play(RATE, 2_000, 1));
}

#[test]
fn qa_manual_correction_reaches_buses_sends_sidechains_and_printed_clips() {
    // Output into a bus.
    let mut rig = Rig::new();
    let late = rig.track();
    let channel = rig.channel_on(late_impulse(100), late);
    rig.steps(channel, &[0]);
    let bus = rig.track();
    let beside = impulse_track(&mut rig);
    rig.track_mut(late).output = Some(bus);
    rig.track_mut(beside).output = Some(bus);
    rig.track_mut(late).latency_offset_ms = ms(100.0);
    assert_aligned(&rig, 100, 2.0, "corrected track into a bus");

    // A send from the corrected track into a return another track also
    // sends to, with the corrected track's own output still on the master.
    let mut rig = Rig::new();
    let late = rig.track();
    let channel = rig.channel_on(late_impulse(100), late);
    rig.steps(channel, &[0]);
    let back = rig.track();
    let dry = impulse_track(&mut rig);
    rig.track_mut(late).latency_offset_ms = ms(100.0);
    rig.track_mut(late).sends.push(Send {
        target: back,
        gain: 0.5,
    });
    rig.track_mut(dry).sends.push(Send {
        target: back,
        gain: 0.25,
    });
    assert_aligned(&rig, 100, 2.75, "corrected send into a shared return");

    // A corrected key: the detector slot must see the late key on the
    // frame the main signal reaches it. Pulses 100 frames late on the key
    // track null against on-time pulses on the main track.
    let pulses = |shift: usize| {
        let data: Vec<f32> = (0..24_000)
            .map(|i| {
                if i >= shift && (i - shift).is_multiple_of(37) {
                    0.5
                } else {
                    0.0
                }
            })
            .collect();
        AudioBuffer::from_interleaved(RATE, 1, data)
    };
    let build = |correction: f64| {
        let mut rig = Rig::new();
        let main = rig.track();
        let key = rig.track();
        rig.track_mut(key).output = None;
        let channel = rig.channel_on(pulses(0), main);
        rig.steps(channel, &[0]);
        let channel = rig.channel_on(pulses(100), key);
        rig.steps(channel, &[0]);
        rig.track_mut(key).latency_offset_ms = correction;
        rig.track_mut(key).sidechains.push(Send {
            target: main,
            gain: 1.0,
        });
        hosted(&mut rig, main, "difference-0");
        rig.pool.set_plugin_factory(Arc::new(LatentFactory));
        rig
    };
    let nulled = build(ms(100.0)).play(RATE, 6_000, 127);
    assert!(
        nulled[2 * 200..].iter().all(|v| v.abs() < 1e-6),
        "a corrected key must meet the signal"
    );
    assert!(
        build(0.0).play(RATE, 6_000, 127)[2 * 200..]
            .iter()
            .any(|v| v.abs() > 0.1),
        "uncorrected, the late key misses it"
    );

    // A printed clip waits for the corrected track like the rest of the mix.
    let mut rig = Rig::new();
    let late = rig.track();
    rig.track_mut(late).latency_offset_ms = ms(100.0);
    let lane = rig.playlist_track();
    let other = rig.playlist_track();
    rig.audio_clip(lane, clip_impulse(100), late, 0, 960);
    let printed = rig.audio_clip(other, clip_impulse(0), TrackId::MASTER, 0, 960);
    if let ClipContent::Audio { output, .. } = &mut rig.clip_mut(printed).content {
        *output = ClipAudioOutput::Direct;
    }
    assert_one_mark(
        &rig.play_song(RATE, 2_000, 127),
        100 + CLIP_LEAD,
        2.0,
        "printed clip and a corrected track",
    );
}

#[test]
fn qa_manual_correction_edits_while_playing_realign_and_follow_history_and_files() {
    // The first track's impulses are 100 frames late in their sample.
    let mut rig = Rig::new();
    let late = rig.track();
    let channel = rig.channel_on(late_impulse(100), late);
    rig.steps(
        channel,
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    );
    let plain = rig.track();
    let channel = rig.channel_on(impulse(RATE), plain);
    rig.steps(
        channel,
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    );

    let mut document = Document::new(rig.project.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut played = 0;

    // Typed through the real command while playing.
    document
        .dispatch(
            Command::UpdateMixerTrack {
                id: late,
                patch: MixerTrackPatch {
                    latency_offset_ms: Some(ms(100.0)),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 100, 2.0, "correction typed");

    // Undo: the two tracks fall 100 frames apart again.
    document.undo().expect("undo the correction");
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    let settle = 2 * STEP - (played % STEP);
    run(&mut processor, settle, 127);
    let apart = marks(&run(&mut processor, STEP, 127));
    assert_eq!(
        apart.iter().map(|mark| mark.0).collect::<Vec<_>>(),
        [0, 100],
        "undone"
    );
    played += settle + STEP;

    // Redo, save, reopen: the reopened project is aligned from its first frame.
    document.redo().expect("redo the correction");
    rig.project = file::from_json(&file::to_json(document.project()).unwrap()).unwrap();
    let reopened = rig.project.mixer.track(late).unwrap().latency_offset_ms;
    assert!(
        (reopened - ms(100.0)).abs() < 1e-9,
        "the correction is saved: {reopened}"
    );
    controller.set_project(&rig.project, &rig.pool);
    assert_pulses_aligned(&mut processor, &mut played, 100, 2.0, "redone and reopened");
    let fresh = marks(&rig.play(RATE, 2_000, 127));
    assert_eq!(
        fresh.len(),
        1,
        "a fresh processor starts aligned: {fresh:?}"
    );
    assert_eq!(fresh[0].0, 100);

    // Out-of-range and non-finite values are refused and change nothing.
    for bad in [1_000.5, -1_000.5, f64::NAN, f64::INFINITY] {
        let before = document.project().clone();
        let refused = document.dispatch(
            Command::UpdateMixerTrack {
                id: late,
                patch: MixerTrackPatch {
                    latency_offset_ms: Some(bad),
                    ..Default::default()
                },
            },
            None,
        );
        assert!(refused.is_err(), "{bad} must be refused");
        assert_eq!(document.project(), &before);
    }
}

#[test]
fn qa_the_largest_correction_is_one_second_of_compensation() {
    // At 8 kHz the full +1000 ms is 8000 frames, the engine's whole
    // compensation bound.
    const SLOW: u32 = 8_000;
    let mut rig = Rig::new();
    let late = rig.track();
    let mut data = vec![0.0; 8_001];
    data[8_000] = 1.0;
    let channel = rig.channel_on(AudioBuffer::from_interleaved(SLOW, 1, data), late);
    rig.steps(channel, &[0]);
    let plain = rig.track();
    let channel = rig.channel_on(impulse(SLOW), plain);
    rig.steps(channel, &[0]);
    rig.track_mut(late).latency_offset_ms = 1_000.0;
    let heard = rig.play(SLOW, 10_000, 127);
    assert_one_mark(&heard, 8_000, 2.0, "one second of correction");
    assert_eq!(heard, rig.play(SLOW, 10_000, 1));
}
