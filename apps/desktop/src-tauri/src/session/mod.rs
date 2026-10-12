//! The session: the one open project, the engine that plays it, and every
//! operation the UI can ask for.
//!
//! Nothing here knows about Tauri. The app hands the session an
//! [`EventSink`] that emits to its windows and calls one method per IPC
//! command; tests hand it a sink that records and an engine with no device.
//!
//! # Locks
//!
//! The document sits behind one mutex, and every method takes it for as long
//! as an edit takes and no longer. Whatever is slow, such as decoding audio,
//! reading and writing files, rendering an export or opening an audio
//! device, runs with the document lock released, on the thread of the
//! caller: the app calls those methods from worker threads and the quick
//! ones from anywhere. Events are emitted while the lock that guards their
//! subject is held, which is what keeps them in order.
//!
//! A thread takes the locks it needs in this order and never the other way
//! round:
//!
//! 1. `save` or `configuring`. Each puts one kind of slow work in a queue:
//!    `save` is held from before a save or a backup copies the project until
//!    its file is written and the document is marked, so files reach the
//!    disk in the order their copies were taken; `configuring` is held while
//!    the audio device is reopened. Nothing is held when either is taken,
//!    and no thread holds both.
//! 2. `recording`, before `state`, for capture exclusion and final admission.
//! 3. `state`, the document lock.
//! 4. `transport`, the sample cache and the engine's controller, each for a
//!    moment, under `state` or alone.
//!
//! `preview` is held only around handing a preview to the controller.
//! `status`, `settings` and `subscribers` are never held together with
//! `state` or with each other. The settings file is written under
//! `settings`, which is why that lock stays clear of the document.
//!
//! Who takes what:
//!
//! - Checked edits/history/imports: recording -> State for a snapshot, no
//!   recording/State/controller guard during native/DSP preparation, then
//!   recording -> State -> borrowed controller lease before Document mutation.
//!   Ready install is infallible; candidate/old-state retirement follows guards.
//! - Transport: `state`, then `transport`.
//! - Save and backup (the autosave thread): `save`, then `state` to copy
//!   the project, nothing but `save` while the file is written, then
//!   `state` again; `settings` once `save` is released.
//! - New and open: `state` to take a ticket, no lock while the file is read
//!   and decoded/prepared, then recording -> State -> ready lease to swap the
//!   project in; retirement and `settings` after admission guards.
//! - The sample loader, file imports and sample reload: no lock while
//!   decoding/preparing, then recording -> State -> borrowed ready lease.
//! - Export: `state` to copy the project, then no lock at all on its own
//!   thread.
//! - The realtime thread: captures and releases bounded retirement alone, then
//!   `subscribers`, then `transport`, and once a second
//!   `status`. It never takes `state`, so no slow edit can stall the meters.
//! - Reopening the audio device: `configuring` throughout, `settings`, then
//!   `state` to hand the project to the new stream, then `status`.
//!
//! # Work that outlives its document
//!
//! Slow work is done for the document that was open when it started. Each
//! document has a number, `generation`, and everything that works with no
//! lock held and then touches the document or the sample pool compares it
//! first and gives up if another document has taken its place.

mod analysis_jobs;
mod analyzers;
mod archive;
mod assistant_connection;
mod audio;
mod audio_editor;
mod preset_assets;
mod audio_logger;
mod autosave;
mod capture_expression;
mod clip_processing;
mod cloud_backup;
mod consolidate;
mod controller_capture;
mod controllers;
mod derived_assets;
mod edit;
mod export;
mod files;
mod flp;
mod input_monitor;
mod library;
mod loop_content;
mod midi;
mod midi_hardware;
mod mixer_presets;
mod pattern_movement;
mod performance_media;
mod phone_remote;
mod playlist_bounce;
mod playlist_export;
mod preparation;
mod presets;
mod realtime;
mod recording;
mod sampler_preprocess;
mod sampler_processing;
mod samples;
mod scores;
mod scripting;
mod slicer;
mod templates;
#[cfg(test)]
mod tests;
mod timeline;
mod transport;
mod versions;
mod window_document;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use windfall_engine::{Controller, Engine, SamplePool};
use windfall_ipc::{AudioHost, AudioSettings, EngineStatus, TransportState};
use windfall_project::{Document, SampleId};

use crate::events::{Event, EventSink};
use crate::samples::SampleCache;
use crate::settings::SettingsStore;
use crate::sync::lock;
use crate::template::default_project;

pub use analyzers::{ACK_TIMEOUT, AUDIO_TIMEOUT, AnalyzerSender};
pub use audio::openable;
pub use autosave::{AUTOSAVE_INTERVAL, backup_timestamp};
pub use files::{EDITED_WHILE_MOVING, NO_FILE_YET};
pub use library::{ClipPlace, PROJECT_REPLACED};
pub use mixer_presets::{MixerPresetLibrary, MixerPresetLibraryEntry, MixerPresetPluginRef};
pub use realtime::{FRAME_INTERVAL, FrameSender};
pub use transport::{EMPTY_PLAYLIST, MUTED_PLAYLIST, SILENT_PLAYLIST};
pub use window_document::WindowDocumentSnapshot;

/// The audio output as the session needs it. [`Engine`] is the real one.
pub trait AudioDevice: Send + Sync {
    fn status(&self) -> EngineStatus;
    /// Closes the stream and opens one with new settings. May take seconds.
    fn reconfigure(&self, settings: &AudioSettings);
    /// Lists the hosts and devices of this machine. May take a while.
    fn devices(&self) -> Vec<AudioHost>;
}

impl AudioDevice for Engine {
    fn status(&self) -> EngineStatus {
        Engine::status(self)
    }

    fn reconfigure(&self, settings: &AudioSettings) {
        Engine::reconfigure(self, settings);
    }

    fn devices(&self) -> Vec<AudioHost> {
        Engine::devices()
    }
}

/// What a session is built from.
pub struct SessionConfig {
    /// Drives the engine that `audio` feeds to a device.
    pub controller: Controller,
    pub audio: Arc<dyn AudioDevice>,
    pub events: Arc<dyn EventSink>,
    /// The folder that holds the content shipped with Windfall.
    pub factory_dir: PathBuf,
    pub settings: SettingsStore,
}

/// A handle to the session. Clones share it.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

/// A handle that does not keep the session alive, for background threads.
pub struct WeakSession {
    inner: Weak<Inner>,
}

impl WeakSession {
    pub fn upgrade(&self) -> Option<Session> {
        self.inner.upgrade().map(|inner| Session { inner })
    }
}

struct Inner {
    /// State's generation mirrored only at committed replacement for event emission.
    window_document_generation: std::sync::atomic::AtomicU64,
    /// Transient timeline watermark, accessed under State and retained across replacement.
    timeline_request: std::sync::atomic::AtomicU64,
    /// Successful guarded Play owner; accessed only while State is held.
    timeline_play_request: std::sync::atomic::AtomicU64,
    archive_job: Mutex<Option<Arc<AtomicBool>>>,
    extractions: Mutex<extractions::Extractions>,
    backup_job: Mutex<Option<Arc<AtomicBool>>>,
    phone_remote: Mutex<Option<phone_remote::Server>>,
    analysis: analysis_jobs::Service,
    /// Live analyzer slots and captures. Locked after `state`, before the
    /// engine's controller; the realtime thread takes it without `state`.
    analyzers: analyzers::Service,
    midi_hardware: Mutex<Option<Arc<windfall_engine::midi_hardware::Runtime>>>,
    midi_configuring: Mutex<()>,
    /// Controller links at work, takes being recorded and the note log.
    /// May be locked under `state`, never the other way round.
    controllers: Mutex<controllers::Hub>,
    /// Wakes the thread that carries out what controllers ask for.
    controllers_wake: std::sync::Condvar,
    /// Serializes editor workers and retains at most one bounded clip view.
    audio_editor: Mutex<audio_editor::Editor>,
    /// Worker-only gate. Never acquired by audio or MIDI input callbacks.
    script_worker: Mutex<()>,
    script_ticket: std::sync::atomic::AtomicU64,
    assistant: crate::assistant::Service,
    audio_analysis_cancel: Mutex<Option<windfall_analysis::CancelToken>>,
    derived_assets: Mutex<derived_assets::Assets>,
    library: crate::library::Library,
    plugins: Mutex<Option<Arc<crate::plugins::PluginManager>>>,
    /// Held by a save or a backup from before it copies the project until
    /// its file is written. Taken before `state`, never under it.
    save: Mutex<()>,
    flp_import: Mutex<Option<flp::PreparedFlp>>,
    state: Mutex<State>,
    /// The transport as the UI was last told. Locked after `state`, never
    /// before it.
    transport: Mutex<TransportState>,
    /// Held while the audio device is reopened, so the settings remembered
    /// are the ones of the stream that ends up open.
    configuring: Mutex<()>,
    /// The engine status as the UI was last told.
    status: Mutex<EngineStatus>,
    settings: Mutex<SettingsStore>,
    subscribers: Mutex<Vec<realtime::Subscriber>>,
    controller: Controller,
    audio: Arc<dyn AudioDevice>,
    events: Arc<dyn EventSink>,
    cache: SampleCache,
    factory_dir: PathBuf,
    recording: Mutex<Option<recording::Take>>,
    recording_finishing: AtomicBool,
    recording_finish_cancelled: AtomicBool,
    input_monitors: Mutex<input_monitor::Monitors>,
    input_monitor_signature: Arc<std::sync::atomic::AtomicU64>,
    exporting: AtomicBool,
    preparing_clips: AtomicBool,
    preparing_samplers: AtomicBool,
    sampler_ticket: std::sync::atomic::AtomicU64,
    sampler_done: std::sync::atomic::AtomicU32,
    sampler_total: std::sync::atomic::AtomicU32,
    /// The export that is running has been asked to stop.
    export_cancelled: AtomicBool,
    /// Counts preview requests, so a file that finishes decoding after a
    /// newer request was made is not played. Held while a preview is
    /// handed to the engine or stopped, so neither can slip in between
    /// another's look at the count and what it then does.
    preview: Mutex<u64>,
    /// The audio logger. Locked after `configuring` and `recording`; nothing
    /// but the controller is taken under it.
    audio_logger: Mutex<audio_logger::Logger>,
    #[cfg(test)]
    pauses: tests::Pauses,
}

/// The open project and the audio that goes with it.
struct State {
    midi_target: Option<windfall_project::ChannelId>,
    document: Document,
    /// The file that saving writes to: where the project was last saved or
    /// opened from. `None` for a project that was never saved and for one
    /// opened from a backup, so that saving either asks for a place first.
    path: Option<PathBuf>,
    /// The folder project samples are relative to: the one `path` is in or,
    /// for a project opened from a backup, the one that holds the project
    /// the backup was made of.
    sample_dir: Option<PathBuf>,
    pool: SamplePool,
    /// The samples in `pool`, which cannot list them itself.
    loaded: HashSet<SampleId>,
    /// Samples being decoded on another thread.
    loading: HashSet<SampleId>,
    /// Samples whose file could not be read. They are reported once and not
    /// tried again until the user asks for the samples to be reloaded.
    failed: HashSet<SampleId>,
    /// Counts the documents this session has held. Work that started for an
    /// earlier document checks it and gives up.
    generation: u64,
    /// Counts the changes made to this document's project, undo and redo
    /// included. Equal counts mean the project is the same as it was.
    edits: u64,
    /// Counts the requests to replace the document, whether or not they got
    /// to. Only the last one made may still do it.
    replacements: u64,
    /// Only the latest MIDI review is retained, bounded by the reader's limits.
    midi_import: Option<midi::Prepared>,
    midi_ticket: u32,
    slice_review: Option<slicer::Prepared>,
    slice_ticket: u32,
}

impl State {
    /// The folder project samples are relative to.
    fn project_dir(&self) -> Option<&Path> {
        self.sample_dir.as_deref()
    }

    fn path_text(&self) -> Option<String> {
        self.path.as_deref().map(crate::paths::display)
    }
}

impl Session {
    pub fn install_plugins(&self, manager: Arc<crate::plugins::PluginManager>) {
        let old_pool = {
            let mut state = self.state();
            let old = state.pool.clone();
            state.pool.set_plugin_factory(manager.runtime.clone());
            old
        };
        let old_manager = lock(&self.inner.plugins).replace(manager);
        drop(old_manager);
        drop(old_pool);
    }
    pub fn refresh_plugins(&self) -> Result<(), String> {
        let runtime = lock(&self.inner.plugins)
            .as_ref()
            .map(|manager| manager.runtime.clone());
        let staged = runtime.as_ref().map(|runtime| runtime.prepare_document());
        let (project, mut pool, preparation) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            (
                state.document.project().clone(),
                state.pool.clone(),
                self.project_preparation(&state),
            )
        };
        if let Some(staged) = &staged {
            pool.set_plugin_factory(staged.clone());
        }
        let mut prepared = preparation
            .prepare(
                &project,
                &pool,
                windfall_engine::ProjectPublicationIntent::Edit,
            )
            .map_err(|error| error.to_string())?;
        let retirement;
        {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            let pool = prepared.pool().clone();
            let lease = prepared.publication(self, &state)?;
            retirement = lease.install();
            if let Some(staged) = &staged {
                staged.install_document();
            }
            state.pool.install_sampler_preparation(&pool);
            self.sync_transport();
        }
        let mut retirement = retirement;
        self.retire_project(&mut retirement);
        Ok(())
    }
    /// Owns recording exclusion while native plugin control work runs.
    pub(crate) fn while_recording_idle<T>(
        &self,
        work: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _recording = self.recording_idle()?;
        work()
    }
    fn capture_plugins(
        &self,
        project: windfall_project::Project,
        revision: Option<u64>,
    ) -> Result<windfall_project::Project, String> {
        let _recording = self.recording_idle()?;
        self.capture_plugins_idle(project, revision)
    }
    /// The caller already owns recording exclusion (recording -> state order).
    fn capture_plugins_idle(
        &self,
        project: windfall_project::Project,
        revision: Option<u64>,
    ) -> Result<windfall_project::Project, String> {
        let manager = lock(&self.inner.plugins).clone();
        match manager {
            Some(manager) => match revision {
                Some(revision) => manager.runtime.capture_at(project, revision),
                None => Ok(project),
            },
            None => Ok(project),
        }
    }
    /// Read alongside the document snapshot, before releasing the state lock.
    fn plugin_revision(&self) -> Option<u64> {
        lock(&self.inner.plugins)
            .as_ref()
            .map(|manager| manager.runtime.document_revision())
    }
    /// Starts a session on the default project and hands it to the engine.
    ///
    /// Decodes the default kit, which takes a few milliseconds. A kit file
    /// that is missing leaves its channel silent and is logged: no window
    /// exists yet to tell.
    pub fn new(config: SessionConfig) -> Self {
        let SessionConfig {
            controller,
            audio,
            events,
            factory_dir,
            settings,
        } = config;
        let factory_dir = crate::paths::clean(&factory_dir);
        let cache = SampleCache::new();
        let roots = std::iter::once(crate::browser::factory_root(&factory_dir))
            .chain(
                settings
                    .settings()
                    .browser_roots
                    .iter()
                    .map(|root| crate::browser::user_root(Path::new(root))),
            )
            .collect();
        let library = crate::library::Library::new(settings.library_file(), roots);
        let analysis = analysis_jobs::Service::new(settings.recordings_dir().join("Analysis"));
        let controller_hub = controllers::Hub::new(
            settings.settings().controllers.clone(),
            &settings.controllers_dir(),
        );

        let project = default_project();
        let decoded = samples::decode_all(&cache, &project, None, &factory_dir);
        for warning in &decoded.warnings {
            log::warn!("{warning}");
        }
        controller.set_project(&project, &decoded.pool);

        Self {
            inner: Arc::new(Inner {
                window_document_generation: std::sync::atomic::AtomicU64::new(0),
                timeline_request: std::sync::atomic::AtomicU64::new(0),
                timeline_play_request: std::sync::atomic::AtomicU64::new(0),
                archive_job: Mutex::new(None),
                backup_job: Mutex::new(None),
                phone_remote: Mutex::new(None),
                analysis,
                analyzers: analyzers::Service::new(),
                midi_hardware: Mutex::new(None),
                midi_configuring: Mutex::new(()),
                controllers: Mutex::new(controller_hub),
                controllers_wake: std::sync::Condvar::new(),
                audio_editor: Mutex::new(audio_editor::Editor::default()),
                audio_analysis_cancel: Mutex::new(None),
                derived_assets: Mutex::new(derived_assets::Assets::default()),
                extractions: Mutex::new(extractions::Extractions::default()),
                library,
                plugins: Mutex::new(None),
                save: Mutex::new(()),
                flp_import: Mutex::new(None),
                state: Mutex::new(State {
                    midi_target: None,
                    document: Document::new(project),
                    path: None,
                    sample_dir: None,
                    pool: decoded.pool,
                    loaded: decoded.loaded,
                    loading: HashSet::new(),
                    failed: decoded.failed,
                    generation: 0,
                    edits: 0,
                    replacements: 0,
                    midi_import: None,
                    midi_ticket: 0,
                    slice_review: None,
                    slice_ticket: 0,
                }),
                transport: Mutex::new(controller.transport()),
                configuring: Mutex::new(()),
                status: Mutex::new(audio.status()),
                settings: Mutex::new(settings),
                subscribers: Mutex::new(Vec::new()),
                controller,
                audio,
                events,
                cache,
                factory_dir,
                recording: Mutex::new(None),
                script_worker: Mutex::new(()),
                assistant: crate::assistant::Service::default(),
                script_ticket: std::sync::atomic::AtomicU64::new(0),
                recording_finishing: AtomicBool::new(false),
                recording_finish_cancelled: AtomicBool::new(false),
                input_monitors: Mutex::new(input_monitor::Monitors::default()),
                input_monitor_signature: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                exporting: AtomicBool::new(false),
                preparing_clips: AtomicBool::new(false),
                preparing_samplers: AtomicBool::new(false),
                sampler_ticket: std::sync::atomic::AtomicU64::new(0),
                sampler_done: std::sync::atomic::AtomicU32::new(0),
                sampler_total: std::sync::atomic::AtomicU32::new(0),
                export_cancelled: AtomicBool::new(false),
                preview: Mutex::new(0),
                audio_logger: Mutex::new(audio_logger::Logger::default()),
                #[cfg(test)]
                pauses: tests::Pauses::default(),
            }),
        }
    }

    pub fn downgrade(&self) -> WeakSession {
        WeakSession {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// The folder that holds the content shipped with Windfall.
    pub fn factory_dir(&self) -> &Path {
        &self.inner.factory_dir
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.state)
    }

    fn store(&self) -> MutexGuard<'_, SettingsStore> {
        lock(&self.inner.settings)
    }

    fn controller(&self) -> &Controller {
        &self.inner.controller
    }

    fn emit(&self, event: Event) {
        self.inner.events.emit(event);
    }
}

mod sample_bank_files;
mod sample_instruments;
mod sample_playback;

mod file_edits;

mod extractions;
