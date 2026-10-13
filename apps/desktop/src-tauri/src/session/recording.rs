//! One owned take at a time. A WAV is streamed outside the audio callback,
//! then imported through the ordinary sample/clip command pipeline.
use super::{ClipPlace, Session};
use crate::sync::lock;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use windfall_codec::{WavSampleFormat, WavWriter};
use windfall_engine::recording::{AlignmentMetrics, Capture, CaptureRoute, recording_inputs};
use windfall_engine::recording_clock::CaptureGate;
use windfall_ipc::{
    RecordingInput, RecordingSource, RecordingState, RecordingTakeInfo, RecordingTakeSelection,
    RecordingTrackInfo,
};
use windfall_project::{DispatchResult, MAX_SONG_TICKS, PlaylistTrackId};

#[path = "recording_multitrack.rs"]
mod multitrack;
#[path = "recording_takes.rs"]
mod takes;

pub(super) trait CaptureHandle: Send {
    // Existing fake-capture fixtures report their input counts through this
    // test contract. Production take lengths use committed WAV sink counts,
    // including the minimum shared length for multitrack capture.
    #[cfg(test)]
    #[allow(dead_code)]
    fn frames(&self) -> u64;
    fn failed(&self) -> bool;
    fn finish(self: Box<Self>) -> Result<u64, String>;
}
impl CaptureHandle for Capture {
    #[cfg(test)]
    fn frames(&self) -> u64 {
        Capture::frames(self)
    }
    fn failed(&self) -> bool {
        Capture::failed(self)
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Capture::finish(*self)
    }
}
pub(super) type Sink = Box<dyn FnMut(&[f32]) -> Result<(), String> + Send>;
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct RecordedInput {
    name: String,
    source: RecordingSource,
    place: ClipPlace,
    path: PathBuf,
    writer: Arc<Mutex<Option<WavWriter>>>,
    written: Arc<AtomicU64>,
    alignment: Arc<AlignmentMetrics>,
    monitor: Option<windfall_engine::recording_monitor::MonitorTelemetry>,
    printed: bool,
}
pub(super) struct Take {
    capture: Option<Box<dyn CaptureHandle>>,
    writer: Arc<Mutex<Option<WavWriter>>>,
    path: PathBuf,
    place: ClipPlace,
    rate: u32,
    written: Arc<AtomicU64>,
    gate: Option<CaptureGate>,
    alignment: Arc<AlignmentMetrics>,
    counted_transport: Option<windfall_engine::Controller>,
    previous_region: Option<windfall_project::TickRange>,
    previous_loop: bool,
    previous_mode: windfall_ipc::PlayMode,
    loop_frames: Option<f64>,
    loop_region: Option<windfall_project::TickRange>,
    split_paths: Vec<PathBuf>,
    monitor_control: Option<windfall_engine::Controller>,
    monitor: Option<windfall_engine::recording_monitor::MonitorTelemetry>,
    inputs: Vec<RecordedInput>,
    group_name: Option<String>,
    printed: bool,
    disk_control: Option<windfall_engine::Controller>,
    installed: Arc<AtomicBool>,
}
impl Drop for Take {
    fn drop(&mut self) {
        if let Some(gate) = &self.gate {
            gate.close();
        }
        self.capture.take();
        if let Some(controller) = self.disk_control.take() {
            controller.stop_disk_taps();
        }
        if let Some(controller) = self.monitor_control.take() {
            controller.stop_input_monitor();
        }
        if let Some(gate) = self.gate.take() {
            gate.release();
        }
        lock(&self.writer).take();
        for input in &mut self.inputs {
            lock(&input.writer).take();
            let _ = fs::remove_file(&input.path);
        }
        if let Some(controller) = self.counted_transport.take() {
            controller.stop();
            let _ = controller.set_timeline_region(self.previous_region);
            controller.set_transport(windfall_ipc::TransportPatch {
                mode: Some(self.previous_mode),
                loop_song: Some(self.previous_loop),
                ..Default::default()
            });
        }
        let installed = self.installed.load(Ordering::Acquire);
        if !installed || self.loop_frames.is_some() || self.group_name.is_some() {
            let _ = fs::remove_file(&self.path);
        }
        if !installed {
            for path in &self.split_paths {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// Keep new recording, editing and reconfiguration excluded until off-guard
/// capture/preparation cleanup has completed, including every early return.
struct FinishingGuard {
    session: Session,
}
impl Drop for FinishingGuard {
    fn drop(&mut self) {
        let _owned = lock(&self.session.inner.recording);
        self.session
            .inner
            .recording_finish_cancelled
            .store(false, Ordering::Release);
        self.session
            .inner
            .recording_finishing
            .store(false, Ordering::Release);
    }
}
impl Session {
    fn recording_finish_guard(&self) -> Result<MutexGuard<'_, Option<Take>>, String> {
        let owned = lock(&self.inner.recording);
        if owned.is_some()
            || !self.inner.recording_finishing.load(Ordering::Acquire)
            || self
                .inner
                .recording_finish_cancelled
                .load(Ordering::Acquire)
        {
            return Err("Recording completion was cancelled or ownership changed.".into());
        }
        Ok(owned)
    }
    pub(super) fn recording_idle(&self) -> Result<MutexGuard<'_, Option<Take>>, String> {
        let take = lock(&self.inner.recording);
        if self.inner.update_installing.load(Ordering::Acquire) {
            return Err("Project edits are frozen for update installation".into());
        }
        if take.is_some()
            || self.inner.recording_finishing.load(Ordering::Acquire)
            || self.inner.performance_capturing.load(Ordering::Acquire)
        {
            return Err("Stop or cancel recording first.".into());
        }
        Ok(take)
    }
    pub fn recording_inputs(&self) -> Vec<RecordingInput> {
        recording_inputs()
    }
    pub fn recording_state(&self) -> RecordingState {
        let take = lock(&self.inner.recording);
        match take.as_ref() {
            Some(take) => RecordingState {
                active: true,
                frames: take.recorded_frames(),
                sample_rate: take.rate,
                start_tick: take.place.start,
                error: take
                    .capture
                    .as_ref()
                    .filter(|c| c.failed())
                    .map(|_| "Input failed. Stop to discard this take.".into()),
                alignment: Some(take.alignment.snapshot()),
                takes: take
                    .loop_frames
                    .map(|span| take_infos(take.recorded_frames(), span)),
                monitor: take.monitor.as_ref().map(|monitor| monitor.snapshot()),
                tracks: take.group_name.as_ref().map(|name| {
                    let mut tracks = vec![RecordingTrackInfo {
                        mixer_track: take.place.mixer_track.expect("armed root track"),
                        name: name.clone(),
                        frames: take.written.load(Ordering::Acquire),
                        alignment: take.alignment.snapshot(),
                        monitor: take.monitor.as_ref().map(|monitor| monitor.snapshot()),
                    }];
                    tracks.extend(take.inputs.iter().map(|input| RecordingTrackInfo {
                        mixer_track: input.place.mixer_track.expect("armed input track"),
                        name: input.name.clone(),
                        frames: input.written.load(Ordering::Acquire),
                        alignment: input.alignment.snapshot(),
                        monitor: input.monitor.as_ref().map(|monitor| monitor.snapshot()),
                    }));
                    tracks
                }),
            },
            None => RecordingState {
                active: false,
                frames: 0,
                sample_rate: 0,
                start_tick: 0,
                error: None,
                alignment: None,
                takes: None,
                monitor: None,
                tracks: None,
            },
        }
    }
    /// Open the ADC before scheduling playback and align its timestamped audio
    /// to the chosen DAC frame. Device-rate conversion runs on the writer worker.
    pub fn recording_start(
        &self,
        source: RecordingSource,
        start: u32,
        track: Option<PlaylistTrackId>,
    ) -> Result<RecordingState, String> {
        self.recording_start_core(
            source,
            start,
            track,
            |source, rate, gate, metrics, monitor, sink| {
                Ok(Box::new(Capture::start_timed(
                    source, rate, gate, metrics, monitor, sink,
                )?))
            },
        )
    }
    #[cfg(test)]
    pub(super) fn recording_start_with<F>(
        &self,
        source: RecordingSource,
        start: u32,
        track: Option<PlaylistTrackId>,
        open: F,
    ) -> Result<RecordingState, String>
    where
        F: FnOnce(RecordingSource, u32, Sink) -> Result<Box<dyn CaptureHandle>, String>,
    {
        self.recording_start_core(
            source,
            start,
            track,
            |source, rate, _gate, _metrics, _monitor, sink| open(source, rate, sink),
        )
    }
    fn recording_start_core<F>(
        &self,
        source: RecordingSource,
        start: u32,
        track: Option<PlaylistTrackId>,
        open: F,
    ) -> Result<RecordingState, String>
    where
        F: FnOnce(
            RecordingSource,
            u32,
            CaptureGate,
            Arc<AlignmentMetrics>,
            Option<windfall_engine::recording_monitor::MonitorWriter>,
            Sink,
        ) -> Result<Box<dyn CaptureHandle>, String>,
    {
        let _configuring = lock(&self.inner.configuring);
        let _saving = lock(&self.inner.save);
        let mut owned = self.recording_idle()?;
        self.input_monitor_stop();
        self.audio_logger_pause_input();
        if self.inner.exporting.load(Ordering::Acquire) {
            return Err("Wait for export before recording.".into());
        }
        let status = self.engine_status();
        if !status.running || status.sample_rate == 0 {
            return Err("Open an audio output before recording.".into());
        }
        if start >= MAX_SONG_TICKS {
            return Err("Recording start is outside the song.".into());
        }
        let punch = if source.loop_recording.is_none() {
            self.state()
                .document
                .project()
                .playlist
                .timeline
                .punch_at(start)
        } else {
            None
        };
        let start = punch.map_or(start, |region| region.start);
        let alignment = source.alignment.clone().unwrap_or_default();
        if !alignment.offset_ms.is_finite() || alignment.offset_ms.abs() > 1000.0 {
            return Err("Recording offset must be between -1000 and 1000 milliseconds.".into());
        }
        if alignment
            .input_sample_rate
            .is_some_and(|rate| !(8_000..=384_000).contains(&rate))
        {
            return Err("Recording input rate must be between 8000 and 384000 Hz.".into());
        }
        let loop_region = source.loop_recording.as_ref().map(|options| options.region);
        let loop_frames = loop_region
            .map(|region| {
                self.controller()
                    .song_range_frames(region, status.sample_rate)
            })
            .transpose()?;
        if loop_region.is_some_and(|region| region.start != start) {
            return Err("Loop recording starts at the selected region's first tick.".into());
        }
        if loop_frames
            .is_some_and(|span| !span.is_finite() || span < f64::from(status.sample_rate) * 0.25)
        {
            return Err("Choose a loop recording region lasting at least 250 milliseconds.".into());
        }
        let mut armed = self.armed_recording_routes(&source, start, track)?;
        let is_group = !armed.is_empty();
        {
            let state = self.state();
            if track.is_some_and(|id| {
                !state
                    .document
                    .project()
                    .playlist
                    .tracks
                    .iter()
                    .any(|t| t.id == id)
            }) {
                return Err("Playlist track no longer exists.".into());
            }
        }
        let folder = self.store().recordings_dir();
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut path = None;
        for _ in 0..1000 {
            let candidate = folder.join(format!(
                "Take-{}-{}.wav",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    path = Some(candidate);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        let path = path.ok_or("Could not reserve a unique recording file.")?;
        let writer = match WavWriter::create(&path, status.sample_rate, 2, WavSampleFormat::Float32)
        {
            Ok(writer) => Arc::new(Mutex::new(Some(writer))),
            Err(e) => {
                let _ = fs::remove_file(&path);
                return Err(e.to_string());
            }
        };
        let written = Arc::new(AtomicU64::new(0));
        let count_in = self.controller().transport().count_in_bars.unwrap_or(0);
        let counted_transport = (count_in > 0
            || alignment.synchronize
            || loop_region.is_some()
            || punch.is_some()
            || is_group)
            .then(|| self.controller().clone());
        let metrics = Arc::new(AlignmentMetrics::default());
        let mut take = Take {
            capture: None,
            writer: writer.clone(),
            path,
            place: ClipPlace {
                start,
                track,
                mixer_track: None,
            },
            rate: status.sample_rate,
            written: written.clone(),
            gate: None,
            alignment: metrics.clone(),
            counted_transport: counted_transport.clone(),
            previous_region: self.controller().timeline_region(),
            previous_loop: self.controller().transport().loop_song,
            previous_mode: self.controller().transport().mode,
            loop_frames,
            loop_region,
            split_paths: Vec::new(),
            monitor_control: None,
            monitor: None,
            inputs: Vec::new(),
            group_name: None,
            printed: false,
            disk_control: None,
            installed: Arc::new(AtomicBool::new(false)),
        };
        let mut capture_source = source.clone();
        if is_group {
            let primary = armed.remove(0);
            take.group_name = Some(primary.name);
            take.place = primary.place;
            capture_source = primary.source;
            take.printed = capture_source.mixer_tap.is_some();
            for route in armed {
                take.inputs.push(multitrack::reserve_input(
                    &folder,
                    route,
                    status.sample_rate,
                )?);
            }
        }
        if let Some(controller) = &counted_transport {
            controller.stop();
            controller.set_timeline_region(Some(loop_region.or(punch).unwrap_or(
                windfall_project::TickRange {
                    start,
                    end: MAX_SONG_TICKS,
                },
            )))?;
            controller.set_transport(windfall_ipc::TransportPatch {
                mode: Some(windfall_ipc::PlayMode::Song),
                loop_song: Some(loop_region.is_some()),
                ..Default::default()
            });
            controller.seek(f64::from(start));
        }
        let gate = self
            .controller()
            .recording_clock()
            .arm(counted_transport.is_some());
        take.gate = Some(gate.clone());
        let tap_settings = std::iter::once(&capture_source)
            .chain(take.inputs.iter().map(|input| &input.source))
            .filter_map(|source| {
                source.mixer_tap.clone().map(|tap| {
                    (
                        tap,
                        source
                            .alignment
                            .as_ref()
                            .map_or(0.0, |alignment| alignment.offset_ms),
                    )
                })
            })
            .collect::<Vec<_>>();
        let mut disk_readers = if tap_settings.is_empty() {
            Vec::new()
        } else {
            take.disk_control = Some(self.controller().clone());
            self.controller()
                .start_disk_taps(tap_settings, gate.clone(), status.sample_rate)?
        }
        .into_iter();
        let monitor_settings = std::iter::once(&capture_source)
            .chain(take.inputs.iter().map(|input| &input.source))
            .filter_map(|source| source.monitor.clone())
            .collect::<Vec<_>>();
        let mut monitors = if monitor_settings.is_empty() {
            Vec::new()
        } else {
            take.monitor_control = Some(self.controller().clone());
            self.controller()
                .start_input_monitors(monitor_settings, status.sample_rate)?
        }
        .into_iter();
        let monitor = capture_source
            .monitor
            .as_ref()
            .and_then(|_| monitors.next());
        if let Some(monitor) = &monitor {
            take.monitor_control = Some(self.controller().clone());
            take.monitor = Some(monitor.telemetry.clone());
        }
        let sink: Sink = Box::new(move |block| {
            lock(&writer)
                .as_mut()
                .ok_or("Recording writer closed.")?
                .write(block)
                .map_err(|error| error.to_string())?;
            written.fetch_add((block.len() / 2) as u64, Ordering::Release);
            Ok(())
        });
        take.capture = Some(if is_group {
            let disk = capture_source
                .mixer_tap
                .as_ref()
                .and_then(|_| disk_readers.next());
            let mut routes = vec![CaptureRoute {
                source: capture_source,
                metrics,
                monitor,
                sink,
                disk,
            }];
            for input in &mut take.inputs {
                let monitor = input.source.monitor.as_ref().and_then(|_| monitors.next());
                input.monitor = monitor.as_ref().map(|monitor| monitor.telemetry.clone());
                let writer = input.writer.clone();
                let written = input.written.clone();
                let disk = input
                    .source
                    .mixer_tap
                    .as_ref()
                    .and_then(|_| disk_readers.next());
                routes.push(CaptureRoute {
                    source: input.source.clone(),
                    metrics: input.alignment.clone(),
                    monitor,
                    disk,
                    sink: Box::new(move |block| {
                        lock(&writer)
                            .as_mut()
                            .ok_or("Track recording writer closed.")?
                            .write(block)
                            .map_err(|error| error.to_string())?;
                        written.fetch_add((block.len() / 2) as u64, Ordering::Release);
                        Ok(())
                    }),
                });
            }
            Box::new(Capture::start_group(routes, status.sample_rate, gate)?)
        } else {
            open(
                capture_source,
                status.sample_rate,
                gate,
                metrics,
                monitor,
                sink,
            )?
        });
        if let Some(controller) = &counted_transport {
            controller.play_count_in(count_in);
        }
        *owned = Some(take);
        drop(owned);
        Ok(self.recording_state())
    }
    pub fn recording_cancel(&self) {
        let (finishing, take) = {
            let mut owned = lock(&self.inner.recording);
            let finishing = if owned.is_some() {
                // Dropping an active take joins capture and stops shared
                // monitor/tap/transport resources. Exclude replacement until
                // all of that off-lock cleanup is finished.
                self.inner
                    .recording_finish_cancelled
                    .store(false, Ordering::Release);
                self.inner
                    .recording_finishing
                    .store(true, Ordering::Release);
                Some(FinishingGuard {
                    session: self.clone(),
                })
            } else if self.inner.recording_finishing.load(Ordering::Acquire) {
                // Do not release finishing exclusion: its owner still has files,
                // capture resources and possibly prepared native units to retire.
                self.inner
                    .recording_finish_cancelled
                    .store(true, Ordering::Release);
                None
            } else {
                None
            };
            (finishing, owned.take())
        };
        #[cfg(test)]
        if finishing.is_some() {
            self.pause("recording:cancel-cleanup");
        }
        drop(take);
        drop(finishing);
    }
    pub fn recording_stop(&self) -> Result<DispatchResult, String> {
        self.recording_stop_with_selection(None)
    }
    /// None keeps all passes; an explicit list keeps exactly those zero-based
    /// takes, including the final partial pass when selected.
    pub fn recording_stop_with_selection(
        &self,
        selection: Option<RecordingTakeSelection>,
    ) -> Result<DispatchResult, String> {
        let mut owned = lock(&self.inner.recording);
        let current = owned.as_ref().ok_or("No recording is active.")?;
        if let Some(RecordingTakeSelection::Only { indices }) = &selection {
            let count = current.loop_frames.map_or(1, |span| {
                take_infos(current.recorded_frames(), span).len() as u32
            });
            if indices.is_empty() || indices.iter().any(|index| *index >= count) {
                return Err("Select at least one existing recording take to keep.".into());
            }
        }
        if let Some(RecordingTakeSelection::Except { indices }) = &selection {
            let count = current.loop_frames.map_or(1, |span| {
                take_infos(current.recorded_frames(), span).len() as u32
            });
            if (0..count).all(|index| indices.contains(&index)) {
                return Err("Select a take to keep, or use Discard to cancel recording.".into());
            }
        }
        self.inner
            .recording_finish_cancelled
            .store(false, Ordering::Release);
        self.inner
            .recording_finishing
            .store(true, Ordering::Release);
        let _finishing = FinishingGuard {
            session: self.clone(),
        };
        let mut take = owned.take().expect("validated owned take");
        drop(owned);
        if let Some(gate) = &take.gate {
            gate.close();
        }
        if let Some(controller) = &take.counted_transport {
            controller.stop();
        }
        // Joining capture, finalizing WAVs and preparing native units must run
        // without recording/State guards. FinishingGuard retains admission.
        let _captured = take
            .capture
            .take()
            .ok_or("Capture is unavailable.")?
            .finish()?;
        if take.recorded_frames() == 0 {
            return Err("The take contains no audio.".into());
        }
        lock(&take.writer)
            .take()
            .ok_or("Recording writer closed.")?
            .finalize()
            .map_err(|e| e.to_string())?;
        for input in &mut take.inputs {
            lock(&input.writer)
                .take()
                .ok_or("Track writer closed.")?
                .finalize()
                .map_err(|error| error.to_string())?;
        }
        let result = if take.group_name.is_some() {
            self.attach_multitrack_take(&mut take, selection)?
        } else if take.loop_frames.is_some() {
            self.attach_loop_takes(&mut take, selection)?
        } else {
            self.attach_audio_clip_from_file(
                &crate::paths::display(&take.path),
                take.place,
                take.installed.clone(),
            )?
        };
        // Completed source stays on disk for undo/redo and future project saves.
        if take.loop_frames.is_none() && take.group_name.is_none() {
            take.path = PathBuf::new();
            for input in &mut take.inputs {
                input.path = PathBuf::new();
            }
        }
        take.split_paths.clear();
        Ok(result)
    }
}
impl Take {
    fn recorded_frames(&self) -> u64 {
        std::iter::once(self.written.load(Ordering::Acquire))
            .chain(
                self.inputs
                    .iter()
                    .map(|input| input.written.load(Ordering::Acquire)),
            )
            .min()
            .unwrap_or(0)
    }
}

fn take_boundary(index: u32, span: f64) -> u64 {
    (f64::from(index) * span - 1e-6).ceil().max(0.0) as u64
}
fn take_infos(frames: u64, span: f64) -> Vec<RecordingTakeInfo> {
    let count = ((frames as f64 / span).ceil() as u32).min(256);
    (0..count)
        .map(|index| {
            let start = take_boundary(index, span);
            let end = take_boundary(index + 1, span);
            RecordingTakeInfo {
                index,
                frames: frames.min(end).saturating_sub(start),
                complete: frames >= end,
            }
        })
        .collect()
}
