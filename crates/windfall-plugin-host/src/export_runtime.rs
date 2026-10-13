//! Versioned, self-contained Windfall export runtime. Probes run off audio.
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, c_char},
    path::{Path, PathBuf},
};

pub const ABI: u32 = 1;
pub const SYMBOL: &[u8] = b"windfall_runtime_stamp\0";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub abi: u32,
    pub version: String,
    pub target: String,
}
impl Stamp {
    pub fn current() -> Self {
        Self {
            abi: ABI,
            version: env!("CARGO_PKG_VERSION").into(),
            target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        }
    }
    pub fn validate(&self, expected: &Self) -> Result<(), String> {
        let target_matches = self.target == expected.target
            || (self.target == "universal-macos"
                && matches!(expected.target.as_str(), "x86_64-macos" | "aarch64-macos"))
            || (expected.target == "universal-macos"
                && matches!(self.target.as_str(), "x86_64-macos" | "aarch64-macos"));
        if self.abi != expected.abi || self.version != expected.version || !target_matches {
            return Err(format!(
                "Windfall runtime mismatch: expected ABI {} version {} target {}, found ABI {} version {} target {}",
                expected.abi,
                expected.version,
                expected.target,
                self.abi,
                self.version,
                self.target
            ));
        }
        Ok(())
    }
}
pub fn filename() -> String {
    format!(
        "{}windfall_plugin{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}
pub fn helper_filename() -> String {
    format!("windfall-plugin-audio{}", std::env::consts::EXE_SUFFIX)
}
pub fn active_helpers(path: &Path) -> Result<u64, String> {
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    let read =
        unsafe { library.get::<unsafe extern "C" fn() -> u64>(b"windfall_runtime_helpers\0") }
            .map_err(|e| e.to_string())?;
    Ok(unsafe { read() })
}
/// Ordered lookup, never CWD/PATH. An invalid preferred file is a refusal,
/// rather than silently selecting a different runtime farther down the list.
pub fn resolve(locations: &[PathBuf]) -> Result<PathBuf, String> {
    for path in locations {
        if path.exists() {
            probe(path, &Stamp::current())?;
            return Ok(path.clone());
        }
    }
    Err(format!(
        "Windfall runtime is missing; searched {}",
        locations
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
pub fn probe(path: &Path, expected: &Stamp) -> Result<Stamp, String> {
    if !path.is_file() {
        return Err(format!("Windfall runtime is missing: {}", path.display()));
    }
    // SAFETY: an export runtime is native code, exactly like a scanned plugin.
    // Keep the library alive until its version string has been copied.
    let library = unsafe { libloading::Library::new(path) }
        .map_err(|e| format!("Cannot load Windfall runtime {}: {e}", path.display()))?;
    let stamp = unsafe { library.get::<unsafe extern "C" fn() -> *const c_char>(SYMBOL) }
        .map_err(|_| format!("Windfall runtime {} has no ABI stamp", path.display()))?;
    let pointer = unsafe { stamp() };
    unsafe { read_stamp(pointer, expected) }
}
unsafe fn read_stamp(pointer: *const c_char, expected: &Stamp) -> Result<Stamp, String> {
    if pointer.is_null() {
        return Err("Windfall runtime has a null ABI stamp".into());
    }
    let value: Stamp = serde_json::from_slice(unsafe { CStr::from_ptr(pointer) }.to_bytes())
        .map_err(|e| format!("Windfall runtime has an invalid ABI stamp: {e}"))?;
    value.validate(expected)?;
    Ok(value)
}
/// Used after a factory refusal, while the format module still owns its DLL.
pub(crate) fn last_error(path: &Path) -> Option<String> {
    let binary = if path.is_dir() {
        if path.extension().is_some_and(|e| e == "clap") {
            path.join("Contents/MacOS/Windfall")
        } else {
            crate::paths::vst3_binary(path)?
        }
    } else {
        path.to_owned()
    };
    let library = unsafe { libloading::Library::new(binary) }.ok()?;
    let read = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char, usize) -> usize>(b"windfall_runtime_error\0")
    }
    .ok()?;
    let mut bytes = vec![0u8; 8192];
    let size = unsafe { read(bytes.as_mut_ptr().cast(), bytes.len()) };
    (size > 0 && size < bytes.len()).then(|| String::from_utf8_lossy(&bytes[..size]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_stamp_refuses_abi_version_target_and_unknown_fields() {
        let expected = Stamp::current();
        for mut bad in [expected.clone(), expected.clone(), expected.clone()]
            .into_iter()
            .enumerate()
        {
            match bad.0 {
                0 => bad.1.abi += 1,
                1 => bad.1.version.push_str("-old"),
                _ => bad.1.target.push_str("-other"),
            }
            assert!(
                bad.1
                    .validate(&expected)
                    .unwrap_err()
                    .contains("runtime mismatch")
            );
        }
        assert!(
            serde_json::from_str::<Stamp>(r#"{"abi":1,"version":"x","target":"x","extra":1}"#)
                .is_err()
        );
    }
    #[test]
    fn abi_entry_refuses_null_malformed_and_missing_stamp_fields() {
        let expected = Stamp::current();
        assert!(
            unsafe { read_stamp(std::ptr::null(), &expected) }
                .unwrap_err()
                .contains("null ABI stamp")
        );
        for text in [
            c"not JSON",
            c"{}",
            c"{\"abi\":1,\"version\":\"x\",\"target\":\"x\",\"unknown\":0}",
        ] {
            assert!(
                unsafe { read_stamp(text.as_ptr(), &expected) }
                    .unwrap_err()
                    .contains("invalid ABI stamp")
            );
        }
    }
}
