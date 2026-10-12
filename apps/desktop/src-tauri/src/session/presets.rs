//! Preset files on disk: taking a channel's or a plugin's state out of the
//! open project, writing it with its audio, reading it back, and the user's
//! preset library.
//!
//! [`windfall_project::preset`] has the format and the rules of loading.
//! This module adds what needs a disk: a sampler preset's audio is copied
//! into a folder beside the preset file and named in the file by a path
//! relative to it, and is added to the project from there when the preset
//! loads.
//!
//! Nothing here holds the document lock while it reads or writes a file or
//! captures native plugin state, and nothing here edits the project except
//! through an ordinary command, so loading a preset is one undo step.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use windfall_ipc::{PresetCapture, PresetEntry, PresetReview};
use windfall_project::preset::{
    MAX_PRESET_ASSET_BYTES, MAX_PRESET_FILE_BYTES, MAX_PRESET_NAME_BYTES,
};
use windfall_project::{
    Channel, ChannelId, ChannelPreset, Command, DispatchResult, MixerTrackPreset, PluginBinding,
    PluginPreset, PluginPresetState, PluginTarget, Preset, PresetAsset, PresetAudio, Project,
    SamplePath, file,
};

use super::Session;
use super::samples::locate;
use crate::paths;

/// The extension of a preset file.
pub const PRESET_EXTENSION: &str = "wfpreset";
/// The folder of the user's preset library, beside the settings file.
const LIBRARY_FOLDER: &str = "presets";
/// The most presets the library lists.
const MAX_LIBRARY_PRESETS: usize = 2048;
/// The longest file name a preset or its audio is given, in characters.
const MAX_FILE_NAME_CHARS: usize = 120;

impl Session {
    /// The project as it is now, with the live state of its native plugins
    /// captured into it. Refused when the project is not the one the caller
    /// saw, by generation or by revision.
    fn preset_snapshot(
        &self,
        generation: u64,
        revision: u64,
    ) -> Result<(Project, Option<PathBuf>, windfall_dsp::asset::AssetPins), String> {
        let _recording = self.recording_idle()?;
        let (project, plugin_revision, directory, asset_pins) = {
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed before the preset was taken; try again".into());
            }
            self.pin_extraction(state.project_dir());
            (
                state.document.project().clone(),
                self.plugin_revision(),
                state.project_dir().map(Path::to_path_buf),
                state.document.effect_asset_pins(),
            )
        };
        // Native state is captured with the document lock released.
        let project = self.capture_plugins_idle(project, plugin_revision)?;
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed while the preset was taken; try again".into());
        }
        Ok((project, directory, asset_pins))
    }

    /// What a channel sounds like, as a preset, and the audio file its
    /// sampler plays.
    pub fn preset_capture_channel(
        &self,
        id: ChannelId,
        generation: u64,
        revision: u64,
    ) -> Result<PresetCapture, String> {
        let (project, directory, _asset_pins) = self.preset_snapshot(generation, revision)?;
        let (body, sample) = ChannelPreset::capture(&project, id)?;
        let name = body.name.clone();
        let (asset, audio) = match sample {
            Some(sample) => {
                let source = locate(&sample, directory.as_deref(), &self.inner.factory_dir)?;
                let asset = PresetAsset {
                    id: 1,
                    name: bounded_name(&sample.name),
                    // Where the audio goes is known when the file is written.
                    path: file_name_of(&source)?,
                    bytes: 0,
                };
                (Some(asset), Some(paths::display(&source)))
            }
            None => (None, None),
        };
        let preset = Preset::of_channel(&name, body, asset);
        preset.validate()?;
        Ok(PresetCapture { preset, audio })
    }

    /// The state of one instrument or effect, as a preset.
    pub fn preset_capture_plugin(
        &self,
        target: PluginTarget,
        generation: u64,
        revision: u64,
    ) -> Result<PresetCapture, String> {
        let (project, _, _asset_pins) = self.preset_snapshot(generation, revision)?;
        let body = PluginPreset::capture(&project, target)?;
        let name = match target {
            PluginTarget::Instrument { channel } => {
                project.channel(channel).map(|channel| channel.name.clone())
            }
            PluginTarget::Effect { .. } => match &body.state {
                PluginPresetState::Effect { params, .. } => Some(params.kind().name().to_owned()),
                PluginPresetState::Hosted { binding, .. } => Some(binding.name.clone()),
                PluginPresetState::Instrument { .. } => None,
            },
        }
        .unwrap_or_else(|| "Preset".to_owned());
        let preset = Preset::of_plugin(&name, body);
        preset.validate()?;
        Ok(PresetCapture {
            preset,
            audio: None,
        })
    }

    /// Writes a preset to `path`, which gains the `.wfpreset` extension if
    /// it lacks it, and returns the path written.
    ///
    /// A sampler preset's audio is copied first, into the folder
    /// `<name> audio` beside the preset, and the file names it by that
    /// relative path. If the copy fails nothing is written. The preset file
    /// itself is written whole or not at all.
    pub fn preset_write(&self, path: &str, capture: PresetCapture) -> Result<String, String> {
        let target = with_extension(paths::absolute(path)?);
        let PresetCapture { mut preset, audio } = capture;
        preset.validate()?;
        match (preset.assets.as_mut_slice(), audio) {
            ([], None) => {}
            ([asset], Some(source)) => {
                let source = paths::absolute(&source)?;
                let folder_name = format!("{} audio", paths::stem(&target));
                let file_name = file_name_of(&source)?;
                let folder = target
                    .parent()
                    .ok_or("The preset has no folder.")?
                    .join(&folder_name);
                asset.bytes = copy_audio(&source, &folder.join(&file_name))?;
                asset.path = format!("{folder_name}/{file_name}");
            }
            _ => return Err("The preset and its audio do not belong together.".into()),
        }
        let json = preset.to_json()?;
        if json.len() > MAX_PRESET_FILE_BYTES {
            return Err("The preset is larger than 256 MiB.".into());
        }
        file::write_atomic(&target, json.as_bytes())
            .map_err(|error| format!("Could not save the preset: {error}"))?;
        Ok(paths::display(&target))
    }

    /// Reads the preset at `path` and says what loading it would lack: audio
    /// that is not beside it and plugins that are not installed. A mixer
    /// track preset in the mixer's own file format is read as one too.
    pub fn preset_read(&self, path: &str) -> Result<PresetReview, String> {
        let source = paths::absolute(path)?;
        let bytes = read_capped(&source, MAX_PRESET_FILE_BYTES)?;
        let (preset, mut notes) = match Preset::from_json(&bytes) {
            Ok(read) => read,
            Err(error) => match serde_json::from_slice::<MixerTrackPreset>(&bytes) {
                Ok(track) if track.validate().is_ok() => {
                    (Preset::of_mixer_track(track), Vec::new())
                }
                _ => return Err(error),
            },
        };
        let folder = source.parent().unwrap_or(Path::new("."));
        let mut missing_audio = Vec::new();
        for asset in &preset.assets {
            match asset_file(folder, asset).and_then(|file| fs::metadata(file).ok()) {
                Some(metadata) if metadata.is_file() => {
                    if metadata.len() != asset.bytes {
                        notes.push(format!(
                            "The audio \"{}\" is not the size it was when the preset was saved; it may have been replaced.",
                            asset.name
                        ));
                    }
                }
                _ => missing_audio.push(asset.name.clone()),
            }
        }
        for binding in hosted(&preset) {
            if !Path::new(&binding.path).exists() {
                notes.push(format!(
                    "The plugin \"{}\" ({}) is not installed at {}. Its saved state is kept, and it stays silent or bypassed until the plugin is there.",
                    binding.name,
                    binding.format.to_uppercase(),
                    binding.path
                ));
            }
        }
        Ok(PresetReview {
            path: paths::display(&source),
            preset,
            notes,
            missing_audio,
        })
    }

    /// Loads the channel preset at `path` onto a channel, as one undo step.
    /// `expected` is the channel as it was when the preset was chosen.
    ///
    /// The preset's audio is added to the project from beside the preset
    /// file. When it is not there the load is refused, unless
    /// `without_audio` says to load the settings anyway and leave the
    /// sampler with no sample.
    pub fn preset_apply_channel(
        &self,
        path: &str,
        id: ChannelId,
        expected: Channel,
        name_color: bool,
        without_audio: bool,
    ) -> Result<DispatchResult, String> {
        self.preset_apply_channel_guarded(path, id, expected, name_color, without_audio, None)
    }

    pub fn preset_apply_channel_guarded(
        &self,
        path: &str,
        id: ChannelId,
        expected: Channel,
        name_color: bool,
        without_audio: bool,
        guard: Option<(u64, u64)>,
    ) -> Result<DispatchResult, String> {
        let review = self.preset_read(path)?;
        let kind = review.preset.kind;
        let Some(body) = review.preset.channel else {
            return Err(format!(
                "\"{}\" is a {} preset, not a channel preset.",
                paths::name(Path::new(&review.path)),
                kind.label()
            ));
        };
        let folder = Path::new(&review.path).parent().unwrap_or(Path::new("."));
        let audio = match review.preset.assets.first() {
            None => None,
            Some(asset) => match asset_file(folder, asset).filter(|file| file.is_file()) {
                Some(found) => {
                    let found = paths::clean(&found);
                    let directory = self.state().project_dir().map(Path::to_path_buf);
                    Some(PresetAudio {
                        name: asset.name.clone(),
                        path: file::sample_path_for(
                            &found,
                            directory.as_deref(),
                            &self.inner.factory_dir,
                        ),
                    })
                }
                None if without_audio => None,
                None => {
                    return Err(format!(
                        "The preset's audio \"{}\" is not beside it. Put it back, or load the preset without its audio.",
                        asset.name
                    ));
                }
            },
        };
        let command = Command::ApplyChannelPreset {
            id,
            expected,
            preset: body,
            sample: None,
            audio,
            name_color,
        };
        match guard {
            Some((generation, revision)) => {
                self.dispatch_file_command(command, generation, revision)
            }
            None => self.dispatch(command, None),
        }
    }

    /// The folder of the user's preset library.
    fn preset_library_dir(&self) -> PathBuf {
        self.store()
            .recordings_dir()
            .parent()
            .unwrap_or(Path::new("."))
            .join(LIBRARY_FOLDER)
    }

    /// The presets of the user's library, by name. A file that is not a
    /// readable preset is left out and logged.
    pub fn preset_library(&self) -> Result<Vec<PresetEntry>, String> {
        let folder = self.preset_library_dir();
        let listing = match fs::read_dir(&folder) {
            Ok(listing) => listing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("Could not read the preset library: {error}")),
        };
        let mut entries = Vec::new();
        for item in listing.flatten() {
            let path = item.path();
            let is_preset = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(PRESET_EXTENSION));
            if !is_preset || !item.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if entries.len() >= MAX_LIBRARY_PRESETS {
                break;
            }
            let read = read_capped(&path, MAX_PRESET_FILE_BYTES)
                .and_then(|bytes| Preset::from_json(&bytes).map(|(preset, _)| preset));
            match read {
                Ok(preset) => entries.push(entry_of(&path, &preset)),
                Err(error) => log::warn!("preset library: {}: {error}", path.display()),
            }
        }
        entries.sort_by(|a, b| {
            (a.name.to_lowercase(), &a.path).cmp(&(b.name.to_lowercase(), &b.path))
        });
        Ok(entries)
    }

    /// Saves a preset into the library under `name`, replacing the preset
    /// of that name if there is one.
    pub fn preset_library_save(
        &self,
        name: &str,
        mut capture: PresetCapture,
    ) -> Result<PresetEntry, String> {
        let stem = safe_file_stem(name)?;
        capture.preset.name = bounded_name(name.trim());
        let target = self
            .preset_library_dir()
            .join(format!("{stem}.{PRESET_EXTENSION}"));
        let written = self.preset_write(&paths::display(&target), capture)?;
        let review = self.preset_read(&written)?;
        Ok(entry_of(Path::new(&written), &review.preset))
    }

    /// Deletes a preset of the library, with the audio folder that is its
    /// own. A path outside the library is refused.
    pub fn preset_library_delete(&self, path: &str) -> Result<(), String> {
        let folder = paths::clean(&self.preset_library_dir());
        let target = paths::absolute(path)?;
        let inside = target
            .parent()
            .is_some_and(|parent| paths::same(parent, &folder));
        let is_preset = target
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(PRESET_EXTENSION));
        if !inside || !is_preset {
            return Err("Only a preset of the preset library can be deleted here.".into());
        }
        fs::remove_file(&target)
            .map_err(|error| format!("Could not delete the preset: {error}"))?;
        let audio = folder.join(format!("{} audio", paths::stem(&target)));
        if audio.is_dir()
            && let Err(error) = fs::remove_dir_all(&audio)
        {
            log::warn!(
                "preset library: could not remove {}: {error}",
                audio.display()
            );
        }
        Ok(())
    }
}

fn entry_of(path: &Path, preset: &Preset) -> PresetEntry {
    PresetEntry {
        path: paths::display(path),
        name: if preset.name.is_empty() {
            paths::stem(path)
        } else {
            preset.name.clone()
        },
        kind: preset.kind,
        plugin: preset.plugin_identity(),
    }
}

/// Every hosted plugin a preset carries.
fn hosted(preset: &Preset) -> Vec<&PluginBinding> {
    let mut bindings = Vec::new();
    if let Some(channel) = &preset.channel {
        bindings.extend(&channel.plugin);
    }
    if let Some(plugin) = &preset.plugin
        && let PluginPresetState::Hosted { binding, .. } = &plugin.state
    {
        bindings.push(binding);
    }
    if let Some(track) = &preset.mixer_track {
        bindings.extend(&track.plugins);
    }
    bindings
}

/// Where an asset's audio is: its relative path under the preset's folder.
/// `None` for a path that would leave the folder.
fn asset_file(folder: &Path, asset: &PresetAsset) -> Option<PathBuf> {
    file::resolve_sample_path(
        &SamplePath::Project(asset.path.clone()),
        Some(folder),
        Path::new(""),
    )
}

fn with_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PRESET_EXTENSION));
    if has {
        path
    } else {
        PathBuf::from(format!("{}.{PRESET_EXTENSION}", paths::display(&path)))
    }
}

/// A name a preset accepts: no control characters, at most 256 bytes.
fn bounded_name(name: &str) -> String {
    let mut name: String = name.chars().filter(|c| !c.is_control()).collect();
    while name.len() > MAX_PRESET_NAME_BYTES {
        name.pop();
    }
    name.trim().to_owned()
}

/// The file name of an audio file, made safe to be one part of a relative
/// path on every platform.
fn file_name_of(file: &Path) -> Result<String, String> {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("The preset's audio has no usable file name.")?;
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    };
    let stem = safe_file_stem(stem)?;
    Ok(match extension {
        Some(extension) => format!("{stem}.{}", safe_part(extension)),
        None => stem,
    })
}

fn safe_part(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// A file name without its extension that every platform accepts, made from
/// what the user typed.
pub(super) fn safe_file_stem(name: &str) -> Result<String, String> {
    let cleaned: String = safe_part(name.trim())
        .chars()
        .take(MAX_FILE_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim_start().to_owned();
    let upper = cleaned.to_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.ends_with(|c: char| c.is_ascii_digit()));
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." || device {
        return Err(format!("\"{name}\" cannot be used as a file name."));
    }
    Ok(cleaned)
}

/// Reads a file that may be at most `limit` bytes long.
fn read_capped(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let opened = File::open(path).map_err(|error| format!("Could not open the preset: {error}"))?;
    let mut bytes = Vec::new();
    opened
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the preset: {error}"))?;
    if bytes.len() > limit {
        return Err("The preset file is larger than 256 MiB.".into());
    }
    Ok(bytes)
}

/// Copies a preset's audio into place and returns its size. The copy is
/// staged beside its destination and renamed onto it, so a failed copy
/// leaves no half file there.
fn copy_audio(source: &Path, destination: &Path) -> Result<u64, String> {
    let failed = |error: io::Error| {
        format!(
            "The preset's audio \"{}\" could not be copied: {error}",
            paths::name(source)
        )
    };
    let metadata = fs::metadata(source).map_err(failed)?;
    if !metadata.is_file() {
        return Err(format!(
            "The preset's audio \"{}\" is not a file.",
            paths::name(source)
        ));
    }
    if metadata.len() > MAX_PRESET_ASSET_BYTES {
        return Err(format!(
            "The preset's audio \"{}\" is larger than a preset can bring along.",
            paths::name(source)
        ));
    }
    if paths::same(&paths::clean(source), &paths::clean(destination)) {
        return Ok(metadata.len());
    }
    let folder = destination.parent().ok_or("The preset has no folder.")?;
    fs::create_dir_all(folder).map_err(failed)?;
    let mut staged = tempfile::NamedTempFile::new_in(folder).map_err(failed)?;
    let mut input = File::open(source).map_err(failed)?;
    let copied = io::copy(&mut input, staged.as_file_mut()).map_err(failed)?;
    staged.as_file().sync_all().map_err(failed)?;
    staged
        .persist(destination)
        .map_err(|error| failed(error.error))?;
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe_and_device_names_are_refused() {
        assert_eq!(safe_file_stem("  Deep kick  ").unwrap(), "Deep kick");
        assert_eq!(safe_file_stem("a/b\\c:d*e?").unwrap(), "a_b_c_d_e_");
        assert_eq!(safe_file_stem("trailing. . ").unwrap(), "trailing");
        assert_eq!(
            safe_file_stem(&"x".repeat(500)).unwrap().chars().count(),
            120
        );
        for name in ["", "   ", ".", "..", "CON", "nul", "com1", "LPT9"] {
            assert!(safe_file_stem(name).is_err(), "{name:?}");
        }
        assert_eq!(safe_file_stem("COMET").unwrap(), "COMET");
        assert_eq!(
            file_name_of(Path::new("C:/sounds/Kick 01.WAV")).unwrap(),
            "Kick 01.WAV"
        );
        assert_eq!(file_name_of(Path::new("/sounds/noext")).unwrap(), "noext");
        assert_eq!(bounded_name("a\u{7}b\n"), "ab");
        assert!(bounded_name(&"é".repeat(400)).len() <= MAX_PRESET_NAME_BYTES);
    }

    #[test]
    fn the_extension_is_added_only_when_missing() {
        assert_eq!(
            with_extension(PathBuf::from("C:/presets/Kick")),
            PathBuf::from("C:/presets/Kick.wfpreset")
        );
        assert_eq!(
            with_extension(PathBuf::from("C:/presets/Kick.WFPRESET")),
            PathBuf::from("C:/presets/Kick.WFPRESET")
        );
        assert_eq!(
            with_extension(PathBuf::from("C:/presets/Kick.v2")),
            PathBuf::from("C:/presets/Kick.v2.wfpreset")
        );
    }

    #[test]
    fn an_assets_path_cannot_leave_the_presets_folder() {
        let folder = Path::new("C:/presets");
        let asset = |path: &str| PresetAsset {
            id: 1,
            name: "Kick".into(),
            path: path.into(),
            bytes: 0,
        };
        assert_eq!(
            asset_file(folder, &asset("Kick audio/Kick.wav")),
            Some(folder.join("Kick audio").join("Kick.wav"))
        );
        for path in [
            "../Kick.wav",
            "a/../../Kick.wav",
            "/Kick.wav",
            "a\\Kick.wav",
            "",
        ] {
            assert_eq!(asset_file(folder, &asset(path)), None, "{path:?}");
        }
    }
}
