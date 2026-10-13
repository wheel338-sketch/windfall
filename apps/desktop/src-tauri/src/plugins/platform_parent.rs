//! Read the calling window's native owner on Tauri's GUI thread. Return only
//! an integer handle, never a borrowed raw-window-handle across the dispatch.
#[cfg(target_os = "linux")]
pub(crate) fn linux_owner(window: tauri::WebviewWindow) -> Result<Option<usize>, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let owner = window.clone();
    window
        .run_on_main_thread(move || {
            let result = owner
                .window_handle()
                .map_err(|error| error.to_string())
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Xlib(handle) if handle.window != 0 => {
                        Ok(Some(handle.window as usize))
                    }
                    RawWindowHandle::Xcb(handle) => Ok(Some(handle.window.get() as usize)),
                    // X11 editors still work as detached XWayland windows under a
                    // native Wayland shell. There is no XID for a Wayland owner.
                    RawWindowHandle::Wayland(_) => Ok(None),
                    _ => Err("The desktop did not supply an X11 owner window".into()),
                });
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "Native owner lookup timed out".to_owned())?
}
