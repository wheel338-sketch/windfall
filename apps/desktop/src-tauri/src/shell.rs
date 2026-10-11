//! Starting the session inside the Tauri app: where its files are, the
//! audio engine, and the sink that turns its events into window events.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager};
use windfall_engine::Engine;

use crate::events::{Event, EventSink};
use crate::session::{AUTOSAVE_INTERVAL, Session, SessionConfig, openable};
use crate::settings::{SETTINGS_FILE, SettingsStore};

/// Set to `1` to run the whole audio path with the output turned all the
/// way down, so automated runs make no sound. Meters still move.
pub const SILENT_ENV: &str = "WINDFALL_SILENT";

/// The factory content in the source tree, for running without a bundle.
pub(crate) const DEV_FACTORY_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../content/factory");

/// Name of the factory content folder inside the bundle's resources.
const FACTORY_RESOURCE: &str = "factory";

/// Emits the session's events to every window.
struct WindowEvents(AppHandle);

impl EventSink for WindowEvents {
    fn emit(&self, event: Event) {
        let name = event.name();
        let sent = event
            .payload()
            .map_err(tauri::Error::from)
            .and_then(|payload| self.0.emit_str(name, payload));
        if let Err(error) = sent {
            log::warn!("could not send the event {name}: {error}");
        }
    }
}

/// Opens the audio engine with the remembered settings, starts a session on
/// the default project, and starts the threads that serve it.
pub fn start(app: &AppHandle) -> Result<Session, Box<dyn Error>> {
    let config_dir = app.path().app_config_dir()?;
    let settings = SettingsStore::load(config_dir.join(SETTINGS_FILE));

    // Returns once the device is open or has refused. That is quick for a
    // driver that works, and the window does not respond until then. The
    // devices are listed first only if the remembered request names a
    // sample rate or a buffer size to check against them.
    let opening = Instant::now();
    let engine = Arc::new(Engine::start(&openable(
        &settings.settings().audio,
        Engine::devices,
    )));
    let opened_in = opening.elapsed();
    let controller = engine.controller();
    if std::env::var(SILENT_ENV).is_ok_and(|value| value == "1") {
        controller.set_output_gain(0.0);
        log::info!("{SILENT_ENV} is set: the audio output is turned all the way down");
    }
    let status = engine.status();
    match &status.error {
        None => log::info!(
            "audio: {} / {} at {} Hz, {} frames, opened in {} ms",
            status.host,
            status.device.as_deref().unwrap_or("no device"),
            status.sample_rate,
            status.buffer_frames,
            opened_in.as_millis()
        ),
        Some(error) => log::warn!("audio is not running: {error}"),
    }

    let session = Session::new(SessionConfig {
        controller,
        audio: engine,
        events: Arc::new(WindowEvents(app.clone())),
        factory_dir: factory_dir(app),
        settings,
    });
    log::info!("factory content: {}", session.factory_dir().display());
    if let Err(error) = session.start_midi_hardware() {
        log::warn!("MIDI: {error}");
    }
    match crate::plugins::PluginManager::new(&config_dir) {
        Ok(manager) => {
            session.install_plugins(manager.clone());
            manager.attach(session.downgrade());
            let _ = manager.scan(None);
            app.manage(manager);
        }
        Err(error) => log::warn!("plugins: {error}"),
    }
    session.spawn_realtime()?;
    session.spawn_controllers()?;
    session.spawn_autosave(AUTOSAVE_INTERVAL)?;
    Ok(session)
}

/// Where the factory content is: the source tree while developing, and the
/// bundle's resources in an installed app. Each falls back to the other, so
/// a development build that was moved still finds a bundled copy.
fn factory_dir(app: &AppHandle) -> PathBuf {
    let dev = PathBuf::from(DEV_FACTORY_DIR);
    let bundled = app
        .path()
        .resolve(FACTORY_RESOURCE, BaseDirectory::Resource)
        .ok()
        .filter(|folder| folder.is_dir());
    match bundled {
        Some(bundled) if !tauri::is_dev() || !dev.is_dir() => bundled,
        _ => dev,
    }
}
