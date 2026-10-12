//! IPC commands for preset, score and template files, the saved plugin
//! database and the plugin manager's folder and rescan workflows. Each is a
//! thin wrapper, like those in `commands`, and is registered in its handler.
//!
//! All of them read or write files, so all of them are `async` and do their
//! work on a blocking-task thread.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use windfall_ipc::{
    PluginAnnotation, PluginDatabaseView, PluginManagerState, PresetCapture, PresetEntry,
    PresetReview, ScoreReview, ScoreWritten, TemplateInfo,
};
use windfall_project::{Channel, ChannelId, DispatchResult, DocumentSnapshot, PluginTarget, Score};

use crate::plugin_database::PluginDatabaseStore;
use crate::plugins::{PluginManager, Rescan};
use crate::session::Session;

/// Runs slow work off the async runtime's own threads.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("The task stopped unexpectedly: {error}"))?
}

/// What the plugin manager says the last scan found. Empty where native
/// hosting did not start, which leaves the database with Windfall's own
/// instruments and effects.
fn scan_state(app: &AppHandle) -> PluginManagerState {
    app.try_state::<Arc<PluginManager>>()
        .map(|manager| manager.state())
        .unwrap_or_default()
}

fn manager(app: &AppHandle) -> Result<Arc<PluginManager>, String> {
    app.try_state::<Arc<PluginManager>>()
        .map(|manager| manager.inner().clone())
        .ok_or_else(|| "Native plugin hosting is not available on this system.".to_owned())
}

#[tauri::command]
pub async fn preset_capture_channel(
    session: State<'_, Session>,
    id: ChannelId,
    generation: u64,
    revision: u64,
) -> Result<PresetCapture, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_capture_channel(id, generation, revision)).await
}

#[tauri::command]
pub async fn preset_capture_plugin(
    session: State<'_, Session>,
    target: PluginTarget,
    generation: u64,
    revision: u64,
) -> Result<PresetCapture, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_capture_plugin(target, generation, revision)).await
}

#[tauri::command]
pub async fn preset_write(
    session: State<'_, Session>,
    path: String,
    capture: PresetCapture,
) -> Result<String, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_write(&path, capture)).await
}

#[tauri::command]
pub async fn preset_read(
    session: State<'_, Session>,
    path: String,
) -> Result<PresetReview, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_read(&path)).await
}

#[tauri::command]
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri maps named IPC arguments; retain the guarded wire contract"
)]
pub async fn preset_apply_channel(
    session: State<'_, Session>,
    path: String,
    id: ChannelId,
    expected: Channel,
    name_color: bool,
    without_audio: bool,
    generation: u64,
    revision: u64,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || {
        session.preset_apply_channel_guarded(
            &path,
            id,
            expected,
            name_color,
            without_audio,
            Some((generation, revision)),
        )
    })
    .await
}

#[tauri::command]
pub async fn preset_library(session: State<'_, Session>) -> Result<Vec<PresetEntry>, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_library()).await
}

#[tauri::command]
pub async fn preset_library_save(
    session: State<'_, Session>,
    name: String,
    capture: PresetCapture,
) -> Result<PresetEntry, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_library_save(&name, capture)).await
}

#[tauri::command]
pub async fn preset_library_delete(
    session: State<'_, Session>,
    path: String,
) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.preset_library_delete(&path)).await
}

#[tauri::command]
pub async fn score_write(
    session: State<'_, Session>,
    path: String,
    score: Score,
) -> Result<ScoreWritten, String> {
    let session = session.inner().clone();
    blocking(move || session.score_write(&path, score)).await
}

#[tauri::command]
pub async fn score_read(session: State<'_, Session>, path: String) -> Result<ScoreReview, String> {
    let session = session.inner().clone();
    blocking(move || session.score_read(&path)).await
}

#[tauri::command]
pub async fn templates(session: State<'_, Session>) -> Result<Vec<TemplateInfo>, String> {
    let session = session.inner().clone();
    blocking(move || session.templates()).await
}

#[tauri::command]
pub async fn template_save(
    session: State<'_, Session>,
    name: String,
) -> Result<TemplateInfo, String> {
    let session = session.inner().clone();
    blocking(move || session.template_save(&name)).await
}

#[tauri::command]
pub async fn template_new(
    session: State<'_, Session>,
    id: String,
) -> Result<DocumentSnapshot, String> {
    let session = session.inner().clone();
    blocking(move || session.template_new(&id)).await
}

#[tauri::command]
pub async fn template_delete(session: State<'_, Session>, id: String) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.template_delete(&id)).await
}

#[tauri::command]
pub async fn plugin_database(
    app: AppHandle,
    database: State<'_, Arc<PluginDatabaseStore>>,
) -> Result<PluginDatabaseView, String> {
    let database = database.inner().clone();
    blocking(move || Ok(database.view(&scan_state(&app)))).await
}

#[tauri::command]
pub async fn plugin_database_annotate(
    app: AppHandle,
    database: State<'_, Arc<PluginDatabaseStore>>,
    format: String,
    id: String,
    annotation: PluginAnnotation,
) -> Result<PluginDatabaseView, String> {
    let database = database.inner().clone();
    blocking(move || database.annotate(&format, &id, annotation, &scan_state(&app))).await
}

#[tauri::command]
pub async fn plugin_database_forget_missing(
    app: AppHandle,
    database: State<'_, Arc<PluginDatabaseStore>>,
) -> Result<PluginDatabaseView, String> {
    let database = database.inner().clone();
    blocking(move || database.forget_missing(&scan_state(&app))).await
}

/// Takes a folder the user added out of the scan paths and scans again, so
/// its plugins leave the list.
#[tauri::command]
pub async fn plugins_remove_folder(
    app: AppHandle,
    session: State<'_, Session>,
    folder: String,
) -> Result<(), String> {
    let manager = manager(&app)?;
    let session = session.inner().clone();
    blocking(move || {
        session.while_recording_idle(|| {
            manager.remove_folder(&folder)?;
            manager.scan(None)
        })
    })
    .await
}

/// Retries every blocked plugin, or with `everything` verifies every plugin
/// again from nothing.
#[tauri::command]
pub async fn plugins_rescan(
    app: AppHandle,
    session: State<'_, Session>,
    everything: bool,
) -> Result<(), String> {
    let manager = manager(&app)?;
    let session = session.inner().clone();
    let from = if everything {
        Rescan::Everything
    } else {
        Rescan::Blocked
    };
    blocking(move || session.while_recording_idle(|| manager.rescan(from))).await
}

#[tauri::command]
pub async fn score_capture(
    session: State<'_, Session>,
    pattern: windfall_project::PatternId,
    channel: ChannelId,
    only: Option<Vec<windfall_project::NoteId>>,
    generation: u64,
    revision: u64,
) -> Result<Score, String> {
    let session = session.inner().clone();
    blocking(move || session.score_capture(pattern, channel, only, generation, revision)).await
}
#[tauri::command]
pub async fn score_apply(
    session: State<'_, Session>,
    score: Score,
    pattern: windfall_project::PatternId,
    channel: ChannelId,
    mode: windfall_project::ScoreLoadMode,
    generation: u64,
    revision: u64,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.score_apply(score, pattern, channel, mode, generation, revision)).await
}

#[tauri::command]
pub async fn preset_apply_plugin(
    session: State<'_, Session>,
    path: String,
    target: PluginTarget,
    generation: u64,
    revision: u64,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.preset_apply_plugin(&path, target, generation, revision)).await
}
#[tauri::command]
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri maps named IPC arguments; retain the guarded wire contract"
)]
pub async fn plugins_insert(
    app: AppHandle,
    session: State<'_, Session>,
    path: String,
    id: String,
    destination: Option<PluginTarget>,
    track: Option<windfall_project::TrackId>,
    generation: u64,
    revision: u64,
) -> Result<DispatchResult, String> {
    let manager = manager(&app)?;
    let session = session.inner().clone();
    blocking(move || {
        use windfall_project::{
            ChannelSource, Command, EffectId, InstrumentKind, PluginPreset, PluginPresetState,
        };
        let target = destination.unwrap_or_else(|| {
            if track.is_some() {
                PluginTarget::Effect {
                    effect: EffectId(0),
                }
            } else {
                PluginTarget::Instrument {
                    channel: ChannelId(0),
                }
            }
        });
        let plugin = session.while_recording_idle(|| manager.binding(&path, &id, target))?;
        let project = session.document_snapshot().project;
        let command = match destination {
            Some(PluginTarget::Instrument { channel }) => {
                let expected = project
                    .channel(channel)
                    .cloned()
                    .ok_or("This channel no longer exists")?;
                let (mut preset, _) = windfall_project::ChannelPreset::capture(&project, channel)?;
                preset.source = ChannelSource::Instrument {
                    params: InstrumentKind::SubtractiveSynth.default_params(),
                    asset: None,
                };
                preset.sample_asset = None;
                let mut plugin = plugin;
                plugin.target = PluginTarget::Instrument {
                    channel: ChannelId(0),
                };
                preset.plugin = Some(plugin);
                Command::ApplyChannelPreset {
                    id: channel,
                    expected,
                    preset,
                    sample: None,
                    audio: None,
                    name_color: false,
                }
            }
            Some(target @ PluginTarget::Effect { .. }) => {
                let expected = PluginPreset::capture(&project, target)?.state;
                let mut binding = plugin;
                binding.target = PluginTarget::Effect {
                    effect: EffectId(0),
                };
                Command::ApplyPluginPreset {
                    target,
                    expected,
                    preset: PluginPreset {
                        state: PluginPresetState::Hosted {
                            instrument: false,
                            binding,
                        },
                    },
                }
            }
            None => match track {
                Some(track) => Command::AddPluginEffect { track, plugin },
                None => Command::AddPluginInstrument { plugin },
            },
        };
        let result = session.dispatch_file_command(command, generation, revision)?;
        Ok(result)
    })
    .await
}
