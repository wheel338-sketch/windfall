//! The audio logger: a bounded history of what was played, kept in the
//! background, from which an unplanned take can be put on the playlist.
//!
//! The engine keeps the histories of the master output and of the logged
//! mixer tracks (`windfall_engine::audio_logger`). This module keeps the
//! history of the logged hardware input, remembers the settings, keeps the
//! logger alive across device changes, and turns the end of a history into
//! an audio clip through the ordinary checked import.
//!
//! # Locks
//!
//! `audio_logger` is taken after `configuring` and after `recording`, and
//! nothing but the engine's controller is taken under it: never `state`,
//! `settings` or `save`. Devices are opened and closed under it, which can
//! take a moment, so nothing on the realtime feed's thread takes it.
//!
//! # The hardware input
//!
//! The input is captured at its own rate: the output's where the device can
//! do it, and the nearest it can do otherwise. It yields to a recording
//! take. The take closes it before opening its own capture, and
//! [`Session::audio_logger_maintain`] opens it again, onto the same
//! history, once the take is over. A capture that fails leaves its history
//! readable and an error to show; it is not retried until the logger is
//! configured again.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windfall_codec::{WavSampleFormat, WavWriter};
use windfall_engine::audio_logger::{
    AudioLogger, HistoryRing, LoggedAudio, LoggerSpec, MAX_SOURCE_BYTES, QUEUE_SECONDS, input_rate,
};
use windfall_engine::recording::Capture;
use windfall_ipc::{
    AUDIO_LOGGER_MAX_SECONDS, AUDIO_LOGGER_MAX_TRACKS, AUDIO_LOGGER_MIN_SECONDS,
    AudioLoggerDumpRequest, AudioLoggerInput, AudioLoggerSettings, AudioLoggerSource,
    AudioLoggerSourceStatus, AudioLoggerState, RecordingSource,
};
use windfall_project::{DispatchResult, MAX_SONG_TICKS, TrackId};

use super::recording::{CaptureHandle, Sink};
use super::{ClipPlace, Session};
use crate::sync::lock;

/// How often the logger is looked after: a lost stream is taken up again
/// and an input that made way for a recording take is reopened.
const WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// The rate an input is asked for while no audio output is open.
const FALLBACK_RATE: u32 = 48_000;

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// Opens the logged hardware input. The app's is [`NativeInput`]; tests
/// supply one that opens no device.
pub(super) trait InputBackend: Send + Sync {
    /// The rate the input will be captured at, given the output's rate.
    fn rate(&self, input: &AudioLoggerInput, preferred: u32) -> Result<u32, String>;
    /// Starts capturing at `rate`. `sink` is handed interleaved stereo on a
    /// worker thread, never on the driver's callback.
    fn open(
        &self,
        input: &AudioLoggerInput,
        rate: u32,
        sink: Sink,
    ) -> Result<Box<dyn CaptureHandle>, String>;
}

struct NativeInput;

fn recording_source(input: &AudioLoggerInput) -> RecordingSource {
    RecordingSource {
        host: input.host.clone(),
        device: input.device.clone(),
        left: input.left,
        right: input.right,
        alignment: None,
        loop_recording: None,
        monitor: None,
        armed_tracks: None,
        mixer_tap: None,
    }
}

impl InputBackend for NativeInput {
    fn rate(&self, input: &AudioLoggerInput, preferred: u32) -> Result<u32, String> {
        input_rate(&recording_source(input), preferred)
    }

    fn open(
        &self,
        input: &AudioLoggerInput,
        rate: u32,
        sink: Sink,
    ) -> Result<Box<dyn CaptureHandle>, String> {
        Ok(Box::new(Capture::start(
            recording_source(input),
            rate,
            sink,
        )?))
    }
}

/// The logged hardware input: its history, and the capture that feeds it
/// while there is one.
struct InputLog {
    source: AudioLoggerInput,
    history: Arc<Mutex<HistoryRing>>,
    /// `None` once the capture failed or made way for a recording take.
    capture: Option<Box<dyn CaptureHandle>>,
}

pub(super) struct Logger {
    backend: Arc<dyn InputBackend>,
    /// The engine's side: the master and track histories.
    output: Option<AudioLogger>,
    output_error: Option<String>,
    input: Option<InputLog>,
    input_error: Option<String>,
    /// The input was closed for a recording take and is to be reopened.
    input_paused: bool,
    effect_generation: Option<u64>,
}

impl Default for Logger {
    fn default() -> Self {
        Self {
            backend: Arc::new(NativeInput),
            output: None,
            output_error: None,
            input: None,
            input_error: None,
            input_paused: false,
            effect_generation: None,
        }
    }
}

/// Deletes a file that was reserved and never became part of the project.
struct Reserved(PathBuf);

impl Drop for Reserved {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.0);
        }
    }
}

/// The settings as they can be run: a length inside the bounds and at most
/// four tracks, each once. A settings file is never trusted to hold that.
fn usable(mut settings: AudioLoggerSettings) -> AudioLoggerSettings {
    settings.seconds = settings
        .seconds
        .clamp(AUDIO_LOGGER_MIN_SECONDS, AUDIO_LOGGER_MAX_SECONDS);
    let mut tracks: Vec<TrackId> = Vec::new();
    for track in settings.tracks {
        if !tracks.contains(&track) && tracks.len() < AUDIO_LOGGER_MAX_TRACKS {
            tracks.push(track);
        }
    }
    settings.tracks = tracks;
    let mut effects = Vec::new();
    for effect in settings.effects {
        if !effects.contains(&effect) && effects.len() < AUDIO_LOGGER_MAX_TRACKS {
            effects.push(effect);
        }
    }
    settings.effects = effects;
    settings
}

fn check(settings: &AudioLoggerSettings) -> Result<(), String> {
    if !(AUDIO_LOGGER_MIN_SECONDS..=AUDIO_LOGGER_MAX_SECONDS).contains(&settings.seconds) {
        return Err(format!(
            "The audio logger keeps {AUDIO_LOGGER_MIN_SECONDS} to {AUDIO_LOGGER_MAX_SECONDS} seconds."
        ));
    }
    if &usable(settings.clone()) != settings {
        return Err(format!(
            "The audio logger follows at most {AUDIO_LOGGER_MAX_TRACKS} mixer tracks, each once."
        ));
    }
    Ok(())
}

fn logs_output(settings: &AudioLoggerSettings) -> bool {
    settings.enabled
        && (settings.master || !settings.tracks.is_empty() || !settings.effects.is_empty())
}

fn file_label(source: AudioLoggerSource) -> String {
    match source {
        AudioLoggerSource::Master => "master".to_owned(),
        AudioLoggerSource::Track { track } => format!("track-{}", track.0),
        AudioLoggerSource::Input => "input".to_owned(),
        AudioLoggerSource::Effect { effect } => format!("effect-{}", effect.0),
    }
}

impl Session {
    pub(super) fn audio_logger_project_replaced(&self) {
        let _configuring = lock(&self.inner.configuring);
        let mut settings = self.audio_logger_settings();
        if settings.effects.is_empty() {
            return;
        }
        settings.effects.clear();
        self.store()
            .update(|stored| stored.audio_logger.effects.clear());
        let generation = self.state().generation;
        self.start_output(&mut self.audio_logger(), &settings, generation);
    }
    fn audio_logger(&self) -> MutexGuard<'_, Logger> {
        lock(&self.inner.audio_logger)
    }

    fn audio_logger_settings(&self) -> AudioLoggerSettings {
        usable(self.store().settings().audio_logger.clone())
    }

    /// What the logger holds right now. Reads no device.
    pub fn audio_logger_state(&self) -> AudioLoggerState {
        let settings = self.audio_logger_settings();
        let names: Vec<(TrackId, String)> = {
            let state = self.state();
            let tracks = state.document.project().mixer.tracks.iter();
            tracks.map(|track| (track.id, track.name.clone())).collect()
        };
        let effect_names: Vec<_> = self
            .state()
            .document
            .project()
            .mixer
            .tracks
            .iter()
            .flat_map(|track| {
                track.effects.iter().map(move |slot| {
                    (
                        slot.id,
                        format!("{} · {}", track.name, slot.params.kind().name()),
                    )
                })
            })
            .collect();
        let generation = self.state().generation;
        let logger = self.audio_logger();
        let running = logger
            .output
            .as_ref()
            .is_some_and(|output| !output.detached());
        let label = |source: AudioLoggerSource| match source {
            AudioLoggerSource::Master => "Master output".to_owned(),
            AudioLoggerSource::Effect { effect } => effect_names
                .iter()
                .find(|(id, _)| *id == effect)
                .map_or_else(
                    || format!("Effect {} (not in this project)", effect.0),
                    |(_, name)| format!("Slot: {name}"),
                ),
            AudioLoggerSource::Track { track } => {
                names.iter().find(|(id, _)| *id == track).map_or_else(
                    || format!("Mixer track {} (not in this project)", track.0),
                    |(_, name)| format!("Mixer track: {name}"),
                )
            }
            AudioLoggerSource::Input => settings.input.as_ref().map_or_else(
                || "Hardware input".to_owned(),
                |input| format!("Input: {}", input.device),
            ),
        };
        let empty = |source: AudioLoggerSource, error: Option<String>| AudioLoggerSourceStatus {
            source,
            label: label(source),
            sample_rate: 0,
            capacity_seconds: 0.0,
            available_seconds: 0.0,
            available_frames: 0,
            dropped_frames: 0,
            memory_bytes: 0,
            active: false,
            error,
        };
        let mut sources = Vec::new();
        match &logger.output {
            Some(output) => {
                for stats in output.stats() {
                    let rate = f64::from(stats.sample_rate.max(1));
                    let present = match stats.source {
                        AudioLoggerSource::Effect { effect } => {
                            logger.effect_generation == Some(generation)
                                && effect_names.iter().any(|(id, _)| *id == effect)
                        }
                        AudioLoggerSource::Track { track } => {
                            names.iter().any(|(id, _)| *id == track)
                        }
                        _ => true,
                    };
                    sources.push(AudioLoggerSourceStatus {
                        source: stats.source,
                        label: label(stats.source),
                        sample_rate: stats.sample_rate,
                        capacity_seconds: stats.capacity_frames as f64 / rate,
                        available_seconds: stats.available_frames as f64 / rate,
                        available_frames: stats.available_frames,
                        dropped_frames: stats.dropped_frames,
                        memory_bytes: stats.memory_bytes,
                        active: running && present,
                        error: None,
                    });
                }
            }
            None if logs_output(&settings) => {
                // Asked for and not running: the rows still say what is
                // waiting, with nothing in them.
                if settings.master {
                    sources.push(empty(AudioLoggerSource::Master, None));
                }
                for &track in &settings.tracks {
                    sources.push(empty(AudioLoggerSource::Track { track }, None));
                }
                for &effect in &settings.effects {
                    sources.push(empty(AudioLoggerSource::Effect { effect }, None));
                }
            }
            None => {}
        }
        match &logger.input {
            Some(input) => {
                let history = lock(&input.history);
                let rate = history.sample_rate();
                sources.push(AudioLoggerSourceStatus {
                    source: AudioLoggerSource::Input,
                    label: format!("Input: {}", input.source.device),
                    sample_rate: rate,
                    capacity_seconds: history.capacity() as f64 / f64::from(rate.max(1)),
                    available_seconds: history.len() as f64 / f64::from(rate.max(1)),
                    available_frames: history.len() as u64,
                    // The capture stops on its first lost frame instead of
                    // dropping quietly, so a gap is always an error.
                    dropped_frames: 0,
                    memory_bytes: history.memory_bytes()
                        + u64::from(rate) * u64::from(QUEUE_SECONDS) * 8,
                    active: input.capture.is_some(),
                    error: logger.input_error.clone().or_else(|| {
                        logger.input_paused.then(|| {
                            "Paused while a recording take uses the audio input.".to_owned()
                        })
                    }),
                });
            }
            None if settings.enabled && settings.input.is_some() => {
                sources.push(empty(AudioLoggerSource::Input, logger.input_error.clone()));
            }
            None => {}
        }
        AudioLoggerState {
            memory_bytes: sources.iter().map(|source| source.memory_bytes).sum(),
            max_source_bytes: MAX_SOURCE_BYTES,
            min_seconds: AUDIO_LOGGER_MIN_SECONDS,
            max_seconds: AUDIO_LOGGER_MAX_SECONDS,
            max_tracks: AUDIO_LOGGER_MAX_TRACKS as u32,
            running,
            error: logger.output_error.clone(),
            sources,
            settings,
        }
    }

    /// Remembers new logger settings and runs the logger by them. A part of
    /// the logger whose settings did not change keeps its history; a part
    /// whose length or sources changed starts over, empty.
    ///
    /// Refused only for settings outside the bounds. A device that cannot
    /// be opened is not an error of this call: the returned state says so.
    pub fn audio_logger_configure(
        &self,
        settings: AudioLoggerSettings,
    ) -> Result<AudioLoggerState, String> {
        check(&settings)?;
        {
            let _configuring = lock(&self.inner.configuring);
            let before = self.audio_logger_settings();
            self.store()
                .update(|stored| stored.audio_logger = settings.clone());
            // Held so that a take cannot start while the input is reopened.
            let recording = self.recording_idle();
            let generation = self.state().generation;
            let mut logger = self.audio_logger();
            let output = |settings: &AudioLoggerSettings| {
                (
                    logs_output(settings),
                    settings.seconds,
                    settings.master,
                    settings.tracks.clone(),
                    settings.effects.clone(),
                )
            };
            if output(&before) != output(&settings) || logger.output.is_none() {
                self.start_output(&mut logger, &settings, generation);
            }
            let input = |settings: &AudioLoggerSettings| {
                (settings.enabled, settings.seconds, settings.input.clone())
            };
            let wanted = settings.enabled && settings.input.is_some();
            if input(&before) != input(&settings) || (wanted && logger.input.is_none()) {
                self.open_input(&mut logger, &settings, false, recording.is_ok());
            }
        }
        Ok(self.audio_logger_state())
    }

    /// Starts the logger with the remembered settings, as when the app
    /// starts. Every history starts empty.
    pub fn audio_logger_restore(&self) {
        let _configuring = lock(&self.inner.configuring);
        self.restart_audio_logger();
    }

    /// The audio output was reopened: its old stream and the tap in it are
    /// gone, and the new one may run at another rate. Every history starts
    /// over at the rate of what is open now. The caller holds `configuring`.
    pub(super) fn audio_logger_reconfigured(&self) {
        self.restart_audio_logger();
    }

    fn restart_audio_logger(&self) {
        let settings = self.audio_logger_settings();
        let recording = self.recording_idle();
        let generation = self.state().generation;
        let mut logger = self.audio_logger();
        self.start_output(&mut logger, &settings, generation);
        self.open_input(&mut logger, &settings, false, recording.is_ok());
    }

    /// Closes the logged input so that a recording take can open the device.
    /// Its history is kept, and [`audio_logger_maintain`] reopens it after
    /// the take. The caller holds `recording`.
    ///
    /// [`audio_logger_maintain`]: Self::audio_logger_maintain
    pub(super) fn audio_logger_pause_input(&self) {
        let mut logger = self.audio_logger();
        let closed = logger.input.as_mut().and_then(|input| input.capture.take());
        if let Some(capture) = closed {
            // Joins the capture's worker, which closes the device.
            drop(capture);
            logger.input_paused = true;
        }
    }

    /// Looks after a running logger: takes its tap to a stream that came
    /// back, notes an input that failed, and reopens an input that made way
    /// for a recording take. Called twice a second by the thread
    /// [`spawn_audio_logger`](Self::spawn_audio_logger) starts. Skips a
    /// round while the audio device is being reopened.
    pub fn audio_logger_maintain(&self) {
        let _configuring = match self.inner.configuring.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        let settings = self.audio_logger_settings();
        if !settings.enabled {
            return;
        }
        let recording = self.recording_idle();
        let generation = self.state().generation;
        let mut logger = self.audio_logger();
        self.heal_output(&mut logger, &settings, generation);
        let failed = logger
            .input
            .as_mut()
            .filter(|input| {
                input
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.failed())
            })
            .and_then(|input| input.capture.take());
        if let Some(capture) = failed {
            logger.input_error = Some(capture.finish().err().unwrap_or_else(|| {
                "The logged audio input stopped. Its history so far is kept.".to_owned()
            }));
        }
        if logger.input_paused && recording.is_ok() {
            self.open_input(&mut logger, &settings, true, true);
        }
    }

    /// Starts the logger with the remembered settings and the thread that
    /// looks after it. The thread stops when the last handle to the session
    /// is dropped. Opening the logged input happens on that thread, so a
    /// slow driver does not hold up the window.
    pub fn spawn_audio_logger(&self) -> io::Result<JoinHandle<()>> {
        let session = self.downgrade();
        thread::Builder::new()
            .name("windfall-audio-logger-watch".to_owned())
            .spawn(move || {
                if let Some(session) = session.upgrade() {
                    session.audio_logger_restore();
                }
                loop {
                    thread::sleep(WATCH_INTERVAL);
                    let Some(session) = session.upgrade() else {
                        break;
                    };
                    session.audio_logger_maintain();
                }
            })
    }

    /// Starts the engine's side from scratch, or stops it. The old logger
    /// is dropped only after the new tap has taken the old one's place.
    fn start_output(&self, logger: &mut Logger, settings: &AudioLoggerSettings, generation: u64) {
        logger.effect_generation = (!settings.effects.is_empty()).then_some(generation);
        logger.output_error = None;
        if !logs_output(settings) {
            self.controller().stop_audio_logger();
            logger.output = None;
            return;
        }
        let spec = LoggerSpec {
            seconds: settings.seconds,
            master: settings.master,
            tracks: settings.tracks.clone(),
            effects: settings.effects.clone(),
            worker: true,
        };
        match self.controller().start_audio_logger(&spec) {
            Ok(output) => logger.output = Some(output),
            Err(error) => {
                logger.output = None;
                logger.output_error = Some(error);
            }
        }
    }

    /// Takes a logger whose stream went away to the stream that is open
    /// now: with its history if the rate is the same, and from scratch if
    /// it is not. With no stream open there is nothing to do but wait.
    fn heal_output(&self, logger: &mut Logger, settings: &AudioLoggerSettings, generation: u64) {
        if !logs_output(settings) {
            return;
        }
        let Some(rate) = self.controller().audio_logger_rate() else {
            return;
        };
        match &logger.output {
            Some(output) if !output.detached() => {}
            Some(output) if output.sample_rate() == rate => {
                logger.output_error = self.controller().reattach_audio_logger(output).err();
            }
            _ => self.start_output(logger, settings, generation),
        }
    }

    /// Closes the logged input and, if one is asked for and `open` allows,
    /// opens it again. `keep` continues the history that is there when the
    /// rate and length still fit; otherwise the history starts empty.
    fn open_input(
        &self,
        logger: &mut Logger,
        settings: &AudioLoggerSettings,
        keep: bool,
        open: bool,
    ) {
        // The device has to be closed before it can be opened again.
        let previous = logger.input.take().map(|mut input| {
            drop(input.capture.take());
            input.history
        });
        logger.input_error = None;
        logger.input_paused = false;
        let Some(source) = settings.input.clone().filter(|_| settings.enabled) else {
            return;
        };
        if !open {
            // A take is using the device. Reopen when it is over.
            logger.input_paused = true;
            let kept = previous.filter(|_| keep);
            logger.input = kept.map(|history| InputLog {
                source,
                history,
                capture: None,
            });
            return;
        }
        let preferred = self
            .controller()
            .audio_logger_rate()
            .unwrap_or(FALLBACK_RATE);
        let rate = match logger.backend.rate(&source, preferred) {
            Ok(rate) => rate,
            Err(error) => {
                logger.input_error = Some(error);
                return;
            }
        };
        let fits = |history: &Arc<Mutex<HistoryRing>>| {
            let history = lock(history);
            history.sample_rate() == rate
                && history.capacity() == HistoryRing::capacity_for(rate, settings.seconds)
        };
        let history = match previous.filter(|history| keep && fits(history)) {
            Some(history) => history,
            None => match HistoryRing::new(rate, settings.seconds) {
                Ok(history) => Arc::new(Mutex::new(history)),
                Err(error) => {
                    logger.input_error = Some(error);
                    return;
                }
            },
        };
        let fed = history.clone();
        let sink: Sink = Box::new(move |block| {
            lock(&fed).push_interleaved(block);
            Ok(())
        });
        let capture = match logger.backend.open(&source, rate, sink) {
            Ok(capture) => Some(capture),
            Err(error) => {
                logger.input_error = Some(error);
                None
            }
        };
        logger.input = Some(InputLog {
            source,
            history,
            capture,
        });
    }

    /// Puts the end of one history on the playlist as an audio clip, as one
    /// undo step. The audio is written as a 32-bit float WAV into the
    /// recordings folder and attached through the ordinary checked import,
    /// so undo, redo, save and reopen treat it like any other clip.
    ///
    /// Slow: it copies and writes up to the whole history. The logger keeps
    /// running meanwhile. Refused while a recording take is active or being
    /// finished, for a history with nothing in it, and when another project
    /// was opened before the clip could be added; whatever was written is
    /// then deleted and the project is left as it was.
    pub fn audio_logger_dump(
        &self,
        request: AudioLoggerDumpRequest,
    ) -> Result<DispatchResult, String> {
        if request
            .seconds
            .is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0)
        {
            return Err("Choose how many seconds of logged audio to keep.".into());
        }
        // The document the clip is for. Everything below gives up if
        // another has taken its place by the time the clip is added.
        let generation = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if request.track.is_some_and(|id| {
                let mut tracks = state.document.project().playlist.tracks.iter();
                !tracks.any(|track| track.id == id)
            }) {
                return Err("Playlist track no longer exists.".into());
            }
            state.generation
        };
        let start = match request.start {
            Some(start) => start,
            None => {
                let tick = self.controller().playhead_tick();
                tick.max(0.0).min(f64::from(MAX_SONG_TICKS)) as u32
            }
        };
        if start >= MAX_SONG_TICKS {
            return Err("The clip start is outside the song.".into());
        }
        let audio = self.audio_logger_tail(request.source, request.seconds)?;
        if audio.samples.is_empty() {
            return Err(match request.source {
                AudioLoggerSource::Master => {
                    "The audio logger has not heard anything from the master output yet."
                }
                AudioLoggerSource::Track { .. } => {
                    "The audio logger has not heard anything from that mixer track yet."
                }
                AudioLoggerSource::Effect { .. } => "No audio has reached that effect slot yet. Enable its recorder and play audio through the chain.",
                AudioLoggerSource::Input => {
                    "The audio logger has not heard anything from the audio input yet."
                }
            }
            .into());
        }

        let folder = self.store().recordings_dir();
        fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create the recordings folder: {error}"))?;
        let mut reserved = None;
        for _ in 0..1000 {
            let candidate = folder.join(format!(
                "Logged-{}-{}-{}.wav",
                file_label(request.source),
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    reserved = Some(Reserved(candidate));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!("Could not create the logged audio file: {error}"));
                }
            }
        }
        let mut reserved = reserved.ok_or("Could not reserve a file for the logged audio.")?;
        let mut writer =
            WavWriter::create(&reserved.0, audio.sample_rate, 2, WavSampleFormat::Float32)
                .map_err(|error| error.to_string())?;
        writer
            .write(&audio.samples)
            .map_err(|error| error.to_string())?;
        writer.finalize().map_err(|error| error.to_string())?;
        drop(audio);
        #[cfg(test)]
        self.pause("audio-logger:written");
        let result = self.add_audio_clip_from_file_in(
            &crate::paths::display(&reserved.0),
            ClipPlace {
                track: request.track,
                start,
                mixer_track: None,
            },
            generation,
        )?;
        // The project now refers to the file: undo, redo and saving need it.
        reserved.0 = PathBuf::new();
        Ok(result)
    }

    /// Copies the newest `seconds` of a history, or all of it.
    fn audio_logger_tail(
        &self,
        source: AudioLoggerSource,
        seconds: Option<f64>,
    ) -> Result<LoggedAudio, String> {
        let frames = |rate: u32| seconds.map(|seconds| (seconds * f64::from(rate)).round() as u64);
        let generation = self.state().generation;
        let logger = self.audio_logger();
        if matches!(source, AudioLoggerSource::Effect { .. })
            && logger.effect_generation != Some(generation)
        {
            return Err(
                "The slot recording belongs to an earlier project. Start a new recording.".into(),
            );
        }
        match source {
            AudioLoggerSource::Input => {
                let input = logger
                    .input
                    .as_ref()
                    .ok_or("The audio logger is not logging an audio input.")?;
                let history = lock(&input.history);
                Ok(LoggedAudio {
                    sample_rate: history.sample_rate(),
                    samples: history.tail(frames(history.sample_rate()))?,
                })
            }
            source => {
                let output = logger
                    .output
                    .as_ref()
                    .ok_or("The audio logger is not running.")?;
                output.tail(source, frames(output.sample_rate()))
            }
        }
    }

    /// Replaces what opens the logged input, for tests with no device.
    #[cfg(test)]
    pub(super) fn audio_logger_set_backend(&self, backend: Arc<dyn InputBackend>) {
        self.audio_logger().backend = backend;
    }
}
