//! Preview retains a bounded, immutable result. Apply never reruns authored code.
use super::*;
use std::time::{Duration, Instant};
use windfall_ipc::{AudioScriptPreview, AudioScriptRequest};
use windfall_project::scripting::Program;

pub(super) struct Preview {
    pub request: u64,
    token: u32,
    range: windfall_ipc::AudioEditRange,
    audio: AudioBuffer,
}

impl Session {
    pub fn audio_script_preview(&self, input: AudioScriptRequest) -> Result<AudioScriptPreview, String> {
        let _worker = match self.inner.script_worker.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err("A script worker is already running.".into()),
        };
        let start = Instant::now();
        let cancelled = || self.inner.script_ticket.load(Ordering::Acquire) != input.request
            || start.elapsed() > Duration::from_secs(5);
        if cancelled() { return Err("Audio script cancelled or deadline exceeded.".into()); }
        let program = Program::compile(&input.source).map_err(|e| e.to_string())?;
        let mut editor = lock(&self.inner.audio_editor);
        editor.script_preview = None;
        let prepared = editor.prepared.as_ref().filter(|p| p.token == input.token)
            .ok_or("This audio editor expired. Reopen it.")?;
        {
            let _recording = self.recording_idle()?;
            if !prepared.valid(&self.state()) { return Err(STALE.into()); }
        }
        let range = input.range;
        if range.start_frame >= range.end_frame || range.end_frame as usize > prepared.view.frames() {
            return Err("Select a valid audio script range.".into());
        }
        let channels = usize::from(prepared.view.channels());
        let selection = AudioBuffer::from_interleaved(prepared.view.sample_rate(), prepared.view.channels(),
            prepared.view.samples()[range.start_frame as usize * channels..range.end_frame as usize * channels].to_vec());
        let audio = program.process_audio(&selection, &input.parameters, 100_000_000, cancelled)
            .map_err(|e| e.to_string())?;
        let reply = AudioScriptPreview { request: input.request, token: input.token, range,
            frames: audio.frames() as u32, sample_rate: audio.sample_rate(),
            peaks: audio_edit::overview(&audio, 512) };
        if cancelled() { return Err("Audio script cancelled or deadline exceeded.".into()); }
        let _recording = self.recording_idle()?;
        if !prepared.valid(&self.state()) { return Err(STALE.into()); }
        editor.script_preview = Some(Preview { request: input.request, token: input.token, range, audio });
        Ok(reply)
    }

    pub fn audio_script_audition(&self, token: u32, request: u64) -> Result<(), String> {
        let mut latest = lock(&self.inner.preview);
        *latest = latest.checked_add(1).ok_or("Preview tickets exhausted.")?;
        let ticket = *latest;
        drop(latest);
        let editor = lock(&self.inner.audio_editor);
        let (preview, prepared) = self.checked_audio_script(&editor, token, request)?;
        let _recording = self.recording_idle()?;
        if !prepared.valid(&self.state()) { return Err(STALE.into()); }
        let latest = lock(&self.inner.preview);
        if *latest == ticket { self.controller().preview(preview.audio.clone()); }
        Ok(())
    }

    fn checked_audio_script<'a>(&self, editor: &'a Editor, token: u32, request: u64) -> Result<(&'a Preview, &'a Prepared), String> {
        if self.inner.script_ticket.load(Ordering::Acquire) != request {
            return Err("Audio script preview cancelled. Run script again.".into());
        }
        let preview = editor.script_preview.as_ref().filter(|p| p.token == token && p.request == request)
            .ok_or("Run script and review its preview before applying.")?;
        let prepared = editor.prepared.as_ref().filter(|p| p.token == token).ok_or(STALE)?;
        Ok((preview, prepared))
    }

    pub fn audio_script_apply(&self, token: u32, request: u64) -> Result<DispatchResult, String> {
        let mut editor = lock(&self.inner.audio_editor);
        let (preview, _) = self.checked_audio_script(&editor, token, request)?;
        let range = preview.range;
        let audio = preview.audio.clone();
        self.audio_editor_commit_with(&mut editor, AudioEditRequest { token, operation: AudioEditOperation::Script,
            start_frame: range.start_frame, end_frame: range.end_frame }, None, None,
            |view, _| audio_edit::replace_selection(view, range.start_frame, range.end_frame, audio))
    }
}
