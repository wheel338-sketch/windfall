//! Actual CLAP fixtures through the desktop owner, session and export path.
use super::{Rig, SAMPLE_RATE, rms};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
use windfall_engine::plugins::{PluginFactory, PluginTransport};
use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, PlayMode};
use windfall_project::{Command, EffectId, PluginTarget, Project, TrackId};

fn library() -> &'static Path {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        if let Some(source) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") {
            let source = PathBuf::from(source);
            assert!(source.is_file(), "explicit native fixture must be a file");
            let source = source.canonicalize().expect("canonical native fixture");
            let helper = PathBuf::from(
                std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
                    .expect("explicit fixture requires the current desktop helper"),
            );
            assert!(helper.is_file(), "explicit desktop helper must be a file");
            assert_eq!(
                helper.canonicalize().expect("canonical desktop helper"),
                root.join("target/debug/windfall-desktop.exe")
                    .canonicalize()
                    .expect("build the current desktop helper explicitly"),
                "the fixture runner must use the actual desktop scanner/audio entry"
            );
            return source;
        }
        let target = root.join("target/plugin-session-fixtures");
        let output = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(root.join("crates/windfall-plugin-host/test-plugins/Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "windfall-desktop",
                "--bin",
                "windfall-desktop",
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("debug/windfall_test_plugins.dll")
    })
}
pub(super) fn manager(rig: &Rig) -> (std::sync::Arc<crate::plugins::PluginManager>, String) {
    let folder = rig.folder.path().join("plugins");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("windfall-test.clap");
    std::fs::copy(library(), &path).unwrap();
    let scanner = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
            PathBuf::from,
        )
        .join("debug/windfall-desktop.exe");
    assert!(scanner.is_file(), "desktop scanner helper must be built");
    let manager = crate::plugins::PluginManager::fixture(rig.folder.path(), &path, &scanner);
    rig.session.install_plugins(manager.clone());
    (manager, path.to_string_lossy().into_owned())
}
fn transport() -> PluginTransport {
    PluginTransport {
        playing: false,
        tempo_bpm: 120.0,
        position_beats: 0.0,
        position_seconds: 0.0,
        numerator: 4,
        denominator: 4,
        meter_anchor: None,
    }
}

#[test]
fn piano_expression_native_helper_sequences_slide_portamento_and_midi_only_bends() {
    use windfall_dsp::{NoteArticulation, NoteExpression};
    use windfall_project::NoteInit;
    for id in ["org.windfall.test.sine", "org.windfall.test.midi-sine"] {
        for articulation in [NoteArticulation::Slide, NoteArticulation::Portamento] {
            let mut rig = Rig::new();
            let (manager, path) = manager(&rig); // production desktop scanner helper
            let binding = manager
                .binding(
                    &path,
                    id,
                    PluginTarget::Instrument {
                        channel: windfall_project::ChannelId(0),
                    },
                )
                .unwrap();
            let added = rig
                .session
                .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
                .unwrap();
            let channel = windfall_project::ChannelId(added.created[0]);
            rig.session
                .dispatch(
                    Command::AddNotes {
                        pattern: rig.pattern(),
                        channel,
                        notes: vec![
                            NoteInit {
                                start: 0,
                                length: 1920,
                                key: 69,
                                velocity: Some(1.0),
                                pan: None,
                                expression: Some(NoteExpression {
                                    color_group: Some(7),
                                    ..NoteExpression::NEUTRAL
                                }),
                            },
                            NoteInit {
                                start: 960,
                                length: 1920,
                                key: 71,
                                velocity: Some(1.0),
                                pan: None,
                                expression: Some(NoteExpression {
                                    articulation,
                                    glide_ticks: 240,
                                    color_group: Some(7),
                                    ..NoteExpression::NEUTRAL
                                }),
                            },
                        ],
                    },
                    None,
                )
                .unwrap();
            rig.session
                .transport_set(windfall_ipc::TransportPatch {
                    mode: Some(PlayMode::Pattern),
                    pattern: Some(rig.pattern()),
                    ..Default::default()
                })
                .unwrap();
            rig.session.transport_play().unwrap();
            let pcm = rig.run(45_000);
            let frequency = |start: usize, end: usize| {
                let mono: Vec<_> = pcm[start * 2..end * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|frame| frame[0])
                    .collect();
                let crossings: Vec<_> = mono
                    .windows(2)
                    .enumerate()
                    .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
                    .map(|(index, _)| index)
                    .collect();
                assert!(
                    crossings.len() > 3,
                    "native fixture produced no tone: {id:?} {articulation:?}"
                );
                f64::from(SAMPLE_RATE) * (crossings.len() - 1) as f64
                    / (crossings[crossings.len() - 1] - crossings[0]) as f64
            };
            assert!((frequency(8000, 18000) - 440.0).abs() < 1.0);
            assert!((frequency(33000, 43000) - 493.883).abs() < 1.0);
            let saved = windfall_project::file::to_json(&rig.project()).unwrap();
            assert_eq!(
                windfall_project::file::from_json(&saved).unwrap(),
                rig.project()
            );
            rig.session.transport_stop();
            assert!(rms(&rig.run(4096)) < 0.01);
        }
    }
}

#[test]
fn r4_deduplicated_clap_instrument_document_adoption() {
    r4_document_adoption(false, true);
}
#[test]
fn r4_deduplicated_vst3_instrument_document_adoption() {
    r4_document_adoption(true, true);
}
#[test]
fn r4_deduplicated_clap_effect_document_adoption() {
    r4_document_adoption(false, false);
}
#[test]
fn r4_deduplicated_vst3_effect_document_adoption() {
    r4_document_adoption(true, false);
}

fn r4_document_adoption(vst3: bool, instrument: bool) {
    use windfall_project::{
        AutomationId, AutomationPoint, AutomationTarget, ClipContent, ClipInit,
    };
    let mut rig = Rig::new();
    let (manager, binding, id) = if vst3 {
        let (runtime, binding) = crate::plugins::vst3_fixture(if instrument { 2 } else { 0 });
        let manager = crate::plugins::PluginManager::fixture_runtime(rig.folder.path(), runtime);
        rig.session.install_plugins(manager.clone());
        (manager, binding, 7)
    } else {
        let (manager, path) = manager(&rig);
        let binding = manager
            .binding(
                &path,
                if instrument {
                    "org.windfall.test.sine"
                } else {
                    "org.windfall.test.gain"
                },
                if instrument {
                    PluginTarget::Instrument {
                        channel: windfall_project::ChannelId(0),
                    }
                } else {
                    PluginTarget::Effect {
                        effect: EffectId(0),
                    }
                },
            )
            .unwrap();
        (manager, binding, if instrument { 1 } else { 7 })
    };
    let added = rig
        .session
        .dispatch(
            if instrument {
                Command::AddPluginInstrument { plugin: binding }
            } else {
                Command::AddPluginEffect {
                    track: TrackId(0),
                    plugin: binding,
                }
            },
            None,
        )
        .unwrap();
    let channel = if instrument {
        windfall_project::ChannelId(added.created[0])
    } else {
        windfall_project::ChannelId(
            rig.session
                .dispatch(
                    Command::AddChannel {
                        name: Some("Native effect source".into()),
                        sample: None,
                        instrument: Some(windfall_project::InstrumentKind::SubtractiveSynth),
                        index: None,
                        mixer_track: None,
                    },
                    None,
                )
                .unwrap()
                .created[0],
        )
    };
    let target = if instrument {
        PluginTarget::Instrument { channel }
    } else {
        PluginTarget::Effect {
            effect: EffectId(*added.created.last().unwrap()),
        }
    };
    let automation_target = if instrument {
        AutomationTarget::InstrumentParam { channel, param: 0 }
    } else {
        AutomationTarget::EffectParam {
            track: TrackId(0),
            effect: EffectId(*added.created.last().unwrap()),
            param: 0,
        }
    };
    let range = rig.project().automation_range(&automation_target).unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.5,
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel,
                step: 0,
            },
            None,
        )
        .unwrap();
    let track = rig
        .session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    let automation = rig
        .session
        .dispatch(
            Command::AddAutomation {
                name: None,
                target: automation_target,
                points: Some(vec![
                    AutomationPoint {
                        tick: 0,
                        value: range.normalized(0.75),
                        curve: 0.0,
                        hold: true,
                    },
                    AutomationPoint {
                        tick: 96,
                        value: range.normalized(0.25),
                        curve: 0.0,
                        hold: true,
                    },
                ]),
            },
            None,
        )
        .unwrap()
        .created[0];
    let track = windfall_project::PlaylistTrackId(track);
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Pattern {
                            pattern: rig.pattern(),
                        },
                    },
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation {
                            automation: AutomationId(automation),
                        },
                    },
                ],
            },
            None,
        )
        .unwrap();
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(PlayMode::Song),
            ..Default::default()
        })
        .unwrap();
    rig.session.transport_play().unwrap();
    let mut out = [0.0; 960];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    assert_eq!(manager.runtime.fixture_parameter(target, id), Some(0.75));
    let token = manager.runtime.selected_token(target).unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    // The installed value equals the automation cache: apply_plugin must
    // adopt its document generation even though set_param is deduplicated.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
        0
    );
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            for _ in 0..10 {
                rig.processor.process(&mut out);
            }
        }),
        0
    );
    assert_eq!(manager.runtime.fixture_parameter(target, id), Some(0.25));
    assert!(
        rig.session
            .controller()
            .frame()
            .automated
            .iter()
            .any(|p| p.automation == AutomationId(automation) && p.value == range.normalized(0.25))
    );
    let runtime = manager.runtime.clone();
    let project = rig.project();
    let task = std::thread::spawn(move || runtime.capture(project));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
            0
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let captured = task.join().unwrap().unwrap();
    let value = |p: &Project| {
        p.plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .value
    };
    assert_eq!(
        value(&captured),
        0.25,
        "{vst3}: deduplicated document suppressed newer automation"
    );
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    let save_path = rig.file("deduplicated.windfall");
    let path = save_path.clone();
    let task = rig
        .session
        .background(move |session| session.project_save(Some(&path)));
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
            0
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    task.join().unwrap().unwrap();
    assert_eq!(
        value(&windfall_project::file::load(Path::new(&save_path)).unwrap()),
        0.25
    );
    let options = ExportOptions {
        path: rig.file("deduplicated.wav"),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        tail_secs: 0.0,
        ..Default::default()
    };
    let export = options.clone();
    let task = rig
        .session
        .background(move |session| session.export_audio(export));
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
            0
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    task.join().unwrap().unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    let exported = windfall_codec::decode_file(&options.path).unwrap();
    rig.session.project_open(&save_path).unwrap();
    rig.run(256);
    assert_eq!(value(&rig.project()), 0.25);
    assert_eq!(manager.runtime.fixture_parameter(target, id), Some(0.25));
    rig.events.take();
    let reference = ExportOptions {
        path: rig.file("reopened.wav"),
        ..options
    };
    let export = reference.clone();
    let task = rig
        .session
        .background(move |session| session.export_audio(export));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
            0
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    task.join().unwrap().unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert_eq!(
        exported.samples(),
        windfall_codec::decode_file(&reference.path)
            .unwrap()
            .samples()
    );
}

#[test]
fn saturated_clap_parameter_edit_retries_without_replacing_playback_or_allocating() {
    let mut rig = Rig::new();
    let (manager, path) = manager(&rig);
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.sine",
            PluginTarget::Instrument {
                channel: windfall_project::ChannelId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = windfall_project::ChannelId(added.created[0]);
    let target = PluginTarget::Instrument { channel };
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 1,
                value: 0.5,
            },
            None,
        )
        .unwrap();
    rig.run(256);
    let token = manager.runtime.selected_token(target).unwrap();
    let epoch = rig.session.controller().hardware_epoch();
    for _ in 0..1024 {
        assert!(
            rig.session
                .controller()
                .hardware_note(epoch, channel, 64, 100)
        );
    }
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
        0
    );
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 1,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
        0
    );
    let mut out = [0.0; 256];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            for _ in 0..8 {
                rig.processor.process(&mut out);
            }
        }),
        0
    );
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    assert_eq!(
        rig.project()
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == 1)
            .unwrap()
            .value,
        0.75
    );
    let native = manager.runtime.capture(rig.project()).unwrap();
    assert_eq!(
        native
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == 1)
            .unwrap()
            .value,
        0.75,
        "committed cached parameter must eventually reach native after saturation drains"
    );
    assert_eq!(manager.runtime.selected_token(target), Some(token));
}

#[test]
fn runtime_repair_saturated_controls_survive_capture_save_backup_zip_and_export() {
    for action in [
        "capture",
        "save",
        "backup",
        "zip",
        "export",
        "unobserved-save",
    ] {
        let mut rig = Rig::new();
        let (manager, path) = manager(&rig);
        let binding = manager
            .binding(
                &path,
                "org.windfall.test.sine",
                PluginTarget::Instrument {
                    channel: windfall_project::ChannelId(0),
                },
            )
            .unwrap();
        let added = rig
            .session
            .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
            .unwrap();
        let channel = windfall_project::ChannelId(added.created[0]);
        let target = PluginTarget::Instrument { channel };
        rig.session
            .dispatch(
                Command::ToggleStep {
                    pattern: rig.pattern(),
                    channel,
                    step: 0,
                },
                None,
            )
            .unwrap();
        rig.session
            .dispatch(
                Command::SetPluginParam {
                    target,
                    id: 1,
                    value: 0.5,
                },
                None,
            )
            .unwrap();
        rig.run(256);
        let saved = rig.file("pending.windfall");
        rig.session.project_save(Some(&saved)).unwrap();
        rig.run(256);
        let token = manager.runtime.selected_token(target).unwrap();
        let epoch = rig.session.controller().hardware_epoch();
        for _ in 0..1024 {
            assert!(
                rig.session
                    .controller()
                    .hardware_note(epoch, channel, 64, 100)
            );
        }
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
            0
        );
        rig.session
            .dispatch(
                Command::SetPluginParam {
                    target,
                    id: 1,
                    value: 0.75,
                },
                None,
            )
            .unwrap();
        if action != "unobserved-save" {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
                0
            );
        }
        let value = |project: &Project| {
            project
                .plugin(target)
                .unwrap()
                .parameters
                .iter()
                .find(|param| param.id == 1)
                .unwrap()
                .value
        };
        if action == "export" {
            let options = ExportOptions {
                path: rig.file("pending.wav"),
                format: ExportFormat::Wav,
                bit_depth: BitDepth::Float32,
                sample_rate: SAMPLE_RATE,
                mode: PlayMode::Pattern,
                tail_secs: 0.0,
                ..Default::default()
            };
            rig.session.export_audio(options.clone()).unwrap();
            assert_eq!(rig.events.wait_for_export().error, None);
            let first = windfall_codec::decode_file(&options.path).unwrap();
            assert_eq!(manager.runtime.selected_token(target), Some(token));
            rig.run(2048);
            rig.events.take();
            let mut reference = options;
            reference.path = rig.file("settled.wav");
            rig.session.export_audio(reference.clone()).unwrap();
            assert_eq!(rig.events.wait_for_export().error, None);
            let settled = windfall_codec::decode_file(&reference.path).unwrap();
            assert_eq!(
                first.samples(),
                settled.samples(),
                "export before draining must render committed intent"
            );
            continue;
        }
        let captured = match action {
            "capture" => manager.runtime.capture(rig.project()).unwrap(),
            "save" | "unobserved-save" => {
                rig.session.project_save(Some(&saved)).unwrap();
                windfall_project::file::load(Path::new(&saved)).unwrap()
            }
            "backup" => {
                let path = rig
                    .session
                    .write_backup("2026-10-07_21-00-00")
                    .unwrap()
                    .unwrap();
                windfall_project::file::load(&path).unwrap()
            }
            "zip" => {
                let zip = rig.file("pending.zip");
                rig.session.project_archive_save(&zip).unwrap();
                assert_eq!(manager.runtime.selected_token(target), Some(token));
                rig.session.project_archive_open(&zip).unwrap().project
            }
            _ => unreachable!(),
        };
        assert_eq!(
            value(&captured),
            0.75,
            "{action}: older native state overwrote committed intent"
        );
        if action != "zip" {
            assert_eq!(manager.runtime.selected_token(target), Some(token));
        }
        let reopened = rig.file("reopened.windfall");
        windfall_project::file::save(&captured, Path::new(&reopened)).unwrap();
        rig.session.project_open(&reopened).unwrap();
        rig.run(512);
        assert_eq!(value(&rig.project()), 0.75);
        assert_eq!(
            value(&manager.runtime.capture(rig.project()).unwrap()),
            0.75
        );
    }
}

#[test]
fn runtime_repair_vst3_bundle_binary_changes_require_rescan() {
    use std::io::Write;
    let rig = Rig::new();
    let bundle = rig.folder.path().join("Bundle.vst3");
    let inner = bundle.join("Contents/x86_64-win/Bundle.vst3");
    std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
    std::fs::copy(library(), &inner).unwrap();
    let scanner = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
            PathBuf::from,
        )
        .join("debug/windfall-desktop.exe");
    let manager = crate::plugins::PluginManager::fixture(rig.folder.path(), &bundle, &scanner);
    let entry = manager
        .state()
        .entries
        .into_iter()
        .find(|entry| entry.format == "vst3" && !entry.instrument)
        .unwrap();
    let binding = manager
        .binding(
            &entry.path,
            &entry.id,
            PluginTarget::Effect {
                effect: EffectId(901),
            },
        )
        .unwrap();
    let directory = std::fs::metadata(&bundle).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&inner)
        .unwrap()
        .write_all(b"changed binary overlay")
        .unwrap();
    let after = std::fs::metadata(&bundle).unwrap();
    assert_eq!(directory.len(), after.len());
    assert_eq!(directory.modified().unwrap(), after.modified().unwrap());
    assert!(
        manager
            .runtime
            .discover(binding.clone())
            .unwrap_err()
            .contains("changed"),
        "the loaded binary must match the approved scan"
    );
    let rescanned = crate::plugins::PluginManager::fixture(
        &rig.folder.path().join("rescanned"),
        &bundle,
        &scanner,
    );
    assert!(rescanned.runtime.discover(binding.clone()).is_ok());
    std::fs::remove_file(&inner).unwrap();
    assert!(rescanned.runtime.discover(binding).is_err());
}

#[test]
fn production_vst3_catalog_enables_checked_roles_and_excludes_rejected_layouts() {
    let rig = Rig::new();
    let folder = rig.folder.path().join("vst3-scan");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("windfall-test.vst3");
    std::fs::copy(library(), &path).unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target"),
        PathBuf::from,
    );
    let manager = crate::plugins::PluginManager::fixture(
        rig.folder.path(),
        &path,
        &target.join("debug/windfall-desktop.exe"),
    );
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let descriptors = module.descriptors();
    let state = manager.state();
    for (index, instrument) in [(0, false), (2, true)] {
        let entry = state
            .entries
            .iter()
            .find(|entry| entry.id == descriptors[index].id)
            .unwrap();
        assert!(entry.usable, "{:?}", entry.error);
        assert_eq!(entry.format, "vst3");
        assert_eq!(entry.instrument, instrument);
        assert!(
            manager
                .binding(
                    &entry.path,
                    &entry.id,
                    if instrument {
                        PluginTarget::Instrument {
                            channel: windfall_project::ChannelId(900),
                        }
                    } else {
                        PluginTarget::Effect {
                            effect: EffectId(900),
                        }
                    }
                )
                .is_ok()
        );
    }
    // Layout rejection is omitted from the usable catalog. Only dangerous
    // scanner failures enter its separate blocklist.
    assert!(
        state
            .entries
            .iter()
            .all(|entry| entry.id != descriptors[1].id)
    );
    assert!(
        manager
            .binding(
                &path.to_string_lossy(),
                &descriptors[1].id,
                PluginTarget::Effect {
                    effect: EffectId(900)
                }
            )
            .is_err()
    );
}

#[test]
fn native_vst3_session_save_backup_reopen_editor_routing_and_offline_export() {
    let mut rig = Rig::new();
    let (runtime, binding) = crate::plugins::vst3_fixture(2);
    let manager =
        crate::plugins::PluginManager::fixture_runtime(rig.folder.path(), runtime.clone());
    rig.session.install_plugins(manager);
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = windfall_project::ChannelId(added.created[0]);
    let target = PluginTarget::Instrument { channel };
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel,
                step: 0,
            },
            None,
        )
        .unwrap();
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.01);
    runtime
        .editor_binding(
            target,
            Some(rig.project().plugin(target).unwrap().clone()),
            false,
        )
        .unwrap();
    assert!(runtime.editor(target, true).unwrap_err().contains("editor"));
    let saved = rig.file("vst3.windfall");
    let save_path = saved.clone();
    let task = rig
        .session
        .background(move |session| session.project_save(Some(&save_path)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        rig.run(256);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    task.join().unwrap().unwrap();
    let captured = windfall_project::file::load(Path::new(&saved))
        .unwrap()
        .plugins;
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 7,
                value: 0.6,
            },
            None,
        )
        .unwrap();
    rig.run(256);
    let task = rig
        .session
        .background(|session| session.write_backup("2026-10-07_12-30-00"));
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        rig.run(256);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(task.join().unwrap().unwrap().is_some());
    rig.session.project_open(&saved).unwrap();
    rig.run(256);
    assert_eq!(rig.project().plugins, captured);
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.01);
    let options = ExportOptions {
        path: rig.file("vst3.wav"),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        tail_secs: 0.0,
        ..Default::default()
    };
    let export_options = options.clone();
    let task = rig
        .session
        .background(move |session| session.export_audio(export_options));
    while !task.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        rig.run(256);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    task.join().unwrap().unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert!(
        rms(windfall_codec::decode_file(&options.path)
            .unwrap()
            .samples())
            > 0.01
    );
    assert_eq!(rig.project().plugins, captured);
}

#[test]
fn native_session_instrument_effect_export_and_state_round_trip() {
    let mut rig = Rig::new();
    let (manager, path) = manager(&rig);
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.sine",
            PluginTarget::Instrument {
                channel: windfall_project::ChannelId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = windfall_project::ChannelId(added.created[0]);
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target: PluginTarget::Instrument { channel },
                id: 1,
                value: 0.7,
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel,
                step: 0,
            },
            None,
        )
        .unwrap();
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.gain",
            PluginTarget::Effect {
                effect: EffectId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(
            Command::AddPluginEffect {
                track: TrackId(0),
                plugin: binding,
            },
            None,
        )
        .unwrap();
    let effect = EffectId(*added.created.last().unwrap());
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target: PluginTarget::Effect { effect },
                id: 7,
                value: 0.5,
            },
            None,
        )
        .unwrap();
    rig.run(256); // Stopped buffers select installed playback instances.
    assert_eq!(rig.session.controller().latency_frames(), 32);
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.05);
    let captured = manager.runtime.capture(rig.project()).unwrap();
    assert!(
        captured
            .plugins
            .iter()
            .all(|plugin| plugin.state.starts_with(b"WFPS"))
    );
    let options = ExportOptions {
        path: rig.file("plugins.wav"),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        tail_secs: 0.0,
        ..Default::default()
    };
    rig.session.export_audio(options.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert!(
        rms(windfall_codec::decode_file(&options.path)
            .unwrap()
            .samples())
            > 0.01
    );
    assert_eq!(
        manager.runtime.capture(rig.project()).unwrap().plugins,
        captured.plugins,
        "export instances cannot replace playback state"
    );
    let saved = rig.file("plugins.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session.project_open(&saved).unwrap();
    rig.run(256);
    let reopened = rig.project();
    assert_eq!(reopened.plugins, captured.plugins);
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.05);

    let mut missing = reopened;
    let retained = missing
        .plugins
        .iter_mut()
        .find(|plugin| plugin.target == PluginTarget::Instrument { channel })
        .unwrap();
    retained.path = rig.file("missing.clap");
    let retained = retained.clone();
    let missing_file = rig.file("missing.windfall");
    windfall_project::file::save(&missing, Path::new(&missing_file)).unwrap();
    rig.session.project_open(&missing_file).unwrap();
    rig.session.transport_play().unwrap();
    let audio = rig.run(24_000);
    assert!(
        audio[audio.len() - 512..]
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(
        rig.project().plugin(PluginTarget::Instrument { channel }),
        Some(&retained)
    );
    assert!(
        manager
            .state()
            .instances
            .iter()
            .any(|instance| instance.target == retained.target)
    );
}

#[test]
fn render_and_speculative_instances_cannot_take_editor_or_state_ownership() {
    let rig = Rig::new();
    let (manager, path) = manager(&rig);
    let target = PluginTarget::Effect {
        effect: EffectId(900),
    };
    let mut binding = manager
        .binding(&path, "org.windfall.test.gain", target)
        .unwrap();
    binding
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.5;
    let mut first = manager.runtime.effect(&binding, SAMPLE_RATE, 256).unwrap();
    first.transport(transport());
    first.process(&mut [1.0; 64], &mut [1.0; 64]);
    let mut project = Project::new("Ownership");
    project.plugins = vec![binding.clone()];
    let first_state = manager.runtime.capture(project.clone()).unwrap().plugins[0]
        .state
        .clone();
    assert_ne!(
        first_state, binding.state,
        "state capture must contain the processed native parameter change, not the discovery preset"
    );
    let mut changed = binding.clone();
    changed
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.9;
    let mut speculative = manager.runtime.effect(&changed, SAMPLE_RATE, 256).unwrap();
    speculative.process(&mut [1.0; 64], &mut [1.0; 64]);
    let staged = manager.runtime.prepare_document();
    let mut rejected = staged.effect(&changed, SAMPLE_RATE, 256).unwrap();
    rejected.process(&mut [1.0; 64], &mut [1.0; 64]);
    assert!(
        manager
            .runtime
            .editor_binding(target, Some(binding.clone()), false)
            .is_ok(),
        "a refused staged replacement must leave the installed editor usable"
    );
    drop(rejected);
    drop(staged);
    assert_eq!(
        manager.runtime.capture(project.clone()).unwrap().plugins[0].state,
        first_state
    );
    let render = manager.runtime.render_factory().unwrap();
    let mut export = render.effect(&changed, SAMPLE_RATE, 256).unwrap();
    export.transport(transport());
    export.process(&mut [1.0; 64], &mut [1.0; 64]);
    assert_eq!(
        manager.runtime.capture(project.clone()).unwrap().plugins[0].state,
        first_state
    );
    manager.runtime.retry(); // A replacement document invalidates previous tokens.
    assert_eq!(
        manager.runtime.capture(project.clone()).unwrap().plugins[0].state,
        binding.state
    );
    assert!(manager.runtime.editor(target, true).is_err());
    let second_path = rig.folder.path().join("plugins/second.clap");
    std::fs::copy(library(), &second_path).unwrap();
    changed.path = second_path.to_string_lossy().into_owned();
    manager.runtime.approve(&[windfall_ipc::PluginEntry {
        path: changed.path.clone(),
        id: changed.id.clone(),
        format: "clap".into(),
        name: changed.name.clone(),
        vendor: "Fixture".into(),
        instrument: false,
        usable: true,
        error: None,
    }]);
    let mut replacement = manager.runtime.effect(&changed, SAMPLE_RATE, 256).unwrap();
    replacement.transport(transport());
    replacement.process(&mut [1.0; 64], &mut [1.0; 64]);
    assert_eq!(
        manager.runtime.capture(project).unwrap().plugins[0].state,
        binding.state,
        "equal numeric targets with different bindings cannot capture each other's state"
    );
    assert!(
        manager
            .runtime
            .editor_binding(target, Some(binding), false)
            .is_err()
    );
    let mut missing = changed.clone();
    missing.path = rig.file("missing.clap");
    let mut unavailable = manager.runtime.effect(&missing, SAMPLE_RATE, 256).unwrap();
    assert!(
        manager
            .runtime
            .editor_binding(target, Some(changed.clone()), false)
            .is_ok(),
        "speculative failure cannot deselect an installed instance"
    );
    unavailable.transport(transport());
    let mut left = [0.25; 64];
    let mut right = left;
    unavailable.process(&mut left, &mut right);
    assert_eq!(left, [0.25; 64], "missing effects bypass their input");
    assert!(
        manager
            .runtime
            .editor_binding(target, Some(changed.clone()), false)
            .is_err(),
        "an installed missing instance clears stale editor ownership"
    );
    let staged = manager.runtime.prepare_document();
    let mut accepted = staged.effect(&changed, SAMPLE_RATE, 256).unwrap();
    accepted.transport(transport());
    staged.install_document();
    assert!(
        manager
            .runtime
            .editor_binding(target, Some(changed), false)
            .is_ok(),
        "an accepted staged replacement publishes its own revision"
    );
}

// N4 uses explicit helper/fixture injection; historical in-process R4 tests above
// remain unchanged. This exercises the actual current desktop helper entry.
fn n4_manager(rig: &Rig, vst3: bool) -> (std::sync::Arc<crate::plugins::PluginManager>, String) {
    let library = PathBuf::from(
        std::env::var_os("WINDFALL_BRIDGE_FIXTURE")
            .expect("build the owned bridge fixture explicitly"),
    );
    let helper = PathBuf::from(
        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
            .expect("build the current desktop helper explicitly"),
    );
    assert!(library.is_file() && helper.is_file());
    let folder = rig.folder.path().join("n4-plugins");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(if vst3 { "bridge.vst3" } else { "bridge.clap" });
    std::fs::copy(library, &path).unwrap();
    let manager = crate::plugins::PluginManager::bridge_fixture(rig.folder.path(), &path, &helper);
    rig.session.install_plugins(manager.clone());
    (manager, path.to_string_lossy().into_owned())
}
fn n4_setup(
    vst3: bool,
    instrument: bool,
) -> (
    Rig,
    std::sync::Arc<crate::plugins::PluginManager>,
    PluginTarget,
) {
    let mut rig = Rig::new();
    let (manager, path) = n4_manager(&rig, vst3);
    let id = if vst3 {
        manager
            .state()
            .entries
            .into_iter()
            .find(|entry| {
                entry.name
                    == if instrument {
                        "VST3 Test Instrument"
                    } else {
                        "VST3 Bridge Delayed Effect"
                    }
            })
            .unwrap()
            .id
    } else if instrument {
        "org.windfall.test.sine".into()
    } else {
        "org.windfall.test.bridge-delayed".into()
    };
    let binding = manager
        .binding(
            &path,
            &id,
            if instrument {
                PluginTarget::Instrument {
                    channel: windfall_project::ChannelId(0),
                }
            } else {
                PluginTarget::Effect {
                    effect: EffectId(0),
                }
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(
            if instrument {
                Command::AddPluginInstrument { plugin: binding }
            } else {
                Command::AddPluginEffect {
                    track: TrackId(0),
                    plugin: binding,
                }
            },
            None,
        )
        .unwrap();
    let channel = if instrument {
        windfall_project::ChannelId(added.created[0])
    } else {
        windfall_project::ChannelId(
            rig.session
                .dispatch(
                    Command::AddChannel {
                        name: Some("N4 source".into()),
                        sample: None,
                        instrument: Some(windfall_project::InstrumentKind::SubtractiveSynth),
                        index: None,
                        mixer_track: None,
                    },
                    None,
                )
                .unwrap()
                .created[0],
        )
    };
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel,
                step: 0,
            },
            None,
        )
        .unwrap();
    let target = if instrument {
        PluginTarget::Instrument { channel }
    } else {
        PluginTarget::Effect {
            effect: EffectId(*added.created.last().unwrap()),
        }
    };
    rig.run(256);
    (rig, manager, target)
}
fn n4_role(vst3: bool, instrument: bool) {
    let (mut rig, manager, target) = n4_setup(vst3, instrument);
    let status = manager.runtime.bridge_status();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].0, target);
    let token = status[0].1;
    assert_ne!(status[0].2.process_id, std::process::id());
    assert!(!status[0].2.failed && !status[0].2.reaped);
    assert!(
        manager
            .runtime
            .is_current(manager.runtime.document_revision(), token)
    );
    if !instrument {
        assert_eq!(
            rig.session.controller().latency_frames(),
            561,
            "source instrument12 plus actual native37 plus fixed2B256"
        );
    }
    assert!(
        manager
            .runtime
            .editor_binding(
                target,
                Some(rig.project().plugin(target).unwrap().clone()),
                true
            )
            .unwrap_err()
            .contains("no editor")
    );
    rig.session.transport_play().unwrap();
    let mut heard = Vec::new();
    let mut out = [0.0; 960];
    let mut callback_max = std::time::Duration::ZERO;
    let mut callback_total = std::time::Duration::ZERO;
    for _ in 0..25 {
        let start = std::time::Instant::now();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
        let elapsed = start.elapsed();
        callback_max = callback_max.max(elapsed);
        callback_total += elapsed;
        heard.extend_from_slice(&out);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    eprintln!(
        "N4 {vst3:?}/{instrument:?}: 25 callbacks, zero allocator calls, wall max={callback_max:?} mean={:?}",
        callback_total / 25
    );
    assert!(rms(&heard) > 0.001);
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    let project = rig.project();
    let captured = manager.runtime.capture(project.clone()).unwrap();
    assert!(captured.plugin(target).unwrap().state.starts_with(b"WFPS"));
    assert_eq!(
        rig.project(),
        project,
        "capture cannot mutate document/history"
    );
    let render = manager.runtime.render_factory().unwrap();
    if instrument {
        let mut audio = render
            .instrument(project.plugin(target).unwrap(), SAMPLE_RATE, 512)
            .unwrap();
        audio.transport(transport());
        audio.note_on(60, 0.75);
        let mut left = [0.0; 1024];
        let mut right = left;
        audio.process(&mut left, &mut right);
        assert!(left.iter().any(|sample| sample.abs() > 0.001));
    } else {
        let mut audio = render
            .effect(project.plugin(target).unwrap(), SAMPLE_RATE, 512)
            .unwrap();
        assert_eq!(
            audio.latency(),
            549,
            "native37 plus fixed2B256 at the facade"
        );
        audio.transport(transport());
        let mut left = [0.5; 1024];
        let mut right = left;
        audio.process(&mut left, &mut right);
        assert_eq!(left.iter().position(|sample| *sample != 0.0), Some(549));
        // CLAP starts at unity gain; VST3 starts at gain0.5. The documented
        // five-millisecond dry/wet transition completes after 240 frames.
        let reference = if vst3 { 0.25 } else { 0.5 };
        assert!(
            left[789..]
                .iter()
                .all(|sample| (*sample - reference).abs() < 1e-6)
        );
    }
    assert_eq!(render.render_error(), None);
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    assert_eq!(manager.runtime.bridge_status()[0].1, token);
    assert!(
        manager
            .runtime
            .capture_at(project, manager.runtime.document_revision().wrapping_add(1))
            .unwrap()
            .plugins
            == rig.project().plugins,
        "stale revision may not capture current helper"
    );
}
#[test]
fn n4_production_clap_effect_session_routes_through_current_desktop_helper() {
    n4_role(false, false);
}
#[test]
fn n4_production_vst3_effect_session_routes_through_current_desktop_helper() {
    n4_role(true, false);
}
#[test]
fn n4_production_clap_instrument_session_routes_through_current_desktop_helper() {
    n4_role(false, true);
}
#[test]
fn n4_production_vst3_instrument_session_routes_through_current_desktop_helper() {
    n4_role(true, true);
}

#[test]
fn n4_native_deactivation_edit_supersedes_unprocessed_document_intent() {
    let rig = Rig::new();
    let (manager, path) = n4_manager(&rig, true);
    let id = manager
        .state()
        .entries
        .into_iter()
        .find(|entry| entry.name == "VST3 Bridge Deactivation Edit")
        .unwrap()
        .id;
    let target = PluginTarget::Effect {
        effect: EffectId(990),
    };
    let mut binding = manager.binding(&path, &id, target).unwrap();
    binding
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.625;
    let mut audio = manager.runtime.effect(&binding, SAMPLE_RATE, 512).unwrap();
    audio.transport(transport()); // Installed identity, with no DSP acknowledgement.
    let token = manager.runtime.selected_token(target).unwrap();
    let mut project = Project::new("deactivation edit");
    project.plugins = vec![binding];
    let captured = manager.runtime.capture(project.clone()).unwrap();
    assert_eq!(
        captured
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == 7)
            .unwrap()
            .value,
        0.375,
        "newer native deactivation edit must win over retained 0.625 intent"
    );
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    assert!(!manager.runtime.bridge_status()[0].2.failed);
    let pid = manager.runtime.bridge_status()[0].2.process_id;
    let mut left = [0.5; 256];
    let mut right = left;
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        left.fill(0.5);
        right.fill(0.5);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| audio.process(&mut left, &mut right)),
            0
        );
    }
    assert!(left.iter().all(|value| (*value - 0.1875).abs() < 1e-6));
    let again = manager.runtime.capture(project.clone()).unwrap();
    assert_eq!(
        again
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == 7)
            .unwrap()
            .value,
        0.375
    );
    assert_eq!(manager.runtime.bridge_status()[0].2.process_id, pid);
    assert_eq!(
        project
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == 7)
            .unwrap()
            .value,
        0.625
    );
}

fn n4_document_adoption(vst3: bool, instrument: bool) {
    use windfall_project::{
        AutomationId, AutomationPoint, AutomationTarget, ClipContent, ClipInit,
    };
    let (mut rig, manager, target) = n4_setup(vst3, instrument);
    let id = if instrument && !vst3 { 1 } else { 7 };
    let channel = if let PluginTarget::Instrument { channel } = target {
        channel
    } else {
        rig.project().channels.last().unwrap().id
    };
    let automation_target = match target {
        PluginTarget::Instrument { channel } => {
            AutomationTarget::InstrumentParam { channel, param: 0 }
        }
        PluginTarget::Effect { effect } => AutomationTarget::EffectParam {
            track: TrackId::MASTER,
            effect,
            param: 0,
        },
    };
    let range = rig.project().automation_range(&automation_target).unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.5,
            },
            None,
        )
        .unwrap();
    let track = windfall_project::PlaylistTrackId(
        rig.session
            .dispatch(
                Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let automation = AutomationId(
        rig.session
            .dispatch(
                Command::AddAutomation {
                    name: None,
                    target: automation_target,
                    points: Some(vec![
                        AutomationPoint {
                            tick: 0,
                            value: range.normalized(0.75),
                            curve: 0.0,
                            hold: true,
                        },
                        AutomationPoint {
                            tick: 96,
                            value: range.normalized(0.25),
                            curve: 0.0,
                            hold: true,
                        },
                    ]),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Pattern {
                            pattern: rig.pattern(),
                        },
                    },
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(960),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation { automation },
                    },
                ],
            },
            None,
        )
        .unwrap();
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(PlayMode::Song),
            ..Default::default()
        })
        .unwrap();
    rig.session.transport_play().unwrap();
    let mut out = [0.0; 960];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    let token = manager.runtime.selected_token(target).unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    // The automation cache already holds 0.75, so this applies only the local
    // committed document metadata. It cannot manufacture helper DSP proof.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
        0
    );
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
    }
    let value = |project: &Project| {
        project
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == id)
            .unwrap()
            .value
    };
    let document = rig.project();
    assert_eq!(value(&document), 0.75);
    assert!(
        rig.session
            .controller()
            .frame()
            .automated
            .iter()
            .any(|point| point.automation == automation && point.value == range.normalized(0.25))
    );
    assert_eq!(
        value(&manager.runtime.capture(document.clone()).unwrap()),
        0.25,
        "matching adoption must preserve later actual helper automation, {vst3}/{instrument}/{channel:?}"
    );
    assert_eq!(rig.project(), document);
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    let file = rig.file("bridge-adoption.windfall");
    rig.session.project_save(Some(&file)).unwrap();
    assert_eq!(
        value(&windfall_project::file::load(Path::new(&file)).unwrap()),
        0.25
    );
}
#[test]
fn n4_deduplicated_clap_effect_document_adoption_preserves_helper_automation() {
    n4_document_adoption(false, false);
}
#[test]
fn n4_deduplicated_vst3_effect_document_adoption_preserves_helper_automation() {
    n4_document_adoption(true, false);
}
#[test]
fn n4_deduplicated_clap_instrument_document_adoption_preserves_helper_automation() {
    n4_document_adoption(false, true);
}
#[test]
fn n4_deduplicated_vst3_instrument_document_adoption_preserves_helper_automation() {
    n4_document_adoption(true, true);
}

fn n4_native_final_block_export_failure(vst3: bool) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use windfall_project::{
        AutomationId, AutomationPoint, AutomationTarget, ClipContent, ClipInit,
    };
    let (mut rig, manager, sibling) = n4_setup(vst3, true);
    let sibling_token = manager.runtime.selected_token(sibling).unwrap();
    let sibling_pid = manager.runtime.bridge_status()[0].2.process_id;
    let path = rig.project().plugin(sibling).unwrap().path.clone();
    let id = if vst3 {
        manager
            .state()
            .entries
            .into_iter()
            .find(|entry| entry.name == "VST3 Bridge Controlled Process Error")
            .unwrap()
            .id
    } else {
        "org.windfall.test.bridge-controlled-process-error".into()
    };
    let mut binding = manager
        .binding(
            &path,
            &id,
            PluginTarget::Effect {
                effect: EffectId(0),
            },
        )
        .unwrap();
    binding
        .parameters
        .iter_mut()
        .find(|param| param.id == 7)
        .unwrap()
        .value = 0.5;
    let effect = EffectId(
        *rig.session
            .dispatch(
                Command::AddPluginEffect {
                    track: TrackId::MASTER,
                    plugin: binding,
                },
                None,
            )
            .unwrap()
            .created
            .last()
            .unwrap(),
    );
    let target = PluginTarget::Effect { effect };
    let automation_target = AutomationTarget::EffectParam {
        track: TrackId::MASTER,
        effect,
        param: 0,
    };
    let range = rig.project().automation_range(&automation_target).unwrap();
    let track = windfall_project::PlaylistTrackId(
        rig.session
            .dispatch(
                Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let automation = AutomationId(
        rig.session
            .dispatch(
                Command::AddAutomation {
                    name: None,
                    target: automation_target,
                    points: Some(vec![
                        AutomationPoint {
                            tick: 0,
                            value: range.normalized(0.5),
                            curve: 0.0,
                            hold: true,
                        },
                        AutomationPoint {
                            tick: 95,
                            value: range.normalized(0.75),
                            curve: 0.0,
                            hold: true,
                        },
                    ]),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(96),
                        offset: None,
                        muted: None,
                        content: ClipContent::Pattern {
                            pattern: rig.pattern(),
                        },
                    },
                    ClipInit {
                        track,
                        start: 0,
                        length: Some(96),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation { automation },
                    },
                ],
            },
            None,
        )
        .unwrap();
    rig.session.transport_play().unwrap(); // Live Pattern has no Song automation.
    let mut out = [0.0; 1024];
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
    }
    assert!(rms(&out) > 0.001);
    let document = rig.project();
    let pool = rig.session.state().pool.clone();
    let options = windfall_engine::RenderOptions {
        mode: PlayMode::Song,
        sample_rate: SAMPLE_RATE,
        block_frames: 1024,
        ..Default::default()
    };
    let mut prefix = Vec::new();
    let result = windfall_engine::render_streaming_checked(
        &document,
        &pool,
        &options,
        &mut |block| {
            prefix.extend_from_slice(block);
            true
        },
        &mut |_| true,
    );
    assert!(
        matches!(result, Err(windfall_engine::RenderError::Plugin(_))),
        "{result:?}"
    );
    assert!(
        prefix.len() > 1000 && rms(&prefix) > 0.001,
        "healthy native prefix before final processing block"
    );
    let failed = windfall_engine::render_reporting(&document, &pool, &options, &mut |_| true);
    assert!(failed.plugin_error.is_some() && failed.sampler_error.is_none());
    assert_eq!(failed.audio.frames(), 0);
    let track = document
        .mixer
        .tracks
        .iter()
        .find(|track| track.id != TrackId::MASTER)
        .unwrap()
        .id;
    let mut stem_project = document.clone();
    let PluginTarget::Instrument { channel } = sibling else {
        unreachable!()
    };
    stem_project
        .channels
        .iter_mut()
        .find(|entry| entry.id == channel)
        .unwrap()
        .mixer_track = track;
    for mode in [
        windfall_engine::StemMode::TrackOutputs,
        windfall_engine::StemMode::ToMaster,
    ] {
        let stem_options = windfall_engine::StemOptions {
            mode,
            tracks: Some(vec![track]),
            include_mix: true,
            numbered: false,
        };
        let mut prefix_frames = 0;
        let mut prefix_level = 0.0_f32;
        let result = windfall_engine::render_stems(
            &stem_project,
            &pool,
            &options,
            &stem_options,
            &mut |_, block| {
                prefix_frames += block.len() / 2;
                prefix_level = prefix_level.max(rms(block));
                true
            },
            &mut |_| true,
        );
        assert!(matches!(result, Err(windfall_engine::StemError::Plugin(_))));
        assert!(
            prefix_level > 0.001,
            "{mode:?}: healthy audible prefix before refusal"
        );
        if mode == windfall_engine::StemMode::ToMaster {
            // Two 1024-frame blocks precede failure; the mix strips the sum
            // of instrument/facade PDC: CLAP1093, VST1078 reported frames.
            assert_eq!(prefix_frames, if vst3 { 970 } else { 955 });
        } else {
            assert!(
                prefix_frames > 1000,
                "mix and track prefixes precede refusal"
            );
        }
        let healthy = windfall_engine::render_stems(
            &stem_project,
            &pool,
            &windfall_engine::RenderOptions {
                mode: PlayMode::Pattern,
                ..options.clone()
            },
            &stem_options,
            &mut |_, _| true,
            &mut |_| true,
        )
        .unwrap();
        assert!(healthy.completed && healthy.plugin_error.is_none());
    }
    let destination = rig.file("native-failed.wav");
    std::fs::write(&destination, b"original destination").unwrap();
    let competitor = Arc::new(AtomicBool::new(false));
    let marked = competitor.clone();
    let competing_path = destination.clone();
    *crate::sync::lock(&rig.events.hook) = Some(Box::new(move |event| {
        if let crate::events::Event::ExportProgress(progress) = event
            && !progress.done
            && progress.fraction > 0.0
            && !marked.swap(true, Ordering::AcqRel)
        {
            std::fs::write(&competing_path, b"competitor destination").unwrap();
        }
    }));
    rig.events.take();
    rig.session
        .export_audio(ExportOptions {
            path: destination.clone(),
            format: ExportFormat::Wav,
            bit_depth: BitDepth::Float32,
            sample_rate: SAMPLE_RATE,
            mode: PlayMode::Song,
            tail_secs: 0.0,
            ..Default::default()
        })
        .unwrap();
    let progress = rig.events.wait_for_export();
    assert!(
        progress
            .error
            .as_deref()
            .is_some_and(|error| error.contains("plugin render failed")),
        "{progress:?}"
    );
    assert!(
        progress.files.is_none() && progress.cancelled != Some(true) && progress.fraction < 1.0
    );
    assert!(competitor.load(Ordering::Acquire));
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"competitor destination"
    );
    assert!(std::fs::read_dir(rig.folder.path()).unwrap().all(|entry| {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        !name.ends_with(".stage") && !name.ends_with(".backup")
    }));
    rig.events.take();
    rig.session
        .export_audio(ExportOptions {
            path: rig.file("native-failed-stems.wav"),
            format: ExportFormat::Wav,
            bit_depth: BitDepth::Float32,
            sample_rate: SAMPLE_RATE,
            mode: PlayMode::Song,
            stems: Some(windfall_ipc::ExportStems {
                mode: windfall_ipc::StemMode::ToMaster,
                tracks: Some(vec![track]),
                include_mix: true,
                numbered: false,
                folder: false,
            }),
            ..Default::default()
        })
        .unwrap();
    let stems_progress = rig.events.wait_for_export();
    assert!(
        stems_progress
            .error
            .as_deref()
            .is_some_and(|error| error.contains("plugin render failed"))
    );
    assert!(stems_progress.files.is_none() && stems_progress.cancelled != Some(true));
    assert!(std::fs::read_dir(rig.folder.path()).unwrap().all(|entry| {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        !name.contains("native-failed-stems")
            && !name.ends_with(".stage")
            && !name.ends_with(".backup")
    }));
    assert_eq!(rig.project(), document);
    assert_eq!(manager.runtime.selected_token(sibling), Some(sibling_token));
    assert!(
        manager
            .runtime
            .bridge_status()
            .iter()
            .any(|(before, token, status)| *before == sibling
                && *token == sibling_token
                && status.process_id == sibling_pid
                && !status.failed)
    );
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    assert!(rms(&out) > 0.001);
    // A fresh render in Pattern mode never reaches the failing Song control.
    let recovered = windfall_engine::render_reporting(
        &document,
        &pool,
        &windfall_engine::RenderOptions {
            mode: PlayMode::Pattern,
            ..options
        },
        &mut |_| true,
    );
    assert!(recovered.plugin_error.is_none() && rms(recovered.audio.samples()) > 0.001);
    assert_eq!(
        manager.runtime.selected_token(target),
        manager
            .runtime
            .bridge_status()
            .iter()
            .find(|(before, _, _)| *before == target)
            .map(|(_, token, _)| *token)
    );
}
#[test]
fn n4_clap_native_final_block_failure_discards_export_staging_and_preserves_live_sibling() {
    n4_native_final_block_export_failure(false);
}
#[test]
fn n4_vst3_native_final_block_failure_discards_export_staging_and_preserves_live_sibling() {
    n4_native_final_block_export_failure(true);
}

fn n4_measured_graph_impulse(vst3: bool) {
    let (mut rig, manager, target) = n4_setup(vst3, false);
    let source = rig.project().channels.last().unwrap().id;
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: source,
                step: 0,
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id: 7,
                value: 1.0,
            },
            None,
        )
        .unwrap();
    let mut samples = vec![0.0; 4096 * 2];
    samples[1024 * 2..1024 * 2 + 2].fill(0.5);
    let path = rig.file("bridge-impulse.wav");
    windfall_codec::write_wav(
        Path::new(&path),
        &windfall_core::AudioBuffer::from_interleaved(SAMPLE_RATE, 2, samples),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .add_audio_clip_from_file(
            &path,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: Some(TrackId::MASTER),
            },
        )
        .unwrap();
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
    rig.session.transport_play().unwrap();
    let mut heard = Vec::new();
    let mut out = [0.0; 1024];
    let mut frames = [1, 7, 64, 480, 512].into_iter().cycle();
    while heard.len() < 4096 * 2 {
        let count = frames.next().unwrap().min((4096 * 2 - heard.len()) / 2);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out[..count * 2])),
            0
        );
        heard.extend_from_slice(&out[..count * 2]);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let onset = heard
        .as_chunks::<2>()
        .0
        .iter()
        .position(|frame| frame.iter().any(|sample| sample.abs() > 1e-6));
    assert_eq!(rig.session.controller().latency_frames(), 561);
    assert_eq!(
        onset,
        Some(1585),
        "input1024 + source-path PDC12 + real native37 + bridge512"
    );
    let pool = rig.session.state().pool.clone();
    let project = rig.project();
    let mut reference = None;
    for block_frames in [1, 7, 64, 480, 512] {
        let rendered = windfall_engine::render_reporting(
            &project,
            &pool,
            &windfall_engine::RenderOptions {
                sample_rate: SAMPLE_RATE,
                mode: PlayMode::Song,
                block_frames,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert!(rendered.plugin_error.is_none() && rendered.sampler_error.is_none());
        let onset = rendered
            .audio
            .samples()
            .as_chunks::<2>()
            .0
            .iter()
            .position(|frame| frame.iter().any(|sample| sample.abs() > 1e-6));
        assert_eq!(
            onset,
            Some(1024),
            "offline strips the measured full graph PDC"
        );
        if let Some(reference) = &reference {
            assert_eq!(rendered.audio.samples(), reference);
        } else {
            reference = Some(rendered.audio.samples().to_vec());
        }
    }
    assert!(!manager.runtime.bridge_status()[0].2.failed);
}
#[test]
fn n4_clap_graph_impulse_measures_native_pipeline_and_pdc_alignment() {
    n4_measured_graph_impulse(false);
}
#[test]
fn n4_vst3_graph_impulse_measures_native_pipeline_and_pdc_alignment() {
    n4_measured_graph_impulse(true);
}

fn n4_failed_facades(vst3: bool) {
    let (mut rig, manager, sibling) = n4_setup(vst3, true);
    rig.session.transport_play().unwrap();
    let sibling_token = manager.runtime.selected_token(sibling).unwrap();
    let sibling_pid = manager.runtime.bridge_status()[0].2.process_id;
    let project = rig.project();
    let path = project.plugin(sibling).unwrap().path.clone();
    let names = if vst3 {
        [
            "VST3 Process Error",
            "VST3 NaN",
            "VST3 Bridge Permanent Process Hang",
        ]
    } else {
        [
            "Test Process Panic",
            "Test NaN",
            "Test Bridge Permanent Process Hang",
        ]
    };
    let entries = manager.state().entries;
    let target = PluginTarget::Effect {
        effect: EffectId(991),
    };
    let mut live_out = [0.0; 512];
    for name in names {
        let id = &entries.iter().find(|entry| entry.name == name).unwrap().id;
        let binding = manager.binding(&path, id, target).unwrap();
        let mut failed = manager.runtime.effect(&binding, SAMPLE_RATE, 256).unwrap();
        failed.transport(transport());
        let token = manager.runtime.selected_token(target).unwrap();
        let mut left = [0.5; 256];
        let mut right = left;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            left.fill(0.5);
            right.fill(0.5);
            assert_eq!(
                crate::test_alloc::allocator_calls(|| failed.process(&mut left, &mut right)),
                0
            );
            assert_eq!(
                crate::test_alloc::allocator_calls(|| rig.processor.process(&mut live_out)),
                0
            );
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
            let status = manager
                .runtime
                .bridge_status()
                .into_iter()
                .find(|entry| entry.0 == target)
                .unwrap()
                .2;
            if status.failed && status.reaped {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{name} did not fail/reap within its bound"
            );
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        for _ in 0..4 {
            left.fill(0.5);
            right.fill(0.5);
            assert_eq!(
                crate::test_alloc::allocator_calls(|| failed.process(&mut left, &mut right)),
                0
            );
        }
        assert_eq!(
            left, [0.5; 256],
            "{name}: effect fallback retains its negotiated dry delay"
        );
        let visible_deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !manager
            .state()
            .instances
            .iter()
            .any(|entry| entry.target == target && entry.error.contains("retry manually"))
        {
            assert!(
                std::time::Instant::now() < visible_deadline,
                "failed helper was not visible in manager"
            );
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        assert_eq!(rig.project(), project);
        assert_eq!(manager.runtime.selected_token(sibling), Some(sibling_token));
        let healthy = manager
            .runtime
            .bridge_status()
            .into_iter()
            .find(|entry| entry.0 == sibling)
            .unwrap()
            .2;
        assert_eq!(healthy.process_id, sibling_pid);
        assert!(!healthy.failed && !healthy.reaped);
        assert!(
            !manager
                .runtime
                .is_current(manager.runtime.document_revision().wrapping_add(1), token)
        );
        drop(failed); // Control-side facade retirement; no document/engine guard.
        assert_eq!(manager.runtime.selected_token(target), None);
        let delayed_id = &entries
            .iter()
            .find(|entry| {
                entry.name
                    == if vst3 {
                        "VST3 Bridge Delayed Effect"
                    } else {
                        "Test Bridge Delayed Effect"
                    }
            })
            .unwrap()
            .id;
        let healthy_binding = manager.binding(&path, delayed_id, target).unwrap();
        let mut replacement = manager
            .runtime
            .effect(&healthy_binding, SAMPLE_RATE, 256)
            .unwrap();
        replacement.transport(transport());
        assert_ne!(manager.runtime.selected_token(target), Some(token));
        assert_eq!(manager.runtime.selected_token(sibling), Some(sibling_token));
        drop(replacement);
    }
}
#[test]
fn n4_clap_crash_nonfinite_and_hang_keep_sibling_alive_and_failure_visible() {
    n4_failed_facades(false);
}
#[test]
fn n4_vst3_error_nonfinite_and_hang_keep_sibling_alive_and_failure_visible() {
    n4_failed_facades(true);
}

fn n4_document_barriers(vst3: bool, instrument: bool) {
    let (mut rig, manager, target) = n4_setup(vst3, instrument);
    let id = if instrument && !vst3 { 1 } else { 7 };
    let value = |project: &Project| {
        project
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == id)
            .unwrap()
            .value
    };
    let token = manager.runtime.selected_token(target).unwrap();
    let saved = rig.file("n4-pending.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    // No audio/adoption/native process occurs between commit and these captures.
    let pending = rig.project();
    let backup = rig
        .session
        .write_backup("2026-10-08_21-00-00")
        .unwrap()
        .unwrap();
    assert_eq!(value(&windfall_project::file::load(&backup).unwrap()), 0.75);
    rig.session.project_save(Some(&saved)).unwrap();
    assert_eq!(
        value(&windfall_project::file::load(Path::new(&saved)).unwrap()),
        0.75
    );
    assert_eq!(
        rig.project(),
        pending,
        "native capture may not mutate the live document"
    );
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    rig.session.undo().unwrap();
    assert_ne!(value(&rig.project()), 0.75);
    rig.session.redo().unwrap();
    assert_eq!(value(&rig.project()), 0.75);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [])),
        0
    );
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.625,
            },
            None,
        )
        .unwrap();
    super::archive::start_recording(&rig);
    let recording_project = rig.project();
    assert!(
        rig.session
            .project_save(Some(&rig.file("n4-recording.windfall")))
            .is_err()
    );
    assert!(rig.session.write_backup("2026-10-08_21-00-01").is_err());
    assert!(rig.session.refresh_plugins().is_err());
    assert!(rig.session.project_new().is_err());
    assert_eq!(rig.project(), recording_project);
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    rig.session.recording_cancel();
    let stale = rig.project();
    let revision = manager.runtime.document_revision();
    rig.session.project_new().unwrap();
    let mut out = [0.0; 512];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    assert!(!manager.runtime.is_current(revision, token));
    assert_eq!(
        manager.runtime.capture_at(stale.clone(), revision).unwrap(),
        stale
    );
    rig.session.project_open(&saved).unwrap();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    assert_eq!(value(&rig.project()), 0.75);
    assert_ne!(manager.runtime.selected_token(target), Some(token));
    assert_eq!(
        value(&manager.runtime.capture(rig.project()).unwrap()),
        0.75
    );
}
#[test]
fn n4_clap_effect_pending_save_undo_recording_and_replacement_barriers() {
    n4_document_barriers(false, false);
}
#[test]
fn n4_vst3_effect_pending_save_undo_recording_and_replacement_barriers() {
    n4_document_barriers(true, false);
}
#[test]
fn n4_clap_instrument_pending_save_undo_recording_and_replacement_barriers() {
    n4_document_barriers(false, true);
}
#[test]
fn n4_vst3_instrument_pending_save_undo_recording_and_replacement_barriers() {
    n4_document_barriers(true, true);
}

fn n4_instrument_death_and_manual_retry(vst3: bool) {
    let (mut rig, manager, target) = n4_setup(vst3, true);
    rig.session.transport_play().unwrap();
    let mut out = [0.0; 512];
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
    }
    assert!(rms(&out) > 0.001);
    let before = manager
        .runtime
        .bridge_status()
        .into_iter()
        .find(|entry| entry.0 == target)
        .unwrap();
    assert!(!before.2.failed && before.2.process_id != std::process::id());
    let document = rig.project();
    let revision = manager.runtime.document_revision();
    // Windows test-only process death, directed at this just-launched owned
    // synthetic helper. No native facade/control hook or fixture change.
    let killed = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &before.2.process_id.to_string()])
        .output()
        .unwrap();
    assert!(killed.status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
        assert!(out.iter().all(|sample| sample.is_finite()));
        let status = manager
            .runtime
            .bridge_status()
            .into_iter()
            .find(|entry| entry.0 == target)
            .unwrap()
            .2;
        if status.failed && status.reaped {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    for _ in 0..8 {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
    }
    assert_eq!(out, [0.0; 512], "dead instrument settles to silence");
    assert_eq!(rig.project(), document);
    assert!(manager.runtime.capture(document.clone()).is_err());
    assert_eq!(rig.project(), document);
    rig.session.transport_stop();
    rig.session.refresh_plugins().unwrap(); // Explicit existing control path.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
        0
    );
    let after = manager
        .runtime
        .bridge_status()
        .into_iter()
        .find(|entry| entry.0 == target)
        .unwrap();
    assert_ne!(after.1, before.1);
    assert_ne!(after.2.process_id, before.2.process_id);
    assert!(!after.2.failed && !after.2.reaped);
    assert!(!manager.runtime.is_current(revision, before.1));
    rig.session.transport_play().unwrap();
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)),
            0
        );
    }
    assert!(rms(&out) > 0.001);
    assert_eq!(rig.project(), document);
}
#[test]
fn n4_clap_instrument_helper_death_is_silent_and_explicit_retry_recovers() {
    n4_instrument_death_and_manual_retry(false);
}
#[test]
fn n4_vst3_instrument_helper_death_is_silent_and_explicit_retry_recovers() {
    n4_instrument_death_and_manual_retry(true);
}

fn n4_settled_gain_with_unprocessed_note(vst3: bool) {
    use std::time::Duration;
    use windfall_plugin_host::bridge::{
        protocol::{Config, DEFAULT_BLOCK, Kind},
        supervisor::{self, Launch},
    };
    let (mut rig, manager, target) = n4_setup(vst3, true);
    let PluginTarget::Instrument { channel } = target else {
        unreachable!()
    };
    let id = if vst3 { 7 } else { 1 };
    let value = |project: &Project| {
        project
            .plugin(target)
            .unwrap()
            .parameters
            .iter()
            .find(|param| param.id == id)
            .unwrap()
            .value
    };
    assert_eq!(value(&rig.project()), 0.5, "actual launch value");
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.625,
            },
            None,
        )
        .unwrap();
    let mut block = [0.0; 512];
    // Complete and collect many matching fixed blocks before the pending note.
    // This remains stopped, so no pattern automation or note release intervenes.
    for _ in 0..16 {
        std::thread::sleep(Duration::from_millis(8));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| rig.processor.process(&mut block)),
            0
        );
    }
    let before = manager.runtime.bridge_status()[0].clone();
    assert!(!before.2.failed);
    rig.session.controller().note_on(channel, 60, 0.75);
    // Admit the note into the facade, but never publish its incomplete B256
    // input block. Capture must not turn that note intent into DSP proof.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut [0.0; 2])),
        0
    );
    let document = rig.project();
    let captured = manager.runtime.capture(document.clone()).unwrap();
    assert_eq!(
        value(&captured),
        0.625,
        "an unprocessed note cannot restore launch-time gain0.5"
    );
    assert_eq!(rig.project(), document);
    assert_eq!(manager.runtime.selected_token(target), Some(before.1));
    assert_eq!(
        manager.runtime.bridge_status()[0].2.process_id,
        before.2.process_id
    );

    let saved = rig.file("n4-settled-note.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let loaded = windfall_project::file::load(Path::new(&saved)).unwrap();
    assert_eq!(value(&loaded), 0.625);
    // Independently restore ONLY opaque state, with no companion parameters
    // that could conceal a stale value written into VST3 native state.
    let binding = loaded.plugin(target).unwrap();
    let plugin = PathBuf::from(&binding.path);
    let (control, audio, _) = supervisor::launch(Launch {
        wrapper: Default::default(),
        helper: PathBuf::from(std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER").unwrap()),
        plugin: plugin.clone(),
        id: binding.id.clone(),
        format: binding.format.clone(),
        approved_binary: windfall_plugin_host::paths::plugin_file_identity(&plugin).unwrap(),
        config: Config {
            identity: supervisor::fresh_identity(9001, 1, 9001).unwrap(),
            sample_rate: SAMPLE_RATE,
            block: DEFAULT_BLOCK,
            native_latency: 0,
            kind: Kind::Instrument,
        },
        parameters: Vec::new(),
        state: binding.state.clone(),
        offline: false,
        startup_timeout: Duration::from_secs(5),
        audio_timeout: Duration::from_secs(2),
        cancelled: None,
    })
    .unwrap();
    let (_, parameters, _) = control.describe(Duration::from_secs(2)).unwrap();
    assert_eq!(
        parameters
            .iter()
            .find(|param| param.spec.id == id)
            .unwrap()
            .spec
            .value,
        0.625,
        "actual opaque native state must retain the settled gain"
    );
    drop(audio);
    assert!(control.terminate().reaped);

    // A newer committed document value still takes precedence when genuinely
    // pending; it must not change the live document while saving.
    rig.session
        .dispatch(
            Command::SetPluginParam {
                target,
                id,
                value: 0.75,
            },
            None,
        )
        .unwrap();
    let pending = rig.project();
    rig.session.project_save(Some(&saved)).unwrap();
    assert_eq!(
        value(&windfall_project::file::load(Path::new(&saved)).unwrap()),
        0.75
    );
    assert_eq!(rig.project(), pending);
    // Return to the settled snapshot saved above, using the real Session path.
    windfall_project::file::save(&loaded, Path::new(&saved)).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut block)),
        0
    );
    assert_eq!(value(&rig.project()), 0.625);
    assert_ne!(manager.runtime.selected_token(target), Some(before.1));
    assert_eq!(
        value(&manager.runtime.capture(rig.project()).unwrap()),
        0.625
    );
}

#[test]
fn n4_clap_settled_gain_survives_unprocessed_note_capture_save_and_reopen() {
    n4_settled_gain_with_unprocessed_note(false);
}
#[test]
fn n4_vst3_settled_gain_survives_unprocessed_note_capture_save_and_reopen() {
    n4_settled_gain_with_unprocessed_note(true);
}
