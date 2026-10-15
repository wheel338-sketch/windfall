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
            return Err(windfall_core::notice!(
                "pluginHost.runtimeMismatch",
                expectedAbi = expected.abi,
                expectedVersion = expected.version,
                expectedTarget = expected.target,
                foundAbi = self.abi,
                foundVersion = self.version,
                foundTarget = self.target
            )
            .to_string());
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
    Err(windfall_core::notice!(
        "pluginHost.runtimeNotFound",
        searched = locations
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
    .to_string())
}
pub fn probe(path: &Path, expected: &Stamp) -> Result<Stamp, String> {
    if !path.is_file() {
        return Err(
            windfall_core::notice!("pluginHost.runtimeMissing", path = path.display()).to_string(),
        );
    }
    #[cfg(target_os = "macos")]
    unchanged_since_loaded(path)?;
    // SAFETY: an export runtime is native code, exactly like a scanned plugin.
    // Keep the library alive until its version string has been copied.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| {
        windfall_core::notice!(
            "pluginHost.runtimeLoadFailed",
            path = path.display(),
            error = e.to_string()
        )
        .to_string()
    })?;
    let stamp = unsafe { library.get::<unsafe extern "C" fn() -> *const c_char>(SYMBOL) }.map_err(
        |_| windfall_core::notice!("pluginHost.runtimeNoStamp", path = path.display()).to_string(),
    )?;
    let pointer = unsafe { stamp() };
    unsafe { read_stamp(pointer, expected) }
}
/// macOS cannot unload a Rust dylib, and loading a path again returns the
/// image already loaded from it, whatever the file holds now. So the first
/// load of each path records the file's fingerprint, and a later probe of a
/// file that has changed since is refused rather than answered with the old
/// image: the replacement can only be used after a restart.
#[cfg(target_os = "macos")]
fn unchanged_since_loaded(path: &Path) -> Result<(), String> {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::{Mutex, OnceLock};
    static LOADED: OnceLock<Mutex<HashMap<std::path::PathBuf, u64>>> = OnceLock::new();
    let bytes = std::fs::read(path).map_err(|e| {
        windfall_core::notice!(
            "pluginHost.runtimeLoadFailed",
            path = path.display(),
            error = e.to_string()
        )
        .to_string()
    })?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let fingerprint = hasher.finish();
    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let mut loaded = LOADED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match loaded.get(&key) {
        Some(previous) if *previous != fingerprint => Err(windfall_core::notice!(
            "pluginHost.runtimeReplaced",
            path = path.display()
        )
        .to_string()),
        Some(_) => Ok(()),
        None => {
            loaded.insert(key, fingerprint);
            Ok(())
        }
    }
}
unsafe fn read_stamp(pointer: *const c_char, expected: &Stamp) -> Result<Stamp, String> {
    if pointer.is_null() {
        return Err("Windfall runtime has a null ABI stamp".into());
    }
    let value: Stamp = serde_json::from_slice(unsafe { CStr::from_ptr(pointer) }.to_bytes())
        .map_err(|e| {
            windfall_core::notice!("pluginHost.runtimeBadStamp", error = e.to_string()).to_string()
        })?;
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
                    .map_err(|error| windfall_core::english::english(&error))
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
                    .map_err(|error| windfall_core::english::english(&error))
                    .unwrap_err()
                    .contains("invalid ABI stamp")
            );
        }
    }
}
