//! What a CLAP plugin can call back into: the host's side of the
//! extensions.
//!
//! A plugin may call these from any thread, at any time, whatever the
//! specification says about which thread each belongs to. So none of them
//! does any work. Each one sets a flag or stores a number, and the instance
//! acts on it in its next `idle` on the main thread. The two exceptions are
//! timers, which keep a list, and that list is only touched after checking
//! that the call really is on the main thread.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortRescanFlags, HostAudioPorts, HostAudioPortsImpl};
use clack_extensions::gui::{GuiSize, HostGui, HostGuiImpl};
use clack_extensions::latency::{HostLatency, HostLatencyImpl};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_ports::{
    HostNotePorts, HostNotePortsImpl, NoteDialects, NotePortRescanFlags,
};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
};
use clack_extensions::state::{HostState, HostStateImpl};
use clack_extensions::tail::{HostTail, HostTailImpl};
use clack_extensions::thread_check::{HostThreadCheck, HostThreadCheckImpl};
use clack_extensions::timer::{HostTimer, HostTimerImpl, TimerId};
use clack_host::prelude::*;

use crate::host::{LogLevel, LogSink};

/// What a plugin has asked for since the instance last looked.
pub(crate) mod flag {
    pub const CALLBACK: u32 = 1 << 0;
    pub const RESTART: u32 = 1 << 1;
    pub const FLUSH: u32 = 1 << 2;
    pub const LATENCY: u32 = 1 << 3;
    pub const TAIL: u32 = 1 << 4;
    pub const STATE_DIRTY: u32 = 1 << 5;
    pub const PARAMS: u32 = 1 << 6;
    pub const GUI_CLOSED: u32 = 1 << 7;
    pub const GUI_RESIZE: u32 = 1 << 8;
    pub const ALL: u32 = u32::MAX;
}

thread_local! {
    /// Stands for the thread it is read on: its address is different on
    /// every living thread.
    static THREAD_MARK: u8 = const { 0 };
    /// Set on a thread while it is inside a plugin's audio call.
    static IN_AUDIO_CALL: Cell<bool> = const { Cell::new(false) };
}

/// A number that is the same for every call on one thread and differs
/// between threads that are alive at the same time.
fn thread_token() -> usize {
    THREAD_MARK.with(|mark| std::ptr::from_ref(mark) as usize)
}

/// Marks the current thread as being inside a plugin's audio call for as
/// long as it lives.
pub(crate) struct AudioCall;

impl AudioCall {
    #[inline]
    pub fn enter() -> Self {
        IN_AUDIO_CALL.set(true);
        Self
    }
}

impl Drop for AudioCall {
    #[inline]
    fn drop(&mut self) {
        IN_AUDIO_CALL.set(false);
    }
}

/// The host's thread-safe state for one plugin. The plugin can reach it
/// from any thread.
pub(crate) struct Shared {
    main_thread: usize,
    flags: AtomicU32,
    /// The editor size the plugin last asked for, packed.
    requested_size: AtomicU64,
    /// Whether an editor is open, so a resize request can be answered.
    pub editor_open: AtomicBool,
    log: LogSink,
}

impl Shared {
    /// Must be called on the thread that will own the instance.
    pub fn new(log: LogSink) -> Self {
        Self {
            main_thread: thread_token(),
            flags: AtomicU32::new(0),
            requested_size: AtomicU64::new(0),
            editor_open: AtomicBool::new(false),
            log,
        }
    }

    fn raise(&self, bits: u32) {
        self.flags.fetch_or(bits, Ordering::AcqRel);
    }

    /// Returns which of `bits` were set, and clears them.
    pub fn take(&self, bits: u32) -> u32 {
        self.flags.fetch_and(!bits, Ordering::AcqRel) & bits
    }

    pub fn on_main_thread(&self) -> bool {
        thread_token() == self.main_thread
    }

    pub fn requested_size(&self) -> (u32, u32) {
        let size = GuiSize::unpack_from_u64(self.requested_size.load(Ordering::Acquire));
        (size.width, size.height)
    }
}

impl<'a> SharedHandler<'a> for Shared {
    fn request_restart(&self) {
        self.raise(flag::RESTART);
    }

    fn request_process(&self) {
        // The engine processes every active plugin on every block.
    }

    fn request_callback(&self) {
        self.raise(flag::CALLBACK);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        // Passing a message on can allocate and block, which the audio
        // thread must not do on the host's account.
        if IN_AUDIO_CALL.get() {
            return;
        }
        let level = match severity {
            LogSeverity::Debug => LogLevel::Debug,
            LogSeverity::Info => LogLevel::Info,
            LogSeverity::Warning => LogLevel::Warning,
            _ => LogLevel::Error,
        };
        (self.log)(level, message);
    }
}

impl HostThreadCheckImpl for Shared {
    fn is_main_thread(&self) -> bool {
        self.on_main_thread()
    }

    fn is_audio_thread(&self) -> bool {
        IN_AUDIO_CALL.get()
    }
}

impl HostParamsImplShared for Shared {
    fn request_flush(&self) {
        self.raise(flag::FLUSH);
    }
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        if !self.editor_open.load(Ordering::Acquire) || new_size.width == 0 || new_size.height == 0
        {
            return Err(HostError::Message("no editor is open"));
        }
        self.requested_size
            .store(new_size.pack_to_u64(), Ordering::Release);
        self.raise(flag::GUI_RESIZE);
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Err(HostError::Message("the user opens editors, not the plugin"))
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Err(HostError::Message(
            "the user closes editors, not the plugin",
        ))
    }

    fn closed(&self, _was_destroyed: bool) {
        self.raise(flag::GUI_CLOSED);
    }
}

/// The shortest period a plugin's timer may have. Faster than the screen
/// refreshes gains nothing.
const SHORTEST_TIMER: Duration = Duration::from_millis(8);
/// The most timers one plugin may have running.
const MAX_TIMERS: usize = 64;

struct Timer {
    id: u32,
    period: Duration,
    due: Instant,
}

/// The host's main-thread state for one plugin.
pub(crate) struct MainThread<'a> {
    shared: &'a Shared,
    timers: RefCell<Vec<Timer>>,
    next_timer: Cell<u32>,
}

impl<'a> MainThread<'a> {
    pub fn new(shared: &'a Shared) -> Self {
        Self {
            shared,
            timers: RefCell::new(Vec::new()),
            next_timer: Cell::new(1),
        }
    }

    /// Appends the timers that are due at `now` to `due` and schedules
    /// their next ticks. A timer that fell behind skips the ticks it
    /// missed.
    pub fn collect_due(&self, now: Instant, due: &mut Vec<u32>) {
        for timer in self.timers.borrow_mut().iter_mut() {
            if timer.due <= now {
                due.push(timer.id);
                timer.due = (timer.due + timer.period).max(now);
            }
        }
    }

    /// When the earliest timer is due.
    pub fn next_due(&self) -> Option<Instant> {
        self.timers.borrow().iter().map(|timer| timer.due).min()
    }
}

impl<'a> MainThreadHandler<'a> for MainThread<'a> {}

impl HostTimerImpl for MainThread<'_> {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        if !self.shared.on_main_thread() {
            return Err(HostError::Message("timers belong to the main thread"));
        }
        let mut timers = self.timers.borrow_mut();
        if timers.len() >= MAX_TIMERS {
            return Err(HostError::Message("too many timers"));
        }
        let id = self.next_timer.get();
        self.next_timer.set(id.wrapping_add(1).max(1));
        let period = Duration::from_millis(u64::from(period_ms)).max(SHORTEST_TIMER);
        timers.push(Timer {
            id,
            period,
            due: Instant::now() + period,
        });
        Ok(TimerId(id))
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        if !self.shared.on_main_thread() {
            return Err(HostError::Message("timers belong to the main thread"));
        }
        let mut timers = self.timers.borrow_mut();
        let before = timers.len();
        timers.retain(|timer| timer.id != timer_id.0);
        if timers.len() < before {
            Ok(())
        } else {
            Err(HostError::Message("no such timer"))
        }
    }
}

impl HostParamsImplMainThread for MainThread<'_> {
    fn rescan(&self, _flags: ParamRescanFlags) {
        self.shared.raise(flag::PARAMS);
    }

    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostStateImpl for MainThread<'_> {
    fn mark_dirty(&self) {
        self.shared.raise(flag::STATE_DIRTY);
    }
}

impl HostLatencyImpl for MainThread<'_> {
    fn changed(&self) {
        self.shared.raise(flag::LATENCY);
    }
}

impl HostAudioPortsImpl for MainThread<'_> {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        true
    }

    fn rescan(&self, _flags: AudioPortRescanFlags) {
        // The host reads ports when it creates a plugin. A plugin whose
        // ports changed gets them read again by being created again.
        self.shared.raise(flag::RESTART);
    }
}

impl HostNotePortsImpl for MainThread<'_> {
    fn supported_dialects(&self) -> NoteDialects {
        NoteDialects::CLAP | NoteDialects::MIDI
    }

    fn rescan(&self, _flags: NotePortRescanFlags) {
        self.shared.raise(flag::RESTART);
    }
}

/// The host's audio-thread state for one active plugin.
pub(crate) struct AudioThread<'a> {
    shared: &'a Shared,
}

impl<'a> AudioThread<'a> {
    pub fn new(shared: &'a Shared) -> Self {
        Self { shared }
    }
}

impl<'a> AudioProcessorHandler<'a> for AudioThread<'a> {}

impl HostTailImpl for AudioThread<'_> {
    fn changed(&mut self) {
        self.shared.raise(flag::TAIL);
    }
}

/// Ties the three handlers together and lists the extensions the host
/// offers to plugins.
pub(crate) struct WindfallHost;

// clack-extensions =0.2.0 publishes HostNotePorts through a double reference
// (&&clap_host_note_ports), exposing a pointer-to-pointer as the C vtable.
// Register an ABI-correct local table using the same public owner dispatcher.
// Keep the pinned dependency unchanged; native rescan tests exercise this table.
#[derive(Clone, Copy)]
struct NativeNotePorts;
// SAFETY: identifier and side match the raw CLAP host-note-ports ABI below.
unsafe impl clack_host::extensions::Extension for NativeNotePorts {
    const IDENTIFIERS: &'static [&'static std::ffi::CStr] =
        <HostNotePorts as clack_host::extensions::Extension>::IDENTIFIERS;
    type ExtensionSide =
        <HostNotePorts as clack_host::extensions::Extension>::ExtensionSide;
    unsafe fn from_raw(_: clack_host::extensions::RawExtension<Self::ExtensionSide>) -> Self { Self }
}
// SAFETY: a single static reference to the exact C struct, with matching callbacks.
unsafe impl clack_host::extensions::ExtensionImplementation<WindfallHost> for NativeNotePorts {
    const IMPLEMENTATION: clack_host::extensions::RawExtensionImplementation =
        clack_host::extensions::RawExtensionImplementation::new(
            &clap_sys::ext::note_ports::clap_host_note_ports {
                supported_dialects: Some(native_note_dialects),
                rescan: Some(native_note_rescan),
            },
        );
}
unsafe extern "C" fn native_note_dialects(host: *const clap_sys::host::clap_host) -> u32 {
    // SAFETY: the public dispatcher validates the CLAP host and owner access.
    unsafe { clack_host::extensions::wrapper::HostWrapper::<WindfallHost>::handle_main_thread(host, |state| Ok(state.supported_dialects().bits())) }.unwrap_or(0)
}
unsafe extern "C" fn native_note_rescan(host: *const clap_sys::host::clap_host, flags: u32) {
    // SAFETY: the public dispatcher validates the CLAP host and owner access.
    let _ = unsafe { clack_host::extensions::wrapper::HostWrapper::<WindfallHost>::handle_main_thread(host, |state| {
        HostNotePortsImpl::rescan(state, NotePortRescanFlags::from_bits_truncate(flags));
        Ok(())
    }) };
}

impl HostHandlers for WindfallHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;
    type AudioProcessor<'a> = AudioThread<'a>;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostThreadCheck>()
            .register::<HostParams>()
            .register::<HostState>()
            .register::<HostLatency>()
            .register::<HostTail>()
            .register::<HostGui>()
            .register::<HostTimer>()
            .register::<HostAudioPorts>()
            .register::<NativeNotePorts>();
    }
}

/// Lets the log sink be shared without naming its type everywhere.
pub(crate) fn silent_log() -> LogSink {
    Arc::new(|_, _| {})
}
