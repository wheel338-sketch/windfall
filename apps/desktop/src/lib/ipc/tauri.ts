import { Channel, invoke } from "@tauri-apps/api/core"
import { message, translatedText } from "@/lib/i18n"
import { reportError } from "@/lib/errors"
import { nativeRemoteContent } from "./remote-content"
import { listen } from "@tauri-apps/api/event"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { open, save } from "@tauri-apps/plugin-dialog"

import type { DocumentSnapshot, RealtimeFrame } from "@/bindings"
import type {
  AnalyzerEvent,
  AnalyzerStatus,
} from "@/features/analyzers/types"

import {
  AUDIO_EXTENSIONS,
  EVENTS,
  errorMessage,
  type Backend,
  type Unsubscribe,
} from "./backend"

/** Rejections from Rust are plain strings; the UI wants `Error`s. */
async function invokeCall<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  try {
    return await invoke<T>(command, args)
  } catch (error) {
    throw new Error(errorMessage(error), { cause: error })
  }
}

function on<T>(event: string, handler: (payload: T) => void): Unsubscribe {
  let cancelled = false
  let unlisten: Unsubscribe | null = null
  void listen<T>(event, (message) => handler(message.payload)).then((fn) => {
    if (cancelled) fn()
    else unlisten = fn
  })
  return () => {
    cancelled = true
    unlisten?.()
  }
}

const PROJECT_FILTER = () => [{ name: translatedText("Windfall project"), extensions: ["windfall"] }]
const MIDI_FILTER = () => [{ name: translatedText("MIDI file"), extensions: ["mid", "midi"] }]

export function createTauriBackend(): Backend {
  let documentGeneration: number | null = null
  let documentEpoch = 0
  let callerToken: string | null = null
  type WindowSnapshot = { generation: number; caller?: { generation: number; token: string }; snapshot: DocumentSnapshot }
  let loading: Promise<WindowSnapshot> | null = null
  function loadIdentity() {
    if (loading) return loading
    const epoch = documentEpoch
    loading = invokeCall<WindowSnapshot>("window_document_snapshot").then((owned) => {
      if (!owned.caller?.token) throw new Error("The native window did not supply its caller identity")
      callerToken = owned.caller.token
      if (epoch === documentEpoch) documentGeneration = owned.generation
      return owned
    }).finally(() => { loading = null })
    return loading
  }
  async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const epoch = documentEpoch
    if (callerToken === null) await loadIdentity()
    if (epoch !== documentEpoch) throw new Error("The project was replaced before this window action could start")
    // Capture once before invoking; never retry a refused mutation with a new identity.
    const caller = { generation: generation(), token: callerToken }
    return invokeCall<T>(command, { ...args, caller })
  }
  function generation() {
    if (documentGeneration === null) throw new Error("The window is still loading its project")
    return documentGeneration
  }
  const realtimeHandlers = new Set<(frame: RealtimeFrame) => void>()
  let realtimeStarted = false
  let realtimeToken: string | null = null

  // The shell streams frames for the life of the window, so one channel is
  // opened on first use and shared by every subscriber.
  function startRealtime() {
    if (realtimeStarted) return
    realtimeStarted = true
    const token = crypto.randomUUID()
    realtimeToken = token
    const channel = new Channel<RealtimeFrame>()
    channel.onmessage = (frame) => {
      if (realtimeToken !== token) return
      for (const handler of realtimeHandlers) handler(frame)
    }
    call<void>("realtime_subscribe", { channel, token }).catch((error: unknown) => {
      if (realtimeToken === token) {
        realtimeStarted = false
        realtimeToken = null
      }
      console.error("realtime_subscribe failed:", errorMessage(error))
    })
  }

  return {
    kind: "tauri",
    proposalStamp: () => call("proposal_stamp"),
    assistantRespond: (prompt) => call("assistant_respond", { prompt }),
    proposalApply: (proposal) => call("proposal_apply", { proposal }),
    proposalUndo: (stamp) => call("proposal_undo", { stamp }),
    scriptBegin: () => call("script_begin"),
    scriptCancel: (request) => call("script_cancel", { request }),
    scriptParameters: (source) => call("script_parameters", { source }),
    scriptPreview: (request, input) => call("script_preview", { request, input }),
    assistantConnection: () => call("assistant_connection"),
    assistantConfigure: (update) => call("assistant_configure", { update }),
    assistantRequestBegin: () => call("assistant_request_begin"),
    assistantRequestCancel: (request) => call("assistant_request_cancel", { request }),
    assistantRemote: (request, prompt) => call("assistant_remote", { request, prompt }),
    presetCaptureChannel: (id, generation, revision) => call("preset_capture_channel", { id, generation, revision }),
    presetCapturePlugin: (target, generation, revision) => call("preset_capture_plugin", { target, generation, revision }),
    presetWrite: (path, capture) => call("preset_write", { path, capture }),
    presetRead: (path) => call("preset_read", { path }),
    presetApplyChannel: (path, id, expected, nameColor, withoutAudio, generation, revision) => call("preset_apply_channel", { path, id, expected, nameColor, withoutAudio, generation, revision }),
    presetApplyPlugin: (path, target, generation, revision) => call("preset_apply_plugin", { path, target, generation, revision }),
    pluginsInsert: (path, id, destination, track, generation, revision) => call("plugins_insert", { path, id, destination, track, generation, revision }),
    presetLibrary: () => call("preset_library"),
    presetLibrarySave: (name, capture) => call("preset_library_save", { name, capture }),
    presetLibraryDelete: (path) => call("preset_library_delete", { path }),
    scoreCapture: (pattern, channel, only, generation, revision) => call("score_capture", { pattern, channel, only, generation, revision }),
    scoreApply: (score, pattern, channel, mode, generation, revision) => call("score_apply", { score, pattern, channel, mode, generation, revision }),
    scoreWrite: (path, score) => call("score_write", { path, score }),
    scoreRead: (path) => call("score_read", { path }),
    templates: () => call("templates"),
    templateSave: (name) => call("template_save", { name }),
    templateNew: (id) => call("template_new", { id }),
    templateDelete: (id) => call("template_delete", { id }),
    pluginDatabase: () => call("plugin_database"),
    pluginDatabaseAnnotate: (format, id, annotation) => call("plugin_database_annotate", { format, id, annotation }),
    pluginDatabaseForgetMissing: () => call("plugin_database_forget_missing"),
    pluginsRemoveFolder: (folder) => call("plugins_remove_folder", { folder }),
    pluginsRescan: (everything) => call("plugins_rescan", { everything }),
    pickPresetPath: async (saveName) => {
      const filters = [{ name: translatedText("Windfall preset"), extensions: ["wfpreset", "wfmixer"] }]
      const path = saveName === undefined ? await open({ multiple: false, filters }) : await save({ defaultPath: `${saveName.replace(/[<>:"/\\|?*]/g, "_")}.wfpreset`, filters })
      return typeof path === "string" ? path : null
    },
    pickScorePath: async (saveName) => {
      const filters = [{ name: translatedText("Windfall score"), extensions: ["wfscore"] }, { name: translatedText("MIDI score"), extensions: ["mid", "midi"] }]
      const path = saveName === undefined ? await open({ multiple: false, filters }) : await save({ defaultPath: `${saveName.replace(/[<>:"/\\|?*]/g, "_")}.wfscore`, filters })
      return typeof path === "string" ? path : null
    },
    async pickVideoFile() {
      const picked = await open({ title: translatedText("Choose a local video"), multiple: false, filters: [{ name: translatedText("Video"), extensions: ["mp4", "webm", "ogv", "mov", "avi"] }] })
      return typeof picked === "string" ? picked : null
    },
    mediaVideoRead: (path, generation, revision) => call("media_video_read", { path, generation, revision }),
    performanceScene: (scene, generation, revision) => call("performance_scene", { scene, generation, revision }),
    mediaRenderAudio: (range, generation, revision) => call("media_render_audio", { range, generation, revision }),
    performanceState: () => call("performance_state"),
    performancePad: (track, scene, generation, revision) => call("performance_pad", { track, scene, generation, revision }),
    performanceStopAll: () => call("performance_stop_all"),
    performanceRecordState: () => call("performance_record_state"),
    performanceRecordStart: (generation, revision) => call("performance_record_start", { generation, revision }),
    performanceRecordStop: () => call("performance_record_stop"),
    performanceRecordCancel: () => call("performance_record_cancel"),
    performanceRecordSave: () => call("performance_record_save"),
    performanceTakeReplay: (index, generation, revision) => call("performance_take_replay", { index, generation, revision }),
    performanceTakeExport: (index, path, generation, revision) => call("performance_take_export", { index, path, generation, revision }),
    ...nativeRemoteContent(call),
    analyzerCapability: () => call("analyzer_capability"),
    analyzerSubscribe: (slot, handler) => {
      // One channel per slot. Events that arrive after the slot stopped
      // belong to nobody and are dropped here.
      let stopped = false
      const channel = new Channel<AnalyzerEvent>()
      channel.onmessage = (event) => {
        if (!stopped) handler(event)
      }
      const status = call<AnalyzerStatus>("analyzer_subscribe", {
        slot,
        channel,
      })
      return {
        status,
        stop: () => {
          stopped = true
          call<void>("analyzer_unsubscribe", { slot }).catch(
            (error: unknown) => {
              console.error("analyzer_unsubscribe failed:", errorMessage(error))
            }
          )
        },
      }
    },
    analyzerSelect: (select) => call("analyzer_select", { select }),
    analyzerAck: (slot, sequence) => {
      call<void>("analyzer_ack", { ack: { slot, sequence } }).catch(
        (error: unknown) => {
          console.error("analyzer_ack failed:", errorMessage(error))
        }
      )
    },
    analysisCapability: () => call("analysis_capability"),
    analysisRuntimeSettings: () => call("analysis_runtime_settings"),
    analysisRuntimeConfigure: (settings) => call("analysis_runtime_configure", { settings }),
    pickAnalysisRuntime: async () => {
      const picked = await open({ title: "Choose installed ONNX Runtime library", multiple: false,
        filters: [{ name: "Shared library", extensions: ["dll", "so", "dylib"] }] })
      return typeof picked === "string" ? picked : null
    },
    analysisModelImport: (path, model) =>
      call("analysis_model_import", { path, model }),
    pickAnalysisModel: async () => {
      const picked = await open({
        title: translatedText("Import analysis model"),
        multiple: false,
        directory: false,
        filters: [{ name: translatedText("Analysis model"), extensions: ["onnx"] }],
      })
      return typeof picked === "string" ? picked : null
    },
    analysisSubmit: (request) => call("analysis_submit", { request }),
    analysisStatus: (job) => call("analysis_status", { job }),
    analysisCancel: (job) => call("analysis_cancel", { job }),
    analysisCancelPreparation: () => call("analysis_cancel_preparation"),
    analysisForget: (job) => call("analysis_forget", { job }),
    analysisRetryCleanup: (job) => call("analysis_retry_cleanup", { job }),
    analysisReview: (ticket) => call("analysis_review", { ticket }),
    analysisPreview: (ticket, role) => call("analysis_preview", { ticket, role }),
    analysisApply: (request) => call("analysis_apply", { request }),
    analysisShutdown: () => call("analysis_shutdown"),
    detachPanel: (id) => call("detach_panel", { id }),
    dockPanel: (id) => call("dock_panel", { id }),
    onPanelDocked: (listener) => on(EVENTS.panelDocked, listener),
    mixerWaveformTracks: (tracks, generation, revision) => call("mixer_waveform_tracks", { tracks, generation, revision }),
    mixerPresetCapture: (id, generation, revision) => call("mixer_preset_capture", { id, generation, revision }),
    mixerPresetSave: async (preset) => {
      const path = await save({ defaultPath: `${preset.name.replace(/[<>:"/\\|?*]/g, "_") || "Mixer track"}.wfmixer`, filters: [{ name: translatedText("Windfall mixer preset"), extensions: ["wfmixer"] }] })
      return path ? call("mixer_preset_write", { path, preset }) : null
    },
    mixerPresetLoad: async () => {
      const path = await open({ multiple: false, filters: [{ name: translatedText("Windfall mixer preset"), extensions: ["wfmixer"] }] })
      return typeof path === "string" ? call("mixer_preset_read", { path }) : null
    },
    mixerPresetLibrary: () => call("mixer_preset_library"),
    mixerPresetLibrarySave: (preset) => call("mixer_preset_library_save", { preset }),
    mixerPresetLibraryRead: (file) => call("mixer_preset_library_read", { file }),
    mixerPresetLibraryDelete: (file) => call("mixer_preset_library_delete", { file }),
    currentMixerTarget: (track, generation, revision) => call("current_mixer_target", { track, generation, revision }),
    timelineState: () => call("timeline_state"),
    timelineRegion: (region, generation, revision, request, cancel) =>
      call("timeline_region", {
        region,
        generation,
        revision,
        request,
        cancel,
      }),
    midiHardwareState: () => call("midi_hardware_state"),
    midiHardwareRefresh: () => call("midi_hardware_refresh"),
    midiHardwareConfigure: (settings) =>
      call("midi_hardware_configure", { settings }),
    midiHardwareTarget: (channel, generation, revision) =>
      call("midi_hardware_target", { channel, generation, revision }),
    midiHardwarePanic: () => call("midi_hardware_panic"),
    controllerState: () => call("controller_state"),
    controllerSettingsSet: (settings) =>
      call("controller_settings_set", { settings }),
    controllerLearn: (targets, global, multi, generation) =>
      call("controller_learn", { targets, global, multi, generation }),
    controllerLearnCancel: () => call("controller_learn_cancel"),
    noteRecordArm: (armed) => call("note_record_arm", { armed }),
    automationRecordArm: (armed) => call("automation_record_arm", { armed }),
    scoreLogDump: (pattern, seconds, generation) =>
      call("score_log_dump", { pattern, seconds, generation }),
    scoreLogClear: () => call("score_log_clear"),
    audioEditorOpen: (clip) => call("audio_editor_open", { clip }),
    audioEditorApply: (request) => call("audio_editor_apply", { request }),
    audioEditorDiscard: (token) => call("audio_editor_discard", { token }),
    audioEditorSaveWavpack: (token, start, end, path, depth) => call("audio_editor_save_wavpack", { token, start, end, path, depth }),
    m4aCapability: () => call("m4a_capability"),
    audioScriptPreview: (input) => call("audio_script_preview", { input }),
    audioScriptApply: (token, request) => call("audio_script_apply", { token, request }),
    audioScriptAudition: (token, request) => call("audio_script_audition", { token, request }),
    audioEditorRegions: (token, regions) => call("audio_editor_regions", { token, regions }),
    audioEditorTransfer: (request) => call("audio_editor_transfer", { request }),
    samplePlaybackDetect: (channel, options, generation, revision) => call("sample_playback_detect", { channel, options, generation, revision }),
    audioAnalysisDetect: (token, polyphonic) => call("audio_analysis_detect", { token, polyphonic }),
    audioAnalysisAudition: (edit, dry) => call("audio_analysis_audition", { edit, dry }),
    audioAnalysisApply: (edit) => call("audio_analysis_apply", { edit }),
    audioAnalysisSendMidi: (edit, selected) => call("audio_analysis_send_midi", { edit, selected }),
    audioAnalysisCancel: () => call("audio_analysis_cancel"),
    samplePlaybackExport: async (channel, generation, revision) => {
      const path = await save({ title: translatedText("Export source WAV with slices"), filters: [{ name: translatedText("WAV audio"), extensions: ["wav"] }] })
      if (path) await call("sample_playback_export", { channel, path, generation, revision })
    },
    sliceAnalyze: (clip, options) => call("slice_analyze", { clip, options }),
    sliceApply: (token, markers) => call("slice_apply", { token, markers }),
    sliceDiscard: (token) => call("slice_discard", { token }),
    pluginsState: () => call("plugins_state"),
    pluginsScan: (retry) => call("plugins_scan", { retry }),
    pluginsAddFolder: (folder) => call("plugins_add_folder", { folder }),
    pluginsAdd: (path, id, track) => call("plugins_add", { path, id, track }),
    pluginEditor: (target, open) => call("plugin_editor", { target, open }),
    pluginEditorScale: (application) => call("plugin_editor_scale", { application }),
    onWindowScale: (handler) => on("window:dpi", handler),

    documentSnapshot: async () => {
      const epoch = documentEpoch
      const owned = await loadIdentity()
      if (epoch !== documentEpoch) throw new Error("The project was replaced while its snapshot loaded")
      documentGeneration = owned.generation
      return owned.snapshot
    },
    dispatch: async (command, gesture) => call("dispatch", { command, gesture, generation: generation() }),
    undo: async () => call("undo", { generation: generation() }),
    redo: async () => call("redo", { generation: generation() }),
    historyJump: async (cursor) => call("history_jump", { cursor, generation: generation() }),

    flpPreview: (path, options) => call("flp_preview", { path, options }),
    flpOpen: (token) => call("flp_open", { token }),
    flpCancel: (token) => call("flp_cancel", { token }),
    async pickFlpFile() {
      const picked = await open({
        title: translatedText("Import FL Studio project"),
        multiple: false,
        directory: false,
        filters: [{ name: translatedText("FL Studio project"), extensions: ["flp"] }],
      })
      return typeof picked === "string" ? picked : null
    },

    projectNew: () => call("project_new"),
    projectOpen: (path) => call("project_open", { path }),
    projectSave: (path) => call("project_save", { path }),
    projectSaveNewVersion: (path) => call("project_save_new_version", { path }),
    projectArchiveSave: (path) => call("project_archive_save", { path }),
    projectArchiveCancel: () => call("project_archive_cancel"),
    pickProjectArchivePath: (suggestedName) =>
      save({
        title: translatedText("Export portable project archive (choose a new filename)"),
        defaultPath: `${suggestedName}.zip`,
        filters: [{ name: translatedText("Windfall project archive"), extensions: ["zip"] }],
      }),
    recentProjects: () => call("recent_projects"),
    midiPreview: (path, options) => call("midi_preview", { path, options }),
    importMidi: (token) => call("import_midi", { token }),
    midiDiscard: (token) => call("midi_discard", { token }),
    exportMidi: (path, options) => call("export_midi", { path, options }),
    sheetMusicSave: async (suggestedName, xml, format = "musicxml") => {
      const path = await save({
        title: translatedText("Export sheet music"),
        defaultPath: `${suggestedName.replace(/[<>:"/\\|?*]/g, "_") || "Score"}.${format === "pdf" ? "pdf" : "musicxml"}`,
        filters:
          format === "pdf"
            ? [{ name: translatedText("PDF score"), extensions: ["pdf"] }]
            : [{ name: translatedText("MusicXML score"), extensions: ["musicxml", "xml"] }],
      })
      return path ? call("write_sheet_music", { path, xml }) : null
    },
    async pickMidiFile() {
      const result = await open({
        title: translatedText("Import MIDI"),
        multiple: false,
        filters: MIDI_FILTER(),
      })
      return typeof result === "string" ? result : null
    },
    pickMidiExportPath: (suggestedName) =>
      save({
        title: translatedText("Export MIDI"),
        defaultPath: `${suggestedName}.mid`,
        filters: MIDI_FILTER(),
      }),

    transportPlay: (guard) => call("transport_play", { guard }),
    transportStop: () => call("transport_stop"),
    transportToggle: () => call("transport_toggle"),
    transportSeek: (tick, guard) => call("transport_seek", { tick, guard }),
    transportSet: (patch, guard) => call("transport_set", { patch, guard }),
    transportState: () => call("transport_state"),

    engineStatus: () => call("engine_status"),
    processMemory: () => call("process_memory"),
    audioLoggerState: () => call("audio_logger_state"),
    audioLoggerConfigure: (settings) =>
      call("audio_logger_configure", { settings }),
    audioLoggerDump: (request) => call("audio_logger_dump", { request }),
    recordingInputs: () => call("recording_inputs"),
    inputMonitorStart: () => call("input_monitor_start"),
    inputMonitorStop: () => call("input_monitor_stop"),
    inputMonitorState: () => call("input_monitor_state"),
    recordingState: () => call("recording_state"),
    recordingStart: (source, start, track) =>
      call("recording_start", { source, start, track }),
    recordingStop: (takes) => call("recording_stop", { takes: takes ?? null }),
    recordingCancel: () => call("recording_cancel"),
    engineDevices: () => call("engine_devices"),
    engineConfigure: (settings) => call("engine_configure", { settings }),
    engineSettings: () => call("engine_settings"),

    auditionNoteOn: (channel, key, velocity) =>
      call("audition_note_on", { channel, key, velocity }),
    auditionNoteOff: (channel, key) =>
      call("audition_note_off", { channel, key }),
    previewPlay: (path, browser) => call("preview_play", { path, browser }),
    previewStop: () => call("preview_stop"),

    browserRoots: () => call("browser_roots"),
    librarySearch: (search) => call("library_search", { search }),
    libraryRefresh: () => call("library_refresh"),
    libraryCancel: (generation) => call("library_cancel", { generation }),
    libraryFile: (path) => call("library_file", { path }),
    libraryMetadata: (path) => call("library_metadata", { path }),
    librarySetMetadata: (path, metadata) =>
      call("library_set_metadata", { path, metadata }),
    browserAddRoot: (path) => call("browser_add_root", { path }),
    browserRemoveRoot: (path) => call("browser_remove_root", { path }),
    browserList: (path) => call("browser_list", { path }),
    sampleInfo: (path, browser) => call("sample_info", { path, browser }),
    prepareClipCommand: (command) => call("prepare_clip_command", { command }),
    samplerPreparationBegin: () => call("sampler_preparation_begin"),
    samplerPreparationCancel: (request) =>
      call("sampler_preparation_cancel", { request }),
    samplerPreparationProgress: (request) =>
      call("sampler_preparation_progress", { request }),
    prepareSamplerCommand: (command, request) =>
      call("prepare_sampler_command", { command, request }),
    samplerPreprocess: (request) => call("sampler_preprocess", { request }),
    samplerPreprocessPreview: (request) => call("sampler_preprocess_preview", { request }),
    collectDerivedAssets: () => call("collect_derived_assets"),
    detectClipTempo: (sample) => call("detect_clip_tempo", { sample }),
    sampleInfoById: (sample) => call("sample_info_by_id", { sample }),
    addChannelFromFile: (path, index, browser) =>
      call("add_channel_from_file", { path, index, browser }),
    setChannelSampleFromFile: (channel, path, browser) =>
      call("set_channel_sample_from_file", { channel, path, browser }),
    addAudioClipFromFile: (path, place, browser) =>
      call("add_audio_clip_from_file", { path, ...place, browser }),
    addAudioClipFromSample: (sample, place) =>
      call("add_audio_clip_from_sample", { sample, ...place }),
    bounceSelectedClips: (clips) => call("bounce_selected_clips", { clips }),
    consolidatePlaylistTracks: (tracks) =>
      call("consolidate_playlist_tracks", { tracks }),
    automate: (target) => call("automate", { target }),
    samplesReload: () => call("samples_reload"),
    musicLightingStatus: () => call("music_lighting_status"),
    sampleInstrumentLoadBank: (channel, expected, path) => call("sample_instrument_load_bank", { channel, expected, path }),
    sampleInstrumentGeneralMidi: (channel, expected, path, bank, program) => call("sample_instrument_general_midi", { channel, expected, path, bank, program }),
    sampleInstrumentMissingSources: (channel) => call("sample_instrument_missing_sources", { channel }),
    sampleInstrumentRelinkSource: (channel, expected, sample, expectedPath, path) => call("sample_instrument_relink_source", { channel, expected, sample, expectedPath, path }),
    sampleInstrumentSaveBank: (channel, expected, path) => call("sample_instrument_save_bank", { channel, expected, path }),
    sampleInstrumentImportZone: (channel, expected, path, pad) => call("sample_instrument_import_zone", { channel, expected, path, pad }),
    soundfontPresets: (path) => call("soundfont_presets", { path }),
    sampleInstrumentSpeech: (channel, expected, text, pitch, duration, request) => call("sample_instrument_speech", { channel, expected, text, pitch, duration, request }),
    sampleInstrumentSpeechBegin: (channel) => call("sample_instrument_speech_begin", { channel }),
    sampleInstrumentSpeechCancel: (channel, request) => call("sample_instrument_speech_cancel", { channel, request }),

    exportAudio: (options, request) => call("export_audio", { options, request }),
    exportPlaylistTracks: (options, stems, request) =>
      call("export_playlist_tracks", { options, stems, request }),
    exportCancel: () => call("export_cancel"),

    onProjectPatch: (handler) => on(EVENTS.projectPatch, handler),
    onProjectLoaded: (handler) => on<WindowSnapshot>(EVENTS.projectLoaded, (owned) => {
      documentEpoch += 1
      documentGeneration = owned.generation
      handler(owned.snapshot)
    }),
    onTransportState: (handler) => on(EVENTS.transportState, handler),
    onEngineStatus: (handler) => on(EVENTS.engineStatus, handler),
    onExportProgress: (handler) => on(EVENTS.exportProgress, handler),
    onMidiHardwareNote: (handler) => on(EVENTS.midiHardwareNote, handler),
    onProjectWarnings: (handler) => on(EVENTS.projectWarnings, handler),

    subscribeRealtime(handler) {
      realtimeHandlers.add(handler)
      startRealtime()
      return () => {
        realtimeHandlers.delete(handler)
        if (realtimeHandlers.size === 0 && realtimeToken !== null) {
          const token = realtimeToken
          realtimeToken = null
          realtimeStarted = false
          void call<void>("realtime_unsubscribe", { token }).catch((error: unknown) =>
            console.error("realtime_unsubscribe failed:", errorMessage(error)))
        }
      }
    },

    async pickProjectToOpen() {
      const picked = await open({
        title: translatedText("Open project"),
        multiple: false,
        directory: false,
        filters: [
          {
            name: translatedText("Windfall project or archive"),
            extensions: ["windfall", "zip"],
          },
        ],
      })
      return typeof picked === "string" ? picked : null
    },
    pickProjectSavePath: (suggestedName) =>
      save({
        title: translatedText("Save project as"),
        defaultPath: `${suggestedName}.windfall`,
        filters: PROJECT_FILTER(),
      }),
    pickExportPath: (suggestedName, format = "wav") =>
      save({
        title: translatedText("Export audio"),
        defaultPath: `${suggestedName}.${format}`,
        filters: [
          { name: message("{format} audio", { format: format.toUpperCase() }), extensions: [format] },
        ],
      }),
    async pickFolder() {
      const picked = await open({
        title: translatedText("Choose a folder"),
        multiple: false,
        directory: true,
      })
      return typeof picked === "string" ? picked : null
    },

    async pickAudioFile() {
      const picked = await open({
        title: translatedText("Choose an audio file"),
        multiple: false,
        directory: false,
        filters: [{ name: translatedText("Audio"), extensions: AUDIO_EXTENSIONS }],
      })
      return typeof picked === "string" ? picked : null
    },

    setWindowTitle: (title) => getCurrentWindow().setTitle(title),

    onCloseRequested(guard) {
      const window = getCurrentWindow()
      let cancelled = false
      let unlisten: Unsubscribe | null = null
      void window
        .onCloseRequested(async (event) => {
          // The default close has to be stopped synchronously; the window is
          // destroyed by hand once the guard agrees.
          event.preventDefault()
          const epoch = documentEpoch
          if (!(await guard())) return
          try {
            if (epoch !== documentEpoch) throw new Error("The project was replaced while confirming window close")
            await call("window_close")
          } catch (error) {
            // Native caller/operation ownership can refuse a stale close.
            reportError(error, translatedText("The window could not close"))
          }
        })
        .then((fn) => {
          if (cancelled) fn()
          else unlisten = fn
        })
      return () => {
        cancelled = true
        unlisten?.()
      }
    },
  }
}
