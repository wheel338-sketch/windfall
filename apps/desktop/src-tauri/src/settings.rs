//! What the app remembers between runs: the audio device, the browser's
//! folders and the recent projects.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windfall_ipc::{AudioSettings, MidiHardwareSettings};
use windfall_project::file::write_atomic;

use crate::paths;

/// Name of the settings file inside the app's config folder.
pub const SETTINGS_FILE: &str = "settings.json";

/// Recent projects remembered at most.
pub const MAX_RECENT_PROJECTS: usize = 10;

/// The contents of the settings file. Every field has a default, so a file
/// written by an older version still loads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub midi_hardware: MidiHardwareSettings,
    /// Folders the user added to the browser, in the order they were added.
    pub browser_roots: Vec<String>,
    /// Project files, most recently used first.
    pub recent_projects: Vec<String>,
}

/// The settings and the file they are kept in.
///
/// Nothing here can fail in a way the app has to handle: a settings file
/// that is missing or damaged gives the defaults, and a file that cannot be
/// written is logged and the settings live on in memory.
#[derive(Debug)]
pub struct SettingsStore {
    file: PathBuf,
    settings: Settings,
}

impl SettingsStore {
    /// A separate per-user library file; library edits never rewrite device settings.
    pub fn library_file(&self) -> PathBuf {
        self.file
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::library::METADATA_FILE)
    }
    pub fn recordings_dir(&self) -> PathBuf {
        self.file
            .parent()
            .unwrap_or(Path::new("."))
            .join("recordings")
    }
    /// The per-user folder of mixer track presets, next to the settings file.
    pub fn mixer_presets_dir(&self) -> PathBuf {
        self.file
            .parent()
            .unwrap_or(Path::new("."))
            .join("mixer-presets")
    }

    /// Reads the settings file at `file`, which need not exist.
    pub fn load(file: PathBuf) -> Self {
        let settings = match fs::read_to_string(&file) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|error| {
                log::warn!(
                    "the settings file {} is damaged ({error}); starting from the defaults",
                    file.display()
                );
                Settings::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => {
                log::warn!(
                    "could not read the settings file {} ({error}); starting from the defaults",
                    file.display()
                );
                Settings::default()
            }
        };
        Self { file, settings }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings and writes them to disk if anything changed.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings == before {
            return;
        }
        if let Err(error) = self.write() {
            log::warn!(
                "could not write the settings file {}: {error}",
                self.file.display()
            );
        }
    }

    /// Puts a project at the front of the recent list.
    pub fn remember_project(&mut self, project: &Path) {
        self.update(|settings| {
            let recent = &mut settings.recent_projects;
            recent.retain(|held| !paths::same(Path::new(held), project));
            recent.insert(0, paths::display(project));
            keep_existing(recent);
        });
    }

    /// The recent projects that still exist, most recent first. Ones that
    /// are gone are forgotten for good.
    pub fn recent_projects(&mut self) -> Vec<String> {
        self.update(|settings| keep_existing(&mut settings.recent_projects));
        self.settings.recent_projects.clone()
    }

    /// Writes the file the way a project is written: to a temporary file
    /// of this write's own that is then renamed into place. A crash cannot
    /// leave half a settings file, and two copies of Windfall writing at
    /// once cannot write into each other's.
    fn write(&self) -> io::Result<()> {
        let mut json = serde_json::to_string_pretty(&self.settings).map_err(io::Error::other)?;
        json.push('\n');
        write_atomic(&self.file, json.as_bytes())
    }
}

fn keep_existing(recent: &mut Vec<String>) {
    recent.retain(|held| Path::new(held).is_file());
    recent.truncate(MAX_RECENT_PROJECTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(folder: &Path, name: &str) -> PathBuf {
        let file = folder.join(name);
        fs::write(&file, b"{}").unwrap();
        file
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(folder.path().join("nowhere").join(SETTINGS_FILE));
        assert_eq!(store.settings(), &Settings::default());
    }

    #[test]
    fn a_damaged_file_gives_the_defaults_and_is_replaced_by_the_next_write() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(SETTINGS_FILE);
        for damaged in [
            "",
            "{ \"audio\": ",
            "[1, 2, 3]",
            "{\"recentProjects\": 7}",
            "\u{0}\u{1}",
        ] {
            fs::write(&file, damaged).unwrap();
            let store = SettingsStore::load(file.clone());
            assert_eq!(store.settings(), &Settings::default(), "{damaged:?}");
        }

        let mut store = SettingsStore::load(file.clone());
        store.update(|settings| settings.audio.buffer_frames = Some(256));
        let reloaded = SettingsStore::load(file);
        assert_eq!(reloaded.settings().audio.buffer_frames, Some(256));
    }

    #[test]
    fn settings_survive_a_reload_and_unknown_fields_are_ignored() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("config").join(SETTINGS_FILE);
        let mut store = SettingsStore::load(file.clone());
        store.update(|settings| {
            settings.audio = AudioSettings {
                output_channels: None,
                host: Some("WASAPI".to_owned()),
                device: None,
                sample_rate: Some(44_100),
                buffer_frames: Some(128),
            };
            settings.browser_roots.push("D:\\Samples".to_owned());
        });
        assert_eq!(
            SettingsStore::load(file.clone()).settings(),
            store.settings()
        );
        // The write left nothing but the file behind.
        let left: Vec<_> = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [SETTINGS_FILE]);

        fs::write(
            &file,
            r#"{ "audio": { "bufferFrames": 512 }, "fromTheFuture": true }"#,
        )
        .unwrap();
        let store = SettingsStore::load(file);
        assert_eq!(store.settings().audio.buffer_frames, Some(512));
        assert!(store.settings().browser_roots.is_empty());
    }

    #[test]
    fn two_stores_writing_one_file_at_once_leave_a_whole_file() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(SETTINGS_FILE);
        // Two copies of the app, each writing its settings over and over.
        let writers: Vec<_> = (0..2_u32)
            .map(|writer| {
                let file = file.clone();
                std::thread::spawn(move || {
                    let mut store = SettingsStore::load(file);
                    for round in 0..40 {
                        store.update(|settings| {
                            settings.audio.buffer_frames = Some(64 + writer);
                            settings.browser_roots = vec![format!("{writer}"); round % 7 + 1];
                        });
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }

        // Whichever wrote last, the file is all of one write.
        let json = fs::read_to_string(&file).unwrap();
        let settings: Settings = serde_json::from_str(&json).unwrap();
        let writer = settings.audio.buffer_frames.unwrap() - 64;
        assert!(writer < 2);
        assert!(
            settings
                .browser_roots
                .iter()
                .all(|root| *root == writer.to_string())
        );
        let left: Vec<_> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [SETTINGS_FILE]);
    }

    #[test]
    fn recent_projects_are_newest_first_without_repeats_or_missing_files() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(SETTINGS_FILE);
        let mut store = SettingsStore::load(file.clone());
        let a = touch(folder.path(), "a.windfall");
        let b = touch(folder.path(), "b.windfall");
        let c = touch(folder.path(), "c.windfall");

        store.remember_project(&a);
        store.remember_project(&b);
        store.remember_project(&c);
        store.remember_project(&a);
        let shown = |files: &[&PathBuf]| -> Vec<String> {
            files.iter().map(|file| paths::display(file)).collect()
        };
        assert_eq!(store.recent_projects(), shown(&[&a, &c, &b]));

        fs::remove_file(&c).unwrap();
        assert_eq!(store.recent_projects(), shown(&[&a, &b]));
        assert_eq!(
            SettingsStore::load(file).settings().recent_projects,
            shown(&[&a, &b])
        );
    }

    #[test]
    fn only_the_ten_most_recent_projects_are_kept() {
        let folder = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::load(folder.path().join(SETTINGS_FILE));
        let files: Vec<PathBuf> = (0..12)
            .map(|index| touch(folder.path(), &format!("{index}.windfall")))
            .collect();
        for file in &files {
            store.remember_project(file);
        }
        let recent = store.recent_projects();
        assert_eq!(recent.len(), MAX_RECENT_PROJECTS);
        assert_eq!(recent[0], paths::display(&files[11]));
        assert_eq!(recent[9], paths::display(&files[2]));
    }
}
