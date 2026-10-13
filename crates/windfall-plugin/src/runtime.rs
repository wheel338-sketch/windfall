//! The plugin binary IS the engine runtime; there is no desktop/device DLL.
//! Export a stable C stamp and bounded error copy for loaders before factory use.
use std::{ffi::c_char, sync::Mutex};
static ERROR: Mutex<String> = Mutex::new(String::new());
pub(crate) fn error(message: impl Into<String>) {
    *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = message.into();
}
#[unsafe(no_mangle)]
pub extern "C" fn windfall_runtime_stamp() -> *const c_char {
    macro_rules! stamp { ($target:literal) => { concat!("{\"abi\":1,\"version\":\"", env!("CARGO_PKG_VERSION"), "\",\"target\":\"", $target, "\"}\0") }; }
    // Static bytes avoid leaving heap storage behind when a probe unloads DLLs.
    const STAMP: &str = if cfg!(all(windows, target_arch = "x86_64")) { stamp!("x86_64-windows") }
        else if cfg!(all(target_os = "linux", target_arch = "x86_64")) { stamp!("x86_64-linux") }
        else if cfg!(all(target_os = "macos", target_arch = "aarch64")) { stamp!("aarch64-macos") }
        else if cfg!(all(target_os = "macos", target_arch = "x86_64")) { stamp!("x86_64-macos") }
        else { stamp!("unsupported") };
    STAMP.as_ptr().cast()
}
#[unsafe(no_mangle)]
/// # Safety
/// `output` must address `capacity` writable bytes, valid for this control-side call.
pub unsafe extern "C" fn windfall_runtime_error(output: *mut c_char, capacity: usize) -> usize {
    if output.is_null() || capacity == 0 { return 0; }
    let message = ERROR.lock().unwrap_or_else(|e| e.into_inner());
    let count = message.len().min(capacity - 1);
    unsafe { std::ptr::copy_nonoverlapping(message.as_ptr(), output.cast(), count);
        output.add(count).write(0); }
    count
}
