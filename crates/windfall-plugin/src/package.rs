//! Off-audio distributable bundle construction. Destination must be new; a
//! staged directory is published only after binary, state and metadata succeed.
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
use crate::Program;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform { WindowsX64, LinuxX64, MacUniversal, MacArm64, MacX64 }
impl Platform {
    pub fn native() -> Result<Self, String> {
        if cfg!(all(windows, target_arch = "x86_64")) { Ok(Self::WindowsX64) }
        else if cfg!(all(target_os = "linux", target_arch = "x86_64")) { Ok(Self::LinuxX64) }
        else if cfg!(all(target_os = "macos", target_arch = "aarch64")) { Ok(Self::MacArm64) }
        else if cfg!(all(target_os = "macos", target_arch = "x86_64")) { Ok(Self::MacX64) }
        else { Err("No export bundle layout for this target architecture".into()) }
    }
    fn vst_binary(self) -> &'static str { match self {
        Self::WindowsX64 => "Windfall.vst3/Contents/x86_64-win/Windfall.vst3",
        Self::LinuxX64 => "Windfall.vst3/Contents/x86_64-linux/Windfall.so",
        _ => "Windfall.vst3/Contents/MacOS/Windfall",
    }}
    fn clap_binary(self) -> &'static str { match self {
        Self::WindowsX64 | Self::LinuxX64 => "Windfall.clap",
        _ => "Windfall.clap/Contents/MacOS/Windfall",
    }}
    fn mac(self) -> bool { matches!(self, Self::MacUniversal | Self::MacArm64 | Self::MacX64) }
}
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub fn export_bundle(program: &Program, binary: &Path, destination: &Path, platform: Platform) -> Result<(), String> {
    let packet = program.encode()?;
    if destination.exists() { return Err("Plugin export destination already exists; choose a new folder".into()); }
    if !binary.is_file() { return Err(format!("Plugin export runtime is missing: {}", binary.display())); }
    let parent = destination.parent().filter(|p| p.is_dir()).ok_or("Plugin export parent must exist")?;
    let stage = parent.join(format!(".windfall-plugin-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        for relative in [platform.vst_binary(), platform.clap_binary()] {
            let target = stage.join(relative);
            if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            fs::copy(binary, target).map_err(|e| e.to_string())?;
        }
        let resources = stage.join("Windfall.vst3/Contents/Resources");
        fs::create_dir_all(&resources).map_err(|e| e.to_string())?;
        write(&resources.join("windfall-state.json"), &packet)?;
        let metadata = serde_json::json!({ "Name": "Windfall", "Version": env!("CARGO_PKG_VERSION"),
            "Factory Info": { "Vendor": "Windfall", "URL": "https://github.com/erivgout/windfall", "E-Mail": "", "Flags": { "Unicode": true } },
            "Classes": [{ "CID": crate::vst3::CLASS_ID, "Category": "Audio Module Class", "Name": "Windfall", "Vendor": "Windfall",
                "Version": env!("CARGO_PKG_VERSION"), "SDKVersion": "VST 3", "Sub Categories": ["Instrument", "Synth"], "Class Flags": 0, "Cardinality": 2147483647 }]
        });
        write(&resources.join("moduleinfo.json"), &serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?)?;
        if platform.mac() {
            let clap_resources = stage.join("Windfall.clap/Contents/Resources");
            fs::create_dir_all(&clap_resources).map_err(|e| e.to_string())?;
            write(&clap_resources.join("windfall-state.json"), &packet)?;
            for format in ["vst3", "clap"] {
                let numeric_version = env!("CARGO_PKG_VERSION").split('-').next().unwrap_or("0.1.0");
                let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>org.windfall.program.{format}</string><key>CFBundleExecutable</key><string>Windfall</string><key>CFBundleName</key><string>Windfall</string><key>CFBundlePackageType</key><string>BNDL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>{numeric_version}</string></dict></plist>");
                write(&stage.join(format!("Windfall.{format}/Contents/Info.plist")), plist.as_bytes())?;
                write(&stage.join(format!("Windfall.{format}/Contents/PkgInfo")), b"BNDL????")?;
            }
        } else { write(&stage.join("windfall-state.json"), &packet)?; }
        write(&stage.join("README.txt"), b"Windfall plugin export\nInstall Windfall.vst3 as a complete bundle. Install Windfall.clap together with windfall-state.json (on macOS the state is inside the bundle). Do not move or rename individual bundle contents. Host transport, tempo and time signature govern song playback; internal clock/navigation automation is retained in state and suspended. 16 stereo output buses, MIDI channels 1-16 address rack rows. Program state can also be loaded from the plugin control panel. Signing/notarization is performed by the distributor.\n")?;
        // Recheck immediately before publication; never deliberately overwrite.
        if destination.exists() { return Err("Plugin export destination was created concurrently".into()); }
        publish_new_directory(&stage, destination)
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&stage); }
    result
}
fn publish_new_directory(stage: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))] {
        use std::{ffi::{CString, c_char}, os::unix::ffi::OsStrExt};
        let from = CString::new(stage.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let to = CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")] {
            unsafe extern "C" { fn renameat2(from_dir: i32, from: *const c_char, to_dir: i32, to: *const c_char, flags: u32) -> i32; }
            if unsafe { renameat2(-100, from.as_ptr(), -100, to.as_ptr(), 1) } != 0 { return Err(std::io::Error::last_os_error().to_string()); }
        }
        #[cfg(target_os = "macos")] {
            unsafe extern "C" { fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> i32; }
            if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 4) } != 0 { return Err(std::io::Error::last_os_error().to_string()); }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))] {
        // Windows MoveFileEx without REPLACE_EXISTING refuses existing targets.
        fs::rename(stage, destination).map_err(|e| e.to_string())
    }
}
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())
}
fn module_path() -> Option<PathBuf> {
    #[cfg(windows)] {
        use windows_sys::Win32::System::LibraryLoader::*;
        let mut module = std::ptr::null_mut();
        if unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, (&raw const crate::clap_entry).cast(), &mut module) } == 0 { return None; }
        let mut path = [0u16; 32768];
        let size = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
        if size == 0 || size >= path.len() { return None; }
        Some(PathBuf::from(String::from_utf16_lossy(&path[..size])))
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))] {
        use std::ffi::{c_char, c_void, CStr};
        #[repr(C)] struct Info { name: *const c_char, base: *mut c_void, symbol: *const c_char, address: *mut c_void }
        #[cfg_attr(target_os = "linux", link(name = "dl"))]
        unsafe extern "C" { fn dladdr(address: *const c_void, info: *mut Info) -> i32; }
        let mut info: Info = unsafe { std::mem::zeroed() };
        if unsafe { dladdr((&raw const crate::clap_entry).cast(), &mut info) } == 0 || info.name.is_null() { return None; }
        Some(PathBuf::from(unsafe { CStr::from_ptr(info.name) }.to_string_lossy().into_owned()))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))] { None }
}
pub(crate) fn initial_program() -> Result<Program, String> {
    let Some(path) = module_path() else { return Ok(Program::default()); };
    let Some(parent) = path.parent() else { return Ok(Program::default()); };
    let resource = parent.parent().map(|p| p.join("Resources/windfall-state.json"));
    for path in resource.into_iter().chain([parent.join("windfall-state.json")]) {
        match fs::File::open(&path) {
            Ok(file) => { let mut bytes = Vec::new(); file.take(crate::MAX_STATE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?; return Program::decode(&bytes); }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error.to_string()),
        }
    } Ok(Program::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_package_layout_contains_loadable_state_and_factory_metadata() {
        for platform in [Platform::WindowsX64, Platform::LinuxX64, Platform::MacUniversal, Platform::MacArm64, Platform::MacX64] {
            let root = std::env::temp_dir().join(format!("windfall-package-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap(); fs::write(root.join("runtime"), b"test runtime bytes").unwrap();
            let destination = root.join("package"); let program = Program::default();
            export_bundle(&program, &root.join("runtime"), &destination, platform).unwrap();
            assert_eq!(fs::read(destination.join(platform.vst_binary())).unwrap(), b"test runtime bytes");
            assert!(destination.join(platform.clap_binary()).is_file());
            let state = fs::read(destination.join("Windfall.vst3/Contents/Resources/windfall-state.json")).unwrap();
            assert_eq!(Program::decode(&state).unwrap().encode().unwrap(), program.encode().unwrap());
            let metadata: serde_json::Value = serde_json::from_slice(&fs::read(destination.join("Windfall.vst3/Contents/Resources/moduleinfo.json")).unwrap()).unwrap();
            assert_eq!(metadata["Classes"][0]["CID"], crate::vst3::CLASS_ID);
            if platform.mac() { assert!(destination.join("Windfall.clap/Contents/Info.plist").is_file()); }
            assert!(export_bundle(&program, &root.join("runtime"), &destination, platform).is_err());
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn missing_runtime_never_publishes_or_overwrites_a_destination() {
        let root = std::env::temp_dir().join(format!("windfall-package-missing-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        assert!(export_bundle(&Program::default(), &root.join("missing"), &root.join("out"), Platform::WindowsX64).is_err());
        assert!(!root.join("out").exists()); assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
}
