//! Native sample-bank IO and speech rendering, on IPC workers only.
use super::Session;
use std::io::Read;
use std::path::{Path, PathBuf};
use windfall_dsp::sample_bank::{soundfont, sources};
use windfall_project::{
    ChannelId, ChannelSource, Command, DispatchResult, InstrumentKind, SampleId,
    SampleInstrumentAsset, SamplePath,
};

pub(super) fn read_soundfont(path: &Path) -> Result<Vec<u8>, String> {
    // Read through a hard bound too: a racing file growth cannot bypass metadata.
    const LIMIT: u64 = 160 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("Could not open SoundFont: {e}"))?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read SoundFont: {e}"))?;
    if bytes.len() as u64 > LIMIT {
        return Err("SoundFont file exceeds 160 MiB".into());
    }
    Ok(bytes)
}

struct RenderedFile(PathBuf);
impl Drop for RenderedFile {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
impl Session {
    pub fn soundfont_presets(&self, path: &str) -> Result<Vec<soundfont::SoundFontPreset>, String> {
        let file = crate::paths::absolute(path)?;
        soundfont::presets(&read_soundfont(&file)?).map_err(|e| e.to_string())
    }
    /// Render, save an owned Float32 source and attach it in one guarded undo
    /// step. Refused/stale renders remove their temporary source on the worker.
    pub fn sample_instrument_speech(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        text: String,
        pitch: f32,
        duration: f32,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current = state
                .document
                .project()
                .channel(channel)
                .ok_or("channel no longer exists")?;
            if current.sample_instrument != expected
                || !matches!(current.source,
                ChannelSource::Instrument { params } if params.kind() == InstrumentKind::TextVoice)
            {
                return Err("text voice changed; reopen its editor".into());
            }
            (state.generation, state.edits, state.replacements)
        };
        let audio = sources::render_text(&text, pitch, duration, &mut || true)
            .map_err(|e| e.to_string())?;
        let folder = self.store().recordings_dir().join("Text voice");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = tempfile::Builder::new()
            .prefix("Speech-")
            .suffix(".wav")
            .tempfile_in(&folder)
            .map_err(|e| e.to_string())?
            .into_temp_path()
            .keep()
            .map_err(|e| e.to_string())?;
        let mut file = RenderedFile(path);
        windfall_codec::write_wav(&file.0, &audio, windfall_codec::WavSampleFormat::Float32)
            .map_err(|e| e.to_string())?;
        let decoded = self.inner.cache.decode(&file.0)?;
        #[cfg(test)]
        self.pause("sample-speech:rendered");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation
                || state.edits != edits
                || state.replacements != replacements
            {
                return Err("project changed while speech was rendered".into());
            }
            let source = SampleId(state.document.project().next_id);
            let command = Command::Batch {
                label: Some("Render text voice".into()),
                commands: vec![
                    Command::AddSample {
                        name: "Text voice source".into(),
                        path: SamplePath::External(crate::paths::display(&file.0)),
                    },
                    Command::SetSampleInstrumentAsset {
                        channel,
                        expected,
                        asset: Some(SampleInstrumentAsset::Speech {
                            text,
                            pitch,
                            duration,
                            version: 1,
                            source: Some(source),
                        }),
                    },
                ],
            };
            self.sample_edit_ticket(&state, command, None, vec![(source, decoded)])?
        };
        let result = self.finish_sample_edit(ticket)?;
        file.0 = PathBuf::new();
        Ok(result)
    }
}
