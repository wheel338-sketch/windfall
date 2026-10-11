//! Sidechain detector, delay-compensation and history transitions while key
//! audio is in flight (`win-mixer-sidechain`, gate 3 of
//! `docs/MIXER-SIDECHAINS.md`).
//!
//! The static cases in `mixer_sidechain_acceptance_qa.rs` build a graph and
//! play it. These change the graph while the key is sounding: latency ahead
//! of the detector slot, latency on the key's own path, bypass, moved and
//! removed effects, route edits and their undo and redo. After each change
//! has had time to settle the key must again meet the signal it controls on
//! the same frame, the audio in between must stay bounded, and none of it
//! may touch the allocator on the audio thread.
//!
//! The hosted detector is a deterministic implementation of the public
//! `HostedEffect` seam. It is not evidence of CLAP or VST3 bus negotiation;
//! `crates/windfall-plugin-host/tests/sidechain_buses.rs` and the desktop
//! `sidechain_bus_tests.rs` cover the native side.

#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

use support::{Rig, idle_limiter, level};
use windfall_core::AudioBuffer;
use windfall_dsp::{CompressorParams, DetectorMode, EffectKind, StereoMatrixParams};
use windfall_engine::Processor;
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory};
use windfall_project::{
    Command, Document, EffectId, EffectParams, MixerTrackPatch, PluginAuxInput, PluginBinding,
    PluginTarget, Send, TrackId,
};

const RATE: u32 = 8_000;
/// Frames a change of plan is given before it is measured: half a second,
/// far longer than any fade, warm-up or delay crossfade at these settings.
const SETTLE: usize = 4_000;
const WINDOW: usize = 2_000;
/// One pattern of 64 steps at 120 BPM: eight seconds, so no case meets the
/// loop point.
const SONG_FRAMES: usize = 64_000;

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

/// A playing processor with a buffer of its own, so that running it can be
/// watched for allocator calls.
struct Live {
    processor: Processor,
    out: Vec<f32>,
    /// Allocator calls made inside `process` so far.
    calls: usize,
}

impl Live {
    fn new(processor: Processor) -> Self {
        Self {
            processor,
            out: vec![0.0; 2 * SETTLE.max(WINDOW)],
            calls: 0,
        }
    }

    /// Processes `frames` frames, `block` at a time, and returns them.
    fn run(&mut self, frames: usize, block: usize) -> &[f32] {
        let out = &mut self.out[..2 * frames];
        let processor = &mut self.processor;
        CALLS.set(0);
        WATCHING.set(true);
        for chunk in out.chunks_mut(2 * block) {
            processor.process(chunk);
        }
        WATCHING.set(false);
        self.calls += CALLS.get();
        &self.out[..2 * frames]
    }

    /// Lets a change settle, checking that nothing runs away meanwhile, and
    /// returns the window after it.
    #[track_caller]
    fn settled(&mut self, what: &str) -> Vec<f32> {
        let transition = self.run(SETTLE, 127);
        let loudest = transition.iter().fold(0.0_f32, |peak, v| peak.max(v.abs()));
        assert!(
            transition.iter().all(|v| v.is_finite()) && loudest <= 2.0,
            "{what}: the transition must stay bounded, peak {loudest}"
        );
        self.run(WINDOW, 127).to_vec()
    }
}

// --- A hosted detector on the public provider seam --------------------------

/// `difference` subtracts the key from the signal, so silence means the two
/// met on the same frame. `copy` puts the key out instead of the signal.
#[derive(Debug, Default)]
struct ProbeFactory {
    revision: AtomicU64,
    creates: AtomicUsize,
}

struct Probe {
    selected: Option<u32>,
    difference: bool,
}

impl HostedEffect for Probe {
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.selected = input;
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        // Only auxiliary input 2 exists; any other saved choice is silent.
        let available = self.selected.is_none_or(|input| input == 2);
        for (at, (left, right)) in left.iter_mut().zip(right).enumerate() {
            let key = key.filter(|_| available).map_or([0.0; 2], |key| key[at]);
            if self.difference {
                *left -= key[0];
                *right -= key[1];
            } else {
                *left = key[0];
                *right = key[1];
            }
        }
    }
    fn set_param(&mut self, _id: u32, _value: f32) {}
    fn set_tempo(&mut self, _bpm: f32) {}
    fn latency(&self) -> usize {
        0
    }
    fn tail(&self) -> usize {
        0
    }
}

impl PluginFactory for ProbeFactory {
    fn revision(&self) -> u64 {
        self.revision.load(Relaxed)
    }
    fn effect(
        &self,
        binding: &PluginBinding,
        _rate: u32,
        _block: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        self.creates.fetch_add(1, Relaxed);
        Ok(Box::new(Probe {
            selected: None,
            difference: binding.id == "difference",
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

fn probe(rig: &mut Rig, track: TrackId, id: &str, input: Option<u32>) -> EffectId {
    let effect = rig.effect(track, EffectKind::Balance.default_params());
    rig.project.plugins.push(PluginBinding {
        target: PluginTarget::Effect { effect },
        format: "clap".into(),
        path: "qa-key-fixture.clap".into(),
        id: id.into(),
        name: id.into(),
        state: Vec::new(),
        parameters: Vec::new(),
        sidechain_input: input,
        auxiliary_inputs: vec![
            PluginAuxInput {
                index: 2,
                name: "Detector".into(),
                channels: 2,
            },
            PluginAuxInput {
                index: 7,
                name: "Saved inactive input".into(),
                channels: 1,
            },
        ],
    });
    effect
}

/// A main track and a key-only track that play the same pulses for the
/// whole eight-second pattern, with the key routed to the main track's
/// detectors.
fn pulse_rig() -> (Rig, TrackId, TrackId) {
    let mut rig = Rig::new();
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).length_steps = 64;
    let main = rig.track();
    let key = rig.track();
    rig.track_mut(key).output = None;
    let pulses: Vec<f32> = (0..SONG_FRAMES)
        .map(|i| if i % 37 == 0 { 0.5 } else { 0.0 })
        .collect();
    for track in [main, key] {
        let channel = rig.channel_on(
            AudioBuffer::from_interleaved(RATE, 1, pulses.clone()),
            track,
        );
        rig.steps(channel, &[0]);
    }
    rig.track_mut(key).sidechains.push(Send {
        target: main,
        gain: 1.0,
    });
    (rig, main, key)
}

#[track_caller]
fn assert_nulled(window: &[f32], what: &str) {
    let loudest = window.iter().fold(0.0_f32, |peak, v| peak.max(v.abs()));
    assert!(
        loudest < 1e-6,
        "{what}: the key no longer meets the signal at the detector, residue {loudest}"
    );
}

#[track_caller]
fn assert_sounding(window: &[f32], what: &str) {
    assert!(
        window.iter().any(|v| v.abs() > 0.1),
        "{what}: expected sound"
    );
}

#[test]
fn qa_key_stays_aligned_through_live_latency_edits_around_the_detector() {
    let (mut rig, main, key) = pulse_rig();
    let limiter = rig.effect(main, idle_limiter(5.0));
    let detector = probe(&mut rig, main, "difference", None);
    rig.pool
        .set_plugin_factory(Arc::new(ProbeFactory::default()));
    let (processor, controller) = rig.processor(RATE);
    let mut live = Live::new(processor);
    controller.play();
    assert_nulled(&live.settled("start"), "start");

    // The look-ahead ahead of the detector is halved.
    rig.effect_mut(limiter).params = idle_limiter(2.5);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("shorter look-ahead"), "shorter look-ahead");

    // And lengthened past where it started.
    rig.effect_mut(limiter).params = idle_limiter(10.0);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("longer look-ahead"), "longer look-ahead");

    // A second latent effect joins ahead of both.
    let joined = rig.effect(main, idle_limiter(5.0));
    rig.track_mut(main).effects.rotate_right(1);
    assert_eq!(rig.track_mut(main).effects[0].id, joined);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("latent effect added"), "latent effect added");

    // A stereo matrix with a delay joins between the limiters and the
    // detector: the matrix-aware key path.
    let matrix = rig.effect(
        main,
        EffectParams::StereoMatrix(StereoMatrixParams {
            left_delay_ms: 4.0,
            right_delay_ms: 4.0,
            ..Default::default()
        }),
    );
    rig.track_mut(main).effects.swap(2, 3);
    assert_eq!(rig.track_mut(main).effects[3].id, detector);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("matrix delay added"), "matrix delay added");

    // The matrix delay is edited while the key passes through its mirror.
    rig.effect_mut(matrix).params = EffectParams::StereoMatrix(StereoMatrixParams {
        left_delay_ms: 1.5,
        right_delay_ms: 1.5,
        ..Default::default()
    });
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("matrix delay edited"), "matrix delay edited");

    // Bypassing a latent effect moves nothing.
    rig.effect_mut(limiter).enabled = false;
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("bypassed"), "bypassed");
    rig.effect_mut(limiter).enabled = true;
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("switched back on"), "switched back on");

    // The detector moves ahead of everything: its key needs no delay now.
    rig.track_mut(main).effects.rotate_right(1);
    assert_eq!(rig.track_mut(main).effects[0].id, detector);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(
        &live.settled("detector moved first"),
        "detector moved first",
    );
    rig.track_mut(main).effects.rotate_left(1);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(&live.settled("detector moved last"), "detector moved last");

    // The effects ahead of it are removed one by one.
    for (gone, what) in [
        (matrix, "matrix removed"),
        (joined, "added limiter removed"),
        (limiter, "first limiter removed"),
    ] {
        rig.remove_effect(gone);
        controller.set_project(&rig.project, &rig.pool);
        assert_nulled(&live.settled(what), what);
    }

    // Positive control: the same graph without the route is not silent, so
    // the nulls above were the key and not a dead track.
    rig.track_mut(key).sidechains.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert_sounding(&live.settled("route removed"), "route removed");
    assert_eq!(
        live.calls, 0,
        "no transition may allocate on the audio thread"
    );
}

#[test]
fn qa_key_stays_aligned_when_latency_changes_on_the_keys_own_path() {
    let (mut rig, main, key) = pulse_rig();
    probe(&mut rig, main, "difference", None);
    rig.pool
        .set_plugin_factory(Arc::new(ProbeFactory::default()));
    let (processor, controller) = rig.processor(RATE);
    let mut live = Live::new(processor);
    controller.play();
    assert_nulled(&live.settled("start"), "start");

    // Latency appears on the key track: the main track's own sound has to
    // wait for the key.
    let limiter = rig.effect(key, idle_limiter(5.0));
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(
        &live.settled("key path gains latency"),
        "key path gains latency",
    );

    rig.effect_mut(limiter).params = idle_limiter(2.5);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(
        &live.settled("key path latency halved"),
        "key path latency halved",
    );

    // The key takes a longer way: through a bus that has latency too.
    let bus = rig.track();
    rig.track_mut(bus).output = None;
    rig.effect(bus, idle_limiter(5.0));
    rig.track_mut(key).sidechains.clear();
    rig.track_mut(key).output = Some(bus);
    rig.track_mut(bus).sidechains.push(Send {
        target: main,
        gain: 1.0,
    });
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(
        &live.settled("key rerouted through a latent bus"),
        "key rerouted through a latent bus",
    );

    // Latency on the main track ahead of the detector at the same time as
    // the key path loses its own.
    rig.effect(main, idle_limiter(5.0));
    rig.track_mut(main).effects.rotate_right(1);
    rig.remove_effect(limiter);
    controller.set_project(&rig.project, &rig.pool);
    assert_nulled(
        &live.settled("both paths change at once"),
        "both paths change at once",
    );

    // Positive control: half the key leaves half the signal.
    rig.track_mut(bus).sidechains[0].gain = 0.5;
    controller.set_project(&rig.project, &rig.pool);
    let half = live.settled("key at half level");
    let loudest = half.iter().fold(0.0_f32, |peak, v| peak.max(v.abs()));
    assert!(
        (loudest - 0.25).abs() < 1e-5,
        "half the key leaves 0.25, got {loudest}"
    );
    assert_eq!(
        live.calls, 0,
        "no transition may allocate on the audio thread"
    );
}

fn compressor(sidechain: bool) -> EffectParams {
    EffectParams::Compressor(CompressorParams {
        sidechain,
        detector: DetectorMode::Peak,
        threshold_db: -18.0,
        ratio: 100.0,
        attack_ms: 0.05,
        release_ms: 5.0,
        knee_db: 0.0,
        ..Default::default()
    })
}

fn peak(window: &[f32]) -> f32 {
    window.iter().fold(0.0, |peak, v| peak.max(v.abs()))
}

#[test]
fn qa_detector_route_edits_and_their_history_take_effect_while_the_key_sounds() {
    // A quiet main signal ducked by a loud key.
    let mut rig = Rig::new();
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).length_steps = 64;
    let main = rig.track();
    let key = rig.track();
    rig.track_mut(key).output = None;
    let main_channel = rig.channel_on(level(RATE, 0.05, 8.0), main);
    let key_channel = rig.channel_on(level(RATE, 1.0, 8.0), key);
    rig.steps(main_channel, &[0]);
    rig.steps(key_channel, &[0]);
    let squeeze = rig.effect(main, compressor(true));

    let mut document = Document::new(rig.project.clone());
    let (processor, controller) = rig.processor(RATE);
    let mut live = Live::new(processor);
    controller.play();
    let apply = |document: &Document, live: &mut Live, what: &str| {
        controller.set_project(document.project(), &rig.pool);
        peak(&live.settled(what))
    };
    let open = |peak: f32| (peak - 0.05).abs() < 1e-5;
    let ducked = |peak: f32| peak < 0.01;

    assert!(
        open(apply(&document, &mut live, "no route")),
        "no route: open"
    );

    // The route is made while both tracks are sounding.
    document
        .dispatch(
            Command::SetSidechain {
                from: key,
                to: main,
                gain: Some(1.0),
            },
            None,
        )
        .unwrap();
    assert!(
        ducked(apply(&document, &mut live, "route made")),
        "route made: ducked"
    );

    // Undo and redo of the route, mid-playback.
    document.undo().expect("undo the route");
    assert!(
        open(apply(&document, &mut live, "route undone")),
        "route undone: open"
    );
    document.redo().expect("redo the route");
    assert!(
        ducked(apply(&document, &mut live, "route redone")),
        "route redone: ducked"
    );

    // The key level is pulled to nothing and back.
    document
        .dispatch(
            Command::SetSidechain {
                from: key,
                to: main,
                gain: Some(0.0),
            },
            None,
        )
        .unwrap();
    assert!(
        open(apply(&document, &mut live, "key level zero")),
        "key level zero: open"
    );
    document.undo().expect("undo the level");
    assert!(
        ducked(apply(&document, &mut live, "key level undone")),
        "key level undone: ducked"
    );

    // The key's source is muted, then unmuted.
    for (muted, what) in [(true, "key muted"), (false, "key unmuted")] {
        document
            .dispatch(
                Command::UpdateMixerTrack {
                    id: key,
                    patch: MixerTrackPatch {
                        muted: Some(muted),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
        let heard = apply(&document, &mut live, what);
        assert!(
            if muted { open(heard) } else { ducked(heard) },
            "{what}: {heard}"
        );
    }

    // The compressor is told to listen to itself, then to the key again.
    for (external, what) in [(false, "internal detection"), (true, "external detection")] {
        document
            .dispatch(
                Command::SetEffectParams {
                    track: main,
                    effect: squeeze,
                    params: compressor(external),
                },
                None,
            )
            .unwrap();
        let heard = apply(&document, &mut live, what);
        assert!(
            if external { ducked(heard) } else { open(heard) },
            "{what}: {heard}"
        );
    }

    // Latency joins the key's path while it is ducking; still ducked after.
    document
        .dispatch(
            Command::AddEffect {
                track: key,
                kind: EffectKind::Limiter,
                index: None,
            },
            None,
        )
        .unwrap();
    assert!(
        ducked(apply(&document, &mut live, "latent key path")),
        "latent key path: ducked"
    );

    // The key's track is deleted and restored with one undo. Deleting a
    // track sends its channels to the master, so the key is now heard there
    // at full level beside the main signal, which is no longer ducked.
    document
        .dispatch(Command::RemoveMixerTrack { id: key }, None)
        .unwrap();
    assert!(document.project().mixer.track(main).is_some());
    let exposed = apply(&document, &mut live, "key track deleted");
    assert!(
        (exposed - 1.05).abs() < 1e-4,
        "key track deleted: open, got {exposed}"
    );
    document.undo().expect("undo the delete");
    assert!(
        ducked(apply(&document, &mut live, "key track restored")),
        "key track restored: ducked"
    );
    assert_eq!(
        live.calls, 0,
        "no transition may allocate on the audio thread"
    );
}

#[test]
fn qa_owner_exchange_and_selection_history_keep_the_key_flowing() {
    // A hosted detector that puts its key out, with the second of its
    // auxiliary inputs saved as the choice.
    let mut rig = Rig::new();
    let pattern = rig.first_pattern();
    rig.pattern_mut(pattern).length_steps = 64;
    let main = rig.track();
    let key = rig.track();
    rig.track_mut(key).output = None;
    let key_channel = rig.channel_on(level(RATE, 0.5, 8.0), key);
    rig.steps(key_channel, &[0]);
    rig.track_mut(key).sidechains.push(Send {
        target: main,
        gain: 1.0,
    });
    rig.effect(main, idle_limiter(5.0));
    probe(&mut rig, main, "copy", Some(2));
    let factory = Arc::new(ProbeFactory::default());
    rig.pool.set_plugin_factory(factory.clone());

    let mut document = Document::new(rig.project.clone());
    let target = document.project().plugins[0].target;
    let (processor, controller) = rig.processor(RATE);
    let mut live = Live::new(processor);
    controller.play();
    let carried = |window: &[f32]| (peak(window) - 0.5).abs() < 1e-5;
    assert!(
        carried(&live.settled("start")),
        "the saved input carries the key"
    );
    assert_eq!(factory.creates.load(Relaxed), 1);

    // The provider changes revision while the key is passing: a new owner
    // is prepared, and the saved choice is applied to it without an edit.
    factory.revision.store(1, Relaxed);
    controller.set_project(document.project(), &rig.pool);
    assert!(
        carried(&live.settled("owner exchanged")),
        "the new owner got the saved input"
    );
    assert_eq!(factory.creates.load(Relaxed), 2, "one replacement owner");

    // A latency edit ahead of the detector in the same breath as a second
    // exchange.
    document
        .dispatch(
            Command::AddEffect {
                track: main,
                kind: EffectKind::Limiter,
                index: Some(0),
            },
            None,
        )
        .unwrap();
    factory.revision.store(2, Relaxed);
    controller.set_project(document.project(), &rig.pool);
    assert!(
        carried(&live.settled("exchange with a latency edit")),
        "exchange with a latency edit"
    );
    assert_eq!(factory.creates.load(Relaxed), 3);

    // Choosing an input the plugin does not have silences the key, without
    // a new owner; undo brings the key back, redo takes it away again.
    document
        .dispatch(
            Command::SetPluginSidechainInput {
                target,
                input: Some(7),
            },
            None,
        )
        .unwrap();
    controller.set_project(document.project(), &rig.pool);
    assert_eq!(peak(&live.settled("absent input chosen")), 0.0);
    document.undo().expect("undo the choice");
    controller.set_project(document.project(), &rig.pool);
    assert!(carried(&live.settled("choice undone")), "choice undone");
    document.redo().expect("redo the choice");
    controller.set_project(document.project(), &rig.pool);
    assert_eq!(peak(&live.settled("choice redone")), 0.0);
    document.undo().expect("undo again");
    controller.set_project(document.project(), &rig.pool);
    assert!(
        carried(&live.settled("choice undone again")),
        "choice undone again"
    );
    assert_eq!(
        factory.creates.load(Relaxed),
        3,
        "choosing an input never replaces the owner"
    );
    assert_eq!(
        live.calls, 0,
        "no transition may allocate on the audio thread"
    );
}
