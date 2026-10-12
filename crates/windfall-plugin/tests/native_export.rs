//! Actual exported DLLs loaded by Windfall's native CLAP/VST3 host. Like the
//! existing host-fixture pattern, build only when the integrator runs this test.
#![cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod common;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
use windfall_plugin::{
    Program,
    package::{Platform, export_bundle},
};
use windfall_plugin_host::{PluginHost, Transport};
fn runtime() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        if let Some(path) = std::env::var_os("WINDFALL_EXPORT_FIXTURE") {
            let path = PathBuf::from(path);
            assert!(path.is_file());
            return path;
        }
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("native-export-runtime");
        let result = Command::new(env!("CARGO"))
            .args([
                "build",
                "--lib",
                "-p",
                "windfall-plugin",
                "--jobs",
                "1",
                "--target-dir",
            ])
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "export fixture build failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let path = target.join("debug").join(format!(
            "{}windfall_plugin{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        assert!(path.is_file());
        path
    })
}
#[test]
fn exported_clap_and_vst3_load_through_own_host_audio_state_tempo_and_reset() {
    for (format, extension) in [("clap", "clap"), ("vst3", "vst3")] {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("export-{format}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let destination = root.join("bundles");
        if destination.exists() {
            std::fs::remove_dir_all(&destination).unwrap();
        }
        let mut program = common::program();
        program.project.mixer.tracks[0]
            .external_output
            .as_mut()
            .unwrap()
            .left = 0;
        program.project.mixer.tracks[0]
            .external_output
            .as_mut()
            .unwrap()
            .right = Some(1);
        export_bundle(
            &program,
            runtime(),
            &destination,
            Platform::native().unwrap(),
        )
        .unwrap();
        let module = PluginHost::windfall()
            .load(&destination.join(format!("Windfall.{extension}")))
            .unwrap();
        let descriptors = module.descriptors();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].name, "Windfall");
        if format == "vst3" {
            assert_eq!(descriptors[0].id, "57494E4446414C4C50524F4752414D01");
        }
        let mut instance = module.create(&descriptors[0].id).unwrap();
        assert_eq!(instance.layout().audio_outputs.len(), 16);
        let state = instance.save_state().unwrap();
        assert!(state.as_bytes().len() > program.encode().unwrap().len());
        #[cfg(windows)]
        {
            let editor = instance
                .open_editor(&windfall_plugin_host::EditorOptions::default())
                .unwrap();
            assert_eq!((editor.width, editor.height), (560, 180));
            assert!(editor.window.is_some());
            instance.close_editor();
        }
        for (tempo, expected_onset) in [(120.0, 240usize), (240.0, 120)] {
            instance.load_state(&state).unwrap();
            let mut instrument = instance.prepare_instrument(48_000.0, 256).unwrap();
            instrument.set_transport(Transport {
                position_beats: 0.99,
                tempo_bpm: tempo,
                playing: true,
                numerator: 7,
                denominator: 8,
                ..Transport::default()
            });
            let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
            instrument.process(&mut left, &mut right);
            assert!(
                left[..expected_onset]
                    .iter()
                    .chain(&right[..expected_onset])
                    .all(|v| *v == 0.0),
                "{format} onset precedes host clock"
            );
            assert!(
                left[expected_onset..].iter().any(|v| v.abs() > 0.01),
                "{format} did not load bundled song"
            );
            assert!(left.iter().zip(&right).all(|(l, r)| (*l + *r).abs() < 1e-6));
            assert!(!instrument.health().failed);
            instrument.reset();
            instrument.set_transport(Transport::default());
            instrument.process(&mut left, &mut right);
            assert!(
                left.iter().chain(&right).all(|v| *v == 0.0),
                "{format} reset retained audio"
            );
            instance.release_instrument(instrument).unwrap();
            assert_eq!(instance.save_state().unwrap().as_bytes(), state.as_bytes());
        }
        drop(instance);
        drop(module);
        // The directory also contains both complete distributable formats.
        assert!(
            destination
                .join("Windfall.vst3/Contents/Resources/moduleinfo.json")
                .is_file()
        );
        assert!(
            Program::decode(
                &std::fs::read(
                    destination.join("Windfall.vst3/Contents/Resources/windfall-state.json")
                )
                .unwrap()
            )
            .is_ok()
        );
    }
}
