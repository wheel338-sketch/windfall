//! Native sample-bank IO and speech rendering, on IPC workers only.
use super::Session;
use std::io::Read;
use std::path::{Path, PathBuf};
use windfall_dsp::sample_bank::{soundfont, sources};
use windfall_project::{
    ChannelId, ChannelSource, Command, DispatchResult, InstrumentKind, SampleId,
    SampleInstrumentAsset, SamplePath,
};

use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[derive(Default)]
pub(super) struct SpeechJobs {
    next: u64,
    jobs: HashMap<ChannelId, (u64, Arc<AtomicBool>)>,
}
struct SpeechLease { session: Session, channel: ChannelId, request: u64, cancelled: Arc<AtomicBool> }
impl Drop for SpeechLease {
    fn drop(&mut self) {
        let mut jobs = super::lock(&self.session.inner.speech_jobs);
        if jobs.jobs.get(&self.channel).is_some_and(|(id, _)| *id == self.request) {
            jobs.jobs.remove(&self.channel);
        }
    }
}

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
        let (generation, edits, replacements, mut zones, budget) = {
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
            (state.generation, state.edits, state.replacements, zones, state.pool.sample_bank_budget_handle())
        };
        let file = crate::paths::absolute(&path)?;
        let (audio, _workspace) = self.inner.cache.decode_sample_bank(&file, &budget)?;
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
    /// General MIDI equivalent: keep the rack channel/notes/mixer assignment,
    /// replace the external destination with the user's own SF2 bank, and use
    /// the same Windfall renderer for both live output and offline export.
    pub fn sample_instrument_general_midi(
        &self, channel: ChannelId, expected: ChannelSource, path: String, bank: u16, program: u16,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, preset) = {
            let _recording=self.recording_idle()?;let state=self.state();let project=state.document.project();
            let current=project.channel(channel).ok_or("channel no longer exists")?;
            if current.source!=expected || !matches!(current.source,ChannelSource::Instrument{params,..} if params.kind()==InstrumentKind::MidiOut) {
                return Err("MIDI output instrument changed; retry choosing the GM bank".into());
            }
            let target=windfall_project::PluginTarget::Instrument{channel};
            if project.plugin(target).is_some(){return Err("Choose an unhosted MIDI output instrument".into());}
            (state.generation,state.edits,state.replacements,windfall_project::PluginPreset::capture(project,target).map_err(|e|e.to_string())?.state)
        };
        let file=crate::paths::absolute(&path)?;let bytes=read_soundfont(&file)?;
        let asset=SampleInstrumentAsset::SoundFont{path:SamplePath::External(crate::paths::display(&file)),bank,program};
        let command=Command::Batch{label:Some("Use SoundFont General MIDI destination".into()),commands:vec![
            Command::ApplyPluginPreset{target:windfall_project::PluginTarget::Instrument{channel},expected:preset,preset:windfall_project::PluginPreset{state:windfall_project::PluginPresetState::Instrument{asset:None,params:InstrumentKind::SoundFont.default_params()}}},
            Command::SetSampleInstrumentAsset{channel,expected:None,asset:Some(asset.clone())},
        ]};
        let ticket={let _recording=self.recording_idle()?;let state=self.state();
            if (state.generation,state.edits,state.replacements)!=(generation,edits,replacements){return Err("Project changed while the GM bank was read".into());}
            self.sample_edit_ticket(&state,command,None,Vec::new())?
        }.with_soundfont(asset,&bytes)?;
        #[cfg(test)] self.pause("general-midi:prepared");
        self.finish_sample_edit(ticket)
    }
    pub fn sample_instrument_missing_sources(&self, channel: ChannelId) -> Result<Vec<SampleId>, String> {
        let state = self.state();
        let current = state.document.project().channel(channel).ok_or("channel no longer exists")?;
        let mut missing: Vec<_> = current.sample_instrument.as_ref().into_iter().flat_map(SampleInstrumentAsset::samples)
            .filter(|id| !state.pool.contains(*id)).collect();
        missing.sort_by_key(|id| id.0); missing.dedup(); Ok(missing)
    }
    pub fn sample_instrument_relink_source(
        &self, channel: ChannelId, expected: Option<SampleInstrumentAsset>, sample: SampleId, expected_path: SamplePath, path: String,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, budget) = {
            let _recording = self.recording_idle()?; let state = self.state();
            let current = state.document.project().channel(channel).ok_or("channel no longer exists")?;
            if current.sample_instrument != expected || !expected.as_ref().is_some_and(|asset| asset.samples().any(|id| id == sample)) {
                return Err("sample-bank mapping changed; retry relinking".into());
            }
            if state.document.project().sample(sample).is_none_or(|asset| asset.path != expected_path) {
                return Err("sample path changed; retry relinking".into());
            }
            (state.generation, state.edits, state.replacements, state.pool.sample_bank_budget_handle())
        };
        let file = crate::paths::absolute(&path)?;
        let (audio, _workspace) = self.inner.cache.decode_sample_bank(&file, &budget)?;
        #[cfg(test)] self.pause("sample-relink:decoded");
        let ticket = {
            let _recording = self.recording_idle()?; let state = self.state();
            if (state.generation, state.edits, state.replacements) != (generation, edits, replacements) { return Err("project changed while relinking".into()); }
            self.sample_edit_ticket(&state, Command::RelinkSamplePath { sample, expected: expected_path, path: SamplePath::External(crate::paths::display(&file)) }, None, vec![(sample, audio)])?
        };
        self.finish_sample_edit(ticket)
    }
    /// Allocate the request synchronously before dispatching a rendering worker,
    /// so cancellation also works while that worker is waiting to be scheduled.
    pub fn sample_instrument_speech_begin(&self, channel: ChannelId) -> Result<u64, String> {
        let state = self.state();
        if !state.document.project().channel(channel).is_some_and(|c|
            matches!(c.source, ChannelSource::Instrument { params, .. } if params.kind() == InstrumentKind::TextVoice)) {
            return Err("choose a text voice instrument".into());
        }
        let mut jobs = super::lock(&self.inner.speech_jobs);
        if jobs.jobs.len() >= 64 && !jobs.jobs.contains_key(&channel) {
            return Err("At most 64 speech requests can be pending".into());
        }
        jobs.next = jobs.next.checked_add(1).ok_or("speech request sequence exhausted")?;
        let request = jobs.next;
        if let Some((_, cancelled)) = jobs.jobs.insert(channel, (request, Arc::new(AtomicBool::new(false)))) {
            cancelled.store(true, Ordering::Release);
        }
        Ok(request)
    }
    pub fn sample_instrument_speech_cancel(&self, channel: ChannelId, request: u64) {
        let _state = self.state();
        if let Some((id, cancelled)) = super::lock(&self.inner.speech_jobs).jobs.get(&channel)
            && *id == request {
            cancelled.store(true, Ordering::Release);
        }
    }
    pub fn sample_instrument_speech(
        &self, channel: ChannelId, expected: Option<SampleInstrumentAsset>,
        text: String, pitch: f32, duration: f32,
    ) -> Result<DispatchResult, String> {
        let request = self.sample_instrument_speech_begin(channel)?;
        self.sample_instrument_speech_request(channel, expected, text, pitch, duration, request)
    }
    /// Render, save an owned Float32 source and attach it in one guarded undo
    /// step. Refused/stale renders remove their temporary source on the worker.
    pub fn sample_instrument_speech_request(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        text: String,
        pitch: f32,
        duration: f32,
        request: u64,
    ) -> Result<DispatchResult, String> {
        let cancelled = super::lock(&self.inner.speech_jobs).jobs.get(&channel)
            .filter(|(id, _)| *id == request).map(|(_, flag)| flag.clone())
            .ok_or("speech request is no longer active")?;
        let lease = SpeechLease { session: self.clone(), channel, request, cancelled };
        let keep_going = || !lease.cancelled.load(Ordering::Acquire);
        if !keep_going() { return Err("Speech rendering cancelled".into()); }
        let (generation, edits, replacements, budget) = {
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
            (state.generation, state.edits, state.replacements, state.pool.sample_bank_budget_handle())
        };
        // Bound both render/decode workspaces before allocating source PCM.
        if !duration.is_finite() || !(0.1..=60.0).contains(&duration) { return Err("invalid speech duration".into()); }
        let _workspace = budget.reserve((duration * 24000.0).round() as usize * 4 * 2)?;
        let audio = sources::render_text(&text, pitch, duration, &mut || keep_going())
            .map_err(|e| e.to_string())?;
        if !keep_going() { return Err("Speech rendering cancelled".into()); }
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
        if !keep_going() { return Err("Speech rendering cancelled".into()); }
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
        let result = self.finish_sample_edit_cancellable(ticket, &keep_going)?;
        file.0 = PathBuf::new();
        Ok(result)
    }
}
