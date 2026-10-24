//! Native editor lifecycle tests with the repository's fixture.
#![cfg(windows)]
mod common;
use std::time::{Duration, Instant};
use windfall_plugin_host::{EditorError, EditorOptions, PluginNotification, gui};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetParent, GetWindowRect, IsWindow, IsWindowVisible, PostMessageW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SendMessageW, SetWindowPos, WM_CLOSE, WM_DPICHANGED,
};

fn monitor_transition(
    instance: &mut windfall_plugin_host::PluginInstance,
    dpi: u32,
) -> Vec<PluginNotification> {
    let hwnd = instance.editor().unwrap().window.unwrap() as *mut std::ffi::c_void;
    let mut rect = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: the editor and synchronous RECT belong to this test's GUI thread.
    unsafe {
        assert_ne!(GetWindowRect(hwnd, &mut rect), 0);
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            (dpi | (dpi << 16)) as usize,
            (&rect as *const _) as isize,
        );
    }
    common::idle(instance)
}

#[test]
fn live_monitor_dpi_scales_clap_content_once_and_reports_refusal_without_closing() {
    let (_module, mut instance) = common::create(common::GAIN);
    instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..Default::default()
        })
        .unwrap();
    // Drain the fixture's one autonomous resize before the monitor matrix.
    let deadline = Instant::now() + Duration::from_millis(90);
    while Instant::now() < deadline {
        gui::pump_events(Some(Duration::from_millis(8)));
        common::idle(&mut instance);
    }
    instance.set_editor_scale(1.25).unwrap();
    let window = instance.editor().unwrap().window;
    for dpi in [96, 144, 192, 120, 96] {
        let notifications = monitor_transition(&mut instance, dpi);
        // Fixture's authored 320 x 200 editor snaps to its 20-pixel grid.
        let factor = 1.25 * f64::from(dpi) / 96.0;
        let width = ((320.0 * factor).round() as u32 / 20 * 20).max(160);
        let height = ((200.0 * factor).round() as u32 / 20 * 20).max(100);
        let editor = instance.editor().unwrap();
        assert_eq!((editor.width, editor.height), (width, height));
        assert_eq!(editor.window, window);
        assert!(notifications.contains(&PluginNotification::EditorResized { width, height }));
        assert!(
            !notifications
                .iter()
                .any(|event| matches!(event, PluginNotification::EditorScaleRefused { .. }))
        );
    }
    let before = instance.editor();
    assert!(instance.set_param(common::gain::MODE, 1.0));
    let notifications = monitor_transition(&mut instance, 192);
    assert_eq!(instance.editor(), before);
    assert!(
        notifications.contains(&PluginNotification::EditorScaleRefused {
            application: 1.25,
            monitor: 2.0
        })
    );
    assert!(instance.set_param(common::gain::MODE, 0.0));
    monitor_transition(&mut instance, 144);
    assert_eq!(instance.editor().unwrap().width, 600);
    instance.close_editor();
}

#[test]
fn joined_vst3_fixture_monitor_scaling_preserves_the_real_view_and_native_owner() {
    let (_owner_module, mut owner) = common::create(common::GAIN);
    let owner_window = owner
        .open_editor(&EditorOptions {
            take_focus: false,
            ..Default::default()
        })
        .unwrap()
        .window
        .unwrap();
    let path = common::plugin_file("r4-editor-vst3", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    // Class 0's real IPlugView is supplied by the round-four hosting worker.
    instance
        .open_editor(&EditorOptions {
            take_focus: false,
            detached: false,
            owner_window: Some(owner_window),
            ..Default::default()
        })
        .unwrap();
    let window = instance.editor().unwrap().window.unwrap();
    monitor_transition(&mut instance, 96);
    let baseline = instance.set_editor_scale(1.0).unwrap();
    instance.set_editor_scale(1.25).unwrap();
    for dpi in [144, 192, 120, 96] {
        let events = monitor_transition(&mut instance, dpi);
        let info = instance.editor().unwrap();
        let factor = 1.25 * f64::from(dpi) / 96.0;
        assert_eq!(info.window, Some(window));
        assert_eq!(
            info.width,
            (f64::from(baseline.width) * factor).round() as u32
        );
        assert_eq!(
            info.height,
            (f64::from(baseline.height) * factor).round() as u32
        );
        assert!(events.contains(&PluginNotification::EditorResized {
            width: info.width,
            height: info.height
        }));
        // SAFETY: live native host HWND, on its GUI owner thread.
        assert_eq!(
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindow(
                    window as *mut std::ffi::c_void,
                    windows_sys::Win32::UI::WindowsAndMessaging::GW_OWNER,
                )
            } as usize,
            owner_window
        );
    }
    instance.close_editor();
    assert_eq!(unsafe { IsWindow(window as *mut std::ffi::c_void) }, 0);
    owner.close_editor();
}

#[test]
fn independent_vst3_view_attaches_scales_gestures_state_and_tears_down_real_windows() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, GW_CHILD, GetDlgItem, GetWindow, SendMessageW,
    };
    let file = common::plugin_file("vst3-real-editor", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&file)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    assert!(instance.has_editor());
    let editor = instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..Default::default()
        })
        .unwrap();
    let parent = editor.window.unwrap() as *mut std::ffi::c_void;
    let child = unsafe { GetWindow(parent, GW_CHILD) };
    assert!(!child.is_null());
    let button = unsafe { GetDlgItem(child, 7) };
    assert!(!button.is_null());
    assert_eq!(unsafe { GetParent(child) }, parent);
    assert_ne!(unsafe { IsWindowVisible(button) }, 0);
    let original = instance.set_editor_scale(1.0).unwrap();
    let double = instance.set_editor_scale(2.0).unwrap();
    assert_eq!(double.window, original.window);
    assert!((double.width as i64 - original.width as i64 * 2).abs() <= 1);
    assert!((double.height as i64 - original.height as i64 * 2).abs() <= 1);
    assert_eq!(instance.set_editor_scale(1.0).unwrap(), original);
    unsafe {
        SendMessageW(button, BM_CLICK, 0, 0);
    }
    let mut notices = Vec::new();
    instance.idle(&mut |notice| notices.push(notice));
    assert!(notices.contains(&PluginNotification::ParamGestureBegin { id: 7 }));
    assert!(notices.contains(&PluginNotification::ParamChanged { id: 7, value: 0.25 }));
    assert!(notices.contains(&PluginNotification::ParamGestureEnd { id: 7 }));
    assert_eq!(instance.param_value(7), Some(0.25));
    let state = instance.save_state().unwrap();
    instance.close_editor();
    assert_eq!(unsafe { IsWindow(parent) }, 0);
    assert_eq!(unsafe { IsWindow(child) }, 0);
    assert_eq!(unsafe { IsWindow(button) }, 0);
    drop(instance);
    drop(module);
    // Force the fixture to unload/reload. Its window class must never retain a
    // callback into the old library mapping.
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&file)
        .unwrap();
    let mut reopened = module.create(&module.descriptors()[0].id).unwrap();
    reopened.load_state(&state).unwrap();
    assert_eq!(reopened.param_value(7), Some(0.25));
    let editor = reopened.open_editor(&EditorOptions::default()).unwrap();
    let parent = editor.window.unwrap() as *mut std::ffi::c_void;
    let button = unsafe { GetDlgItem(GetWindow(parent, GW_CHILD), 7) };
    unsafe {
        SendMessageW(button, BM_CLICK, 0, 0);
    }
    reopened.idle(&mut |_| {});
    assert_eq!(reopened.param_value(7), Some(0.75));
    reopened.close_editor();
    assert_eq!(unsafe { IsWindow(parent) }, 0);
}

#[test]
fn native_content_scale_resizes_restores_reopens_and_keeps_refused_editor_alive() {
    let (_module, mut instance) = common::create(common::GAIN);
    assert_eq!(instance.set_editor_scale(1.25), Err(EditorError::NoEditor));
    instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..Default::default()
        })
        .unwrap();
    let original = instance.set_editor_scale(1.0).unwrap();
    let enlarged = instance.set_editor_scale(2.0).unwrap();
    assert_eq!(enlarged.window, original.window);
    assert!(enlarged.width >= original.width * 2 - 20);
    assert!(enlarged.height >= original.height * 2 - 20);
    assert_eq!(instance.set_editor_scale(1.0).unwrap(), original);
    assert_eq!(
        instance.set_editor_scale(f64::NAN),
        Err(EditorError::InvalidScale)
    );
    assert_eq!(instance.editor(), Some(original));
    assert!(instance.set_param(common::gain::MODE, 1.0));
    assert_eq!(
        instance.set_editor_scale(1.5),
        Err(EditorError::UnsupportedScale)
    );
    assert_eq!(instance.editor(), Some(original));
    assert!(instance.set_param(common::gain::MODE, 0.0));
    instance.close_editor();
    instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..Default::default()
        })
        .unwrap();
    let reopened = instance.set_editor_scale(2.0).unwrap();
    assert_eq!(
        (reopened.width, reopened.height),
        (enlarged.width, enlarged.height)
    );
    instance.close_editor();
}

#[test]
fn fixed_vst3_editor_without_optional_frame_opens_a_visible_window() {
    let path = common::plugin_file("vst3-optional-frame", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let descriptor = module
        .descriptors()
        .into_iter()
        .find(|descriptor| descriptor.name == "VST3 Fixed Editor Without Frame")
        .unwrap();
    let mut instance = module.create(&descriptor.id).unwrap();
    assert!(instance.has_editor());
    for _ in 0..2 {
        let editor = instance
            .open_editor(&EditorOptions {
                take_focus: false,
                ..Default::default()
            })
            .expect("a fixed editor can attach without implementing host resize callbacks");
        let window = editor.window.unwrap() as *mut std::ffi::c_void;
        assert_eq!((editor.width, editor.height), (320, 200));
        assert!(!editor.resizable);
        assert_ne!(unsafe { IsWindowVisible(window) }, 0);
        instance.close_editor();
        assert_eq!(unsafe { IsWindow(window) }, 0);
    }
}

#[test]
fn native_editor_opens_pumps_resizes_and_closes() {
    let (_module, mut instance) = common::create(common::GAIN);
    let info = instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..EditorOptions::default()
        })
        .unwrap();
    let handle = info.window.unwrap() as *mut std::ffi::c_void;
    // SAFETY: live window handle returned by this instance on this thread.
    unsafe {
        assert_ne!(IsWindow(handle), 0);
        assert_ne!(IsWindowVisible(handle), 0);
        assert!(GetParent(handle).is_null(), "editor is a top-level window");
        SetWindowPos(
            handle,
            std::ptr::null_mut(),
            0,
            0,
            700,
            500,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
        );
    }
    assert!(matches!(
        instance.open_editor(&EditorOptions::default()),
        Err(EditorError::AlreadyOpen)
    ));
    let deadline = Instant::now() + Duration::from_millis(80);
    let mut notifications = Vec::new();
    while Instant::now() < deadline {
        gui::pump_events(Some(Duration::from_millis(8)));
        instance.idle(&mut |event| notifications.push(event));
    }
    assert!(
        notifications
            .iter()
            .any(|event| matches!(event, PluginNotification::EditorResized { .. }))
    );
    assert!(instance.param_value(common::gain::TIMER_TICKS).unwrap() > 0.0);
    // SAFETY: post the normal close request to our own live window.
    unsafe {
        PostMessageW(handle, WM_CLOSE, 0, 0);
    }
    gui::pump_events(None);
    instance.idle(&mut |event| notifications.push(event));
    assert!(notifications.contains(&PluginNotification::EditorClosed));
    assert!(instance.editor().is_none());
    // SAFETY: IsWindow accepts handles whose windows have been destroyed.
    assert_eq!(unsafe { IsWindow(handle) }, 0);
    let second = instance
        .open_editor(&EditorOptions {
            take_focus: false,
            ..EditorOptions::default()
        })
        .unwrap();
    instance.close_editor();
    // SAFETY: IsWindow accepts handles whose windows have been destroyed.
    assert_eq!(
        unsafe { IsWindow(second.window.unwrap() as *mut std::ffi::c_void) },
        0
    );
}
