//! Sample pre-processing: renders a sampler channel's sample through offline
//! treatments on a worker and attaches the result as a new sample.
//!
//! It follows the audio editor's asset contract. The source file is never
//! written. The result is a new Float32 WAV in the app's recordings folder,
//! added to the project as an ordinary external sample and given to the
//! channel in one undoable batch. DSP and file work hold neither the document
//! nor the recording lock; the batch goes through the sample edit ticket, so
//! a spectral bank is prepared off the locks too and a stale result changes
//! nothing and deletes its file.
use std::path::PathBuf;

use windfall_core::AudioBuffer;
use windfall_engine::sample_preprocess::{LoopSeam, SamplePreprocess, preprocess};
use windfall_ipc::{SampleInfo, SamplerPreprocessRequest};
use windfall_project::{
    ChannelSource, Command, DispatchResult, SampleId, SamplePath, SamplerLoopMode, SamplerPatch,
};

use super::Session;

const STALE: &str =
    "The project or the sample changed while pre-processing. Nothing was changed; try again.";

/// Owns the rendered file until the sample and the channel edit are both
/// installed. A refused or failed job deletes it.
struct ProcessedFile(PathBuf);
impl Drop for ProcessedFile {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// The same allocation, not merely equal audio: a reload makes a new one.
fn same_audio(a: &AudioBuffer, b: &AudioBuffer) -> bool {
    a.samples().as_ptr() == b.samples().as_ptr()
        && a.samples().len() == b.samples().len()
        && a.sample_rate() == b.sample_rate()
        && a.channels() == b.channels()
}

fn amount(value: Option<f32>, what: &str) -> Result<f32, String> {
    let value = value.unwrap_or(0.0);
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("{what} must be zero or more."))
    }
}

impl Session {
    /// Audition and Apply deliberately share rendering and stale validation.
    pub fn sampler_preprocess_preview(
        &self,
        request: SamplerPreprocessRequest,
    ) -> Result<SampleInfo, String> {
        match self.sampler_preprocess_job(request, true)? {
            PreprocessReply::Preview(info) => Ok(info),
            PreprocessReply::Applied(_) => unreachable!(),
        }
    }
    /// Called on a worker: snapshot, render, write, then commit one batch.
    pub fn sampler_preprocess(
        &self,
        request: SamplerPreprocessRequest,
    ) -> Result<DispatchResult, String> {
        match self.sampler_preprocess_job(request, false)? {
            PreprocessReply::Applied(result) => Ok(result),
            PreprocessReply::Preview(_) => unreachable!(),
        }
    }
    fn sampler_preprocess_job(
        &self,
        request: SamplerPreprocessRequest,
        preview: bool,
    ) -> Result<PreprocessReply, String> {
        let fade_in_ms = amount(request.fade_in_ms, "The fade-in")?;
        let fade_out_ms = amount(request.fade_out_ms, "The fade-out")?;
        let crossfade = amount(request.loop_crossfade, "The loop crossfade")?;

        let (settings, sample, name, source, generation, edits, replacements, loading) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let channel = project
                .channel(request.channel)
                .ok_or("The channel no longer exists.")?;
            let ChannelSource::Sampler(settings) = &channel.source else {
                return Err("Only a sampler channel has a sample to pre-process.".into());
            };
            let sample = settings
                .sample
                .ok_or("Load a sample into the channel first.")?;
            let name = project
                .sample(sample)
                .ok_or("The channel's sample is missing from the project.")?
                .name
                .clone();
            let source = state
                .pool
                .get(sample)
                .cloned()
                .ok_or("The channel's sample is not loaded. Reload samples first.")?;
            (
                settings.clone(),
                sample,
                name,
                source,
                state.generation,
                state.edits,
                state.replacements,
                state.loading.clone(),
            )
        };

        let looping = settings.loop_mode != SamplerLoopMode::Off;
        if crossfade > 0.0 && settings.loop_mode != SamplerLoopMode::Forward {
            return Err(if looping {
                "A ping-pong loop has no seam to crossfade. Use a forward loop.".into()
            } else {
                "Turn the forward loop on before crossfading its seam.".into()
            });
        }
        let options = SamplePreprocess {
            start: settings.start,
            end: settings.end,
            trim: request.trim.unwrap_or(false),
            reversed: settings.reverse,
            reverse: request.reverse.unwrap_or(false),
            remove_dc: request.remove_dc.unwrap_or(false),
            invert_polarity: request.invert_polarity.unwrap_or(false),
            swap_stereo: request.swap_stereo.unwrap_or(false),
            fade_in_ms,
            fade_out_ms,
            // The loop is always handed over, so its points follow the audio
            // through a trim or a reversal even when no seam is blended.
            loop_crossfade: looping.then_some(LoopSeam {
                start: settings.loop_start,
                end: settings.loop_end,
                amount: crossfade,
            }),
            normalize: request.normalize.unwrap_or(false),
        };
        if !options.changes_audio() {
            return Err("Choose at least one treatment to apply.".into());
        }
        let processed = preprocess(&source, &options)?;

        if preview {
            let info = SampleInfo {
                source_warnings: Vec::new(),
                path: String::new(),
                name: format!("{name} (prepared preview)"),
                sample_rate: processed.audio.sample_rate(),
                channels: processed.audio.channels(),
                frames: processed.audio.frames() as u64,
                duration_secs: processed.audio.duration_secs(),
                peaks: windfall_engine::audio_edit::overview(&processed.audio, 512),
            };
            #[cfg(test)]
            self.pause("sampler-preprocess:preview");
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current =
                state.document.project().channel(request.channel).and_then(
                    |channel| match &channel.source {
                        ChannelSource::Sampler(now) => Some(now),
                        _ => None,
                    },
                );
            if state.generation != generation
                || state.edits != edits
                || state.replacements != replacements
                || state.loading != loading
                || current != Some(&settings)
                || !state
                    .pool
                    .get(sample)
                    .is_some_and(|now| same_audio(now, &source))
            {
                return Err(STALE.into());
            }
            drop(state);
            self.controller().preview(processed.audio);
            return Ok(PreprocessReply::Preview(info));
        }

        // Persistent app-owned storage works before the project has a save
        // path. Completed files stay for undo, redo and saved documents.
        let folder = self.store().recordings_dir().join("Sample edits");
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create the sample edit folder: {error}"))?;
        let path = tempfile::Builder::new()
            .prefix("Sample-")
            .suffix(".wav")
            .tempfile_in(&folder)
            .map_err(|error| format!("Could not create the sample edit file: {error}"))?
            .into_temp_path()
            .keep()
            .map_err(|error| format!("Could not keep the sample edit file: {error}"))?;
        let mut file = ProcessedFile(path);
        windfall_codec::write_wav(
            &file.0,
            &processed.audio,
            windfall_codec::WavSampleFormat::Float32,
        )
        .map_err(|error| error.to_string())?;
        // Decode through the ordinary cache off the locks, so history can
        // restore the sample without decoding under the document lock.
        // Float32 WAV keeps the processed audio exactly.
        let decoded = self.inner.cache.decode(&file.0)?;
        #[cfg(test)]
        self.pause("sampler-preprocess:rendered");

        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let current =
                state.document.project().channel(request.channel).and_then(
                    |channel| match &channel.source {
                        ChannelSource::Sampler(now) => Some(now),
                        ChannelSource::Instrument { .. } => None,
                    },
                );
            if state.generation != generation
                || state.edits != edits
                || state.replacements != replacements
                || state.loading != loading
                || current != Some(&settings)
                || !state
                    .pool
                    .get(sample)
                    .is_some_and(|now| same_audio(now, &source))
            {
                return Err(STALE.into());
            }
            let new_sample = SampleId(state.document.project().next_id);
            let (loop_start, loop_end) = match processed.loop_points {
                Some((start, end)) => (Some(start), Some(end)),
                None => (None, None),
            };
            let command = Command::Batch {
                label: Some("Pre-process sample".into()),
                commands: vec![
                    Command::AddSample {
                        name: format!("{name} (processed)"),
                        path: SamplePath::External(crate::paths::display(&file.0)),
                    },
                    Command::SetChannelSample {
                        id: request.channel,
                        sample: Some(new_sample),
                    },
                    Command::UpdateSampler {
                        id: request.channel,
                        patch: SamplerPatch {
                            start: Some(processed.start),
                            end: Some(processed.end),
                            loop_start,
                            loop_end,
                            // A blended seam replaces the playback crossfade.
                            loop_crossfade: (processed.crossfade_frames > 0).then_some(0.0),
                            ..Default::default()
                        },
                    },
                ],
            };
            self.sample_edit_ticket(&state, command, None, vec![(new_sample, decoded)])?
        };
        let result = self.finish_sample_edit(ticket)?;
        self.own_derived(&file.0);
        file.0 = PathBuf::new(); // The document and its history now own the file.
        Ok(PreprocessReply::Applied(result))
    }
}

enum PreprocessReply {
    Applied(DispatchResult),
    Preview(SampleInfo),
}
