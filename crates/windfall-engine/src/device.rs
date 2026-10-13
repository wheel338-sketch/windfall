//! The audio device: a cpal output stream fed by a [`Processor`].
//!
//! A thread the engine owns opens the stream, watches it and reopens it.
//! [`Supervisor`] is that thread's whole job with the device and the clock
//! handed in from outside, so it can be tested without either.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, ErrorKind, FromSample, I24, SampleFormat, SizedSample, StreamConfig,
    SupportedBufferSize, SupportedStreamConfig, U24,
};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use windfall_ipc::{AudioDevice, AudioHost, AudioSettings, EngineStatus};

use crate::controller::Controller;
use crate::mixer::{Frame, MAX_BLOCK};
use crate::processor::Processor;
use crate::shared::Shared;

/// How long a driver gets to bring a stream up before opening counts as
/// failed. Not every driver honors it.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a stream must have been open before it is reopened after a
/// device error. A device that fails the moment it opens is tried again no
/// sooner than this, so it cannot spin the device thread.
const RECOVERY_INTERVAL: Duration = Duration::from_secs(1);

/// Wait after the first failed try at reopening a lost device. Every
/// further failure doubles the wait, up to [`MAX_RETRY_DELAY`].
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);

/// Longest wait between two tries at reopening a lost device. A device that
/// comes back is therefore picked up within this time.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(8);

/// How often the device thread looks at a running stream for an error. The
/// stream's own thread may not wake it: all that thread is allowed to do is
/// leave the error where this one finds it.
const FAULT_POLL: Duration = Duration::from_millis(100);

/// Errors a stream can have waiting for the device thread to collect.
const FAULT_CAPACITY: usize = 64;

/// Device buffers per stretch of time that [`Coverage`] judges as a whole.
const COVERAGE_WINDOW: u32 = 32;

/// Sample rates offered for a device when the driver gives a range.
const STANDARD_RATES: [u32; 13] = [
    8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000,
    352_800, 384_000,
];

/// The running audio engine: an output device stream and the processor that
/// feeds it.
///
/// The stream lives on a thread the engine owns, because on some platforms a
/// stream must stay on the thread that opened it. The engine itself can be
/// shared freely between threads. Dropping it closes the stream.
///
/// When the device goes away while the stream runs, as when an interface is
/// unplugged, playback stops and the engine keeps trying to open the same
/// settings again, first at once and then after waits that grow from one
/// second to eight. Until that works, or [`Engine::reconfigure`] is called,
/// [`Engine::status`] has `running` false and an `error` that ends in
/// "reconnecting".
pub struct Engine {
    controller: Controller,
    status: Arc<Mutex<EngineStatus>>,
    requests: Mutex<Sender<Request>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    /// Close the stream and open one with these settings. The sender is
    /// signalled when the attempt is over.
    Open(AudioSettings, Sender<()>),
    Suspend(Sender<Result<(), String>>),
    Close,
}

impl Engine {
    /// Opens the requested output, or the system default where the settings
    /// leave something out, and starts the stream.
    ///
    /// This never fails and never panics. When no stream could be opened,
    /// [`Engine::status`] has `running` false and `error` says why, and the
    /// controller still accepts every call.
    pub fn start(settings: &AudioSettings) -> Engine {
        let controller = Controller::new();
        let status = Arc::new(Mutex::new(stopped(
            settings,
            "the audio engine is starting",
        )));
        let (requests, inbox) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("windfall-audio-device".to_owned())
            .spawn({
                let controller = controller.clone();
                let status = status.clone();
                move || device_thread(inbox, controller, status)
            })
            .ok();
        let engine = Engine {
            controller,
            status,
            requests: Mutex::new(requests),
            thread: Mutex::new(thread),
        };
        engine.reconfigure(settings);
        engine
    }

    /// Closes the stream and opens one with new settings. Returns once the
    /// attempt is over; [`Engine::status`] tells how it went.
    ///
    /// The project, transport settings and playhead carry over. If the
    /// transport was playing and the new stream opened, playback carries on
    /// from the playhead, with the notes that were sounding cut off. If no
    /// stream could be opened, playback stops and the playhead returns to
    /// where it started. A failed attempt is not repeated: the engine only
    /// retries by itself for a device that was running and went away.
    pub fn reconfigure(&self, settings: &AudioSettings) {
        if let Err(error) = self
            .controller
            .set_audio_threads(settings.processing_threads.map(usize::from))
        {
            *lock(&self.status) = stopped(settings, error);
            return;
        }
        let (done, wait) = mpsc::channel();
        let sent = lock(&self.requests)
            .send(Request::Open(settings.clone(), done))
            .is_ok();
        if !sent || wait.recv().is_err() {
            *lock(&self.status) = stopped(settings, "the audio device thread is not running");
        }
    }

    /// Control-side installer shutdown. Acknowledges only after callback owner
    /// and stream destruction, and cancels automatic device recovery.
    pub fn suspend(&self) -> Result<(), String> {
        let (done, wait) = mpsc::channel();
        lock(&self.requests).send(Request::Suspend(done)).map_err(|_| "Audio device owner is unavailable".to_owned())?;
        wait.recv().map_err(|_| "Audio shutdown acknowledgement was lost".to_owned())?
    }

    /// What is open right now. `buffer_frames` is the size of the buffers
    /// the device actually asks for, which can differ from the size
    /// requested: WASAPI shared mode, for one, always uses the device period.
    pub fn status(&self) -> EngineStatus {
        let mut status = lock(&self.status).clone();
        let delivered = self.controller.shared().buffer_frames();
        if status.running && delivered > 0 {
            status.buffer_frames = delivered;
            status.latency_ms = latency_ms(delivered, status.sample_rate);
        }
        status.latency_frames = self.controller.latency_frames();
        status
    }

    /// Lists every audio host on this machine with its output devices.
    pub fn devices() -> Vec<AudioHost> {
        catch_unwind(list_hosts).unwrap_or_default()
    }

    /// A handle for driving the engine. It stays valid across
    /// [`Engine::reconfigure`].
    pub fn controller(&self) -> Controller {
        self.controller.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = lock(&self.requests).send(Request::Close);
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not take the engine's bookkeeping with it.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn latency_ms(frames: u32, sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        0.0
    } else {
        frames as f32 * 1000.0 / sample_rate as f32
    }
}

/// The status of an engine with no stream, echoing what was asked for.
fn stopped(settings: &AudioSettings, error: impl Into<String>) -> EngineStatus {
    EngineStatus {
        output_channels: 0,
        running: false,
        host: settings.host.clone().unwrap_or_default(),
        device: settings.device.clone(),
        sample_rate: settings.sample_rate.unwrap_or(0),
        buffer_frames: settings.buffer_frames.unwrap_or(0),
        latency_ms: 0.0,
        latency_frames: 0,
        error: Some(error.into()),
    }
}

/// What the status says went wrong while the engine is trying to get a
/// lost device back.
fn reconnecting(reason: &str) -> String {
    format!("{reason}; reconnecting")
}

/// Owns the stream. Everything that opens, closes or drops one happens here.
fn device_thread(
    inbox: Receiver<Request>,
    controller: Controller,
    status: Arc<Mutex<EngineStatus>>,
) {
    let backend = Cpal {
        controller: controller.clone(),
    };
    let mut supervisor = Supervisor::new(backend, controller, status, Instant::now);
    loop {
        let request = match supervisor.patience() {
            Some(wait) => match inbox.recv_timeout(wait) {
                Ok(request) => Some(request),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match inbox.recv() {
                Ok(request) => Some(request),
                Err(_) => break,
            },
        };
        match request {
            Some(Request::Open(settings, done)) => {
                supervisor.configure(settings);
                let _ = done.send(());
            }
            Some(Request::Suspend(done)) => {
                let _ = done.send(supervisor.suspend());
            }
            Some(Request::Close) => break,
            None => {}
        }
        supervisor.poll();
    }
}

/// Opens streams for a [`Supervisor`]. [`Cpal`] is the real one.
trait Backend {
    type Stream: Running;

    /// Opens and starts a stream. `report` collects the host and device
    /// names found along the way, so a failure can still say how far it
    /// got, and on success describes the stream that is running.
    fn open(
        &mut self,
        settings: &AudioSettings,
        report: &mut EngineStatus,
    ) -> Result<Self::Stream, String>;
}

/// A stream that was opened. Dropping it closes it.
trait Running {
    /// The error that ended the stream, once it has one.
    fn fault(&mut self) -> Option<Fault>;
}

/// Why a stream stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fault {
    reason: String,
    /// Opening the device again can be expected to work, now or later.
    recoverable: bool,
}

/// When to try reopening a stream that was lost. It keeps time and nothing
/// else: it is told what happened and when, and says when a try is due.
#[derive(Debug)]
struct Recovery {
    /// When the stream that is running, or was last running, opened.
    opened_at: Option<Instant>,
    retry_at: Option<Instant>,
    /// How long to wait if the next try fails.
    delay: Duration,
}

impl Recovery {
    fn new() -> Self {
        Self {
            opened_at: None,
            retry_at: None,
            delay: FIRST_RETRY_DELAY,
        }
    }

    /// A stream opened at `now`.
    fn opened(&mut self, now: Instant) {
        self.opened_at = Some(now);
        self.retry_at = None;
    }

    /// The running stream lost its device at `now`. The first try is due at
    /// once, unless the stream had only just opened: then it waits out the
    /// rest of [`RECOVERY_INTERVAL`] instead of being forgotten.
    fn lost(&mut self, now: Instant) {
        let earliest = self
            .opened_at
            .map_or(now, |opened| opened + RECOVERY_INTERVAL);
        self.retry_at = Some(now.max(earliest));
        self.delay = FIRST_RETRY_DELAY;
    }

    /// A try that ended at `now` failed. The next one waits twice as long
    /// as this one did, up to [`MAX_RETRY_DELAY`].
    fn failed(&mut self, now: Instant) {
        self.retry_at = Some(now + self.delay);
        self.delay = (self.delay * 2).min(MAX_RETRY_DELAY);
    }

    /// The user chose settings, so nothing is retried on their behalf.
    fn cancel(&mut self) {
        self.retry_at = None;
    }

    /// True once, when a try is due at `now`.
    fn due(&mut self, now: Instant) -> bool {
        let due = self.retry_at.is_some_and(|at| at <= now);
        if due {
            self.retry_at = None;
        }
        due
    }

    /// Time from `now` until the next try, if one is pending.
    fn wait(&self, now: Instant) -> Option<Duration> {
        self.retry_at.map(|at| at.saturating_duration_since(now))
    }
}

/// Looks after the stream: opens it when asked, notices when it fails,
/// tells the controller and the status, and reopens a device that was lost.
///
/// `now` is its only clock, and [`Supervisor::patience`] says how long it
/// can be left alone.
struct Supervisor<B: Backend, C: Fn() -> Instant> {
    backend: B,
    controller: Controller,
    status: Arc<Mutex<EngineStatus>>,
    now: C,
    /// What the user last asked for, and what a reopen asks for again.
    settings: AudioSettings,
    stream: Option<B::Stream>,
    recovery: Recovery,
}

impl<B: Backend, C: Fn() -> Instant> Supervisor<B, C> {
    fn new(backend: B, controller: Controller, status: Arc<Mutex<EngineStatus>>, now: C) -> Self {
        Self {
            backend,
            controller,
            status,
            now,
            settings: AudioSettings::default(),
            stream: None,
            recovery: Recovery::new(),
        }
    }

    /// Closes the stream and opens one with the settings the user asked
    /// for. Playback that was running carries on in the new stream.
    fn configure(&mut self, settings: AudioSettings) {
        self.settings = settings;
        self.recovery.cancel();
        // The old stream goes first: its device may be the one wanted next.
        let closing = match self.controller.begin_close(true) {
            Ok(closing) => closing,
            Err(reason) => {
                *lock(&self.status) = stopped(&self.settings, reason.to_string());
                return;
            }
        };
        self.stream = None;
        self.controller.finish_close(closing);
        if let Err(failure) = self.open() {
            *lock(&self.status) = failure;
        }
    }

    fn suspend(&mut self) -> Result<(), String> {
        self.recovery.cancel();
        let closing = self.controller.begin_close(true).map_err(|error|error.to_string())?;
        self.stream = None;
        self.controller.finish_close(closing);
        let mut status = lock(&self.status);
        status.running = false;
        status.error = None;
        Ok(())
    }

    /// Checks on the stream and makes any try at reopening that is due.
    fn poll(&mut self) {
        if let Some(fault) = self.stream.as_mut().and_then(Running::fault) {
            let closing = match self.controller.begin_close(false) {
                Ok(closing) => closing,
                Err(reason) => {
                    *lock(&self.status) = stopped(&self.settings, reason.to_string());
                    return;
                }
            };
            self.stream = None;
            self.controller.finish_close(closing);
            let mut status = lock(&self.status);
            status.running = false;
            status.error = Some(if fault.recoverable {
                self.recovery.lost((self.now)());
                reconnecting(&fault.reason)
            } else {
                fault.reason
            });
        }
        if self.recovery.due((self.now)())
            && let Err(mut failure) = self.open()
        {
            // Timed from the end of the try: a driver can take seconds to
            // say no, and the wait is for the device, not for the driver.
            self.recovery.failed((self.now)());
            failure.error = failure.error.as_deref().map(reconnecting);
            *lock(&self.status) = failure;
        }
    }

    /// How long until [`Supervisor::poll`] has to be called again. `None`
    /// when nothing will happen until the user asks.
    fn patience(&self) -> Option<Duration> {
        let watch = self.stream.as_ref().map(|_| FAULT_POLL);
        match (self.recovery.wait((self.now)()), watch) {
            (Some(retry), Some(watch)) => Some(retry.min(watch)),
            (retry, watch) => retry.or(watch),
        }
    }

    /// Opens a stream with the current settings. When that fails the
    /// controller is told there is no stream, and the status to show is
    /// returned.
    fn open(&mut self) -> Result<(), EngineStatus> {
        let mut report = stopped(&self.settings, "");
        match self.backend.open(&self.settings, &mut report) {
            Ok(stream) => {
                self.stream = Some(stream);
                self.recovery.opened((self.now)());
                *lock(&self.status) = report;
                Ok(())
            }
            Err(reason) => {
                self.controller.detach();
                report.running = false;
                report.error = Some(reason);
                Err(report)
            }
        }
    }
}

impl<B: Backend, C: Fn() -> Instant> Drop for Supervisor<B, C> {
    fn drop(&mut self) {
        if let Ok(closing) = self.controller.begin_close(false) {
            self.stream = None;
            self.controller.finish_close(closing);
        }
    }
}

/// The real devices of this machine.
struct Cpal {
    controller: Controller,
}

impl Backend for Cpal {
    type Stream = CpalStream;

    fn open(
        &mut self,
        settings: &AudioSettings,
        report: &mut EngineStatus,
    ) -> Result<CpalStream, String> {
        catch_unwind(AssertUnwindSafe(|| {
            open(settings, &self.controller, report)
        }))
        .unwrap_or_else(|_| Err("the audio driver crashed while opening the device".to_owned()))
    }
}

/// An open cpal stream and the errors it has reported.
struct CpalStream {
    /// Declared first so it is dropped first: once it is gone, nothing
    /// reports into `faults` any more.
    _stream: cpal::Stream,
    faults: FaultInbox,
}

impl Running for CpalStream {
    fn fault(&mut self) -> Option<Fault> {
        self.faults.take()
    }
}

/// True for errors after which the same settings may well open again:
/// the device was unplugged or taken, the audio service restarted, or the
/// system changed the format under the stream.
fn recoverable(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::DeviceNotAvailable
            | ErrorKind::StreamInvalidated
            | ErrorKind::DeviceBusy
            | ErrorKind::HostUnavailable
            | ErrorKind::BackendError
    )
}

/// True for errors a stream reports and then carries on from.
fn passing(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
    )
}

/// No error ended the stream.
const ENDED_NOT: u8 = 0;
/// An error ended the stream, and reopening may work.
const ENDED_RECOVERABLE: u8 = 1;
/// An error ended the stream for good.
const ENDED_FOR_GOOD: u8 = 2;

/// Makes the two ends of the path a stream's errors take from the audio
/// thread to the device thread.
fn fault_channel(shared: Arc<Shared>) -> (FaultReporter, FaultInbox) {
    let (errors_in, errors_out) = RingBuffer::new(FAULT_CAPACITY);
    let ended = Arc::new(AtomicU8::new(ENDED_NOT));
    (
        FaultReporter {
            shared,
            errors: errors_in,
            ended: ended.clone(),
        },
        FaultInbox {
            errors: errors_out,
            ended,
        },
    )
}

/// The stream's error callback. cpal calls it on the audio thread, so it
/// may not lock, allocate or free: it counts an xrun, notes whether the
/// error ended the stream, and moves the error itself, which can own a
/// message on the heap, to the device thread to be read and dropped there.
struct FaultReporter {
    shared: Arc<Shared>,
    errors: Producer<cpal::Error>,
    /// One of the `ENDED_` values, written when an error that ended the
    /// stream found the queue full, so that a burst of xruns cannot hide it.
    ended: Arc<AtomicU8>,
}

impl FaultReporter {
    fn report(&mut self, error: cpal::Error) {
        let kind = error.kind();
        if kind == ErrorKind::Xrun {
            self.shared.record_xrun();
        }
        if let Err(PushError::Full(error)) = self.errors.push(error) {
            // Nobody has collected for a while. An error that ended the
            // stream still has to get through, so that much of it is noted
            // where there is always room. The first one counts.
            if !passing(kind) {
                let ended = if recoverable(kind) {
                    ENDED_RECOVERABLE
                } else {
                    ENDED_FOR_GOOD
                };
                let _ = self.ended.compare_exchange(
                    ENDED_NOT,
                    ended,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            }
            // Dropping the error here could free its message on the audio
            // thread, and leaking it is the one way left not to.
            std::mem::forget(error);
        }
    }
}

/// The device thread's end of a stream's errors.
struct FaultInbox {
    errors: Consumer<cpal::Error>,
    ended: Arc<AtomicU8>,
}

impl FaultInbox {
    /// Drops the errors collected so far and returns the one that ended the
    /// stream, if any did.
    fn take(&mut self) -> Option<Fault> {
        let mut fault = None;
        while let Ok(error) = self.errors.pop() {
            if fault.is_none() && !passing(error.kind()) {
                fault = Some(Fault {
                    reason: error.to_string(),
                    recoverable: recoverable(error.kind()),
                });
            }
        }
        fault.or_else(|| {
            // The error itself had no room in the queue, so its text is
            // lost.
            let ended = self.ended.load(Ordering::Acquire);
            (ended != ENDED_NOT).then(|| Fault {
                reason: "the audio stream stopped".to_owned(),
                recoverable: ended == ENDED_RECOVERABLE,
            })
        })
    }
}

/// Opens and starts a stream. `report` collects the host and device names
/// found along the way, so a failure can still say how far it got.
fn open(
    settings: &AudioSettings,
    controller: &Controller,
    report: &mut EngineStatus,
) -> Result<CpalStream, String> {
    let host = find_host(settings.host.as_deref())?;
    report.host = host.id().name().to_owned();
    let device = find_device(&host, settings.device.as_deref())?;
    let name = device_name(&device);
    report.device = Some(name.clone());

    let default = device
        .default_output_config()
        .map_err(|error| format!("could not read the output format of \"{name}\": {error}"))?;
    let preferred = choose_channel_config(
        &device,
        choose_config(&device, default, settings.sample_rate),
        settings.output_channels,
    )?;
    let fallback = choose_channel_config(&device, default, settings.output_channels)?;
    let buffer = settings
        .buffer_frames
        .map(|frames| fit_buffer(frames, preferred.buffer_size()));

    // Each fallback gives up one request: first the buffer size, then the
    // sample rate.
    let mut attempts = vec![(preferred, buffer)];
    if buffer.is_some() {
        attempts.push((preferred, None));
    }
    if preferred != fallback {
        attempts.push((fallback, None));
    }

    let mut last_error = String::new();
    for (config, buffer) in attempts {
        let (stream, faults, admission) = match build(&device, config, buffer, controller) {
            Ok(built) => built,
            Err(BuildFailure::Driver(error)) => {
                last_error = error;
                continue;
            }
            Err(BuildFailure::Preparation(error)) => return Err(error.to_string()),
        };
        let buffer_frames = stream.buffer_size().ok().or(buffer).unwrap_or(0);
        report.running = true;
        report.sample_rate = config.sample_rate();
        report.output_channels = config.channels();
        report.buffer_frames = buffer_frames;
        report.latency_ms = latency_ms(buffer_frames, config.sample_rate());
        report.error = None;
        controller.shared().begin_stream();
        let started = stream
            .play()
            .map_err(|error| format!("could not start the stream on \"{name}\": {error}"));
        let stream = finish_start(controller, admission, stream, started)?;
        return Ok(CpalStream {
            _stream: stream,
            faults,
        });
    }
    Err(format!("could not open \"{name}\": {last_error}"))
}

fn find_host(name: Option<&str>) -> Result<cpal::Host, String> {
    let Some(name) = name else {
        return Ok(cpal::default_host());
    };
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("audio host \"{name}\" is not available"))?;
    cpal::host_from_id(id).map_err(|error| format!("could not open audio host \"{name}\": {error}"))
}

fn find_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device, String> {
    let Some(name) = name else {
        return host
            .default_output_device()
            .ok_or_else(|| "no audio output device was found".to_owned());
    };
    host.output_devices()
        .map_err(|error| format!("could not list audio devices: {error}"))?
        .find(|device| device_name(device) == name)
        .ok_or_else(|| format!("audio output device \"{name}\" was not found"))
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| "unknown device".to_owned())
}

/// The device's default format, or its best format at the wanted sample
/// rate when it has one.
fn choose_config(
    device: &cpal::Device,
    default: SupportedStreamConfig,
    sample_rate: Option<u32>,
) -> SupportedStreamConfig {
    let Some(rate) = sample_rate.filter(|rate| *rate != default.sample_rate()) else {
        return default;
    };
    device
        .supported_output_configs()
        .ok()
        .and_then(|configs| {
            configs
                .filter(|range| writable(range.sample_format()))
                .filter(|range| range.contains_rate(rate))
                .max_by(|a, b| a.cmp_default_heuristics(b))
        })
        .and_then(|range| range.try_with_sample_rate(rate))
        .unwrap_or(default)
}

/// Brings a requested buffer size inside what the driver says it accepts.
fn fit_buffer(frames: u32, supported: &SupportedBufferSize) -> u32 {
    match *supported {
        SupportedBufferSize::Range { min, max } if min <= max => frames.clamp(min, max),
        _ => frames,
    }
    .max(1)
}

fn choose_channel_config(
    device: &cpal::Device,
    preferred: SupportedStreamConfig,
    requested: Option<u16>,
) -> Result<SupportedStreamConfig, String> {
    let most = crate::hardware_output::MAX_OUTPUT_CHANNELS as u16;
    if requested.is_some_and(|channels| channels == 0 || channels > most) {
        return Err(format!("Choose 1–{most} output channels"));
    }
    if preferred.channels() <= most
        && requested.is_none_or(|channels| channels == preferred.channels())
    {
        return Ok(preferred);
    }
    let rate = preferred.sample_rate();
    device
        .supported_output_configs()
        .map_err(|error| error.to_string())?
        .filter(|range| {
            writable(range.sample_format())
                && range.channels() <= most
                && requested.is_none_or(|channels| range.channels() == channels)
        })
        .map(|range| {
            let chosen = rate.clamp(range.min_sample_rate(), range.max_sample_rate());
            (chosen.abs_diff(rate), range.with_sample_rate(chosen))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, config)| config)
        .ok_or_else(|| {
            "The selected device does not offer the requested output channel layout".into()
        })
}

enum BuildFailure {
    Driver(String),
    Preparation(crate::ProjectPreparationError),
}

fn prepared_attachment(
    controller: &Controller,
    sample_rate: u32,
) -> Result<
    (
        Processor,
        crate::project_preparation::AttachmentAdmission,
        crate::ProjectRetirement,
    ),
    crate::ProjectPreparationError,
> {
    use crate::ProjectPreparationError;
    use crate::project_preparation::{AttachmentMode, PublicationRefusal};
    controller.shared().recording_clock.reset_output();
    for attempt in 1..=3 {
        match controller.try_attach(sample_rate, AttachmentMode::Device) {
            Ok(prepared) => return Ok(prepared),
            Err(
                error @ (ProjectPreparationError::IdentityChanged
                | ProjectPreparationError::Publication(PublicationRefusal::StalePlan)),
            ) => {
                if attempt == 3 {
                    return Err(ProjectPreparationError::AttachmentRetries {
                        attempts: attempt,
                        reason: Box::new(error),
                    });
                }
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded attachment retry")
}

fn finish_start<T>(
    controller: &Controller,
    admission: crate::project_preparation::AttachmentAdmission,
    stream: T,
    started: Result<(), String>,
) -> Result<T, String> {
    let result = started.and_then(|()| {
        controller
            .complete_attachment(admission)
            .map_err(|error| error.to_string())
    });
    match result {
        Ok(()) => Ok(stream),
        Err(error) => {
            let closing = controller
                .begin_close(false)
                .map_err(|error| error.to_string())?;
            drop(stream);
            controller.finish_close(closing);
            Err(error)
        }
    }
}

fn build(
    device: &cpal::Device,
    config: SupportedStreamConfig,
    buffer: Option<u32>,
    controller: &Controller,
) -> Result<
    (
        cpal::Stream,
        FaultInbox,
        crate::project_preparation::AttachmentAdmission,
    ),
    BuildFailure,
> {
    let format = config.sample_format();
    if !writable(format) {
        return Err(BuildFailure::Driver(format!(
            "the sample format {format} is not supported"
        )));
    }
    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: buffer.map_or(BufferSize::Default, BufferSize::Fixed),
    };
    let (mut processor, admission, retirement) =
        prepared_attachment(controller, config.sample_rate()).map_err(BuildFailure::Preparation)?;
    drop(retirement);
    processor.set_output_channels(usize::from(config.channels()));
    let mut feeder = Feeder {
        processor,
        shared: controller.shared().clone(),
        channels: usize::from(config.channels()),
        scratch: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
        coverage: Coverage::new(config.sample_rate()),
    };
    let (mut reporter, faults) = fault_channel(controller.shared().clone());
    let timebase = crate::recording_clock::DriverTimebase::default();
    let callback_timebase = timebase.clone();
    let built = device
        .build_output_stream_raw(
            stream_config,
            format,
            move |data: &mut cpal::Data, info: &cpal::OutputCallbackInfo| {
                feeder.shared.recording_clock.publish_driver_output(
                    feeder.processor.output_frame(),
                    feeder.processor.sample_rate(),
                    info.timestamp(),
                    &callback_timebase,
                );
                feeder.fill(data);
            },
            move |error: cpal::Error| reporter.report(error),
            Some(OPEN_TIMEOUT),
        )
        .map(|stream| {
            timebase.initialize(&stream, &controller.shared().recording_clock);
            (stream, faults, admission)
        });
    match built {
        Ok(stream) => Ok(stream),
        Err(error) => {
            // CPAL has dropped its failed Feeder before returning. Starting
            // fenced document publication throughout that destruction.
            let closing = controller
                .begin_close(false)
                .map_err(|error| BuildFailure::Driver(error.to_string()))?;
            controller.finish_close(closing);
            Err(BuildFailure::Driver(error.to_string()))
        }
    }
}

/// True for the sample formats [`Feeder::fill`] can write.
fn writable(format: SampleFormat) -> bool {
    format.is_int() || format.is_uint() || format.is_float()
}

/// The device callback: runs the processor and writes its stereo float
/// output in whatever layout and sample format the device wants.
struct Feeder {
    processor: Processor,
    shared: Arc<Shared>,
    /// Channels per frame of the device buffer.
    channels: usize,
    /// Stereo float audio from the processor, before conversion.
    scratch: Box<[Frame]>,
    coverage: Coverage,
}

impl Feeder {
    fn fill(&mut self, data: &mut cpal::Data) {
        let started = Instant::now();
        let frames = data.len() / self.channels;
        self.processor.begin_device_buffer(started, frames);
        if self.coverage.fell_behind(started, frames) {
            self.shared.record_xrun();
        }
        match data.sample_format() {
            SampleFormat::I8 => self.write::<i8>(data),
            SampleFormat::I16 => self.write::<i16>(data),
            SampleFormat::I24 => self.write::<I24>(data),
            SampleFormat::I32 => self.write::<i32>(data),
            SampleFormat::I64 => self.write::<i64>(data),
            SampleFormat::U8 => self.write::<u8>(data),
            SampleFormat::U16 => self.write::<u16>(data),
            SampleFormat::U24 => self.write::<U24>(data),
            SampleFormat::U32 => self.write::<u32>(data),
            SampleFormat::U64 => self.write::<u64>(data),
            SampleFormat::F32 => self.write::<f32>(data),
            SampleFormat::F64 => self.write::<f64>(data),
            // cpal fills the buffer with silence before the callback, so a
            // format that cannot be written stays silent.
            _ => {}
        }
        self.shared
            .record_buffer(frames, started.elapsed(), self.processor.sample_rate());
        self.processor.end_device_buffer();
    }

    /// Fills the requested device layout in bounded blocks, summing physical
    /// destinations before sample conversion. Every unwritten port is silent.
    fn write<T>(&mut self, data: &mut cpal::Data)
    where
        T: SizedSample + FromSample<f32>,
    {
        let Some(out) = data.as_slice_mut::<T>() else {
            return;
        };
        let clip = !T::FORMAT.is_float();
        for block in out.chunks_mut(MAX_BLOCK * self.channels) {
            let frames = block.len() / self.channels;
            let scratch = &mut self.scratch[..frames];
            self.processor.process(scratch.as_flattened_mut());
            block.fill(T::from_sample(0.0));
            for (output, sample) in block.iter_mut().zip(self.processor.hardware_samples()) {
                let value = if sample.is_finite() { *sample } else { 0.0 };
                *output = T::from_sample(if clip { value.clamp(-1.0, 1.0) } else { value });
            }
        }
    }
}

/// Notices when the device asks for less audio than the time that passes.
///
/// A device that is served on time asks for one second of audio every
/// second. When a buffer comes too late the device runs dry, and the time it
/// spent with nothing to play is never asked for: the count of frames
/// requested drops behind the clock and stays there. That lasting drop is
/// what this looks for, and it catches dropouts no driver reports, such as
/// a callback the system scheduled too late.
struct Coverage {
    sample_rate: f64,
    started: Option<Instant>,
    /// Frames asked for before the buffer being looked at.
    frames: u64,
    /// How far ahead of the clock the request count was at its best in the
    /// current window of buffers. A buffer that is merely late pulls its
    /// own reading down, so the best of a window is the on-time figure.
    best: f64,
    /// The same figure for the window before.
    previous_best: Option<f64>,
    buffers: u32,
}

impl Coverage {
    fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: f64::from(sample_rate),
            started: None,
            frames: 0,
            best: f64::NEG_INFINITY,
            previous_best: None,
            buffers: 0,
        }
    }

    /// Takes note of a buffer of `frames` frames asked for at `now`. Returns
    /// true when this buffer completes a window in which the requests fell
    /// behind the clock.
    fn fell_behind(&mut self, now: Instant, frames: usize) -> bool {
        let started = *self.started.get_or_insert(now);
        let due = now.duration_since(started).as_secs_f64() * self.sample_rate;
        self.best = self.best.max(self.frames as f64 - due);
        self.frames += frames as u64;
        self.buffers += 1;
        if self.buffers < COVERAGE_WINDOW {
            return false;
        }
        // Half a buffer, and never less than a millisecond, is well clear of
        // timing jitter and of the drift between the system clock and the
        // device's own over one window.
        let tolerance = (frames as f64 / 2.0).max(self.sample_rate / 1000.0);
        let behind = self
            .previous_best
            .is_some_and(|previous| self.best < previous - tolerance);
        self.previous_best = Some(self.best);
        self.best = f64::NEG_INFINITY;
        self.buffers = 0;
        behind
    }
}

fn list_hosts() -> Vec<AudioHost> {
    let default_host = cpal::default_host().id();
    cpal::available_hosts()
        .into_iter()
        .filter_map(|id| {
            let host = cpal::host_from_id(id).ok()?;
            // The default device is a handle that follows the system
            // setting, so it is matched to a listed device by id.
            let default_id = host
                .default_output_device()
                .and_then(|device| device.id().ok());
            let devices = host
                .output_devices()
                .ok()?
                .map(|device| {
                    let is_default = default_id.is_some() && device.id().ok() == default_id;
                    describe(&device, is_default)
                })
                .collect();
            Some(AudioHost {
                name: id.name().to_owned(),
                is_default: id == default_host,
                devices,
            })
        })
        .collect()
}

fn describe(device: &cpal::Device, is_default: bool) -> AudioDevice {
    let mut sample_rates = Vec::new();
    let mut output_channels = Vec::new();
    for range in device.supported_output_configs().into_iter().flatten() {
        if writable(range.sample_format())
            && usize::from(range.channels()) <= crate::hardware_output::MAX_OUTPUT_CHANNELS
        {
            output_channels.push(range.channels());
        }
        sample_rates.extend(
            STANDARD_RATES
                .into_iter()
                .filter(|rate| range.contains_rate(*rate)),
        );
    }
    sample_rates.sort_unstable();
    sample_rates.dedup();
    output_channels.sort_unstable();
    output_channels.dedup();
    // Buffer sizes are in frames, so a range only means something at one
    // sample rate. The one reported is for the device's default format.
    let buffer_range =
        device
            .default_output_config()
            .ok()
            .and_then(|config| match *config.buffer_size() {
                SupportedBufferSize::Range { min, max } => Some((min, max)),
                SupportedBufferSize::Unknown => None,
            });
    AudioDevice {
        name: device_name(device),
        is_default,
        sample_rates,
        min_buffer_frames: buffer_range.map(|(low, _)| low),
        max_buffer_frames: buffer_range.map(|(_, high)| high),
        output_channels,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use windfall_project::Project;

    use super::*;
    use crate::pool::SamplePool;
    use crate::test_alloc::allocator_calls;

    /// Exercises the real backend's close/reopen under the supervisor's actual
    /// fault handling. Only the invalidation notice is injected; no endpoint
    /// setting, volume, or system device is changed.
    #[test]
    #[ignore = "opens a real output; set WINDFALL_DEVICE_EVIDENCE and WINDFALL_DEVICE_OUTPUT"]
    fn device_stream_error_recovery() {
        let evidence = std::path::PathBuf::from(std::env::var("WINDFALL_DEVICE_EVIDENCE").unwrap());
        let output = std::env::var("WINDFALL_DEVICE_OUTPUT").unwrap();
        assert!(matches!(
            output.as_str(),
            "Realtek Digital Output (Realtek(R) Audio)" | "Speakers (Steam Streaming Speakers)"
        ));
        let settings = AudioSettings {
            host: Some("WASAPI".into()),
            device: Some(output),
            buffer_frames: Some(480),
            ..AudioSettings::default()
        };
        let controller = Controller::new();
        let status = Arc::new(Mutex::new(stopped(&settings, "starting")));
        let now = Rc::new(Cell::new(Instant::now()));
        let mut supervisor = Supervisor::new(
            Cpal {
                controller: controller.clone(),
            },
            controller.clone(),
            status.clone(),
            {
                let now = now.clone();
                move || now.get()
            },
        );
        supervisor.configure(settings);
        std::thread::sleep(Duration::from_millis(300));
        let opened = lock(&status).running;
        let first_frames = controller.stream_stats().frames;
        let gate = controller.recording_clock().arm(true);
        // Same atomic fallback used if an error callback finds its queue full.
        supervisor
            .stream
            .as_mut()
            .expect("allowed output opens")
            .faults
            .ended
            .store(ENDED_RECOVERABLE, Ordering::Release);
        supervisor.poll();
        let lost = lock(&status).clone();
        now.set(now.get() + Duration::from_secs(2));
        supervisor.poll();
        std::thread::sleep(Duration::from_millis(300));
        let reopened = lock(&status).clone();
        let resumed_frames = controller.stream_stats().frames;
        let invalidated = gate.window().is_err();
        let passed = opened
            && first_frames > 0
            && !lost.running
            && lost
                .error
                .as_ref()
                .is_some_and(|e| e.contains("reconnecting"))
            && reopened.running
            && resumed_frames > 0
            && invalidated;
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::write(evidence.join("stream-error-recovery.json"), format!(
            "{{\n  \"scenario\": \"stream-error-recovery\",\n  \"faultInjection\": \"recoverable stream-ended notice; real Cpal backend\",\n  \"physicalRemoval\": false,\n  \"opened\": {opened},\n  \"firstFrames\": {first_frames},\n  \"outageObserved\": {},\n  \"reopened\": {},\n  \"resumedFrames\": {resumed_frames},\n  \"oldTicketInvalidated\": {invalidated},\n  \"sampleRate\": {},\n  \"passed\": {passed}\n}}\n", !lost.running, reopened.running, reopened.sample_rate
        )).unwrap();
        assert!(
            passed,
            "real driver recovery failed: {lost:?} -> {reopened:?}"
        );
    }

    #[derive(Debug, Default)]
    struct PreparationProbe {
        revision: std::sync::atomic::AtomicU64,
        made: std::sync::atomic::AtomicUsize,
        dropped: std::sync::atomic::AtomicUsize,
        drift: std::sync::atomic::AtomicBool,
        fail: std::sync::atomic::AtomicBool,
        external: Mutex<()>,
    }

    struct PreparationFactory {
        probe: Arc<PreparationProbe>,
        controller: std::sync::Weak<Controller>,
    }
    impl std::fmt::Debug for PreparationFactory {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PreparationFactory")
        }
    }
    struct PreparedUnit {
        probe: Arc<PreparationProbe>,
        controller: std::sync::Weak<Controller>,
    }
    impl crate::plugins::HostedEffect for PreparedUnit {
        fn process(&mut self, _: &mut [f32], _: &mut [f32]) {}
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            37
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl Drop for PreparedUnit {
        fn drop(&mut self) {
            use std::sync::atomic::Ordering::Relaxed;
            assert!(self.probe.external.try_lock().is_ok());
            if let Some(controller) = self.controller.upgrade() {
                // Both calls must finish while the destructor runs. The
                // controller recovers poisoned guards without draining owners.
                controller.transport();
                controller.frame();
            }
            self.probe.dropped.fetch_add(1, Relaxed);
        }
    }
    impl crate::plugins::PluginFactory for PreparationFactory {
        fn provider_identity(&self) -> u64 {
            Arc::as_ptr(&self.probe) as usize as u64
        }
        fn revision(&self) -> u64 {
            self.probe
                .revision
                .load(std::sync::atomic::Ordering::Relaxed)
        }
        fn effect(
            &self,
            _: &windfall_project::PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn crate::plugins::HostedEffect>, String> {
            use std::sync::atomic::Ordering::Relaxed;
            self.probe.made.fetch_add(1, Relaxed);
            if self.probe.fail.load(Relaxed) {
                return Err("startup failed".into());
            }
            if self.probe.drift.load(Relaxed) {
                self.probe.revision.fetch_add(1, Relaxed);
            }
            Ok(Box::new(PreparedUnit {
                probe: self.probe.clone(),
                controller: self.controller.clone(),
            }))
        }
        fn instrument(
            &self,
            _: &windfall_project::PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn crate::plugins::HostedInstrument>, String> {
            Err("effects only".into())
        }
    }

    fn native_attachment() -> (Arc<Controller>, Arc<PreparationProbe>) {
        use windfall_project::{Command, Document, EffectId, PluginBinding, PluginTarget, TrackId};
        let controller = Arc::new(Controller::new());
        let probe = Arc::new(PreparationProbe::default());
        let mut project = Document::new(Project::new("device readiness"));
        project
            .dispatch(
                Command::AddPluginEffect {
                    track: TrackId(0),
                    plugin: PluginBinding {
                        target: PluginTarget::Effect {
                            effect: EffectId(0),
                        },
                        format: "clap".into(),
                        path: "p1-device.clap".into(),
                        id: "device".into(),
                        name: "device".into(),
                        state: vec![],
                        parameters: vec![],
                        sidechain_input: None,
                        auxiliary_inputs: Vec::new(),
                        wrapper: Default::default(),
                    },
                },
                None,
            )
            .unwrap();
        let mut pool = SamplePool::new();
        pool.set_plugin_factory(Arc::new(PreparationFactory {
            probe: probe.clone(),
            controller: Arc::downgrade(&controller),
        }));
        controller.set_project(project.project(), &pool);
        (controller, probe)
    }

    #[test]
    fn p1_device_startup_error_is_fatal_and_identity_drift_exhausts_three_fresh_attempts() {
        use std::sync::atomic::Ordering::Relaxed;
        let (controller, probe) = native_attachment();
        probe.fail.store(true, Relaxed);
        assert!(matches!(
            prepared_attachment(&controller, 48_000),
            Err(crate::ProjectPreparationError::Native { .. })
        ));
        assert_eq!(probe.made.load(Relaxed), 1);
        probe.fail.store(false, Relaxed);
        probe.drift.store(true, Relaxed);
        assert!(matches!(
            prepared_attachment(&controller, 48_000),
            Err(crate::ProjectPreparationError::AttachmentRetries { attempts: 3, .. })
        ));
        assert_eq!(probe.made.load(Relaxed), 4);
        assert_eq!(probe.dropped.load(Relaxed), 3);
        probe.drift.store(false, Relaxed);
        let (processor, admission, retirement) = prepared_attachment(&controller, 48_000).unwrap();
        drop(retirement);
        let processor = finish_start(&controller, admission, processor, Ok(())).unwrap();
        let closing = controller.begin_close(false).unwrap();
        drop(processor);
        controller.finish_close(closing);
        assert_eq!(probe.dropped.load(Relaxed), 4);
    }

    #[test]
    fn p1_device_failed_build_play_and_same_rate_reopen_retire_outside_guards() {
        use std::sync::atomic::Ordering::Relaxed;
        let (controller, probe) = native_attachment();
        // A failed driver's build owns and destroys the processor before
        // returning Err, exactly as build_output_stream_raw's consumed closure.
        let (processor, _, retirement) = prepared_attachment(&controller, 48_000).unwrap();
        drop(retirement);
        assert!(matches!(
            controller.preparation_snapshot().prepare(
                &Project::new("blocked"),
                &SamplePool::new(),
                crate::ProjectPublicationIntent::Edit
            ),
            Err(crate::ProjectPreparationError::StreamTransitioning)
        ));
        drop(processor);
        controller.detach();
        assert_eq!(probe.dropped.load(Relaxed), 1);
        let (processor, admission, retirement) = prepared_attachment(&controller, 48_000).unwrap();
        drop(retirement);
        assert!(
            finish_start(
                &controller,
                admission,
                processor,
                Err("driver play failed".into())
            )
            .is_err()
        );
        assert_eq!(probe.dropped.load(Relaxed), 2);
        let (mut processor, admission, retirement) =
            prepared_attachment(&controller, 48_000).unwrap();
        drop(retirement);
        controller.complete_attachment(admission).unwrap();
        controller.play();
        assert_eq!(allocator_calls(|| processor.process(&mut [0.0; 2])), 0);
        let closing = controller.begin_close(true).unwrap();
        drop(processor);
        controller.finish_close(closing);
        assert_eq!(probe.dropped.load(Relaxed), 3);
        let (mut reopened, admission, retirement) =
            prepared_attachment(&controller, 48_000).unwrap();
        drop(retirement);
        controller.complete_attachment(admission).unwrap();
        assert_eq!(allocator_calls(|| reopened.process(&mut [0.0; 2])), 0);
        assert!(controller.frame().playing);
        let closing = controller.begin_close(false).unwrap();
        drop(reopened);
        controller.finish_close(closing);
        assert_eq!(probe.made.load(Relaxed), 4);
        assert_eq!(probe.dropped.load(Relaxed), 4);
    }

    /// What a scripted backend is to do, and what was asked of it.
    #[derive(Default)]
    struct Script {
        /// How each coming attempt to open ends, in order. Once these run
        /// out, opening works.
        outcomes: VecDeque<Result<(), String>>,
        /// Number of attempts to open so far.
        attempts: usize,
        /// The error the open stream reports next.
        fault: Option<Fault>,
        /// How long every attempt to open takes.
        open_takes: Duration,
    }

    /// A clock that only moves when it is told to.
    type TestClock = Rc<Cell<Instant>>;

    struct Scripted {
        script: Rc<RefCell<Script>>,
        controller: Controller,
        clock: TestClock,
    }

    struct ScriptedStream {
        script: Rc<RefCell<Script>>,
        processor: Processor,
    }

    impl Backend for Scripted {
        type Stream = ScriptedStream;

        fn open(
            &mut self,
            _: &AudioSettings,
            report: &mut EngineStatus,
        ) -> Result<ScriptedStream, String> {
            let mut script = self.script.borrow_mut();
            script.attempts += 1;
            self.clock.set(self.clock.get() + script.open_takes);
            report.host = "Scripted".to_owned();
            // As with a real device, the processor exists before it is known
            // whether the stream will start.
            let (processor, admission, retirement) =
                prepared_attachment(&self.controller, 48_000).map_err(|error| error.to_string())?;
            drop(retirement);
            script.outcomes.pop_front().unwrap_or(Ok(()))?;
            self.controller
                .complete_attachment(admission)
                .map_err(|error| error.to_string())?;
            report.running = true;
            report.sample_rate = 48_000;
            report.error = None;
            Ok(ScriptedStream {
                script: self.script.clone(),
                processor,
            })
        }
    }

    impl Running for ScriptedStream {
        fn fault(&mut self) -> Option<Fault> {
            self.script.borrow_mut().fault.take()
        }
    }

    /// A supervisor over a scripted backend and a clock that starts at
    /// 0 ms. Every call says what time it is made at.
    struct Bench {
        supervisor: Supervisor<Scripted, Box<dyn Fn() -> Instant>>,
        script: Rc<RefCell<Script>>,
        controller: Controller,
        status: Arc<Mutex<EngineStatus>>,
        clock: TestClock,
        epoch: Instant,
    }

    impl Bench {
        fn new() -> Self {
            let script = Rc::new(RefCell::new(Script::default()));
            let controller = Controller::new();
            let status = Arc::new(Mutex::new(stopped(&AudioSettings::default(), "starting")));
            let epoch = Instant::now();
            let clock = Rc::new(Cell::new(epoch));
            let backend = Scripted {
                script: script.clone(),
                controller: controller.clone(),
                clock: clock.clone(),
            };
            let now: Box<dyn Fn() -> Instant> = Box::new({
                let clock = clock.clone();
                move || clock.get()
            });
            Self {
                supervisor: Supervisor::new(backend, controller.clone(), status.clone(), now),
                script,
                controller,
                status,
                clock,
                epoch,
            }
        }

        /// A bench whose stream opened at 0 ms.
        fn running() -> Self {
            let mut bench = Self::new();
            bench.configure(0);
            assert!(bench.status().running);
            bench
        }

        /// Moves the clock on to `ms`. It never goes back.
        fn wait_until(&self, ms: u64) {
            let time = self.epoch + Duration::from_millis(ms);
            self.clock.set(self.clock.get().max(time));
        }

        /// Milliseconds on the clock.
        fn time(&self) -> u64 {
            self.clock.get().duration_since(self.epoch).as_millis() as u64
        }

        fn configure(&mut self, ms: u64) {
            self.wait_until(ms);
            self.supervisor.configure(AudioSettings::default());
        }

        fn poll(&mut self, ms: u64) {
            self.wait_until(ms);
            self.supervisor.poll();
        }

        /// Milliseconds the supervisor can be left alone from `ms` on.
        fn patience(&self, ms: u64) -> Option<u64> {
            self.wait_until(ms);
            self.supervisor
                .patience()
                .map(|wait| wait.as_millis() as u64)
        }

        fn status(&self) -> EngineStatus {
            lock(&self.status).clone()
        }

        fn attempts(&self) -> usize {
            self.script.borrow().attempts
        }

        fn fail_next(&self, attempts: usize, reason: &str) {
            let mut script = self.script.borrow_mut();
            script
                .outcomes
                .extend((0..attempts).map(|_| Err(reason.to_owned())));
        }

        fn end_stream(&self, reason: &str, recoverable: bool) {
            self.script.borrow_mut().fault = Some(Fault {
                reason: reason.to_owned(),
                recoverable,
            });
        }

        /// Runs the open stream's processor for one buffer.
        fn process(&mut self) {
            let stream = self.supervisor.stream.as_mut().expect("a stream is open");
            stream.processor.process(&mut [0.0; 128]);
        }

        /// Lets the supervisor run from `from_ms` the way the device thread
        /// does, sleeping exactly as long as it asks to, until `until_ms`.
        /// Returns the times at which it tried to open a stream.
        fn run(&mut self, from_ms: u64, until_ms: u64) -> Vec<u64> {
            let mut tries = Vec::new();
            let mut wake = from_ms;
            while wake <= until_ms {
                self.wait_until(wake);
                let (started, before) = (self.time(), self.attempts());
                self.supervisor.poll();
                if self.attempts() > before {
                    tries.push(started);
                }
                match self.supervisor.patience() {
                    Some(wait) => wake = self.time() + (wait.as_millis() as u64).max(1),
                    None => break,
                }
            }
            tries
        }
    }

    #[test]
    fn installer_suspend_destroys_the_stream_cancels_recovery_and_reopens_running() {
        let mut bench=Bench::running();
        bench.controller.play();bench.process();
        assert!(bench.controller.frame().playing);
        let attempts=bench.attempts();
        bench.supervisor.suspend().unwrap();
        assert!(bench.supervisor.stream.is_none());
        assert!(!bench.status().running);
        assert!(bench.status().error.is_none());
        assert!(bench.run(0,30_000).is_empty());
        assert_eq!(bench.attempts(),attempts);
        bench.configure(30_001);bench.process();
        assert!(bench.status().running);
        assert!(bench.controller.frame().playing);
        assert_eq!(bench.attempts(),attempts+1);
    }

    #[test]
    fn a_device_lost_just_after_opening_is_reopened_when_the_interval_is_over() {
        let mut bench = Bench::running();
        assert_eq!(bench.patience(0), Some(100));

        bench.end_stream("the device was unplugged", true);
        bench.poll(100);
        let status = bench.status();
        assert!(!status.running);
        assert_eq!(
            status.error.as_deref(),
            Some("the device was unplugged; reconnecting")
        );
        // The request to reopen is kept, not thrown away for coming early.
        assert_eq!(bench.attempts(), 1);
        assert_eq!(bench.patience(100), Some(900));
        bench.poll(999);
        assert_eq!(bench.attempts(), 1);

        bench.poll(1_000);
        assert_eq!(bench.attempts(), 2);
        let status = bench.status();
        assert!(status.running);
        assert_eq!(status.error, None);
        assert_eq!(bench.patience(1_000), Some(100));
    }

    #[test]
    fn a_device_that_stays_away_is_tried_again_at_growing_intervals_until_it_is_back() {
        let mut bench = Bench::running();
        bench.fail_next(6, "could not open \"Interface\"");
        bench.end_stream("the device was unplugged", true);

        // The stream had run for five seconds, so the first try comes at
        // once. Then the waits double from one second to eight and stay
        // there.
        let tries = bench.run(5_000, 35_900);
        assert_eq!(tries, [5_000, 6_000, 8_000, 12_000, 20_000, 28_000]);
        let status = bench.status();
        assert!(!status.running);
        assert_eq!(
            status.error.as_deref(),
            Some("could not open \"Interface\"; reconnecting")
        );
        assert_eq!(bench.patience(35_900), Some(100));

        assert_eq!(bench.run(36_000, 60_000), [36_000]);
        let status = bench.status();
        assert!(status.running);
        assert_eq!(status.error, None);
    }

    #[test]
    fn a_driver_that_is_slow_to_say_no_does_not_shorten_the_wait() {
        let mut bench = Bench::running();
        bench.script.borrow_mut().open_takes = Duration::from_secs(5);
        bench.fail_next(3, "timed out");
        bench.end_stream("unplugged", true);
        // Every try takes five seconds to fail, and the wait of one, two
        // and four seconds starts when it has.
        assert_eq!(bench.run(10_000, 60_000), [10_000, 16_000, 23_000, 32_000]);
        assert!(bench.status().running);
    }

    #[test]
    fn a_device_lost_again_starts_over_with_short_waits() {
        let mut bench = Bench::running();
        bench.fail_next(3, "gone");
        bench.end_stream("unplugged", true);
        assert_eq!(bench.run(5_000, 20_000), [5_000, 6_000, 8_000, 12_000]);
        assert!(bench.status().running);

        bench.fail_next(2, "gone");
        bench.end_stream("unplugged", true);
        assert_eq!(bench.run(30_000, 40_000), [30_000, 31_000, 33_000]);
        assert!(bench.status().running);
    }

    #[test]
    fn choosing_settings_calls_off_the_reconnecting() {
        let mut bench = Bench::running();
        bench.fail_next(3, "gone");
        bench.end_stream("unplugged", true);
        assert_eq!(bench.run(5_000, 7_000), [5_000, 6_000]);
        assert!(bench.patience(7_000).is_some());

        // The user picks something that cannot be opened either. That is
        // their answer, and nothing is tried behind their back.
        bench.configure(7_500);
        assert_eq!(bench.attempts(), 4);
        let status = bench.status();
        assert!(!status.running);
        assert_eq!(status.error.as_deref(), Some("gone"));
        assert_eq!(bench.patience(7_500), None);
        assert!(bench.run(7_500, 60_000).is_empty());

        bench.configure(61_000);
        assert!(bench.status().running);
    }

    #[test]
    fn an_error_that_reopening_cannot_fix_is_not_retried() {
        let mut bench = Bench::running();
        bench.end_stream("the format is not supported", false);
        assert!(bench.run(5_000, 60_000).is_empty());
        let status = bench.status();
        assert!(!status.running);
        assert_eq!(status.error.as_deref(), Some("the format is not supported"));
        assert_eq!(bench.patience(60_000), None);
    }

    #[test]
    fn playback_carries_on_through_new_settings_and_stops_when_the_device_is_lost() {
        let mut bench = Bench::running();
        bench
            .controller
            .set_project(&Project::new("t"), &SamplePool::new());
        bench.controller.play();
        bench.process();
        assert!(bench.controller.frame().playing);

        bench.configure(2_000);
        assert!(bench.controller.transport().playing);
        bench.process();
        assert!(bench.controller.frame().playing);

        // Settings that cannot be opened end playback, and say so.
        bench.fail_next(1, "no such device");
        bench.configure(3_000);
        assert!(!bench.controller.transport().playing);
        bench.configure(4_000);
        bench.process();
        assert!(!bench.controller.frame().playing);

        // So does losing the device, even though it is back at once.
        bench.controller.play();
        bench.process();
        assert!(bench.controller.transport().playing);
        bench.end_stream("unplugged", true);
        bench.poll(9_000);
        assert!(bench.status().running);
        assert!(!bench.controller.transport().playing);
        bench.process();
        assert!(!bench.controller.frame().playing);
    }

    /// A stream error that owns its message, as cpal's do.
    fn error(kind: ErrorKind, message: &str) -> cpal::Error {
        cpal::Error::with_message(kind, message.to_owned())
    }

    #[test]
    fn reporting_a_stream_error_never_touches_the_allocator() {
        let shared = Arc::new(Shared::new());
        let (mut reporter, mut inbox) = fault_channel(shared.clone());
        // More errors than the queue holds, so the ones with no room are
        // covered too.
        let mut errors: Vec<cpal::Error> = (0..FAULT_CAPACITY)
            .map(|_| error(ErrorKind::Xrun, "the buffer underran"))
            .collect();
        errors.insert(3, error(ErrorKind::DeviceNotAvailable, "unplugged"));
        errors.push(error(ErrorKind::StreamInvalidated, "format changed"));
        errors.push(error(ErrorKind::Xrun, "the buffer underran"));

        let calls = allocator_calls(|| {
            for error in errors.drain(..) {
                reporter.report(error);
            }
        });
        assert_eq!(calls, 0, "the error callback used the allocator");
        assert_eq!(shared.xruns(), FAULT_CAPACITY as u32 + 1);

        // The device thread reads the error that ended the stream, text and
        // all, and is the one to free what was queued.
        assert_eq!(
            inbox.take(),
            Some(Fault {
                reason: "unplugged".to_owned(),
                recoverable: true,
            })
        );
    }

    #[test]
    fn an_error_that_ends_the_stream_gets_through_a_full_queue() {
        let (mut reporter, mut inbox) = fault_channel(Arc::new(Shared::new()));
        for _ in 0..FAULT_CAPACITY {
            reporter.report(cpal::Error::new(ErrorKind::Xrun));
        }
        reporter.report(error(ErrorKind::DeviceNotAvailable, "unplugged"));
        let fault = inbox.take().expect("the stream ended");
        assert!(fault.recoverable);
        assert!(!fault.reason.is_empty());
    }

    #[test]
    fn errors_a_stream_carries_on_from_do_not_end_it() {
        let shared = Arc::new(Shared::new());
        let (mut reporter, mut inbox) = fault_channel(shared.clone());
        for kind in [
            ErrorKind::Xrun,
            ErrorKind::DeviceChanged,
            ErrorKind::RealtimeDenied,
        ] {
            reporter.report(cpal::Error::new(kind));
        }
        assert_eq!(inbox.take(), None);
        assert_eq!(shared.xruns(), 1);

        reporter.report(error(ErrorKind::UnsupportedConfig, "no such format"));
        assert_eq!(
            inbox.take(),
            Some(Fault {
                reason: "no such format".to_owned(),
                recoverable: false,
            })
        );
    }

    /// Feeds `Coverage` buffers of 480 frames at 48 kHz, nominally one every
    /// 10 ms, each `delay_ms(index)` later than that, and counts the windows
    /// in which it saw the requests fall behind.
    fn gaps(delay_ms: impl Fn(u32) -> f64, buffers: u32) -> usize {
        let mut coverage = Coverage::new(48_000);
        let start = Instant::now();
        (0..buffers)
            .filter(|&index| {
                let at = f64::from(index) * 0.01 + delay_ms(index) / 1000.0;
                coverage.fell_behind(start + Duration::from_secs_f64(at), 480)
            })
            .count()
    }

    #[test]
    fn buffers_served_on_time_leave_no_gap() {
        assert_eq!(gaps(|_| 0.0, 1_000), 0);
    }

    #[test]
    fn late_buffers_that_catch_up_are_not_a_gap() {
        // Most buffers arrive a few milliseconds late, some on time.
        assert_eq!(gaps(|index| f64::from(index % 5) * 1.5, 1_000), 0);
    }

    #[test]
    fn a_slow_device_clock_is_not_a_gap() {
        // The device runs a tenth of a percent slow against the system.
        assert_eq!(gaps(|index| f64::from(index) * 0.01, 5_000), 0);
    }

    #[test]
    fn time_the_device_never_asked_for_is_a_gap() {
        // From the 500th buffer on everything is 30 ms behind: the device
        // sat dry for that long and never asked for those frames.
        let stalled = |index| if index >= 500 { 30.0 } else { 0.0 };
        assert_eq!(gaps(stalled, 1_000), 1);
    }

    #[test]
    fn a_requested_buffer_size_is_kept_inside_the_range_the_driver_gives() {
        let range = SupportedBufferSize::Range { min: 480, max: 480 };
        assert_eq!(fit_buffer(128, &range), 480);
        assert_eq!(fit_buffer(128, &SupportedBufferSize::Unknown), 128);
        assert_eq!(fit_buffer(0, &SupportedBufferSize::Unknown), 1);
    }
}
