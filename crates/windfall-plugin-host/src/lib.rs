//! Hosts third-party audio plugins for Windfall.
//!
//! The crate does four jobs, in the order a plugin goes through them:
//!
//! 1. **Find and scan.** [`paths`] knows where plugins are installed.
//!    [`scan`] starts the `windfall-plugin-scan` program on each file and
//!    keeps what it reports in a [`PluginCatalog`], so a plugin that
//!    crashes or hangs costs a scanner process and not the app.
//! 2. **Create and set up.** A [`PluginHost`] loads a file as a
//!    [`PluginModule`] and creates [`PluginInstance`]s from it. An instance
//!    is the plugin's main-thread half: parameters, state, the editor
//!    window.
//! 3. **Process.** [`PluginInstance::activate`] returns a
//!    [`PluginProcessor`], the audio-thread half, with every buffer
//!    allocated. It takes notes, parameter changes and the transport, and
//!    processes blocks without allocating, locking or blocking.
//! 4. **Fit the engine.** [`PluginEffect`] and [`PluginInstrument`] wrap a
//!    processor in the operations the engine already uses for the built-in
//!    effects and instruments.
//!
//! CLAP and VST3 effects and instruments are hosted. VST3 module loading
//! supports Windows and Linux; Windows audio and native editors have been
//! exercised here. VST3 state operations require returning the processor
//! before saving or restoring. `docs/plugins/host-evaluation.md`
//! says what each format can do today and why the libraries underneath
//! were chosen.
//!
//! # Threads
//!
//! There are two, and the types keep them apart.
//!
//! - The **main thread** is the one the [`PluginHost`] was made on. Every
//!   [`PluginModule`] and [`PluginInstance`] stays there: the types are not
//!   `Send`. Plugins require this, and on macOS it has to be the thread
//!   the app's windows live on. That thread calls
//!   [`PluginInstance::idle`] regularly.
//! - The **audio thread** owns the [`PluginProcessor`]. It is `Send` so it
//!   can be handed over, and it takes `&mut self` everywhere so it cannot
//!   be used from two threads at once.
//!
//! The two halves talk through lock-free queues. A parameter set on the
//! main thread arrives as an event at the start of the next block. A
//! parameter the user moves in the plugin's editor comes back out of
//! [`PluginInstance::idle`] as a [`PluginNotification`].
//!
//! This is the engine's own rule for processors: built and prepared away
//! from the audio thread, handed over ready, and sent back to be dropped.
//!
//! # What a plugin can still do wrong
//!
//! A plugin's code runs inside the app. The host's guarantees are about
//! the host's own code. [`containment`] lists what is done about a plugin
//! that misbehaves: output that is not a number is replaced by silence, a
//! block that takes too long is counted in [`PluginHealth`], and a plugin
//! that reports a failure is not called again. A plugin that crashes takes
//! the process down. Scanner and separate audio helpers contain those failures;
//! direct in-process instances retain this limitation.
//!
//! # Moving a plugin to another process
//!
//! [`bridge`] provides Windows audio/state helper transport, selected by the
//! desktop's saved wrapper policy. Native helper editors and bounded note-owner
//! recovery use the creating helper owner. The 2026-10-11 additions are source
//! candidates with authored, unexecuted tests; see the implementation hand-off.
//! In-process instances created by the APIs above keep their existing crash
//! and hang limitations. A separate helper is not a security sandbox.
//!
//! Each format implements two crate-private traits: one for the main-thread
//! half and one, `ProcessorBackend`, for the audio thread. A backend for a
//! plugin in a helper process would implement the same two. What would
//! cross the process boundary is already plain data:
//!
//! - **Audio**: one block of at most `max_block` frames per channel. The
//!   helper maps a shared memory region sized at activation. The audio
//!   thread publishes the input there and reads the previous completed
//!   block without waiting. The fixed adapter adds two internal blocks of
//!   latency (collection plus one helper scheduling interval), independently
//!   of irregular callback sizes, in addition to declared native latency. A
//!   helper that misses the deadline yields silence/bypass and a failure
//!   flag; a synchronous wait would violate the engine's no-blocking rule.
//! - **Events**: [`HostEvent`] going in and [`PluginEvent`] coming out are
//!   `Copy` and hold no pointers, so a block's events are two fixed arrays
//!   in the same region. [`Transport`] is the same.
//! - **Main-thread calls** (parameters, text, state, editor): requests and
//!   replies over a pipe. State is bytes already. The editor's window
//!   would belong to the helper, parented to a window of the app.
//!
//! What stays hard is accounting for the pipeline latency and plugin
//! editors that expect to share a thread with their host window.

pub mod au_model;
pub mod bridge;
pub mod containment;
pub mod export_runtime;
pub mod gui;
pub mod ownership;
pub mod paths;
pub mod scan;

mod adapter;
mod check;
mod clap;
mod descriptor;
mod error;
mod events;
mod host;
mod instance;
mod params;
mod processor;
mod state;
mod vst3;

pub use adapter::{PluginEffect, PluginInstrument, PluginNoteControls, PluginNoteExpression};
pub use check::{CheckReport, CheckStep, StepOutcome, check_in_process, check_plugin};
pub use containment::PluginHealth;
pub use descriptor::{
    AudioPort, MAX_PARAMETERS, MAX_PORT_CHANNELS, MAX_PORTS, PluginDescriptor, PluginFormat,
    PluginKind, PluginLayout,
};
pub use error::{DeactivationError, PluginError};
pub use events::{
    FIRST_NOTE_INSTANCE_ID, HostEvent, MAX_NOTE_INSTANCES, MeterAnchor, NoteExpressionKind,
    PluginEvent, Transport,
};
pub use gui::{EditorError, EditorInfo, EditorOptions};
pub use host::{LogLevel, PluginHost, PluginModule};
pub use instance::{PluginInstance, PluginNotification};
pub use params::{MAX_LISTED_CHOICES, PluginParam, PluginParamChoice, PluginParamInfo};
pub use processor::{EVENT_CAPACITY, PluginProcessor, ProcessStatus};
pub use scan::{PluginCatalog, ProcessRunner, ScanRunner, scan_file};
pub use state::{InvalidState, MAX_STATE_BYTES, PluginState};

#[cfg(target_os = "macos")]
pub mod au;

mod threaded;
pub mod wrapper;

pub mod outputs;
pub mod project_expression;
