//! Off-audio distributable bundle construction. Destination must be new; a
//! staged directory is published only after binary, state and metadata succeed.
use crate::Program;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use windfall_plugin_host::export_runtime::{self, Stamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    LinuxX64,
    MacUniversal,
    MacArm64,
    MacX64,
}
impl Platform {
    pub fn native() -> Result<Self, String> {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Ok(Self::WindowsX64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(Self::LinuxX64)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok(Self::MacArm64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Ok(Self::MacX64)
        } else {
            Err("No export bundle layout for this target architecture".into())
        }
    }
    fn vst_binary(self) -> &'static str {
        match self {
            Self::WindowsX64 => "Windfall.vst3/Contents/x86_64-win/Windfall.vst3",
            Self::LinuxX64 => "Windfall.vst3/Contents/x86_64-linux/Windfall.so",
            _ => "Windfall.vst3/Contents/MacOS/Windfall",
        }
    }
    fn clap_binary(self) -> &'static str {
        match self {
            Self::WindowsX64 | Self::LinuxX64 => "Windfall.clap",
            _ => "Windfall.clap/Contents/MacOS/Windfall",
        }
    }
    fn mac(self) -> bool {
        matches!(self, Self::MacUniversal | Self::MacArm64 | Self::MacX64)
    }
    fn helper_name(self) -> &'static str {
        if self == Self::WindowsX64 {
            "windfall-plugin-audio.exe"
        } else {
            "windfall-plugin-audio"
        }
    }
    fn stamp(self) -> Stamp {
        Stamp {
            abi: export_runtime::ABI,
            version: env!("CARGO_PKG_VERSION").into(),
            target: match self {
                Self::WindowsX64 => "x86_64-windows",
                Self::LinuxX64 => "x86_64-linux",
                Self::MacArm64 => "aarch64-macos",
                Self::MacX64 => "x86_64-macos",
                Self::MacUniversal => "universal-macos",
            }
            .into(),
        }
    }
}
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub fn export_bundle(
    program: &Program,
    binary: &Path,
    destination: &Path,
    platform: Platform,
) -> Result<(), String> {
    // A universal app resolves a fat runtime through its current native slice.
    // Preserve both slices' usable distribution stamp when the desktop exports.
    let platform = if platform.mac() {
        let mut magic = [0u8; 4];
        fs::File::open(binary)
            .and_then(|mut f| f.read_exact(&mut magic))
            .map_err(|e| format!("Windfall runtime is missing/unreadable: {e}"))?;
        if matches!(
            magic,
            [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
                | [0xca, 0xfe, 0xba, 0xbf]
                | [0xbf, 0xba, 0xfe, 0xca]
        ) {
            Platform::MacUniversal
        } else {
            platform
        }
    } else {
        platform
    };
    // Native probe verifies the actual exported C ABI before publishing bytes.
    let stamp = export_runtime::probe(binary, &platform.stamp())?;
    if platform == Platform::MacUniversal {
        let file = fs::File::open(binary).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let mut header = Vec::new();
        file.take(8 + 32 * 16)
            .read_to_end(&mut header)
            .map_err(|e| e.to_string())?;
        validate_universal_header(&header, length)?;
    }
    let manifest = if platform == Platform::MacUniversal {
        platform.stamp()
    } else {
        stamp
    };
    export_validated(program, binary, destination, platform, &manifest, true)
}
fn validate_universal_header(header: &[u8], length: u64) -> Result<(), String> {
    let refusal = "A universal macOS export requires valid x86_64 and arm64 Mach-O slices";
    let magic: [u8; 4] = header
        .get(..4)
        .ok_or(refusal)?
        .try_into()
        .map_err(|_| refusal)?;
    let (little, wide) = match magic {
        [0xca, 0xfe, 0xba, 0xbe] => (false, false),
        [0xbe, 0xba, 0xfe, 0xca] => (true, false),
        [0xca, 0xfe, 0xba, 0xbf] => (false, true),
        [0xbf, 0xba, 0xfe, 0xca] => (true, true),
        _ => return Err(refusal.into()),
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let bytes: [u8; 4] = header
            .get(at..at + 4)
            .ok_or(refusal)?
            .try_into()
            .map_err(|_| refusal)?;
        Ok(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let u64_at = |at: usize| -> Result<u64, String> {
        let bytes: [u8; 8] = header
            .get(at..at + 8)
            .ok_or(refusal)?
            .try_into()
            .map_err(|_| refusal)?;
        Ok(if little {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        })
    };
    let count = u32_at(4)? as usize;
    if !(2..=16).contains(&count) {
        return Err(refusal.into());
    }
    let stride = if wide { 32 } else { 20 };
    let table_end = 8 + count * stride;
    if header.len() < table_end {
        return Err(refusal.into());
    }
    let mut x64 = false;
    let mut arm = false;
    let mut spans = Vec::new();
    for index in 0..count {
        let at = 8 + index * stride;
        let cpu = u32_at(at)?;
        let offset = if wide {
            u64_at(at + 8)?
        } else {
            u64::from(u32_at(at + 8)?)
        };
        let size = if wide {
            u64_at(at + 16)?
        } else {
            u64::from(u32_at(at + 12)?)
        };
        let end = offset.checked_add(size).ok_or(refusal)?;
        if offset < table_end as u64
            || size == 0
            || end > length
            || spans
                .iter()
                .any(|&(start, stop)| offset < stop && start < end)
        {
            return Err(refusal.into());
        }
        spans.push((offset, end));
        match cpu {
            0x0100_0007 if !x64 => x64 = true,
            0x0100_000c if !arm => arm = true,
            0x0100_0007 | 0x0100_000c => return Err(refusal.into()),
            _ => {}
        }
    }
    if x64 && arm {
        Ok(())
    } else {
        Err(refusal.into())
    }
}
fn export_validated(
    program: &Program,
    binary: &Path,
    destination: &Path,
    platform: Platform,
    stamp: &Stamp,
    verify_copies: bool,
) -> Result<(), String> {
    let packet = program.encode()?;
    if destination.exists() {
        return Err("Plugin export destination already exists; choose a new folder".into());
    }
    if !binary.is_file() {
        return Err(format!(
            "Plugin export runtime is missing: {}",
            binary.display()
        ));
    }
    let parent = destination
        .parent()
        .filter(|p| p.is_dir())
        .ok_or("Plugin export parent must exist")?;
    let stage = parent.join(format!(
        ".windfall-plugin-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        for relative in [platform.vst_binary(), platform.clap_binary()] {
            let target = stage.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(binary, &target).map_err(|e| e.to_string())?;
            if verify_copies {
                export_runtime::probe(&target, stamp)?;
            }
        }
        let resources = stage.join("Windfall.vst3/Contents/Resources");
        fs::create_dir_all(&resources).map_err(|e| e.to_string())?;
        let manifest = serde_json::to_vec(stamp).map_err(|e| e.to_string())?;
        write(&resources.join("windfall-runtime.json"), &manifest)?;
        let helper = binary
            .parent()
            .ok_or("Runtime has no directory")?
            .join(platform.helper_name());
        if !program.project.plugins.is_empty() && !helper.is_file() {
            return Err(format!(
                "Nested CLAP/VST3 needs the distributed audio helper: {}",
                helper.display()
            ));
        }
        if helper.is_file() {
            // Helper remains next to each format's runtime, never next to a DAW executable.
            for relative in [platform.vst_binary(), platform.clap_binary()] {
                let directory = stage.join(relative).parent().unwrap().to_owned();
                let target = directory.join(platform.helper_name());
                if !target.exists() {
                    fs::copy(&helper, target).map_err(|e| e.to_string())?;
                }
            }
        }
        write(&resources.join("windfall-state.json"), &packet)?;
        let metadata = serde_json::json!({ "Name": "Windfall", "Version": env!("CARGO_PKG_VERSION"),
            "Factory Info": { "Vendor": "Windfall", "URL": "https://github.com/erivgout/windfall", "E-Mail": "", "Flags": { "Unicode": true } },
            "Classes": [{ "CID": crate::vst3::CLASS_ID, "Category": "Audio Module Class", "Name": "Windfall", "Vendor": "Windfall",
                "Version": env!("CARGO_PKG_VERSION"), "SDKVersion": "VST 3", "Sub Categories": ["Instrument", "Synth"], "Class Flags": 0, "Cardinality": 2147483647 }]
        });
        write(
            &resources.join("moduleinfo.json"),
            &serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )?;
        if platform.mac() {
            let clap_resources = stage.join("Windfall.clap/Contents/Resources");
            fs::create_dir_all(&clap_resources).map_err(|e| e.to_string())?;
            write(&clap_resources.join("windfall-state.json"), &packet)?;
            write(&clap_resources.join("windfall-runtime.json"), &manifest)?;
            for format in ["vst3", "clap"] {
                let numeric_version = env!("CARGO_PKG_VERSION")
                    .split('-')
                    .next()
                    .unwrap_or("0.1.0");
                let plist = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>org.windfall.program.{format}</string><key>CFBundleExecutable</key><string>Windfall</string><key>CFBundleName</key><string>Windfall</string><key>CFBundlePackageType</key><string>BNDL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>{numeric_version}</string></dict></plist>"
                );
                write(
                    &stage.join(format!("Windfall.{format}/Contents/Info.plist")),
                    plist.as_bytes(),
                )?;
                write(
                    &stage.join(format!("Windfall.{format}/Contents/PkgInfo")),
                    b"BNDL????",
                )?;
            }
        } else {
            write(&stage.join("windfall-state.json"), &packet)?;
            write(&stage.join("windfall-runtime.json"), &manifest)?;
        }
        write(&stage.join("README.txt"), b"Windfall plugin export\nThe format binaries contain the complete Windfall engine runtime, validated by ABI/version/architecture before export. Install Windfall.vst3 as a complete bundle. On Windows/Linux keep Windfall.clap, windfall-state.json, windfall-runtime.json and windfall-plugin-audio together; on macOS keep the complete CLAP bundle. Do not move individual bundle contents. Nested CLAP/VST3 own isolated helpers; install their original licensed plugin files at the saved paths on the receiving machine. Host transport, tempo and meter govern playback. 16 stereo outputs; MIDI channels 1-16 address rack rows. Signing/notarization is performed by the distributor.\n")?;
        // Recheck immediately before publication; never deliberately overwrite.
        if destination.exists() {
            return Err("Plugin export destination was created concurrently".into());
        }
        publish_new_directory(&stage, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
fn publish_new_directory(stage: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::{
            ffi::{CString, c_char},
            os::unix::ffi::OsStrExt,
        };
        let from = CString::new(stage.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let to = CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        {
            unsafe extern "C" {
                fn renameat2(
                    from_dir: i32,
                    from: *const c_char,
                    to_dir: i32,
                    to: *const c_char,
                    flags: u32,
                ) -> i32;
            }
            if unsafe { renameat2(-100, from.as_ptr(), -100, to.as_ptr(), 1) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> i32;
            }
            if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 4) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // Windows MoveFileEx without REPLACE_EXISTING refuses existing targets.
        fs::rename(stage, destination).map_err(|e| e.to_string())
    }
}
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
pub(crate) fn module_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::LibraryLoader::*;
        let mut module = std::ptr::null_mut();
        if unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                (&raw const crate::clap_entry).cast(),
                &mut module,
            )
        } == 0
        {
            return None;
        }
        let mut path = [0u16; 32768];
        let size =
            unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
        if size == 0 || size >= path.len() {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&path[..size])))
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::{CStr, c_char, c_void};
        #[repr(C)]
        struct Info {
            name: *const c_char,
            base: *mut c_void,
            symbol: *const c_char,
            address: *mut c_void,
        }
        #[cfg_attr(target_os = "linux", link(name = "dl"))]
        unsafe extern "C" {
            fn dladdr(address: *const c_void, info: *mut Info) -> i32;
        }
        let mut info: Info = unsafe { std::mem::zeroed() };
        if unsafe { dladdr((&raw const crate::clap_entry).cast(), &mut info) } == 0
            || info.name.is_null()
        {
            return None;
        }
        Some(PathBuf::from(
            unsafe { CStr::from_ptr(info.name) }
                .to_string_lossy()
                .into_owned(),
        ))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        None
    }
}
pub(crate) fn initial_program() -> Result<Program, String> {
    let Some(path) = module_path() else {
        return Ok(Program::default());
    };
    initial_program_at(&path, &Stamp::current())
}
fn initial_program_at(binary: &Path, expected: &Stamp) -> Result<Program, String> {
    let Some(parent) = binary.parent() else {
        return Ok(Program::default());
    };
    let bundled = parent
        .parent()
        .is_some_and(|p| p.file_name().is_some_and(|n| n == "Contents"));
    let resource = bundled.then(|| {
        parent
            .parent()
            .unwrap()
            .join("Resources/windfall-state.json")
    });
    let format_binary = binary
        .extension()
        .is_some_and(|e| e == "clap" || e == "vst3");
    for path in resource
        .into_iter()
        .chain([parent.join("windfall-state.json")])
    {
        let manifest = path.with_file_name("windfall-runtime.json");
        if bundled || format_binary || manifest.exists() || path.exists() {
            validate_manifest(&manifest, expected)?;
        }
        match fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(crate::MAX_STATE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                return Program::decode(&bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !manifest.exists() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "Windfall bundle program is missing: {}",
                    path.display()
                ));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(Program::default())
}
fn validate_manifest(path: &Path, expected: &Stamp) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| {
        format!(
            "Windfall bundle runtime stamp is missing/unreadable at {}: {e}",
            path.display()
        )
    })?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("Windfall bundle runtime stamp exceeds 4096 bytes".into());
    }
    let stamp: Stamp = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Windfall bundle runtime stamp is invalid: {e}"))?;
    stamp.validate(expected)
}
/// Bundle/application resources first, executable-adjacent distribution second.
/// Explicit overrides are checked by the same probe; no CWD or PATH fallback.
pub fn resolve_runtime(
    resource_directory: &Path,
    executable: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        return export_runtime::resolve(&[path.to_owned()]);
    }
    let name = export_runtime::filename();
    export_runtime::resolve(&[
        resource_directory.join("plugin-runtime").join(&name),
        executable
            .parent()
            .ok_or("Application executable has no directory")?
            .join("plugin-runtime")
            .join(&name),
        executable.with_file_name(name),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_package_layout_contains_loadable_state_and_factory_metadata() {
        for platform in [
            Platform::WindowsX64,
            Platform::LinuxX64,
            Platform::MacUniversal,
            Platform::MacArm64,
            Platform::MacX64,
        ] {
            let root = std::env::temp_dir().join(format!(
                "windfall-package-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::write(root.join("runtime"), b"test runtime bytes").unwrap();
            fs::write(root.join(platform.helper_name()), b"owned helper bytes").unwrap();
            let destination = root.join("package");
            let program = Program::default();
            // Layout construction is separate from the native ABI probe, which
            // the actual DLL integration test exercises. Inert bytes never pass export_bundle.
            export_validated(
                &program,
                &root.join("runtime"),
                &destination,
                platform,
                &platform.stamp(),
                false,
            )
            .unwrap();
            assert_eq!(
                fs::read(destination.join(platform.vst_binary())).unwrap(),
                b"test runtime bytes"
            );
            assert!(destination.join(platform.clap_binary()).is_file());
            for relative in [platform.vst_binary(), platform.clap_binary()] {
                assert_eq!(
                    fs::read(
                        destination
                            .join(relative)
                            .with_file_name(platform.helper_name())
                    )
                    .unwrap(),
                    b"owned helper bytes"
                );
            }
            let state =
                fs::read(destination.join("Windfall.vst3/Contents/Resources/windfall-state.json"))
                    .unwrap();
            assert_eq!(
                Program::decode(&state).unwrap().encode().unwrap(),
                program.encode().unwrap()
            );
            let metadata: serde_json::Value = serde_json::from_slice(
                &fs::read(destination.join("Windfall.vst3/Contents/Resources/moduleinfo.json"))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(metadata["Classes"][0]["CID"], crate::vst3::CLASS_ID);
            if platform.mac() {
                assert!(
                    destination
                        .join("Windfall.clap/Contents/Info.plist")
                        .is_file()
                );
            }
            assert!(
                export_bundle(&program, &root.join("runtime"), &destination, platform).is_err()
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn missing_runtime_never_publishes_or_overwrites_a_destination() {
        let root = std::env::temp_dir().join(format!(
            "windfall-package-missing-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        assert!(
            export_bundle(
                &Program::default(),
                &root.join("missing"),
                &root.join("out"),
                Platform::WindowsX64
            )
            .is_err()
        );
        assert!(!root.join("out").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn bundle_first_state_and_stamp_refuse_missing_malformed_abi_version_and_target() {
        let root = std::env::temp_dir().join(format!(
            "windfall-stamp-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let directory = root.join("Contents/x86_64-win");
        let resources = root.join("Contents/Resources");
        fs::create_dir_all(&directory).unwrap();
        fs::create_dir_all(&resources).unwrap();
        let state = resources.join("windfall-state.json");
        let manifest = resources.join("windfall-runtime.json");
        let binary = directory.join("Windfall.vst3");
        let expected = Platform::WindowsX64.stamp();
        fs::write(&state, Program::default().encode().unwrap()).unwrap();
        assert!(
            initial_program_at(&binary, &expected)
                .err()
                .unwrap()
                .contains("stamp is missing")
        );
        fs::write(&manifest, b"invalid").unwrap();
        assert!(
            initial_program_at(&binary, &expected)
                .err()
                .unwrap()
                .contains("stamp is invalid")
        );
        for at in 0..3 {
            let mut bad = expected.clone();
            match at {
                0 => bad.abi += 1,
                1 => bad.version.push_str("-bad"),
                _ => bad.target.push_str("-bad"),
            }
            fs::write(&manifest, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(
                initial_program_at(&binary, &expected)
                    .err()
                    .unwrap()
                    .contains("runtime mismatch")
            );
        }
        fs::write(&manifest, serde_json::to_vec(&expected).unwrap()).unwrap();
        fs::write(directory.join("windfall-state.json"), b"invalid fallback").unwrap();
        assert!(initial_program_at(&binary, &expected).is_ok());
        fs::remove_file(&state).unwrap();
        assert!(
            initial_program_at(&binary, &expected)
                .err()
                .unwrap()
                .contains("program is missing")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn automatic_lookup_reports_all_locations_and_never_uses_invalid_preferred_runtime() {
        let root = std::env::temp_dir().join(format!(
            "windfall-resolution-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let error = resolve_runtime(&root, &root.join("Windfall.exe"), None).unwrap_err();
        assert!(error.contains("plugin-runtime"));
        assert!(error.contains(&export_runtime::filename()));
        let override_path = root.join("not-a-runtime");
        fs::write(&override_path, b"not native code").unwrap();
        assert!(
            resolve_runtime(&root, &root.join("Windfall.exe"), Some(&override_path))
                .unwrap_err()
                .contains("Cannot load Windfall runtime")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn universal_layout_requires_both_checked_nonoverlapping_architecture_slices() {
        let mut header = vec![0u8; 48];
        header[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        header[4..8].copy_from_slice(&2u32.to_be_bytes());
        for (at, cpu, offset) in [(8, 0x0100_0007u32, 64u32), (28, 0x0100_000cu32, 96u32)] {
            header[at..at + 4].copy_from_slice(&cpu.to_be_bytes());
            header[at + 8..at + 12].copy_from_slice(&offset.to_be_bytes());
            header[at + 12..at + 16].copy_from_slice(&32u32.to_be_bytes());
        }
        assert!(validate_universal_header(&header, 128).is_ok());
        assert!(validate_universal_header(&header[..47], 128).is_err());
        assert!(validate_universal_header(&header, 127).is_err());
        let mut bad = header.clone();
        bad[28..32].copy_from_slice(&0x0100_0007u32.to_be_bytes());
        assert!(validate_universal_header(&bad, 128).is_err());
        bad = header;
        bad[36..40].copy_from_slice(&80u32.to_be_bytes());
        assert!(validate_universal_header(&bad, 128).is_err());
    }
}
