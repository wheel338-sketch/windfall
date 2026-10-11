//! Playlist bounce uses the existing offline renderer and sample edit transaction.
use std::collections::HashSet;
use std::path::PathBuf;

use windfall_core::AudioBuffer;
use windfall_engine::{RenderOptions, render_streaming_checked};
use windfall_ipc::PlayMode;
use windfall_project::{
    ClipAudioOutput, ClipContent, ClipId, ClipPatch, ClipUpdate, Command, DispatchResult,
    PlaylistTrackId, Project, SampleId, SamplePath, TickRange, TrackId,
};

use super::library::audio_clip_commands;
use super::{ClipPlace, Session};

const STALE: &str =
    "The project or its audio changed while bouncing. Select the clips and try again.";

/// Own a persistent render until its sample and clip have both been installed.
pub(super) struct RenderFile(pub(super) PathBuf);
impl Drop for RenderFile {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// The track order determines the destination; clip starts determine the span.
pub(super) fn bounce_source(
    project: &Project,
    selection: &[ClipId],
) -> Result<(Project, TickRange, PlaylistTrackId), String> {
    if selection.is_empty() {
        return Err("Select at least one playlist clip to bounce.".into());
    }
    let ids: HashSet<_> = selection.iter().copied().collect();
    let clips: Vec<_> = project
        .playlist
        .clips
        .iter()
        .filter(|clip| ids.contains(&clip.id))
        .collect();
    if clips.len() != ids.len() {
        return Err("A selected clip no longer exists.".into());
    }
    let tracks: HashSet<_> = clips.iter().map(|clip| clip.track).collect();
    let destination = project
        .playlist
        .tracks
        .iter()
        .find(|track| tracks.contains(&track.id))
        .ok_or("A selected playlist track no longer exists.")?
        .id;
    let range = TickRange {
        start: clips.iter().map(|clip| clip.start).min().unwrap(),
        end: clips
            .iter()
            .map(|clip| clip.start.saturating_add(clip.length))
            .max()
            .unwrap(),
    };
    range.check()?;
    let mut source = project.clone();
    // Preserve metadata references while silencing every unselected clip,
    // including overlapping clips on a selected playlist track.
    for clip in &mut source.playlist.clips {
        if !ids.contains(&clip.id) {
            clip.muted = true;
        }
    }
    for track in &mut source.playlist.tracks {
        track.solo = tracks.contains(&track.id);
        if track.solo {
            track.muted = false;
        }
    }
    Ok((source, range, destination))
}

impl Session {
    /// Called on a worker: snapshot, render, persist, prepare, then commit one batch.
    pub fn bounce_selected_clips(&self, selection: Vec<ClipId>) -> Result<DispatchResult, String> {
        let sample_rate = self.inner.audio.status().sample_rate.max(1);
        let (
            project,
            pool,
            range,
            destination,
            generation,
            edits,
            replacements,
            loading,
            plugin_revision,
        ) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let (project, range, destination) =
                bounce_source(state.document.project(), &selection)?;
            if state.pool.needs_sampler_preparation(&project) {
                return Err("Wait for sampler preparation before bouncing.".into());
            }
            let mut pool = state.pool.clone();
            pool.share_sampler_budget(&state.pool);
            (
                project,
                pool,
                range,
                destination,
                state.generation,
                state.edits,
                state.replacements,
                state.loading.clone(),
                self.plugin_revision(),
            )
        };
        let project = {
            let _recording = self.recording_idle()?;
            self.capture_plugins_idle(project, plugin_revision)?
        };
        let render_pool = pool.prepare_plugin_render();
        let (mut file, audio) = self.render_print(&project, &render_pool, range, sample_rate)?;
        #[cfg(test)]
        self.pause("bounce:rendered");
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation
                || state.edits != edits
                || state.replacements != replacements
                || state.loading != loading
                || !state.pool.same_sources(&pool)
                || self.plugin_revision() != plugin_revision
            {
                return Err(STALE.into());
            }
            let project = state.document.project();
            let sample = SampleId(project.next_id);
            let mut commands = vec![Command::AddSample {
                name: "Bounce".into(),
                path: SamplePath::External(crate::paths::display(&file.0)),
            }];
            let mut clip_commands = audio_clip_commands(
                project,
                sample,
                &audio,
                "Bounce",
                ClipPlace {
                    track: Some(destination),
                    start: range.start,
                    mixer_track: Some(TrackId::MASTER),
                },
                project.next_id.saturating_add(1),
            );
            // Tempo automation can make buffer duration differ from stored-tempo ticks.
            for command in &mut clip_commands {
                if let Command::AddClips { clips } = command {
                    for clip in clips {
                        clip.length = Some(range.end - range.start);
                        clip.muted = Some(false);
                        // The renderer already printed the complete mixer chain.
                        if let ClipContent::Audio { output, .. } = &mut clip.content {
                            *output = ClipAudioOutput::Direct;
                        }
                    }
                }
            }
            commands.extend(clip_commands);
            commands.push(Command::UpdateClips {
                updates: project
                    .playlist
                    .clips
                    .iter()
                    .filter(|clip| selection.contains(&clip.id))
                    .map(|clip| ClipUpdate {
                        id: clip.id,
                        patch: ClipPatch {
                            muted: Some(true),
                            ..Default::default()
                        },
                    })
                    .collect(),
            });
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some("Bounce selected clips".into()),
                    commands,
                },
                None,
                vec![(sample, audio)],
            )?
        };
        let result = self.finish_sample_edit(ticket)?;
        file.0 = PathBuf::new(); // Keep the source for undo/redo and save/reopen.
        Ok(result)
    }

    /// Renders `range` of a prepared project copy with no tail and keeps it
    /// as a Float32 WAV among the recordings. The file is removed again if
    /// the returned guard is dropped before its path is cleared.
    pub(super) fn render_print(
        &self,
        project: &Project,
        render_pool: &windfall_engine::SamplePool,
        range: TickRange,
        sample_rate: u32,
    ) -> Result<(RenderFile, AudioBuffer), String> {
        let options = RenderOptions {
            region: Some(range),
            mode: PlayMode::Song,
            sample_rate,
            tail_secs: 0.0,
            auto_tail: false,
            ..Default::default()
        };
        let mut samples = Vec::new();
        let rendered = render_streaming_checked(
            project,
            render_pool,
            &options,
            &mut |block| {
                samples.extend_from_slice(block);
                true
            },
            &mut |_| true,
        )
        .map_err(|error| error.to_string())?;
        if !rendered.completed || rendered.dropped_clips != 0 || rendered.frames == 0 {
            return Err("The bounce did not render all of the selected span.".into());
        }
        let audio = AudioBuffer::from_interleaved(sample_rate, 2, samples);
        let folder = self.store().recordings_dir().join("Bounces");
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let path = tempfile::Builder::new()
            .prefix("Bounce-")
            .suffix(".wav")
            .tempfile_in(&folder)
            .map_err(|error| error.to_string())?
            .into_temp_path()
            .keep()
            .map_err(|error| error.to_string())?;
        let file = RenderFile(path);
        windfall_codec::write_wav(&file.0, &audio, windfall_codec::WavSampleFormat::Float32)
            .map_err(|error| error.to_string())?;
        let audio = self.inner.cache.decode(&file.0)?;
        Ok((file, audio))
    }
}
