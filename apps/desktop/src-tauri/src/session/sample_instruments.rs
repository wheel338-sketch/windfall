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
    /// Decode a source and attach one mapping/layer in one undo step. No sampler
    /// channel is created as a side effect, and existing source IDs are reused.
    pub fn sample_instrument_import_zone(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        path: String,
        pad: Option<u8>,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, mut zones) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current = state
                .document
                .project()
                .channel(channel)
                .ok_or("channel no longer exists")?;
            let ChannelSource::Instrument { params, .. } = current.source else {
                return Err("choose a sample-bank instrument".into());
            };
            if current.sample_instrument != expected {
                return Err("zone bank changed; retry the import".into());
            }
            match params.kind() {
                InstrumentKind::MultiSampler if pad.is_none() => {}
                InstrumentKind::PadBank if pad.is_some_and(|key| (36..=51).contains(&key)) => {}
                _ => return Err(
                    "source import requires a multisampler zone or one of the sixteen drum pads"
                        .into(),
                ),
            }
            let zones = match &expected {
                Some(SampleInstrumentAsset::Zones { zones }) => zones.clone(),
                None => Vec::new(),
                _ => return Err("this instrument has no editable zone bank".into()),
            };
            if zones.len() >= windfall_dsp::sample_bank::MAX_BANK_ZONES {
                return Err("a bank may have at most 256 zones".into());
            }
            (state.generation, state.edits, state.replacements, zones)
        };
        let file = crate::paths::absolute(&path)?;
        let audio = self.inner.cache.decode(&file)?;
        #[cfg(test)]
        self.pause("sample-zone:decoded");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation
                || state.edits != edits
                || state.replacements != replacements
            {
                return Err("project changed while the source was decoded".into());
            }
            let path = SamplePath::External(crate::paths::display(&file));
            let source = state
                .document
                .project()
                .samples
                .iter()
                .find(|sample| sample.path == path)
                .map_or(SampleId(state.document.project().next_id), |sample| {
                    sample.id
                });
            zones.push(windfall_dsp::sample_bank::SampleZone {
                sample: source.0,
                key_low: pad.unwrap_or(0),
                key_high: pad.unwrap_or(127),
                root_key: pad.unwrap_or(60),
                ..Default::default()
            });
            let command = Command::Batch {
                label: Some("Import sample-bank source".into()),
                commands: vec![
                    Command::AddSample {
                        name: crate::paths::stem(&file),
                        path,
                    },
                    Command::SetSampleInstrumentAsset {
                        channel,
                        expected,
                        asset: Some(SampleInstrumentAsset::Zones { zones }),
                    },
                ],
            };
            self.sample_edit_ticket(&state, command, None, vec![(source, audio)])?
        };
        self.finish_sample_edit(ticket)
    }
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
                ChannelSource::Instrument { params, .. } if params.kind() == InstrumentKind::TextVoice)
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
