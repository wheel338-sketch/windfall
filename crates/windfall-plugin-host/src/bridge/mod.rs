//! Windows per-instance audio/state crash containment. Native code in helpers
//! has the user's permissions; this is not a security sandbox.

pub mod adapter;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod auth;
pub mod control;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod helper;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod mapping;
pub mod protocol;
pub mod retry;
pub mod slots;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod supervisor;
