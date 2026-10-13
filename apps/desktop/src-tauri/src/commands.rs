//! The IPC commands of docs/ARCHITECTURE.md, each a thin wrapper around one
//! [`Session`] method.
//!
//! Commands whose order matters and that are quick, such as edits and the
//! transport, are plain functions: Tauri runs those on the main thread, one
//! at a time, in the order the UI sent them, so two moves of one fader
//! cannot overtake each other. Commands that read files, decode audio or
//! wait for a device are `async` and do their work on a blocking-task
//! thread.

use tauri::ipc::{Channel, Invoke};
use tauri::{
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use windfall_ipc::{
    AudioHost, AudioSettings, BrowserEntry, BrowserRoot, EngineStatus, ExportOptions,
    FlpImportOptions, FlpImportPreview, RealtimeFrame, SampleInfo, TransportPatch, TransportState,
};
use windfall_project::{
    AutomationTarget, ChannelId, Command, DispatchResult, DocumentSnapshot, PlaylistTrackId,
    ProjectPatch, SampleId, TrackId,
};

use crate::session::{ClipPlace, Session};
use windfall_ipc::{LibraryFileToken, LibraryMetadata, LibraryResults, LibrarySearch};
use windfall_ipc::{MidiExportOptions, MidiImportOptions, MidiImportPreview};

// Native analysis controls. Slow preparation, cleanup and joining stay on
// blocking tasks; cancellation itself never waits for the preparation gate.
#[tauri::command]
fn music_lighting_status(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<serde_json::Value, String> {
    let _window_call = caller.enter(&window, "music_lighting_status")?;
    Ok(serde_json::to_value(session.music_lighting_status())
        .expect("plain lighting status serializes"))
}
#[tauri::command]
async fn sample_instrument_load_bank(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: Option<windfall_project::SampleInstrumentAsset>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_instrument_load_bank")?;
    let session = session.inner().clone();
    blocking(move || session.sample_instrument_load_bank(channel, expected, path)).await
}
#[tauri::command]
fn sample_instrument_missing_sources(
    session: State<'_, Session>,
    channel: ChannelId,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<SampleId>, String> {
    let _window_call = caller.enter(&window, "sample_instrument_missing_sources")?;

    session.sample_instrument_missing_sources(channel)
}
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Named Tauri wire arguments plus authenticated window context"
)]
async fn sample_instrument_relink_source(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: Option<windfall_project::SampleInstrumentAsset>,
    sample: SampleId,
    expected_path: windfall_project::SamplePath,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_instrument_relink_source")?;

    let session = session.inner().clone();
    blocking(move || {
        session.sample_instrument_relink_source(channel, expected, sample, expected_path, path)
    })
    .await
}
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Named Tauri wire arguments plus authenticated window context"
)]
async fn sample_instrument_general_midi(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: windfall_project::ChannelSource,
    path: String,
    bank: u16,
    program: u16,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_instrument_general_midi")?;

    let session = session.inner().clone();
    blocking(move || session.sample_instrument_general_midi(channel, expected, path, bank, program))
        .await
}
#[tauri::command]
async fn sample_instrument_save_bank(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: Option<windfall_project::SampleInstrumentAsset>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "sample_instrument_save_bank")?;
    let session = session.inner().clone();
    blocking(move || session.sample_instrument_save_bank(channel, expected, path)).await
}
#[tauri::command]
async fn sample_instrument_import_zone(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: Option<windfall_project::SampleInstrumentAsset>,
    path: String,
    pad: Option<u8>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_instrument_import_zone")?;
    let session = session.inner().clone();
    blocking(move || session.sample_instrument_import_zone(channel, expected, path, pad)).await
}
#[tauri::command]
async fn soundfont_presets(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<windfall_dsp::sample_bank::soundfont::SoundFontPreset>, String> {
    let _window_call = caller.enter(&window, "soundfont_presets")?;
    let session = session.inner().clone();
    blocking(move || session.soundfont_presets(&path)).await
}
// Tauri wire arguments include native caller identity; domain methods stay unchanged.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn sample_instrument_speech_begin(
    session: State<'_, Session>,
    channel: ChannelId,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u64, String> {
    let _window_call = caller.enter(&window, "sample_instrument_speech_begin")?;

    session.sample_instrument_speech_begin(channel)
}
#[tauri::command]
fn sample_instrument_speech_cancel(
    session: State<'_, Session>,
    channel: ChannelId,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "sample_instrument_speech_cancel")?;
    session.sample_instrument_speech_cancel(channel, request);
    Ok(())
}
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Named Tauri wire arguments plus authenticated window context"
)]
async fn sample_instrument_speech(
    session: State<'_, Session>,
    channel: ChannelId,
    expected: Option<windfall_project::SampleInstrumentAsset>,
    text: String,
    pitch: f32,
    duration: f32,
    request: Option<u64>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_instrument_speech")?;
    let session = session.inner().clone();
    blocking(move || match request {
        Some(request) => session
            .sample_instrument_speech_request(channel, expected, text, pitch, duration, request),
        None => session.sample_instrument_speech(channel, expected, text, pitch, duration),
    })
    .await
}

#[tauri::command]
fn phone_remote_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::RemoteSettingsState, String> {
    let _window_call = caller.enter(&window, "phone_remote_state")?;
    Ok(session.phone_remote_state())
}
#[tauri::command]
async fn phone_remote_set(
    session: State<'_, Session>,
    address: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::RemoteSettingsState, String> {
    let _window_call = caller.enter(&window, "phone_remote_set")?;
    let session = session.inner().clone();
    blocking(move || session.phone_remote_set(address.as_deref())).await
}
#[tauri::command]
async fn backup_versions(
    session: State<'_, Session>,
    folder: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<windfall_ipc::BackupVersion>, String> {
    let _window_call = caller.enter(&window, "backup_versions")?;
    let session = session.inner().clone();
    blocking(move || session.backup_versions(&folder)).await
}
#[tauri::command]
async fn backup_save(
    session: State<'_, Session>,
    folder: String,
    expected: Option<String>,
    keep: usize,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::BackupVersion, String> {
    let _window_call = caller.enter(&window, "backup_save")?;
    let job = session.backup_job()?;
    let session = session.inner().clone();
    blocking(move || session.backup_save_with(&folder, expected.as_deref(), keep, generation, job))
        .await
}
#[tauri::command]
async fn backup_restore(
    session: State<'_, Session>,
    folder: String,
    id: String,
    target: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "backup_restore")?;
    let job = session.backup_job()?;
    let session = session.inner().clone();
    blocking(move || session.backup_restore_with(&folder, &id, &target, job)).await
}
#[tauri::command]
fn backup_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "backup_cancel")?;
    session.backup_cancel();
    Ok(())
}
#[tauri::command]
fn loop_catalog(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::LoopCatalog, String> {
    let _window_call = caller.enter(&window, "loop_catalog")?;
    Ok(session.loop_catalog())
}
#[tauri::command]
async fn loop_preview(
    session: State<'_, Session>,
    id: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "loop_preview")?;
    let session = session.inner().clone();
    blocking(move || session.loop_preview(&id)).await
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn loop_insert(
    session: State<'_, Session>,
    id: String,
    key: Option<u8>,
    track: Option<PlaylistTrackId>,
    start: u32,
    generation: u64,
    revision: u64,
    row: Option<usize>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "loop_insert")?;
    let session = session.inner().clone();
    blocking(move || {
        session.loop_insert_with_rows(
            &id,
            key,
            ClipPlace {
                track,
                start,
                mixer_track: None,
            },
            generation,
            revision,
            row,
        )
    })
    .await
}
#[tauri::command]
fn analysis_capability(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisCapability, String> {
    let _window_call = caller.enter(&window, "analysis_capability")?;
    Ok(session.analysis_capability())
}
#[tauri::command]
async fn analysis_model_import(
    session: State<'_, Session>,
    path: String,
    model: windfall_ipc::AnalysisModel,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisModel, String> {
    let _window_call = caller.enter(&window, "analysis_model_import")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_model_import(&path, model)).await
}
#[tauri::command]
async fn analysis_runtime_settings(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisRuntimeSettings, String> {
    let _window_call = caller.enter(&window, "analysis_runtime_settings")?;

    let session = session.inner().clone();
    blocking(move || session.analysis_runtime_settings()).await
}
#[tauri::command]
async fn analysis_runtime_configure(
    session: State<'_, Session>,
    settings: windfall_ipc::AnalysisRuntimeSettings,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisRuntimeSettings, String> {
    let _window_call = caller.enter(&window, "analysis_runtime_configure")?;

    let session = session.inner().clone();
    blocking(move || session.analysis_runtime_configure(settings)).await
}
#[tauri::command]
async fn analysis_submit(
    session: State<'_, Session>,
    request: windfall_ipc::AnalysisSubmit,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisJob, String> {
    let _window_call = caller.enter(&window, "analysis_submit")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_submit(request)).await
}
#[tauri::command]
fn analysis_status(
    session: State<'_, Session>,
    job: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisJob, String> {
    let _window_call = caller.enter(&window, "analysis_status")?;
    session.analysis_status(&job)
}
#[tauri::command]
async fn analysis_cancel(
    session: State<'_, Session>,
    job: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisJob, String> {
    let _window_call = caller.enter(&window, "analysis_cancel")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_cancel(&job)).await
}
#[tauri::command]
fn analysis_cancel_preparation(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analysis_cancel_preparation")?;
    session.analysis_cancel_preparation();
    Ok(())
}
#[tauri::command]
async fn analysis_forget(
    session: State<'_, Session>,
    job: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analysis_forget")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_forget(&job)).await
}
#[tauri::command]
async fn analysis_retry_cleanup(
    session: State<'_, Session>,
    job: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analysis_retry_cleanup")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_retry_cleanup(&job)).await
}
#[tauri::command]
async fn analysis_review(
    session: State<'_, Session>,
    ticket: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalysisReview, String> {
    let _window_call = caller.enter(&window, "analysis_review")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_review(&ticket)).await
}
#[tauri::command]
async fn analysis_preview(
    session: State<'_, Session>,
    ticket: String,
    role: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analysis_preview")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_preview(&ticket, &role)).await
}
#[tauri::command]
async fn analysis_apply(
    session: State<'_, Session>,
    request: windfall_ipc::AnalysisApply,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "analysis_apply")?;
    let session = session.inner().clone();
    blocking(move || session.analysis_apply(request)).await
}
#[tauri::command]
async fn analysis_shutdown(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analysis_shutdown")?;
    let session = session.inner().clone();
    blocking(move || {
        session.analysis_shutdown();
        Ok(())
    })
    .await
}

#[tauri::command]
fn mixer_waveform_tracks(
    session: State<'_, Session>,
    tracks: Vec<TrackId>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "mixer_waveform_tracks")?;
    session.mixer_waveform_tracks(tracks, generation, revision)
}

#[tauri::command]
async fn mixer_preset_capture(
    session: State<'_, Session>,
    id: TrackId,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let _window_call = caller.enter(&window, "mixer_preset_capture")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_capture(id, generation, revision)).await
}
#[tauri::command]
async fn mixer_preset_write(
    session: State<'_, Session>,
    path: String,
    preset: windfall_project::MixerTrackPreset,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "mixer_preset_write")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_write(path, preset)).await
}
#[tauri::command]
async fn mixer_preset_read(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let _window_call = caller.enter(&window, "mixer_preset_read")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_read(path)).await
}
#[tauri::command]
async fn mixer_preset_library(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<crate::session::MixerPresetLibrary, String> {
    let _window_call = caller.enter(&window, "mixer_preset_library")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library()).await
}
#[tauri::command]
async fn mixer_preset_library_save(
    session: State<'_, Session>,
    preset: windfall_project::MixerTrackPreset,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<crate::session::MixerPresetLibraryEntry, String> {
    let _window_call = caller.enter(&window, "mixer_preset_library_save")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_save(preset)).await
}
#[tauri::command]
async fn mixer_preset_library_read(
    session: State<'_, Session>,
    file: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let _window_call = caller.enter(&window, "mixer_preset_library_read")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_read(file)).await
}
#[tauri::command]
async fn mixer_preset_library_delete(
    session: State<'_, Session>,
    file: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "mixer_preset_library_delete")?;
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_delete(file)).await
}

#[tauri::command]
async fn sample_playback_detect(
    session: State<'_, Session>,
    channel: windfall_project::ChannelId,
    options: windfall_ipc::SliceOptions,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sample_playback_detect")?;
    let session = session.inner().clone();
    blocking(move || session.sample_playback_detect(channel, options, generation, revision)).await
}

#[tauri::command]
async fn slice_analyze(
    session: State<'_, Session>,
    clip: windfall_project::ClipId,
    options: windfall_ipc::SliceOptions,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::SliceReview, String> {
    let _window_call = caller.enter(&window, "slice_analyze")?;
    let session = session.inner().clone();
    blocking(move || session.slice_analyze(clip, options)).await
}
#[tauri::command]
async fn slice_apply(
    session: State<'_, Session>,
    token: u32,
    markers: Vec<u32>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "slice_apply")?;
    let session = session.inner().clone();
    blocking(move || session.slice_apply(token, markers)).await
}
#[tauri::command]
fn slice_discard(
    session: State<'_, Session>,
    token: u32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "slice_discard")?;
    session.slice_discard(token);
    Ok(())
}

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

#[tauri::command]
fn document_snapshot(session: State<'_, Session>) -> DocumentSnapshot {
    session.document_snapshot()
}

#[tauri::command]
fn window_document_snapshot(
    session: State<'_, Session>,
    window: WebviewWindow,
) -> Result<crate::window_calls::WindowBootstrap, String> {
    let snapshot = session.window_document_snapshot();
    let caller = window
        .state::<crate::window_calls::WindowCalls>()
        .caller(window.label(), snapshot.generation)?;
    Ok(crate::window_calls::WindowBootstrap {
        generation: snapshot.generation,
        caller,
        snapshot: snapshot.snapshot,
    })
}

#[tauri::command]
fn current_mixer_target(
    session: State<'_, Session>,
    track: Option<TrackId>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "current_mixer_target")?;
    session.current_mixer_target(track, generation, revision)
}

#[tauri::command]
async fn dispatch(
    session: State<'_, Session>,
    command: Command,
    gesture: Option<u64>,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "dispatch")?;
    let session = session.inner().clone();
    blocking(move || session.dispatch_from_window(generation, command, gesture)).await
}

#[tauri::command]
async fn prepare_clip_command(
    session: State<'_, Session>,
    command: Command,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "prepare_clip_command")?;
    let session = session.inner().clone();
    blocking(move || session.prepare_clip_command(command)).await
}
#[tauri::command]
fn sampler_preparation_begin(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u64, String> {
    let _window_call = caller.enter(&window, "sampler_preparation_begin")?;
    session.sampler_preparation_begin()
}
#[tauri::command]
fn sampler_preparation_cancel(
    session: State<'_, Session>,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "sampler_preparation_cancel")?;
    session.sampler_preparation_cancel(request);
    Ok(())
}
#[tauri::command]
fn sampler_preparation_progress(
    session: State<'_, Session>,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::SamplerPreparationProgress, String> {
    let _window_call = caller.enter(&window, "sampler_preparation_progress")?;
    Ok(session.sampler_preparation_progress(request))
}
#[tauri::command]
async fn prepare_sampler_command(
    session: State<'_, Session>,
    command: Command,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "prepare_sampler_command")?;
    let session = session.inner().clone();
    blocking(move || session.prepare_sampler_command(command, request)).await
}
#[tauri::command]
async fn detect_clip_tempo(
    session: State<'_, Session>,
    sample: SampleId,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<windfall_ipc::ClipTempoCandidate>, String> {
    let _window_call = caller.enter(&window, "detect_clip_tempo")?;
    let session = session.inner().clone();
    blocking(move || session.detect_clip_tempo(sample)).await
}
#[tauri::command]
async fn audio_editor_open(
    session: State<'_, Session>,
    clip: windfall_project::ClipId,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AudioEditPreview, String> {
    let _window_call = caller.enter(&window, "audio_editor_open")?;
    let session = session.inner().clone();
    blocking(move || session.audio_editor_open(clip)).await
}
#[tauri::command]
async fn audio_editor_apply(
    session: State<'_, Session>,
    request: windfall_ipc::AudioEditRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_editor_apply")?;
    let session = session.inner().clone();
    blocking(move || session.audio_editor_apply(request)).await
}
#[tauri::command]
async fn audio_script_preview(
    session: State<'_, Session>,
    input: windfall_ipc::AudioScriptRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AudioScriptPreview, String> {
    let _window_call = caller.enter(&window, "audio_script_preview")?;

    let session = session.inner().clone();
    blocking(move || session.audio_script_preview(input)).await
}
#[tauri::command]
async fn audio_script_apply(
    session: State<'_, Session>,
    token: u32,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_script_apply")?;

    let session = session.inner().clone();
    blocking(move || session.audio_script_apply(token, request)).await
}
#[tauri::command]
async fn audio_script_audition(
    session: State<'_, Session>,
    token: u32,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audio_script_audition")?;

    let session = session.inner().clone();
    blocking(move || session.audio_script_audition(token, request)).await
}
#[tauri::command]
async fn audio_editor_discard(
    session: State<'_, Session>,
    token: u32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audio_editor_discard")?;
    let session = session.inner().clone();
    blocking(move || {
        session.audio_editor_discard(token);
        Ok(())
    })
    .await
}

#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Named Tauri wire arguments plus authenticated window context"
)]
async fn audio_editor_save_wavpack(
    session: State<'_, Session>,
    token: u32,
    start: u32,
    end: u32,
    path: String,
    depth: windfall_ipc::BitDepth,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audio_editor_save_wavpack")?;

    let session = session.inner().clone();
    blocking(move || session.audio_editor_save_wavpack(token, start, end, path, depth)).await
}

#[tauri::command]
async fn m4a_capability(
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Option<String>, String> {
    let _window_call = caller.enter(&window, "m4a_capability")?;

    blocking(|| {
        Ok(windfall_codec::m4a_capability()
            .err()
            .map(|e| e.to_string()))
    })
    .await
}

#[tauri::command]
async fn audio_editor_regions(
    session: State<'_, Session>,
    token: u32,
    regions: Vec<windfall_project::AudioRegion>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_editor_regions")?;
    let session = session.inner().clone();
    blocking(move || session.audio_editor_regions(token, regions)).await
}

#[tauri::command]
async fn audio_editor_transfer(
    session: State<'_, Session>,
    request: windfall_ipc::AudioEditTransferRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_editor_transfer")?;
    let session = session.inner().clone();
    blocking(move || session.audio_editor_transfer(request)).await
}
#[tauri::command]
async fn audio_analysis_detect(
    session: State<'_, Session>,
    token: u32,
    polyphonic: bool,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AudioAnalysisPreview, String> {
    let _window_call = caller.enter(&window, "audio_analysis_detect")?;
    let session = session.inner().clone();
    blocking(move || session.audio_analysis_detect(token, polyphonic)).await
}
#[tauri::command]
async fn audio_analysis_audition(
    session: State<'_, Session>,
    edit: windfall_ipc::AudioAnalysisEdit,
    dry: bool,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audio_analysis_audition")?;
    let session = session.inner().clone();
    blocking(move || session.audio_analysis_audition(edit, dry)).await
}
#[tauri::command]
async fn audio_analysis_apply(
    session: State<'_, Session>,
    edit: windfall_ipc::AudioAnalysisEdit,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_analysis_apply")?;
    let session = session.inner().clone();
    blocking(move || session.audio_analysis_apply(edit)).await
}
#[tauri::command]
async fn audio_analysis_send_midi(
    session: State<'_, Session>,
    edit: windfall_ipc::AudioAnalysisEdit,
    selected: Vec<u32>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_analysis_send_midi")?;
    let session = session.inner().clone();
    blocking(move || session.audio_analysis_send_midi(edit, selected)).await
}
#[tauri::command]
fn audio_analysis_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audio_analysis_cancel")?;
    session.audio_analysis_cancel();
    Ok(())
}
#[tauri::command]
fn automate(
    session: State<'_, Session>,
    target: AutomationTarget,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "automate")?;
    session.automate(target)
}

#[tauri::command]
async fn undo(
    session: State<'_, Session>,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Option<ProjectPatch>, String> {
    let _window_call = caller.enter(&window, "undo")?;
    let session = session.inner().clone();
    blocking(move || session.undo_from_window(generation)).await
}

#[tauri::command]
async fn redo(
    session: State<'_, Session>,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Option<ProjectPatch>, String> {
    let _window_call = caller.enter(&window, "redo")?;
    let session = session.inner().clone();
    blocking(move || session.redo_from_window(generation)).await
}

#[tauri::command]
async fn history_jump(
    session: State<'_, Session>,
    cursor: u32,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<ProjectPatch, String> {
    let _window_call = caller.enter(&window, "history_jump")?;
    let session = session.inner().clone();
    blocking(move || session.history_jump_from_window(generation, cursor)).await
}

#[tauri::command]
async fn project_new(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DocumentSnapshot, String> {
    let _window_call = caller.enter(&window, "project_new")?;
    let session = session.inner().clone();
    blocking(move || session.project_new()).await
}

#[tauri::command]
async fn project_open(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DocumentSnapshot, String> {
    let _window_call = caller.enter(&window, "project_open")?;
    let session = session.inner().clone();
    if std::path::Path::new(&path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let job = session.archive_job()?;
        return blocking(move || session.project_archive_open_with(&path, job)).await;
    }
    blocking(move || session.project_open(&path)).await
}

#[tauri::command]
async fn flp_preview(
    session: State<'_, Session>,
    path: String,
    options: FlpImportOptions,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<FlpImportPreview, String> {
    let _window_call = caller.enter(&window, "flp_preview")?;
    let session = session.inner().clone();
    blocking(move || session.flp_preview(&path, &options)).await
}

#[tauri::command]
fn flp_open(
    session: State<'_, Session>,
    token: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DocumentSnapshot, String> {
    let _window_call = caller.enter(&window, "flp_open")?;
    session.flp_open(token)
}

#[tauri::command]
fn flp_cancel(
    session: State<'_, Session>,
    token: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "flp_cancel")?;
    session.flp_cancel(token);
    Ok(())
}
#[tauri::command]
async fn project_save(
    session: State<'_, Session>,
    path: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "project_save")?;
    let session = session.inner().clone();
    blocking(move || session.project_save(path.as_deref())).await
}

#[tauri::command]
async fn project_save_new_version(
    session: State<'_, Session>,
    path: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "project_save_new_version")?;
    let session = session.inner().clone();
    blocking(move || session.project_save_new_version(path.as_deref())).await
}

#[tauri::command]
async fn project_archive_save(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "project_archive_save")?;
    let session = session.inner().clone();
    let job = session.archive_job()?;
    blocking(move || session.project_archive_save_with(&path, job)).await
}

#[tauri::command]
fn project_archive_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "project_archive_cancel")?;
    session.project_archive_cancel();
    Ok(())
}

#[tauri::command]
async fn recent_projects(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<String>, String> {
    let _window_call = caller.enter(&window, "recent_projects")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.recent_projects())).await
}

#[tauri::command]
fn transport_play(
    session: State<'_, Session>,
    guard: Option<windfall_ipc::TimelinePlaybackState>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<TransportState, String> {
    let _window_call = caller.enter(&window, "transport_play")?;
    match guard {
        Some(guard) => session.timeline_transport_play(guard),
        None => session.transport_play(),
    }
}

#[tauri::command]
fn transport_stop(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<TransportState, String> {
    let _window_call = caller.enter(&window, "transport_stop")?;
    Ok(session.transport_stop())
}

#[tauri::command]
fn transport_toggle(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<TransportState, String> {
    let _window_call = caller.enter(&window, "transport_toggle")?;
    session.transport_toggle()
}

#[tauri::command]
fn transport_seek(
    session: State<'_, Session>,
    tick: f64,
    guard: Option<windfall_ipc::TimelinePlaybackState>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "transport_seek")?;
    match guard {
        Some(guard) => session.timeline_transport_seek(tick, guard),
        None => {
            session.transport_seek(tick);
            Ok(())
        }
    }
}

#[tauri::command]
fn transport_set(
    session: State<'_, Session>,
    patch: TransportPatch,
    guard: Option<windfall_ipc::TimelinePlaybackState>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<TransportState, String> {
    let _window_call = caller.enter(&window, "transport_set")?;
    match guard {
        Some(guard) => session.timeline_transport_set(patch, guard),
        None => session.transport_set(patch),
    }
}

#[tauri::command]
fn transport_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<TransportState, String> {
    let _window_call = caller.enter(&window, "transport_state")?;
    Ok(session.transport_state())
}

#[tauri::command]
fn timeline_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::TimelinePlaybackState, String> {
    let _window_call = caller.enter(&window, "timeline_state")?;
    Ok(session.timeline_state())
}

// Tauri wire arguments include native caller identity; domain methods stay unchanged.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn timeline_region(
    session: State<'_, Session>,
    region: Option<windfall_project::TickRange>,
    generation: u64,
    revision: u64,
    request: Option<u64>,
    cancel: Option<windfall_ipc::TimelinePlaybackState>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::TimelinePlaybackState, String> {
    let _window_call = caller.enter(&window, "timeline_region")?;
    session.timeline_region_request(region, generation, revision, request, cancel)
}

#[tauri::command]
fn realtime_subscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    channel: Channel<RealtimeFrame>,
    token: String,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "realtime_subscribe")?;
    session.subscribe_realtime_owned(
        window.label(),
        Some(token),
        Box::new(move |frame| channel.send(frame.clone()).is_ok()),
    );
    Ok(())
}

#[tauri::command]
fn realtime_unsubscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    token: String,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "realtime_unsubscribe")?;
    session.unsubscribe_realtime_owned(window.label(), &token);
    Ok(())
}

#[tauri::command]
fn engine_status(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<EngineStatus, String> {
    let _window_call = caller.enter(&window, "engine_status")?;
    Ok(session.engine_status())
}

/// OS accounting stays off the async runtime and outside document/audio locks.
#[tauri::command]
async fn process_memory(
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Option<u64>, String> {
    let _window_call = caller.enter(&window, "process_memory")?;
    blocking(|| Ok(crate::process_memory::resident_bytes())).await
}

#[tauri::command]
async fn engine_devices(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<AudioHost>, String> {
    let _window_call = caller.enter(&window, "engine_devices")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_devices())).await
}

#[tauri::command]
async fn engine_settings(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<AudioSettings, String> {
    let _window_call = caller.enter(&window, "engine_settings")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_settings())).await
}

#[tauri::command]
async fn engine_configure(
    session: State<'_, Session>,
    settings: AudioSettings,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<EngineStatus, String> {
    let _window_call = caller.enter(&window, "engine_configure")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_configure(settings))).await
}

#[tauri::command]
fn audition_note_on(
    session: State<'_, Session>,
    channel: ChannelId,
    key: u8,
    velocity: f32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audition_note_on")?;
    session.audition_note_on(channel, key, velocity);
    Ok(())
}

#[tauri::command]
fn audition_note_off(
    session: State<'_, Session>,
    channel: ChannelId,
    key: u8,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "audition_note_off")?;
    session.audition_note_off(channel, key);
    Ok(())
}

#[tauri::command]
async fn preview_play(
    session: State<'_, Session>,
    path: String,
    browser: Option<LibraryFileToken>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "preview_play")?;
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_preview(&path, &token),
        None => session.preview_play(&path),
    })
    .await
}

#[tauri::command]
fn preview_stop(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "preview_stop")?;
    session.preview_stop();
    Ok(())
}

#[tauri::command]
async fn browser_roots(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<BrowserRoot>, String> {
    let _window_call = caller.enter(&window, "browser_roots")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.browser_roots())).await
}

#[tauri::command]
async fn library_search(
    session: State<'_, Session>,
    search: LibrarySearch,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<LibraryResults, String> {
    let _window_call = caller.enter(&window, "library_search")?;
    let session = session.inner().clone();
    blocking(move || session.library_search(&search)).await
}

#[tauri::command]
fn library_refresh(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "library_refresh")?;
    session.library_refresh();
    Ok(())
}

#[tauri::command]
fn library_cancel(
    session: State<'_, Session>,
    generation: u32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "library_cancel")?;
    session.library_cancel(generation);
    Ok(())
}

#[tauri::command]
async fn library_file(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<LibraryFileToken, String> {
    let _window_call = caller.enter(&window, "library_file")?;
    let session = session.inner().clone();
    blocking(move || session.library_file(&path)).await
}

#[tauri::command]
async fn library_metadata(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<LibraryMetadata, String> {
    let _window_call = caller.enter(&window, "library_metadata")?;
    let session = session.inner().clone();
    blocking(move || session.library_metadata(&path)).await
}

#[tauri::command]
async fn library_set_metadata(
    session: State<'_, Session>,
    path: String,
    metadata: LibraryMetadata,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<LibraryMetadata, String> {
    let _window_call = caller.enter(&window, "library_set_metadata")?;
    let session = session.inner().clone();
    blocking(move || session.library_set_metadata(&path, metadata)).await
}

#[tauri::command]
async fn browser_add_root(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<BrowserRoot>, String> {
    let _window_call = caller.enter(&window, "browser_add_root")?;
    let session = session.inner().clone();
    blocking(move || session.browser_add_root(&path)).await
}

#[tauri::command]
async fn browser_remove_root(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<BrowserRoot>, String> {
    let _window_call = caller.enter(&window, "browser_remove_root")?;
    let session = session.inner().clone();
    blocking(move || session.browser_remove_root(&path)).await
}

#[tauri::command]
async fn browser_list(
    session: State<'_, Session>,
    path: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<BrowserEntry>, String> {
    let _window_call = caller.enter(&window, "browser_list")?;
    let session = session.inner().clone();
    blocking(move || session.browser_list(&path)).await
}

#[tauri::command]
async fn sample_info(
    session: State<'_, Session>,
    path: String,
    browser: Option<LibraryFileToken>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<SampleInfo, String> {
    let _window_call = caller.enter(&window, "sample_info")?;
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_sample_info(&path, &token),
        None => session.sample_info(&path),
    })
    .await
}

#[tauri::command]
async fn sample_info_by_id(
    session: State<'_, Session>,
    sample: SampleId,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<SampleInfo, String> {
    let _window_call = caller.enter(&window, "sample_info_by_id")?;
    let session = session.inner().clone();
    blocking(move || session.sample_info_by_id(sample)).await
}

#[tauri::command]
async fn samples_reload(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u32, String> {
    let _window_call = caller.enter(&window, "samples_reload")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.samples_reload())).await
}

#[tauri::command]
async fn add_channel_from_file(
    session: State<'_, Session>,
    path: String,
    index: Option<u32>,
    browser: Option<LibraryFileToken>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "add_channel_from_file")?;
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_add_channel(&path, index, token),
        None => session.add_channel_from_file(&path, index),
    })
    .await
}

#[tauri::command]
async fn set_channel_sample_from_file(
    session: State<'_, Session>,
    channel: ChannelId,
    path: String,
    browser: Option<LibraryFileToken>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "set_channel_sample_from_file")?;
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_replace_sample(channel, &path, token),
        None => session.set_channel_sample_from_file(channel, &path),
    })
    .await
}

// Tauri wire arguments include native caller identity; domain methods stay unchanged.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn add_audio_clip_from_file(
    session: State<'_, Session>,
    path: String,
    track: Option<PlaylistTrackId>,
    start: u32,
    mixer_track: Option<TrackId>,
    browser: Option<LibraryFileToken>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "add_audio_clip_from_file")?;
    let session = session.inner().clone();
    let place = ClipPlace {
        track,
        start,
        mixer_track,
    };
    blocking(move || match browser {
        Some(token) => session.browser_add_clip(&path, place, token),
        None => session.add_audio_clip_from_file(&path, place),
    })
    .await
}

#[tauri::command]
fn add_audio_clip_from_sample(
    session: State<'_, Session>,
    sample: SampleId,
    track: Option<PlaylistTrackId>,
    start: u32,
    mixer_track: Option<TrackId>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "add_audio_clip_from_sample")?;
    let place = ClipPlace {
        track,
        start,
        mixer_track,
    };
    session.add_audio_clip_from_sample(sample, place)
}

#[tauri::command]
async fn export_audio(
    session: State<'_, Session>,
    options: ExportOptions,
    request: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "export_audio")?;
    let session = session.inner().clone();
    blocking(move || session.start_export_correlated(options, None, request)).await
}
#[tauri::command]
async fn export_plugin_bundle(
    session: State<'_, Session>,
    destination: String,
    runtime: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "export_plugin_bundle")?;
    let session = session.inner().clone();
    blocking(move || session.export_plugin_bundle(&destination, &runtime)).await
}

#[tauri::command]
async fn bounce_selected_clips(
    session: State<'_, Session>,
    clips: Vec<windfall_project::ClipId>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "bounce_selected_clips")?;
    let session = session.inner().clone();
    blocking(move || session.bounce_selected_clips(clips)).await
}

#[tauri::command]
async fn consolidate_playlist_tracks(
    session: State<'_, Session>,
    tracks: Vec<windfall_project::PlaylistTrackId>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "consolidate_playlist_tracks")?;
    let session = session.inner().clone();
    blocking(move || session.consolidate_playlist_tracks(tracks)).await
}

#[tauri::command]
async fn export_playlist_tracks(
    session: State<'_, Session>,
    options: ExportOptions,
    stems: windfall_ipc::ExportPlaylistStems,
    request: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "export_playlist_tracks")?;
    let session = session.inner().clone();
    blocking(move || session.start_export_correlated(options, Some(stems), request)).await
}

#[tauri::command]
async fn sampler_preprocess(
    session: State<'_, Session>,
    request: windfall_ipc::SamplerPreprocessRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "sampler_preprocess")?;
    let session = session.inner().clone();
    blocking(move || session.sampler_preprocess(request)).await
}

#[tauri::command]
async fn sampler_preprocess_preview(
    session: State<'_, Session>,
    request: windfall_ipc::SamplerPreprocessRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::SampleInfo, String> {
    let _window_call = caller.enter(&window, "sampler_preprocess_preview")?;
    let session = session.inner().clone();
    blocking(move || session.sampler_preprocess_preview(request)).await
}

#[tauri::command]
async fn collect_derived_assets(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u32, String> {
    let _window_call = caller.enter(&window, "collect_derived_assets")?;
    let session = session.inner().clone();
    blocking(move || session.collect_derived_assets()).await
}

#[tauri::command]
fn export_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "export_cancel")?;
    session.export_cancel();
    Ok(())
}

#[tauri::command]
async fn midi_preview(
    session: State<'_, Session>,
    path: String,
    options: MidiImportOptions,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<MidiImportPreview, String> {
    let _window_call = caller.enter(&window, "midi_preview")?;
    let session = session.inner().clone();
    blocking(move || session.midi_preview(&path, options)).await
}

#[tauri::command]
async fn import_midi(
    session: State<'_, Session>,
    token: u32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "import_midi")?;
    let session = session.inner().clone();
    blocking(move || session.import_midi(token)).await
}

#[tauri::command]
fn midi_discard(
    session: State<'_, Session>,
    token: u32,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "midi_discard")?;
    session.midi_discard(token);
    Ok(())
}

#[tauri::command]
async fn export_midi(
    session: State<'_, Session>,
    path: String,
    options: MidiExportOptions,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "export_midi")?;
    let session = session.inner().clone();
    blocking(move || session.export_midi(&path, options)).await
}

/// Writes a piano-roll score without touching the project or audio engine.
#[tauri::command]
async fn write_sheet_music(
    path: String,
    xml: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<String, String> {
    let _window_call = caller.enter(&window, "write_sheet_music")?;
    blocking(move || {
        std::fs::write(&path, xml.as_bytes())
            .map_err(|error| format!("Could not save sheet music: {error}"))?;
        Ok(path)
    })
    .await
}

#[tauri::command]
fn plugins_state(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::PluginManagerState, String> {
    let _window_call = caller.enter(&window, "plugins_state")?;
    Ok(manager.state())
}

#[tauri::command]
fn plugins_scan(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    retry: Option<String>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "plugins_scan")?;
    session.while_recording_idle(|| manager.inner().scan(retry))
}

#[tauri::command]
fn plugins_add_folder(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    folder: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "plugins_add_folder")?;
    session.while_recording_idle(|| manager.add_folder(folder))
}

#[tauri::command]
async fn plugins_add(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    path: String,
    id: String,
    track: Option<TrackId>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "plugins_add")?;
    let manager = manager.inner().clone();
    let session = session.inner().clone();
    blocking(move || {
        let target = track.map_or(
            windfall_project::PluginTarget::Instrument {
                channel: ChannelId(0),
            },
            |_| windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(0),
            },
        );
        let plugin = session.while_recording_idle(|| manager.binding(&path, &id, target))?;
        let command = match track {
            Some(track) => Command::AddPluginEffect { track, plugin },
            None => Command::AddPluginInstrument { plugin },
        };
        session.dispatch(command, None)
    })
    .await
}

#[tauri::command]
async fn plugin_editor(
    app: AppHandle,
    session: State<'_, Session>,
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    target: windfall_project::PluginTarget,
    open: bool,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "plugin_editor")?;
    #[cfg(windows)]
    let parent = app
        .get_webview_window(window.label())
        .and_then(|window| window.hwnd().ok())
        .map(|handle| handle.0 as usize);
    #[cfg(target_os = "macos")]
    let parent = app
        .get_webview_window("main")
        .and_then(|window| window.ns_window().ok())
        .map(|window| window as usize);
    #[cfg(not(any(windows, target_os = "macos")))]
    let parent = {
        let _ = app;
        None
    };
    let runtime = manager.runtime.clone();
    let session = session.inner().clone();
    blocking(move || {
        session.while_recording_idle(|| {
            let binding = session
                .document_snapshot()
                .project
                .plugin(target)
                .cloned()
                .ok_or("Plugin no longer exists")?;
            runtime.editor_binding_with_parent(target, Some(binding), open, parent)
        })
    })
    .await
}

fn panel_title(id: &str) -> Result<&'static str, String> {
    match id {
        "browser" => Ok("Browser"),
        "mixer" => Ok("Mixer"),
        "channelRack" => Ok("Channel rack"),
        "playlist" => Ok("Playlist"),
        "pianoRoll" => Ok("Piano roll"),
        _ => Err(format!("Unknown panel: {id}")),
    }
}

#[tauri::command]
async fn plugin_editor_scale(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    application: f64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "plugin_editor_scale")?;
    let runtime = manager.runtime.clone();
    blocking(move || runtime.set_editor_scale(application)).await
}

#[tauri::command]
async fn plugin_note_capabilities(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    target: windfall_project::PluginTarget,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<crate::plugins::NoteCapabilities, String> {
    let _window_call = caller.enter(&window, "plugin_note_capabilities")?;
    let runtime = manager.runtime.clone();
    blocking(move || runtime.note_capabilities(target)).await
}

// Creating a WebView2 window in a synchronous command can deadlock on Windows.
#[tauri::command]
async fn detach_panel(
    app: AppHandle,
    id: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "detach_panel")?;
    let title = panel_title(&id)?;
    let label = format!("panel-{id}");
    if app
        .state::<crate::window_calls::WindowCalls>()
        .closing(&label)
    {
        return Err("This window is closing.".into());
    }
    if let Some(window) = app.get_webview_window(&label) {
        return window.set_focus().map_err(|error| error.to_string());
    }
    let window = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App(format!("index.html?view=panel&id={id}").into()),
    )
    .title(title)
    .inner_size(960.0, 640.0)
    .build()
    .map_err(|error| error.to_string())?;
    crate::panel_layout::restore(&window);
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && let Some(main) = app.get_webview_window("main")
            && let Err(error) = main.emit("panel-docked", &id)
        {
            log::warn!("Could not notify the main window that {id} docked: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
async fn dock_panel(
    app: AppHandle,
    id: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "dock_panel")?;
    panel_title(&id)?;
    if app.get_webview_window("main").is_none() {
        return Err("Reopen the main window before docking a panel.".into());
    }
    if let Some(window) = app.get_webview_window(&format!("panel-{id}")) {
        let calls = app.state::<crate::window_calls::WindowCalls>();
        let token = calls.dock_target(window.label())?;
        crate::panel_layout::capture(&window, true);
        if let Err(error) = window.destroy() {
            calls.failed_dock(window.label(), token);
            return Err(error.to_string());
        }
    }
    Ok(())
}

/// Close only the authenticated window after its UI has resolved unsaved work.
#[tauri::command]
fn window_close(
    window: WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "window_close")?;
    let result = window.destroy().map_err(|error| error.to_string());
    if result.is_err()
        && window
            .app_handle()
            .get_webview_window(window.label())
            .is_some()
    {
        caller.failed_close(&window);
    }
    result
}

/// The handler for every custom command.
pub fn handler() -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        window_close,
        crate::updates::update_check,
        crate::updates::update_download,
        crate::updates::update_install,
        crate::updates::update_install_begin,
        crate::updates::update_install_cancel,
        analysis_capability,
        analysis_model_import,
        analysis_runtime_settings,
        analysis_runtime_configure,
        analysis_submit,
        analysis_status,
        analysis_cancel,
        analysis_cancel_preparation,
        analysis_forget,
        analysis_retry_cleanup,
        analysis_review,
        analysis_preview,
        analysis_apply,
        analysis_shutdown,
        detach_panel,
        dock_panel,
        current_mixer_target,
        mixer_waveform_tracks,
        mixer_preset_capture,
        mixer_preset_write,
        mixer_preset_read,
        mixer_preset_library,
        mixer_preset_library_save,
        mixer_preset_library_read,
        mixer_preset_library_delete,
        midi_hardware_state,
        midi_hardware_refresh,
        midi_hardware_configure,
        midi_hardware_target,
        midi_hardware_panic,
        controller_state,
        controller_settings_set,
        controller_learn,
        controller_learn_cancel,
        note_record_arm,
        automation_record_arm,
        score_log_dump,
        score_log_clear,
        recording_inputs,
        input_monitor_start,
        input_monitor_stop,
        input_monitor_state,
        recording_state,
        recording_start,
        recording_stop,
        recording_cancel,
        audio_logger_state,
        audio_logger_configure,
        audio_logger_dump,
        document_snapshot,
        window_document_snapshot,
        dispatch,
        automate,
        undo,
        redo,
        history_jump,
        project_new,
        project_open,
        flp_preview,
        flp_open,
        flp_cancel,
        project_save,
        project_save_new_version,
        project_archive_save,
        project_archive_cancel,
        recent_projects,
        transport_play,
        transport_stop,
        transport_toggle,
        transport_seek,
        transport_set,
        transport_state,
        timeline_state,
        timeline_region,
        realtime_subscribe,
        realtime_unsubscribe,
        analyzer_capability,
        analyzer_subscribe,
        analyzer_select,
        performance_pad,
        performance_stop_all,
        performance_state,
        media_render_audio,
        media_video_read,
        performance_scene,
        performance_record_state,
        performance_record_start,
        performance_record_stop,
        performance_record_cancel,
        performance_record_save,
        performance_take_replay,
        performance_take_export,
        analyzer_ack,
        analyzer_unsubscribe,
        proposal_stamp,
        assistant_respond,
        proposal_apply,
        proposal_undo,
        script_begin,
        script_cancel,
        script_parameters,
        script_preview,
        assistant_connection,
        assistant_configure,
        assistant_request_begin,
        assistant_request_cancel,
        assistant_remote,
        analyzer_smoke_enabled,
        analyzer_smoke_control,
        engine_status,
        process_memory,
        engine_devices,
        engine_settings,
        engine_configure,
        audition_note_on,
        audition_note_off,
        preview_play,
        preview_stop,
        browser_roots,
        library_search,
        library_refresh,
        library_cancel,
        library_file,
        library_metadata,
        library_set_metadata,
        browser_add_root,
        browser_remove_root,
        browser_list,
        sample_info,
        sample_info_by_id,
        prepare_clip_command,
        sampler_preparation_begin,
        sampler_preparation_cancel,
        sampler_preparation_progress,
        prepare_sampler_command,
        detect_clip_tempo,
        audio_editor_open,
        audio_editor_apply,
        audio_script_preview,
        audio_script_apply,
        audio_script_audition,
        audio_editor_discard,
        audio_editor_save_wavpack,
        m4a_capability,
        audio_analysis_detect,
        audio_analysis_audition,
        audio_analysis_apply,
        audio_analysis_send_midi,
        audio_analysis_cancel,
        audio_editor_regions,
        audio_editor_transfer,
        slice_analyze,
        sample_playback_detect,
        slice_apply,
        slice_discard,
        samples_reload,
        soundfont_presets,
        sample_instrument_speech,
        sample_instrument_speech_begin,
        sample_instrument_speech_cancel,
        sample_instrument_import_zone,
        sample_instrument_load_bank,
        music_lighting_status,
        sample_instrument_relink_source,
        sample_instrument_missing_sources,
        sample_instrument_save_bank,
        sample_instrument_general_midi,
        add_channel_from_file,
        set_channel_sample_from_file,
        add_audio_clip_from_file,
        add_audio_clip_from_sample,
        bounce_selected_clips,
        consolidate_playlist_tracks,
        export_playlist_tracks,
        sampler_preprocess,
        sampler_preprocess_preview,
        collect_derived_assets,
        export_audio,
        export_plugin_bundle,
        export_cancel,
        midi_preview,
        import_midi,
        midi_discard,
        export_midi,
        write_sheet_music,
        plugins_state,
        plugins_scan,
        plugins_add_folder,
        plugins_add,
        plugin_editor,
        plugin_editor_scale,
        plugin_note_capabilities,
        crate::files_commands::preset_capture_channel,
        crate::files_commands::preset_capture_plugin,
        crate::files_commands::preset_write,
        crate::files_commands::preset_read,
        crate::files_commands::preset_apply_channel,
        crate::files_commands::preset_library,
        crate::files_commands::preset_library_save,
        crate::files_commands::preset_library_delete,
        crate::files_commands::score_write,
        crate::files_commands::score_read,
        crate::files_commands::templates,
        crate::files_commands::template_save,
        crate::files_commands::template_new,
        crate::files_commands::template_delete,
        crate::files_commands::plugin_database,
        crate::files_commands::plugin_database_annotate,
        crate::files_commands::plugin_database_forget_missing,
        crate::files_commands::plugins_remove_folder,
        crate::files_commands::plugins_rescan,
        crate::files_commands::score_capture,
        crate::files_commands::score_apply,
        crate::files_commands::preset_apply_plugin,
        crate::files_commands::plugins_insert,
        phone_remote_state,
        phone_remote_set,
        backup_versions,
        backup_save,
        backup_restore,
        backup_cancel,
        loop_catalog,
        loop_preview,
        loop_insert,
        sample_playback_export,
    ]
}

#[tauri::command]
async fn sample_playback_export(
    session: State<'_, Session>,
    channel: ChannelId,
    path: String,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "sample_playback_export")?;
    let session = session.inner().clone();
    blocking(move || session.sample_playback_export(channel, &path, generation, revision)).await
}

#[tauri::command]
fn midi_hardware_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let _window_call = caller.enter(&window, "midi_hardware_state")?;
    Ok(session.midi_hardware_state())
}
#[tauri::command]
async fn midi_hardware_refresh(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let _window_call = caller.enter(&window, "midi_hardware_refresh")?;
    let session = session.inner().clone();
    blocking(move || session.midi_hardware_refresh()).await
}
#[tauri::command]
async fn midi_hardware_configure(
    session: State<'_, Session>,
    settings: windfall_ipc::MidiHardwareSettings,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let _window_call = caller.enter(&window, "midi_hardware_configure")?;
    let session = session.inner().clone();
    blocking(move || session.midi_hardware_configure(settings)).await
}
#[tauri::command]
fn midi_hardware_target(
    session: State<'_, Session>,
    channel: Option<ChannelId>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let _window_call = caller.enter(&window, "midi_hardware_target")?;
    session.midi_hardware_target(channel, generation, revision)
}
#[tauri::command]
fn midi_hardware_panic(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "midi_hardware_panic")?;
    session.midi_hardware_panic();
    Ok(())
}

#[tauri::command]
fn controller_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "controller_state")?;
    Ok(session.controller_state())
}
#[tauri::command]
async fn controller_settings_set(
    session: State<'_, Session>,
    settings: windfall_ipc::ControllerSettings,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "controller_settings_set")?;
    let session = session.inner().clone();
    blocking(move || session.controller_settings_set(settings)).await
}
#[tauri::command]
fn controller_learn(
    session: State<'_, Session>,
    targets: Vec<windfall_project::LinkTarget>,
    global: bool,
    multi: bool,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "controller_learn")?;
    session.controller_learn(targets, global, multi, generation)
}
#[tauri::command]
fn controller_learn_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "controller_learn_cancel")?;
    Ok(session.controller_learn_cancel())
}
#[tauri::command]
async fn note_record_arm(
    session: State<'_, Session>,
    armed: bool,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "note_record_arm")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.note_record_arm(armed))).await
}
#[tauri::command]
async fn automation_record_arm(
    session: State<'_, Session>,
    armed: bool,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "automation_record_arm")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.automation_record_arm(armed))).await
}
#[tauri::command]
async fn score_log_dump(
    session: State<'_, Session>,
    pattern: windfall_project::PatternId,
    seconds: Option<f32>,
    generation: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "score_log_dump")?;
    let session = session.inner().clone();
    blocking(move || session.score_log_dump(pattern, seconds, generation)).await
}
#[tauri::command]
fn score_log_clear(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::ControllerState, String> {
    let _window_call = caller.enter(&window, "score_log_clear")?;
    Ok(session.score_log_clear())
}

#[tauri::command]
async fn recording_inputs(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<windfall_ipc::RecordingInput>, String> {
    let _window_call = caller.enter(&window, "recording_inputs")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.recording_inputs())).await
}
#[tauri::command]
async fn input_monitor_start(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let _window_call = caller.enter(&window, "input_monitor_start")?;
    let session = session.inner().clone();
    blocking(move || session.input_monitor_start()).await
}
#[tauri::command]
async fn input_monitor_stop(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let _window_call = caller.enter(&window, "input_monitor_stop")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.input_monitor_stop())).await
}
#[tauri::command]
async fn input_monitor_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let _window_call = caller.enter(&window, "input_monitor_state")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.input_monitor_state())).await
}
#[tauri::command]
fn recording_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::RecordingState, String> {
    let _window_call = caller.enter(&window, "recording_state")?;
    Ok(session.recording_state())
}
#[tauri::command]
async fn recording_start(
    session: State<'_, Session>,
    source: windfall_ipc::RecordingSource,
    start: u32,
    track: Option<PlaylistTrackId>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::RecordingState, String> {
    let _window_call = caller.enter(&window, "recording_start")?;
    let session = session.inner().clone();
    blocking(move || session.recording_start(source, start, track)).await
}
#[tauri::command]
async fn recording_stop(
    session: State<'_, Session>,
    takes: Option<windfall_ipc::RecordingTakeSelection>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "recording_stop")?;
    let session = session.inner().clone();
    blocking(move || session.recording_stop_with_selection(takes)).await
}
#[tauri::command]
async fn recording_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "recording_cancel")?;
    let session = session.inner().clone();
    blocking(move || {
        session.recording_cancel();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn audio_logger_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AudioLoggerState, String> {
    let _window_call = caller.enter(&window, "audio_logger_state")?;
    let session = session.inner().clone();
    blocking(move || Ok(session.audio_logger_state())).await
}
#[tauri::command]
async fn audio_logger_configure(
    session: State<'_, Session>,
    settings: windfall_ipc::AudioLoggerSettings,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AudioLoggerState, String> {
    let _window_call = caller.enter(&window, "audio_logger_configure")?;
    let session = session.inner().clone();
    blocking(move || session.audio_logger_configure(settings)).await
}
#[tauri::command]
async fn audio_logger_dump(
    session: State<'_, Session>,
    request: windfall_ipc::AudioLoggerDumpRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "audio_logger_dump")?;
    let session = session.inner().clone();
    blocking(move || session.audio_logger_dump(request)).await
}
// Live analyzer controls. Each only changes the service's bookkeeping; the
// analysis itself runs on the realtime thread, which also sends the events.
#[tauri::command]
fn analyzer_capability(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalyzerCapability, String> {
    let _window_call = caller.enter(&window, "analyzer_capability")?;
    Ok(session.analyzer_capability())
}
#[tauri::command]
fn analyzer_subscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    slot: String,
    channel: Channel<tauri::ipc::InvokeResponseBody>,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalyzerStatus, String> {
    let _window_call = caller.enter(&window, "analyzer_subscribe")?;
    Ok({
        // The session hands over each event as JSON text it has already
        // measured against the size limit, so it goes out as it is.
        session.analyzer_subscribe(
            window.label(),
            &slot,
            Box::new(move |event| {
                channel
                    .send(tauri::ipc::InvokeResponseBody::Json(event.to_owned()))
                    .is_ok()
            }),
        )
    })
}
#[tauri::command]
fn analyzer_select(
    session: State<'_, Session>,
    window: WebviewWindow,
    select: windfall_ipc::AnalyzerSelect,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AnalyzerStatus, String> {
    let _window_call = caller.enter(&window, "analyzer_select")?;
    Ok(session.analyzer_select(window.label(), select))
}
#[tauri::command]
fn analyzer_ack(
    session: State<'_, Session>,
    window: WebviewWindow,
    ack: windfall_ipc::AnalyzerAck,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analyzer_ack")?;
    session.analyzer_ack(window.label(), ack);
    Ok(())
}
#[tauri::command]
fn analyzer_unsubscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    slot: String,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "analyzer_unsubscribe")?;
    session.analyzer_unsubscribe(window.label(), &slot);
    Ok(())
}

#[tauri::command]
fn proposal_stamp(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::proposal::ProposalStamp, String> {
    let _window_call = caller.enter(&window, "proposal_stamp")?;
    Ok(session.proposal_stamp())
}
#[tauri::command]
async fn assistant_respond(
    session: State<'_, Session>,
    prompt: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::assistant::AssistantReply, String> {
    let _window_call = caller.enter(&window, "assistant_respond")?;
    let session = session.inner().clone();
    blocking(move || session.assistant_respond(&prompt)).await
}
#[tauri::command]
async fn proposal_apply(
    session: State<'_, Session>,
    proposal: windfall_project::proposal::CommandProposal,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<DispatchResult, String> {
    let _window_call = caller.enter(&window, "proposal_apply")?;
    let session = session.inner().clone();
    blocking(move || session.proposal_apply(proposal)).await
}
#[tauri::command]
async fn proposal_undo(
    session: State<'_, Session>,
    stamp: windfall_project::proposal::ProposalStamp,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<ProjectPatch, String> {
    let _window_call = caller.enter(&window, "proposal_undo")?;
    let session = session.inner().clone();
    blocking(move || session.proposal_undo(stamp)).await
}
#[tauri::command]
fn script_begin(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u64, String> {
    let _window_call = caller.enter(&window, "script_begin")?;
    Ok(session.script_begin())
}
#[tauri::command]
fn script_cancel(
    session: State<'_, Session>,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "script_cancel")?;
    session.script_cancel(request);
    Ok(())
}
#[tauri::command]
async fn script_parameters(
    session: State<'_, Session>,
    source: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<Vec<windfall_project::scripting::ScriptParameter>, String> {
    let _window_call = caller.enter(&window, "script_parameters")?;
    let session = session.inner().clone();
    blocking(move || session.script_parameters(&source)).await
}
#[tauri::command]
async fn script_preview(
    session: State<'_, Session>,
    request: u64,
    input: windfall_project::scripting::PianoScriptRequest,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::scripting::PianoScriptPreview, String> {
    let _window_call = caller.enter(&window, "script_preview")?;
    let session = session.inner().clone();
    blocking(move || session.script_preview(request, input)).await
}

#[tauri::command]
async fn assistant_connection(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AssistantConnection, String> {
    let _window_call = caller.enter(&window, "assistant_connection")?;
    let session = session.inner().clone();
    blocking(move || session.assistant_connection()).await
}
#[tauri::command]
async fn assistant_configure(
    session: State<'_, Session>,
    update: windfall_ipc::AssistantConnectionUpdate,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::AssistantConnection, String> {
    let _window_call = caller.enter(&window, "assistant_configure")?;
    let session = session.inner().clone();
    blocking(move || session.assistant_configure(update)).await
}
#[tauri::command]
fn assistant_request_begin(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<u64, String> {
    let _window_call = caller.enter(&window, "assistant_request_begin")?;
    Ok(session.assistant_request_begin())
}
#[tauri::command]
fn assistant_request_cancel(
    session: State<'_, Session>,
    request: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "assistant_request_cancel")?;
    session.assistant_request_cancel(request);
    Ok(())
}
#[tauri::command]
async fn assistant_remote(
    session: State<'_, Session>,
    request: u64,
    prompt: String,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::assistant::AssistantReply, String> {
    let _window_call = caller.enter(&window, "assistant_remote")?;
    let session = session.inner().clone();
    blocking(move || session.assistant_remote(request, &prompt)).await
}

#[tauri::command]
fn analyzer_smoke_enabled(
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<bool, String> {
    let _window_call = caller.enter(&window, "analyzer_smoke_enabled")?;
    Ok(crate::analyzer_smoke::enabled())
}

#[tauri::command]
async fn analyzer_smoke_control(
    session: State<'_, Session>,
    operation: String,
    value: Option<f64>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<serde_json::Value, String> {
    let _window_call = caller.enter(&window, "analyzer_smoke_control")?;
    let session = session.inner().clone();
    blocking(move || crate::analyzer_smoke::control(&session, &operation, value)).await
}

#[tauri::command]
fn performance_pad(
    session: State<'_, Session>,
    track: u8,
    scene: Option<u8>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_pad")?;
    session.performance_pad(track, scene, generation, revision)
}
#[tauri::command]
fn performance_stop_all(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_stop_all")?;
    session.performance_stop_all();
    Ok(())
}
#[tauri::command]
fn performance_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::PerformanceState, String> {
    let _window_call = caller.enter(&window, "performance_state")?;
    Ok(session.performance_state())
}
#[tauri::command]
fn performance_record_state(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::PerformanceRecordingState, String> {
    let _window_call = caller.enter(&window, "performance_record_state")?;
    Ok(session.performance_record_state())
}
#[tauri::command]
async fn performance_record_start(
    session: State<'_, Session>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_record_start")?;

    let session = session.inner().clone();
    blocking(move || session.performance_record_start(generation, revision)).await
}
#[tauri::command]
fn performance_record_stop(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_record_stop")?;
    session.performance_record_stop();
    Ok(())
}
#[tauri::command]
fn performance_record_cancel(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_record_cancel")?;
    session.performance_record_cancel();
    Ok(())
}
#[tauri::command]
async fn performance_record_save(
    session: State<'_, Session>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_record_save")?;

    let session = session.inner().clone();
    blocking(move || session.performance_record_save()).await
}
#[tauri::command]
fn performance_take_replay(
    session: State<'_, Session>,
    index: Option<usize>,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_take_replay")?;
    session.performance_take_replay(index, generation, revision)
}
#[tauri::command]
async fn performance_take_export(
    session: State<'_, Session>,
    index: usize,
    path: String,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_take_export")?;

    let session = session.inner().clone();
    blocking(move || session.performance_take_export(index, &path, generation, revision)).await
}

#[tauri::command]
async fn media_render_audio(
    session: State<'_, Session>,
    range: windfall_project::TickRange,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_ipc::MediaRenderAudio, String> {
    let _window_call = caller.enter(&window, "media_render_audio")?;
    let session = session.inner().clone();
    blocking(move || session.media_render_audio(range, generation, revision)).await
}

#[tauri::command]
fn performance_scene(
    session: State<'_, Session>,
    scene: u8,
    generation: u64,
    revision: u64,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "performance_scene")?;
    session.performance_scene(scene, generation, revision)
}
#[tauri::command]
async fn media_video_read(
    session: State<'_, Session>,
    path: String,
    generation: u64,
    revision: Option<u64>,
    window: tauri::WebviewWindow,
    caller: crate::window_calls::WindowCaller,
) -> Result<tauri::ipc::Response, String> {
    let _window_call = caller.enter(&window, "media_video_read")?;
    let session = session.inner().clone();
    blocking(move || session.media_video_read_checked(&path, generation, revision))
        .await
        .map(tauri::ipc::Response::new)
}
