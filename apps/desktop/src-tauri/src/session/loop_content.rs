use super::{ClipPlace, Session, library::audio_clip_commands};
use crate::{paths, sync::lock};
use sha2::{Digest, Sha256};
use std::io::Write;
use windfall_factory::{Rendered, loops};
use windfall_ipc::{LoopCatalog, LoopEntry};
use windfall_project::{ClipContent, Command, DispatchResult, SampleId, SamplePath};

impl Session {
    pub fn loop_catalog(&self) -> LoopCatalog {
        let state = self.state();
        LoopCatalog {
            generation: state.generation,
            revision: state.document.revision(),
            entries: loops::manifest()
                .iter()
                .map(|l| LoopEntry {
                    id: l.id.into(),
                    name: l.name.into(),
                    genre: l.genre.into(),
                    tempo: l.tempo,
                    key: l.key,
                    mode: "minor".into(),
                    bars: l.bars,
                    provenance: loops::PROVENANCE.into(),
                })
                .collect(),
        }
    }
    pub fn loop_preview(&self, id: &str) -> Result<(), String> {
        let asset = loops::manifest()
            .iter()
            .find(|l| l.id == id)
            .ok_or("Unknown factory loop.")?;
        let rendered = asset.render();
        let path = self.loop_file(&rendered, asset, f64::from(asset.tempo), asset.key)?;
        self.preview_play(&paths::display(&path))
    }
    /// Synthesize at the project's current tempo and selected pitch class,
    /// then atomically install sources + tracks + clip as a single undo step.
    pub fn loop_insert(
        &self,
        id: &str,
        key: Option<u8>,
        place: ClipPlace,
        generation: u64,
        revision: u64,
    ) -> Result<DispatchResult, String> {
        let asset = loops::manifest()
            .iter()
            .find(|l| l.id == id)
            .ok_or("Unknown factory loop.")?;
        let tempo = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed. Refresh the loop browser.".into());
            }
            state.document.project().settings.tempo_bpm
        };
        let rendered = asset
            .render_fit(tempo, key.unwrap_or(asset.key))
            .map_err(str::to_owned)?;
        let file = self.loop_file(&rendered, asset, tempo, key.unwrap_or(asset.key))?;
        let buffer = self.inner.cache.decode(&file)?;
        let path = SamplePath::External(paths::display(&file));
        #[cfg(test)]
        self.pause("loop:rendered");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed while the loop was rendered.".into());
            }
            let project = state.document.project();
            let held = project.samples.iter().find(|s| s.path == path);
            let sample = held.map(|s| s.id).unwrap_or(SampleId(project.next_id));
            let next = if held.is_some() {
                project.next_id
            } else {
                project.next_id.saturating_add(1)
            };
            let mut commands = Vec::new();
            if held.is_none() {
                commands.push(Command::AddSample {
                    name: format!(
                        "{} ({} BPM, key {} minor)",
                        asset.name,
                        tempo,
                        key.unwrap_or(asset.key)
                    ),
                    path,
                });
            }
            let mut clips = audio_clip_commands(project, sample, &buffer, asset.name, place, next);
            // Exact musical span rather than ceil(duration) rounding; source is
            // within half a sample of this duration at the chosen tempo.
            for command in &mut clips {
                if let Command::AddClips { clips } = command {
                    for clip in clips {
                        if matches!(clip.content, ClipContent::Audio { .. }) {
                            clip.length = Some(u32::from(asset.bars) * 4 * windfall_core::PPQ);
                        }
                    }
                }
            }
            commands.extend(clips);
            let source = state.pool.get(sample).unwrap_or(&buffer).clone();
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some("Insert factory loop".into()),
                    commands,
                },
                None,
                vec![(sample, source)],
            )?
        };
        self.finish_sample_edit(ticket)
    }
    fn loop_file(
        &self,
        rendered: &Rendered,
        asset: &loops::StarterLoop,
        tempo: f64,
        key: u8,
    ) -> Result<std::path::PathBuf, String> {
        let folder = lock(&self.inner.settings)
            .recordings_dir()
            .join("FactoryLoops");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let bytes = rendered.to_wav();
        let checksum = format!("{:x}", Sha256::digest(&bytes));
        let file = folder.join(format!(
            "{}-{:x}-{key}-{checksum}.wav",
            asset.id,
            tempo.to_bits()
        ));
        let mut staged = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        staged.write_all(&bytes).map_err(|e| e.to_string())?;
        staged.as_file().sync_all().map_err(|e| e.to_string())?;
        match staged.persist_noclobber(&file) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Never adopt modified bytes under a generated content address.
                let mut source = std::fs::File::open(&file).map_err(|e| e.to_string())?;
                if source.metadata().map_err(|e| e.to_string())?.len() != bytes.len() as u64 {
                    return Err("Generated loop cache was modified.".into());
                }
                let mut actual = Vec::with_capacity(bytes.len());
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(&mut source, bytes.len() as u64 + 1),
                    &mut actual,
                )
                .map_err(|e| e.to_string())?;
                if actual != bytes {
                    return Err("Generated loop cache checksum mismatch.".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        let provenance = serde_json::to_vec_pretty(&serde_json::json!({"schema":1,"generator":"loops-v1","sourceId":asset.id,"sourceTempo":asset.tempo,"sourceKey":asset.key,"tempo":tempo,"key":key,"mode":"minor","bars":asset.bars,"sha256":checksum,"provenance":loops::PROVENANCE})).map_err(|e|e.to_string())?;
        let sidecar = file.with_extension("wav.json");
        let mut metadata = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        metadata.write_all(&provenance).map_err(|e| e.to_string())?;
        metadata.as_file().sync_all().map_err(|e| e.to_string())?;
        match metadata.persist_noclobber(&sidecar) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let source = std::fs::File::open(&sidecar).map_err(|e| e.to_string())?;
                if source.metadata().map_err(|e| e.to_string())?.len() != provenance.len() as u64 {
                    return Err("Generated loop provenance was modified.".into());
                }
                let mut actual = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(source, provenance.len() as u64 + 1),
                    &mut actual,
                )
                .map_err(|e| e.to_string())?;
                if actual != provenance {
                    return Err("Generated loop provenance was modified.".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        // Cache lifetime is deliberate: undo/redo, save and archives may refer
        // to it. No request deletes a source retained by project history.
        Ok(file)
    }
}
