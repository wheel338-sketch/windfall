//! Mixer track presets: capture from a track, single files anywhere, and the
//! user's preset library folder.
//!
//! The library is a folder of `.wfmixer` files next to the settings file. A
//! preset is named by its file name inside that folder, never by a path, so
//! nothing the UI sends can reach outside it.
use super::Session;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use windfall_project::{MixerTrackPreset, TrackId};

/// The extension of a mixer track preset file, without the dot.
pub const MIXER_PRESET_EXTENSION: &str = "wfmixer";
/// The largest preset file that is read, and the largest that is written.
const MAX_PRESET_BYTES: u64 = 256 * 1024 * 1024;
/// A file up to this size is read whole and checked when the library is
/// listed. A larger one is only skimmed for its name and what it refers to,
/// and is checked when it is read to be applied.
const FULL_CHECK_BYTES: u64 = 8 * 1024 * 1024;
/// Presets the library lists, and the most it holds.
pub const MAX_LIBRARY_PRESETS: usize = 512;
/// Characters of a preset's name that its file name keeps.
const MAX_FILE_STEM_CHARS: usize = 96;

/// A hosted plugin a preset refers to. The plugin itself is not in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerPresetPluginRef {
    pub format: String,
    pub id: String,
    pub name: String,
    pub path: String,
}

/// One file of the library, as the preset browser lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerPresetLibraryEntry {
    /// The file's name in the library folder: what read and delete take.
    pub file: String,
    /// The full path, for display.
    pub path: String,
    /// The preset's name, or the file's when the file cannot be read.
    pub name: String,
    pub effects: u32,
    pub plugins: Vec<MixerPresetPluginRef>,
    pub bytes: u64,
    /// Milliseconds since the Unix epoch, where the file system says.
    pub modified: Option<u64>,
    /// Why this file cannot be applied. The other files are unaffected.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerPresetLibrary {
    pub folder: String,
    pub entries: Vec<MixerPresetLibraryEntry>,
    /// The folder holds more presets than are listed.
    pub truncated: bool,
}

/// What listing reads of a file too large to check whole. Everything else
/// in the file, plugin state above all, is skipped without being kept.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Skimmed {
    version: u32,
    name: String,
    effects: Vec<serde::de::IgnoredAny>,
    plugins: Vec<MixerPresetPluginRef>,
}

fn plugin_refs(preset: &MixerTrackPreset) -> Vec<MixerPresetPluginRef> {
    preset
        .plugins
        .iter()
        .map(|plugin| MixerPresetPluginRef {
            format: plugin.format.clone(),
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            path: plugin.path.clone(),
        })
        .collect()
}

fn is_preset_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(MIXER_PRESET_EXTENSION))
}

/// Reads and checks a whole preset file. Never reads more than the limit.
fn read_preset_file(path: &Path) -> Result<MixerTrackPreset, String> {
    let file = fs::File::open(path).map_err(|error| {
        windfall_ipc::ui_message(
            "mixerPresets.openFailed",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_PRESET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            windfall_ipc::ui_message(
                "mixerPresets.readFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
    if bytes.len() as u64 > MAX_PRESET_BYTES {
        return Err("Preset file exceeds 256 MiB".into());
    }
    let preset: MixerTrackPreset = serde_json::from_slice(&bytes).map_err(|error| {
        windfall_ipc::ui_message(
            "mixerPresets.invalid",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    preset.validate()?;
    Ok(preset)
}

fn encode(preset: &MixerTrackPreset) -> Result<Vec<u8>, String> {
    preset.validate()?;
    let bytes = serde_json::to_vec_pretty(preset).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PRESET_BYTES {
        return Err("Preset file exceeds 256 MiB".into());
    }
    Ok(bytes)
}

/// The file name a preset of this name is saved under, without extension:
/// the name with whatever a file name cannot hold replaced.
fn file_stem(name: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(MAX_FILE_STEM_CHARS)
        .collect();
    // Windows drops trailing dots and spaces, and a leading dot hides a file.
    stem = stem
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    let device = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && device.ends_with(|c: char| c.is_ascii_digit()));
    if stem.is_empty() {
        "Mixer track".to_owned()
    } else if reserved {
        format!("_{stem}")
    } else {
        stem
    }
}

/// What the browser shows of one library file. A file that cannot be used
/// still gets an entry, with the reason.
fn describe(path: &Path) -> MixerPresetLibraryEntry {
    let file = crate::paths::name(path);
    let metadata = fs::metadata(path);
    let bytes = metadata.as_ref().map_or(0, fs::Metadata::len);
    let modified = metadata
        .as_ref()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_millis().min(u128::from(u64::MAX)) as u64);
    let read = match &metadata {
        Err(error) => Err(windfall_ipc::ui_message(
            "mixerPresets.openFailed",
            serde_json::json!({ "error": error.to_string() }),
        )),
        Ok(_) if bytes > MAX_PRESET_BYTES => Err("Preset file exceeds 256 MiB".to_owned()),
        Ok(_) if bytes <= FULL_CHECK_BYTES => read_preset_file(path).map(|preset| {
            let plugins = plugin_refs(&preset);
            (preset.name, preset.effects.len(), plugins)
        }),
        Ok(_) => skim(path),
    };
    let (name, effects, plugins, error) = match read {
        Ok((name, effects, plugins)) => (name, effects as u32, plugins, None),
        Err(error) => (crate::paths::stem(path), 0, Vec::new(), Some(error)),
    };
    MixerPresetLibraryEntry {
        file,
        path: crate::paths::display(path),
        name,
        effects,
        plugins,
        bytes,
        modified,
        error,
    }
}

fn skim(path: &Path) -> Result<(String, usize, Vec<MixerPresetPluginRef>), String> {
    let file = fs::File::open(path).map_err(|error| {
        windfall_ipc::ui_message(
            "mixerPresets.openFailed",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let skimmed: Skimmed = serde_json::from_reader(io::BufReader::new(file.take(MAX_PRESET_BYTES)))
        .map_err(|error| {
            windfall_ipc::ui_message(
                "mixerPresets.invalid",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
    if skimmed.version != 1 {
        return Err("Unsupported mixer preset version".into());
    }
    if skimmed.effects.len() > windfall_project::MAX_EFFECT_SLOTS
        || skimmed.plugins.len() > skimmed.effects.len()
    {
        return Err("Mixer preset exceeds the effect slot limit".into());
    }
    Ok((skimmed.name, skimmed.effects.len(), skimmed.plugins))
}

impl Session {
    pub fn mixer_preset_capture(
        &self,
        id: TrackId,
        generation: u64,
        revision: u64,
    ) -> Result<MixerTrackPreset, String> {
        let _recording = self.recording_idle()?;
        let (project, _asset_pins, plugin_revision) = {
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed before preset capture; try again".into());
            }
            (
                state.document.project().clone(),
                state.document.effect_asset_pins(),
                self.plugin_revision(),
            )
        };
        #[cfg(test)]
        self.pause("mixer-preset:capture");
        let project = self.capture_plugins_idle(project, plugin_revision)?;
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed during preset capture; try again".into());
        }
        MixerTrackPreset::capture(&project, id)
    }

    pub fn mixer_preset_write(
        &self,
        path: String,
        preset: MixerTrackPreset,
    ) -> Result<String, String> {
        let bytes = encode(&preset)?;
        let path = crate::paths::absolute(&path)?;
        windfall_project::file::write_atomic(&path, &bytes).map_err(|error| {
            windfall_ipc::ui_message(
                "mixerPresets.saveFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
        Ok(crate::paths::display(&path))
    }

    pub fn mixer_preset_read(&self, path: String) -> Result<MixerTrackPreset, String> {
        read_preset_file(&crate::paths::absolute(&path)?)
    }

    /// The user's mixer preset library folder. It exists once a preset has
    /// been saved into it.
    pub fn mixer_preset_library_dir(&self) -> PathBuf {
        self.store().mixer_presets_dir()
    }

    /// The path of a library file named by the UI, which may name nothing
    /// but a preset file directly inside the library folder.
    fn mixer_preset_library_file(&self, file: &str) -> Result<PathBuf, String> {
        let name = Path::new(file);
        let mut components = name.components();
        let plain = matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && !file.contains(['/', '\\', ':'])
            && file.len() <= 255;
        if !plain || !is_preset_file(name) {
            return Err("That is not a preset of the mixer preset library".into());
        }
        Ok(self.mixer_preset_library_dir().join(name))
    }

    /// Lists the library by name. A file that cannot be read, is not a
    /// preset, is of another version or is too large is listed with its
    /// error and does not stop the others from being listed.
    pub fn mixer_preset_library(&self) -> Result<MixerPresetLibrary, String> {
        let folder = self.mixer_preset_library_dir();
        let mut files = match fs::read_dir(&folder) {
            Ok(listing) => listing
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| is_preset_file(path) && path.is_file())
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(windfall_ipc::ui_message(
                    "mixerPresets.libraryUnreadable",
                    serde_json::json!({"folder": folder.display().to_string(), "error": error.to_string()}),
                ));
            }
        };
        // By file name first, so the files listed of an overfull folder are
        // the same ones every time.
        files.sort_by_key(|path| crate::paths::name(path).to_lowercase());
        let truncated = files.len() > MAX_LIBRARY_PRESETS;
        files.truncate(MAX_LIBRARY_PRESETS);
        let mut entries: Vec<_> = files.iter().map(|path| describe(path)).collect();
        entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.file.to_lowercase()));
        Ok(MixerPresetLibrary {
            folder: crate::paths::display(&folder),
            entries,
            truncated,
        })
    }

    /// Saves a preset into the library under a file name made from its
    /// name. A name that is taken gets a number: nothing is overwritten.
    pub fn mixer_preset_library_save(
        &self,
        preset: MixerTrackPreset,
    ) -> Result<MixerPresetLibraryEntry, String> {
        let bytes = encode(&preset)?;
        let folder = self.mixer_preset_library_dir();
        let refused = |error: io::Error| {
            windfall_ipc::ui_message(
                "mixerPresets.saveFailed",
                serde_json::json!({ "error": error.to_string() }),
            )
        };
        fs::create_dir_all(&folder).map_err(refused)?;
        let held = fs::read_dir(&folder)
            .map_err(refused)?
            .filter_map(Result::ok)
            .filter(|entry| is_preset_file(&entry.path()))
            .count();
        if held >= MAX_LIBRARY_PRESETS {
            return Err(windfall_ipc::ui_message(
                "mixerPresets.libraryFull",
                serde_json::json!({"max": MAX_LIBRARY_PRESETS}),
            ));
        }
        let stem = file_stem(&preset.name);
        for number in 1..=MAX_LIBRARY_PRESETS + 1 {
            let path = folder.join(if number == 1 {
                format!("{stem}.{MIXER_PRESET_EXTENSION}")
            } else {
                format!("{stem} {number}.{MIXER_PRESET_EXTENSION}")
            });
            // Creating the file claims the name: of two saves at once, only
            // one can create it, and the other moves on to the next number.
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(refused(error)),
            }
            #[cfg(test)]
            self.pause("mixer-preset:library-claimed");
            if let Err(error) = windfall_project::file::write_atomic(&path, &bytes) {
                // The claim is empty and would list as a damaged preset.
                let _ = fs::remove_file(&path);
                return Err(refused(error));
            }
            return Ok(describe(&path));
        }
        Err("Too many presets of the library share this name".into())
    }

    pub fn mixer_preset_library_read(&self, file: String) -> Result<MixerTrackPreset, String> {
        read_preset_file(&self.mixer_preset_library_file(&file)?)
    }

    pub fn mixer_preset_library_delete(&self, file: String) -> Result<(), String> {
        let path = self.mixer_preset_library_file(&file)?;
        fs::remove_file(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                "That preset is no longer in the library".to_owned()
            } else {
                windfall_ipc::ui_message(
                    "mixerPresets.deleteFailed",
                    serde_json::json!({ "error": error.to_string() }),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::file_stem;

    #[test]
    fn a_preset_name_becomes_a_file_name_any_system_takes() {
        assert_eq!(file_stem("Vocal chain"), "Vocal chain");
        assert_eq!(file_stem("Drums: \"tight\" <1/2>"), "Drums_ _tight_ _1_2_");
        assert_eq!(file_stem("  ..hidden.. "), "hidden");
        assert_eq!(file_stem(""), "Mixer track");
        assert_eq!(file_stem(" . "), "Mixer track");
        assert_eq!(file_stem("con"), "_con");
        assert_eq!(file_stem("LPT1.backup"), "_LPT1.backup");
        assert_eq!(file_stem("Console"), "Console");
        assert_eq!(file_stem("tab\there"), "tab_here");
        assert_eq!(file_stem(&"é".repeat(300)).chars().count(), 96);
    }
}
