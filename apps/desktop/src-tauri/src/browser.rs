//! The file browser: listing a folder the way the browser panel shows it.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::Path;

use windfall_ipc::{BrowserEntry, BrowserEntryKind, BrowserRoot, BrowserRootKind};
use windfall_project::file::FILE_EXTENSION;

use crate::paths;

/// Name of the factory content root in the browser.
pub const FACTORY_ROOT_NAME: &str = "Factory";

/// The root that holds the content shipped with Windfall.
pub fn factory_root(folder: &Path) -> BrowserRoot {
    BrowserRoot {
        name: FACTORY_ROOT_NAME.to_owned(),
        path: paths::display(folder),
        kind: BrowserRootKind::Factory,
    }
}

/// A root for a folder the user added, named after the folder.
pub fn user_root(folder: &Path) -> BrowserRoot {
    let name = folder
        .file_name()
        // A drive has no name of its own.
        .map_or_else(
            || paths::display(folder),
            |name| name.to_string_lossy().into_owned(),
        );
    BrowserRoot {
        name,
        path: paths::display(folder),
        kind: BrowserRootKind::User,
    }
}

/// Lists a folder: folders first, then files, each group ordered by name
/// with letter case ignored and numbers compared by value, so "Kick 2" comes
/// before "Kick 10". Hidden and system files are left out.
pub fn list_folder(folder: &Path) -> Result<Vec<BrowserEntry>, String> {
    let shown = paths::display(folder);
    let entries = fs::read_dir(folder).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => windfall_ipc::ui_message(
            "browser.folderMissing",
            serde_json::json!({ "folder": shown }),
        ),
        io::ErrorKind::PermissionDenied => windfall_ipc::ui_message(
            "browser.folderDenied",
            serde_json::json!({ "folder": shown }),
        ),
        io::ErrorKind::NotADirectory => {
            windfall_ipc::ui_message("browser.notAFolder", serde_json::json!({ "folder": shown }))
        }
        _ if folder.is_file() => {
            windfall_ipc::ui_message("browser.notAFolder", serde_json::json!({ "folder": shown }))
        }
        _ => windfall_ipc::ui_message(
            "browser.folderUnreadable",
            serde_json::json!({ "error": error.to_string(), "folder": shown }),
        ),
    })?;

    let mut listed = Vec::new();
    // An entry that cannot be read is left out; the rest of the folder is
    // still worth showing.
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || is_hidden(&entry) {
            continue;
        }
        let path = entry.path();
        // Follows links, so a shortcut to a folder opens as a folder.
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let kind = if metadata.is_dir() {
            BrowserEntryKind::Folder
        } else {
            file_kind(&path)
        };
        listed.push(BrowserEntry {
            name,
            path: paths::display(&path),
            kind,
        });
    }
    listed.sort_by(|a, b| {
        let folder = |entry: &BrowserEntry| entry.kind == BrowserEntryKind::Folder;
        folder(b)
            .cmp(&folder(a))
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    Ok(listed)
}

pub(crate) fn file_kind(path: &Path) -> BrowserEntryKind {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return BrowserEntryKind::Other;
    };
    if windfall_codec::is_audio_extension(extension)
        || ["wfpreset", "syn", "speech"]
            .iter()
            .any(|e| extension.eq_ignore_ascii_case(e))
    {
        BrowserEntryKind::Audio
    } else if extension.eq_ignore_ascii_case(FILE_EXTENSION) {
        BrowserEntryKind::Project
    } else if extension.eq_ignore_ascii_case("sf2") {
        BrowserEntryKind::SoundFont
    } else {
        BrowserEntryKind::Other
    }
}

#[cfg(windows)]
fn is_hidden(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;

    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.file_attributes() & (HIDDEN | SYSTEM) != 0)
}

#[cfg(not(windows))]
fn is_hidden(_entry: &fs::DirEntry) -> bool {
    // Elsewhere a hidden file is one whose name starts with a dot.
    false
}

/// Orders names the way a person reads them: letter case is ignored and a
/// run of digits is compared as a number.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let lower_a: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let lower_b: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let (mut rest_a, mut rest_b) = (lower_a.as_slice(), lower_b.as_slice());

    while let (Some(first_a), Some(first_b)) = (rest_a.first(), rest_b.first()) {
        if first_a.is_ascii_digit() && first_b.is_ascii_digit() {
            let (number_a, after_a) = split_number(rest_a);
            let (number_b, after_b) = split_number(rest_b);
            // With leading zeros gone, the longer number is the larger one.
            let order = number_a
                .len()
                .cmp(&number_b.len())
                .then_with(|| number_a.cmp(number_b));
            if order != Ordering::Equal {
                return order;
            }
            (rest_a, rest_b) = (after_a, after_b);
        } else {
            if first_a != first_b {
                return first_a.cmp(first_b);
            }
            (rest_a, rest_b) = (&rest_a[1..], &rest_b[1..]);
        }
    }
    // Names that differ only in case or in leading zeros still need one
    // fixed order.
    rest_a.len().cmp(&rest_b.len()).then_with(|| a.cmp(b))
}

/// Splits off the digits at the front, without their leading zeros.
fn split_number(text: &[char]) -> (&[char], &[char]) {
    let end = text
        .iter()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(end);
    let zeros = digits.iter().take_while(|c| **c == '0').count();
    (&digits[zeros.min(digits.len().saturating_sub(1))..], rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_inside_names_are_compared_by_value() {
        let mut names = vec![
            "Kick 10.wav",
            "kick 2.wav",
            "Kick 1.wav",
            "Snare.wav",
            "Kick 002b.wav",
            "clap.wav",
            "Kick.wav",
            "10 Hat.wav",
            "9 Hat.wav",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "9 Hat.wav",
                "10 Hat.wav",
                "clap.wav",
                "Kick 1.wav",
                "kick 2.wav",
                "Kick 002b.wav",
                "Kick 10.wav",
                "Kick.wav",
                "Snare.wav",
            ]
        );
    }

    #[test]
    fn names_that_differ_only_in_case_or_zeros_have_a_fixed_order() {
        assert_eq!(natural_cmp("a", "a"), Ordering::Equal);
        assert_ne!(natural_cmp("Kick", "kick"), Ordering::Equal);
        assert_ne!(natural_cmp("Kick 01", "Kick 1"), Ordering::Equal);
        assert_eq!(
            natural_cmp("Kick 0", "Kick 00"),
            natural_cmp("Kick 00", "Kick 0").reverse()
        );
        assert_eq!(natural_cmp("Kick", "Kick 2"), Ordering::Less);
    }

    #[test]
    fn a_folder_lists_folders_first_and_classifies_files() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        for name in ["Zebra", "apple", "Loops 10", "Loops 9"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        for name in [
            "kick 10.WAV",
            "Kick 2.wav",
            "beat.windfall",
            "Song.Windfall",
            "notes.txt",
            "take.flac",
            "voice.mp3",
            "pad.ogg",
            "string.aiff",
            "README",
            "GM Bank.SF2",
            ".hidden.wav",
        ] {
            fs::write(root.join(name), b"").unwrap();
        }
        fs::create_dir(root.join(".git")).unwrap();

        let listed = list_folder(root).unwrap();
        let summary: Vec<(&str, BrowserEntryKind)> = listed
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect();
        use BrowserEntryKind::{Audio, Folder, Other, Project, SoundFont};
        assert_eq!(
            summary,
            [
                ("apple", Folder),
                ("Loops 9", Folder),
                ("Loops 10", Folder),
                ("Zebra", Folder),
                ("beat.windfall", Project),
                ("GM Bank.SF2", SoundFont),
                ("Kick 2.wav", Audio),
                ("kick 10.WAV", Audio),
                ("notes.txt", Other),
                ("pad.ogg", Audio),
                ("README", Other),
                ("Song.Windfall", Project),
                ("string.aiff", Audio),
                ("take.flac", Audio),
                ("voice.mp3", Audio),
            ]
        );
        assert_eq!(listed[0].path, paths::display(&root.join("apple")));
    }

    #[cfg(windows)]
    #[test]
    fn files_windows_marks_hidden_are_left_out() {
        let folder = tempfile::tempdir().unwrap();
        let hidden = folder.path().join("desktop.ini");
        fs::write(&hidden, b"").unwrap();
        fs::write(folder.path().join("kick.wav"), b"").unwrap();
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg("+s")
            .arg(&hidden)
            .status()
            .unwrap();
        assert!(status.success());

        let names: Vec<String> = list_folder(folder.path())
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["kick.wav"]);
    }

    #[test]
    fn a_folder_that_cannot_be_listed_says_why() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("gone");
        // The reason is a code and the folder it is about; the words are the
        // catalog's, in the language of the interface.
        assert_eq!(
            list_folder(&missing).unwrap_err(),
            windfall_ipc::ui_message(
                "browser.folderMissing",
                serde_json::json!({ "folder": missing.display().to_string() })
            )
        );

        let file = folder.path().join("kick.wav");
        fs::write(&file, b"").unwrap();
        assert_eq!(
            list_folder(&file).unwrap_err(),
            windfall_ipc::ui_message(
                "browser.notAFolder",
                serde_json::json!({ "folder": file.display().to_string() })
            )
        );
    }

    #[test]
    fn roots_are_named_after_their_folder() {
        let root = user_root(Path::new(if cfg!(windows) {
            r"D:\Samples\Drum Kits"
        } else {
            "/samples/Drum Kits"
        }));
        assert_eq!(root.name, "Drum Kits");
        assert_eq!(root.kind, BrowserRootKind::User);
        assert_eq!(factory_root(Path::new("/x")).name, "Factory");
    }
}
