use super::*;
use windfall_plugin_host::PluginNotification;
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain([0]).collect() }
fn load_editor(window: usize, path: &Path) -> String {
    unsafe {
        let panel = GetWindow(window as HWND, GW_CHILD);
        assert!(!panel.is_null(), "export editor panel was not embedded");
        let input = GetDlgItem(panel, 2);
        assert!(!input.is_null());
        assert_ne!(SetWindowTextW(input, wide(&path.to_string_lossy()).as_ptr()), 0);
        SendMessageW(panel, WM_COMMAND, 1, GetDlgItem(panel, 1) as isize);
        // Child Z-order is not creation order; inspect every control label.
        let mut current = GetWindow(panel, GW_CHILD);
        let mut labels = String::new();
        while !current.is_null() {
            let mut text = [0u16; 4096];
            let size = GetWindowTextW(current, text.as_mut_ptr(), text.len() as i32);
            labels.push_str(&String::from_utf16_lossy(&text[..size.max(0) as usize]));
            current = GetWindow(current, GW_HWNDNEXT);
        }
        labels
    }
}
#[test]
fn native_editor_loads_emit_own_host_dirty_parameters_pdc_and_restart_notifications() {
    for format in ["clap", "vst3"] {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-notifications-{format}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let destination = root.join("bundles");
        if destination.exists() { std::fs::remove_dir_all(&destination).unwrap(); }
        export_bundle(&Program::default(), runtime(), &destination, Platform::native().unwrap()).unwrap();
        let module = PluginHost::windfall().load(&destination.join(format!("Windfall.{format}"))).unwrap();
        let mut instance = module.create(&module.descriptors()[0].id).unwrap();
        let editor = instance.open_editor(&windfall_plugin_host::EditorOptions::default()).unwrap();
        let mut processor = instance.activate(48_000.0, 256).unwrap();
        instance.idle(&mut |_| {});
        let mut program = Program::default();
        program.project.settings.name = "Editor replacement".into(); program.project.next_id = 50;
        program.project.mixer.tracks[0].effects.push(windfall_project::EffectSlot {
            id: windfall_project::EffectId(40), impulse: None, enabled: true, mix: 1.0,
            params: windfall_dsp::EffectParams::Limiter(Default::default()),
        });
        let packet = root.join("editor-program.json"); std::fs::write(&packet, program.encode().unwrap()).unwrap();
        assert!(load_editor(editor.window.unwrap(), &packet).contains("Program queued"));
        let mut notifications = Vec::new(); instance.idle(&mut |n| notifications.push(n));
        assert!(notifications.contains(&PluginNotification::StateChanged));
        assert!(notifications.contains(&PluginNotification::ParamsRescanned));
        assert!(notifications.contains(&PluginNotification::RestartRequested));
        let expected = 240;
        if format == "vst3" { assert!(notifications.contains(&PluginNotification::LatencyChanged { samples: expected })); }
        // Honour restart before consuming the new program. Pending replacement
        // must settle even when no old/new callback boundary ever ran.
        processor.reset(); instance.deactivate(processor).unwrap();
        let processor = instance.activate(48_000.0, 256).unwrap();
        assert_eq!(processor.latency_samples(), expected);
        notifications.clear(); instance.idle(&mut |n| notifications.push(n));
        if format == "clap" { assert!(notifications.contains(&PluginNotification::LatencyChanged { samples: expected })); }
        instance.deactivate(processor).unwrap();
        let state = instance.save_state().unwrap();
        std::fs::write(&packet, b"malformed program").unwrap();
        assert!(!load_editor(editor.window.unwrap(), &packet).contains("Program queued"));
        notifications.clear(); instance.idle(&mut |n| notifications.push(n));
        assert!(!notifications.contains(&PluginNotification::StateChanged));
        assert_eq!(instance.save_state().unwrap().as_bytes(), state.as_bytes());
        instance.close_editor();
    }
}
