//! Windows plugin owner thread. Native instances never leave this thread;
//! only prepared audio adapters cross into the engine.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory};
use windfall_plugin_host::{
    PluginEffect, PluginHost, PluginInstance, PluginInstrument, PluginNotification, PluginState,
    ownership::{AudioOwnership, ControlOwnership, exchange},
};
use windfall_project::{PluginBinding, PluginTarget, Project};

type Job = Box<dyn FnOnce(&mut Owner) + Send>;
/// A control caller can cancel queued work, but joins work already running.
/// Its recording guard therefore outlives every native call and recovery.
#[derive(Default)]
struct JobLifetime {
    // Queued, running, completed/deliverable, cancelled queued, cancelled running.
    phase: std::sync::atomic::AtomicU8,
}
impl JobLifetime {
    fn cancelled(&self) -> bool {
        matches!(self.phase.load(std::sync::atomic::Ordering::Acquire), 3 | 4)
    }
}
type Approved =
    Arc<Mutex<BTreeMap<(String, String), windfall_plugin_host::paths::PluginFileIdentity>>>;
type Selection =
    Arc<Mutex<std::collections::HashMap<PluginTarget, Arc<std::sync::atomic::AtomicU64>>>>;
fn selected(selection: &Selection, target: PluginTarget) -> Option<u64> {
    selection
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&target)
        .map(|token| token.load(std::sync::atomic::Ordering::Relaxed))
        .filter(|token| *token != 0)
}
#[derive(Clone)]
pub enum Update {
    Parameter {
        target: PluginTarget,
        id: u32,
        value: f32,
        gesture: u64,
    },
    Capture {
        target: PluginTarget,
        serial: u64,
    },
}
#[derive(Clone)]
pub struct PendingUpdate {
    pub binding: u64,
    pub token: u64,
    pub revision: u64,
    pub update: Update,
}
pub(crate) struct CapturedState {
    pub bytes: Vec<u8>,
    pub parameters: Vec<(u32, f32)>,
    pub restart: bool,
    serial: u64,
}
struct Instance {
    identity: u64,
    target: PluginTarget,
    plugin: PluginInstance,
    binding: PluginBinding,
    revision: u64,
    playback: bool,
    ownership: ControlOwnership<Adapter>,
    rate: u32,
    block: usize,
    instrument: bool,
    latency: usize,
    tail: usize,
    returned: Option<Adapter>,
    dirty: bool,
    dirty_serial: u64,
    capture_sent: bool,
    restart: bool,
    notifications: Vec<PluginNotification>,
    controls: ParameterControls,
}
struct OwnerCapture {
    state: PluginState,
    parameters: Vec<(u32, f32)>,
    recovery_error: Option<String>,
}
struct CaptureIntent {
    id: u32,
    generation: u64,
    document: u64,
    value: f32,
}

/// One callback writer publishes desired values; only completed native
/// processing or inactive owner reconciliation acknowledges their generation.
/// Storage is allocated with the binding. Audio never spins or allocates.
pub(super) struct ParameterControl {
    pub(super) id: u32,
    pub(super) value: std::sync::atomic::AtomicU32,
    pub(super) generation: std::sync::atomic::AtomicU64,
    pub(super) applied: std::sync::atomic::AtomicU64,
    pub(super) document_value: std::sync::atomic::AtomicU32,
    pub(super) document_generation: std::sync::atomic::AtomicU64,
    pub(super) observed_document: std::sync::atomic::AtomicU64,
    pub(super) applied_document: std::sync::atomic::AtomicU64,
}
impl ParameterControl {
    fn new(id: u32, value: f32) -> Self {
        Self {
            id,
            value: std::sync::atomic::AtomicU32::new(value.to_bits()),
            generation: std::sync::atomic::AtomicU64::new(2),
            applied: std::sync::atomic::AtomicU64::new(2),
            document_value: std::sync::atomic::AtomicU32::new(value.to_bits()),
            document_generation: std::sync::atomic::AtomicU64::new(2),
            observed_document: std::sync::atomic::AtomicU64::new(2),
            applied_document: std::sync::atomic::AtomicU64::new(2),
        }
    }
    pub(super) fn publish(&self, value: f32) {
        use std::sync::atomic::Ordering;
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.value.store(value.to_bits(), Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
        self.observe_document(value);
    }
    pub(super) fn observe_document(&self, value: f32) {
        use std::sync::atomic::Ordering;
        // One bounded read matches the committed generation/value pair. A
        // stale plan with different values cannot acknowledge newer intent.
        // Adoption alone never acknowledges actual native processing.
        let document = self.document_generation.load(Ordering::Acquire);
        if document & 1 == 0
            && self.document_value.load(Ordering::Relaxed) == value.to_bits()
            && self.document_generation.load(Ordering::Acquire) == document
        {
            self.observed_document.store(document, Ordering::Release);
        }
    }
    pub(super) fn document(&self) -> (u64, f32) {
        use std::sync::atomic::Ordering;
        loop {
            let generation = self.document_generation.load(Ordering::Acquire);
            if generation & 1 != 0 {
                continue;
            }
            let value = f32::from_bits(self.document_value.load(Ordering::Relaxed));
            if self.document_generation.load(Ordering::Acquire) == generation {
                return (generation, value);
            }
        }
    }
    pub(super) fn commit(&self, value: f32) {
        use std::sync::atomic::Ordering;
        if self.document_value.load(Ordering::Relaxed) != value.to_bits() {
            self.document_generation.fetch_add(1, Ordering::AcqRel);
            self.document_value
                .store(value.to_bits(), Ordering::Relaxed);
            self.document_generation.fetch_add(1, Ordering::Release);
        }
    }
    // Owner-side snapshot only. An odd generation means a callback is writing.
    pub(super) fn snapshot(&self) -> (u64, f32) {
        use std::sync::atomic::Ordering;
        loop {
            let generation = self.generation.load(Ordering::Acquire);
            if generation & 1 != 0 {
                continue;
            }
            let value = f32::from_bits(self.value.load(Ordering::Relaxed));
            if self.generation.load(Ordering::Acquire) == generation {
                return (generation, value);
            }
        }
    }
    pub(super) fn settled(&self, generation: u64) -> bool {
        self.applied.load(std::sync::atomic::Ordering::Acquire) == generation
    }
}
pub(super) type ParameterControls = Arc<[ParameterControl]>;
type ControlRegistry = Arc<Mutex<BTreeMap<u64, (PluginTarget, u64, ParameterControls)>>>;
pub(super) fn parameter_controls(binding: &PluginBinding) -> ParameterControls {
    binding
        .parameters
        .iter()
        .filter(|param| !param.read_only)
        .map(|param| ParameterControl::new(param.id, param.value))
        .collect()
}

impl Instance {
    fn check_capture_document(
        &self,
        desired: &[windfall_project::PluginParameter],
    ) -> Result<(), String> {
        for control in self.controls.iter() {
            let (document, value) = control.document();
            if document > 2
                && desired
                    .iter()
                    .find(|param| param.id == control.id)
                    .is_none_or(|param| param.value != value)
            {
                return Err("Plugin parameters changed during state capture; save again".into());
            }
        }
        Ok(())
    }
    fn capture_intents(&self, desired: &[windfall_project::PluginParameter]) -> Vec<CaptureIntent> {
        self.controls
            .iter()
            .filter_map(|control| {
                let (generation, value) = control.snapshot();
                let wanted = desired
                    .iter()
                    .find(|param| param.id == control.id)
                    .map_or(value, |param| param.value);
                let (document, committed) = control.document();
                let document_pending = control
                    .applied_document
                    .load(std::sync::atomic::Ordering::Acquire)
                    != document;
                let explicit_snapshot = wanted != committed;
                (!control.settled(generation) || document_pending || explicit_snapshot).then_some(
                    CaptureIntent {
                        id: control.id,
                        generation,
                        document,
                        value: if document_pending || explicit_snapshot {
                            wanted
                        } else {
                            value
                        },
                    },
                )
            })
            .collect()
    }
    fn retire(&mut self, adapter: Adapter) {
        // Even refusal tears down only after the returned audio half is
        // destroyed on this owner. It must never be queued for resume here.
        let _ = self.discard(adapter);
    }
    fn discard(&mut self, adapter: Adapter) -> Result<(), String> {
        match adapter {
            Adapter::Effect(adapter) => self
                .plugin
                .release_effect(adapter)
                .map_err(|error| error.error.to_string()),
            Adapter::Instrument(adapter) => self
                .plugin
                .release_instrument(adapter)
                .map_err(|error| error.error.to_string()),
        }
    }
    fn release(&mut self, adapter: Adapter) -> Result<(), String> {
        let result = match adapter {
            Adapter::Effect(adapter) => self
                .plugin
                .release_effect(adapter)
                .map_err(|error| (error.error.to_string(), Adapter::Effect(error.returned))),
            Adapter::Instrument(adapter) => self
                .plugin
                .release_instrument(adapter)
                .map_err(|error| (error.error.to_string(), Adapter::Instrument(error.returned))),
        };
        match result {
            Ok(()) => Ok(()),
            Err((error, adapter)) => {
                self.resume_returned(adapter);
                Err(error)
            }
        }
    }

    fn resume_returned(&mut self, adapter: Adapter) {
        self.ownership.cancel();
        self.returned = self.ownership.resume(adapter).err();
    }

    fn restore(&mut self) -> Result<(), String> {
        let adapter = if self.instrument {
            Adapter::Instrument(
                self.plugin
                    .prepare_instrument_with_options(
                        self.rate as f32,
                        super::wrapper::max_block(&self.binding, self.block),
                        &super::wrapper::native(&self.binding)?,
                    )
                    .map_err(|error| error.to_string())?,
            )
        } else {
            Adapter::Effect(
                self.plugin
                    .prepare_effect_with_options(
                        self.rate as f32,
                        super::wrapper::max_block(&self.binding, self.block),
                        &super::wrapper::native(&self.binding)?,
                    )
                    .map_err(|error| error.to_string())?,
            )
        };
        if adapter.latency() != self.latency || adapter.tail() != self.tail {
            self.discard(adapter)?;
            return Err(
                "Plugin latency/tail changed during state capture; retry the plugin".into(),
            );
        }
        if let Err(adapter) = self.ownership.resume(adapter) {
            self.discard(adapter)?;
            return Err("The plugin resume queue is unexpectedly full".into());
        }
        Ok(())
    }

    /// Owner-thread wait only. Audio returns at a boundary without waiting on
    /// this thread or any session lock. A timeout leaves active state untouched.
    fn capture(
        &mut self,
        lifetime: &JobLifetime,
        rebuilding: bool,
        desired: &[windfall_project::PluginParameter],
    ) -> Result<OwnerCapture, String> {
        if lifetime.cancelled() {
            return Err("Plugin capture was cancelled".into());
        }
        self.check_capture_document(desired)?;
        if self.binding.format == "clap" {
            let state = self
                .plugin
                .save_state()
                .map_err(|error| error.to_string())?;
            let mut parameters = self.parameter_values();
            self.check_capture_document(desired)?;
            // CLAP state remains active. Unapplied controls accompany the
            // opaque state and are restored after it on reopen/offline create.
            for intent in self.capture_intents(desired) {
                if let Some(param) = parameters.iter_mut().find(|param| param.0 == intent.id) {
                    param.1 = intent.value;
                }
            }
            return Ok(OwnerCapture {
                state,
                parameters,
                recovery_error: None,
            });
        }
        self.ownership.request();
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        let adapter = loop {
            if lifetime.cancelled() {
                self.ownership.cancel();
                return Err("Plugin capture was cancelled".into());
            }
            if let Some(adapter) = self.ownership.take_returned() {
                break adapter;
            }
            if std::time::Instant::now() >= deadline {
                self.ownership.cancel();
                return Err("Plugin state capture needs a live audio block boundary; the processor was not returned".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if lifetime.cancelled() {
            self.resume_returned(adapter);
            return Err("Plugin capture was cancelled".into());
        }
        let failed = adapter.failed();
        self.release(adapter)?;
        if failed {
            self.ownership.cancel();
            return Err(
                "A failed plugin remains bypassed/silent; retry before capturing state".into(),
            );
        }
        let captured = (|| {
            self.check_capture_document(desired)?;
            // Read after the returned boundary: callbacks may publish new controls
            // while the owner waits, and their generations must not be acknowledged
            // using a value read before that boundary.
            let intents = self.capture_intents(desired);
            let mut reconciled = Vec::with_capacity(intents.len());
            for CaptureIntent {
                id,
                generation,
                document,
                value,
            } in intents
            {
                // Lifecycle edits linearize after the queued controls returned by
                // this boundary. They supersede both accepted and rejected points.
                if self.plugin.deactivation_param_value(id).is_some() {
                    let control = self
                        .controls
                        .iter()
                        .find(|control| control.id == id)
                        .expect("known control");
                    reconciled.push((id, control.snapshot().0, document));
                } else if self.plugin.set_param(id, f64::from(value)) {
                    reconciled.push((id, generation, document));
                } else {
                    return Err("Plugin refused a pending parameter during state capture".into());
                }
            }
            // Deactivation can leave controller edits, rescans and dirty flags.
            // Service them while inactive, before serializing. They are included
            // in this capture rather than scheduling an endless dirty-capture loop.
            self.plugin.idle(&mut |notification| match notification {
                PluginNotification::StateChanged => {
                    self.dirty = true;
                    self.dirty_serial = self.dirty_serial.saturating_add(1);
                }
                PluginNotification::RestartRequested
                | PluginNotification::ParamsRescanned
                | PluginNotification::LatencyChanged { .. } => {
                    self.restart = true;
                    self.dirty = true;
                    self.dirty_serial = self.dirty_serial.saturating_add(1);
                }
                notification => self.notifications.push(notification),
            });
            if lifetime.cancelled() {
                return Err("Plugin capture was cancelled".into());
            }
            let state = self
                .plugin
                .save_state()
                .map_err(|error| error.to_string())?;
            let parameters = self.parameter_values();
            self.check_capture_document(desired)?;
            Ok((state, parameters, reconciled))
        })();
        // Even a failed save must give the sounding instance back when safe.
        let restore = self.restore();
        let recovery_error = match restore {
            Ok(()) => None,
            // A requested restart will install a new plan and its correct
            // latency buffers. Preserve proven inactive state for that plan;
            // never resume an adapter with changed metadata into the old slot.
            Err(error) if rebuilding && self.restart && captured.is_ok() => Some(error),
            Err(error) => return Err(error),
        };
        if lifetime.cancelled() {
            return Err("Plugin capture was cancelled".into());
        }
        captured.map(|(state, parameters, reconciled)| {
            for (id, generation, document) in reconciled {
                let control = self
                    .controls
                    .iter()
                    .find(|control| control.id == id)
                    .expect("known control");
                control
                    .applied
                    .store(generation, std::sync::atomic::Ordering::Release);
                control
                    .applied_document
                    .fetch_max(document, std::sync::atomic::Ordering::Release);
            }
            OwnerCapture {
                state,
                parameters,
                recovery_error,
            }
        })
    }
    fn parameter_values(&mut self) -> Vec<(u32, f32)> {
        let ids: Vec<_> = self.plugin.params().iter().map(|param| param.id).collect();
        ids.into_iter()
            .filter_map(|id| self.plugin.param_value(id).map(|value| (id, value as f32)))
            .collect()
    }
}
pub(crate) fn binding_identity(binding: &PluginBinding) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    binding.format.hash(&mut hash);
    binding.path.hash(&mut hash);
    binding.id.hash(&mut hash);
    binding.state.hash(&mut hash);
    if !binding.wrapper.is_default() {
        binding.wrapper.hash(&mut hash);
    }
    hash.finish()
}
struct Owner {
    selection: Selection,
    host: PluginHost,
    instances: BTreeMap<u64, Instance>,
    #[cfg(windows)]
    bridges: BTreeMap<u64, super::bridge::Record>,
    next: u64,
    approved: Approved,
    controls: ControlRegistry,
}

/// Control-side handle; synchronous requests are never made by audio processing.
#[derive(Clone)]
pub struct Runtime {
    selection: Selection,
    jobs: mpsc::Sender<Job>,
    #[cfg(all(test, windows))]
    capture_ack_fault: Arc<Mutex<Option<super::capture_ack::CaptureAckTestFault>>>,
    errors: Arc<Mutex<Vec<(PluginTarget, String)>>>,
    approved: Approved,
    controls: ControlRegistry,
    updates: Arc<Mutex<mpsc::Receiver<PendingUpdate>>>,
    revision: Arc<std::sync::atomic::AtomicU64>,
    next_revision: Arc<std::sync::atomic::AtomicU64>,
    prepared_revision: Option<u64>,
    rendering: bool,
    #[cfg(windows)]
    bridge_enabled: bool,
    #[cfg(windows)]
    bridge_helper: Option<std::path::PathBuf>,
    render_error: Arc<Mutex<Option<String>>>,
    editor_scale_percent: Arc<std::sync::atomic::AtomicU32>,
}
impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRuntime").finish_non_exhaustive()
    }
}
impl Runtime {
    pub fn new() -> Result<Self, String> {
        if !cfg!(windows) {
            return Err("Native plugin integration currently requires Windows".into());
        }
        let (jobs, receiver) = mpsc::channel::<Job>();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let approved: Approved = Default::default();
        let owner_approved = approved.clone();
        let controls: ControlRegistry = Default::default();
        let owner_controls = controls.clone();
        let owner_errors = errors.clone();
        let (updates, update_receiver) = mpsc::sync_channel(4096);
        let revision = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let owner_revision = revision.clone();
        let selection: Selection = Default::default();
        let owner_selection = selection.clone();
        std::thread::Builder::new()
            .name("plugin-owner".into())
            .spawn(move || {
                let mut owner = Owner {
                    selection: owner_selection,
                    host: PluginHost::windfall(),
                    instances: BTreeMap::new(),
                    #[cfg(windows)]
                    bridges: BTreeMap::new(),
                    next: 0,
                    approved: owner_approved,
                    controls: owner_controls,
                };
                let mut gestures = std::collections::HashMap::new();
                let mut gesture = 1_u64 << 63;
                loop {
                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(job) => job(&mut owner),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    // Bridge process IO/reaping is confined to this control owner.
                    #[cfg(windows)]
                    {
                        let retired = owner
                            .bridges
                            .iter()
                            .filter_map(|(token, record)| {
                                (!record.alive.load(std::sync::atomic::Ordering::Acquire))
                                    .then_some(*token)
                            })
                            .collect::<Vec<_>>();
                        for token in retired {
                            owner
                                .controls
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .remove(&token);
                            owner.bridges.remove(&token);
                        }
                        for (token, record) in &mut owner.bridges {
                            if record.playback
                                && selected(&owner.selection, record.binding.target) == Some(*token)
                                && record.revision
                                    == owner_revision.load(std::sync::atomic::Ordering::Acquire)
                            {
                                record.dirty_serial = record.control.native_change_serial();
                                if record.dirty_serial > record.acknowledged_serial
                                    && !record.capture_sent
                                {
                                    record.capture_sent = updates
                                        .try_send(PendingUpdate {
                                            binding: binding_identity(&record.binding),
                                            token: *token,
                                            revision: record.revision,
                                            update: Update::Capture {
                                                target: record.binding.target,
                                                serial: record.dirty_serial,
                                            },
                                        })
                                        .is_ok();
                                }
                                let status = record.status();
                                if status.failed {
                                    let mut errors = owner_errors
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner());
                                    errors.retain(|(target, _)| *target != record.binding.target);
                                    errors.push((
                                        record.binding.target,
                                        format!(
                                            "Process bridge failed; retry manually: {}",
                                            status.error.unwrap_or_else(|| {
                                                "native helper unavailable".into()
                                            })
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    windfall_plugin_host::gui::pump_events(Some(Duration::ZERO));
                    let selection = owner.selection.clone();
                    for (token, record) in &mut owner.instances {
                        // A cancellation can race the callback's return. Always
                        // service late returns, including unselected/stale units.
                        if let Some(adapter) = record
                            .returned
                            .take()
                            .or_else(|| record.ownership.take_returned())
                        {
                            // Cancelled captures resume the exact active adapter.
                            // Notification maintenance never quiesces native work.
                            record.resume_returned(adapter);
                        }
                        let target = &record.target;
                        let instance = &mut record.plugin;
                        let current = record.playback
                            && selected(&selection, *target) == Some(*token)
                            && record.revision
                                == owner_revision.load(std::sync::atomic::Ordering::Relaxed);
                        let send = |update| {
                            if current {
                                let _ = updates.try_send(PendingUpdate {
                                    binding: record.identity,
                                    token: *token,
                                    revision: record.revision,
                                    update,
                                });
                            }
                        };
                        let mut state_changed = false;
                        let mut restart = false;
                        let mut notify = |notification, captured| match notification {
                            PluginNotification::ParamGestureBegin { id } => {
                                gesture = gesture.wrapping_add(1);
                                gestures.insert((*token, id), gesture);
                            }
                            PluginNotification::ParamGestureEnd { id } => {
                                gestures.remove(&(*token, id));
                            }
                            PluginNotification::ParamChanged { id, value } => {
                                if let Some(gesture) = gestures.get(&(*token, id)) {
                                    send(Update::Parameter {
                                        target: *target,
                                        id,
                                        value: value as f32,
                                        gesture: *gesture,
                                    });
                                } else if captured {
                                    if record
                                        .binding
                                        .parameters
                                        .iter()
                                        .any(|param| param.id == id && !param.read_only)
                                    {
                                        send(Update::Parameter {
                                            target: *target,
                                            id,
                                            value: value as f32,
                                            gesture: 0,
                                        });
                                    }
                                } else if record
                                    .binding
                                    .parameters
                                    .iter()
                                    .any(|param| param.id == id && !param.read_only)
                                {
                                    state_changed = true;
                                }
                            }
                            PluginNotification::StateChanged => state_changed = true,
                            PluginNotification::EditorScaleRefused { application, monitor } if current => {
                                let mut errors = owner_errors.lock().unwrap_or_else(|error| error.into_inner());
                                errors.retain(|(before, _)| *before != *target);
                                errors.push((*target, format!("The plugin refused content scaling at application scale {application} and monitor scale {monitor}.")));
                            }
                            PluginNotification::LatencyChanged { .. }
                            | PluginNotification::RestartRequested
                            | PluginNotification::ParamsRescanned => {
                                state_changed = true;
                                restart = true;
                            }
                            _ => {}
                        };
                        for notification in record.notifications.drain(..) {
                            notify(notification, true);
                        }
                        instance.idle(&mut |notification| notify(notification, false));
                        if state_changed {
                            record.dirty = true;
                            record.dirty_serial = record.dirty_serial.saturating_add(1);
                            record.restart |= restart;
                        }
                        if current && record.dirty && !record.capture_sent {
                            record.capture_sent = updates
                                .try_send(PendingUpdate {
                                    binding: record.identity,
                                    token: *token,
                                    revision: record.revision,
                                    update: Update::Capture {
                                        target: *target,
                                        serial: record.dirty_serial,
                                    },
                                })
                                .is_ok();
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            selection,
            jobs,
            #[cfg(all(test, windows))]
            capture_ack_fault: Arc::new(Mutex::new(None)),
            errors,
            approved,
            controls,
            updates: Arc::new(Mutex::new(update_receiver)),
            revision,
            next_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            prepared_revision: None,
            rendering: false,
            // Existing native-owner/R4 fixtures use their historical provider.
            // New bridge tests explicitly inject a real helper; release builds
            // on Windows use this process route and the installed executable.
            #[cfg(windows)]
            bridge_enabled: !cfg!(test),
            #[cfg(windows)]
            bridge_helper: None,
            render_error: Arc::new(Mutex::new(None)),
            editor_scale_percent: Arc::new(std::sync::atomic::AtomicU32::new(100)),
        })
    }
    pub fn approve_catalog(&self, catalog: &windfall_plugin_host::scan::PluginCatalog) {
        let mut approved = self
            .approved
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        approved.clear();
        for (path, plugin) in catalog.plugins() {
            if let Some(identity) = catalog.identity(path) {
                approved.insert(
                    (
                        path.to_string_lossy().into_owned(),
                        plugin.descriptor.id.clone(),
                    ),
                    identity.clone(),
                );
            }
        }
    }
    /// Control-only metadata publication before an engine plan is queued.
    /// It neither calls nor waits for the native owner, including during capture.
    pub(crate) fn commit_parameters(&self, project: &Project) {
        let controls = self
            .controls
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (token, (target, identity, values)) in controls.iter() {
            if selected(&self.selection, *target) == Some(*token)
                && let Some(binding) = project
                    .plugin(*target)
                    .filter(|binding| binding_identity(binding) == *identity)
            {
                for control in values.iter() {
                    if let Some(param) = binding
                        .parameters
                        .iter()
                        .find(|param| param.id == control.id)
                    {
                        control.commit(param.value);
                    }
                }
            }
        }
    }
    #[cfg(all(test, windows))]
    pub(crate) fn approve(&self, entries: &[windfall_ipc::PluginEntry]) {
        let mut approved = self
            .approved
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        approved.clear();
        for entry in entries.iter().filter(|entry| entry.usable) {
            if let Ok(identity) =
                windfall_plugin_host::paths::plugin_file_identity(std::path::Path::new(&entry.path))
            {
                approved.insert((entry.path.clone(), entry.id.clone()), identity);
            }
        }
    }
    pub fn drain(&self) -> Vec<PendingUpdate> {
        self.updates
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_iter()
            .take(4096)
            .filter(|update| self.is_current(update.revision, update.token))
            .collect()
    }
    pub fn is_current(&self, revision: u64, token: u64) -> bool {
        revision == self.revision.load(std::sync::atomic::Ordering::Relaxed)
            && self
                .selection
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .any(|selected| selected.load(std::sync::atomic::Ordering::Relaxed) == token)
    }
    pub fn document_revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(all(test, windows))]
    pub(crate) fn selected_token(&self, target: PluginTarget) -> Option<u64> {
        selected(&self.selection, target)
    }
    #[cfg(all(test, windows))]
    pub(crate) fn fixture_parameter(&self, target: PluginTarget, id: u32) -> Option<f64> {
        self.call(move |owner| {
            Ok(selected(&owner.selection, target)
                .and_then(|token| owner.instances.get_mut(&token))
                .and_then(|record| record.plugin.param_value(id)))
        })
        .unwrap()
    }
    fn selection(&self, target: PluginTarget) -> Option<Arc<std::sync::atomic::AtomicU64>> {
        if self.rendering {
            None
        } else {
            Some(
                self.selection
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .entry(target)
                    .or_default()
                    .clone(),
            )
        }
    }
    pub fn retry(&self) {
        let revision = self
            .next_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.revision
            .store(revision, std::sync::atomic::Ordering::Relaxed);
    }
    /// Prepare a replacement without invalidating the installed document.
    pub fn prepare_document(&self) -> Arc<Self> {
        let mut staged = self.clone();
        staged.prepared_revision = Some(
            self.next_revision
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1,
        );
        Arc::new(staged)
    }
    /// Call only after a replacement has passed its guards and been installed.
    pub fn install_document(&self) {
        if let Some(revision) = self.prepared_revision {
            self.revision
                .store(revision, std::sync::atomic::Ordering::Relaxed);
        }
    }
    fn preparation_revision(&self) -> u64 {
        self.prepared_revision
            .unwrap_or_else(|| self.revision.load(std::sync::atomic::Ordering::Relaxed))
    }
    fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Owner) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.call_with_lifetime(Duration::from_secs(15), move |owner, _| job(owner))
    }
    fn call_with_lifetime<T: Send + 'static>(
        &self,
        timeout: Duration,
        job: impl FnOnce(&mut Owner, &JobLifetime) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        use std::sync::atomic::Ordering;
        let (sender, receiver) = mpsc::sync_channel(1);
        let lifetime = Arc::new(JobLifetime::default());
        let owner_lifetime = lifetime.clone();
        self.jobs
            .send(Box::new(move |owner| {
                if owner_lifetime
                    .phase
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }
                let result = job(owner, &owner_lifetime);
                if owner_lifetime
                    .phase
                    .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    let _ = sender.send(result);
                } else {
                    // Cancellation won the completion race. Native-bearing
                    // results must be destroyed here, on their creating owner.
                    drop(result);
                    let _ = sender.send(Err(
                        "The plugin owner request timed out and was cancelled".into(),
                    ));
                }
            }))
            .map_err(|_| "The plugin owner stopped".to_owned())?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("The plugin owner stopped".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                loop {
                    let phase = lifetime.phase.load(Ordering::Acquire);
                    match phase {
                        0 | 1 => {
                            if lifetime
                                .phase
                                .compare_exchange(
                                    phase,
                                    phase + 3,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_err()
                            {
                                continue;
                            }
                            if phase == 1 {
                                // Join native work/recovery while the caller's
                                // exclusion remains held. Only an error crosses
                                // back; cancelled payloads stay on the owner.
                                let _ = receiver.recv();
                            }
                            break;
                        }
                        2 => {
                            return receiver
                                .recv()
                                .map_err(|_| "The plugin owner stopped".to_owned())?;
                        }
                        _ => break,
                    }
                }
                Err("The plugin owner request timed out and was cancelled".into())
            }
        }
    }
    fn approved_binding(
        owner: &Owner,
        binding: &PluginBinding,
    ) -> Result<windfall_plugin_host::paths::PluginFileIdentity, String> {
        binding.validate().map_err(str::to_owned)?;
        let format = match binding.format.as_str() {
            "clap" => windfall_plugin_host::PluginFormat::Clap,
            "vst3" => windfall_plugin_host::PluginFormat::Vst3,
            _ => return Err("Unsupported native plugin format".into()),
        };
        if windfall_plugin_host::PluginFormat::of(std::path::Path::new(&binding.path))
            != Some(format)
        {
            return Err("Plugin path does not match its declared format".into());
        }
        let stamp =
            windfall_plugin_host::paths::plugin_file_identity(std::path::Path::new(&binding.path))
                .map_err(|error| error.to_string())?;
        if owner
            .approved
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(binding.path.clone(), binding.id.clone()))
            != Some(&stamp)
        {
            return Err(
                "This plugin is unscanned, changed, or blocked. Scan it before loading".into(),
            );
        }
        Ok(stamp)
    }
    fn create(owner: &mut Owner, binding: &PluginBinding) -> Result<PluginInstance, String> {
        Self::approved_binding(owner, binding)?;
        let format = match binding.format.as_str() {
            "clap" => windfall_plugin_host::PluginFormat::Clap,
            "vst3" => windfall_plugin_host::PluginFormat::Vst3,
            _ => return Err("Unsupported native plugin format".into()),
        };
        let module = owner
            .host
            .load(std::path::Path::new(&binding.path))
            .map_err(|error| error.to_string())?;
        let descriptor = module
            .descriptors()
            .into_iter()
            .find(|descriptor| descriptor.id == binding.id)
            .ok_or_else(|| "The scanned plugin ID is absent from this file".to_owned())?;
        let kind = match binding.target {
            PluginTarget::Effect { .. } => windfall_plugin_host::PluginKind::Effect,
            PluginTarget::Instrument { .. } => windfall_plugin_host::PluginKind::Instrument,
        };
        if descriptor.format != format || descriptor.kind != kind {
            return Err(
                "Plugin format or instrument/effect role does not match its binding".into(),
            );
        }
        let mut instance = module
            .create(&binding.id)
            .map_err(|error| error.to_string())?;
        if !binding.state.is_empty() {
            let state = PluginState::from_bytes(binding.state.clone());
            instance
                .load_state(&state)
                .map_err(|error| error.to_string())?;
        }
        for param in &binding.parameters {
            if !param.read_only {
                instance.set_param(param.id, f64::from(param.value));
            }
        }
        Ok(instance)
    }
    pub fn discover(&self, mut binding: PluginBinding) -> Result<PluginBinding, String> {
        #[cfg(windows)]
        if self.bridge_enabled {
            let helper = super::bridge::helper_path(self.bridge_helper.as_deref())?;
            return self.call(move |owner| {
                let stamp = Self::approved_binding(owner, &binding)?;
                owner.next = owner
                    .next
                    .checked_add(1)
                    .ok_or("Plugin owner token exhausted")?;
                let options =
                    super::bridge::options(helper, &binding, stamp, owner.next, 0, 48_000, false)?;
                let (control, audio, _) =
                    windfall_plugin_host::bridge::supervisor::launch(options)?;
                let result = control.describe(Duration::from_secs(2));
                control.terminate();
                drop(audio);
                let (state, parameters, layout) = result?;
                binding.state = state;
                binding.auxiliary_inputs = layout
                    .audio_inputs
                    .into_iter()
                    .enumerate()
                    .filter(|(_, input)| !input.main && input.channels > 0)
                    .map(|(index, input)| windfall_project::PluginAuxInput {
                        index: index as u32,
                        name: input.name,
                        channels: input.channels,
                    })
                    .collect();
                binding.parameters = parameters
                    .into_iter()
                    .map(|parameter| windfall_project::PluginParameter {
                        id: parameter.spec.id,
                        name: parameter.name,
                        min: parameter.spec.min as f32,
                        max: parameter.spec.max as f32,
                        value: parameter.spec.value as f32,
                        stepped: parameter.spec.stepped,
                        read_only: parameter.spec.read_only,
                        automatable: parameter.automatable,
                    })
                    .collect();
                binding.validate().map_err(str::to_owned)?;
                Ok(binding)
            });
        }
        self.call(move |owner| {
            let mut instance = Self::create(owner, &binding)?;
            binding.auxiliary_inputs = instance
                .layout()
                .audio_inputs
                .iter()
                .enumerate()
                .filter(|(_, input)| !input.main && input.channels > 0)
                .map(|(index, input)| windfall_project::PluginAuxInput {
                    index: index as u32,
                    name: input.name.clone(),
                    channels: input.channels,
                })
                .collect();
            let params = instance.params().to_vec();
            binding.parameters = params
                .into_iter()
                .filter(|param| !param.hidden)
                .map(|param| windfall_project::PluginParameter {
                    id: param.id,
                    name: param.name,
                    min: param.min as f32,
                    max: param.max as f32,
                    value: instance.param_value(param.id).unwrap_or(param.default) as f32,
                    stepped: param.stepped,
                    read_only: param.read_only,
                    automatable: param.automatable,
                })
                .collect();
            binding.state = instance
                .save_state()
                .map_err(|error| error.to_string())?
                .into_bytes();
            binding.validate().map_err(str::to_owned)?;
            Ok(binding)
        })
    }
    /// Captures live opaque state into a save copy, leaving document history alone.
    pub fn capture(&self, project: Project) -> Result<Project, String> {
        let revision = self.revision.load(std::sync::atomic::Ordering::Relaxed);
        self.capture_at(project, revision)
    }
    /// Captures only from the document revision paired with the save snapshot.
    pub fn capture_at(&self, mut project: Project, revision: u64) -> Result<Project, String> {
        let current_revision = self.revision.clone();
        let errors = self.errors.clone();
        self.call_with_lifetime(Duration::from_secs(15), move |owner, lifetime| {
            if current_revision.load(std::sync::atomic::Ordering::Relaxed) != revision {
                return Ok(project);
            }
            for binding in &mut project.plugins {
                let selected_token = selected(&owner.selection, binding.target);
                #[cfg(windows)]
                if let Some(record) = selected_token.and_then(|token| owner.bridges.get(&token))
                    && record.playback
                    && record.revision == revision
                    && record.binding.path == binding.path
                    && record.binding.id == binding.id
                    && record.binding.format == binding.format
                    && record.binding.state == binding.state
                {
                    if lifetime.cancelled() {
                        return Err("Plugin capture was cancelled".into());
                    }
                    let (state, parameters) = record.capture(&binding.parameters)?;
                    if lifetime.cancelled()
                        || current_revision.load(std::sync::atomic::Ordering::Acquire) != revision
                        || selected(&owner.selection, binding.target) != selected_token
                    {
                        return Err(
                            "Plugin ownership changed during state capture; save again".into()
                        );
                    }
                    binding.state = state;
                    for param in &mut binding.parameters {
                        if let Some(value) = parameters.iter().find(|value| value.0 == param.id) {
                            param.value = value.1;
                        }
                    }
                    continue;
                }

                if let Some(record) =
                    selected_token.and_then(|token| owner.instances.get_mut(&token))
                    && record.playback
                    && record.revision == revision
                    && record.binding.path == binding.path
                    && record.binding.id == binding.id
                    && record.binding.format == binding.format
                    && record.binding.state == binding.state
                {
                    let captured = match record.capture(lifetime, false, &binding.parameters) {
                        Ok(captured) => captured,
                        Err(error) => {
                            let mut errors =
                                errors.lock().unwrap_or_else(|error| error.into_inner());
                            errors.retain(|(target, _)| *target != binding.target);
                            errors.push((binding.target, error.clone()));
                            return Err(error);
                        }
                    };
                    if current_revision.load(std::sync::atomic::Ordering::Relaxed) != revision
                        || selected(&owner.selection, binding.target) != selected_token
                    {
                        return Err(
                            "Plugin ownership changed during state capture; save again".into()
                        );
                    }
                    binding.state = captured.state.into_bytes();
                    for param in &mut binding.parameters {
                        if let Some((_, value)) =
                            captured.parameters.iter().find(|value| value.0 == param.id)
                        {
                            param.value = *value;
                        }
                    }
                    errors
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .retain(|(target, _)| *target != binding.target);
                }
            }
            Ok(project)
        })
    }
    pub fn editor(&self, target: PluginTarget, open: bool) -> Result<(), String> {
        self.editor_binding(target, None, open)
    }
    /// Called by the session worker with recording exclusion, never by idle.
    pub(crate) fn capture_pending(
        &self,
        request: PendingUpdate,
        desired: Vec<windfall_project::PluginParameter>,
    ) -> Result<Option<CapturedState>, String> {
        let Update::Capture { target, serial } = request.update else {
            return Err("Not a native state request".into());
        };
        let revision = self.revision.clone();
        let errors = self.errors.clone();
        let captured =
            self.call_with_lifetime(Duration::from_secs(15), move |owner, lifetime| {
                if revision.load(std::sync::atomic::Ordering::Relaxed) != request.revision
                    || selected(&owner.selection, target) != Some(request.token)
                {
                    return Ok(None);
                }
                #[cfg(windows)]
                if let Some(record) = owner.bridges.get_mut(&request.token).filter(|record| {
                    record.playback
                        && record.revision == request.revision
                        && binding_identity(&record.binding) == request.binding
                        && record.dirty_serial >= serial
                        && record.dirty_serial > record.acknowledged_serial
                }) {
                    if lifetime.cancelled() {
                        record.capture_sent = false;
                        return Err("Plugin capture was cancelled".into());
                    }
                    let serial = record.dirty_serial;
                    let (bytes, parameters) = match record.capture(&desired) {
                        Ok(captured) => captured,
                        Err(error) => {
                            record.capture_sent = false;
                            return Err(error);
                        }
                    };
                    if lifetime.cancelled()
                        || revision.load(std::sync::atomic::Ordering::Acquire) != request.revision
                    {
                        record.capture_sent = false;
                        return Err("Plugin capture ownership changed".into());
                    }
                    return Ok(Some(CapturedState {
                        bytes,
                        parameters,
                        restart: false,
                        serial,
                    }));
                }
                let Some(record) = owner.instances.get_mut(&request.token).filter(|record| {
                    record.playback
                        && record.revision == request.revision
                        && record.identity == request.binding
                        && record.dirty
                        && record.dirty_serial >= serial
                }) else {
                    return Ok(None);
                };
                let captured = record.capture(lifetime, true, &desired)?;
                if let Some(error) = captured.recovery_error {
                    let mut errors = errors.lock().unwrap_or_else(|error| error.into_inner());
                    errors.retain(|(before, _)| *before != target);
                    errors.push((target, error));
                }
                let state = captured.state.into_bytes();
                let restart = record.restart;
                Ok(Some(CapturedState {
                    bytes: state,
                    parameters: captured.parameters,
                    restart,
                    serial: record.dirty_serial,
                }))
            })?;
        Ok(captured)
    }
    /// Only acknowledge after the session has accepted the captured edit.
    /// A concurrent parameter commit must leave its dirty ticket retryable.
    /// This legacy wrapper still reports delivery failures; new postcommit
    /// callers use `acknowledge_committed_capture` and retain its typed ticket.
    pub(crate) fn acknowledge_capture(
        &self,
        request: &PendingUpdate,
        captured: &CapturedState,
    ) -> Result<(), String> {
        self.retry_capture_ack(super::capture_ack::CaptureAckTicket {
            token: request.token,
            serial: captured.serial,
        })
        .map(|_| ())
    }
    /// Bookkeeping after document and ready-engine acceptance, off all guards.
    /// A transport failure cannot turn that accepted edit into a preparation
    /// refusal. Even a timeout after owner-side effects retains a retry ticket.
    pub(crate) fn acknowledge_committed_capture(
        &self,
        request: &PendingUpdate,
        captured: &CapturedState,
    ) -> super::CaptureAcknowledgement {
        self.acknowledge_committed_capture_with(request, captured, |_| {
            self.acknowledge_capture(request, captured)?;
            #[cfg(all(test, windows))]
            if let Update::Capture { target, .. } = request.update
                && self
                    .capture_ack_fault
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_mut()
                    .is_some_and(|fault| fault.consume(target))
            {
                return Err("Test bookkeeping acknowledgement delivery is unconfirmed".into());
            }
            Ok(super::capture_ack::CaptureAckCompletion::Acknowledged)
        })
    }
    /// Session tests may lose one real bookkeeping receipt after its effects.
    /// Runtime derives the actual request token/captured serial; tests cannot
    /// forge a ticket or stop capture/native construction through this seam.
    #[cfg(all(test, windows))]
    pub(crate) fn unconfirm_next_capture_acknowledgement(
        &self,
        target: PluginTarget,
    ) -> super::capture_ack::CaptureAckTestScope {
        super::capture_ack::CaptureAckTestScope::arm(self.capture_ack_fault.clone(), target)
    }
    fn acknowledge_committed_capture_with(
        &self,
        request: &PendingUpdate,
        captured: &CapturedState,
        deliver: impl FnOnce(
            super::capture_ack::CaptureAckTicket,
        ) -> Result<super::capture_ack::CaptureAckCompletion, String>,
    ) -> super::CaptureAcknowledgement {
        let ticket = super::capture_ack::CaptureAckTicket {
            token: request.token,
            serial: captured.serial,
        };
        match deliver(ticket) {
            Ok(_) => super::CaptureAcknowledgement::Confirmed,
            Err(warning) => super::CaptureAcknowledgement::Pending { ticket, warning },
        }
    }
    /// Retry an accepted capture's bookkeeping only. A unique retired owner is
    /// terminal completion. Newer dirty/restart intent remains owned by its
    /// newer serial; no state is recaptured and no native control is processed.
    pub(crate) fn retry_capture_ack(
        &self,
        ticket: super::capture_ack::CaptureAckTicket,
    ) -> Result<super::capture_ack::CaptureAckCompletion, String> {
        self.retry_capture_ack_with_checkpoint(ticket, Duration::from_secs(15), |_, _| {})
    }
    fn retry_capture_ack_with_checkpoint(
        &self,
        ticket: super::capture_ack::CaptureAckTicket,
        timeout: Duration,
        checkpoint: impl FnOnce(&mut Owner, &JobLifetime) + Send + 'static,
    ) -> Result<super::capture_ack::CaptureAckCompletion, String> {
        self.call_with_lifetime(timeout, move |owner, lifetime| {
            #[cfg(windows)]
            if let Some(record) = owner.bridges.get_mut(&ticket.token) {
                record.capture_sent = false;
                record.acknowledged_serial = record.acknowledged_serial.max(ticket.serial);
                checkpoint(owner, lifetime);
                return Ok(super::capture_ack::CaptureAckCompletion::Acknowledged);
            }
            let completion = if let Some(record) = owner.instances.get_mut(&ticket.token) {
                record.capture_sent = false;
                if record.dirty_serial == ticket.serial {
                    record.dirty = false;
                    record.restart = false;
                }
                super::capture_ack::CaptureAckCompletion::Acknowledged
            } else {
                super::capture_ack::CaptureAckCompletion::Retired
            };
            checkpoint(owner, lifetime);
            Ok(completion)
        })
    }
    pub fn editor_binding(
        &self,
        target: PluginTarget,
        binding: Option<PluginBinding>,
        open: bool,
    ) -> Result<(), String> {
        self.editor_binding_with_parent(target, binding, open, None)
    }
    pub fn editor_binding_with_parent(
        &self,
        target: PluginTarget,
        binding: Option<PluginBinding>,
        open: bool,
        parent: Option<usize>,
    ) -> Result<(), String> {
        let revision = self.revision.load(std::sync::atomic::Ordering::Relaxed);
        let scale = self.editor_scale_percent.clone();
        self.call(move |owner| {
            #[cfg(windows)]
            if let Some(record) =
                selected(&owner.selection, target).and_then(|token| owner.bridges.get(&token))
                && record.playback
                && record.revision == revision
                && binding.as_ref().is_none_or(|binding| {
                    binding.path == record.binding.path
                        && binding.id == record.binding.id
                        && binding.format == record.binding.format
                        && binding.state == record.binding.state
                        && binding.wrapper == record.binding.wrapper
                })
            {
                return record
                    .control
                    .editor_with_options(open, super::wrapper::editor(&record.binding, parent));
            }
            let record = selected(&owner.selection, target)
                .and_then(|token| owner.instances.get_mut(&token))
                .filter(|record| record.revision == revision)
                .filter(|record| {
                    binding.as_ref().is_none_or(|binding| {
                        record.binding.path == binding.path
                            && record.binding.id == binding.id
                            && record.binding.format == binding.format
                            && record.binding.state == binding.state
                            && record.binding.wrapper == binding.wrapper
                    })
                })
                .ok_or_else(|| "This plugin is missing or has not been prepared".to_owned())?;
            let instance = &mut record.plugin;
            if open {
                instance
                    .open_editor(&super::wrapper::editor(&record.binding, parent))
                    .map_err(|error| error.to_string())?;
                let application =
                    f64::from(scale.load(std::sync::atomic::Ordering::Relaxed)) / 100.0;
                if let Err(error) = instance.set_editor_scale(application)
                    && (application != 1.0
                        || error != windfall_plugin_host::gui::EditorError::UnsupportedScale)
                {
                    return Err(format!(
                        "The editor opened, but its content could not be scaled: {error}"
                    ));
                }
            } else {
                instance.close_editor();
            }
            Ok(())
        })
    }

    /// All GUI calls run on the same owner thread as editor creation. Stores a
    /// display preference for reopen; never changes plugin/project state.
    pub fn set_editor_scale(&self, application: f64) -> Result<(), String> {
        if ![0.75, 1.0, 1.25, 1.5, 1.75, 2.0].contains(&application) {
            return Err("Invalid application editor scale".into());
        }
        let preference = self.editor_scale_percent.clone();
        self.call(move |owner| {
            preference.store(
                (application * 100.0).round() as u32,
                std::sync::atomic::Ordering::Relaxed,
            );
            let mut errors = Vec::new();
            for record in owner.instances.values_mut() {
                if record.plugin.editor().is_some()
                    && let Err(error) = record.plugin.set_editor_scale(application)
                {
                    errors.push(format!("{}: {error}", record.binding.name));
                }
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            }
        })
    }
    #[cfg(all(test, windows))]
    pub(crate) fn bridge_fixture(helper: &std::path::Path) -> Arc<Self> {
        let mut runtime = Self::new().unwrap();
        runtime.bridge_enabled = true;
        runtime.bridge_helper = Some(helper.to_owned());
        Arc::new(runtime)
    }
    #[cfg(windows)]
    pub fn bridge_status(
        &self,
    ) -> Vec<(
        PluginTarget,
        u64,
        windfall_plugin_host::bridge::supervisor::Status,
    )> {
        let revision = self.document_revision();
        self.call(move |owner| {
            Ok(owner
                .bridges
                .iter()
                .filter(|(token, record)| {
                    record.playback
                        && record.revision == revision
                        && selected(&owner.selection, record.binding.target) == Some(**token)
                })
                .map(|(token, record)| (record.binding.target, *token, record.status()))
                .collect())
        })
        .unwrap_or_default()
    }
    #[cfg(windows)]
    fn bridge_audio(
        &self,
        binding: &PluginBinding,
        rate: u32,
    ) -> Result<super::bridge::Audio, String> {
        let binding = binding.clone();
        let revision = self.preparation_revision();
        let offline = self.rendering;
        let selection = self.selection(binding.target);
        let helper = super::bridge::helper_path(self.bridge_helper.as_deref())?;
        let render_error = self.render_error.clone();
        self.call(move |owner| {
            let stamp = Self::approved_binding(owner, &binding)?;
            owner.next = owner
                .next
                .checked_add(1)
                .ok_or("Plugin owner token exhausted")?;
            let token = owner.next;
            let controls = parameter_controls(&binding);
            let mut options =
                super::bridge::options(helper, &binding, stamp, token, revision, rate, offline)?;
            if !offline
                && let Some(old) = selected(&owner.selection, binding.target)
                    .and_then(|token| owner.bridges.get(&token))
                && old.binding == binding
                && let Some(state) = old.control.last_valid_state()
            {
                options.state = state.state;
            }
            let (record, audio) = super::bridge::launch(
                options,
                binding.clone(),
                controls.clone(),
                selection,
                render_error,
            )?;
            if !offline {
                owner
                    .controls
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(
                        token,
                        (binding.target, binding_identity(&binding), controls),
                    );
            }
            owner.bridges.insert(token, record);
            Ok(audio)
        })
    }
    pub fn errors(&self) -> Vec<(PluginTarget, String)> {
        self.errors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    fn record<T>(&self, target: PluginTarget, result: Result<T, String>) -> Result<T, String> {
        if self.rendering {
            return result;
        }
        let mut errors = self
            .errors
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        errors.retain(|(owner, _)| *owner != target);
        if let Err(error) = &result {
            errors.push((target, error.clone()));
        }
        result
    }
}

// Preserve inline preallocated instrument note storage across the existing
// owner/callback exchange. Boxing this variant would introduce another heap
// owner and retirement step into that allocation-free transfer contract.
#[allow(clippy::large_enum_variant)]
enum Adapter {
    Effect(PluginEffect),
    Instrument(PluginInstrument),
}
impl Adapter {
    fn dropped_events(&self) -> u32 {
        match self {
            Self::Effect(adapter) => adapter.health().dropped_events,
            Self::Instrument(adapter) => adapter.health().dropped_events,
        }
    }
    fn failed(&self) -> bool {
        match self {
            Self::Effect(adapter) => adapter.health().failed,
            Self::Instrument(adapter) => adapter.health().failed,
        }
    }
    fn latency(&self) -> usize {
        match self {
            Self::Effect(adapter) => adapter.latency_samples(),
            Self::Instrument(adapter) => adapter.latency_samples(),
        }
    }
    fn tail(&self) -> usize {
        match self {
            Self::Effect(adapter) => adapter.tail_samples(),
            Self::Instrument(adapter) => adapter.tail_samples(),
        }
    }
}
#[derive(Clone, Copy)]
struct ExpressionOwner {
    id: windfall_dsp::NoteInstanceId,
    native: u32,
    key: u8,
    channel: u8,
    velocity: f32,
    pan: f32,
    pitch: f32,
    expression: windfall_dsp::NoteExpression,
    started: bool,
    dirty: bool,
}
impl ExpressionOwner {
    const EMPTY: Self = Self {
        id: windfall_dsp::NoteInstanceId(0),
        native: 0,
        key: 0,
        channel: 0,
        velocity: 0.0,
        pan: 0.0,
        pitch: 0.0,
        expression: windfall_dsp::NoteExpression::NEUTRAL,
        started: false,
        dirty: false,
    };
    fn controls(
        self,
        map: windfall_dsp::ExpressionMap,
        law: windfall_core::PanLawRamp,
        clock: u64,
    ) -> windfall_plugin_host::PluginNoteExpression {
        let mut controls = map.plugin_controls(
            self.pan,
            f64::from(self.pitch) - f64::from(self.key),
            self.expression,
        );
        let (left, right) = law.gains_at(clock, (controls.values[1] * 2.0 - 1.0) as f32);
        let volume = left.max(right);
        controls.values[1] = if volume > 0.0 {
            f64::from((right - left) / volume + 1.0) * 0.5
        } else {
            0.5
        };
        controls.values[0] *= f64::from(volume);
        controls.mask |= windfall_plugin_host::NoteExpressionKind::Pan.mask()
            | windfall_plugin_host::NoteExpressionKind::Volume.mask();
        windfall_plugin_host::PluginNoteExpression {
            values: controls.values,
            mask: controls.mask,
        }
    }
}

struct Audio {
    sidechain_input: Option<u32>,
    instrument: bool,
    selection: Option<Arc<std::sync::atomic::AtomicU64>>,
    ownership: Option<AudioOwnership<Adapter>>,
    latency: usize,
    tail: usize,
    token: u64,
    jobs: mpsc::Sender<Job>,
    /// Allocated from the binding on control; callbacks only replace values.
    pending_params: Box<[(u32, Option<f32>)]>,
    controls: ParameterControls,
    held: [f32; 16 * 128],
    replayed: [f32; 16 * 128],
    midi_controls: Box<[(Option<[u8; 3]>, bool)]>,
    expression_owners: Box<[ExpressionOwner]>,
    expression_map: windfall_dsp::ExpressionMap,
    note_law: windfall_core::PanLawRamp,
    pan_clock: u64,
    pan_ramp_frames: u64,
    next_note_id: u32,
    note_instances: bool,
    note_pitch: bool,
    reconcile_notes: bool,
    reset_notes: bool,
    transport: Option<windfall_plugin_host::Transport>,
    tempo: Option<f32>,
}
impl Audio {
    fn reconcile(&mut self) {
        let Self {
            ownership,
            pending_params,
            sidechain_input,
            controls,
            held,
            replayed,
            midi_controls,
            expression_owners,
            expression_map,
            note_law,
            pan_clock,
            reconcile_notes,
            reset_notes,
            transport,
            tempo,
            ..
        } = self;
        let Some(adapter) = ownership.as_mut().and_then(AudioOwnership::current_mut) else {
            return;
        };
        if let Adapter::Effect(effect) = adapter {
            effect.processor().set_sidechain_input(*sidechain_input);
        }
        if let Adapter::Instrument(instrument) = adapter {
            for (bytes, dirty) in midi_controls.iter_mut() {
                if *reset_notes && bytes.is_some() {
                    *dirty = true;
                }
                if *dirty && let Some(bytes) = bytes {
                    let len = if matches!(bytes[0] & 0xf0, 0xc0 | 0xd0) {
                        2
                    } else {
                        3
                    };
                    if instrument.midi_message(&bytes[..len]) {
                        *dirty = false;
                    }
                }
            }
        }
        if let Adapter::Instrument(instrument) = adapter
            && *reconcile_notes
        {
            instrument.set_bend_range_semitones(expression_map.bend_range_semitones);
            if *reset_notes {
                if !instrument.all_notes_off() {
                    return;
                }
                replayed.fill(0.0);
                for note in expression_owners.iter_mut() {
                    note.started = false;
                    note.dirty = true;
                }
                *reset_notes = false;
            }
            let mut complete = true;
            for note in expression_owners.iter_mut().filter(|note| note.native != 0) {
                let accepted = if !note.started {
                    instrument.note_on_expression_instance(
                        note.native,
                        note.key,
                        note.channel,
                        note.velocity,
                        note.controls(*expression_map, *note_law, *pan_clock),
                    )
                } else if note.dirty {
                    instrument.set_note_expression(
                        note.native,
                        note.controls(*expression_map, *note_law, *pan_clock),
                    )
                } else {
                    true
                };
                if accepted {
                    note.started = true;
                    note.dirty = false;
                } else {
                    complete = false;
                }
            }
            for key in 0..16 * 128 {
                if replayed[key] != held[key] {
                    let accepted = if held[key] > 0.0 {
                        instrument.note_on_channel((key % 128) as u8, (key / 128) as u8, held[key])
                    } else {
                        instrument.note_off_channel((key % 128) as u8, (key / 128) as u8)
                    };
                    if accepted {
                        replayed[key] = held[key];
                    } else {
                        complete = false;
                    }
                }
            }
            *reconcile_notes = !complete;
        }
        let processor = match adapter {
            Adapter::Effect(adapter) => adapter.processor(),
            Adapter::Instrument(adapter) => adapter.processor(),
        };
        if let Some(transport) = transport {
            processor.set_transport(*transport);
        }
        if let Some(tempo) = tempo {
            processor.set_tempo(f64::from(*tempo));
        }
        for ((id, value), control) in pending_params.iter_mut().zip(controls.iter()) {
            debug_assert_eq!(*id, control.id);
            if control.settled(
                control
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire),
            ) {
                *value = None;
            }
            if let Some(pending) = *value
                && processor.set_param(0, *id, f64::from(pending))
            {
                *value = None;
            }
        }
    }
    fn adapter(&self) -> Option<&Adapter> {
        self.ownership.as_ref().and_then(AudioOwnership::current)
    }
    fn adapter_mut(&mut self) -> Option<&mut Adapter> {
        self.ownership
            .as_mut()
            .and_then(AudioOwnership::current_mut)
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        if let Some(selection) = &self.selection {
            let _ = selection.compare_exchange(
                self.token,
                0,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        if let Some(mut ownership) = self.ownership.take() {
            let token = self.token;
            let job: Job = Box::new(move |owner| {
                if let Some(mut record) = owner.instances.remove(&token) {
                    owner
                        .controls
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&token);
                    record.plugin.close_editor();
                    if let Some(adapter) = ownership.retire() {
                        record.retire(adapter);
                    }
                    if let Some(adapter) = record.ownership.take_returned() {
                        record.retire(adapter);
                    }
                    if let Some(adapter) = record.returned.take() {
                        record.retire(adapter);
                    }
                }
            });
            if let Err(error) = self.jobs.send(job) {
                // The native owner has failed. Leaking is preferable to COM
                // teardown/deactivation on a controller or audio thread.
                std::mem::forget(error.0);
            }
        }
    }
}
impl HostedEffect for Audio {
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.sidechain_input = input;
    }
    fn adopt_parameters(&mut self, parameters: &[windfall_project::PluginParameter]) {
        for control in self.controls.iter() {
            if let Some(param) = parameters.iter().find(|param| param.id == control.id) {
                control.observe_document(param.value);
            }
        }
    }
    fn control_boundary(&mut self) {
        if let Some(ownership) = &mut self.ownership {
            if let Some(adapter) = ownership.current() {
                self.latency = adapter.latency();
                self.tail = adapter.tail();
            }
            ownership.boundary();
            if ownership.current().is_none() {
                self.reconcile_notes = true;
                self.reset_notes = true;
            }
        }
    }
    fn transport(&mut self, transport: windfall_engine::plugins::PluginTransport) {
        if let Some(selection) = &self.selection {
            selection.store(self.token, std::sync::atomic::Ordering::Relaxed);
        }
        let transport = windfall_plugin_host::Transport {
            playing: transport.playing,
            tempo_bpm: transport.tempo_bpm,
            position_beats: transport.position_beats,
            position_seconds: transport.position_seconds,
            numerator: transport.numerator,
            denominator: transport.denominator,
            meter_anchor: transport
                .meter_anchor
                .map(|anchor| windfall_plugin_host::MeterAnchor {
                    bar_origin_beats: anchor.bar_origin_beats,
                    bar_origin_index: anchor.bar_origin_index,
                }),
        };
        self.transport = Some(transport);
        match self.adapter_mut() {
            Some(Adapter::Effect(adapter)) => adapter.set_transport(transport),
            Some(Adapter::Instrument(adapter)) => adapter.set_transport(transport),
            None => {}
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        if self.instrument && !self.note_law.settled(self.pan_clock) {
            let frames = left.len().min(right.len());
            for index in 0..frames {
                for slot in 0..self.expression_owners.len() {
                    if self.expression_owners[slot].native != 0 {
                        self.publish_expression_owner(slot);
                    }
                }
                self.process_pan_block(
                    &mut left[index..index + 1],
                    &mut right[index..index + 1],
                    None,
                );
                self.pan_clock = self.pan_clock.saturating_add(1);
            }
        } else {
            self.process_pan_block(left, right, key);
            self.pan_clock = self
                .pan_clock
                .saturating_add(left.len().min(right.len()) as u64);
        }
    }
    fn set_param(&mut self, id: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        if let Some(control) = self.controls.iter().find(|control| control.id == id) {
            control.publish(value);
        }
        let accepted = match self.adapter_mut() {
            Some(Adapter::Effect(adapter)) => {
                adapter.processor().set_param(0, id, f64::from(value))
            }
            Some(Adapter::Instrument(adapter)) => {
                adapter.processor().set_param(0, id, f64::from(value))
            }
            None => false,
        };
        if let Some((_, pending)) = self.pending_params.iter_mut().find(|(key, _)| *key == id) {
            *pending = (!accepted).then_some(value);
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.tempo = Some(bpm);
        match self.adapter_mut() {
            Some(Adapter::Effect(adapter)) => adapter.set_tempo(bpm),
            Some(Adapter::Instrument(adapter)) => adapter.set_tempo(bpm),
            None => {}
        }
    }
    fn latency(&self) -> usize {
        self.adapter().map_or(self.latency, Adapter::latency)
    }
    fn tail(&self) -> usize {
        self.adapter().map_or(self.tail, Adapter::tail)
    }
}

impl Audio {
    fn process_pan_block(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        // Reconcile after engine commands and note expirations, immediately
        // before sounding. A boundary alone must never resurrect a released key.
        self.reconcile();
        let dropped = self.adapter().map(Adapter::dropped_events);
        let instrument = self.instrument;
        match self.adapter_mut() {
            Some(Adapter::Effect(adapter)) => adapter.process_sidechain(left, right, key),
            Some(Adapter::Instrument(adapter)) => adapter.process(left, right),
            None if instrument => {
                left.fill(0.0);
                right.fill(0.0);
            }
            None => {}
        }
        if !left.is_empty()
            && !right.is_empty()
            && self.adapter().is_some_and(|adapter| {
                !adapter.failed() && Some(adapter.dropped_events()) == dropped
            })
        {
            for ((id, pending), control) in self.pending_params.iter().zip(self.controls.iter()) {
                debug_assert_eq!(*id, control.id);
                if pending.is_none() {
                    control.applied.store(
                        control
                            .generation
                            .load(std::sync::atomic::Ordering::Acquire),
                        std::sync::atomic::Ordering::Release,
                    );
                    control.applied_document.fetch_max(
                        control
                            .observed_document
                            .load(std::sync::atomic::Ordering::Acquire),
                        std::sync::atomic::Ordering::Release,
                    );
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "capture_ack_tests.rs"]
mod capture_ack_tests;
#[cfg(all(test, windows))]
#[path = "ownership_tests.rs"]
pub(crate) mod ownership_tests;
#[cfg(all(test, windows))]
#[path = "sidechain_bus_tests.rs"]
mod sidechain_bus_tests;
impl HostedInstrument for Audio {
    fn midi_message(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > 3 || bytes[1..].iter().any(|byte| *byte > 127) {
            return false;
        }
        let kind = bytes[0] & 0xf0;
        let channel = bytes[0] & 15;
        if bytes.len() == 3 && kind == 0x90 && bytes[2] != 0 {
            self.start_on_channel(bytes[1], channel, f32::from(bytes[2]) / 127.0);
            return true;
        }
        if bytes.len() == 3 && (kind == 0x80 || kind == 0x90) {
            self.end_on_channel(bytes[1], channel);
            return true;
        }
        let index = match kind {
            0xb0 if bytes.len() == 3 => usize::from(bytes[1]),
            0xd0 if bytes.len() == 2 => 128,
            0xe0 if bytes.len() == 3 => 129,
            0xc0 if bytes.len() == 2 => 130,
            _ => return false,
        };
        let mut stored = [0; 3];
        stored[..bytes.len()].copy_from_slice(bytes);
        let slot = usize::from(channel) * 131 + index;
        self.midi_controls[slot] = (Some(stored), true);
        if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
            if adapter.midi_message(bytes) {
                self.midi_controls[slot].1 = false;
                return true;
            }
            return false;
        }
        true // retained until ownership returns, just like pending parameters
    }

    fn set_expression_map(&mut self, map: windfall_dsp::ExpressionMap) {
        if self.expression_map == map {
            return;
        }
        self.expression_map = map;
        for slot in 0..self.expression_owners.len() {
            if self.expression_owners[slot].native != 0 {
                self.expression_owners[slot].dirty = true;
                self.publish_expression_owner(slot);
            }
        }
    }
    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        if self.pan_clock == 0 {
            self.note_law = windfall_core::PanLawRamp::at_rest(law);
        } else {
            self.note_law
                .retarget(law, self.pan_clock, self.pan_ramp_frames);
        }
        for note in self
            .expression_owners
            .iter_mut()
            .filter(|note| note.native != 0)
        {
            note.dirty = true;
        }
        self.reconcile_notes = true;
    }
    fn supports_note_instances(&self) -> bool {
        self.note_instances
    }
    fn supports_note_channels(&self) -> bool {
        true
    }
    fn supports_note_pitch(&self) -> bool {
        self.note_pitch
    }

    fn note_on_instance(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let expression = expression.clamped();
        if !self.note_instances {
            self.start_on_channel(key, expression.color_group.unwrap_or(0), velocity);
            return;
        }
        if id.0 == 0 || !velocity.is_finite() {
            return;
        }
        if velocity <= 0.0 {
            self.end_instance(id);
            return;
        }
        self.end_instance(id);
        let slot = self
            .expression_owners
            .iter()
            .position(|note| note.native == 0)
            .unwrap_or_else(|| {
                self.expression_owners
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, note)| note.id.0)
                    .map_or(0, |(index, _)| index)
            });
        if self.expression_owners.is_empty() {
            return;
        }
        if self.expression_owners[slot].native != 0 {
            self.end_instance(self.expression_owners[slot].id);
        }
        let native = loop {
            let next = self.next_note_id;
            self.next_note_id = if next == i32::MAX as u32 {
                windfall_plugin_host::FIRST_NOTE_INSTANCE_ID
            } else {
                next + 1
            };
            if !self
                .expression_owners
                .iter()
                .any(|note| note.native == next)
            {
                break next;
            }
        };
        self.expression_owners[slot] = ExpressionOwner {
            id,
            native,
            key: key.min(127),
            channel: expression.color_group.unwrap_or(0),
            velocity: velocity.clamp(0.0, 1.0),
            pan: if pan.is_finite() {
                pan.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            pitch: f32::from(key.min(127)) + expression.fine_pitch_cents / 100.0,
            expression,
            started: false,
            dirty: true,
        };
        self.publish_expression_owner(slot);
    }
    fn note_off_instance(&mut self, id: windfall_dsp::NoteInstanceId, key: u8) {
        if self.note_instances {
            self.end_instance(id);
        } else {
            self.end_on_channel(key, 0);
        }
    }
    fn note_off_on_channel(&mut self, id: windfall_dsp::NoteInstanceId, key: u8, channel: u8) {
        if self.note_instances {
            self.end_instance(id);
        } else {
            self.end_on_channel(key, channel);
        }
    }
    fn set_note_expression(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let Some(slot) = self
            .expression_owners
            .iter()
            .position(|note| note.native != 0 && note.id == id)
        else {
            return;
        };
        let expression = expression.clamped();
        let note = &mut self.expression_owners[slot];
        let key = f32::from(note.key);
        note.pitch = (note.pitch
            + (expression.fine_pitch_cents - note.expression.fine_pitch_cents) / 100.0)
            .clamp(key - 120.0, key + 120.0);
        note.expression = expression;
        note.pan = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        note.dirty = true;
        self.publish_expression_owner(slot);
    }
    fn set_note_pitch(&mut self, id: windfall_dsp::NoteInstanceId, pitch: f32) {
        let Some(slot) = self
            .expression_owners
            .iter()
            .position(|note| note.native != 0 && note.id == id)
        else {
            return;
        };
        if !pitch.is_finite() {
            return;
        }
        let key = f32::from(self.expression_owners[slot].key);
        self.expression_owners[slot].pitch = pitch.clamp(key - 120.0, key + 120.0);
        self.expression_owners[slot].dirty = true;
        self.publish_expression_owner(slot);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        self.start_on_channel(key, 0, velocity);
    }
    fn note_off(&mut self, key: u8) {
        self.end_on_channel(key, 0);
    }
    fn all_notes_off(&mut self) {
        self.held.fill(0.0);
        self.expression_owners.fill(ExpressionOwner::EMPTY);
        if !self.reconcile_notes {
            let accepted = if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
                adapter.all_notes_off()
            } else {
                false
            };
            if !accepted {
                self.reconcile_notes = true;
                self.reset_notes = true;
            }
        } else {
            self.reset_notes = true;
        }
    }
    fn voices(&self) -> usize {
        if self.reconcile_notes {
            return self.held.iter().filter(|velocity| **velocity > 0.0).count()
                + self
                    .expression_owners
                    .iter()
                    .filter(|note| note.native != 0)
                    .count();
        }
        match self.adapter() {
            Some(Adapter::Instrument(adapter)) => adapter.active_voices(),
            _ => 0,
        }
    }
}

impl Audio {
    fn publish_expression_owner(&mut self, slot: usize) {
        if self.reconcile_notes {
            return;
        }
        let note = self.expression_owners[slot];
        let controls = note.controls(self.expression_map, self.note_law, self.pan_clock);
        let bend_range = self.expression_map.bend_range_semitones;
        let accepted = if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
            adapter.set_bend_range_semitones(bend_range);
            if note.started {
                adapter.set_note_expression(note.native, controls)
            } else {
                adapter.note_on_expression_instance(
                    note.native,
                    note.key,
                    note.channel,
                    note.velocity,
                    controls,
                )
            }
        } else {
            false
        };
        if accepted {
            self.expression_owners[slot].started = true;
            self.expression_owners[slot].dirty = false;
        } else {
            self.reconcile_notes = true;
            if self.adapter().is_none() {
                self.reset_notes = true;
            }
        }
    }
    fn end_instance(&mut self, id: windfall_dsp::NoteInstanceId) {
        let Some(slot) = self
            .expression_owners
            .iter()
            .position(|note| note.native != 0 && note.id == id)
        else {
            return;
        };
        let note = self.expression_owners[slot];
        self.expression_owners[slot] = ExpressionOwner::EMPTY;
        if note.started {
            let accepted = if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
                adapter.note_off_instance(note.native, note.expression.release)
            } else {
                false
            };
            if !accepted {
                self.reconcile_notes = true;
                self.reset_notes = true;
            }
        }
    }
    fn start_on_channel(&mut self, key: u8, channel: u8, velocity: f32) {
        let key = key.min(127);
        let channel = channel.min(15);
        self.held[usize::from(channel) * 128 + usize::from(key)] = if velocity.is_finite() {
            velocity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if !self.reconcile_notes {
            let accepted = if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
                adapter.note_on_channel(key, channel, velocity)
            } else {
                false
            };
            if !accepted {
                self.reconcile_notes = true;
                self.reset_notes = true;
            }
        }
    }
    fn end_on_channel(&mut self, key: u8, channel: u8) {
        let key = key.min(127);
        let channel = channel.min(15);
        self.held[usize::from(channel) * 128 + usize::from(key)] = 0.0;
        if !self.reconcile_notes {
            let accepted = if let Some(Adapter::Instrument(adapter)) = self.adapter_mut() {
                adapter.note_off_channel(key, channel)
            } else {
                false
            };
            if !accepted {
                self.reconcile_notes = true;
                self.reset_notes = true;
            }
        }
    }
}
impl PluginFactory for Runtime {
    fn provider_identity(&self) -> u64 {
        Arc::as_ptr(&self.revision) as usize as u64
    }
    fn render_factory(&self) -> Option<Arc<dyn PluginFactory>> {
        if self.rendering {
            return None;
        }
        let mut runtime = self.clone();
        runtime.render_error = Arc::new(Mutex::new(None));
        runtime.rendering = true;
        Some(Arc::new(runtime))
    }
    fn render_error(&self) -> Option<String> {
        if self.rendering {
            self.render_error
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        } else {
            None
        }
    }
    fn revision(&self) -> u64 {
        self.preparation_revision()
    }
    fn effect(
        &self,
        binding: &PluginBinding,
        rate: u32,
        block: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        #[cfg(windows)]
        let default_bridge = self.bridge_enabled;
        #[cfg(not(windows))]
        let default_bridge = false;
        let bridged = self.record(
            binding.target,
            super::wrapper::use_bridge(binding, default_bridge),
        )?;
        #[cfg(windows)]
        if bridged {
            let result = self.bridge_audio(binding, rate);
            if self.rendering
                && let Err(error) = &result
            {
                *self
                    .render_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(error.clone());
            }
            return self
                .record(binding.target, result)
                .map(|audio| Box::new(audio) as Box<dyn HostedEffect>);
        }
        #[cfg(not(windows))]
        let _ = bridged;
        let binding = binding.clone();
        let target = binding.target;
        let jobs = self.jobs.clone();
        let revision = self.preparation_revision();
        let playback = !self.rendering;
        let selection = self.selection(target);
        let fallback_selection = selection.clone();
        self.record(
            target,
            self.call(move |owner| {
                let mut instance = Self::create(owner, &binding)?;
                let adapter = instance
                    .prepare_effect_with_options(
                        rate as f32,
                        super::wrapper::max_block(&binding, block),
                        &super::wrapper::native(&binding)?,
                    )
                    .map_err(|error| error.to_string())?;
                if adapter.latency_samples() > rate as usize {
                    let _ = instance.release_effect(adapter);
                    return Err("Plugin latency exceeds the one-second compensation limit".into());
                }
                owner.next += 1;
                let token = owner.next;
                let adapter = Adapter::Effect(adapter);
                let latency = adapter.latency();
                let tail = adapter.tail();
                let (ownership, audio) = exchange(adapter);
                let pending_params = binding
                    .parameters
                    .iter()
                    .filter(|param| !param.read_only)
                    .map(|param| (param.id, None))
                    .collect();
                let controls = parameter_controls(&binding);
                if playback {
                    owner
                        .controls
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .insert(
                            token,
                            (target, binding_identity(&binding), controls.clone()),
                        );
                }
                owner.instances.insert(
                    token,
                    Instance {
                        identity: binding_identity(&binding),
                        target,
                        plugin: instance,
                        binding,
                        revision,
                        playback,
                        ownership,
                        rate,
                        block,
                        instrument: false,
                        controls: controls.clone(),
                        latency,
                        tail,
                        returned: None,
                        dirty: false,
                        dirty_serial: 0,
                        capture_sent: false,
                        restart: false,
                        notifications: Vec::new(),
                    },
                );

                Ok(Box::new(Audio {
                    sidechain_input: None,
                    instrument: false,
                    selection,
                    ownership: Some(audio),
                    latency,
                    tail,
                    token,
                    jobs,
                    pending_params,
                    controls,
                    held: [0.0; 16 * 128],
                    replayed: [0.0; 16 * 128],
                    midi_controls: vec![(None, false); 16 * 131].into_boxed_slice(),
                    expression_owners: Box::new([]),
                    expression_map: windfall_dsp::ExpressionMap::default(),
                    note_law: windfall_core::PanLawRamp::at_rest(windfall_core::PanLaw::Balance),
                    pan_clock: 0,
                    pan_ramp_frames: (f64::from(rate) * 0.005).round().max(1.0) as u64,
                    next_note_id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
                    note_instances: false,
                    note_pitch: false,
                    reconcile_notes: false,
                    reset_notes: false,
                    transport: None,
                    tempo: None,
                }) as Box<dyn HostedEffect>)
            }),
        )
        .or_else(|_| {
            Ok(Box::new(Audio {
                sidechain_input: None,
                instrument: false,
                selection: fallback_selection,
                ownership: None,
                latency: 0,
                tail: 0,
                token: 0,
                jobs: self.jobs.clone(),
                pending_params: Box::new([]),
                controls: Arc::new([]),
                held: [0.0; 16 * 128],
                replayed: [0.0; 16 * 128],
                midi_controls: vec![(None, false); 16 * 131].into_boxed_slice(),
                expression_owners: Box::new([]),
                expression_map: windfall_dsp::ExpressionMap::default(),
                note_law: windfall_core::PanLawRamp::at_rest(windfall_core::PanLaw::Balance),
                pan_clock: 0,
                pan_ramp_frames: (f64::from(rate) * 0.005).round().max(1.0) as u64,
                next_note_id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
                note_instances: false,
                note_pitch: false,
                reconcile_notes: false,
                reset_notes: false,
                transport: None,
                tempo: None,
            }) as Box<dyn HostedEffect>)
        })
    }
    fn instrument(
        &self,
        binding: &PluginBinding,
        rate: u32,
        block: usize,
    ) -> Result<Box<dyn HostedInstrument>, String> {
        #[cfg(windows)]
        let default_bridge = self.bridge_enabled;
        #[cfg(not(windows))]
        let default_bridge = false;
        let bridged = self.record(
            binding.target,
            super::wrapper::use_bridge(binding, default_bridge),
        )?;
        #[cfg(windows)]
        if bridged {
            let result = self.bridge_audio(binding, rate);
            if self.rendering
                && let Err(error) = &result
            {
                *self
                    .render_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(error.clone());
            }
            return self
                .record(binding.target, result)
                .map(|audio| Box::new(audio) as Box<dyn HostedInstrument>);
        }
        #[cfg(not(windows))]
        let _ = bridged;
        let binding = binding.clone();
        let target = binding.target;
        let jobs = self.jobs.clone();
        let revision = self.preparation_revision();
        let playback = !self.rendering;
        let selection = self.selection(target);
        let fallback_selection = selection.clone();
        self.record(
            target,
            self.call(move |owner| {
                let mut instance = Self::create(owner, &binding)?;
                let adapter = instance
                    .prepare_instrument_with_options(
                        rate as f32,
                        super::wrapper::max_block(&binding, block),
                        &super::wrapper::native(&binding)?,
                    )
                    .map_err(|error| error.to_string())?;
                if adapter.latency_samples() > rate as usize {
                    let _ = instance.release_instrument(adapter);
                    return Err("Plugin latency exceeds the one-second compensation limit".into());
                }
                owner.next += 1;
                let token = owner.next;
                let note_instances = adapter.supports_note_instances();
                let note_pitch = adapter.supports_note_pitch();
                let adapter = Adapter::Instrument(adapter);
                let latency = adapter.latency();
                let tail = adapter.tail();
                let (ownership, audio) = exchange(adapter);
                let pending_params = binding
                    .parameters
                    .iter()
                    .filter(|param| !param.read_only)
                    .map(|param| (param.id, None))
                    .collect();
                let controls = parameter_controls(&binding);
                if playback {
                    owner
                        .controls
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .insert(
                            token,
                            (target, binding_identity(&binding), controls.clone()),
                        );
                }
                owner.instances.insert(
                    token,
                    Instance {
                        identity: binding_identity(&binding),
                        target,
                        plugin: instance,
                        binding,
                        revision,
                        playback,
                        ownership,
                        rate,
                        block,
                        instrument: true,
                        controls: controls.clone(),
                        latency,
                        tail,
                        returned: None,
                        dirty: false,
                        dirty_serial: 0,
                        capture_sent: false,
                        restart: false,
                        notifications: Vec::new(),
                    },
                );

                Ok(Box::new(Audio {
                    sidechain_input: None,
                    instrument: true,
                    selection,
                    ownership: Some(audio),
                    latency,
                    tail,
                    token,
                    jobs,
                    pending_params,
                    controls,
                    held: [0.0; 16 * 128],
                    replayed: [0.0; 16 * 128],
                    midi_controls: vec![(None, false); 16 * 131].into_boxed_slice(),
                    expression_owners: vec![
                        ExpressionOwner::EMPTY;
                        windfall_plugin_host::MAX_NOTE_INSTANCES
                    ]
                    .into_boxed_slice(),
                    note_law: windfall_core::PanLawRamp::at_rest(windfall_core::PanLaw::Balance),
                    pan_clock: 0,
                    pan_ramp_frames: (f64::from(rate) * 0.005).round().max(1.0) as u64,
                    next_note_id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
                    expression_map: windfall_dsp::ExpressionMap::default(),
                    note_instances,
                    note_pitch,
                    reconcile_notes: false,
                    reset_notes: false,
                    transport: None,
                    tempo: None,
                }) as Box<dyn HostedInstrument>)
            }),
        )
        .or_else(|_| {
            Ok(Box::new(Audio {
                sidechain_input: None,
                instrument: true,
                selection: fallback_selection,
                ownership: None,
                latency: 0,
                tail: 0,
                token: 0,
                jobs: self.jobs.clone(),
                pending_params: Box::new([]),
                controls: Arc::new([]),
                held: [0.0; 16 * 128],
                replayed: [0.0; 16 * 128],
                midi_controls: vec![(None, false); 16 * 131].into_boxed_slice(),
                expression_owners: vec![
                    ExpressionOwner::EMPTY;
                    windfall_plugin_host::MAX_NOTE_INSTANCES
                ]
                .into_boxed_slice(),
                note_law: windfall_core::PanLawRamp::at_rest(windfall_core::PanLaw::Balance),
                pan_clock: 0,
                pan_ramp_frames: (f64::from(rate) * 0.005).round().max(1.0) as u64,
                next_note_id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
                expression_map: windfall_dsp::ExpressionMap::default(),
                note_instances: false,
                note_pitch: false,
                reconcile_notes: false,
                reset_notes: false,
                transport: None,
                tempo: None,
            }) as Box<dyn HostedInstrument>)
        })
    }
}
