//! A plugin with several sidechain inputs gets the key in the one that was
//! chosen, and silence in the others.
//!
//! The fixture is the multi-sidechain plugin of the test plugins, in both
//! formats. After its main stereo input it has a one-channel, a two-channel
//! and a four-channel sidechain input, and it adds each of their seven
//! channels to its output with a mark of its own:
//!
//! | input      | channel | added to left | added to right |
//! |------------|---------|---------------|----------------|
//! | Key Mono   | 1       | key           | -key           |
//! | Key Stereo | 1, 2    | first         | second         |
//! | Key Quad   | 1, 2    | -first        | -second        |
//! | Key Quad   | 3, 4    | 2 x third     | 2 x fourth     |
//!
//! The key the tests send differs on its two sides and from frame to frame,
//! so an output that equals the main signal plus exactly one row of marks
//! shows which input was fed, that a stereo key was folded for the mono
//! input, that the wider input got the key in its first two channels only,
//! and that every other channel held silence.
//!
//! The first half drives the host in this process. The second half goes
//! through a real audio helper process, where the key travels in the shared
//! mapping's key plane and the choice in its selection word.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use windfall_plugin_host::{AudioPort, PluginHost, PluginInstance, PluginModule, ProcessStatus};

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every call made on a thread while that
/// thread is inside [`allocator_calls`].
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

/// Runs `work` and returns how many times it allocated, reallocated or
/// freed memory.
fn allocator_calls(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    WATCHING.set(true);
    work();
    WATCHING.set(false);
    CALLS.get()
}

const RATE: f64 = 48_000.0;
const FORMATS: [&str; 2] = ["clap", "vst3"];
const MULTI_CLAP: &str = "org.windfall.test.multi-sidechain";
const MULTI_VST3: &str = "VST3 Multi Auxiliary Detector";

/// The fixture's sidechain inputs as the host must report them, in the
/// order of their native input indices 1, 2 and 3.
const KEY_INPUTS: [(&str, u32); 3] = [("Key Mono", 1), ("Key Stereo", 2), ("Key Quad", 4)];

/// The sidechain input a choice ends up feeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fed {
    /// No input: the main signal passes unchanged.
    Nothing,
    Mono,
    Stereo,
    Quad,
}

/// What the host's choices mean for this fixture. `None` is the first
/// sidechain input. Index 0 is the main input, which is never a sidechain,
/// and indices past the last input name nothing.
const CHOICES: [(Option<u32>, Fed); 8] = [
    (None, Fed::Mono),
    (Some(1), Fed::Mono),
    (Some(2), Fed::Stereo),
    (Some(3), Fed::Quad),
    (Some(4), Fed::Nothing),
    (Some(0), Fed::Nothing),
    (Some(63), Fed::Nothing),
    (Some(2), Fed::Stereo),
];

/// The fixture's output for one frame, by the table at the top.
fn marked(main: [f32; 2], key: [f32; 2], fed: Fed) -> [f32; 2] {
    match fed {
        Fed::Nothing => main,
        Fed::Mono => {
            let folded = (key[0] + key[1]) * 0.5;
            [main[0] + folded, main[1] - folded]
        }
        Fed::Stereo => [main[0] + key[0], main[1] + key[1]],
        Fed::Quad => [main[0] - key[0], main[1] - key[1]],
    }
}

/// A main signal and a key that differ from each other, between their sides
/// and from frame to frame. Both are functions of the position in the
/// stream, so a test can make them in pieces of any length.
fn main_at(frame: usize) -> [f32; 2] {
    [
        (frame as f32 * 0.1).sin() * 0.5,
        (frame as f32 * 0.07).cos() * 0.25,
    ]
}

fn key_at(frame: usize) -> [f32; 2] {
    [
        0.125 + (frame % 17) as f32 / 64.0,
        -0.375 + (frame % 29) as f32 / 128.0,
    ]
}

fn fixture_id(module: &PluginModule, format: &str) -> String {
    if format == "clap" {
        MULTI_CLAP.to_owned()
    } else {
        module
            .descriptors()
            .into_iter()
            .find(|descriptor| descriptor.name == MULTI_VST3)
            .expect("the VST3 fixture has the multi-detector class")
            .id
    }
}

fn create(format: &str) -> (PluginModule, PluginInstance) {
    let path = common::plugin_file("sidechain-buses", &format!("multi.{format}"));
    let module = PluginHost::windfall()
        .load(&path)
        .expect("the fixture loads");
    let instance = module
        .create(&fixture_id(&module, format))
        .expect("the multi-sidechain plugin can be created");
    (module, instance)
}

fn assert_key_inputs(inputs: &[AudioPort], format: &str) {
    assert_eq!(inputs.len(), 4, "{format}: {inputs:?}");
    assert!(inputs[0].main, "{format}");
    assert_eq!(inputs[0].channels, 2, "{format}");
    for (index, (name, channels)) in KEY_INPUTS.iter().enumerate() {
        // The native index a selection names is the position in this list.
        let port = &inputs[index + 1];
        assert_eq!(port.name, *name, "{format} input {}", index + 1);
        assert_eq!(port.channels, *channels, "{format} input {}", index + 1);
        assert!(!port.main, "{format} input {}", index + 1);
    }
}

#[test]
fn the_layout_names_every_sidechain_input_with_its_width_and_index() {
    for format in FORMATS {
        let (_module, instance) = create(format);
        assert_key_inputs(&instance.layout().audio_inputs, format);
        assert_eq!(instance.layout().audio_outputs.len(), 1, "{format}");
    }
}

#[test]
fn the_key_reaches_the_chosen_input_and_the_others_stay_silent() {
    for format in FORMATS {
        let (_module, mut instance) = create(format);
        // 300 frames with a largest block of 128: the key is cut into the
        // same pieces as the main signal.
        let mut processor = instance.activate(RATE, 128).unwrap();
        let frames = 300;
        let mut position = 0;
        for (choice, fed) in CHOICES {
            processor.set_sidechain_input(choice);
            let main: Vec<[f32; 2]> = (position..position + frames).map(main_at).collect();
            let key: Vec<[f32; 2]> = (position..position + frames).map(key_at).collect();
            let mut left: Vec<f32> = main.iter().map(|frame| frame[0]).collect();
            let mut right: Vec<f32> = main.iter().map(|frame| frame[1]).collect();
            assert_eq!(
                processor.process_sidechain(&mut left, &mut right, Some(&key)),
                ProcessStatus::Continue,
                "{format} {choice:?}"
            );
            for frame in 0..frames {
                assert_eq!(
                    [left[frame], right[frame]],
                    marked(main[frame], key[frame], fed),
                    "{format} choice {choice:?} frame {frame}"
                );
            }
            position += frames;
        }
        assert!(!processor.health().failed, "{format}");
        assert_eq!(processor.health().scrubbed_samples, 0, "{format}");
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn a_block_without_a_key_and_a_short_key_feed_silence() {
    for format in FORMATS {
        let (_module, mut instance) = create(format);
        let mut processor = instance.activate(RATE, 256).unwrap();
        processor.set_sidechain_input(Some(2));
        let frames = 256;
        let main: Vec<[f32; 2]> = (0..frames).map(main_at).collect();
        let key: Vec<[f32; 2]> = (0..frames).map(key_at).collect();
        let fresh = || -> (Vec<f32>, Vec<f32>) {
            (
                main.iter().map(|frame| frame[0]).collect(),
                main.iter().map(|frame| frame[1]).collect(),
            )
        };

        // A key first, so the sidechain buffers hold something to forget.
        let (mut left, mut right) = fresh();
        processor.process_sidechain(&mut left, &mut right, Some(&key));
        assert_eq!([left[5], right[5]], marked(main[5], key[5], Fed::Stereo));

        // The plain call has no key: nothing of the last one may remain.
        let (mut left, mut right) = fresh();
        processor.process(&mut left, &mut right);
        for frame in 0..frames {
            assert_eq!([left[frame], right[frame]], main[frame], "{format} {frame}");
        }

        // A key shorter than the block is silent past its end.
        let (mut left, mut right) = fresh();
        processor.process_sidechain(&mut left, &mut right, Some(&key[..100]));
        for frame in 0..frames {
            let fed = if frame < 100 {
                Fed::Stereo
            } else {
                Fed::Nothing
            };
            assert_eq!(
                [left[frame], right[frame]],
                marked(main[frame], key[frame], fed),
                "{format} {frame}"
            );
        }
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn key_samples_that_are_not_numbers_become_silence() {
    for format in FORMATS {
        let (_module, mut instance) = create(format);
        let mut processor = instance.activate(RATE, 64).unwrap();
        for (choice, fed) in [(Some(1), Fed::Mono), (Some(3), Fed::Quad)] {
            processor.set_sidechain_input(choice);
            let mut key: Vec<[f32; 2]> = (0..64).map(key_at).collect();
            key[3] = [f32::NAN, 0.25];
            key[4] = [0.5, f32::INFINITY];
            key[5] = [f32::NEG_INFINITY, f32::NAN];
            let mut left = vec![0.0_f32; 64];
            let mut right = vec![0.0_f32; 64];
            processor.process_sidechain(&mut left, &mut right, Some(&key));
            assert!(
                left.iter().chain(&right).all(|sample| sample.is_finite()),
                "{format} {choice:?}"
            );
            // The mono fold of a pair with a bad side is bad as a whole. The
            // wider input takes each side on its own.
            let expected = match fed {
                Fed::Mono => [[0.0; 2]; 3],
                _ => [[0.0, -0.25], [-0.5, 0.0], [0.0, 0.0]],
            };
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    [left[3 + index], right[3 + index]],
                    expected,
                    "{format} {choice:?} frame {}",
                    3 + index
                );
            }
            assert_eq!(
                [left[6], right[6]],
                marked([0.0; 2], key[6], fed),
                "{format} {choice:?}"
            );
            assert_eq!(processor.health().scrubbed_samples, 0, "{format}");
        }
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn choosing_and_feeding_a_sidechain_input_does_not_allocate() {
    for format in FORMATS {
        let (_module, mut instance) = create(format);
        let mut processor = instance.activate(RATE, 128).unwrap();
        let key: Vec<[f32; 2]> = (0..300).map(key_at).collect();
        let mut left = vec![0.25_f32; 300];
        let mut right = vec![-0.25_f32; 300];
        // One block outside the guard: a plugin may set itself up on its
        // first block, which is not the host's doing.
        processor.process_sidechain(&mut left, &mut right, Some(&key));
        let calls = allocator_calls(|| {
            for (choice, _) in CHOICES {
                processor.set_sidechain_input(choice);
                processor.process_sidechain(&mut left, &mut right, Some(&key));
                processor.process(&mut left, &mut right);
            }
        });
        assert_eq!(calls, 0, "{format}");
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn the_effect_adapter_keeps_the_choice_and_feeds_the_same_input() {
    for format in FORMATS {
        let (_module, mut instance) = create(format);
        let mut effect = instance.prepare_effect(48_000.0, 256).unwrap();
        effect.processor().set_sidechain_input(Some(3));
        for block in 0..3 {
            let start = block * 256;
            let main: Vec<[f32; 2]> = (start..start + 256).map(main_at).collect();
            let key: Vec<[f32; 2]> = (start..start + 256).map(key_at).collect();
            let mut left: Vec<f32> = main.iter().map(|frame| frame[0]).collect();
            let mut right: Vec<f32> = main.iter().map(|frame| frame[1]).collect();
            let calls =
                allocator_calls(|| effect.process_sidechain(&mut left, &mut right, Some(&key)));
            assert!(block == 0 || calls == 0, "{format} block {block}: {calls}");
            for frame in 0..256 {
                assert_eq!(
                    [left[frame], right[frame]],
                    marked(main[frame], key[frame], Fed::Quad),
                    "{format} block {block} frame {frame}"
                );
            }
        }
        instance.release_effect(effect).map_err(|_| ()).unwrap();

        // A processor made afresh starts at the first sidechain input: the
        // choice belongs to the caller, who has to make it again.
        let mut effect = instance.prepare_effect(48_000.0, 256).unwrap();
        let main: Vec<[f32; 2]> = (0..256).map(main_at).collect();
        let key: Vec<[f32; 2]> = (0..256).map(key_at).collect();
        let mut left: Vec<f32> = main.iter().map(|frame| frame[0]).collect();
        let mut right: Vec<f32> = main.iter().map(|frame| frame[1]).collect();
        effect.process_sidechain(&mut left, &mut right, Some(&key));
        assert_eq!(
            [left[9], right[9]],
            marked(main[9], key[9], Fed::Mono),
            "{format}"
        );
        instance.release_effect(effect).map_err(|_| ()).unwrap();
    }
}

/// The same fixture behind a real audio helper process.
#[cfg(windows)]
mod helper_process {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    use windfall_plugin_host::bridge::{
        adapter::Audio,
        protocol::{Config, Kind},
        supervisor::{self, Control, Launch},
    };

    const BLOCK: usize = 64;
    /// The helper answers two blocks late, whatever the plugin's own latency.
    const DELAY: usize = BLOCK * 2;

    /// The fixture file the helper loads: the explicitly built one if the
    /// run names it, as the other helper tests do, or the one this crate's
    /// tests build.
    fn fixture(format: &str) -> PathBuf {
        if let Some(source) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") {
            // Publish the copy completely before any helper can load it.
            static COPY: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _copy = COPY.lock().unwrap();
            let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join(format!("sidechain-buses-fixture-{}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let target = folder.join(format!("multi.{format}"));
            if !target.exists() {
                std::fs::copy(source, &target).unwrap();
            }
            target
        } else {
            common::plugin_file("sidechain-buses-helper", &format!("multi.{format}"))
        }
    }

    fn launch(format: &str, offline: bool) -> (Control, Audio) {
        let plugin = fixture(format);
        let id = {
            let module = PluginHost::windfall().load(&plugin).unwrap();
            fixture_id(&module, format)
        };
        let options = Launch {
            helper: PathBuf::from(env!("CARGO_BIN_EXE_windfall-plugin-audio")),
            // The helper refuses a file that is not the one approved here.
            approved_binary: windfall_plugin_host::paths::plugin_file_identity(&plugin).unwrap(),
            plugin,
            id,
            format: format.into(),
            config: Config {
                identity: supervisor::fresh_identity(1, 7, 99).unwrap(),
                sample_rate: 48_000,
                block: BLOCK,
                native_latency: 0,
                kind: Kind::Effect,
            },
            parameters: vec![],
            state: vec![],
            offline,
            startup_timeout: Duration::from_secs(5),
            audio_timeout: Duration::from_secs(2),
            cancelled: None,
        };
        let (control, audio, _) = supervisor::launch(options).unwrap();
        (control, audio)
    }

    #[test]
    fn a_fresh_helper_describes_every_sidechain_input() {
        for format in FORMATS {
            let (control, audio) = launch(format, false);
            let (_, _, layout) = control.describe(Duration::from_secs(2)).unwrap();
            assert_key_inputs(&layout.audio_inputs, format);
            // What the helper reports is what the plugin reports in process.
            let (_module, instance) = create(format);
            assert_eq!(layout, *instance.layout(), "{format}");
            drop(audio);
            assert!(control.terminate().reaped, "{format}");
        }
    }

    /// Renders the plan through an offline helper, `callback` frames at a
    /// time, moving to the next choice of the plan every `each` blocks, and
    /// checks every frame the helper answered.
    fn render_plan(format: &str, callback: usize, plan: &[(Option<u32>, Fed)], each: usize) {
        let (control, mut audio) = launch(format, true);
        let total = plan.len() * each * BLOCK;
        let cancelled = AtomicBool::new(false);
        let mut heard = Vec::with_capacity(total);
        // What each whole block of the stream has to have been fed.
        let mut block_fed = vec![Fed::Nothing; total / BLOCK];
        let mut at = 0;
        while at < total {
            let count = callback.min(total - at);
            let (choice, fed) = plan[at / (each * BLOCK)];
            audio.set_sidechain_input(choice);
            // A choice takes effect at a whole block: it holds for every
            // block that starts during the callback that follows it, and
            // not for the rest of a block that had already started.
            for block in at.div_ceil(BLOCK)..(at + count).div_ceil(BLOCK) {
                block_fed[block] = fed;
            }
            let key: Vec<[f32; 2]> = (at..at + count).map(key_at).collect();
            let mut left: Vec<f32> = (at..at + count).map(|frame| main_at(frame)[0]).collect();
            let mut right: Vec<f32> = (at..at + count).map(|frame| main_at(frame)[1]).collect();
            audio
                .process_offline_sidechain(
                    &mut left,
                    &mut right,
                    Some(&key),
                    Instant::now() + Duration::from_secs(5),
                    &cancelled,
                )
                .unwrap_or_else(|error| panic!("{format} callback {callback}: {error:?}"));
            heard.extend(left.iter().zip(&right).map(|(left, right)| [*left, *right]));
            at += count;
        }
        assert!(
            heard[..DELAY].iter().all(|frame| *frame == [0.0; 2]),
            "{format} callback {callback}: sound before the helper answered"
        );
        for (frame, actual) in heard[DELAY..].iter().enumerate() {
            let fed = block_fed[frame / BLOCK];
            assert_eq!(
                *actual,
                marked(main_at(frame), key_at(frame), fed),
                "{format} callback {callback} frame {frame} fed {fed:?}"
            );
        }
        // Every kind of answer was heard, whatever the callback length.
        for fed in [Fed::Nothing, Fed::Mono, Fed::Stereo, Fed::Quad] {
            assert!(
                block_fed[..(total - DELAY) / BLOCK].contains(&fed),
                "{format} callback {callback}: {fed:?} was never checked"
            );
        }
        assert_eq!(audio.health().missed_blocks, 0, "{format}");
        assert_eq!(audio.health().corrupt_blocks, 0, "{format}");
        assert!(audio.health().completed_blocks > 0, "{format}");
        assert!(!control.status().failed, "{format}");
        drop(audio);
        assert!(control.terminate().reaped, "{format}");
    }

    #[test]
    fn the_key_plane_and_selection_word_reach_the_chosen_input_offline() {
        for format in FORMATS {
            for callback in [BLOCK, 1, 7, 48, 480] {
                // Each choice lasts at least two callbacks, so that none of
                // the plan is skipped by a long one.
                let each = (callback.div_ceil(BLOCK) * 2).max(4);
                render_plan(format, callback, &CHOICES, each);
            }
        }
    }

    #[test]
    fn a_choice_made_inside_a_block_waits_for_the_next_whole_block() {
        for format in FORMATS {
            let (control, mut audio) = launch(format, true);
            let cancelled = AtomicBool::new(false);
            let mut heard = Vec::new();
            let mut feed = |audio: &mut Audio, from: usize, count: usize| {
                let key: Vec<[f32; 2]> = (from..from + count).map(key_at).collect();
                let mut left: Vec<f32> = (from..from + count)
                    .map(|frame| main_at(frame)[0])
                    .collect();
                let mut right: Vec<f32> = (from..from + count)
                    .map(|frame| main_at(frame)[1])
                    .collect();
                audio
                    .process_offline_sidechain(
                        &mut left,
                        &mut right,
                        Some(&key),
                        Instant::now() + Duration::from_secs(5),
                        &cancelled,
                    )
                    .unwrap();
                heard.extend(left.iter().zip(&right).map(|(left, right)| [*left, *right]));
            };
            audio.set_sidechain_input(Some(2));
            // Two whole blocks, then ten frames into the third.
            feed(&mut audio, 0, BLOCK * 2 + 10);
            audio.set_sidechain_input(Some(3));
            // The rest of the third block and four more.
            feed(&mut audio, BLOCK * 2 + 10, BLOCK * 5 - 10);
            // Enough again to hear all of it back.
            feed(&mut audio, BLOCK * 7, DELAY);
            for frame in 0..BLOCK * 7 {
                let fed = if frame < BLOCK * 3 {
                    Fed::Stereo
                } else {
                    Fed::Quad
                };
                assert_eq!(
                    heard[DELAY + frame],
                    marked(main_at(frame), key_at(frame), fed),
                    "{format} frame {frame}"
                );
            }
            assert_eq!(audio.health().missed_blocks, 0, "{format}");
            drop(audio);
            assert!(control.terminate().reaped, "{format}");
        }
    }

    #[test]
    fn a_key_shorter_than_the_audio_is_silent_past_its_own_end_offline() {
        for format in FORMATS {
            let (control, mut audio) = launch(format, true);
            let cancelled = AtomicBool::new(false);
            audio.set_sidechain_input(Some(2));
            // Five blocks of audio with a key that ends inside the second.
            let frames = BLOCK * 5;
            let short = BLOCK + 10;
            let key: Vec<[f32; 2]> = (0..short).map(key_at).collect();
            let mut left: Vec<f32> = (0..frames).map(|frame| main_at(frame)[0]).collect();
            let mut right: Vec<f32> = (0..frames).map(|frame| main_at(frame)[1]).collect();
            audio
                .process_offline_sidechain(
                    &mut left,
                    &mut right,
                    Some(&key),
                    Instant::now() + Duration::from_secs(5),
                    &cancelled,
                )
                .unwrap();
            for frame in 0..frames - DELAY {
                let fed = if frame < short {
                    Fed::Stereo
                } else {
                    Fed::Nothing
                };
                assert_eq!(
                    [left[DELAY + frame], right[DELAY + frame]],
                    marked(main_at(frame), key_at(frame), fed),
                    "{format} frame {frame}"
                );
            }
            drop(audio);
            assert!(control.terminate().reaped, "{format}");
        }
    }

    #[test]
    fn the_realtime_callback_feeds_the_chosen_input_without_allocating() {
        for format in FORMATS {
            let (control, mut audio) = launch(format, false);
            // A steady main signal and key: the realtime path never waits,
            // so the test looks for the settled answer instead of a frame.
            let main = [0.1_f32, 0.2];
            let steady = [0.25_f32, 0.75];
            let key = [steady; BLOCK];
            let mut previous = None;
            // Every step changes the answer, so each one is seen to arrive.
            let plan = [
                (Some(2), Fed::Stereo),
                (None, Fed::Mono),
                (Some(3), Fed::Quad),
                (Some(1), Fed::Mono),
                (Some(4), Fed::Nothing),
                (Some(2), Fed::Stereo),
                (Some(0), Fed::Nothing),
                (Some(3), Fed::Quad),
                (Some(63), Fed::Nothing),
            ];
            for (choice, fed) in plan {
                let expected = marked(main, steady, fed);
                assert_ne!(
                    previous,
                    Some(expected),
                    "every step of the plan has to change the answer"
                );
                assert_eq!(
                    allocator_calls(|| audio.set_sidechain_input(choice)),
                    0,
                    "{format}"
                );
                let deadline = Instant::now() + Duration::from_secs(4);
                let mut settled = 0;
                loop {
                    let before = audio.health();
                    let mut left = [main[0]; BLOCK];
                    let mut right = [main[1]; BLOCK];
                    assert_eq!(
                        allocator_calls(|| audio.process_sidechain(
                            &mut left,
                            &mut right,
                            Some(&key)
                        )),
                        0,
                        "{format} {choice:?}"
                    );
                    let after = audio.health();
                    // Only a block the helper itself answered counts: the
                    // dry fallback of a missed block is the main signal,
                    // which is also what an input that got no key gives.
                    let answered = after.completed_blocks == before.completed_blocks + 1
                        && after.missed_blocks == before.missed_blocks;
                    let matches = left.iter().zip(&right).all(|(left, right)| {
                        (left - expected[0]).abs() < 1e-6 && (right - expected[1]).abs() < 1e-6
                    });
                    settled = if answered && matches { settled + 1 } else { 0 };
                    if settled == 4 {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "{format} {choice:?}: {:?}/{:?} never settled at {expected:?}",
                        &left[..2],
                        &right[..2]
                    );
                    assert!(!control.status().failed, "{format} {choice:?}");
                    std::thread::sleep(Duration::from_millis(2));
                }
                previous = Some(expected);
            }
            assert_eq!(audio.health().corrupt_blocks, 0, "{format}");
            drop(audio);
            assert!(control.terminate().reaped, "{format}");
        }
    }
}
