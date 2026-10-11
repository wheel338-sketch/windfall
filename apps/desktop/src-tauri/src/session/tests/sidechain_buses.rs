//! A saved sidechain-input choice through the whole desktop: the session's
//! document, the engine's detector route, the native provider and a real
//! plugin with three sidechain inputs.
//!
//! One mixer track carries the fixture plugin and a steady signal of its
//! own. A second track, disconnected from the mix, feeds only the first
//! one's detector with a steady key that is four times as loud on the left
//! as on the right. The fixture adds what each sidechain input holds to its
//! output with a mark of its own (see `plugins/sidechain_bus_tests.rs`), so
//! the master's steady level says which input the key reached:
//!
//! - the first, mono one: up on the left and down by as much on the right,
//! - the second, stereo one: up on both sides, four to one,
//! - the third, four-channel one: down on both sides, four to one,
//! - none: the track's own signal and nothing else.
//!
//! Every test needs the explicitly built fixture and desktop executable:
//! `WINDFALL_BRIDGE_FIXTURE` and `WINDFALL_DESKTOP_BRIDGE_HELPER`.
use super::{Rig, SAMPLE_RATE};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{Command, EffectId, MixerTrackPatch, PluginAuxInput, PluginTarget, TrackId};

const KEY: [f32; 2] = [0.4, 0.1];
const OWN: [f32; 2] = [0.05, 0.05];
/// How far two steady levels may differ and still be the same level.
const CLOSE: f32 = 2e-4;

struct Stage {
    rig: Rig,
    manager: Arc<crate::plugins::PluginManager>,
    target: PluginTarget,
    carrier: TrackId,
    source: TrackId,
    /// The provider answers through a helper process, two blocks late and
    /// without waiting, instead of in this process.
    helper: bool,
}

fn steady_file(rig: &Rig, name: &str, level: [f32; 2]) -> String {
    let frames = SAMPLE_RATE as usize * 8;
    let samples: Vec<f32> = (0..frames).flat_map(|_| level).collect();
    let path = rig.file(name);
    windfall_codec::write_wav(
        Path::new(&path),
        &windfall_core::AudioBuffer::from_interleaved(SAMPLE_RATE, 2, samples),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    path
}

fn track(rig: &Rig, name: &str, level: [f32; 2]) -> TrackId {
    let id = TrackId(
        rig.session
            .dispatch(
                Command::AddMixerTrack {
                    name: Some(name.into()),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id,
                patch: MixerTrackPatch {
                    volume: Some(1.0),
                    pan: Some(0.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let file = steady_file(rig, &format!("{name}.wav"), level);
    rig.session
        .add_audio_clip_from_file(
            &file,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: Some(id),
            },
        )
        .unwrap();
    id
}

fn stage(vst3: bool, helper: bool) -> Stage {
    let rig = Rig::new();
    let fixture = PathBuf::from(
        std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("explicit prebuilt native fixture"),
    );
    let desktop = PathBuf::from(
        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
            .expect("explicit current desktop scanner and helper"),
    );
    assert!(fixture.is_file() && desktop.is_file());
    let folder = rig.folder.path().join("sidechain-plugins");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(if vst3 { "multi.vst3" } else { "multi.clap" });
    std::fs::copy(fixture, &path).unwrap();
    // Both providers scan with the real desktop scanner before loading.
    let manager = if helper {
        crate::plugins::PluginManager::bridge_fixture(rig.folder.path(), &path, &desktop)
    } else {
        crate::plugins::PluginManager::fixture(rig.folder.path(), &path, &desktop)
    };
    rig.session.install_plugins(manager.clone());
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
    let binding = manager
        .binding(
            &entry.path,
            &entry.id,
            PluginTarget::Effect {
                effect: EffectId(0),
            },
        )
        .unwrap();
    assert_eq!(
        binding.auxiliary_inputs,
        [(1, "Key Mono", 1), (2, "Key Stereo", 2), (3, "Key Quad", 4)].map(
            |(index, name, channels)| PluginAuxInput {
                index,
                name: name.into(),
                channels,
            }
        )
    );

    let carrier = track(&rig, "Carrier", OWN);
    let source = track(&rig, "Detector source", KEY);
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: TrackId::MASTER,
                patch: MixerTrackPatch {
                    volume: Some(1.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    // The source is heard only by the detector it feeds.
    rig.session
        .dispatch(
            Command::SetTrackOutput {
                id: source,
                output: None,
            },
            None,
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(
            Command::AddPluginEffect {
                track: carrier,
                plugin: binding,
            },
            None,
        )
        .unwrap();
    let target = PluginTarget::Effect {
        effect: EffectId(*added.created.last().unwrap()),
    };
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
    Stage {
        rig,
        manager,
        target,
        carrier,
        source,
        helper,
    }
}

impl Stage {
    fn route(&self, gain: Option<f32>) {
        self.rig
            .session
            .dispatch(
                Command::SetSidechain {
                    from: self.source,
                    to: self.carrier,
                    gain,
                },
                None,
            )
            .unwrap();
    }

    /// The user's choice of sidechain input, as the plugin panel makes it.
    fn choose(&self, input: Option<u32>) {
        self.rig
            .session
            .dispatch(
                Command::SetPluginSidechainInput {
                    target: self.target,
                    input,
                },
                None,
            )
            .unwrap();
        assert_eq!(self.saved_choice(), input);
    }

    fn saved_choice(&self) -> Option<u32> {
        self.rig
            .project()
            .plugin(self.target)
            .unwrap()
            .sidechain_input
    }

    /// Plays the song from its start and returns the level the master
    /// settles at. The audio callback may not allocate while it does.
    fn level(&mut self) -> [f32; 2] {
        self.rig.session.transport_stop();
        self.rig.session.transport_seek(0.0);
        self.rig.session.transport_play().unwrap();
        let mut out = [0.0_f32; 512];
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = [f32::NAN; 2];
        let mut steady = 0;
        let mut callbacks = 0;
        loop {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| self.rig.processor.process(&mut out)),
                0
            );
            if self.helper {
                std::thread::sleep(Duration::from_millis(8));
            }
            callbacks += 1;
            let frames = out.as_chunks::<2>().0;
            let first = frames[0];
            let close =
                |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
            let flat = frames.iter().all(|frame| close(*frame, first));
            steady = if flat && close(first, last) {
                steady + 1
            } else {
                0
            };
            last = first;
            // Long enough for the fallback of a helper that has not yet
            // answered to have given way to its answers.
            if callbacks >= 40 && steady >= 12 {
                self.rig.session.transport_stop();
                return first;
            }
            assert!(
                Instant::now() < deadline,
                "the master never settled; last {first:?}"
            );
        }
    }

    /// The process that hosts the playing plugin, when a helper does.
    fn helper_status(&self) -> (u64, windfall_plugin_host::bridge::supervisor::Status) {
        // A helper for an edited document is started off the audio thread
        // and takes over when it has answered.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let all = self.manager.runtime.bridge_status();
            if let Some(status) = all.iter().find(|entry| entry.0 == self.target) {
                return (status.1, status.2.clone());
            }
            assert!(
                Instant::now() < deadline,
                "the playing plugin has a helper; known: {all:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn save_while_playing(&mut self, name: &str) -> String {
        let path = self.rig.file(name);
        let save_path = path.clone();
        self.rig.session.transport_play().unwrap();
        let task = self
            .rig
            .session
            .background(move |session| session.project_save(Some(&save_path)));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !task.is_finished() {
            assert!(Instant::now() < deadline, "the save hung");
            self.rig.run(256);
            std::thread::sleep(Duration::from_millis(1));
        }
        task.join().unwrap().unwrap();
        self.rig.session.transport_stop();
        path
    }
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < CLOSE && (a[1] - b[1]).abs() < CLOSE
}

/// The four levels the master can settle at, measured through user choices
/// before anything is exchanged.
struct Levels {
    nothing: [f32; 2],
    mono: [f32; 2],
    stereo: [f32; 2],
    quad: [f32; 2],
}

fn measure_choices(stage: &mut Stage) -> Levels {
    // Without a detector route there is no key: the track's own signal.
    let nothing = stage.level();
    assert!(nothing[0] > 0.01 && nothing[1] > 0.01, "{nothing:?}");
    stage.route(Some(1.0));
    // A plugin that was never told gets the key in its first input.
    assert_eq!(stage.saved_choice(), None);
    let mono = stage.level();
    stage.choose(Some(2));
    let stereo = stage.level();
    stage.choose(Some(3));
    let quad = stage.level();
    stage.choose(Some(1));
    assert!(close(stage.level(), mono), "input 1 is the first input");

    let key = [stereo[0] - nothing[0], stereo[1] - nothing[1]];
    assert!(key[0] > 0.05 && key[1] > 0.01, "{key:?}");
    assert!(
        (key[0] / key[1] - KEY[0] / KEY[1]).abs() < 0.01,
        "the stereo input keeps the key's sides apart: {key:?}"
    );
    let folded = (key[0] + key[1]) * 0.5;
    assert!(
        close(mono, [nothing[0] + folded, nothing[1] - folded]),
        "the mono input gets the fold of both sides: {mono:?} from {nothing:?} and {key:?}"
    );
    assert!(
        close(quad, [nothing[0] - key[0], nothing[1] - key[1]]),
        "the wide input gets the key in its first two channels only: {quad:?}"
    );
    Levels {
        nothing,
        mono,
        stereo,
        quad,
    }
}

fn reopen(stage: &mut Stage, path: &str) {
    stage.rig.session.project_open(path).unwrap();
    stage.rig.run(256);
    stage
        .rig
        .session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
}

fn saved_choice_survives_exchange(vst3: bool, helper: bool) {
    let mut stage = stage(vst3, helper);
    let levels = measure_choices(&mut stage);
    for (other, name) in [
        (levels.nothing, "nothing"),
        (levels.mono, "mono"),
        (levels.quad, "quad"),
    ] {
        assert!(
            !close(levels.stereo, other),
            "the stereo level is unlike the {name} level"
        );
    }

    // The choice that is saved from here on: the second sidechain input.
    stage.choose(Some(2));
    assert!(close(stage.level(), levels.stereo));
    let document = stage.rig.project();

    if helper {
        // The native owner dies and an explicit retry starts another.
        let (token, before) = stage.helper_status();
        assert!(!before.failed && before.process_id != std::process::id());
        let killed = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &before.process_id.to_string()])
            .output()
            .unwrap();
        assert!(killed.status.success());
        stage.rig.session.transport_play().unwrap();
        let mut out = [0.0_f32; 512];
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| stage.rig.processor.process(&mut out)),
                0
            );
            let status = stage.helper_status().1;
            if status.failed && status.reaped {
                break;
            }
            assert!(Instant::now() < deadline, "the dead helper was not reaped");
            std::thread::sleep(Duration::from_millis(8));
        }
        // With no helper the effect passes the track's own signal: the key
        // goes nowhere, least of all into the main signal.
        assert!(close(stage.level(), levels.nothing));
        stage.rig.session.transport_stop();
        stage.rig.session.refresh_plugins().unwrap();
        let (after_token, after) = stage.helper_status();
        assert_ne!(after_token, token);
        assert_ne!(after.process_id, before.process_id);
        assert!(!after.failed && !after.reaped);
    } else {
        // Saving captures native state on the owner thread. For VST3 the
        // owner takes the processor back, deactivates the plugin and
        // prepares a new processor for the audio thread.
        let path = stage.save_while_playing("exchange.windfall");
        let saved = windfall_project::file::load(Path::new(&path)).unwrap();
        assert_eq!(saved.plugin(stage.target).unwrap().sidechain_input, Some(2));
    }
    // Nobody chose again: the document is as it was.
    assert_eq!(stage.rig.project(), document);
    let after = stage.level();
    assert!(
        close(after, levels.stereo),
        "after the exchange the key reaches the second input again: {after:?}, not {:?}",
        levels.stereo
    );
    assert!(
        !close(after, levels.mono),
        "the key did not fall back to the first input"
    );

    // Save and reopen: the choice is in the file and is applied to the
    // processor of the reopened project.
    let path = stage.save_while_playing("reopened.windfall");
    let file = windfall_project::file::load(Path::new(&path)).unwrap();
    assert_eq!(file.plugin(stage.target).unwrap().sidechain_input, Some(2));
    assert_eq!(file.plugin(stage.target).unwrap().auxiliary_inputs.len(), 3);
    let old_token = helper.then(|| stage.helper_status().0);
    reopen(&mut stage, &path);
    assert_eq!(stage.saved_choice(), Some(2));
    if let Some(old_token) = old_token {
        assert_ne!(stage.helper_status().0, old_token);
    }
    let reopened = stage.level();
    assert!(
        close(reopened, levels.stereo),
        "the reopened project feeds the second input: {reopened:?}"
    );

    // A file that names an input the plugin does not have: the key reaches
    // no input, and is not put into the first one or the main signal.
    let mut absent = file.clone();
    absent
        .plugins
        .iter_mut()
        .find(|plugin| plugin.target == stage.target)
        .unwrap()
        .sidechain_input = Some(9);
    let absent_path = stage.rig.file("absent.windfall");
    std::fs::write(
        &absent_path,
        windfall_project::file::to_json(&absent).unwrap(),
    )
    .unwrap();
    reopen(&mut stage, &absent_path);
    assert_eq!(stage.saved_choice(), Some(9));
    let silent = stage.level();
    assert!(
        close(silent, levels.nothing),
        "an absent saved input gets no key: {silent:?}, not {:?}",
        levels.nothing
    );
    // The plugin is still playing: a real input can be chosen again.
    stage.choose(Some(3));
    assert!(close(stage.level(), levels.quad));
    if helper {
        assert!(!stage.helper_status().1.failed);
    }
}

#[test]
fn saved_clap_sidechain_input_survives_helper_death_retry_and_reopen() {
    saved_choice_survives_exchange(false, true);
}

#[test]
fn saved_vst3_sidechain_input_survives_helper_death_retry_and_reopen() {
    saved_choice_survives_exchange(true, true);
}

#[test]
fn saved_vst3_sidechain_input_survives_in_process_owner_exchange_and_reopen() {
    saved_choice_survives_exchange(true, false);
}

#[test]
fn saved_clap_sidechain_input_survives_in_process_save_and_reopen() {
    saved_choice_survives_exchange(false, false);
}
