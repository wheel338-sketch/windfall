//! Consolidating playlist tracks: each chosen lane's pattern and audio clips
//! are rendered to one audio clip on that lane and muted, as one undo step.
//!
//! This is the track-scoped sibling of [`Session::bounce_selected_clips`].
//! It needs no selection: the lane decides what is rendered. A lane keeps
//! its own print, so several lanes consolidated together stay apart.

use std::collections::HashSet;
use std::path::PathBuf;

use windfall_project::{
    ClipAudioOutput, ClipContent, ClipId, ClipPatch, ClipUpdate, Command, DispatchResult,
    PlaylistTrackId, Project, SampleId, SamplePath, TickRange, TrackId,
};

use super::library::audio_clip_commands;
use super::{ClipPlace, Session};

const STALE: &str =
    "The project or its audio changed while consolidating. Choose the tracks and try again.";

/// What consolidating one lane renders and mutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Recipe {
    pub track: PlaylistTrackId,
    /// The print's name: the lane's, marked as a print.
    pub name: String,
    /// The lane's unmuted pattern and audio clips that the active layout
    /// shows, in playlist order. These are rendered, then muted.
    pub sources: Vec<ClipId>,
    /// From the first source's start to the last source's end.
    pub range: TickRange,
}

/// The recipes for `tracks`, in playlist order whatever order was asked.
/// Either every lane can be consolidated or none is.
pub(super) fn consolidate_plan(
    project: &Project,
    tracks: &[PlaylistTrackId],
) -> Result<Vec<Recipe>, String> {
    if tracks.is_empty() {
        return Err("Choose at least one playlist track to consolidate.".into());
    }
    let playlist = &project.playlist;
    let book = &playlist.arrangement_book;
    for id in tracks {
        if !playlist.tracks.iter().any(|track| track.id == *id) {
            return Err("A chosen playlist track no longer exists.".into());
        }
    }
    let wanted: HashSet<_> = tracks.iter().copied().collect();
    let mut recipes = Vec::new();
    for track in playlist
        .tracks
        .iter()
        .filter(|track| wanted.contains(&track.id))
    {
        let sources: Vec<_> = playlist
            .clips
            .iter()
            .filter(|clip| {
                clip.track == track.id
                    && !clip.muted
                    && clip.length > 0
                    && !matches!(clip.content, ClipContent::Automation { .. })
                    && book.shows(clip)
            })
            .collect();
        if sources.is_empty() {
            return Err(format!(
                "\"{}\" has no unmuted pattern or audio clip to consolidate.",
                track.name
            ));
        }
        let range = TickRange {
            start: sources.iter().map(|clip| clip.start).min().unwrap(),
            end: sources
                .iter()
                .map(|clip| clip.start.saturating_add(clip.length))
                .max()
                .unwrap(),
        };
        range.check()?;
        recipes.push(Recipe {
            track: track.id,
            name: format!("{} (consolidated)", track.name),
            sources: sources.iter().map(|clip| clip.id).collect(),
            range,
        });
    }
    Ok(recipes)
}

/// The project a recipe is rendered from: the lane's sources alone among
/// the pattern and audio clips, on a lane that is audible. Automation clips
/// keep playing wherever they sit, so the print carries the fader rides and
/// tempo changes the lane is heard with. Solo is cleared rather than used:
/// the live flags are never the render switch.
pub(super) fn consolidate_source(project: &Project, recipe: &Recipe) -> Project {
    let chosen: HashSet<_> = recipe.sources.iter().copied().collect();
    let mut source = project.clone();
    for clip in &mut source.playlist.clips {
        if !chosen.contains(&clip.id) && !matches!(clip.content, ClipContent::Automation { .. }) {
            clip.muted = true;
        }
    }
    for track in &mut source.playlist.tracks {
        track.solo = false;
        if track.id == recipe.track {
            track.muted = false;
        }
    }
    source
}

impl Session {
    /// Called on a worker. Renders each of `tracks` to one audio clip on
    /// that lane, spanning the lane's sources, and mutes those sources: one
    /// batch, one undo step, however many lanes. Undo unmutes the sources
    /// and removes the prints; the rendered files stay for redo and for a
    /// saved project.
    ///
    /// The print plays straight to the output, because the render already
    /// passed the whole mixer. Only the lane's unmuted pattern and audio
    /// clips in the active arrangement are rendered and muted; its
    /// automation clips, and every other lane, are left as they are. A lane
    /// with nothing to render, a missing lane, a render that fails and an
    /// edit made while rendering all leave the project unchanged and remove
    /// the files that were written.
    pub fn consolidate_playlist_tracks(
        &self,
        tracks: Vec<PlaylistTrackId>,
    ) -> Result<DispatchResult, String> {
        let sample_rate = self.inner.audio.status().sample_rate.max(1);
        let (project, pool, recipes, generation, edits, replacements, loading, plugin_revision) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let recipes = consolidate_plan(project, &tracks)?;
            if state.pool.needs_sampler_preparation(project) {
                return Err("Wait for sampler preparation before consolidating.".into());
            }
            let mut pool = state.pool.clone();
            pool.share_sampler_budget(&state.pool);
            (
                project.clone(),
                pool,
                recipes,
                state.generation,
                state.edits,
                state.replacements,
                state.loading.clone(),
                self.plugin_revision(),
            )
        };
        // Native state is captured once; every lane renders from that capture.
        let project = {
            let _recording = self.recording_idle()?;
            self.capture_plugins_idle(project, plugin_revision)?
        };
        let render_pool = pool.prepare_plugin_render();
        let mut prints = Vec::with_capacity(recipes.len());
        for recipe in &recipes {
            let source = consolidate_source(&project, recipe);
            prints.push(self.render_print(&source, &render_pool, recipe.range, sample_rate)?);
        }
        #[cfg(test)]
        self.pause("consolidate:rendered");
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
            // A batch cannot pass an id on, so the ids are worked out here
            // in the order the commands allocate them: a sample, then its clip.
            let mut next_id = project.next_id;
            let mut commands = Vec::new();
            let mut sources = Vec::with_capacity(recipes.len());
            for (recipe, (file, audio)) in recipes.iter().zip(&prints) {
                let sample = SampleId(next_id);
                commands.push(Command::AddSample {
                    name: recipe.name.clone(),
                    path: SamplePath::External(crate::paths::display(&file.0)),
                });
                let mut clip_commands = audio_clip_commands(
                    project,
                    sample,
                    audio,
                    &recipe.name,
                    ClipPlace {
                        track: Some(recipe.track),
                        start: recipe.range.start,
                        mixer_track: Some(TrackId::MASTER),
                    },
                    next_id.saturating_add(1),
                );
                for command in &mut clip_commands {
                    if let Command::AddClips { clips } = command {
                        for clip in clips {
                            // Tempo automation can make the buffer's
                            // duration differ from the span in ticks.
                            clip.length = Some(recipe.range.end - recipe.range.start);
                            clip.muted = Some(false);
                            if let ClipContent::Audio { output, .. } = &mut clip.content {
                                *output = ClipAudioOutput::Direct;
                            }
                        }
                    }
                }
                commands.extend(clip_commands);
                next_id = next_id.saturating_add(2);
                sources.push((sample, audio.clone()));
            }
            let muted: HashSet<_> = recipes
                .iter()
                .flat_map(|recipe| recipe.sources.iter().copied())
                .collect();
            commands.push(Command::UpdateClips {
                updates: project
                    .playlist
                    .clips
                    .iter()
                    .filter(|clip| muted.contains(&clip.id))
                    .map(|clip| ClipUpdate {
                        id: clip.id,
                        patch: ClipPatch {
                            muted: Some(true),
                            ..Default::default()
                        },
                    })
                    .collect(),
            });
            let label = if recipes.len() == 1 {
                "Consolidate track"
            } else {
                "Consolidate tracks"
            };
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some(label.into()),
                    commands,
                },
                None,
                sources,
            )?
        };
        let result = self.finish_sample_edit(ticket)?;
        // Keep the sources for undo/redo and save/reopen.
        for (file, _) in &mut prints {
            file.0 = PathBuf::new();
        }
        Ok(result)
    }
}
