//! Prepared analysis editor. All FFT/stretch/IO occurs outside recording/State.
use super::{lock, Editor, Session, STALE};
use std::time::Duration;
use windfall_analysis::{classical, CancelToken, Work};
use windfall_core::TICKS_PER_STEP;
use windfall_ipc::{AudioAnalysisEdit, AudioAnalysisPreview, AudioEditOperation, AudioEditRequest};
use windfall_project::{
    AudioEditRecipe, AudioWarpMarker, ChannelId, ClipContent, ClipInit, Command, DispatchResult,
    NoteInit, PatternId, PatternPatch, MAX_PATTERN_TICKS,
};

struct Preparation<'a>(&'a Session);
impl Drop for Preparation<'_> {
    fn drop(&mut self) {
        *lock(&self.0.inner.audio_analysis_cancel) = None;
    }
}
fn begin(session: &Session) -> Result<(Preparation<'_>, Work), String> {
    let cancel = CancelToken::default();
    *lock(&session.inner.audio_analysis_cancel) = Some(cancel.clone());
    let work =
        Work::new(cancel, 100_000_000_000, Duration::from_secs(120)).map_err(|e| e.to_string())?;
    Ok((Preparation(session), work))
}
fn checked_recipe<'a>(
    editor: &'a Editor,
    edit: &AudioAnalysisEdit,
) -> Result<&'a super::Prepared, String> {
    let prepared = editor
        .prepared
        .as_ref()
        .filter(|p| p.token == edit.token)
        .ok_or("This audio analysis editor expired.")?;
    let original = prepared
        .recipe
        .as_ref()
        .ok_or("Detect audio before reviewing edits.")?;
    if edit.recipe.analysis != original.analysis
        || edit.recipe.source_clip != original.source_clip
        || edit.recipe.source_timing != original.source_timing
    {
        return Err(
            "Detected estimates and source provenance cannot be rewritten. Edit overrides instead."
                .into(),
        );
    }
    edit.recipe.check()?;
    Ok(prepared)
}
impl Session {
    pub fn audio_analysis_cancel(&self) {
        if let Some(cancel) = &*lock(&self.inner.audio_analysis_cancel) {
            cancel.cancel();
        }
    }
    pub fn audio_analysis_detect(
        &self,
        token: u32,
        polyphonic: bool,
    ) -> Result<AudioAnalysisPreview, String> {
        let mut editor = lock(&self.inner.audio_editor);
        let prepared = editor
            .prepared
            .as_mut()
            .filter(|p| p.token == token)
            .ok_or("This audio editor expired.")?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) {
                return Err(STALE.into());
            }
        }
        let (_preparation, mut work) = begin(self)?;
        // Reopen a saved monophonic proposal exactly; switching conversion mode
        // creates a fresh detection and resets overrides explicitly.
        if prepared
            .recipe
            .as_ref()
            .is_none_or(|r| r.analysis.polyphonic != polyphonic)
        {
            let analysis = classical::detect(&prepared.analysis_view, polyphonic, &mut work)
                .map_err(|e| e.to_string())?;
            let source_clip = prepared
                .recipe
                .as_ref()
                .map_or_else(|| prepared.clip.clone(), |r| r.source_clip.clone());
            let frames = analysis.frames;
            let markers = std::iter::once(0)
                .chain(analysis.transients.iter().copied())
                .chain(std::iter::once(frames))
                .map(|frame| AudioWarpMarker {
                    source_frame: frame,
                    target_frame: frame,
                })
                .collect();
            prepared.recipe = Some(AudioEditRecipe {
                source_clip,
                source_timing: Some(prepared.analysis_timing),
                analysis,
                overrides: vec![],
                markers,
                denoise: None,
            });
        }
        work.check().map_err(|e| e.to_string())?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) {
                return Err(STALE.into());
            }
        }
        prepared.rendered = None;
        Ok(AudioAnalysisPreview {
            token,
            recipe: prepared.recipe.clone().expect("detected"),
        })
    }
    pub fn audio_analysis_audition(
        &self,
        edit: AudioAnalysisEdit,
        dry: bool,
    ) -> Result<(), String> {
        // Reserve before rendering; Stop/newer browser preview always wins.
        let ticket = {
            let mut latest = lock(&self.inner.preview);
            *latest = latest.checked_add(1).ok_or("Preview tickets exhausted.")?;
            *latest
        };
        let mut editor = lock(&self.inner.audio_editor);
        let prepared = checked_recipe(&editor, &edit)?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) {
                return Err(STALE.into());
            }
        }
        let (_preparation, mut work) = begin(self)?;
        let output = if dry {
            prepared.analysis_view.clone()
        } else {
            match &prepared.rendered {
                Some((recipe, audio)) if recipe == &edit.recipe => audio.clone(),
                _ => classical::render(&prepared.analysis_view, &edit.recipe, &mut work)
                    .map_err(|e| e.to_string())?,
            }
        };
        work.check().map_err(|e| e.to_string())?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) {
                return Err(STALE.into());
            }
        }
        if !dry {
            editor.prepared.as_mut().expect("locked editor").rendered =
                Some((edit.recipe, output.clone()));
        }
        let latest = lock(&self.inner.preview);
        if *latest == ticket {
            self.controller().preview(output);
        }
        Ok(())
    }
    pub fn audio_analysis_apply(&self, edit: AudioAnalysisEdit) -> Result<DispatchResult, String> {
        let mut editor = lock(&self.inner.audio_editor);
        let prepared = checked_recipe(&editor, &edit)?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) {
                return Err(STALE.into());
            }
        }
        let (_preparation, mut work) = begin(self)?;
        let audio = match &prepared.rendered {
            Some((recipe, audio)) if recipe == &edit.recipe => audio.clone(),
            _ => classical::render(&prepared.analysis_view, &edit.recipe, &mut work)
                .map_err(|e| e.to_string())?,
        };
        work.check().map_err(|e| e.to_string())?;
        self.audio_editor_commit(
            &mut editor,
            AudioEditRequest {
                token: edit.token,
                operation: AudioEditOperation::Stretch {
                    ratio: 1.0,
                    semitones: 0.0,
                    quality: Default::default(),
                    formants: false,
                },
                start_frame: 0,
                end_frame: audio.frames() as u32,
            },
            Some((audio, edit.recipe)),
        )
    }
    pub fn audio_analysis_send_midi(
        &self,
        edit: AudioAnalysisEdit,
        selected: Vec<u32>,
    ) -> Result<DispatchResult, String> {
        let editor = lock(&self.inner.audio_editor);
        let prepared = checked_recipe(&editor, &edit)?;
        if selected.is_empty() || selected.len() > windfall_project::MAX_AUDIO_NOTES {
            return Err("Select at least one detected note.".into());
        }
        let notes = classical::reviewed_notes(&edit.recipe).map_err(|e| e.to_string())?;
        let (project, base) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !prepared.valid(&state) {
                return Err(STALE.into());
            }
            (state.document.project().clone(), prepared.clip.start)
        };
        let clock = windfall_engine::SongClock::new(&project)?;
        windfall_project::MeterMap::new(
            project.settings.time_signature,
            &project.playlist.timeline.meters,
        )?;
        let rate = edit.recipe.analysis.sample_rate as f64;
        let mut seen = std::collections::HashSet::new();
        let mut midi = Vec::with_capacity(selected.len());
        for index in selected {
            if !seen.insert(index) {
                return Err("Duplicate selected note.".into());
            }
            let n = notes
                .get(index as usize)
                .ok_or("Selected note no longer exists.")?;
            let start = clock
                .tick_after(base, n.start_frame as f64 / rate)?
                .checked_sub(base)
                .ok_or("Invalid note onset.")?;
            let end = clock
                .tick_after(base, n.end_frame as f64 / rate)?
                .checked_sub(base)
                .ok_or("Invalid note offset.")?;
            if start >= end || end > MAX_PATTERN_TICKS {
                return Err(
                    "Converted notes exceed pattern resolution/duration. Select a shorter range."
                        .into(),
                );
            }
            let key = n.midi_pitch.round();
            if !(0.0..=127.0).contains(&key) {
                return Err("Pitch exceeds MIDI range.".into());
            }
            midi.push(NoteInit {
                start,
                length: end - start,
                key: key as u8,
                velocity: Some((n.rms.sqrt() * 1.6).clamp(1.0 / 127.0, 1.0) as f32),
                pan: None,
                expression: None,
            });
        }
        let end = midi
            .iter()
            .map(|n| n.start + n.length)
            .max()
            .expect("selected");
        let pattern = PatternId(project.next_id);
        // Explicit existing Master route makes AddChannel allocate one ID.
        let channel = ChannelId(project.next_id.checked_add(1).ok_or("IDs exhausted.")?);
        let command = Command::Batch {
            label: Some("Audio to MIDI".into()),
            commands: vec![
                Command::AddPattern {
                    name: Some("Audio score".into()),
                },
                Command::AddChannel {
                    name: Some("Audio score".into()),
                    sample: None,
                    instrument: Some(windfall_project::InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: Some(windfall_project::TrackId::MASTER),
                },
                Command::UpdatePattern {
                    id: pattern,
                    patch: PatternPatch {
                        length_steps: Some(end.div_ceil(TICKS_PER_STEP)),
                        ..Default::default()
                    },
                },
                Command::AddNotes {
                    pattern,
                    channel,
                    notes: midi,
                },
                Command::AddClips {
                    clips: vec![ClipInit {
                        track: prepared.clip.track,
                        start: base,
                        length: Some(end),
                        offset: Some(0),
                        muted: Some(false),
                        content: ClipContent::Pattern { pattern },
                    }],
                },
            ],
        };
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !prepared.valid(&state) {
                return Err(STALE.into());
            }
            self.sample_edit_ticket(&state, command, None, Vec::new())?
        };
        let mut plan = ticket.prepare()?;
        let mut retirement = None;
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if !prepared.valid(&state) {
                return Err(STALE.into());
            }
            plan.commit(&mut state, &mut retirement)?
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        Ok(result)
    }
}
