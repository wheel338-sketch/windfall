//! The multi-sidechain fixture through the desktop's two native providers.
//!
//! The fixture is a real plugin, scanned by the real desktop scanner before
//! it may load. After its main stereo input it has three sidechain inputs
//! and marks its output with what each of their channels held:
//!
//! | native input   | channels | added to left     | added to right     |
//! |----------------|----------|-------------------|--------------------|
//! | 1 `Key Mono`   | 1        | key               | -key               |
//! | 2 `Key Stereo` | 2        | first             | second             |
//! | 3 `Key Quad`   | 4        | -first, 2 x third | -second, 2 x fourth |
//!
//! So the output of a block says which input the key reached, that a stereo
//! key was folded for the mono input, and that the channels past the second
//! of the wide input held silence, as did every input that was not chosen.
//!
//! Every test needs the explicitly built fixture and desktop executable:
//! `WINDFALL_BRIDGE_FIXTURE` and `WINDFALL_DESKTOP_BRIDGE_HELPER`. None of
//! them starts another Cargo.
use super::*;
use std::path::PathBuf;
use windfall_project::{EffectId, PluginAuxInput};

const BLOCK: usize = 256;

/// The sidechain input a choice ends up feeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fed {
    /// No input: the main signal passes unchanged.
    Nothing,
    Mono,
    Stereo,
    Quad,
}

/// The fixture's output for one frame, by the table above.
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

fn expected_inputs() -> Vec<PluginAuxInput> {
    [(1, "Key Mono", 1), (2, "Key Stereo", 2), (3, "Key Quad", 4)]
        .into_iter()
        .map(|(index, name, channels)| PluginAuxInput {
            index,
            name: name.into(),
            channels,
        })
        .collect()
}

fn transport() -> windfall_engine::plugins::PluginTransport {
    windfall_engine::plugins::PluginTransport {
        playing: true,
        tempo_bpm: 120.0,
        position_beats: 0.0,
        position_seconds: 0.0,
        numerator: 4,
        denominator: 4,
        meter_anchor: None,
    }
}

/// A plugin manager whose catalog holds the scanned fixture, and the binding
/// the manager discovered for the multi-sidechain plugin in it.
struct Scanned {
    manager: Arc<super::super::PluginManager>,
    binding: PluginBinding,
    // Last: the folder outlives everything that reads from it.
    _folder: tempfile::TempDir,
}

/// Scans a copy of the fixture with the real desktop scanner and discovers
/// the multi-sidechain plugin, in process or through a helper process.
fn scanned(vst3: bool, bridge: bool) -> Scanned {
    let fixture = PathBuf::from(
        std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("explicit prebuilt native fixture"),
    );
    let desktop = PathBuf::from(
        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
            .expect("explicit current desktop scanner and helper"),
    );
    assert!(fixture.is_file() && desktop.is_file());
    let folder = tempfile::tempdir().unwrap();
    let plugins = folder.path().join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let path = plugins.join(if vst3 { "multi.vst3" } else { "multi.clap" });
    std::fs::copy(fixture, &path).unwrap();
    let manager = if bridge {
        super::super::PluginManager::bridge_fixture(folder.path(), &path, &desktop)
    } else {
        super::super::PluginManager::fixture(folder.path(), &path, &desktop)
    };
    let name = if vst3 {
        "VST3 Multi Auxiliary Detector"
    } else {
        "Test Multi Sidechain"
    };
    let entry = manager
        .state()
        .entries
        .into_iter()
        .find(|entry| entry.name == name)
        .expect("the scanner lists the multi-sidechain plugin");
    assert!(entry.usable, "{:?}", entry.error);
    assert!(!entry.instrument);
    let binding = manager
        .binding(
            &entry.path,
            &entry.id,
            PluginTarget::Effect {
                effect: EffectId(4242),
            },
        )
        .unwrap();
    Scanned {
        manager,
        binding,
        _folder: folder,
    }
}

#[test]
fn discovery_stores_every_sidechain_input_with_its_name_width_and_native_index() {
    for vst3 in [false, true] {
        for bridge in [false, true] {
            let scanned = scanned(vst3, bridge);
            assert_eq!(
                scanned.binding.auxiliary_inputs,
                expected_inputs(),
                "vst3={vst3} bridge={bridge}"
            );
            assert_eq!(scanned.binding.sidechain_input, None);
            scanned.binding.validate().unwrap();
        }
    }
}

/// A steady main signal and key, for the helper path: it answers two blocks
/// late and never waits when live, so the tests look for the settled answer.
const MAIN: [f32; 2] = [0.1, 0.2];
const KEY: [f32; 2] = [0.25, 0.75];

/// Processes whole blocks until `needed` of them in a row are the fixture's
/// answer for `fed`.
fn settle(audio: &mut dyn HostedEffect, fed: Fed, live: bool, what: &str) {
    let expected = marked(MAIN, KEY, fed);
    let key = [KEY; BLOCK];
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut settled = 0;
    loop {
        let mut left = [MAIN[0]; BLOCK];
        let mut right = [MAIN[1]; BLOCK];
        let mut process = || audio.process_sidechain(&mut left, &mut right, Some(&key));
        if live {
            assert_eq!(crate::test_alloc::allocator_calls(process), 0, "{what}");
        } else {
            process();
        }
        let matches = left.iter().zip(&right).all(|(left, right)| {
            (left - expected[0]).abs() < 1e-6 && (right - expected[1]).abs() < 1e-6
        });
        settled = if matches { settled + 1 } else { 0 };
        if settled == 4 {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: {:?}/{:?} never settled at {expected:?}",
            &left[..2],
            &right[..2]
        );
        if live {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Every step differs from the one before, so each choice is seen to
/// arrive. A choice that feeds nothing is followed by one that feeds
/// something, which a helper that had stopped answering could not do.
const PLAN: [(Option<u32>, Fed); 9] = [
    (Some(2), Fed::Stereo),
    (None, Fed::Mono),
    (Some(3), Fed::Quad),
    (Some(1), Fed::Mono),
    (Some(4), Fed::Nothing),
    (Some(2), Fed::Stereo),
    (Some(0), Fed::Nothing),
    (Some(3), Fed::Quad),
    (Some(2), Fed::Stereo),
];

fn helper_facade_feeds_each_choice(vst3: bool) {
    let scanned = scanned(vst3, true);
    let runtime = &scanned.manager.runtime;
    let target = scanned.binding.target;

    // Live: the realtime facade the engine plays through.
    let mut audio = runtime.effect(&scanned.binding, 48_000, BLOCK).unwrap();
    audio.transport(transport());
    let token = runtime.selected_token(target).unwrap();
    for (choice, fed) in PLAN {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| audio.set_sidechain_input(choice)),
            0
        );
        settle(
            audio.as_mut(),
            fed,
            true,
            &format!("vst3={vst3} live {choice:?}"),
        );
    }
    let status = runtime.bridge_status();
    assert_eq!(status.len(), 1);
    assert_eq!((status[0].0, status[0].1), (target, token));
    assert_ne!(status[0].2.process_id, std::process::id());
    assert!(!status[0].2.failed && !status[0].2.reaped);

    // Offline: the render provider's own helper, which waits for answers.
    let render = runtime.render_factory().unwrap();
    let mut offline = render.effect(&scanned.binding, 48_000, BLOCK).unwrap();
    offline.transport(transport());
    for (choice, fed) in PLAN {
        offline.set_sidechain_input(choice);
        settle(
            offline.as_mut(),
            fed,
            false,
            &format!("vst3={vst3} offline {choice:?}"),
        );
    }
    assert_eq!(render.render_error(), None);
    assert_eq!(
        runtime.selected_token(target),
        Some(token),
        "a render instance never becomes the playback owner"
    );
}

#[test]
fn helper_facade_feeds_the_chosen_clap_sidechain_input_live_and_offline() {
    helper_facade_feeds_each_choice(false);
}

#[test]
fn helper_facade_feeds_the_chosen_vst3_sidechain_input_live_and_offline() {
    helper_facade_feeds_each_choice(true);
}

/// One block through the in-process owner's audio half, checked frame by
/// frame. Returns the position after it.
fn check_block(audio: &mut dyn HostedEffect, at: usize, fed: Fed, what: &str) -> usize {
    const FRAMES: usize = 64;
    let main: [[f32; 2]; FRAMES] = std::array::from_fn(|frame| main_at(at + frame));
    let key: [[f32; 2]; FRAMES] = std::array::from_fn(|frame| key_at(at + frame));
    let mut left: [f32; FRAMES] = std::array::from_fn(|frame| main[frame][0]);
    let mut right: [f32; FRAMES] = std::array::from_fn(|frame| main[frame][1]);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| audio.process_sidechain(
            &mut left,
            &mut right,
            Some(&key)
        )),
        0,
        "{what}"
    );
    for frame in 0..FRAMES {
        assert_eq!(
            [left[frame], right[frame]],
            marked(main[frame], key[frame], fed),
            "{what} frame {frame}"
        );
    }
    at + FRAMES
}

/// Has the owner take the processor back at a block boundary and destroy
/// it, runs `between` while the audio half has none, then has the owner
/// prepare a new processor and hand it over.
fn replace_processor(
    runtime: &Runtime,
    target: PluginTarget,
    audio: &mut dyn HostedEffect,
    between: impl FnOnce(&mut dyn HostedEffect),
) {
    let token = runtime.selected_token(target).unwrap();
    let (ready, ready_rx) = mpsc::channel();
    let (resume, resume_rx) = mpsc::channel::<()>();
    runtime
        .jobs
        .send(Box::new(move |owner| {
            let record = owner.instances.get_mut(&token).unwrap();
            record.ownership.request();
            let adapter = loop {
                if let Some(adapter) = record.ownership.take_returned() {
                    break adapter;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            // Deactivates and destroys the processor the audio half had.
            record.release(adapter).unwrap();
            ready.send(()).unwrap();
            resume_rx.recv().unwrap();
            // Activates the plugin again: a processor made afresh.
            record.restore().unwrap();
        }))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        audio.control_boundary();
        if ready_rx.try_recv().is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    between(audio);
    resume.send(()).unwrap();
    // The owner has finished the job once it answers the next one.
    runtime.call(|_| Ok(())).unwrap();
    audio.control_boundary();
}

fn capture_while_audio_runs(
    runtime: &Arc<Runtime>,
    project: Project,
    audio: &mut dyn HostedEffect,
) -> Project {
    let runtime = runtime.clone();
    let task = std::thread::spawn(move || runtime.capture(project));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline, "capture hung");
        audio.control_boundary();
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = task.join().unwrap();
    audio.control_boundary();
    result.unwrap()
}

fn owner_exchange_keeps_the_saved_choice(vst3: bool) {
    let mut scanned = scanned(vst3, false);
    // The selection as a saved project holds it: the second sidechain
    // input, which is not the one a processor starts with.
    scanned.binding.sidechain_input = Some(2);
    scanned.binding.validate().unwrap();
    let runtime = scanned.manager.runtime.clone();
    let binding = scanned.binding.clone();
    let target = binding.target;
    let what = if vst3 { "vst3" } else { "clap" };

    let mut audio = runtime.effect(&binding, 48_000, 64).unwrap();
    audio.transport(transport());
    // What the engine does once, when it adopts the binding. Nothing below
    // makes the choice again.
    audio.set_sidechain_input(binding.sidechain_input);
    let token = runtime.selected_token(target).unwrap();
    let mut at = check_block(audio.as_mut(), 0, Fed::Stereo, what);

    // The owner replaces the processor. With none, the effect passes its
    // input and must not put the key anywhere, the main signal included.
    replace_processor(&runtime, target, audio.as_mut(), |audio| {
        at = check_block(audio, at, Fed::Nothing, what);
    });
    assert_eq!(runtime.selected_token(target), Some(token));
    for _ in 0..3 {
        at = check_block(audio.as_mut(), at, Fed::Stereo, what);
    }

    // Saving captures native state. For VST3 that deactivates the plugin
    // and prepares another processor; CLAP is captured while it runs.
    let mut project = Project::new("Saved sidechain choice");
    project.plugins.push(binding.clone());
    let saved = capture_while_audio_runs(&runtime, project, audio.as_mut());
    assert_eq!(saved.plugins[0].sidechain_input, Some(2));
    assert_eq!(saved.plugins[0].auxiliary_inputs, expected_inputs());
    assert_eq!(runtime.selected_token(target), Some(token));
    for _ in 0..3 {
        at = check_block(audio.as_mut(), at, Fed::Stereo, what);
    }

    // A second replacement, and the choice the user then really changes.
    replace_processor(&runtime, target, audio.as_mut(), |_| {});
    at = check_block(audio.as_mut(), at, Fed::Stereo, what);
    audio.set_sidechain_input(Some(3));
    at = check_block(audio.as_mut(), at, Fed::Quad, what);
    replace_processor(&runtime, target, audio.as_mut(), |_| {});
    at = check_block(audio.as_mut(), at, Fed::Quad, what);
    // A saved input the plugin does not have stays silent across it too.
    audio.set_sidechain_input(Some(9));
    at = check_block(audio.as_mut(), at, Fed::Nothing, what);
    replace_processor(&runtime, target, audio.as_mut(), |_| {});
    check_block(audio.as_mut(), at, Fed::Nothing, what);

    // The state the save captured opens with the same choice.
    let render = runtime.render_factory().unwrap();
    let mut reopened = render.effect(&saved.plugins[0], 48_000, 64).unwrap();
    reopened.transport(transport());
    reopened.set_sidechain_input(saved.plugins[0].sidechain_input);
    check_block(reopened.as_mut(), 0, Fed::Stereo, what);
    assert!(runtime.errors().is_empty(), "{:?}", runtime.errors());
}

#[test]
fn in_process_owner_exchange_keeps_the_saved_clap_sidechain_input() {
    owner_exchange_keeps_the_saved_choice(false);
}

#[test]
fn in_process_owner_exchange_keeps_the_saved_vst3_sidechain_input() {
    owner_exchange_keeps_the_saved_choice(true);
}
