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
//! The host window is implemented for Windows. On macOS and Linux only
//! plugins that open their own window work so far. The pieces to fill in
//! are a `HostWindow` for Cocoa and for X11 with the same four operations
//! as the one in `win32.rs`.
//!
//! # The event loop
//!
//! Windows of every editor belong to the thread that opened them, and that
//! thread has to run a message loop. In the app that is the shell's own
//! loop. A program without one can use [`pump_events`].

use std::time::Duration;

#[cfg(windows)]
pub(crate) mod win32;

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
    /// Where to put the window's top left corner on the screen, in pixels.
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
    /// The host's window around the editor, as the platform's handle (an
    /// `HWND` on Windows). `None` when the plugin made its own window.
    pub window: Option<usize>,
    /// The size of the editor in pixels. Zero when the plugin made its own
    /// window and does not say.
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
    /// The plugin only embeds its editor in a host window, and this
    /// platform's host window is not written yet.
    #[error("editors that need a host window are not supported on this platform yet")]
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
    #[cfg(not(windows))]
    if let Some(wait) = wait {
        std::thread::sleep(wait);
    }
}
