//! Standalone helper for process integration checks. Packaged desktop uses its
//! own executable's identical helper entry, once production wiring is accepted.
fn main() {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    if windfall_plugin_host::bridge::helper::entry() {
        return;
    }
    std::process::exit(64);
}
