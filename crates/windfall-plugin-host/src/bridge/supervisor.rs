//! Per-instance process/control owner. No method here belongs on audio.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod editor;

use super::{
    adapter::{Audio, ParameterSpec, Signals},
    control::{ControlParameter, Decoder, DescribedParameter, Message, Owner, Packet, ReadStep},
    mapping::Mapping,
    protocol::{Config, Identity},
    slots::Region,
};
use std::{
    io,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

static SESSIONS: AtomicU64 = AtomicU64::new(1);
pub fn fresh_identity(token: u64, revision: u64, binding: u64) -> io::Result<Identity> {
    // Control-side CAS loop keeps the workspace's Rust 1.90 API baseline and
    // latches exhaustion without ever wrapping a session component.
    let mut serial = SESSIONS.load(Ordering::Acquire);
    loop {
        if serial > u32::MAX as u64 {
            return Err(io::Error::other("bridge session counter exhausted"));
        }
        match SESSIONS.compare_exchange(serial, serial + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(current) => serial = current,
        }
    }
    let session = (u64::from(std::process::id()) << 32) | serial;
    if serial > u32::MAX as u64 || token == 0 {
        return Err(io::Error::other("bridge session/token exhausted"));
    }
    Ok(Identity {
        session,
        token,
        revision,
        binding,
    })
}

#[derive(Clone)]
pub struct Launch {
    pub wrapper: crate::wrapper::WrapperOptions,
    pub helper: PathBuf,
    pub plugin: PathBuf,
    pub id: String,
    pub format: String,
    pub approved_binary: crate::paths::PluginFileIdentity,
    pub config: Config,
    pub parameters: Vec<ParameterSpec>,
    pub state: Vec<u8>,
    pub offline: bool,
    pub startup_timeout: Duration,
    pub audio_timeout: Duration,
    pub cancelled: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone)]
pub struct Captured {
    pub state: Vec<u8>,
    pub parameters: Vec<ControlParameter>,
    pub processed_generation: u64,
    /// Inactive state reconciliation, distinct from completed native DSP.
    pub reconciled_generation: u64,
    pub epoch: u64,
}
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub process_id: u32,
    pub failed: bool,
    pub error: Option<String>,
    pub reaped: bool,
}
enum Job {
    Request {
        body: Box<Message>,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        response: mpsc::SyncSender<Result<Packet, String>>,
    },
    Stop,
}
type ReadmissionSnapshot = (Vec<u8>, Vec<DescribedParameter>, crate::PluginLayout);
struct ReadmissionCache {
    request: u64,
    snapshot: ReadmissionSnapshot,
}
struct Inner {
    jobs: mpsc::SyncSender<Job>,
    worker: Mutex<Option<JoinHandle<()>>>,
    status: Arc<Mutex<Status>>,
    signals: Arc<Signals>,
    owner: Owner,
    region: Region,
    last_state: Mutex<Option<(u64, Captured)>>,
    last_readmission: Mutex<Option<ReadmissionCache>>,
}
impl Inner {
    fn stop(&self) {
        // All facade/control references retire away from the callback. Queue
        // fullness cannot prevent stop: disconnect also stops the worker.
        let _ = self.jobs.try_send(Job::Stop);
        self.signals.retired.store(true, Ordering::Release);
        self.signals.failed.store(true, Ordering::Release);
        // Keep the join lock through confirmed exit: concurrent stop callers
        // must not return merely because another caller took the JoinHandle.
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop();
    }
}
#[derive(Clone)]
pub struct Control(Arc<Inner>);
impl Control {
    /// Discover on a fresh helper before any audio submission. Native metadata
    /// and state are queried on the creating owner, never inside the desktop.
    pub fn describe(&self, timeout: Duration) -> Result<ReadmissionSnapshot, String> {
        let packet = self.call(Message::Describe, timeout)?;
        packet.validate().map_err(|error| error.to_string())?;
        match packet.body {
            Message::Described { parameters, layout } => Ok((packet.state, parameters, layout)),
            Message::Error { message } => Err(message),
            _ => Err("unexpected native discovery response".into()),
        }
    }
    pub fn last_valid_readmission(&self) -> Option<ReadmissionSnapshot> {
        self.0
            .last_readmission
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|cache| cache.snapshot.clone())
    }
    pub fn readmission_requested(&self) -> bool {
        self.0.region.readmission_requested()
    }
    pub fn readmit(
        &self,
        epoch: u64,
        pending: &[ParameterSpec],
        timeout: Duration,
    ) -> Result<ReadmissionSnapshot, String> {
        self.check_capture_epoch(epoch)?;
        let packet = self.call(
            Message::Readmit {
                epoch,
                pending: pending.iter().copied().map(Into::into).collect(),
            },
            timeout,
        )?;
        self.check_capture_epoch(epoch)?;
        packet.validate().map_err(|error| error.to_string())?;
        match packet.body {
            Message::Described { parameters, layout } => {
                let mut cache = self
                    .0
                    .last_readmission
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                self.check_capture_epoch(epoch)?;
                if cache
                    .as_ref()
                    .is_none_or(|cache| packet.request > cache.request)
                {
                    *cache = Some(ReadmissionCache {
                        request: packet.request,
                        snapshot: (packet.state.clone(), parameters.clone(), layout.clone()),
                    });
                }
                Ok((packet.state, parameters, layout))
            }
            Message::Error { message } => Err(message),
            _ => Err("unexpected capability re-admission response".into()),
        }
    }
    /// Control/render caller only. Ends this instance and confirms process exit
    /// before returning; audio may retain its mapping for bounded fallback.
    pub fn terminate(&self) -> Status {
        self.0.stop();
        self.status()
    }
    pub fn status(&self) -> Status {
        self.0
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    pub fn owner(&self) -> Owner {
        self.0.owner
    }
    pub fn native_change_serial(&self) -> u64 {
        self.0.region.native_change_serial()
    }
    pub fn last_valid_state(&self) -> Option<Captured> {
        self.0
            .last_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|(_, captured)| captured.clone())
    }
    fn call(&self, body: Message, timeout: Duration) -> Result<Packet, String> {
        if self.0.signals.failed.load(Ordering::Acquire) {
            return Err(self
                .status()
                .error
                .unwrap_or_else(|| "audio helper is unavailable".into()));
        }
        let (response, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or("invalid bridge control deadline")?;
        self.0
            .jobs
            .try_send(Job::Request {
                body: Box::new(body),
                deadline,
                cancelled: cancelled.clone(),
                response,
            })
            .map_err(|_| "audio helper control queue is full or stopped".to_owned())?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Release);
                // Join native work or its confirmed process termination before
                // a session caller can release its recording exclusion guard.
                receiver
                    .recv()
                    .unwrap_or_else(|_| Err("audio helper stopped while cancelling control".into()))
            }
            Err(_) => Err("audio helper control worker stopped".into()),
        }
    }
    pub fn capture(
        &self,
        epoch: u64,
        desired_generation: u64,
        pending: &[ParameterSpec],
        timeout: Duration,
    ) -> Result<Captured, String> {
        self.check_capture_epoch(epoch)?;
        let packet = self.call(
            Message::Capture {
                epoch,
                desired_generation,
                pending: pending.iter().copied().map(Into::into).collect(),
            },
            timeout,
        )?;
        self.check_capture_epoch(epoch)?;
        let captured = match packet.body {
            Message::Captured {
                epoch: reply_epoch,
                processed_generation,
                reconciled_generation,
                parameters,
            } if reply_epoch == epoch
                && processed_generation <= desired_generation
                && reconciled_generation <= desired_generation =>
            {
                if parameters
                    .iter()
                    .any(|parameter| !ParameterSpec::from(*parameter).valid())
                    || packet.state.is_empty()
                {
                    return Err("malformed native capture response".into());
                }
                Captured {
                    epoch,
                    state: packet.state,
                    parameters,
                    processed_generation,
                    reconciled_generation,
                }
            }
            Message::Error { message } => return Err(message),
            _ => return Err("stale or unsupported native capture response".into()),
        };
        self.publish_state(packet.request, captured.clone())?;
        Ok(captured)
    }
    fn check_capture_epoch(&self, epoch: u64) -> Result<(), String> {
        if self
            .0
            .region
            .timeline_epoch()
            .map_err(|error| error.to_string())?
            != epoch
        {
            return Err("capture timeline ownership changed".into());
        }
        Ok(())
    }
    fn publish_state(&self, request: u64, captured: Captured) -> Result<(), String> {
        let mut latest = self
            .0
            .last_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Successful capture linearizes at this Acquire under the cache lock.
        // A subsequent reset does not relabel this explicitly stamped state.
        self.check_capture_epoch(captured.epoch)?;
        if latest
            .as_ref()
            .is_none_or(|(previous, _)| request > *previous)
        {
            *latest = Some((request, captured));
        }
        Ok(())
    }
    pub fn editor(&self, open: bool) -> Result<(), String> {
        self.editor_with_options(open, crate::EditorOptions::default())
    }
    /// Called only after the desktop callback/facade is destroyed. The helper
    /// releases native notes and deactivates on its GUI/native owner, then the
    /// supervisor confirms exit. A nonanswer is contained by the same deadline.
    pub fn shutdown_for_update(&self) -> Result<(), String> {
        let result = self.call(Message::Shutdown, Duration::from_secs(2)).and_then(|packet| {
            match packet.body {
                Message::Stopped => Ok(()),
                Message::Error { message } => Err(message),
                _ => Err("unexpected update shutdown bridge response".into()),
            }
        });
        self.terminate();
        result
    }
    pub fn editor_with_options(
        &self,
        open: bool,
        options: crate::EditorOptions,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let options = crate::gui::bridge_editor::helper_options(options, true);
        options.validate().map_err(|error| error.to_string())?;
        let packet = self.call(Message::Editor { open, options }, Duration::from_secs(2))?;
        match packet.body {
            Message::Error { message } => Err(message),
            Message::Stopped => Ok(()),
            _ => Err("unexpected editor bridge response".into()),
        }
    }
}

struct Process(Child);
impl Process {
    fn reap(&mut self) -> Result<(), String> {
        if self
            .0
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        if let Err(error) = self.0.kill() {
            return if self
                .0
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                Ok(())
            } else {
                Err(format!(
                    "helper termination failed; exit is unconfirmed: {error}"
                ))
            };
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if self
                .0
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("helper termination deadline exceeded; exit is unconfirmed".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.reap();
    }
}

/// Startup is synchronous only on the factory/control caller; callback receives
/// an already negotiated, allocated audio half. Production helper discovery is
/// the desktop executable, not a Cargo target lookup.
pub fn launch(options: Launch) -> Result<(Control, Audio, usize), String> {
    // Supported capability, no clamp: reject before file/listener/map/spawn IO.
    super::auth::check_startup_timeout(options.startup_timeout)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now()
        .checked_add(options.startup_timeout)
        .ok_or("invalid helper startup deadline")?;
    options
        .config
        .validate()
        .map_err(|error| error.to_string())?;
    if crate::paths::plugin_file_identity(&options.plugin).map_err(|error| error.to_string())?
        != options.approved_binary
    {
        return Err("approved native plugin binary changed before bridge launch".into());
    }
    if options.config.native_latency != 0
        || !options.helper.is_file()
        || options.parameters.len() > super::protocol::PARAM_CAPACITY
        || options.startup_timeout.is_zero()
        || options.audio_timeout.is_zero()
    {
        return Err("invalid audio helper launch configuration".into());
    }
    let identity = options.config.identity;
    let name = format!(
        "Local\\Windfall-Audio-{:016x}-{:016x}",
        identity.session, identity.token
    );
    let mapping = Mapping::create(&name).map_err(|error| error.to_string())?;
    let mut region =
        Region::initialize(mapping, options.config).map_err(|error| error.to_string())?;
    let listener =
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let mut command = Command::new(&options.helper);
    command
        .arg("--windfall-audio-helper")
        .arg(
            listener
                .local_addr()
                .map_err(|error| error.to_string())?
                .to_string(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if std::env::var_os("WINDFALL_PLUGIN_HELPER_DIAGNOSTICS").is_some() {
        command.stderr(Stdio::inherit());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let key = super::auth::Key::generate().map_err(|error| error.to_string())?;
    let mut process = Process(command.spawn().map_err(|error| error.to_string())?);
    key.write_to_child(
        &mut process.0,
        identity.session,
        deadline,
        options.cancelled.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let mut socket = super::auth::accept(
        &listener,
        &mut process.0,
        &key,
        deadline,
        options.cancelled.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    socket
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(Some(options.startup_timeout))
        .map_err(|error| error.to_string())?;
    // Private debug test executable only; ordinary production launch keeps the
    // OS socket buffer unchanged. Force a bounded receiver-backpressure test.
    #[cfg(all(windows, debug_assertions))]
    if options.helper.file_stem().and_then(|name| name.to_str())
        == Some("windfall-plugin-audio-backpressure")
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{SO_SNDBUF, SOL_SOCKET, setsockopt};
        let size: i32 = 4096;
        // SAFETY: valid owned socket and correctly sized SO_SNDBUF integer.
        if unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                SOL_SOCKET,
                SO_SNDBUF,
                (&size as *const i32).cast(),
                std::mem::size_of_val(&size) as i32,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    if options
        .cancelled
        .as_deref()
        .is_some_and(|flag| flag.load(Ordering::Acquire))
    {
        return Err("audio helper startup cancelled".into());
    }
    let owner: Owner = identity.into();
    let mut load = Packet::new(
        1,
        owner,
        Message::Load {
            wrapper: Box::new(options.wrapper.clone()),
            mapping: name,
            settings: options.config.into(),
            path: options.plugin.to_string_lossy().into_owned(),
            id: options.id,
            format: options.format,
            approved_binary: options.approved_binary,
            parameters: options.parameters.iter().copied().map(Into::into).collect(),
            offline: options.offline,
        },
    );
    load.state = options.state;
    let _backpressure = load
        .write_bounded(&mut socket, || {
            if options
                .cancelled
                .as_deref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err(std::io::Error::other("audio helper startup cancelled"));
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "audio helper startup deadline exceeded",
                ));
            }
            if process.0.try_wait()?.is_some() {
                return Err(std::io::Error::other(
                    "audio helper process exited during Load",
                ));
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    #[cfg(all(windows, debug_assertions))]
    if options.helper.file_stem().and_then(|name| name.to_str())
        == Some("windfall-plugin-audio-backpressure")
    {
        eprintln!("controlled startup Load: {_backpressure} WouldBlock retries");
    }
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let mut decoder = Decoder::default();
    let ready = wait_reply(
        &mut process,
        &mut socket,
        &mut decoder,
        owner,
        1,
        deadline,
        Cancellation {
            request: options.cancelled.as_deref(),
            retired: None,
        },
    )?;
    if let Message::Error { message } = &ready.body {
        return Err(message.clone());
    }
    let Message::Ready {
        note_instances,
        expression_masks,
        native_latency,
        tail,
        parameters,
    } = ready.body
    else {
        return Err("audio helper did not negotiate activation".into());
    };
    let parameters: Vec<ParameterSpec> = parameters.into_iter().map(Into::into).collect();
    region
        .negotiate_latency(native_latency as usize)
        .map_err(|error| error.to_string())?;
    let signals = Arc::new(Signals::default());
    let mut audio = Audio::new(region.clone(), signals.clone(), &parameters)
        .map_err(|error| error.to_string())?;
    audio
        .configure_note_capabilities(note_instances, expression_masks)
        .map_err(|error| error.to_string())?;
    let status = Arc::new(Mutex::new(Status {
        process_id: process.0.id(),
        ..Default::default()
    }));
    let (jobs, incoming) = mpsc::sync_channel(8);
    let worker_status = status.clone();
    let worker_signals = signals.clone();
    let control_region = region.clone();
    let worker = std::thread::Builder::new()
        .name("plugin-bridge-supervisor".into())
        .spawn(move || {
            let mut terminal_reply = None;
            let result = supervise(
                &mut process,
                &mut socket,
                &mut decoder,
                owner,
                incoming,
                Supervision {
                    region,
                    signals: worker_signals.clone(),
                    audio_timeout: options.audio_timeout,
                },
                &mut terminal_reply,
            );
            worker_signals.failed.store(true, Ordering::Release);
            let exit = process.reap();
            let mut status = worker_status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            status.reaped = exit.is_ok();
            if let Err(error) = result.and(exit) {
                status.failed = true;
                status.error = Some(error);
            }
            drop(status);
            // Terminal native work must be reaped before its caller releases
            // recording/replacement exclusion. Healthy queued cancellations
            // still reply in supervise without terminating the owner.
            if let Some((response, error)) = terminal_reply {
                let _ = response.send(Err(error));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok((
        Control(Arc::new(Inner {
            jobs,
            worker: Mutex::new(Some(worker)),
            status,
            signals,
            owner,
            region: control_region,
            last_state: Mutex::new(None),
            last_readmission: Mutex::new(None),
        })),
        audio,
        tail.min(usize::MAX as u64) as usize,
    ))
}

#[derive(Default)]
struct Cancellation<'a> {
    request: Option<&'a AtomicBool>,
    retired: Option<&'a AtomicBool>,
}
impl Cancellation<'_> {
    fn requested(&self) -> bool {
        [self.request, self.retired]
            .into_iter()
            .flatten()
            .any(|flag| flag.load(Ordering::Acquire))
    }
}
fn wait_reply(
    process: &mut Process,
    socket: &mut TcpStream,
    decoder: &mut Decoder,
    owner: Owner,
    request: u64,
    deadline: Instant,
    cancellation: Cancellation<'_>,
) -> Result<Packet, String> {
    loop {
        if Instant::now() >= deadline || cancellation.requested() {
            process.reap()?;
            return Err("audio helper control deadline exceeded; process terminated".into());
        }
        let step = decoder
            .poll_step(socket)
            .map_err(|error| error.to_string())?;
        // A bounded read/decode can cross the deadline or cancellation edge.
        if Instant::now() >= deadline || cancellation.requested() {
            process.reap()?;
            return Err("audio helper control deadline exceeded; process terminated".into());
        }
        if let ReadStep::Packet(packet) = step {
            if packet.request != request || packet.owner != owner {
                return Err("stale audio helper control response".into());
            }
            return Ok(*packet);
        }
        if process
            .0
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("audio helper process exited".into());
        }
        if matches!(step, ReadStep::Idle) {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
}
struct Supervision {
    region: Region,
    signals: Arc<Signals>,
    audio_timeout: Duration,
}
type TerminalReply = Option<(mpsc::SyncSender<Result<Packet, String>>, String)>;
fn supervise(
    process: &mut Process,
    socket: &mut TcpStream,
    decoder: &mut Decoder,
    owner: Owner,
    incoming: mpsc::Receiver<Job>,
    context: Supervision,
    terminal_reply: &mut TerminalReply,
) -> Result<(), String> {
    let Supervision {
        region,
        signals,
        audio_timeout,
    } = context;
    let mut request = 1u64;
    let mut watchdog = OwnerWatchdog::default();
    loop {
        if signals.retired.load(Ordering::Acquire) {
            return Ok(());
        }
        if signals.failed.load(Ordering::Acquire) || region.helper_failed() {
            return Err("audio helper or callback latched a processing failure".into());
        }
        if process
            .0
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("audio helper process exited".into());
        }
        match incoming.try_recv() {
            Ok(Job::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Ok(Job::Request {
                body,
                deadline,
                cancelled,
                response,
            }) => {
                if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                    let _ = response.send(Err("queued audio helper control cancelled".into()));
                    continue;
                }
                request = request
                    .checked_add(1)
                    .ok_or("bridge request counter exhausted")?;
                socket
                    .set_nonblocking(false)
                    .map_err(|error| error.to_string())?;
                socket
                    .set_write_timeout(Some(deadline.saturating_duration_since(Instant::now())))
                    .map_err(|error| error.to_string())?;
                let write = Packet::new(request, owner, *body)
                    .write(socket)
                    .map_err(|error| error.to_string());
                socket
                    .set_nonblocking(true)
                    .map_err(|error| error.to_string())?;
                let result = write.and_then(|()| {
                    wait_reply(
                        process,
                        socket,
                        decoder,
                        owner,
                        request,
                        deadline,
                        Cancellation {
                            request: Some(&cancelled),
                            retired: Some(&signals.retired),
                        },
                    )
                });
                match result {
                    Ok(packet) => {
                        let _ = response.send(Ok(packet));
                    }
                    Err(error) => {
                        *terminal_reply = Some((response, error.clone()));
                        return Err(error);
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if watchdog.stalled(
            region.owner_completions(),
            region.pending_sequence().is_some(),
            Instant::now(),
            audio_timeout,
        ) {
            return Err("audio helper stalled; process terminated".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Once work arms the watchdog, slot turnover/seek cancellation cannot erase
/// its age. Only actual native-owner completion releases that stall obligation.
#[derive(Default)]
struct OwnerWatchdog {
    observed: u32,
    started: Option<Instant>,
}
impl OwnerWatchdog {
    fn stalled(&mut self, completions: u32, pending: bool, now: Instant, limit: Duration) -> bool {
        if completions != self.observed {
            self.observed = completions;
            self.started = pending.then_some(now);
        } else if self.started.is_none() && pending {
            self.started = Some(now);
        }
        self.started
            .is_some_and(|started| now.duration_since(started) >= limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    #[test]
    fn watchdog_arms_only_work_and_completion_disarms_each_obligation() {
        let now = Instant::now();
        let limit = Duration::from_millis(10);
        let mut watchdog = OwnerWatchdog::default();
        assert!(!watchdog.stalled(0, false, now, limit));
        assert!(!watchdog.stalled(0, false, now + limit * 10, limit));
        assert!(!watchdog.stalled(0, true, now + limit * 11, limit));
        assert!(!watchdog.stalled(0, false, now + limit * 11 + limit / 2, limit));
        assert!(watchdog.stalled(0, true, now + limit * 12, limit));
        assert!(!watchdog.stalled(1, false, now + limit * 13, limit));
        assert!(!watchdog.stalled(1, false, now + limit * 20, limit));
        assert!(!watchdog.stalled(1, true, now + limit * 21, limit));
        assert!(watchdog.stalled(1, false, now + limit * 22, limit));
    }
    fn test_control() -> Control {
        let (jobs, _incoming) = mpsc::sync_channel(8);
        Control(Arc::new(Inner {
            jobs,
            worker: Mutex::new(None),
            status: Arc::new(Mutex::new(Status::default())),
            signals: Arc::new(Signals::default()),
            owner: Owner {
                session: 1,
                token: 2,
                revision: 3,
                binding: 4,
            },
            region: Region::initialize(
                super::super::slots::LocalWords::new(),
                Config {
                    identity: Identity {
                        session: 1,
                        token: 2,
                        revision: 3,
                        binding: 4,
                    },
                    sample_rate: 48_000,
                    block: 64,
                    native_latency: 0,
                    kind: super::super::protocol::Kind::Effect,
                },
            )
            .unwrap(),
            last_state: Mutex::new(None),
            last_readmission: Mutex::new(None),
        }))
    }
    fn captured(value: f64, epoch: u64) -> Captured {
        Captured {
            state: crate::PluginState::native(&value.to_le_bytes()).into_bytes(),
            parameters: vec![],
            processed_generation: 1,
            reconciled_generation: 0,
            epoch,
        }
    }
    #[test]
    fn reset_after_helper_reply_refuses_stale_cache_publication() {
        let control = test_control();
        let old = captured(0.25, 1);
        control.publish_state(1, old.clone()).unwrap();
        // Caller has a valid helper reply; reset precedes the final host check.
        control.0.region.publish_timeline_epoch(1, 2).unwrap();
        assert!(control.check_capture_epoch(1).is_err());
        assert!(control.publish_state(2, captured(0.75, 1)).is_err());
        assert_eq!(control.last_valid_state().unwrap().state, old.state);
        for epoch in [0, 1, 3, u64::MAX] {
            assert!(
                control
                    .capture(epoch, 1, &[], Duration::from_secs(1))
                    .is_err()
            );
        }
        assert_eq!(control.0.region.timeline_epoch(), Ok(2));
    }
    #[test]
    fn reset_during_cache_lock_wait_refuses_older_caller_without_replacing_state() {
        let control = test_control();
        let old = captured(0.25, 1);
        control.publish_state(1, old.clone()).unwrap();
        let lock = control.0.last_state.lock().unwrap();
        let gate = Arc::new(Barrier::new(2));
        let other = control.clone();
        let other_gate = gate.clone();
        let caller = std::thread::spawn(move || {
            other.check_capture_epoch(1).unwrap(); // Reply-side check succeeds.
            other_gate.wait();
            other.publish_state(2, captured(0.75, 1))
        });
        gate.wait();
        control.0.region.publish_timeline_epoch(1, 2).unwrap();
        drop(lock);
        assert!(caller.join().unwrap().is_err());
        assert_eq!(control.last_valid_state().unwrap().state, old.state);
    }
    #[test]
    fn capture_before_reset_retains_its_old_epoch_without_relabeling_the_cache() {
        let control = test_control();
        control.publish_state(1, captured(0.5, 1)).unwrap();
        control.0.region.publish_timeline_epoch(1, 2).unwrap();
        assert_eq!(control.last_valid_state().unwrap().epoch, 1);
        let mut current = captured(0.75, 2);
        current.processed_generation = 0;
        control.publish_state(2, current.clone()).unwrap();
        assert!(control.publish_state(3, captured(0.25, 1)).is_err());
        let cached = control.last_valid_state().unwrap();
        assert_eq!(cached.epoch, 2);
        assert_eq!(cached.processed_generation, 0);
        assert_eq!(cached.state, current.state);
    }
    #[test]
    fn reversed_caller_publication_cannot_replace_a_newer_capture() {
        let control = test_control();
        let captured = |value: f64| Captured {
            state: crate::PluginState::native(&value.to_le_bytes()).into_bytes(),
            parameters: vec![],
            processed_generation: 1,
            reconciled_generation: 0,
            epoch: 1,
        };
        let old = captured(0.25);
        let newer = captured(0.75);
        let gate = Arc::new(Barrier::new(2));
        let other = control.clone();
        let other_gate = gate.clone();
        let caller = std::thread::spawn(move || {
            other_gate.wait(); // Caller A received request1, then pauses.
            other_gate.wait(); // Caller B has published request2.
            other.publish_state(1, old).unwrap();
        });
        gate.wait();
        control.publish_state(2, newer.clone()).unwrap();
        gate.wait();
        caller.join().unwrap();
        assert_eq!(control.last_valid_state().unwrap().state, newer.state);
    }
}
