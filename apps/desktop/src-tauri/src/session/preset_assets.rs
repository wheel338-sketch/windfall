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
pub(super) fn read_original(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(windfall_codec::MAX_DRUMSYNTH_PRESET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
pub(super) struct OwnedPreset {
    pub path: PathBuf,
    pub original: PathBuf,
    pub original_audio: AudioBuffer,
    pub bytes: Vec<u8>,
    pub installed: bool,
}
impl Drop for OwnedPreset {
    fn drop(&mut self) {
        if !self.installed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
impl Session {
    pub(super) fn own_preset(&self, import: &mut Import) -> Result<Option<OwnedPreset>, String> {
        if !import
            .file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ds"))
        {
            return Ok(None);
        }
        let bytes = read_original(&import.file)?;
        let render = windfall_codec::render_drumsynth_source(
            &bytes,
            &windfall_codec::DecodeOptions::default(),
        )
        .map_err(|e| e.to_string())?;
        let current = self.inner.cache.decode(&import.file)?;
        if !self.inner.cache.same_file_version(&import.buffer, &current)
            || render.audio.samples() != import.buffer.samples()
        {
            return Err("The preset source changed before ownership was copied. Try again.".into());
        }
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
                "{}-{}-{}.ds",
                crate::paths::stem(&import.file),
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
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
        let owned = OwnedPreset {
            path: path.clone(),
            original: import.file.clone(),
            original_audio: import.buffer.clone(),
            bytes,
            installed: false,
        };
        import.file = path;
        import.buffer = self.inner.cache.decode(&import.file)?;
        Ok(Some(owned))
    }
}
