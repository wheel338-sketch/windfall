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
