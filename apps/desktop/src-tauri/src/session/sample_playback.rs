//! Detect off-lock; compile and publish against the same document/source.
use super::Session;
use windfall_project::{ChannelId, Command, DispatchResult, slicer::SliceOptions};

#[path = "sample_wav_metadata.rs"]
mod wav_metadata;

impl Session {
    pub fn sample_playback_detect(
        &self,
        channel: ChannelId,
        options: SliceOptions,
        generation: u64,
        revision: u64,
    ) -> Result<DispatchResult, String> {
        let (mut document, pool, preparation, settings, audio, sample, edits, file) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed before slice detection started.".into());
            }
            let settings = state
                .document
                .project()
                .channel(channel)
                .and_then(|c| c.sample_playback.clone())
                .ok_or("Load a sample instrument source first.")?;
            let sample = settings
                .sample
                .ok_or("Select a source sample for detection.")?;
            let audio = state
                .pool
                .get(sample)
                .cloned()
                .ok_or("The sample is not loaded. Reload samples and try again.")?;
            let file = if matches!(options, SliceOptions::WavMarkers) {
                let source = state
                    .document
                    .project()
                    .sample(sample)
                    .ok_or("The source sample no longer exists.")?;
                Some(super::samples::locate(
                    source,
                    state.project_dir(),
                    &self.inner.factory_dir,
                )?)
            } else {
                None
            };
            (
                state.document.clone(),
                state.pool.clone(),
                self.project_preparation(&state),
                settings,
                audio,
                sample,
                state.edits,
                file,
            )
        };
        let metadata = file
            .as_ref()
            .map(windfall_codec::read_wav_metadata_file)
            .transpose()
            .map_err(|e| e.to_string())?;
        let detected = if let Some(metadata) = &metadata {
            let current = self.inner.cache.decode(file.as_ref().unwrap())?;
            if !self.inner.cache.same_file_version(&audio, &current) {
                return Err("The source file changed. Reopen or reload its audio first.".into());
            }
            wav_metadata::read(metadata, &audio)?
        } else {
            windfall_project::sample_playback::detect(&audio, sample, settings.source_bpm, options)?
        };
        let mut asset = settings;
        asset.legacy_key_wrap = false;
        asset.slices = detected.slices;
        if metadata.is_some() {
            // Both cue-frame decks use the self-contained interchange source.
            asset.sample_b = asset.slices.iter().any(|s| s.deck == 1).then_some(sample);
        }
        asset.source_beats = detected.source_beats;
        asset.source_bpm = detected.source_bpm;
        let command = Command::SetSamplePlayback {
            channel,
            asset: Some(asset),
        };
        document
            .dispatch(command.clone(), None)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        self.pause("sample-playback:detected");
        let mut prepared = preparation
            .prepare(
                document.project(),
                &pool,
                windfall_engine::ProjectPublicationIntent::Edit,
            )
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        self.pause("sample-playback:prepared");
        if let Some(file) = &file {
            let current = self.inner.cache.decode(file)?;
            let now = windfall_codec::read_wav_metadata_file(file).map_err(|e| e.to_string())?;
            if !self.inner.cache.same_file_version(&audio, &current)
                || metadata.as_ref() != Some(&now)
            {
                return Err("The source file changed during WAV marker import. Try again.".into());
            }
        }
        let retirement;
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if state.generation != generation
                || state.edits != edits
                || state.document.revision() != revision
                || state
                    .pool
                    .get(sample)
                    .is_none_or(|a| a.samples().as_ptr() != audio.samples().as_ptr())
                || pool.iter().any(|(id, old)| {
                    state
                        .pool
                        .get(id)
                        .is_none_or(|a| a.samples().as_ptr() != old.samples().as_ptr())
                })
                || state.pool.iter().any(|(id, a)| {
                    pool.get(id)
                        .is_none_or(|old| a.samples().as_ptr() != old.samples().as_ptr())
                })
            {
                return Err(
                    "The project or source changed during slice detection. Try again.".into(),
                );
            }
            let pool = prepared.pool().clone();
            let lease = prepared.publication(self, &state)?;
            let applied = state
                .document
                .dispatch(command, None)
                .map_err(|e| e.to_string())?;
            self.commit_prepared_parameters(&state, &applied.touched);
            retirement = lease.install();
            DispatchResult {
                created: applied.created,
                patch: self.publish_prepared(&mut state, &applied.touched, &pool),
            }
        };
        let mut retirement = retirement;
        self.retire_project(&mut retirement);
        Ok(result)
    }

    /// Exports both sources, authored regions and slice attributes off-lock.
    pub fn sample_playback_export(
        &self,
        channel: ChannelId,
        path: &str,
        generation: u64,
        revision: u64,
    ) -> Result<(), String> {
        let path = crate::paths::absolute(path)?;
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        {
            return Err("Slice metadata export requires a .wav filename.".into());
        }
        let (asset, audio, audio_b) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed before slice export started.".into());
            }
            let asset = state
                .document
                .project()
                .channel(channel)
                .and_then(|c| c.sample_playback.clone())
                .ok_or("Load a sample instrument source first.")?;
            let audio = asset
                .sample
                .and_then(|id| state.pool.get(id))
                .cloned()
                .ok_or("Source A is not loaded. Reload samples first.")?;
            let audio_b = asset.sample_b.and_then(|id| state.pool.get(id)).cloned();
            (asset, audio, audio_b)
        };
        let (audio, metadata) = wav_metadata::sources(&asset, &audio, audio_b.as_ref())?;
        let mut writer = windfall_codec::WavWriter::create_with_metadata(
            &path,
            audio.sample_rate(),
            audio.channels(),
            windfall_codec::WavSampleFormat::Float32,
            &windfall_codec::AudioTags::default(),
            &metadata,
        )
        .map_err(|e| e.to_string())?;
        writer.write(audio.samples()).map_err(|e| e.to_string())?;
        writer.finalize().map_err(|e| e.to_string())
    }
}
