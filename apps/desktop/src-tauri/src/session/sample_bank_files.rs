//! Portable own sample-bank manifests. JSON stores relative audio references;
//! PCM is decoded/copied only by IPC workers, never embedded in parameters.
use super::Session;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use windfall_dsp::sample_bank::{MAX_BANK_BYTES, MAX_BANK_ZONES, SampleZone};
use windfall_project::{
    ChannelId, ChannelSource, Command, DispatchResult, Document, InstrumentKind, SampleId,
    SampleInstrumentAsset, SamplePath,
};

const SCHEMA: &str = "windfall.sample-bank";
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BankSource {
    id: u32,
    name: String,
    path: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BankFile {
    schema: String,
    version: u32,
    sources: Vec<BankSource>,
    zones: Vec<SampleZone>,
}
impl BankFile {
    fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA || self.version != 1 {
            return Err("unsupported Windfall sample-bank format".into());
        }
        if self.zones.len() > MAX_BANK_ZONES || self.sources.len() > MAX_BANK_ZONES {
            return Err("sample bank exceeds 256 zones/sources".into());
        }
        let mut ids = HashSet::new();
        for source in &self.sources {
            if !ids.insert(source.id)
                || source.name.len() > 256
                || source.name.chars().any(char::is_control)
            {
                return Err("duplicate source ID or invalid source name".into());
            }
            if let Some(reason) = SamplePath::Project(source.path.clone()).problem() {
                return Err(reason.into());
            }
        }
        for zone in &self.zones {
            zone.validate().map_err(str::to_owned)?;
            if !ids.contains(&zone.sample) {
                return Err("mapping names a source outside the bank manifest".into());
            }
        }
        Ok(())
    }
}
struct ExportSources(Vec<PathBuf>);
impl Drop for ExportSources {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}
impl Session {
    pub fn sample_instrument_load_bank(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        path: String,
    ) -> Result<DispatchResult, String> {
        let (generation, edits, replacements, project, budget) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current = state
                .document
                .project()
                .channel(channel)
                .ok_or("channel no longer exists")?;
            if current.sample_instrument != expected {
                return Err("sample-bank instrument changed".into());
            }
            if !matches!(current.source, ChannelSource::Instrument { params, .. } if matches!(params.kind(),
                InstrumentKind::MultiSampler | InstrumentKind::SamplePlayer | InstrumentKind::PadBank))
            {
                return Err("choose a zone-bank instrument".into());
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                state.document.project().clone(),
                state.pool.sample_bank_budget_handle(),
            )
        };
        let file = crate::paths::absolute(&path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(&file)
            .map_err(|e| e.to_string())?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err("sample-bank manifest exceeds 4 MiB".into());
        }
        let mut bank: BankFile = serde_json::from_slice(&bytes).map_err(|e| {
            windfall_ipc::ui_message(
                "sampleBanks.manifestInvalid",
                serde_json::json!({"error": e.to_string()}),
            )
        })?;
        bank.validate()?;
        let folder = file.parent().ok_or("bank source folder is missing")?;
        let mut draft = Document::new(project);
        let mut remap = HashMap::new();
        let mut commands = Vec::new();
        let mut audio = Vec::new();
        let mut total = 0usize;
        let mut workspaces = Vec::new();
        for source in &bank.sources {
            let source_file = folder.join(&source.path);
            let (decoded, workspace) =
                self.inner.cache.decode_sample_bank(&source_file, &budget)?;
            workspaces.push(workspace);
            total = total
                .checked_add(decoded.samples().len().saturating_mul(4))
                .ok_or("bank byte overflow")?;
            if total > MAX_BANK_BYTES {
                return Err("sample-bank sources exceed 256 MiB".into());
            }
            let command = Command::AddSample {
                name: source.name.clone(),
                path: SamplePath::External(crate::paths::display(&source_file)),
            };
            let id = SampleId(
                draft
                    .dispatch(command.clone(), None)
                    .map_err(|e| e.to_string())?
                    .created[0],
            );
            remap.insert(source.id, id.0);
            audio.push((id, decoded));
            commands.push(command);
        }
        for zone in &mut bank.zones {
            zone.sample = remap[&zone.sample];
        }
        commands.push(Command::SetSampleInstrumentAsset {
            channel,
            expected,
            asset: Some(SampleInstrumentAsset::Zones { zones: bank.zones }),
        });
        #[cfg(test)]
        self.pause("sample-bank:decoded");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if (state.generation, state.edits, state.replacements)
                != (generation, edits, replacements)
            {
                return Err("project changed while the bank was loaded".into());
            }
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some("Load sample bank".into()),
                    commands,
                },
                None,
                audio,
            )?
        };
        self.finish_sample_edit(ticket)
    }

    /// Save the selected zone bank and its decoded sources in Windfall's own
    /// format. Publication is atomic and stale work deletes newly written WAVs.
    pub fn sample_instrument_save_bank(
        &self,
        channel: ChannelId,
        expected: Option<SampleInstrumentAsset>,
        path: String,
    ) -> Result<(), String> {
        let (generation, edits, replacements, zones, sources) = {
            let state = self.state();
            let project = state.document.project();
            let current = project.channel(channel).ok_or("channel no longer exists")?;
            if current.sample_instrument != expected {
                return Err("sample bank changed; retry export".into());
            }
            let Some(SampleInstrumentAsset::Zones { zones }) = &current.sample_instrument else {
                return Err("this instrument has no zone bank to save".into());
            };
            let mut seen = HashSet::new();
            let mut sources = Vec::new();
            for zone in zones {
                if !seen.insert(zone.sample) {
                    continue;
                }
                let sample = project
                    .sample(SampleId(zone.sample))
                    .ok_or("source reference is missing")?;
                let audio = state
                    .pool
                    .get(sample.id)
                    .ok_or("source audio is not loaded")?
                    .clone();
                sources.push((sample.id.0, sample.name.clone(), audio));
            }
            (
                state.generation,
                state.edits,
                state.replacements,
                zones.clone(),
                sources,
            )
        };
        let file = crate::paths::absolute(&path)?;
        let parent = file.parent().ok_or("bank destination folder is missing")?;
        const SOURCE_FOLDER: &str = "Windfall bank sources";
        let folder = parent.join(SOURCE_FOLDER);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut written = ExportSources(Vec::new());
        let mut manifest = Vec::new();
        for (id, name, audio) in sources {
            let source = tempfile::Builder::new()
                .prefix("Zone-")
                .suffix(".wav")
                .tempfile_in(&folder)
                .map_err(|e| e.to_string())?
                .into_temp_path()
                .keep()
                .map_err(|e| e.to_string())?;
            written.0.push(source.clone());
            windfall_codec::write_wav(&source, &audio, windfall_codec::WavSampleFormat::Float32)
                .map_err(|e| e.to_string())?;
            let name_on_disk = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("bank source name is invalid")?;
            manifest.push(BankSource {
                id,
                name,
                path: format!("{SOURCE_FOLDER}/{name_on_disk}"),
            });
        }
        let bank = BankFile {
            schema: SCHEMA.into(),
            version: 1,
            sources: manifest,
            zones,
        };
        bank.validate()?;
        let bytes = serde_json::to_vec_pretty(&bank).map_err(|e| e.to_string())?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        temporary.write_all(&bytes).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        let state = self.state();
        if (state.generation, state.edits, state.replacements) != (generation, edits, replacements)
        {
            return Err("project changed while the sample bank was saved".into());
        }
        // Export is an immutable snapshot; release State before the file commit.
        // A later edit cannot change these source copies or mutate the project.
        drop(state);
        temporary.persist(&file).map_err(|e| e.to_string())?;
        written.0.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_rejects_wrong_version_traversal_duplicate_ids_and_missing_sources() {
        let mut bank = BankFile {
            schema: SCHEMA.into(),
            version: 1,
            sources: vec![BankSource {
                id: 3,
                name: "Own".into(),
                path: "own.wav".into(),
            }],
            zones: vec![SampleZone {
                sample: 3,
                ..Default::default()
            }],
        };
        bank.validate().unwrap();
        bank.version = 2;
        assert!(bank.validate().is_err());
        bank.version = 1;
        bank.sources[0].path = "../escape.wav".into();
        assert!(bank.validate().is_err());
        bank.sources[0].path = "own.wav".into();
        bank.zones[0].sample = 4;
        assert!(bank.validate().is_err());
        bank.zones[0].sample = 3;
        bank.sources.push(BankSource {
            id: 3,
            name: "Duplicate".into(),
            path: "other.wav".into(),
        });
        assert!(bank.validate().is_err());
    }
}
