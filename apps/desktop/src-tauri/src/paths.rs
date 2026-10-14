//! Paths as the shell compares them.

use std::path::{Component, Path, PathBuf};

/// Writes a path the one way the shell compares paths: without `.` and `..`
/// parts, without a trailing separator, and without Windows' `\\?\` prefix.
/// The disk is not touched, so links are not followed.
///
/// Windows takes `..` in a `\\?\` path as the name of a folder, so such a
/// path, which nothing sensible produces, keeps its prefix.
pub fn clean(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in dunce::simplified(path).components() {
        match component {
            Component::CurDir => {}
            // At the root there is nothing to pop, and `..` stays there.
            Component::ParentDir => {
                cleaned.pop();
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

/// The cleaned form of a path the UI sent, which must be a full path:
/// a relative one would silently depend on the folder the app was started
/// from.
pub fn absolute(path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    if path.trim().is_empty() || !given.is_absolute() {
        return Err(windfall_ipc::ui_message(
            "paths.notFull",
            serde_json::json!({"path": path}),
        ));
    }
    Ok(clean(given))
}

/// A file or folder name without its extension, for display.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .or(path.file_name())
        .map_or_else(|| display(path), |name| name.to_string_lossy().into_owned())
}

/// A file or folder name with its extension, for messages.
pub fn name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| display(path), |name| name.to_string_lossy().into_owned())
}

/// The path as text for the UI and for messages.
pub fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Whether two cleaned paths name the same place. Windows ignores letter
/// case in file names.
pub fn same(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_parts_and_trailing_separators_are_removed() {
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        let messy = Path::new(root)
            .join("music")
            .join(".")
            .join("kits")
            .join("..")
            .join("loops")
            .join("");
        assert_eq!(clean(&messy), Path::new(root).join("music").join("loops"));
        assert_eq!(clean(&Path::new(root).join("..")), Path::new(root));
    }

    #[cfg(windows)]
    #[test]
    fn the_verbatim_prefix_is_dropped() {
        assert_eq!(
            clean(Path::new(r"\\?\C:\Samples\Kicks\kick.wav")),
            Path::new(r"C:\Samples\Kicks\kick.wav")
        );
        assert!(same(Path::new(r"C:\Samples"), Path::new(r"c:\samples")));
    }

    #[test]
    fn a_relative_path_is_refused() {
        assert_eq!(
            windfall_core::english::english(&absolute("kick.wav").unwrap_err()),
            "\"kick.wav\" is not a full path"
        );
        assert!(absolute("  ").is_err());
    }

    #[test]
    fn the_stem_drops_only_the_last_extension() {
        assert_eq!(stem(Path::new("Kick Punch.wav")), "Kick Punch");
        assert_eq!(stem(Path::new("take.2.flac")), "take.2");
    }
}
