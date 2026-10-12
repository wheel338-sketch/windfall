//! Cached, isolated discovery and a native owner thread for audio instances.
#[cfg(windows)]
mod bridge;
mod capture_ack;
mod manager_workflows;
mod runtime;
mod wrapper;
pub(crate) use capture_ack::{CaptureAcknowledgement, CaptureOutcome};
pub use manager_workflows::Rescan;
pub use runtime::Runtime;
pub(crate) use runtime::binding_identity;
#[cfg(all(test, windows))]
pub(crate) use runtime::ownership_tests::fixture as vst3_fixture;
pub(crate) use runtime::{PendingUpdate, Update};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use windfall_ipc::{PluginEntry, PluginManagerState};
use windfall_plugin_host::{
    PluginFormat, PluginKind, paths,
    scan::{PluginCatalog, ProcessRunner},
};
use windfall_project::{PluginBinding, PluginTarget};

const ACKNOWLEDGEMENT_WARNING: &str =
    "Native plugin state was accepted; acknowledgement is pending:";

fn overlay_capture_ack_warning(status: &mut PluginManagerState, warning: Option<&str>) {
    if let Some(warning) = warning {
        status.error = Some(format!("{ACKNOWLEDGEMENT_WARNING} {warning}"));
    }
}

fn update_capture_status(
    status: &mut PluginManagerState,
    pending_empty: bool,
    captures: &capture_ack::CaptureUpdates,
) {
    // The acknowledgement warning belongs only to state()'s cloned snapshot.
    // Keep other producers' errors intact while delivery is pending or settles.
    if pending_empty
        && captures.is_empty()
        && status.error.as_ref().is_some_and(|error| {
            error.starts_with("Native plugin edit is waiting:")
                || error.starts_with("Native plugin state is waiting:")
        })
    {
        status.error = None;
    }
}

pub struct PluginManager {
    catalog: Mutex<PluginCatalog>,
    status: Mutex<PluginManagerState>,
    cache: PathBuf,
    folders_file: PathBuf,
    pub runtime: Arc<Runtime>,
    session: Mutex<Option<crate::session::WeakSession>>,
    retry_requested: std::sync::atomic::AtomicBool,
    // Other status producers (for example scanning) cannot hide an accepted
    // state's outstanding acknowledgement while its control worker is waiting.
    capture_ack_warning: Mutex<Option<String>>,
    #[cfg(test)]
    fixture_scanner: Mutex<Option<PathBuf>>,
}
impl PluginManager {
    #[cfg(all(test, windows))]
    pub(crate) fn fixture_runtime(folder: &Path, runtime: Arc<Runtime>) -> Arc<Self> {
        let mut manager = Self::new(folder).unwrap();
        Arc::get_mut(&mut manager).unwrap().runtime = runtime;
        manager
    }
    #[cfg(all(test, windows))]
    pub(crate) fn fixture(folder: &Path, file: &Path, scanner: &Path) -> Arc<Self> {
        let manager = Self::new(folder).unwrap();
        *manager.fixture_scanner.lock().unwrap() = Some(scanner.to_path_buf());
        manager.status.lock().unwrap().folders =
            vec![file.parent().unwrap().to_string_lossy().into_owned()];
        let files = paths::find_plugins(&[file.parent().unwrap().to_path_buf()]);
        manager
            .catalog
            .lock()
            .unwrap()
            .refresh(&files, &ProcessRunner::new(scanner), &mut |_| {})
            .unwrap();
        manager.update_entries();
        manager
            .runtime
            .approve_catalog(&manager.catalog.lock().unwrap());
        manager
    }
    #[cfg(all(test, windows))]
    pub(crate) fn bridge_fixture(folder: &Path, file: &Path, helper: &Path) -> Arc<Self> {
        let mut manager = Self::fixture(folder, file, helper);
        Arc::get_mut(&mut manager).unwrap().runtime = Runtime::bridge_fixture(helper);
        manager
            .runtime
            .approve_catalog(&manager.catalog.lock().unwrap());
        manager
    }
    pub fn new(folder: &Path) -> Result<Arc<Self>, String> {
        let cache = folder.join("plugins.json");
        let folders_file = folder.join("plugin-folders.json");
        let mut folders = paths::standard_folders(PluginFormat::Clap);
        folders.extend(paths::standard_folders(PluginFormat::Vst3));
        let custom: Vec<String> = std::fs::read(&folders_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        folders.extend(custom.into_iter().map(PathBuf::from));
        folders.sort();
        folders.dedup();
        let manager = Arc::new(Self {
            catalog: Mutex::new(PluginCatalog::load(&cache)),
            status: Mutex::new(PluginManagerState {
                folders: folders
                    .iter()
                    .map(|folder| folder.to_string_lossy().into_owned())
                    .collect(),
                ..Default::default()
            }),
            cache,
            folders_file,
            runtime: Arc::new(Runtime::new()?),
            session: Mutex::new(None),
            retry_requested: std::sync::atomic::AtomicBool::new(false),
            capture_ack_warning: Mutex::new(None),
            #[cfg(test)]
            fixture_scanner: Mutex::new(None),
        });
        manager.update_entries();
        Ok(manager)
    }
    fn publish_capture_status(&self, pending_empty: bool, captures: &capture_ack::CaptureUpdates) {
        *self
            .capture_ack_warning
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = captures.warning().map(str::to_owned);
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        update_capture_status(&mut status, pending_empty, captures);
    }
    pub fn state(&self) -> PluginManagerState {
        let mut state = self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        overlay_capture_ack_warning(
            &mut state,
            self.capture_ack_warning
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_deref(),
        );
        state.instances = self
            .runtime
            .errors()
            .into_iter()
            .map(|(target, error)| windfall_ipc::PluginInstanceStatus { target, error })
            .collect();
        state
    }
    pub fn attach(self: &Arc<Self>, session: crate::session::WeakSession) {
        *self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(session);
        let manager = Arc::downgrade(self);
        std::thread::spawn(move || {
            let mut pending: Vec<(u64, u64, u64, windfall_project::Command, Option<u64>)> =
                Vec::new();
            let mut captures = capture_ack::CaptureUpdates::default();
            let mut capture_retry = std::time::Instant::now();
            let mut acknowledgement_retry = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let Some(manager) = manager.upgrade() else {
                    break;
                };
                let Some(session) = manager
                    .session
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                    .and_then(crate::session::WeakSession::upgrade)
                else {
                    break;
                };
                for update in manager.runtime.drain() {
                    match update.update.clone() {
                        runtime::Update::Parameter {
                            target,
                            id,
                            value,
                            gesture,
                        } => {
                            pending.retain(|(_, _, _, command, _)| !matches!(command, windfall_project::Command::SetPluginParam { target: before, id: before_id, .. } if *before == target && *before_id == id));
                            pending.push((
                                update.revision,
                                update.token,
                                update.binding,
                                windfall_project::Command::SetPluginParam { target, id, value },
                                Some(gesture),
                            ));
                        }
                        runtime::Update::Capture { .. } => {
                            captures.push(update);
                        }
                    }
                }
                // Reconcile values before opaque state changes its fingerprint.
                pending.sort_by_key(|(_, _, _, command, _)| {
                    matches!(command, windfall_project::Command::SetPluginState { .. })
                });
                pending.retain(|(before, token, binding, command, gesture)| {
                    match session.dispatch_plugin_update(
                        &manager.runtime,
                        (*before, *token, *binding),
                        command.clone(),
                        *gesture,
                    ) {
                        Ok(_) => false,
                        Err(error) => {
                            manager
                                .status
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .error = Some(format!("Native plugin edit is waiting: {error}"));
                            true
                        }
                    }
                });
                // These accepted edits have already changed the document. Retry
                // only bookkeeping, independently of recording/capture admission.
                // Every ticket is tried once per pass, with finite backoff.
                if captures.has_pending_ack() && std::time::Instant::now() >= acknowledgement_retry
                {
                    captures.retry_ack(|ticket| manager.runtime.retry_capture_ack(ticket));
                    acknowledgement_retry =
                        std::time::Instant::now() + std::time::Duration::from_millis(250);
                }
                // Parameters are reconciled before taking opaque state. A take
                // retains these identity-bound requests for a later worker tick.
                if pending.is_empty() && std::time::Instant::now() >= capture_retry {
                    let had_pending_ack = captures.has_pending_ack();
                    let progress = captures.capture(|request| {
                        session.capture_plugin_update(&manager.runtime, request)
                    });
                    if progress.restart {
                        manager
                            .retry_requested
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Some(error) = progress.error {
                        manager
                            .status
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .error = Some(format!("Native plugin state is waiting: {error}"));
                        capture_retry =
                            std::time::Instant::now() + std::time::Duration::from_millis(250);
                    }
                    if !had_pending_ack && captures.has_pending_ack() {
                        acknowledgement_retry =
                            std::time::Instant::now() + std::time::Duration::from_millis(250);
                    }
                }
                if pending.len() > 4096 {
                    pending.drain(..pending.len() - 4096);
                    manager.status.lock().unwrap_or_else(|error| error.into_inner()).error = Some("Too many pending native plugin edits; reopen the plugin to reconcile its state".into());
                }
                manager.publish_capture_status(pending.is_empty(), &captures);
                // Preserve native edits before replacing their current owner. A
                // take can refuse refresh; one flag retains the request until idle.
                if pending.is_empty()
                    && captures.is_empty()
                    && manager
                        .retry_requested
                        .swap(false, std::sync::atomic::Ordering::Relaxed)
                    && session.refresh_plugins().is_err()
                {
                    manager
                        .retry_requested
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });
    }
    fn update_entries(&self) {
        // Registry discovery happens on Cocoa main before acquiring manager
        // locks. AUv2 has registry identities, not a loadable .component path.
        #[cfg(target_os = "macos")]
        let audio_units = self.runtime.audio_unit_catalog();
        let catalog = self
            .catalog
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut state = self
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.entries = catalog
            .plugins()
            .take(4096)
            .map(|(path, plugin)| PluginEntry {
                path: path.to_string_lossy().into_owned(),
                format: match plugin.descriptor.format {
                    PluginFormat::Clap => "clap",
                    PluginFormat::Vst3 => "vst3",
                    PluginFormat::Au => "au",
                }
                .into(),
                id: plugin.descriptor.id.clone(),
                name: plugin.descriptor.name.clone(),
                vendor: plugin.descriptor.vendor.clone(),
                instrument: plugin.descriptor.kind == PluginKind::Instrument,
                usable: plugin.is_usable(),
                error: plugin
                    .failure
                    .as_ref()
                    .map(|failure| failure.message.clone()),
            })
            .collect();
        #[cfg(target_os = "macos")]
        match audio_units {
            Ok(catalog) => state.entries.extend(catalog.entries.iter().map(|entry| {
                let descriptor = entry.descriptor();
                PluginEntry { path: entry.component.path(), format: "au".into(), id: descriptor.id,
                    name: descriptor.name, vendor: descriptor.vendor, instrument: descriptor.kind == PluginKind::Instrument,
                    usable: true, error: None }
            })),
            Err(error) => state.error = Some(format!("Audio Unit registry discovery failed: {error}")),
        }
        let mut blocked = catalog.blocked();
        // Safe loader refusals still need a visible diagnostic. The catalog's
        // crash/hang blocklist remains separate from these rejected files.
        blocked.extend(catalog.files().filter_map(|(path, scan)| {
            let failure = scan
                .failure
                .as_ref()
                .filter(|failure| !failure.is_dangerous())?;
            Some(windfall_plugin_host::scan::BlockedPlugin {
                path: path.to_path_buf(),
                plugin: None,
                failure: failure.clone(),
            })
        }));
        state.blocked = blocked
            .into_iter()
            .take(4096)
            .map(|blocked| PluginEntry {
                path: blocked.path.to_string_lossy().into_owned(),
                format: match PluginFormat::of(&blocked.path) {
                    Some(PluginFormat::Vst3) => "vst3",
                    _ => "clap",
                }
                .into(),
                id: blocked
                    .plugin
                    .as_ref()
                    .map_or(String::new(), |plugin| plugin.0.clone()),
                name: blocked
                    .plugin
                    .map_or_else(|| "Blocked file".into(), |plugin| plugin.1),
                vendor: String::new(),
                instrument: false,
                usable: false,
                error: Some(blocked.failure.message),
            })
            .collect();
    }
    pub fn add_folder(&self, folder: String) -> Result<(), String> {
        if !Path::new(&folder).is_dir() {
            return Err("Choose an existing plugin folder".into());
        }
        let folders = {
            let mut state = self
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.folders.len() >= 64 {
                return Err("At most 64 plugin folders can be configured".into());
            }
            if !state.folders.contains(&folder) {
                state.folders.push(folder);
            }
            state.folders.clone()
        };
        std::fs::write(
            &self.folders_file,
            serde_json::to_vec(&folders).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }
    pub fn scan(self: &Arc<Self>, retry: Option<String>) -> Result<(), String> {
        {
            let mut state = self
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.scanning {
                return Err("A plugin scan is already running".into());
            }
            state.scanning = true;
            state.completed = 0;
            state.total = 0;
            state.error = None;
        }
        let manager = self.clone();
        std::thread::Builder::new()
            .name("plugin-scan".into())
            .spawn(move || {
                let result = (|| {
                    let folders = manager
                        .state()
                        .folders
                        .into_iter()
                        .map(PathBuf::from)
                        .collect::<Vec<_>>();
                    let files = paths::find_plugins(&folders);
                    if files.len() > 4096 {
                        return Err(
                            "More than 4096 plugin files were found; narrow the configured folders"
                                .to_owned(),
                        );
                    }
                    manager
                        .status
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .total = files.len() as u32;
                    let mut catalog = manager
                        .catalog
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if let Some(path) = retry {
                        catalog.retry(Path::new(&path));
                    }
                    let program = std::env::current_exe().map_err(|error| error.to_string())?;
                    #[cfg(test)]
                    let program = manager
                        .fixture_scanner
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or(program);
                    catalog
                        .refresh(&files, &ProcessRunner::new(program), &mut |path| {
                            let mut state = manager
                                .status
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            state.current = Some(path.to_string_lossy().into_owned());
                            state.completed += 1;
                        })
                        .map_err(|error| error.to_string())?;
                    catalog
                        .save(&manager.cache)
                        .map_err(|error| error.to_string())
                })();
                manager.update_entries();
                if result.is_ok() {
                    manager.runtime.approve_catalog(
                        &manager
                            .catalog
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()),
                    );
                }
                manager
                    .retry_requested
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                let mut state = manager
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.scanning = false;
                state.current = None;
                state.completed = state.total;
                state.error = result.err();
            })
            .map_err(|error| {
                self.status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .scanning = false;
                error.to_string()
            })?;
        Ok(())
    }
    pub fn binding(
        &self,
        path: &str,
        id: &str,
        target: PluginTarget,
    ) -> Result<PluginBinding, String> {
        let entry = self
            .state()
            .entries
            .into_iter()
            .find(|entry| entry.path == path && entry.id == id && entry.usable)
            .ok_or_else(|| "Scan this plugin successfully before loading it".to_owned())?;
        self.runtime.discover(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            wrapper: Default::default(),
            target,
            format: entry.format,
            path: entry.path,
            id: entry.id,
            name: entry.name,
            state: Vec::new(),
            parameters: Vec::new(),
        })
    }
}

/// Scanner helper mode uses the same installed executable, in a child process.
pub fn scanner_entry() -> bool {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        return false;
    };
    if PluginFormat::of(&path).is_none() {
        return false;
    }
    let mut skip = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--skip"
            && let Some(id) = args.next()
        {
            skip.push(id.to_string_lossy().into_owned());
        }
    }
    windfall_plugin_host::scan::probe::silence_error_dialogs();
    windfall_plugin_host::scan::probe::run(&path, &skip, &mut std::io::stdout().lock());
    true
}

/// Audio/state helper must run before Tauri, using this installed executable.
pub fn helper_entry() -> bool {
    #[cfg(windows)]
    {
        windfall_plugin_host::bridge::helper::entry()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
