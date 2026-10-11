//! Detect off-lock; compile and publish against the same document/source.
use super::Session;
use windfall_project::{ChannelId, Command, DispatchResult, slicer::SliceOptions};

impl Session {
    pub fn sample_playback_detect(
        &self,
        channel: ChannelId,
        options: SliceOptions,
        generation: u64,
        revision: u64,
    ) -> Result<DispatchResult, String> {
        let (mut document, pool, preparation, settings, audio, sample, edits) = {
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
            (
                state.document.clone(),
                state.pool.clone(),
                self.project_preparation(&state),
                settings,
                audio,
                sample,
                state.edits,
            )
        };
        let detected = windfall_project::sample_playback::detect(
            &audio,
            sample,
            settings.source_bpm,
            options,
        )?;
        let mut asset = settings;
        asset.slices = detected.slices;
        asset.source_beats = detected.source_beats;
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
}
