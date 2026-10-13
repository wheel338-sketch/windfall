//! The owned fixture is nested in a real export loaded by Windfall's own host.
//! Future integrator execution only; fixture builds are never run by edit workers.
#![cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod common;
use std::{path::{Path, PathBuf}, process::Command, sync::OnceLock, time::Duration};
use windfall_plugin::{Program, package::{Platform, export_bundle}};
use windfall_plugin_host::{PluginHost, Transport, export_runtime::{active_helpers, helper_filename}};
fn artifacts() -> &'static (PathBuf, PathBuf, PathBuf) {
    static FILES: OnceLock<(PathBuf, PathBuf, PathBuf)> = OnceLock::new();
    FILES.get_or_init(|| {
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("nested-export-artifacts");
        let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("../windfall-plugin-host");
        let build = |args: &[&str], manifest: Option<&Path>| {
            let mut command = Command::new(env!("CARGO"));
            command.args(["build", "--jobs", "1", "--target-dir"]).arg(&target);
            if let Some(manifest) = manifest { command.arg("--manifest-path").arg(manifest); }
            let result = command.args(args).output().unwrap();
            assert!(result.status.success(), "fixture build failed: {}", String::from_utf8_lossy(&result.stderr));
        };
        let library_name = |stem: &str| format!("{}{stem}{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
        let runtime = if let Some(path) = std::env::var_os("WINDFALL_EXPORT_FIXTURE") { PathBuf::from(path) }
            else { build(&["-p", "windfall-plugin", "--lib"], None); target.join("debug").join(library_name("windfall_plugin")) };
        let helper = if let Some(path) = std::env::var_os("WINDFALL_BRIDGE_HELPER") { PathBuf::from(path) }
            else { build(&["-p", "windfall-plugin-host", "--bin", "windfall-plugin-audio"], None); target.join("debug").join(helper_filename()) };
        let fixture = if let Some(path) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") { PathBuf::from(path) }
            else { build(&["--lib"], Some(&host.join("test-plugins/Cargo.toml"))); target.join("debug").join(library_name("windfall_test_plugins")) };
        for path in [&runtime, &helper, &fixture] { assert!(path.is_file(), "missing {}", path.display()); }
        (runtime, helper, fixture)
    })
}
fn runtime_binary(bundle: &Path, format: &str) -> PathBuf {
    if format == "vst3" { windfall_plugin_host::paths::vst3_binary(bundle).unwrap() }
    else if cfg!(target_os = "macos") { bundle.join("Contents/MacOS/Windfall") }
    else { bundle.to_owned() }
}
fn exported_runtime(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let (runtime, helper, _) = artifacts();
    let target = root.join(windfall_plugin_host::export_runtime::filename());
    std::fs::copy(runtime, &target).unwrap();
    std::fs::copy(helper, root.join(helper_filename())).unwrap();
    target
}
fn fixture(root: &Path, format: &str) -> PathBuf {
    // The host's existing macOS fixture utility creates a complete bundle.
    #[cfg(target_os = "macos")]
    if format == "vst3" {
        let bundle = root.join("owned.vst3");
        let native = bundle.join("Contents/MacOS/windfall-fixture");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::copy(&artifacts().2, native).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>windfall-fixture</string><key>CFBundleIdentifier</key><string>org.windfall.owned.fixture</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>").unwrap();
        return bundle;
    }
    let file = root.join(format!("owned.{format}"));
    std::fs::copy(&artifacts().2, &file).unwrap(); file
}
fn program_with_native(path: &Path, format: &str) -> Program {
    let module = PluginHost::windfall().load(path).unwrap();
    let descriptor = module.descriptors().into_iter().find(|d| if format == "clap" {
        d.id == "org.windfall.test.sine"
    } else { d.name == "VST3 Test Instrument" }).unwrap();
    let mut instance = module.create(&descriptor.id).unwrap();
    let parameters = instance.params().iter().map(|p| windfall_project::PluginParameter {
        id: p.id, name: p.name.clone(), min: p.min as f32, max: p.max as f32, value: p.default as f32,
        read_only: p.read_only, stepped: p.stepped, automatable: p.automatable,
    }).collect();
    let state = instance.save_state().unwrap().into_bytes();
    let mut program = common::program();
    program.project.channels[0].source = windfall_project::ChannelSource::Instrument {
        params: windfall_dsp::InstrumentParams::SubtractiveSynth(Default::default()), asset: None,
    };
    program.project.patterns[0].lanes[0].notes[0].start = 0;
    program.project.patterns[0].lanes[0].notes[0].key = 69;
    program.project.patterns[0].lanes[0].notes[0].length = 3840;
    program.project.mixer.tracks[0].external_output = None;
    program.project.plugins.push(windfall_project::PluginBinding {
        target: windfall_project::PluginTarget::Instrument { channel: program.project.channels[0].id },
        format: format.into(), path: path.to_string_lossy().into_owned(), id: descriptor.id,
        name: descriptor.name, state, parameters, wrapper: Default::default(), sidechain_input: None, auxiliary_inputs: vec![],
    });
    program.validate().unwrap(); program
}
#[test]
fn owned_unstamped_fixture_and_missing_nested_helper_are_visible_refusals() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("nested-refusals-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let runtime = exported_runtime(&root.join("distribution"));
    let native = fixture(&root, "clap");
    assert!(windfall_plugin_host::export_runtime::probe(&native, &windfall_plugin_host::export_runtime::Stamp::current()).unwrap_err().contains("no ABI stamp"));
    let program = program_with_native(&native, "clap");
    std::fs::remove_file(runtime.with_file_name(helper_filename())).unwrap();
    let destination = root.join("no-helper");
    if destination.exists() { std::fs::remove_dir_all(&destination).unwrap(); }
    assert!(export_bundle(&program, &runtime, &destination, Platform::native().unwrap()).unwrap_err().contains("distributed audio helper"));
    assert!(!destination.exists());
    std::fs::copy(&artifacts().1, runtime.with_file_name(helper_filename())).unwrap();
    export_bundle(&program, &runtime, &destination, Platform::native().unwrap()).unwrap();
    for outer in ["clap", "vst3"] {
        let bundle = destination.join(format!("Windfall.{outer}"));
        let binary = runtime_binary(&bundle, outer);
        std::fs::remove_file(binary.with_file_name(helper_filename())).unwrap();
        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collected = messages.clone(); let mut host = PluginHost::windfall();
        host.set_log(move |_, message| collected.lock().unwrap().push(message.to_owned()));
        let module = host.load(&bundle).unwrap();
        let mut instance = module.create(&module.descriptors()[0].id).unwrap();
        let error = instance.activate(48_000.0, 256).err().unwrap().to_string();
        if outer == "vst3" { assert!(error.contains("helper is missing"), "{error}"); }
        else { assert!(messages.lock().unwrap().iter().any(|m| m.contains("helper is missing"))); }
        assert_eq!(active_helpers(&binary).unwrap(), 0);
    }
}
#[test]
fn nested_clap_and_vst3_in_both_exports_measure_audio_state_reset_and_helper_retirement() {
    for nested in ["clap", "vst3"] { for outer in ["clap", "vst3"] {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("nested-{nested}-{outer}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let runtime = exported_runtime(&root.join("distribution"));
        let native = fixture(&root, nested);
        let program = program_with_native(&native, nested);
        let bundle_root = root.join("export");
        if bundle_root.exists() { std::fs::remove_dir_all(&bundle_root).unwrap(); }
        export_bundle(&program, &runtime, &bundle_root, Platform::native().unwrap()).unwrap();
        let bundle = bundle_root.join(format!("Windfall.{outer}"));
        let binary = runtime_binary(&bundle, outer);
        assert!(binary.with_file_name(helper_filename()).is_file());
        let module = PluginHost::windfall().load(&bundle).unwrap();
        let mut instance = module.create(&module.descriptors()[0].id).unwrap();
        assert_eq!(active_helpers(&binary).unwrap(), 0);
        let mut processor = instance.activate(48_000.0, 256).unwrap();
        assert_eq!(active_helpers(&binary).unwrap(), 1);
        assert_eq!(processor.latency_samples(), 1024 + if nested == "clap" { 32 } else { 17 });
        // Distinct export DLLs cannot share module-local PID/serial namespaces.
        let other_format = if outer == "clap" { "vst3" } else { "clap" };
        let other_bundle = bundle_root.join(format!("Windfall.{other_format}"));
        let other_binary = runtime_binary(&other_bundle, other_format);
        let other_module = PluginHost::windfall().load(&other_bundle).unwrap();
        let mut other_instance = other_module.create(&other_module.descriptors()[0].id).unwrap();
        let other_processor = other_instance.activate(48_000.0, 256).unwrap();
        assert_eq!(active_helpers(&other_binary).unwrap(), 1);
        assert_eq!(active_helpers(&binary).unwrap(), 1);
        let mut pcm = Vec::new();
        for block in 0..80 {
            processor.set_transport(Transport { playing: true, position_beats: block as f64 * 256.0 / 24_000.0,
                position_seconds: block as f64 * 256.0 / 48_000.0, ..Default::default() });
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            processor.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|x| x.is_finite()));
            pcm.extend_from_slice(&left);
            std::thread::sleep(Duration::from_millis(8));
        }
        let settled = &pcm[4096..];
        let rms = (settled.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / settled.len() as f64).sqrt();
        assert!(rms > 0.02, "{nested} in {outer} produced no fixture audio: RMS {rms}");
        let crossings = settled.windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.0).count();
        let hz = crossings as f64 * 48_000.0 / settled.len() as f64;
        assert!((hz - 440.0).abs() < 5.0, "nested fixture frequency {hz}");
        processor.reset(); processor.set_transport(Transport::default());
        for _ in 0..12 {
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            processor.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|x| *x == 0.0), "old nested PCM survived hard reset");
            std::thread::sleep(Duration::from_millis(8));
        }
        instance.deactivate(processor).unwrap();
        assert_eq!(active_helpers(&binary).unwrap(), 0, "helper was not reaped off audio");
        assert_eq!(active_helpers(&other_binary).unwrap(), 1, "retiring one DLL killed the other's native owner");
        other_instance.deactivate(other_processor).unwrap();
        assert_eq!(active_helpers(&other_binary).unwrap(), 0);
        let state = instance.save_state().unwrap();
        instance.load_state(&state).unwrap();
        let mut restored = instance.activate(48_000.0, 256).unwrap();
        assert_eq!(active_helpers(&binary).unwrap(), 1);
        restored.note_on(0, 69, 1.0);
        let mut energy = 0.0;
        for _ in 0..20 {
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            restored.process(&mut left, &mut right);
            energy += left.iter().map(|x| x.abs()).sum::<f32>();
            std::thread::sleep(Duration::from_millis(8));
        }
        assert!(energy > 1.0, "restored nested instrument is silent");
        instance.deactivate(restored).unwrap();
        assert_eq!(active_helpers(&binary).unwrap(), 0);
    } }
}
#[test]
fn nested_gain_effects_preserve_stereo_and_measure_saved_parameter_attenuation() {
    let mut reference = common::program();
    reference.project.mixer.tracks[0].external_output = Some(windfall_project::ExternalOutputRoute { left: 0, right: Some(1), exclusive: false });
    let mut engine = windfall_plugin::GuestEngine::activate(&reference, 48_000.0).unwrap();
    let mut expected = 0.0f64;
    for block in 0..64 {
        engine.sync_transport(0.99 + block as f64 * 256.0 / 24_000.0, 120.0, true);
        expected += engine.render(256).unwrap().chunks_exact(windfall_plugin::OUTPUT_CHANNELS).map(|p| f64::from(p[0]).powi(2)).sum::<f64>();
    }
    assert!(expected > 1.0);
    for nested in ["clap", "vst3"] { for outer in ["clap", "vst3"] {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("nested-effect-{nested}-{outer}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let runtime = exported_runtime(&root.join("distribution")); let path = fixture(&root, nested);
        let module = PluginHost::windfall().load(&path).unwrap();
        let descriptor = module.descriptors().into_iter().find(|d| if nested == "clap" { d.id == "org.windfall.test.gain" } else { d.name == "VST3 Test Effect" }).unwrap();
        let mut native = module.create(&descriptor.id).unwrap();
        let mut parameters = native.params().iter().map(|p| windfall_project::PluginParameter {
            id: p.id, name: p.name.clone(), min: p.min as f32, max: p.max as f32, value: p.default as f32,
            read_only: p.read_only, stepped: p.stepped, automatable: p.automatable,
        }).collect::<Vec<_>>();
        // Owned VST3 fixture multiplies its normalized gain by two; the CLAP
        // fixture exposes linear gain directly. Both must measure quarter gain.
        parameters.iter_mut().find(|p| p.id == 7).unwrap().value = if nested == "vst3" { 0.125 } else { 0.25 };
        let mut program = reference.clone();
        program.project.next_id = 50;
        program.project.mixer.tracks[0].effects.push(windfall_project::EffectSlot { id: windfall_project::EffectId(40), impulse: None,
            params: windfall_dsp::EffectParams::Delay(Default::default()), enabled: true, mix: 1.0 });
        program.project.plugins.push(windfall_project::PluginBinding { target: windfall_project::PluginTarget::Effect { effect: windfall_project::EffectId(40) },
            format: nested.into(), path: path.to_string_lossy().into_owned(), id: descriptor.id, name: descriptor.name,
            state: native.save_state().unwrap().into_bytes(), parameters, wrapper: Default::default(), sidechain_input: None, auxiliary_inputs: vec![] });
        drop(native); drop(module);
        let destination = root.join("bundles"); if destination.exists() { std::fs::remove_dir_all(&destination).unwrap(); }
        export_bundle(&program, &runtime, &destination, Platform::native().unwrap()).unwrap();
        let bundle = destination.join(format!("Windfall.{outer}")); let binary = runtime_binary(&bundle, outer);
        let module = PluginHost::windfall().load(&bundle).unwrap();
        let mut instance = module.create(&module.descriptors()[0].id).unwrap();
        let mut processor = instance.activate(48_000.0, 256).unwrap();
        assert!(processor.latency_samples() >= 1024);
        let mut measured = 0.0f64;
        for block in 0..64 {
            processor.set_transport(Transport { playing: true, position_beats: 0.99 + block as f64 * 256.0 / 24_000.0,
                position_seconds: block as f64 * 256.0 / 48_000.0, ..Default::default() });
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]); processor.process(&mut left, &mut right);
            assert!(left.iter().zip(&right).all(|(l,r)| (*l + *r).abs() < 1e-6));
            measured += left.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
            std::thread::sleep(Duration::from_millis(8));
        }
        let ratio = (measured / expected).sqrt();
        assert!((ratio - 0.25).abs() < 0.005, "nested {nested}/{outer} gain ratio {ratio}");
        processor.reset(); processor.set_transport(Transport::default());
        for _ in 0..8 {
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]); processor.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|x| *x == 0.0));
            std::thread::sleep(Duration::from_millis(8));
        }
        instance.deactivate(processor).unwrap(); assert_eq!(active_helpers(&binary).unwrap(), 0);
        let state = instance.save_state().unwrap(); instance.load_state(&state).unwrap();
        let reopened = instance.activate(48_000.0, 256).unwrap();
        instance.deactivate(reopened).unwrap(); assert_eq!(active_helpers(&binary).unwrap(), 0);
    } }
}
