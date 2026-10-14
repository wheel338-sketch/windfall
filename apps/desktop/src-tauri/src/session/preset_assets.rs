//! Preserve original preset bytes before installing an imported sound. IO is
//! off the document/audio locks; failed imports remove only their own file.
use super::{Session, library::Import};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use windfall_core::AudioBuffer;

static SERIAL: AtomicU64 = AtomicU64::new(0);
pub(crate) fn read_original(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let limit = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wfpreset"))
    {
        windfall_engine::preset_render::MAX_PRESET_BYTES
    } else {
        windfall_codec::MAX_DRUMSYNTH_PRESET_BYTES
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(windfall_ipc::ui_message(
            "presets.sourceTooLarge",
            serde_json::json!({"limit": limit}),
        ));
    }
    Ok(bytes)
}
pub(super) struct OwnedPreset {
    pub path: PathBuf,
    pub rendered: PathBuf,
    pub rate: u32,
    pub original: PathBuf,
    pub original_version: crate::samples::FileKey,
    pub bytes: Vec<u8>,
    pub settings: Option<windfall_project::PresetRenderSettings>,
    pub installed: bool,
    rendered_owned: bool,
}
impl Drop for OwnedPreset {
    fn drop(&mut self) {
        if !self.installed {
            let _ = fs::remove_file(&self.path);
            if self.rendered_owned {
                let _ = fs::remove_file(&self.rendered);
            }
        }
    }
}
impl Session {
    pub(super) fn own_preset(&self, import: &mut Import) -> Option<OwnedPreset> {
        import.owned.take()
    }

    /// Both public DrumSynth and Windfall recipes use this same RAII ownership
    /// and subsequent import batch. No installed asset can be overwritten.
    pub(super) fn stage_preset(
        &self,
        original: &std::path::Path,
        bytes: Vec<u8>,
        audio: &AudioBuffer,
        original_version: crate::samples::FileKey,
    ) -> Result<OwnedPreset, String> {
        let folder = self.store().recordings_dir().join("Imported presets");
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut staged = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        staged
            .as_file_mut()
            .write_all(&bytes)
            .map_err(|e| e.to_string())?;
        staged.as_file().sync_all().map_err(|e| e.to_string())?;
        let mut published = None;
        for _ in 0..1000 {
            let path = folder.join(format!(
                "{}-{}-{}.{}",
                crate::paths::stem(original),
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed),
                original
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("wfpreset")
            ));
            match staged.persist_noclobber(&path) {
                Ok(_) => {
                    published = Some(path);
                    break;
                }
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    staged = error.file
                }
                Err(error) => return Err(error.error.to_string()),
            }
        }
        let path = published.ok_or("Could not reserve an owned preset filename.")?;
        // The sampler owns a rendered WAV at the current project engine rate.
        // The original text is also registered as a project sample asset so
        // ordinary save/save-as/archive carrying preserves both independently.
        let rendered = path.with_extension("wav");
        let rate = audio.sample_rate();
        let mut owned = OwnedPreset {
            path: path.clone(),
            rendered: rendered.clone(),
            rate,
            original: original.to_path_buf(),
            original_version,
            bytes,
            settings: None,
            installed: false,
            rendered_owned: false,
        };
        let staged_wav = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        windfall_codec::write_wav(
            staged_wav.path(),
            audio,
            windfall_codec::WavSampleFormat::Float32,
        )
        .map_err(|e| e.to_string())?;
        staged_wav.as_file().sync_all().map_err(|e| e.to_string())?;
        staged_wav
            .persist_noclobber(&rendered)
            .map_err(|e| e.error.to_string())?;
        owned.rendered_owned = true;
        Ok(owned)
    }
}
