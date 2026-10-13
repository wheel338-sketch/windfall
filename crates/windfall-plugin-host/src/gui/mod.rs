//! Plugin editors as floating native windows.
//!
//! A plugin draws its own editor, but most will only draw it inside a
//! window the host provides. So opening an editor means making a top-level
//! window, handing it to the plugin as the parent, and keeping the two the
//! same size from then on. Either side may ask for a resize: the user by
//! dragging the frame, the plugin when it folds out a panel.
//!
//! Some plugins insist on making their own window instead. Those are shown
//! as they are, and the host has no window of its own for them.
//!
//! # Platforms
//!
//! The native parents are HWND (Windows), NSView (Cocoa) and an XEmbed
//! container XID (Linux). Cocoa uses the application's actual main thread;
//! X11 connections/run-loop callbacks stay on the plugin's creating owner.
//!
//! # The event loop
//!
//! Windows of every editor belong to the thread that opened them, and that
//! thread has to run a message loop. In the app that is the shell's own
//! loop. A program without one can use [`pump_events`].

use std::time::Duration;

#[cfg(any(test, target_os = "macos"))]
pub(crate) mod au_geometry;
#[cfg(any(test, target_os = "macos", target_os = "linux"))]
pub(crate) mod bridge_editor;
#[cfg(target_os = "macos")]
pub(crate) mod cocoa;
#[cfg(target_os = "macos")]
#[allow(dead_code)] // shared ABI includes the exported panel's selectors
pub(crate) mod cocoa_ffi;
#[cfg(any(test, target_os = "macos"))]
pub mod dispatch;
#[cfg(any(test, target_os = "macos", target_os = "linux"))]
pub(crate) mod lifecycle;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) mod native;
#[cfg(any(test, target_os = "linux"))]
pub(crate) mod run_loop;
pub(crate) mod scaling;
#[cfg(windows)]
pub(crate) mod win32;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
#[allow(dead_code)] // shared ABI includes the exported panel's drawing calls
mod x11_ffi;

/// How to open an editor.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorOptions {
    /// The window's title.
    pub title: String,
    /// Bring the window to the front and give it the keyboard. Off, the
    /// window appears without taking the keyboard from what the user is
    /// doing.
    pub take_focus: bool,
    /// Initial native screen coordinates (AppKit points on Cocoa, pixels on
    /// Windows/X11).
    /// `None` leaves it to the system.
    pub position: Option<(i32, i32)>,
    /// Multiplier of platform DPI. Refused native scaling is an explicit error.
    pub scale: f64,
    /// Independent top-level editor versus a window owned by the DAW window.
    pub detached: bool,
    pub owner_window: Option<usize>,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            title: "Plugin".to_owned(),
            take_focus: true,
            position: None,
            scale: 1.0,
            detached: true,
            owner_window: None,
        }
    }
}

impl EditorOptions {
    pub fn validate(&self) -> Result<(), EditorError> {
        if !self.scale.is_finite() || !(0.5..=4.0).contains(&self.scale) {
            return Err(EditorError::Failed(
                "editor scale must be between 0.5 and 4".into(),
            ));
        }
        if !self.detached && self.owner_window.is_none() {
            return Err(EditorError::Failed(
                "attached editor requires the DAW owner window".into(),
            ));
        }
        Ok(())
    }
}

/// An open editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorInfo {
    /// The host's embedding parent: HWND, NSView pointer or X11 window id.
    /// `None` when the plugin made its own floating window.
    pub window: Option<usize>,
    /// Native API dimensions: physical pixels on Windows/X11, logical points
    /// on Cocoa. Zero when a floating plugin does not supply a size.
    pub width: u32,
    pub height: u32,
    /// The user can resize the window.
    pub resizable: bool,
}

/// Why an editor could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditorError {
    #[error("the plugin has no editor")]
    NoEditor,
    #[error("the plugin's editor is already open")]
    AlreadyOpen,
    #[error("the plugin editor does not support application scaling")]
    UnsupportedScale,
    #[error("application scale must be finite and between 75% and 200%")]
    InvalidScale,
    /// The plugin's parent API is not available on this target/window system.
    #[error("the plugin editor's native parent API is unsupported on this platform")]
    UnsupportedPlatform,
    #[error("the editor could not be opened: {0}")]
    Failed(String),
}

/// Handles the window messages waiting for this thread, then returns. With
/// a `wait`, it first sleeps until a message arrives or the time is up.
///
/// A program that has no event loop of its own calls this in a loop, with
/// [`PluginInstance::idle`](crate::PluginInstance::idle) in between. It
/// does nothing on platforms without a host window.
pub fn pump_events(wait: Option<Duration>) {
    #[cfg(windows)]
    win32::pump_events(wait);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native::pump_events(wait);
    #[cfg(target_os = "linux")]
    crate::vst3::pump_run_loops();
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    if let Some(wait) = wait {
        std::thread::sleep(wait);
    }
}
