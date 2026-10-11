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
fn analysis_capability(session: State<'_, Session>) -> windfall_ipc::AnalysisCapability {
    session.analysis_capability()
}
#[tauri::command]
async fn analysis_model_import(
    session: State<'_, Session>,
    path: String,
    model: windfall_ipc::AnalysisModel,
) -> Result<windfall_ipc::AnalysisModel, String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_model_import(&path, model)).await
}
#[tauri::command]
async fn analysis_submit(
    session: State<'_, Session>,
    request: windfall_ipc::AnalysisSubmit,
) -> Result<windfall_ipc::AnalysisJob, String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_submit(request)).await
}
#[tauri::command]
fn analysis_status(
    session: State<'_, Session>,
    job: String,
) -> Result<windfall_ipc::AnalysisJob, String> {
    session.analysis_status(&job)
}
#[tauri::command]
async fn analysis_cancel(
    session: State<'_, Session>,
    job: String,
) -> Result<windfall_ipc::AnalysisJob, String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_cancel(&job)).await
}
#[tauri::command]
fn analysis_cancel_preparation(session: State<'_, Session>) {
    session.analysis_cancel_preparation();
}
#[tauri::command]
async fn analysis_forget(session: State<'_, Session>, job: String) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_forget(&job)).await
}
#[tauri::command]
async fn analysis_retry_cleanup(session: State<'_, Session>, job: String) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_retry_cleanup(&job)).await
}
#[tauri::command]
async fn analysis_review(
    session: State<'_, Session>,
    ticket: String,
) -> Result<windfall_ipc::AnalysisReview, String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_review(&ticket)).await
}
#[tauri::command]
async fn analysis_apply(
    session: State<'_, Session>,
    request: windfall_ipc::AnalysisApply,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.analysis_apply(request)).await
}
#[tauri::command]
async fn analysis_shutdown(session: State<'_, Session>) -> Result<(), String> {
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
) -> Result<(), String> {
    session.mixer_waveform_tracks(tracks, generation, revision)
}

#[tauri::command]
async fn mixer_preset_capture(
    session: State<'_, Session>,
    id: TrackId,
    generation: u64,
    revision: u64,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_capture(id, generation, revision)).await
}
#[tauri::command]
async fn mixer_preset_write(
    session: State<'_, Session>,
    path: String,
    preset: windfall_project::MixerTrackPreset,
) -> Result<String, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_write(path, preset)).await
}
#[tauri::command]
async fn mixer_preset_read(
    session: State<'_, Session>,
    path: String,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_read(path)).await
}
#[tauri::command]
async fn mixer_preset_library(
    session: State<'_, Session>,
) -> Result<crate::session::MixerPresetLibrary, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library()).await
}
#[tauri::command]
async fn mixer_preset_library_save(
    session: State<'_, Session>,
    preset: windfall_project::MixerTrackPreset,
) -> Result<crate::session::MixerPresetLibraryEntry, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_save(preset)).await
}
#[tauri::command]
async fn mixer_preset_library_read(
    session: State<'_, Session>,
    file: String,
) -> Result<windfall_project::MixerTrackPreset, String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_read(file)).await
}
#[tauri::command]
async fn mixer_preset_library_delete(
    session: State<'_, Session>,
    file: String,
) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.mixer_preset_library_delete(file)).await
}

#[tauri::command]
async fn slice_analyze(
    session: State<'_, Session>,
    clip: windfall_project::ClipId,
    options: windfall_ipc::SliceOptions,
) -> Result<windfall_ipc::SliceReview, String> {
    let session = session.inner().clone();
    blocking(move || session.slice_analyze(clip, options)).await
}
#[tauri::command]
async fn slice_apply(
    session: State<'_, Session>,
    token: u32,
    markers: Vec<u32>,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.slice_apply(token, markers)).await
}
#[tauri::command]
fn slice_discard(session: State<'_, Session>, token: u32) {
    session.slice_discard(token);
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
fn current_mixer_target(
    session: State<'_, Session>,
    track: Option<TrackId>,
    generation: u64,
    revision: u64,
) -> Result<(), String> {
    session.current_mixer_target(track, generation, revision)
}

#[tauri::command]
async fn dispatch(
    session: State<'_, Session>,
    command: Command,
    gesture: Option<u64>,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.dispatch(command, gesture)).await
}

#[tauri::command]
async fn prepare_clip_command(
    session: State<'_, Session>,
    command: Command,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.prepare_clip_command(command)).await
}
#[tauri::command]
fn sampler_preparation_begin(session: State<'_, Session>) -> Result<u64, String> {
    session.sampler_preparation_begin()
}
#[tauri::command]
fn sampler_preparation_cancel(session: State<'_, Session>, request: u64) {
    session.sampler_preparation_cancel(request);
}
#[tauri::command]
fn sampler_preparation_progress(
    session: State<'_, Session>,
    request: u64,
) -> windfall_ipc::SamplerPreparationProgress {
    session.sampler_preparation_progress(request)
}
#[tauri::command]
async fn prepare_sampler_command(
    session: State<'_, Session>,
    command: Command,
    request: u64,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.prepare_sampler_command(command, request)).await
}
#[tauri::command]
async fn detect_clip_tempo(
    session: State<'_, Session>,
    sample: SampleId,
) -> Result<Vec<windfall_ipc::ClipTempoCandidate>, String> {
    let session = session.inner().clone();
    blocking(move || session.detect_clip_tempo(sample)).await
}
#[tauri::command]
async fn audio_editor_open(
    session: State<'_, Session>,
    clip: windfall_project::ClipId,
) -> Result<windfall_ipc::AudioEditPreview, String> {
    let session = session.inner().clone();
    blocking(move || session.audio_editor_open(clip)).await
}
#[tauri::command]
async fn audio_editor_apply(
    session: State<'_, Session>,
    request: windfall_ipc::AudioEditRequest,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.audio_editor_apply(request)).await
}
#[tauri::command]
async fn audio_editor_discard(session: State<'_, Session>, token: u32) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || {
        session.audio_editor_discard(token);
        Ok(())
    })
    .await
}
#[tauri::command]
fn automate(
    session: State<'_, Session>,
    target: AutomationTarget,
) -> Result<DispatchResult, String> {
    session.automate(target)
}

#[tauri::command]
async fn undo(session: State<'_, Session>) -> Result<Option<ProjectPatch>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.undo())).await
}

#[tauri::command]
async fn redo(session: State<'_, Session>) -> Result<Option<ProjectPatch>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.redo())).await
}

#[tauri::command]
async fn history_jump(session: State<'_, Session>, cursor: u32) -> Result<ProjectPatch, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.history_jump(cursor))).await
}

#[tauri::command]
async fn project_new(session: State<'_, Session>) -> Result<DocumentSnapshot, String> {
    let session = session.inner().clone();
    blocking(move || session.project_new()).await
}

#[tauri::command]
async fn project_open(
    session: State<'_, Session>,
    path: String,
) -> Result<DocumentSnapshot, String> {
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
) -> Result<FlpImportPreview, String> {
    let session = session.inner().clone();
    blocking(move || session.flp_preview(&path, &options)).await
}

#[tauri::command]
fn flp_open(session: State<'_, Session>, token: u64) -> Result<DocumentSnapshot, String> {
    session.flp_open(token)
}

#[tauri::command]
fn flp_cancel(session: State<'_, Session>, token: u64) {
    session.flp_cancel(token);
}
#[tauri::command]
async fn project_save(session: State<'_, Session>, path: Option<String>) -> Result<String, String> {
    let session = session.inner().clone();
    blocking(move || session.project_save(path.as_deref())).await
}

#[tauri::command]
async fn project_save_new_version(
    session: State<'_, Session>,
    path: Option<String>,
) -> Result<String, String> {
    let session = session.inner().clone();
    blocking(move || session.project_save_new_version(path.as_deref())).await
}

#[tauri::command]
async fn project_archive_save(session: State<'_, Session>, path: String) -> Result<String, String> {
    let session = session.inner().clone();
    let job = session.archive_job()?;
    blocking(move || session.project_archive_save_with(&path, job)).await
}

#[tauri::command]
fn project_archive_cancel(session: State<'_, Session>) {
    session.project_archive_cancel();
}

#[tauri::command]
async fn recent_projects(session: State<'_, Session>) -> Result<Vec<String>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.recent_projects())).await
}

#[tauri::command]
fn transport_play(
    session: State<'_, Session>,
    guard: Option<windfall_ipc::TimelinePlaybackState>,
) -> Result<TransportState, String> {
    match guard {
        Some(guard) => session.timeline_transport_play(guard),
        None => session.transport_play(),
    }
}

#[tauri::command]
fn transport_stop(session: State<'_, Session>) -> TransportState {
    session.transport_stop()
}

#[tauri::command]
fn transport_toggle(session: State<'_, Session>) -> Result<TransportState, String> {
    session.transport_toggle()
}

#[tauri::command]
fn transport_seek(
    session: State<'_, Session>,
    tick: f64,
    guard: Option<windfall_ipc::TimelinePlaybackState>,
) -> Result<(), String> {
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
) -> Result<TransportState, String> {
    match guard {
        Some(guard) => session.timeline_transport_set(patch, guard),
        None => session.transport_set(patch),
    }
}

#[tauri::command]
fn transport_state(session: State<'_, Session>) -> TransportState {
    session.transport_state()
}

#[tauri::command]
fn timeline_state(session: State<'_, Session>) -> windfall_ipc::TimelinePlaybackState {
    session.timeline_state()
}

#[tauri::command]
fn timeline_region(
    session: State<'_, Session>,
    region: Option<windfall_project::TickRange>,
    generation: u64,
    revision: u64,
    request: Option<u64>,
    cancel: Option<windfall_ipc::TimelinePlaybackState>,
) -> Result<windfall_ipc::TimelinePlaybackState, String> {
    session.timeline_region_request(region, generation, revision, request, cancel)
}

#[tauri::command]
fn realtime_subscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    channel: Channel<RealtimeFrame>,
) {
    session.subscribe_realtime(
        window.label(),
        Box::new(move |frame| channel.send(frame.clone()).is_ok()),
    );
}

#[tauri::command]
fn engine_status(session: State<'_, Session>) -> EngineStatus {
    session.engine_status()
}

/// OS accounting stays off the async runtime and outside document/audio locks.
#[tauri::command]
async fn process_memory() -> Result<Option<u64>, String> {
    blocking(|| Ok(crate::process_memory::resident_bytes())).await
}

#[tauri::command]
async fn engine_devices(session: State<'_, Session>) -> Result<Vec<AudioHost>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_devices())).await
}

#[tauri::command]
async fn engine_settings(session: State<'_, Session>) -> Result<AudioSettings, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_settings())).await
}

#[tauri::command]
async fn engine_configure(
    session: State<'_, Session>,
    settings: AudioSettings,
) -> Result<EngineStatus, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.engine_configure(settings))).await
}

#[tauri::command]
fn audition_note_on(session: State<'_, Session>, channel: ChannelId, key: u8, velocity: f32) {
    session.audition_note_on(channel, key, velocity);
}

#[tauri::command]
fn audition_note_off(session: State<'_, Session>, channel: ChannelId, key: u8) {
    session.audition_note_off(channel, key);
}

#[tauri::command]
async fn preview_play(
    session: State<'_, Session>,
    path: String,
    browser: Option<LibraryFileToken>,
) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_preview(&path, &token),
        None => session.preview_play(&path),
    })
    .await
}

#[tauri::command]
fn preview_stop(session: State<'_, Session>) {
    session.preview_stop();
}

#[tauri::command]
async fn browser_roots(session: State<'_, Session>) -> Result<Vec<BrowserRoot>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.browser_roots())).await
}

#[tauri::command]
async fn library_search(
    session: State<'_, Session>,
    search: LibrarySearch,
) -> Result<LibraryResults, String> {
    let session = session.inner().clone();
    blocking(move || session.library_search(&search)).await
}

#[tauri::command]
fn library_refresh(session: State<'_, Session>) {
    session.library_refresh();
}

#[tauri::command]
fn library_cancel(session: State<'_, Session>, generation: u32) {
    session.library_cancel(generation);
}

#[tauri::command]
async fn library_file(
    session: State<'_, Session>,
    path: String,
) -> Result<LibraryFileToken, String> {
    let session = session.inner().clone();
    blocking(move || session.library_file(&path)).await
}

#[tauri::command]
async fn library_metadata(
    session: State<'_, Session>,
    path: String,
) -> Result<LibraryMetadata, String> {
    let session = session.inner().clone();
    blocking(move || session.library_metadata(&path)).await
}

#[tauri::command]
async fn library_set_metadata(
    session: State<'_, Session>,
    path: String,
    metadata: LibraryMetadata,
) -> Result<LibraryMetadata, String> {
    let session = session.inner().clone();
    blocking(move || session.library_set_metadata(&path, metadata)).await
}

#[tauri::command]
async fn browser_add_root(
    session: State<'_, Session>,
    path: String,
) -> Result<Vec<BrowserRoot>, String> {
    let session = session.inner().clone();
    blocking(move || session.browser_add_root(&path)).await
}

#[tauri::command]
async fn browser_remove_root(
    session: State<'_, Session>,
    path: String,
) -> Result<Vec<BrowserRoot>, String> {
    let session = session.inner().clone();
    blocking(move || session.browser_remove_root(&path)).await
}

#[tauri::command]
async fn browser_list(
    session: State<'_, Session>,
    path: String,
) -> Result<Vec<BrowserEntry>, String> {
    let session = session.inner().clone();
    blocking(move || session.browser_list(&path)).await
}

#[tauri::command]
async fn sample_info(
    session: State<'_, Session>,
    path: String,
    browser: Option<LibraryFileToken>,
) -> Result<SampleInfo, String> {
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
) -> Result<SampleInfo, String> {
    let session = session.inner().clone();
    blocking(move || session.sample_info_by_id(sample)).await
}

#[tauri::command]
async fn samples_reload(session: State<'_, Session>) -> Result<u32, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.samples_reload())).await
}

#[tauri::command]
async fn add_channel_from_file(
    session: State<'_, Session>,
    path: String,
    index: Option<u32>,
    browser: Option<LibraryFileToken>,
) -> Result<DispatchResult, String> {
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
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || match browser {
        Some(token) => session.browser_replace_sample(channel, &path, token),
        None => session.set_channel_sample_from_file(channel, &path),
    })
    .await
}

#[tauri::command]
async fn add_audio_clip_from_file(
    session: State<'_, Session>,
    path: String,
    track: Option<PlaylistTrackId>,
    start: u32,
    mixer_track: Option<TrackId>,
    browser: Option<LibraryFileToken>,
) -> Result<DispatchResult, String> {
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
) -> Result<DispatchResult, String> {
    let place = ClipPlace {
        track,
        start,
        mixer_track,
    };
    session.add_audio_clip_from_sample(sample, place)
}

#[tauri::command]
async fn export_audio(session: State<'_, Session>, options: ExportOptions) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.export_audio(options)).await
}

#[tauri::command]
async fn bounce_selected_clips(
    session: State<'_, Session>,
    clips: Vec<windfall_project::ClipId>,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.bounce_selected_clips(clips)).await
}

#[tauri::command]
async fn consolidate_playlist_tracks(
    session: State<'_, Session>,
    tracks: Vec<windfall_project::PlaylistTrackId>,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.consolidate_playlist_tracks(tracks)).await
}

#[tauri::command]
async fn export_playlist_tracks(
    session: State<'_, Session>,
    options: ExportOptions,
    stems: windfall_ipc::ExportPlaylistStems,
) -> Result<(), String> {
    let session = session.inner().clone();
    blocking(move || session.export_playlist_tracks(options, stems)).await
}

#[tauri::command]
async fn sampler_preprocess(
    session: State<'_, Session>,
    request: windfall_ipc::SamplerPreprocessRequest,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.sampler_preprocess(request)).await
}

#[tauri::command]
fn export_cancel(session: State<'_, Session>) {
    session.export_cancel();
}

#[tauri::command]
async fn midi_preview(
    session: State<'_, Session>,
    path: String,
    options: MidiImportOptions,
) -> Result<MidiImportPreview, String> {
    let session = session.inner().clone();
    blocking(move || session.midi_preview(&path, options)).await
}

#[tauri::command]
async fn import_midi(session: State<'_, Session>, token: u32) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.import_midi(token)).await
}

#[tauri::command]
fn midi_discard(session: State<'_, Session>, token: u32) {
    session.midi_discard(token);
}

#[tauri::command]
async fn export_midi(
    session: State<'_, Session>,
    path: String,
    options: MidiExportOptions,
) -> Result<String, String> {
    let session = session.inner().clone();
    blocking(move || session.export_midi(&path, options)).await
}

/// Writes a piano-roll score without touching the project or audio engine.
#[tauri::command]
async fn write_sheet_music(path: String, xml: String) -> Result<String, String> {
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
) -> windfall_ipc::PluginManagerState {
    manager.state()
}

#[tauri::command]
fn plugins_scan(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    retry: Option<String>,
) -> Result<(), String> {
    session.while_recording_idle(|| manager.inner().scan(retry))
}

#[tauri::command]
fn plugins_add_folder(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    folder: String,
) -> Result<(), String> {
    session.while_recording_idle(|| manager.add_folder(folder))
}

#[tauri::command]
async fn plugins_add(
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    session: State<'_, Session>,
    path: String,
    id: String,
    track: Option<TrackId>,
) -> Result<DispatchResult, String> {
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
    session: State<'_, Session>,
    manager: State<'_, std::sync::Arc<crate::plugins::PluginManager>>,
    target: windfall_project::PluginTarget,
    open: bool,
) -> Result<(), String> {
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
            runtime.editor_binding(target, Some(binding), open)
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

// Creating a WebView2 window in a synchronous command can deadlock on Windows.
#[tauri::command]
async fn detach_panel(app: AppHandle, id: String) -> Result<(), String> {
    let title = panel_title(&id)?;
    let label = format!("panel-{id}");
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
async fn dock_panel(app: AppHandle, id: String) -> Result<(), String> {
    panel_title(&id)?;
    if let Some(window) = app.get_webview_window(&format!("panel-{id}")) {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// The handler for every command above.
pub fn handler() -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        analysis_capability,
        analysis_model_import,
        analysis_submit,
        analysis_status,
        analysis_cancel,
        analysis_cancel_preparation,
        analysis_forget,
        analysis_retry_cleanup,
        analysis_review,
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
        analyzer_capability,
        analyzer_subscribe,
        analyzer_select,
        performance_pad,
        performance_stop_all,
        performance_state,
        media_render_audio,
        media_video_read,
        performance_scene,
        analyzer_ack,
        analyzer_unsubscribe,
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
        audio_editor_discard,
        slice_analyze,
        slice_apply,
        slice_discard,
        samples_reload,
        add_channel_from_file,
        set_channel_sample_from_file,
        add_audio_clip_from_file,
        add_audio_clip_from_sample,
        bounce_selected_clips,
        consolidate_playlist_tracks,
        export_playlist_tracks,
        sampler_preprocess,
        export_audio,
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
    ]
}

#[tauri::command]
fn midi_hardware_state(session: State<'_, Session>) -> windfall_ipc::MidiHardwareState {
    session.midi_hardware_state()
}
#[tauri::command]
async fn midi_hardware_refresh(
    session: State<'_, Session>,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let session = session.inner().clone();
    blocking(move || session.midi_hardware_refresh()).await
}
#[tauri::command]
async fn midi_hardware_configure(
    session: State<'_, Session>,
    settings: windfall_ipc::MidiHardwareSettings,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    let session = session.inner().clone();
    blocking(move || session.midi_hardware_configure(settings)).await
}
#[tauri::command]
fn midi_hardware_target(
    session: State<'_, Session>,
    channel: Option<ChannelId>,
    generation: u64,
    revision: u64,
) -> Result<windfall_ipc::MidiHardwareState, String> {
    session.midi_hardware_target(channel, generation, revision)
}
#[tauri::command]
fn midi_hardware_panic(session: State<'_, Session>) {
    session.midi_hardware_panic();
}

#[tauri::command]
fn controller_state(session: State<'_, Session>) -> windfall_ipc::ControllerState {
    session.controller_state()
}
#[tauri::command]
async fn controller_settings_set(
    session: State<'_, Session>,
    settings: windfall_ipc::ControllerSettings,
) -> Result<windfall_ipc::ControllerState, String> {
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
) -> Result<windfall_ipc::ControllerState, String> {
    session.controller_learn(targets, global, multi, generation)
}
#[tauri::command]
fn controller_learn_cancel(session: State<'_, Session>) -> windfall_ipc::ControllerState {
    session.controller_learn_cancel()
}
#[tauri::command]
async fn note_record_arm(
    session: State<'_, Session>,
    armed: bool,
) -> Result<windfall_ipc::ControllerState, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.note_record_arm(armed))).await
}
#[tauri::command]
async fn automation_record_arm(
    session: State<'_, Session>,
    armed: bool,
) -> Result<windfall_ipc::ControllerState, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.automation_record_arm(armed))).await
}
#[tauri::command]
async fn score_log_dump(
    session: State<'_, Session>,
    pattern: windfall_project::PatternId,
    seconds: Option<f32>,
    generation: u64,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.score_log_dump(pattern, seconds, generation)).await
}
#[tauri::command]
fn score_log_clear(session: State<'_, Session>) -> windfall_ipc::ControllerState {
    session.score_log_clear()
}

#[tauri::command]
async fn recording_inputs(
    session: State<'_, Session>,
) -> Result<Vec<windfall_ipc::RecordingInput>, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.recording_inputs())).await
}
#[tauri::command]
async fn input_monitor_start(
    session: State<'_, Session>,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let session = session.inner().clone();
    blocking(move || session.input_monitor_start()).await
}
#[tauri::command]
async fn input_monitor_stop(
    session: State<'_, Session>,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.input_monitor_stop())).await
}
#[tauri::command]
async fn input_monitor_state(
    session: State<'_, Session>,
) -> Result<windfall_ipc::InputMonitorState, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.input_monitor_state())).await
}
#[tauri::command]
fn recording_state(session: State<'_, Session>) -> windfall_ipc::RecordingState {
    session.recording_state()
}
#[tauri::command]
async fn recording_start(
    session: State<'_, Session>,
    source: windfall_ipc::RecordingSource,
    start: u32,
    track: Option<PlaylistTrackId>,
) -> Result<windfall_ipc::RecordingState, String> {
    let session = session.inner().clone();
    blocking(move || session.recording_start(source, start, track)).await
}
#[tauri::command]
async fn recording_stop(
    session: State<'_, Session>,
    takes: Option<windfall_ipc::RecordingTakeSelection>,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.recording_stop_with_selection(takes)).await
}
#[tauri::command]
async fn recording_cancel(session: State<'_, Session>) -> Result<(), String> {
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
) -> Result<windfall_ipc::AudioLoggerState, String> {
    let session = session.inner().clone();
    blocking(move || Ok(session.audio_logger_state())).await
}
#[tauri::command]
async fn audio_logger_configure(
    session: State<'_, Session>,
    settings: windfall_ipc::AudioLoggerSettings,
) -> Result<windfall_ipc::AudioLoggerState, String> {
    let session = session.inner().clone();
    blocking(move || session.audio_logger_configure(settings)).await
}
#[tauri::command]
async fn audio_logger_dump(
    session: State<'_, Session>,
    request: windfall_ipc::AudioLoggerDumpRequest,
) -> Result<DispatchResult, String> {
    let session = session.inner().clone();
    blocking(move || session.audio_logger_dump(request)).await
}
// Live analyzer controls. Each only changes the service's bookkeeping; the
// analysis itself runs on the realtime thread, which also sends the events.
#[tauri::command]
fn analyzer_capability(session: State<'_, Session>) -> windfall_ipc::AnalyzerCapability {
    session.analyzer_capability()
}
#[tauri::command]
fn analyzer_subscribe(
    session: State<'_, Session>,
    window: WebviewWindow,
    slot: String,
    channel: Channel<tauri::ipc::InvokeResponseBody>,
) -> windfall_ipc::AnalyzerStatus {
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
}
#[tauri::command]
fn analyzer_select(
    session: State<'_, Session>,
    window: WebviewWindow,
    select: windfall_ipc::AnalyzerSelect,
) -> windfall_ipc::AnalyzerStatus {
    session.analyzer_select(window.label(), select)
}
#[tauri::command]
fn analyzer_ack(session: State<'_, Session>, window: WebviewWindow, ack: windfall_ipc::AnalyzerAck) {
    session.analyzer_ack(window.label(), ack);
}
#[tauri::command]
fn analyzer_unsubscribe(session: State<'_, Session>, window: WebviewWindow, slot: String) {
    session.analyzer_unsubscribe(window.label(), &slot);
}

#[tauri::command]
fn performance_pad(session: State<'_, Session>, track: u8, scene: Option<u8>, generation: u64, revision: u64) -> Result<(), String> {
    session.performance_pad(track, scene, generation, revision)
}
#[tauri::command]
fn performance_stop_all(session: State<'_, Session>) { session.performance_stop_all(); }
#[tauri::command]
fn performance_state(session: State<'_, Session>) -> windfall_ipc::PerformanceState { session.performance_state() }

#[tauri::command]
async fn media_render_audio(session: State<'_, Session>, range: windfall_project::TickRange, generation: u64, revision: u64) -> Result<windfall_ipc::MediaRenderAudio, String> {
    let session = session.inner().clone();
    blocking(move || session.media_render_audio(range, generation, revision)).await
}

#[tauri::command]
fn performance_scene(session: State<'_, Session>, scene: u8, generation: u64, revision: u64) -> Result<(), String> {
    session.performance_scene(scene, generation, revision)
}
#[tauri::command]
async fn media_video_read(session: State<'_, Session>, path: String, generation: u64) -> Result<tauri::ipc::Response, String> {
    let session = session.inner().clone();
    blocking(move || session.media_video_read(&path, generation)).await.map(tauri::ipc::Response::new)
}
