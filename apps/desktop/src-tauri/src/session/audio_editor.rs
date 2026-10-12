//! File/DSP work holds neither document nor audio/controller locks. The editor
//! lock only serializes these workers; it is never read by the audio callback.
use super::{Session, State};
mod classical_editor;
use crate::sync::lock;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use windfall_core::{AudioBuffer, samples_per_tick};
use windfall_engine::audio_edit;
use windfall_ipc::{AudioEditOperation, AudioEditPreview, AudioEditRequest};
use windfall_project::{
    AudioViewTiming, Clip, ClipContent, ClipId, ClipInit, Command, DispatchResult, MAX_SONG_TICKS,
    SampleId, SamplePath,
};

const STALE: &str = "The project or source changed. Reopen the audio editor and try again.";
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub(super) struct Editor {
    serial: u32,
    prepared: Option<Prepared>,
}
struct Prepared {
    token: u32,
    generation: u64,
    edits: u64,
    clip: Clip,
    source: AudioBuffer,
    view: AudioBuffer,
    clock: windfall_engine::SongClock,
    stored_tempo: f64,
    view_timing: AudioViewTiming,
    analysis_timing: AudioViewTiming,
    analysis_view: AudioBuffer,
    analysis_source: Option<(SampleId, AudioBuffer)>,
    recipe: Option<windfall_project::AudioEditRecipe>,
    rendered: Option<(windfall_project::AudioEditRecipe, AudioBuffer)>,
}
impl Prepared {
    fn valid(&self, state: &State) -> bool {
        let sample = self.clip.content.sample().expect("editor only holds audio");
        state.generation == self.generation
            && state.edits == self.edits
            && state
                .document
                .project()
                .playlist
                .clips
                .iter()
                .any(|clip| clip == &self.clip)
            && state
                .pool
                .get(sample)
                .is_some_and(|now| same_audio(now, &self.source))
            && self.analysis_source.as_ref().is_none_or(|(id, old)|
                state.pool.get(*id).is_some_and(|now| same_audio(now, old)))
    }
}
fn same_audio(a: &AudioBuffer, b: &AudioBuffer) -> bool {
    a.samples().as_ptr() == b.samples().as_ptr()
        && a.samples().len() == b.samples().len()
        && a.sample_rate() == b.sample_rate()
        && a.channels() == b.channels()
}

fn map_region(
    region: &windfall_project::AudioRegion,
    map: impl Fn(u32) -> u32,
) -> windfall_project::AudioRegion {
    let a = map(region.start_frame);
    let b = map(region.end_frame);
    windfall_project::AudioRegion {
        name: region.name.clone(),
        start_frame: a.min(b),
        end_frame: a.max(b),
    }
}
fn region_transform(prepared: &Prepared) -> (f64, f64, f64, bool) {
    let ClipContent::Audio {
        stretch,
        pitch,
        reverse,
        ..
    } = prepared.clip.content
    else {
        unreachable!()
    };
    let (ratio, speed) = match stretch {
        windfall_project::ClipStretch::Tape => (1.0, 2.0_f64.powf(f64::from(pitch) / 12.0)),
        windfall_project::ClipStretch::Spectral { ratio, .. } => (ratio, 1.0),
    };
    let skip = prepared.view_timing.offset_frames * speed;
    (ratio, speed, skip, reverse)
}
fn source_to_view(prepared: &Prepared, frame: u32) -> f64 {
    let (ratio, speed, skip, reverse) = region_transform(prepared);
    let position = if reverse {
        prepared.source.frames() as f64 * ratio - f64::from(frame) * ratio
    } else {
        f64::from(frame) * ratio
    };
    (position - skip) / speed
}
fn view_to_source(prepared: &Prepared, frame: u32) -> u32 {
    let (ratio, speed, skip, reverse) = region_transform(prepared);
    let position = skip + f64::from(frame) * speed;
    let frame = if reverse {
        (prepared.source.frames() as f64 * ratio - position) / ratio
    } else {
        position / ratio
    };
    frame.round().clamp(0.0, prepared.source.frames() as f64) as u32
}
fn view_regions(prepared: &Prepared, state: &State) -> Vec<windfall_project::AudioRegion> {
    state
        .document
        .project()
        .sample(prepared.clip.content.sample().unwrap())
        .map_or_else(Vec::new, |asset| {
            asset
                .regions
                .iter()
                .filter(|r| {
                    let a = source_to_view(prepared, r.start_frame);
                    let b = source_to_view(prepared, r.end_frame);
                    a.max(b) >= 0.0 && a.min(b) <= prepared.view.frames() as f64
                })
                .map(|r| {
                    map_region(r, |frame| {
                        source_to_view(prepared, frame)
                            .round()
                            .clamp(0.0, prepared.view.frames() as f64)
                            as u32
                    })
                })
                .collect()
        })
}
fn edited_regions(
    regions: &[windfall_project::AudioRegion],
    request: AudioEditRequest,
    old_frames: usize,
    new_frames: usize,
    trim_bounds: Option<(usize, usize)>,
) -> Vec<windfall_project::AudioRegion> {
    let a = request.start_frame;
    let b = request.end_frame;
    let mut regions: Vec<_> = regions.to_vec();
    if matches!(
        request.operation,
        AudioEditOperation::Trim | AudioEditOperation::Extract
    ) {
        regions.retain(|r| r.end_frame >= a && r.start_frame <= b);
        for r in &mut regions {
            r.start_frame = r.start_frame.clamp(a, b) - a;
            r.end_frame = r.end_frame.clamp(a, b) - a;
        }
        return regions;
    }
    if request.operation == AudioEditOperation::Cut {
        regions.retain(|r| !(r.start_frame >= a && r.end_frame <= b));
    }
    let delta = new_frames as i64 - old_frames as i64;
    regions
        .iter()
        .map(|r| {
            map_region(r, |frame| {
                let value = match request.operation {
                    AudioEditOperation::Reverse if (a..=b).contains(&frame) => {
                        i64::from(a + b - frame)
                    }
                    AudioEditOperation::Cut if frame > a => {
                        i64::from(frame.max(b)) - i64::from(b - a)
                    }
                    AudioEditOperation::Stretch { ratio, .. } if (a..b).contains(&frame) => {
                        i64::from(a) + ((frame - a) as f64 * ratio).round() as i64
                    }
                    AudioEditOperation::Script if (a..b).contains(&frame) => {
                        i64::from(a)
                            + ((frame - a) as f64
                                * ((i64::from(b - a) + delta) as f64 / f64::from(b - a)))
                            .round() as i64
                    }
                    AudioEditOperation::InsertSilence { frames } if frame >= a => {
                        i64::from(frame) + i64::from(frames)
                    }
                    AudioEditOperation::TrimSilence { .. } if frame >= a && frame <= b => {
                        let (keep, stop) =
                            trim_bounds.expect("successful silence trim has retained bounds");
                        i64::from(a) + frame.clamp(keep as u32, stop as u32) as i64 - keep as i64
                    }
                    _ if frame >= b => i64::from(frame) + delta,
                    _ => i64::from(frame),
                };
                value.clamp(0, new_frames as i64) as u32
            })
        })
        .collect()
}

/// Deletes refused/unattached output only, after all session guards drop.
struct Output(PathBuf);
impl Drop for Output {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.0);
        }
    }
}

impl Session {
    pub fn audio_editor_open(&self, id: ClipId) -> Result<AudioEditPreview, String> {
        let mut editor = lock(&self.inner.audio_editor);
        // Drop the previous view before allocating another. This is worker-only.
        editor.prepared = None;
        let (clip, pool, generation, edits, name, project, recipe) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let clip = project
                .playlist
                .clips
                .iter()
                .find(|c| c.id == id)
                .cloned()
                .ok_or("The clip no longer exists.")?;
            let sample = clip.content.sample().ok_or("Select an audio clip.")?;
            let name = project
                .sample(sample)
                .ok_or("The sample is missing.")?
                .name
                .clone();
            (
                clip,
                state.pool.clone(),
                state.generation,
                state.edits,
                name,
                project.clone(),
                project.sample(sample).and_then(|s| s.processing.clone()),
            )
        };
        let source = pool
            .get(clip.content.sample().unwrap())
            .cloned()
            .ok_or("The clip's audio is not loaded. Reload samples first.")?;
        let clock = windfall_engine::SongClock::new(&project)?;
        let timing = audio_edit::view_timing(&pool, &clip, project.settings.tempo_bpm, &clock)?;
        let view = audio_edit::render_view_timed(&pool, &clip, &timing)?;
        let analysis_timing = if let Some(recipe) = &recipe {
            match recipe.source_timing {
                Some(timing) => timing,
                None => audio_edit::view_timing(&pool, &recipe.source_clip, project.settings.tempo_bpm, &clock)?,
            }
        } else { timing };
        let analysis_view = if let Some(recipe) = &recipe {
            audio_edit::render_view_timed(&pool, &recipe.source_clip, &analysis_timing)?
        } else { view.clone() };
        let analysis_source = recipe.as_ref().map(|recipe| {
            let id = recipe.source_clip.content.sample().expect("checked saved recipe");
            pool.get(id).cloned().map(|audio| (id, audio)).ok_or("The original source is not loaded.")
        }).transpose()?;
        editor.serial = editor
            .serial
            .checked_add(1)
            .ok_or("Audio editor tokens exhausted. Restart Windfall.")?;
        let prepared = Prepared {
            token: editor.serial,
            generation,
            edits,
            clip,
            source,
            view,
            clock,
            stored_tempo: project.settings.tempo_bpm,
            view_timing: timing,
            analysis_timing,
            analysis_view,
            analysis_source,
            recipe,
            rendered: None,
        };
        let preview = AudioEditPreview {
            token: prepared.token,
            clip: id,
            name,
            frames: prepared.view.frames() as u32,
            sample_rate: prepared.view.sample_rate(),
            peaks: audio_edit::overview(&prepared.view, 512),
            spectral: audio_edit::advanced::spectrogram(&prepared.view),
            regions: view_regions(&prepared, &self.state()),
        };
        #[cfg(test)]
        self.pause("audio-editor:rendered");
        let _recording = self.recording_idle()?;
        if !prepared.valid(&self.state()) {
            return Err(STALE.into());
        }
        editor.prepared = Some(prepared);
        Ok(preview)
    }

    pub fn audio_editor_discard(&self, token: u32) {
        let mut editor = lock(&self.inner.audio_editor);
        if editor.prepared.as_ref().is_some_and(|p| p.token == token) {
            editor.prepared = None;
        }
    }

    pub fn audio_editor_regions(
        &self,
        token: u32,
        regions: Vec<windfall_project::AudioRegion>,
    ) -> Result<DispatchResult, String> {
        let mut editor = lock(&self.inner.audio_editor);
        let prepared = editor
            .prepared
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or("This audio editor has expired.")?;
        windfall_project::model::check_audio_regions(&regions)?;
        if regions
            .iter()
            .any(|r| r.end_frame as usize > prepared.view.frames())
        {
            return Err("Regions must fit the clip view.".into());
        }
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !prepared.valid(&state) {
                return Err(STALE.into());
            }
            let sample = prepared.clip.content.sample().unwrap();
            let old = &state
                .document
                .project()
                .sample(sample)
                .ok_or(STALE)?
                .regions;
            let mut source_regions: Vec<_> = old
                .iter()
                .filter(|r| {
                    let a = source_to_view(prepared, r.start_frame);
                    let b = source_to_view(prepared, r.end_frame);
                    a.max(b) < 0.0 || a.min(b) > prepared.view.frames() as f64
                })
                .cloned()
                .collect();
            source_regions.extend(
                regions
                    .iter()
                    .map(|r| map_region(r, |frame| view_to_source(prepared, frame))),
            );
            self.sample_edit_ticket(
                &state,
                Command::SetSampleRegions {
                    sample,
                    expected: old.clone(),
                    regions: source_regions,
                },
                None,
                Vec::new(),
            )?
        };
        let result = self.finish_sample_edit(ticket)?;
        editor.prepared = None;
        Ok(result)
    }

    pub fn audio_editor_apply(&self, request: AudioEditRequest) -> Result<DispatchResult, String> {
        self.audio_editor_apply_with(request, None, |view, request| {
            audio_edit::apply(
                view,
                request.operation,
                request.start_frame,
                request.end_frame,
            )
        })
    }

    pub fn audio_editor_transfer(
        &self,
        request: windfall_ipc::AudioEditTransferRequest,
    ) -> Result<DispatchResult, String> {
        let edit = AudioEditRequest {
            token: request.token,
            operation: AudioEditOperation::Extract,
            start_frame: request.start_frame,
            end_frame: request.end_frame,
        };
        self.audio_editor_apply_with(edit, Some(request.destination), |view, request| {
            audio_edit::apply(
                view,
                request.operation,
                request.start_frame,
                request.end_frame,
            )
        })
    }

    /// The sandbox supplies a bounded processor without document/file access.
    /// Scripted edits share built-in token, source, publication/history guards.
    pub fn audio_editor_run_script_selection(
        &self,
        token: u32,
        range: windfall_ipc::AudioEditRange,
        run: impl FnOnce(&AudioBuffer) -> Result<AudioBuffer, String>,
    ) -> Result<DispatchResult, String> {
        let request = AudioEditRequest {
            token,
            operation: AudioEditOperation::Script,
            start_frame: range.start_frame,
            end_frame: range.end_frame,
        };
        self.audio_editor_apply_with(request, None, |view, request| {
            if request.start_frame >= request.end_frame
                || request.end_frame as usize > view.frames()
            {
                return Err("Select a valid script range.".into());
            }
            let channels = usize::from(view.channels());
            let selection = AudioBuffer::from_interleaved(
                view.sample_rate(),
                view.channels(),
                view.samples()[request.start_frame as usize * channels
                    ..request.end_frame as usize * channels]
                    .to_vec(),
            );
            let replacement = run(&selection)?;
            if replacement.frames() == 0 {
                return Err("A script cannot return an empty selection.".into());
            }
            audio_edit::replace_selection(view, request.start_frame, request.end_frame, replacement)
        })
    }

    fn audio_editor_apply_with(
        &self,
        request: AudioEditRequest,
        destination: Option<windfall_ipc::AudioEditDestination>,
        render: impl FnOnce(&AudioBuffer, AudioEditRequest) -> Result<audio_edit::Edited, String>,
    ) -> Result<DispatchResult, String> {
        let mut editor = lock(&self.inner.audio_editor);
        self.audio_editor_commit_with(&mut editor, request, destination, None, render)
    }

    fn audio_editor_commit(
        &self, editor: &mut Editor, request: AudioEditRequest,
        rendered: Option<(AudioBuffer, windfall_project::AudioEditRecipe)>,
    ) -> Result<DispatchResult, String> {
        self.audio_editor_commit_with(editor, request, None, rendered, |view, request| {
            audio_edit::apply(view, request.operation, request.start_frame, request.end_frame)
        })
    }
    fn audio_editor_commit_with(
        &self, editor: &mut Editor, request: AudioEditRequest,
        destination: Option<windfall_ipc::AudioEditDestination>,
        rendered: Option<(AudioBuffer, windfall_project::AudioEditRecipe)>,
        render: impl FnOnce(&AudioBuffer, AudioEditRequest) -> Result<audio_edit::Edited, String>,
    ) -> Result<DispatchResult, String> {
        let prepared = editor
            .prepared
            .as_ref()
            .filter(|p| p.token == request.token)
            .ok_or("This audio editor has expired. Reopen it.")?;
        let (mut document, mut pool, generation, edits, preparation) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !prepared.valid(&state) {
                return Err(STALE.into());
            }
            (
                state.document.clone(),
                state.pool.clone(),
                state.generation,
                state.edits,
                self.project_preparation(&state),
            )
        };
        let recipe = rendered.as_ref().map(|(_, recipe)| recipe.clone());
        let operation_label = if recipe.is_some() { "Pitch/time/noise" } else { request.operation.label() };
        let audio_edit::Edited {
            audio: output,
            lead,
} = if let Some((audio, _)) = rendered { audio_edit::Edited { audio, lead: 0 } } else { render(&prepared.view, request)? };
        let strip_regions = if let AudioEditOperation::StripSilence {
            threshold_db,
            min_silence_frames,
        } = request.operation
        {
            Some(audio_edit::advanced::sound_regions(
                &prepared.view,
                request.start_frame as usize,
                request.end_frame as usize,
                threshold_db,
                min_silence_frames,
            )?)
        } else {
            None
        };
        let per_tick = samples_per_tick(prepared.stored_tempo, f64::from(output.sample_rate()));
        let shift = if matches!(
            destination,
            Some(windfall_ipc::AudioEditDestination::Sampler { .. })
        ) {
            0
        } else if request.operation == AudioEditOperation::Extract {
            prepared.clip.length
        } else {
            prepared.clock.tick_after(prepared.clip.start, lead as f64 / f64::from(output.sample_rate()))?
                .checked_sub(prepared.clip.start).ok_or("Invalid edit onset.")?
        };
        let clip = &prepared.clip;
        let mut start = clip
            .start
            .checked_add(shift)
            .ok_or("Edit falls outside the song.")?;
        if let Some(windfall_ipc::AudioEditDestination::Playlist { start: at, .. }) = destination {
            start = at;
        }
        // Preserve exact existing edges for same-size edits. Otherwise choose
        // the first integer tick whose view contains every output frame.
        let length = if output.frames() == prepared.view.frames() && shift == 0 {
            clip.length
        } else {
            let last_frame = (output.frames() as f64 - 1.0 + 1e-6) / f64::from(output.sample_rate());
            let end = prepared.clock.tick_after_precise(start, last_frame)?.floor() + 1.0;
            if end > f64::from(MAX_SONG_TICKS) { return Err("Edit falls outside the song.".into()); }
            (end as u32).checked_sub(start).filter(|length| *length > 0)
                .ok_or("Invalid edited clip duration.")?
        };
        if start.saturating_add(length) > MAX_SONG_TICKS {
            return Err("Edit falls outside the song.".into());
        }
        // Persistent app-owned storage works before the project has a save path.
        // Completed files remain for undo/redo and saved documents.
        let folder = self.store().recordings_dir().join("Audio edits");
        fs::create_dir_all(&folder)
            .map_err(|e| format!("Could not create audio edit folder: {e}"))?;
        let mut owned = None;
        for _ in 0..1000 {
            let path = folder.join(format!(
                "Edit-{}-{}.wav",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {
                    owned = Some(Output(path));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("Could not create audio edit file: {e}")),
            }
        }
        let mut owned = owned.ok_or("Could not reserve an audio edit file.")?;
        windfall_codec::write_wav(&owned.0, &output, windfall_codec::WavSampleFormat::Float32)
            .map_err(|e| e.to_string())?;
        // Populate ordinary sample cache off-lock too, so history can restore it
        // without decoding under state. WAV Float32 preserves the edit exactly.
        let decoded = self.inner.cache.decode(&owned.0)?;
        let sample = SampleId(document.project().next_id);
        let label = format!("Audio edit: {operation_label}");
        let source = document
            .project()
            .sample(clip.content.sample().unwrap())
            .unwrap();
        // Several edits in one sitting name the result after the latest one,
        // not after the whole chain.
        let base = match &source.path {
            SamplePath::External(path)
                if std::path::Path::new(path).parent() == Some(folder.as_path()) =>
            {
                source
                    .name
                    .rsplit_once(" (")
                    .map_or(source.name.as_str(), |(base, _)| base)
            }
            _ => &source.name,
        };
        let mut commands = vec![Command::AddSample {
            name: format!("{base} ({operation_label})"),
            path: SamplePath::External(crate::paths::display(&owned.0)),
        }];
        let trim_bounds =
            if let AudioEditOperation::TrimSilence { threshold_db } = request.operation {
                audio_edit::clean::silence_bounds(
                    prepared.view.samples(),
                    usize::from(prepared.view.channels()),
                    request.start_frame as usize,
                    request.end_frame as usize,
                    threshold_db,
                )?
            } else {
                None
            };
        let regions = edited_regions(
            &view_regions(prepared, &self.state()),
            request,
            prepared.view.frames(),
            output.frames(),
            trim_bounds,
        );
        if !regions.is_empty() {
            commands.push(Command::SetSampleRegions {
                sample,
                expected: Vec::new(),
                regions,
            });
        }
        if request.operation != AudioEditOperation::Extract {
            commands.push(Command::RemoveClips {
                clips: vec![clip.id],
            });
        }
        let ClipContent::Audio {
            mixer_track,
            output,
            ..
        } = clip.content
        else {
            unreachable!()
        };
        let target_track =
            if let Some(windfall_ipc::AudioEditDestination::Playlist { track, .. }) = destination {
                if let Some(track) = track {
                    track
                } else {
                    let id = windfall_project::PlaylistTrackId(
                        sample.0.checked_add(1).ok_or("Ids exhausted")?,
                    );
                    commands.push(Command::AddPlaylistTrack {
                        name: Some("Editor selection".into()),
                        index: None,
                    });
                    id
                }
            } else {
                clip.track
            };
        let replacement = ClipInit {
            track: target_track,
            start,
            length: Some(length),
            offset: Some(0),
            muted: Some(clip.muted),
            content: ClipContent::Audio {
                sample,
                mixer_track,
                output,
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,
                stretch: Default::default(),
            },
        };
        let clips = if let Some(regions) = strip_regions {
            let mut clips = Vec::with_capacity(regions.len());
            let mut previous_end = 0;
            for (first, end) in regions {
                let offset = (first as f64 / per_tick).round() as u32;
                let start = prepared.clock.tick_after(clip.start, first as f64 / f64::from(prepared.view.sample_rate()))?;
                let finish = prepared.clock.tick_after(clip.start, end as f64 / f64::from(prepared.view.sample_rate()))?;
                let length = finish.checked_sub(start).filter(|length| *length > 0).ok_or("Silence split is shorter than one tick.")?;
                if start < previous_end || start.saturating_add(length) > MAX_SONG_TICKS {
                    return Err("Silence splits overlap at this tempo/timebase. Increase the minimum silence duration.".into());
                }
                previous_end = start + length;
                clips.push(ClipInit {
                    start,
                    offset: Some(offset),
                    length: Some(length),
                    ..replacement.clone()
                });
            }
            clips
        } else {
            vec![replacement]
        };
        if let Some(windfall_ipc::AudioEditDestination::Sampler { channel }) = destination {
            commands.push(Command::SetChannelSample {
                id: channel,
                sample: Some(sample),
            });
            commands.push(Command::UpdateSampler {
                id: channel,
                patch: windfall_project::SamplerPatch {
                    start: Some(0.0),
                    end: Some(1.0),
                    reverse: Some(false),
                    loop_mode: Some(windfall_project::SamplerLoopMode::Off),
                    stretch: Some(windfall_project::SamplerStretch::Tape),
                    tune: Some(0.0),
                    ..Default::default()
                },
            });
        } else {
            commands.push(Command::AddClips { clips });
        }
        if let Some(processing) = recipe {
            commands.push(Command::SetSampleProcessing { id: sample, processing: Some(processing) });
        }
        let command = Command::Batch {
            label: Some(label),
            commands,
        };
        document
            .dispatch(command.clone(), None)
            .map_err(|e| e.to_string())?;
        pool.insert(sample, decoded.clone());
        let mut plan = preparation
            .prepare(
                document.project(),
                &pool,
                windfall_engine::ProjectPublicationIntent::Edit,
            )
            .map_err(|error| error.to_string())?;
        let retirement;
        #[cfg(test)]
        self.pause("audio-editor:prepared");
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if let Some(cancel) = &*lock(&self.inner.audio_analysis_cancel) {
                if cancel.is_cancelled() { return Err("Analysis cancelled.".into()); }
            }
            if state.generation != generation
                || state.edits != edits
                || !prepared.valid(&state)
                || state
                    .pool
                    .iter()
                    .any(|(id, now)| pool.get(id).is_none_or(|old| !same_audio(now, old)))
                || pool.iter().any(|(id, old)| {
                    id != sample && state.pool.get(id).is_none_or(|now| !same_audio(now, old))
                })
            {
                return Err(STALE.into());
            }
            let pool = plan.pool().clone();
            let lease = plan.publication(self, &state)?;
            let applied = state
                .document
                .dispatch(command, None)
                .map_err(|e| e.to_string())?;
            self.commit_prepared_parameters(&state, &applied.touched);
            retirement = lease.install();
            state.pool.insert(sample, decoded);
            state.loaded.insert(sample);
            state.failed.remove(&sample);
            self.own_derived(&owned.0);
            owned.0 = PathBuf::new(); // Doc + exact ready install now own the persistent source
            DispatchResult {
                created: applied.created,
                patch: self.publish_prepared(&mut state, &applied.touched, &pool),
            }
        };
        let mut retirement = retirement;
        self.retire_project(&mut retirement);
        editor.prepared = None;
        Ok(result)
    }
}
