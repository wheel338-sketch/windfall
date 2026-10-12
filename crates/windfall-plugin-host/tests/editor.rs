//! Native editor lifecycle tests with the repository's fixture.
#![cfg(windows)]
mod common;
use std::time::{Duration, Instant};
use windfall_plugin_host::{EditorError, EditorOptions, PluginNotification, gui};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetParent, IsWindow, IsWindowVisible, PostMessageW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SetWindowPos, WM_CLOSE,
};

#[test]
fn independent_vst3_view_attaches_scales_gestures_state_and_tears_down_real_windows() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_CHILD, GetDlgItem, SendMessageW, BM_CLICK};
    let file = common::plugin_file("vst3-real-editor", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall().load(&file).unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    assert!(instance.has_editor());
    let editor = instance.open_editor(&EditorOptions { take_focus: false, ..Default::default() }).unwrap();
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
    unsafe { SendMessageW(button, BM_CLICK, 0, 0); }
    let mut notices = Vec::new(); instance.idle(&mut |notice| notices.push(notice));
    assert!(notices.contains(&PluginNotification::ParamGestureBegin { id: 7 }));
    assert!(notices.contains(&PluginNotification::ParamChanged { id: 7, value: 0.25 }));
    assert!(notices.contains(&PluginNotification::ParamGestureEnd { id: 7 }));
    assert_eq!(instance.param_value(7), Some(0.25));
    let state = instance.save_state().unwrap();
    instance.close_editor();
    assert_eq!(unsafe { IsWindow(parent) }, 0);
    assert_eq!(unsafe { IsWindow(child) }, 0);
    assert_eq!(unsafe { IsWindow(button) }, 0);
    drop(instance); drop(module);
    // Force the fixture to unload/reload. Its window class must never retain a
    // callback into the old library mapping.
    let module = windfall_plugin_host::PluginHost::windfall().load(&file).unwrap();
    let mut reopened = module.create(&module.descriptors()[0].id).unwrap();
    reopened.load_state(&state).unwrap(); assert_eq!(reopened.param_value(7), Some(0.25));
    let editor = reopened.open_editor(&EditorOptions::default()).unwrap();
    let parent = editor.window.unwrap() as *mut std::ffi::c_void;
    let button = unsafe { GetDlgItem(GetWindow(parent, GW_CHILD), 7) };
    unsafe { SendMessageW(button, BM_CLICK, 0, 0); }
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
