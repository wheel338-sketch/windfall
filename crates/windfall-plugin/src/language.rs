//! The language the exported plugin's panel is shown in.
//!
//! Windfall keeps the interface language the user chose in its settings file
//! (`language` in `settings.json`, in the app's config folder, written by the
//! desktop app whenever the language is set). The plugin runs inside another
//! program and reads that file when a panel opens. Without it (Windfall never
//! ran for this user, or the file cannot be read) the operating system's
//! language decides, and English when that is not one Windfall has. Neither
//! plugin format tells a plugin its host's language, so the system's stands
//! for it.

use std::path::{Path, PathBuf};

use windfall_core::english::Language;

/// The desktop app's identifier: its config folder's name.
const APP_IDENTIFIER: &str = "org.windfall.Windfall";
/// The desktop app's settings file in that folder.
const SETTINGS_FILE: &str = "settings.json";

/// The language for a panel opened now.
pub fn chosen() -> Language {
    config_dir()
        .and_then(|folder| from_settings(&folder.join(SETTINGS_FILE)))
        .or_else(system)
        .unwrap_or(Language::English)
}

/// The language a Windfall settings file names, if it can be read and names
/// one Windfall has.
pub fn from_settings(file: &Path) -> Option<Language> {
    let text = std::fs::read_to_string(file).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&text).ok()?;
    Language::from_code(settings.get("language")?.as_str()?)
}

/// The desktop app's config folder, as Tauri names it on each system. A
/// debug build honours `WINDFALL_CONFIG_DIR` as the desktop app does.
fn config_dir() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    #[cfg(debug_assertions)]
    if let Some(folder) = env("WINDFALL_CONFIG_DIR") {
        return Some(folder);
    }
    user_config_dir(env).map(|folder| folder.join(APP_IDENTIFIER))
}

#[cfg(windows)]
fn user_config_dir(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    env("APPDATA")
}

#[cfg(target_os = "macos")]
fn user_config_dir(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    env("HOME").map(|home| home.join("Library").join("Application Support"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn user_config_dir(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    env("XDG_CONFIG_HOME")
        .filter(|folder| folder.is_absolute())
        .or_else(|| env("HOME").map(|home| home.join(".config")))
}

/// The user's interface language in Windows, by its primary language id.
#[cfg(windows)]
fn system() -> Option<Language> {
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    match id & 0x3ff {
        0x09 => Some(Language::English),
        0x0a => Some(Language::Spanish),
        _ => None,
    }
}

/// The first of the user's preferred languages in macOS.
#[cfg(target_os = "macos")]
fn system() -> Option<Language> {
    Language::from_locale(&crate::panel::preferred_language()?)
}

/// The POSIX locale of messages: `LC_ALL`, then `LC_MESSAGES`, then `LANG`.
#[cfg(not(any(windows, target_os = "macos")))]
fn system() -> Option<Language> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .and_then(|locale| Language::from_locale(&locale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_file_names_the_language_and_anything_else_names_none() {
        let folder =
            std::env::temp_dir().join(format!("windfall-plugin-language-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(SETTINGS_FILE);
        for (json, expected) in [
            (
                r#"{"language":"es","recentProjects":[]}"#,
                Some(Language::Spanish),
            ),
            (r#"{"language":"en"}"#, Some(Language::English)),
            (r#"{"language":"fr"}"#, None),
            (r#"{"recentProjects":[]}"#, None),
            ("not json", None),
        ] {
            std::fs::write(&file, json).unwrap();
            assert_eq!(from_settings(&file), expected, "{json}");
        }
        assert_eq!(from_settings(&folder.join("missing.json")), None);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_config_folder_is_the_desktop_apps() {
        let config = include_str!("../../../apps/desktop/src-tauri/tauri.conf.json");
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(config["identifier"], APP_IDENTIFIER);
        let env = |name: &str| match name {
            "APPDATA" => Some(PathBuf::from("C:/Users/me/AppData/Roaming")),
            "HOME" => Some(PathBuf::from("/home/me")),
            _ => None,
        };
        let folder = user_config_dir(env).unwrap().join(APP_IDENTIFIER);
        assert!(folder.ends_with(APP_IDENTIFIER));
        #[cfg(windows)]
        assert_eq!(
            folder,
            PathBuf::from("C:/Users/me/AppData/Roaming/org.windfall.Windfall")
        );
    }
}
