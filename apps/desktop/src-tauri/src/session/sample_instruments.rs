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
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub(super) struct SpeechJobs {
    next: u64,
    jobs: HashMap<ChannelId, (u64, Arc<AtomicBool>)>,
}
struct SpeechLease {
    session: Session,
    channel: ChannelId,
    request: u64,
    cancelled: Arc<AtomicBool>,
}
impl Drop for SpeechLease {
    fn drop(&mut self) {
        let mut jobs = super::lock(&self.session.inner.speech_jobs);
        if jobs
            .jobs
            .get(&self.channel)
            .is_some_and(|(id, _)| *id == self.request)
        {
            jobs.jobs.remove(&self.channel);
        }
    }
}

pub(super) fn read_soundfont(
    path: &Path,
    budget: &Arc<windfall_dsp::sample_bank::BankBudget>,
) -> Result<(Vec<u8>, windfall_dsp::sample_bank::BankReservation), String> {
    read_soundfont_with_limit(path, budget, 160 * 1024 * 1024)
}

pub(super) fn read_soundfont_with_limit(
    path: &Path,
    budget: &Arc<windfall_dsp::sample_bank::BankBudget>,
    limit: u64,
) -> Result<(Vec<u8>, windfall_dsp::sample_bank::BankReservation), String> {
    let mut file = std::fs::File::open(path).map_err(|e| {
        windfall_ipc::ui_message(
            "soundFonts.openFailed",
            serde_json::json!({"error": e.to_string()}),
        )
    })?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.len() > limit {
        if limit == 160 * 1024 * 1024 {
            return Err("SoundFont file exceeds 160 MiB".into());
        }
        return Err(windfall_ipc::ui_message(
            "soundFonts.tooLarge",
            serde_json::json!({"limit": limit}),
        ));
    }
    let size = usize::try_from(metadata.len()).map_err(|_| "SoundFont file size overflow")?;
    let reservation = budget.reserve(size)?;
    // Exact allocation avoids read_to_end growth temporarily doubling the PCM
    // bytes in the encoded bank. A growth/truncation never expands this buffer.
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes).map_err(|e| {
        windfall_ipc::ui_message(
            "soundFonts.readFailed",
            serde_json::json!({"error": e.to_string()}),
        )
    })?;
    let mut extra = [0; 1];
    if file.read(&mut extra).map_err(|e| e.to_string())? != 0
        || std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .modified()
            .ok()
            != metadata.modified().ok()
    {
        return Err("SoundFont file changed while reading; retry".into());
    }
    Ok((bytes, reservation))
}
pub(super) fn is_soundfont_file(path: &Path) -> bool {
    let mut header = [0u8; 12];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header[..4] == b"RIFF"
        && &header[8..] == b"sfbk"
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
            (
                state.generation,
                state.edits,
                state.replacements,
                zones,
                state.pool.sample_bank_budget_handle(),
            )
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
        let file = self.soundfont_file(path)?;
        let budget = self.state().pool.sample_bank_budget_handle();
        let (bytes, _workspace) = read_soundfont(&file, &budget)?;
        soundfont::presets(&bytes).map_err(|e| e.to_string())
    }
    fn soundfont_file(&self, text: &str) -> Result<PathBuf, String> {
        // The editor displays persisted project/factory paths after archive
        // reopen. Resolve that exact owned reference against its project root.
        let state = self.state();
        let project = state.document.project();
        for asset in project
            .channels
            .iter()
            .filter_map(|c| c.sample_instrument.as_ref())
        {
            if let Some(path) = asset.soundfont_path(project) {
                let (SamplePath::External(stored)
                | SamplePath::Project(stored)
                | SamplePath::Factory(stored)) = path;
                if stored == text {
                    return windfall_project::file::resolve_sample_path(
                        path,
                        state.sample_dir.as_deref(),
                        &self.inner.factory_dir,
                    )
                    .ok_or_else(|| "SoundFont source path cannot be resolved".into());
                }
            }
        }
        crate::paths::absolute(text)
    }
    /// Import the selected preset and attach its source identity in one history step.
    pub fn soundfont_load(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        path: String,
        bank: u16,
        program: u16,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current = state
                .document
                .project()
                .channel(channel)
                .ok_or("Channel no longer exists")?;
            if current.sample_instrument != expected
                || !matches!(current.source,ChannelSource::Instrument{params,..} if params.kind()==InstrumentKind::SoundFont)
            {
                return Err("SoundFont selection changed; reload its editor".into());
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                state.pool.sample_bank_budget_handle(),
            )
        };
        let file = self.soundfont_file(&path)?;
        let (encoded, _lease) = read_soundfont(&file, &budget)?;
        let (ticket, asset) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("Project changed while importing SoundFont".into());
            }
            let project = state.document.project();
            let existing = project.samples.iter().find(|s| {
                windfall_project::file::resolve_sample_path(
                    &s.path,
                    state.sample_dir.as_deref(),
                    &self.inner.factory_dir,
                )
                .is_some_and(|p| crate::paths::clean(&p) == crate::paths::clean(&file))
            });
            let (source, path) = existing.map_or(
                (
                    SampleId(project.next_id),
                    SamplePath::External(crate::paths::display(&file)),
                ),
                |s| (s.id, s.path.clone()),
            );
            let asset = SampleInstrumentAsset::SoundFont {
                path: path.clone(),
                bank,
                program,
                source: Some(source),
            };
            let command = Command::Batch {
                label: Some("Load SoundFont preset".into()),
                commands: vec![
                    Command::AddSample {
                        name: crate::paths::stem(&file),
                        path: path.clone(),
                    },
                    Command::RegisterSoundFontSource {
                        sample: source,
                        expected: path,
                    },
                    Command::SetSampleInstrumentAsset {
                        channel,
                        expected,
                        asset: Some(asset.clone()),
                    },
                ],
            };
            (
                self.sample_edit_ticket(&state, command, None, Vec::new())?,
                asset,
            )
        };
        let ticket = ticket.with_soundfont(asset, &encoded)?;
        #[cfg(test)]
        self.pause("soundfont:prepared");
        self.finish_sample_edit(ticket)
    }
    /// An agent's approved SoundFont load: the proposal's own commands, then
    /// the load, as one undo step named after the proposal, only while the
    /// project is still as the proposal saw it. `file` was confined to the
    /// browser's folders by the caller.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mcp_soundfont_apply(
        &self,
        proposal: &windfall_project::proposal::CommandProposal,
        channel: ChannelId,
        file: &Path,
        bank: u16,
        program: u16,
        replace_instrument: bool,
    ) -> Result<DispatchResult, String> {
        if !proposal.commands.is_empty() {
            // The commands an agent may propose, and their bounds.
            proposal.batch()?;
        }
        self.soundfont_batch(
            proposal.commands.clone(),
            proposal.label.clone(),
            Some(channel),
            Some(proposal.stamp),
            file,
            (bank, program),
            replace_instrument,
        )
    }

    /// A new SoundFont channel playing one preset of `path`, named after
    /// the preset unless `name` is given, as one undo step: the browser's
    /// and the General MIDI instrument choices.
    pub fn soundfont_add_channel(
        &self,
        path: String,
        bank: u16,
        program: u16,
        name: Option<String>,
    ) -> Result<DispatchResult, String> {
        let file = crate::paths::absolute(&path)?;
        let name = match name.filter(|name| !name.trim().is_empty()) {
            Some(name) => name,
            None => self
                .soundfont_presets(&crate::paths::display(&file))?
                .into_iter()
                .find(|preset| preset.bank == bank && preset.program == program)
                .map(|preset| preset.name)
                .ok_or("That preset is not in the SoundFont")?,
        };
        self.soundfont_batch(
            vec![Command::AddChannel {
                name: Some(name),
                sample: None,
                instrument: Some(InstrumentKind::SoundFont),
                index: None,
                mixer_track: None,
            }],
            "Add SoundFont channel".into(),
            None,
            None,
            &file,
            (bank, program),
            false,
        )
    }

    /// `prefix`, then one preset of `file` loaded into `channel` (or into
    /// the first channel `prefix` makes), as one undo step named `label`.
    /// With `stamp` only while the project is as that stamp says.
    #[allow(clippy::too_many_arguments)]
    fn soundfont_batch(
        &self,
        prefix: Vec<Command>,
        label: String,
        channel: Option<ChannelId>,
        stamp: Option<windfall_project::proposal::ProposalStamp>,
        file: &Path,
        (bank, program): (u16, u16),
        replace_instrument: bool,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if stamp.is_some_and(|stamp| {
                stamp.generation != state.generation || stamp.revision != state.document.revision()
            }) {
                return Err("proposal is stale; generate a new preview".into());
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                state.pool.sample_bank_budget_handle(),
            )
        };
        let (encoded, _lease) = read_soundfont(file, &budget)?;
        let (ticket, asset) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("Project changed while importing SoundFont".into());
            }
            let mut commands = prefix;
            let mut candidate = state.document.clone();
            let mut created = Vec::new();
            if !commands.is_empty() {
                created = candidate
                    .dispatch(
                        Command::Batch {
                            label: None,
                            commands: commands.clone(),
                        },
                        None,
                    )
                    .map_err(|e| e.to_string())?
                    .created;
            }
            let channel = match channel {
                Some(channel) => channel,
                None => created
                    .iter()
                    .map(|id| ChannelId(*id))
                    .find(|id| candidate.project().channel(*id).is_some())
                    .ok_or("no channel to load the SoundFont into")?,
            };
            let existing = candidate.project().samples.iter().find(|s| {
                windfall_project::file::resolve_sample_path(
                    &s.path,
                    state.sample_dir.as_deref(),
                    &self.inner.factory_dir,
                )
                .is_some_and(|p| crate::paths::clean(&p) == crate::paths::clean(file))
            });
            let (path, existing) = existing.map_or(
                (SamplePath::External(crate::paths::display(file)), None),
                |s| (s.path.clone(), Some(s.id)),
            );
            let (load, asset) = windfall_project::sample_instrument::soundfont_load_commands(
                &candidate,
                channel,
                crate::paths::stem(file),
                path,
                existing,
                bank,
                program,
                replace_instrument,
            )?;
            commands.extend(load);
            let command = Command::Batch {
                label: Some(label),
                commands,
            };
            (
                self.sample_edit_ticket(&state, command, None, Vec::new())?,
                asset,
            )
        };
        let ticket = ticket.with_soundfont(asset, &encoded)?;
        #[cfg(test)]
        self.pause("soundfont-batch:prepared");
        self.finish_sample_edit(ticket)
    }

    /// The General MIDI SoundFont the user chose in Settings, if any.
    pub fn default_soundfont(&self) -> Option<String> {
        self.store().settings().default_soundfont.clone()
    }

    /// Chooses the General MIDI SoundFont that General MIDI instruments
    /// play from, or with `None` forgets it. Only a SoundFont file is kept.
    pub fn set_default_soundfont(&self, path: Option<String>) -> Result<Option<String>, String> {
        let path = match path.filter(|path| !path.trim().is_empty()) {
            None => None,
            Some(path) => {
                let file = crate::paths::absolute(&path)?;
                if !is_soundfont_file(&file) {
                    return Err(windfall_ipc::ui_message(
                        "soundFonts.notSoundFont",
                        serde_json::json!({"file": crate::paths::display(&file)}),
                    ));
                }
                Some(crate::paths::display(&file))
            }
        };
        self.store()
            .update(|settings| settings.default_soundfont = path.clone());
        Ok(path)
    }
    /// Replacement validation includes every preset sharing this project-owned
    /// file ID. Malformed/missing presets never change paths, PCM or history.
    pub fn soundfont_relink(
        &self,
        channel: ChannelId,
        expected: SampleInstrumentAsset,
        expected_path: SamplePath,
        path: String,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, source, assets, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            if project
                .channel(channel)
                .is_none_or(|c| c.sample_instrument.as_ref() != Some(&expected))
            {
                return Err("SoundFont selection changed; retry relinking".into());
            }
            let source = expected
                .soundfont_source()
                .ok_or("Reload this legacy bank to give it a project source identity")?;
            if project
                .sample(source)
                .is_none_or(|s| s.path != expected_path)
            {
                return Err("SoundFont source path changed; retry relinking".into());
            }
            let assets = project
                .channels
                .iter()
                .filter_map(|c| c.sample_instrument.as_ref())
                .filter(|a| a.soundfont_source() == Some(source))
                .cloned()
                .collect::<Vec<_>>();
            (
                state.generation,
                state.edits,
                state.replacements,
                source,
                assets,
                state.pool.sample_bank_budget_handle(),
            )
        };
        let file = crate::paths::absolute(&path)?;
        let (encoded, _lease) = read_soundfont(&file, &budget)?;
        let new_path = SamplePath::External(crate::paths::display(&file));
        let mut ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("Project changed while relinking SoundFont".into());
            }
            self.sample_edit_ticket(
                &state,
                Command::RelinkSamplePath {
                    sample: source,
                    expected: expected_path,
                    path: new_path.clone(),
                },
                None,
                Vec::new(),
            )?
        };
        for mut asset in assets {
            if let SampleInstrumentAsset::SoundFont { path, .. } = &mut asset {
                *path = new_path.clone();
            }
            ticket = ticket.with_soundfont(asset, &encoded)?;
        }
        #[cfg(test)]
        self.pause("soundfont-relink:prepared");
        self.finish_sample_edit(ticket)
    }
    /// General MIDI equivalent: keep the rack channel/notes/mixer assignment,
    /// replace the external destination with the user's own SF2 bank, and use
    /// the same Windfall renderer for both live output and offline export.
    pub fn sample_instrument_general_midi(
        &self,
        channel: ChannelId,
        expected: ChannelSource,
        path: String,
        bank: u16,
        program: u16,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, preset, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let current = project.channel(channel).ok_or("channel no longer exists")?;
            if current.source != expected
                || !matches!(current.source,ChannelSource::Instrument{params,..} if params.kind()==InstrumentKind::MidiOut)
            {
                return Err("MIDI output instrument changed; retry choosing the GM bank".into());
            }
            let target = windfall_project::PluginTarget::Instrument { channel };
            if project.plugin(target).is_some() {
                return Err("Choose an unhosted MIDI output instrument".into());
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                windfall_project::PluginPreset::capture(project, target)
                    .map_err(|e| e.to_string())?
                    .state,
                state.pool.sample_bank_budget_handle(),
            )
        };
        let file = crate::paths::absolute(&path)?;
        let (bytes, _workspace) = read_soundfont(&file, &budget)?;
        let source_path = SamplePath::External(crate::paths::display(&file));
        let source = {
            let state = self.state();
            state
                .document
                .project()
                .samples
                .iter()
                .find(|s| s.path == source_path)
                .map_or(SampleId(state.document.project().next_id), |s| s.id)
        };
        let asset = SampleInstrumentAsset::SoundFont {
            path: source_path.clone(),
            bank,
            program,
            source: Some(source),
        };
        let command = Command::Batch {
            label: Some("Use SoundFont General MIDI destination".into()),
            commands: vec![
                Command::AddSample {
                    name: crate::paths::stem(&file),
                    path: source_path.clone(),
                },
                Command::RegisterSoundFontSource {
                    sample: source,
                    expected: source_path,
                },
                Command::ApplyPluginPreset {
                    target: windfall_project::PluginTarget::Instrument { channel },
                    expected: preset,
                    preset: windfall_project::PluginPreset {
                        tables: None,
                        state: windfall_project::PluginPresetState::Instrument {
                            asset: None,
                            params: InstrumentKind::SoundFont.default_params(),
                        },
                    },
                },
                Command::SetSampleInstrumentAsset {
                    channel,
                    expected: None,
                    asset: Some(asset.clone()),
                },
            ],
        };
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("Project changed while the GM bank was read".into());
            }
            self.sample_edit_ticket(&state, command, None, Vec::new())?
        }
        .with_soundfont(asset, &bytes)?;
        #[cfg(test)]
        self.pause("general-midi:prepared");
        self.finish_sample_edit(ticket)
    }
    pub fn sample_instrument_missing_sources(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<SampleId>, String> {
        let state = self.state();
        let current = state
            .document
            .project()
            .channel(channel)
            .ok_or("channel no longer exists")?;
        let mut missing: Vec<_> = current
            .sample_instrument
            .as_ref()
            .into_iter()
            .flat_map(SampleInstrumentAsset::samples)
            .filter(|id| !state.pool.contains(*id))
            .collect();
        if let Some(asset) = &current.sample_instrument
            && let Some(source) = asset.soundfont_source()
            && !state
                .pool
                .has_sample_instrument_asset(&asset.canonical(state.document.project()))
        {
            missing.push(source);
        }
        missing.sort_by_key(|id| id.0);
        missing.dedup();
        Ok(missing)
    }
    pub fn sample_instrument_relink_source(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        sample: SampleId,
        expected_path: SamplePath,
        path: String,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current = state
                .document
                .project()
                .channel(channel)
                .ok_or("channel no longer exists")?;
            if current.sample_instrument != expected
                || !expected
                    .as_ref()
                    .is_some_and(|asset| asset.samples().any(|id| id == sample))
            {
                return Err("sample-bank mapping changed; retry relinking".into());
            }
            if state
                .document
                .project()
                .sample(sample)
                .is_none_or(|asset| asset.path != expected_path)
            {
                return Err("sample path changed; retry relinking".into());
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                state.pool.sample_bank_budget_handle(),
            )
        };
        let file = crate::paths::absolute(&path)?;
        let (audio, _workspace) = self.inner.cache.decode_sample_bank(&file, &budget)?;
        #[cfg(test)]
        self.pause("sample-relink:decoded");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("project changed while relinking".into());
            }
            self.sample_edit_ticket(
                &state,
                Command::RelinkSamplePath {
                    sample,
                    expected: expected_path,
                    path: SamplePath::External(crate::paths::display(&file)),
                },
                None,
                vec![(sample, audio)],
            )?
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
        jobs.next = jobs
            .next
            .checked_add(1)
            .ok_or("speech request sequence exhausted")?;
        let request = jobs.next;
        if let Some((_, cancelled)) = jobs
            .jobs
            .insert(channel, (request, Arc::new(AtomicBool::new(false))))
        {
            cancelled.store(true, Ordering::Release);
        }
        Ok(request)
    }
    pub fn sample_instrument_speech_cancel(&self, channel: ChannelId, request: u64) {
        let _state = self.state();
        if let Some((id, cancelled)) = super::lock(&self.inner.speech_jobs).jobs.get(&channel)
            && *id == request
        {
            cancelled.store(true, Ordering::Release);
        }
    }
    pub fn sample_instrument_speech(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        text: String,
        pitch: f32,
        duration: f32,
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
        let cancelled = super::lock(&self.inner.speech_jobs)
            .jobs
            .get(&channel)
            .filter(|(id, _)| *id == request)
            .map(|(_, flag)| flag.clone())
            .ok_or("speech request is no longer active")?;
        let lease = SpeechLease {
            session: self.clone(),
            channel,
            request,
            cancelled,
        };
        let keep_going = || !lease.cancelled.load(Ordering::Acquire);
        if !keep_going() {
            return Err("Speech rendering cancelled".into());
        }
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
            (
                state.generation,
                state.edits,
                state.replacements,
                state.pool.sample_bank_budget_handle(),
            )
        };
        // Bound both render/decode workspaces before allocating source PCM.
        if !duration.is_finite() || !(0.1..=60.0).contains(&duration) {
            return Err("invalid speech duration".into());
        }
        let _workspace = budget.reserve((duration * 24000.0).round() as usize * 4 * 2)?;
        let audio = sources::render_text(&text, pitch, duration, &mut || keep_going())
            .map_err(|e| e.to_string())?;
        if !keep_going() {
            return Err("Speech rendering cancelled".into());
        }
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
        // The rendered source is no longer needed after writing; dropping it
        // here releases its allocation before a separately admitted decode.
        drop(audio);
        let (decoded, _decode_workspace) = self.inner.cache.decode_sample_bank(&file.0, &budget)?;
        #[cfg(test)]
        self.pause("sample-speech:rendered");
        if !keep_going() {
            return Err("Speech rendering cancelled".into());
        }
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
