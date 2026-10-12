//! Native mappings share the exact word ABI; construction/retirement is control-only.
#[cfg(windows)]
#[path = "mapping_windows.rs"]
mod platform;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "mapping_unix.rs"]
mod platform;
pub use platform::Mapping;

/// Stable validated launch names, also tested on Windows. Unix uses a private
/// directory instead of the Win32 Local namespace; never accept path fragments.
pub(crate) fn session_suffix(name: &str) -> std::io::Result<&str> {
    let suffix = name.strip_prefix("Local\\Windfall-Audio-").ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid bridge mapping name"))?;
    if suffix.len() != 33 || suffix.as_bytes()[16] != b'-'
        || suffix.bytes().enumerate().any(|(index, byte)| index != 16 && !byte.is_ascii_hexdigit()) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid bridge session suffix"));
    }
    Ok(suffix)
}
#[cfg(test)]
mod tests {
    #[test]
    fn session_mapping_names_cannot_select_files_or_traverse_directories() {
        assert_eq!(super::session_suffix("Local\\Windfall-Audio-0123456789abcdef-fedcba9876543210").unwrap(), "0123456789abcdef-fedcba9876543210");
        for suffix in ["../private", "0123456789abcdef-../../etc/passwd", "0123456789abcdef-fedcba9876543210\0", "0123456789abcdeg-fedcba9876543210"] {
            assert!(super::session_suffix(&format!("Local\\Windfall-Audio-{suffix}")).is_err());
        }
    }
}
