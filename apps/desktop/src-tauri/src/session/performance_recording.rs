//! Performance takes use the ordinary checked sample/settings transaction.
use super::Session;
use crate::sync::lock;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use windfall_engine::performance_recording::PerformanceRecording;
use windfall_project::{Command, PerformanceTake, SampleId, SamplePath};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Preparing<'a>(&'a std::sync::atomic::AtomicBool);
impl Drop for Preparing<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}
pub(super) struct Capture {
    data: Arc<PerformanceRecording>,
    generation: u64,
    revision: u64,
    stream: (u64, u32),
    stop_requested: Option<std::time::Instant>,
}
impl Session {
    /// Performance pad input is allowed during its own capture. Ordinary
    /// recording and finishing still exclude it using the same first lock.
    pub(super) fn performance_input_guard(&self) -> Result<std::sync::MutexGuard<'_, Option<super::recording::Take>>, String> {
        let guard = lock(&self.inner.recording);
        if guard.is_some() || self.inner.recording_finishing.load(Ordering::Acquire) {
            return Err("Stop or cancel input recording first".into());
        }
        Ok(guard)
    }
    pub fn performance_record_state(&self) -> windfall_ipc::PerformanceRecordingState {
        lock(&self.inner.performance_capture).as_ref().map_or(windfall_ipc::PerformanceRecordingState {
            active: false, finished: false, frames: 0, sample_rate: 0, error: None,
        }, |capture| {
            if capture.data.state().active && (self.controller().performance_stream_stamp() != Some(capture.stream) || capture.stop_requested.is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(2))) { capture.data.invalidate(); }
            capture.data.state()
        })
    }
    pub fn performance_record_start(&self, generation: u64, revision: u64) -> Result<(), String> {
        self.inner.performance_starting.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).map_err(|_| "A performance capture is already being prepared")?;
        let _preparing = Preparing(&self.inner.performance_starting);
        if self.inner.performance_capturing.load(Ordering::Acquire) { return Err("Finish or cancel performance recording first".into()); }
        let stream = self.controller().performance_stream_stamp().ok_or("Performance recording needs an active audio stream")?;
        let rate = stream.1;
        let data = PerformanceRecording::prepare(rate, 60)?;
        let _recording = self.recording_idle()?;
        let mut owned = lock(&self.inner.performance_capture);
        let state = self.state();
        if owned.is_some() || state.generation != generation || state.document.revision() != revision {
            return Err("The project changed or a performance capture already exists".into());
        }
        if state.document.project().settings.stage_media.takes.len() >= 16 {
            return Err("Remove a saved performance before recording another (maximum 16)".into());
        }
        if !state.loading.is_empty() || self.controller().transport().playing {
            return Err("Stop the song and wait for samples to load before performance recording".into());
        }
        if self.controller().performance_stream_stamp() != Some(stream) { return Err("The audio stream changed before recording could start".into()); }
        self.controller().performance_record_begin(data.clone())?;
        *owned = Some(Capture { data, generation, revision, stream, stop_requested: None });
        self.inner.performance_capturing.store(true, Ordering::Release);
        Ok(())
    }
    pub fn performance_record_stop(&self) {
        if let Some(capture) = lock(&self.inner.performance_capture).as_mut() { capture.stop_requested.get_or_insert_with(std::time::Instant::now); }
        self.controller().performance_record_end();
    }
    pub fn performance_record_cancel(&self) {
        let _recording = lock(&self.inner.recording);
        self.controller().performance_record_end();
        let old = lock(&self.inner.performance_capture).take();
        self.inner.performance_capturing.store(false, Ordering::Release);
        drop(_recording);
        drop(old);
    }
    pub fn performance_record_save(&self) -> Result<(), String> {
        let (data, generation, revision) = {
            let owned = lock(&self.inner.performance_capture);
            let capture = owned.as_ref().ok_or("No performance capture is waiting to save")?;
            (capture.data.clone(), capture.generation, capture.revision)
        };
        let recorded = data.snapshot()?;
        if recorded.audio.frames() == 0 { return Err("The performance capture is empty".into()); }
        let folder = self.store().recordings_dir();
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let path = loop {
            let path = folder.join(format!("Performance-{}-{}.wav", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        };
        let result = (|| {
            windfall_codec::write_wav(&path, &recorded.audio, windfall_codec::WavSampleFormat::Float32).map_err(|error| error.to_string())?;
            let ticket = {
                let _recording = self.performance_input_guard()?;
                let owned = lock(&self.inner.performance_capture);
                if owned.as_ref().is_none_or(|capture| !Arc::ptr_eq(&capture.data, &data)) { return Err("Performance capture was cancelled".into()); }
                let state = self.state();
                if state.generation != generation || state.document.revision() != revision { return Err("The project changed during performance recording".into()); }
                let sample = SampleId(state.document.project().next_id);
                let mut media = state.document.project().settings.stage_media.clone();
                media.takes.push(PerformanceTake { sample, sample_rate: recorded.audio.sample_rate(), frames: recorded.audio.frames() as u32, initial_tick: recorded.initial_tick, events: recorded.events.clone() });
                self.sample_edit_ticket(&state, Command::Batch { label: Some("Record performance".into()), commands: vec![
                    Command::AddSample { name: "Recorded performance".into(), path: SamplePath::External(path.to_string_lossy().into_owned()) },
                    Command::SetStageMedia { media },
                ] }, None, vec![(sample, recorded.audio.clone())])?
            };
            let mut prepared = ticket.prepare()?;
            let mut retirement = None;
            let result = {
                let _recording = self.performance_input_guard()?;
                let mut owned = lock(&self.inner.performance_capture);
                if owned.as_ref().is_none_or(|capture| !Arc::ptr_eq(&capture.data, &data)) { return Err("Performance capture was cancelled".into()); }
                let mut state = self.state();
                let result = prepared.commit(&mut state, &mut retirement);
                if result.is_ok() {
                    owned.take();
                    self.inner.performance_capturing.store(false, Ordering::Release);
                }
                result
            };
            if let Some(retirement) = &mut retirement { self.retire_project(retirement); }
            result.map(|_| ())
        })();
        if result.is_ok() { self.own_derived(&path); } else { let _ = std::fs::remove_file(&path); }
        result
    }
    fn performance_take_audio(&self, index: usize, generation: u64, revision: u64) -> Result<(windfall_core::AudioBuffer, Vec<windfall_project::PerformanceLogEvent>), String> {
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision { return Err("The project changed before performance replay/export".into()); }
        let take = state.document.project().settings.stage_media.takes.get(index).ok_or("Recorded performance no longer exists")?;
        let audio = state.pool.get(take.sample).ok_or("Recorded performance audio is missing. Restore its WAV and reload samples.")?;
        if audio.sample_rate() != take.sample_rate || audio.frames() != take.frames as usize || audio.channels() != 2 {
            return Err("Recorded performance WAV no longer matches its saved clock".into());
        }
        Ok((audio.clone(), take.events.clone()))
    }
    pub fn performance_take_replay(&self, index: Option<usize>, generation: u64, revision: u64) -> Result<(), String> {
        let _recording = self.recording_idle()?;
        if let Some(index) = index {
            let (audio, events) = self.performance_take_audio(index, generation, revision)?;
            self.controller().performance_replay_log(audio, events)
        } else { self.controller().performance_replay(None) }
    }
    pub fn performance_take_export(&self, index: usize, path: &str, generation: u64, revision: u64) -> Result<(), String> {
        let (audio, _) = self.performance_take_audio(index, generation, revision)?;
        windfall_codec::write_wav(path, &audio, windfall_codec::WavSampleFormat::Float32).map_err(|error| error.to_string())
    }
}
