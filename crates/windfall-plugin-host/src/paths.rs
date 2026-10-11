//! Where plugins are installed, and finding the plugin files in a set of
//! folders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::descriptor::PluginFormat;

/// The operating system whose folder rules to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    /// The platform this build runs on. Anything that is neither Windows
    /// nor macOS follows the Linux rules.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }

    /// What separates the folders in a variable such as `CLAP_PATH`.
    const fn list_separator(self) -> char {
        match self {
            Self::Windows => ';',
            Self::MacOs | Self::Linux => ':',
        }
    }
}

/// The folders the CLAP and VST3 specifications say plugins of `format` are
/// installed in on this machine, most specific first. Folders that do not
/// exist are still listed.
pub fn standard_folders(format: PluginFormat) -> Vec<PathBuf> {
    standard_folders_on(Platform::current(), format, &|name| {
        std::env::var_os(name).map(PathBuf::from)
    })
}

/// [`standard_folders`] for any platform, reading environment variables
/// through `env`.
pub fn standard_folders_on(
    platform: Platform,
    format: PluginFormat,
    env: &dyn Fn(&str) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    let name = match format {
        PluginFormat::Clap => "CLAP",
        PluginFormat::Vst3 => "VST3",
        PluginFormat::Au => "Components",
    };
    let mut folders = Vec::new();

    // CLAP lets the user name more folders. They come first.
    if format == PluginFormat::Clap
        && let Some(list) = env("CLAP_PATH")
    {
        let list = list.to_string_lossy().into_owned();
        folders.extend(
            list.split(platform.list_separator())
                .filter(|folder| !folder.is_empty())
                .map(PathBuf::from),
        );
    }

    match platform {
        Platform::Windows => {
            if let Some(local) = env("LOCALAPPDATA") {
                folders.push(local.join("Programs").join("Common").join(name));
            }
            if let Some(common) = env("COMMONPROGRAMFILES") {
                folders.push(common.join(name));
            }
        }
        Platform::MacOs => {
            if let Some(home) = env("HOME") {
                folders.push(home.join("Library/Audio/Plug-Ins").join(name));
            }
            folders.push(Path::new("/Library/Audio/Plug-Ins").join(name));
        }
        Platform::Linux => {
            let hidden = format!(".{}", name.to_lowercase());
            if let Some(home) = env("HOME") {
                folders.push(home.join(hidden));
            }
            folders.push(Path::new("/usr/lib").join(name.to_lowercase()));
            if format == PluginFormat::Vst3 {
                folders.push(PathBuf::from("/usr/local/lib/vst3"));
            }
        }
    }

    let mut seen = BTreeSet::new();
    folders.retain(|folder| seen.insert(folder.clone()));
    folders
}

/// A plugin file found on disk.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginFile {
    pub path: PathBuf,
    pub format: PluginFormat,
}

/// How deep [`find_plugins`] follows folders. Vendors nest a level or two.
const MAX_DEPTH: u32 = 8;

/// Every plugin file in `folders` and the folders inside them, sorted by
/// path, each listed once.
///
/// A `.clap` or `.vst3` entry is a plugin whether it is a file or, as on
/// macOS and for VST3 bundles, a folder. Nothing inside it is looked at.
pub fn find_plugins(folders: &[PathBuf]) -> Vec<PluginFile> {
    let mut found = BTreeSet::new();
    for folder in folders {
        walk(folder, 0, &mut found);
    }
    found.into_iter().collect()
}

fn walk(folder: &Path, depth: u32, found: &mut BTreeSet<PluginFile>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(format) = PluginFormat::of(&path) {
            found.insert(PluginFile { path, format });
        } else if depth < MAX_DEPTH && path.is_dir() {
            walk(&path, depth + 1, found);
        }
    }
}

/// The folder inside a VST3 bundle that holds the binary for this machine.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn vst3_architecture_folder() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "x86_64-win"
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        "arm64-win"
    } else if cfg!(all(windows, target_arch = "x86")) {
        "x86-win"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else {
        "x86_64-linux"
    }
}

/// The library inside a VST3 bundle. On macOS, fresh Info.plist metadata
/// selects CFBundleExecutable. Other platforms also accept a single file,
/// as old Windows plugins use, and retain their existing bundle naming rules.
pub fn vst3_binary(path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        crate::vst3::bundle_macos::resolve_source(path)
            .ok()
            .map(|source| source.binary)
    }
    #[cfg(not(target_os = "macos"))]
    {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        let stem = path.file_stem()?;
        let folder = path.join("Contents").join(vst3_architecture_folder());
        let mut name = stem.to_os_string();
        if cfg!(windows) {
            name.push(".vst3");
        } else if !cfg!(target_os = "macos") {
            name.push(".so");
        }
        let binary = folder.join(name);
        binary.is_file().then_some(binary)
    }
}

/// The file whose size and date say whether a plugin has changed: the
/// plugin file itself, or the binary inside a bundle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginFileIdentity {
    pub binary: PathBuf,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

/// Resolves the same VST3 binary used by loading. Missing bundle binaries
/// cannot fall back to stamping a directory. Canonical path, size and date
/// together identify the approved source, including symlink retargeting.
pub fn plugin_file_identity(path: &Path) -> std::io::Result<PluginFileIdentity> {
    let binary = if PluginFormat::of(path) == Some(PluginFormat::Vst3) {
        vst3_binary(path).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "VST3 bundle binary is missing",
            )
        })?
    } else {
        path.to_path_buf()
    };
    let binary = std::fs::canonicalize(binary)?;
    let metadata = std::fs::metadata(&binary)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Plugin binary must be a file",
        ));
    }
    Ok(PluginFileIdentity {
        binary,
        size: metadata.len(),
        modified: metadata.modified()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<PathBuf> {
        match name {
            "COMMONPROGRAMFILES" => Some(PathBuf::from("C:/Program Files/Common Files")),
            "LOCALAPPDATA" => Some(PathBuf::from("C:/Users/me/AppData/Local")),
            "HOME" => Some(PathBuf::from("/home/me")),
            "CLAP_PATH" => Some(PathBuf::from("/extra/one:/extra/two")),
            _ => None,
        }
    }

    #[test]
    fn windows_folders_follow_the_specifications() {
        let none = |name: &str| env(name).filter(|_| name != "CLAP_PATH");
        assert_eq!(
            standard_folders_on(Platform::Windows, PluginFormat::Clap, &none),
            [
                PathBuf::from("C:/Users/me/AppData/Local/Programs/Common/CLAP"),
                PathBuf::from("C:/Program Files/Common Files/CLAP"),
            ]
        );
        assert_eq!(
            standard_folders_on(Platform::Windows, PluginFormat::Vst3, &none),
            [
                PathBuf::from("C:/Users/me/AppData/Local/Programs/Common/VST3"),
                PathBuf::from("C:/Program Files/Common Files/VST3"),
            ]
        );
    }

    #[test]
    fn unix_folders_follow_the_specifications() {
        assert_eq!(
            standard_folders_on(Platform::Linux, PluginFormat::Clap, &env),
            [
                PathBuf::from("/extra/one"),
                PathBuf::from("/extra/two"),
                PathBuf::from("/home/me/.clap"),
                PathBuf::from("/usr/lib/clap"),
            ]
        );
        assert_eq!(
            standard_folders_on(Platform::Linux, PluginFormat::Vst3, &env),
            [
                PathBuf::from("/home/me/.vst3"),
                PathBuf::from("/usr/lib/vst3"),
                PathBuf::from("/usr/local/lib/vst3"),
            ]
        );
        assert_eq!(
            standard_folders_on(Platform::MacOs, PluginFormat::Vst3, &env),
            [
                PathBuf::from("/home/me/Library/Audio/Plug-Ins/VST3"),
                PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
            ]
        );
    }

    #[test]
    fn clap_path_uses_the_platform_separator_and_only_applies_to_clap() {
        let windows = |name: &str| (name == "CLAP_PATH").then(|| PathBuf::from("D:/a;E:/b;"));
        assert_eq!(
            standard_folders_on(Platform::Windows, PluginFormat::Clap, &windows),
            [PathBuf::from("D:/a"), PathBuf::from("E:/b")]
        );
        assert!(standard_folders_on(Platform::Windows, PluginFormat::Vst3, &windows).is_empty());
    }

    #[test]
    fn plugins_are_found_in_nested_folders_and_bundles_are_not_entered() {
        let root = std::env::temp_dir().join(format!("windfall-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let vendor = root.join("Vendor");
        let bundle = root.join("Bundle.vst3").join("Contents");
        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(vendor.join("b.clap"), b"").unwrap();
        std::fs::write(root.join("A.CLAP"), b"").unwrap();
        std::fs::write(root.join("notes.txt"), b"").unwrap();
        std::fs::write(bundle.join("inner.vst3"), b"").unwrap();

        let found = find_plugins(&[root.clone(), root.clone()]);
        let names: Vec<_> = found
            .iter()
            .map(|file| {
                let name = file.path.strip_prefix(&root).unwrap();
                (name.to_string_lossy().replace('\\', "/"), file.format)
            })
            .collect();
        assert_eq!(
            names,
            [
                ("A.CLAP".to_owned(), PluginFormat::Clap),
                ("Bundle.vst3".to_owned(), PluginFormat::Vst3),
                ("Vendor/b.clap".to_owned(), PluginFormat::Clap),
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
