import type {
  FlpImportOptions,
  FlpImportPreview,
  AudioHost,
  AudioSettings,
  AutomationTarget,
  BrowserEntry,
  BrowserRoot,
  LibraryFileToken,
  LibraryMetadata,
  LibraryResults,
  LibrarySearch,
  ChannelId,
  ClipId,
  Command,
  DispatchResult,
  DocumentSnapshot,
  EngineStatus,
  ExportOptions,
  ExportFormat,
  ExportProgress,
  PlaylistTrackId,
  ProjectPatch,
  PluginManagerState,
  PluginTarget,
  RealtimeFrame,
  SampleId,
  SampleInfo,
  TrackId,
  TransportPatch,
  TransportState,
  MidiImportOptions,
  MidiImportPreview,
  MidiExportOptions,
} from "@/bindings"
import type { AnalysisBackend } from "@/features/analysis/types"
import type { AnalyzerBackend } from "@/features/analyzers/types"
import type { PanelId } from "@/lib/store/ui"

import type { ProjectFilesBackend } from "./files"
import type { MixerPresetLibraryBackend } from "./mixer-presets"
import type { RemoteContentBackend } from "./remote-content"

export type Unsubscribe = () => void

/**
 * `AudioSettings` as the shell sends it back. Rust writes an unset field as
 * `null`, where the generated type says the field is left out, so whatever
 * reads these must take both to mean "the default".
 */
export type StoredAudioSettings = {
  [Field in keyof AudioSettings]?: AudioSettings[Field] | null
}

/** Where a new audio clip goes. */
export type AudioClipPlace = {
  /** The playlist track. Left out, a new one is added at the end. */
  track?: PlaylistTrackId
  /** The tick the clip starts on. */
  start: number
  /**
   * The mixer track the clip plays into. Left out, a new one named after
   * the file is added, or the master is used when the mixer is full.
   */
  mixerTrack?: TrackId
}

/**
 * Everything the UI can ask of the app shell. One method per IPC command in
 * docs/ARCHITECTURE.md, plus events, file dialogs and the few window calls
 * the shell needs. A failed call rejects with an `Error` whose message is
 * plain text that can be shown to the user.
 */
export interface Backend extends AnalysisBackend, MixerPresetLibraryBackend, AnalyzerBackend, ProjectFilesBackend, RemoteContentBackend {
  musicLightingStatus?(): Promise<{connected: boolean; message: string}>
  sampleInstrumentLoadBank?(channel: ChannelId, expected: import("@/bindings").SampleInstrumentAsset | null,
    path: string): Promise<DispatchResult>
  sampleInstrumentGeneralMidi?(channel: ChannelId, expected: import("@/bindings").ChannelSource, path: string, bank: number, program: number): Promise<DispatchResult>
  sampleInstrumentMissingSources?(channel: ChannelId): Promise<SampleId[]>
  sampleInstrumentRelinkSource?(channel: ChannelId, expected: import("@/bindings").SampleInstrumentAsset | null,
    sample: SampleId, expectedPath: import("@/bindings").SamplePath, path: string): Promise<DispatchResult>
  sampleInstrumentSaveBank?(channel: ChannelId, expected: import("@/bindings").SampleInstrumentAsset | null,
    path: string): Promise<void>
  sampleInstrumentImportZone?(channel: ChannelId, expected: import("@/bindings").SampleInstrumentAsset | null,
    path: string, pad: number | null): Promise<DispatchResult>
  soundfontPresets?(path: string): Promise<import("@/bindings").SoundFontPreset[]>
  sampleInstrumentSpeech?(channel: ChannelId, expected: import("@/bindings").SampleInstrumentAsset | null,
    text: string, pitch: number, duration: number, request?: number): Promise<DispatchResult>
  sampleInstrumentSpeechBegin?(channel: ChannelId): Promise<number>
  sampleInstrumentSpeechCancel?(channel: ChannelId, request: number): Promise<void>

  pickVideoFile(): Promise<string | null>
  mediaVideoRead(path: string, generation: number): Promise<ArrayBuffer>
  performanceScene(scene: number, generation: number, revision: number): Promise<void>
  mediaRenderAudio(range: import("@/bindings").TickRange, generation: number, revision: number): Promise<import("@/bindings").MediaRenderAudio>
  performanceState(): Promise<import("@/bindings").PerformanceState>
  performancePad(track: number, scene: number | null, generation: number, revision: number): Promise<void>
  performanceStopAll(): Promise<void>
  mixerWaveformTracks(tracks: TrackId[], generation: number, revision: number): Promise<void>
  mixerPresetCapture(id: TrackId, generation: number, revision: number): Promise<import("@/bindings").MixerTrackPreset>
  mixerPresetSave(preset: import("@/bindings").MixerTrackPreset): Promise<string | null>
  mixerPresetLoad(): Promise<import("@/bindings").MixerTrackPreset | null>
  currentMixerTarget(track: TrackId | null, generation: number, revision: number): Promise<void>
  timelineState(): Promise<import("@/bindings").TimelinePlaybackState>
  timelineRegion(
    region: import("@/bindings").TickRange | null,
    generation: number,
    revision: number,
    request?: number,
    cancel?: import("@/bindings").TimelinePlaybackState
  ): Promise<import("@/bindings").TimelinePlaybackState>
  midiHardwareState(): Promise<import("@/bindings").MidiHardwareState>
  midiHardwareRefresh(): Promise<import("@/bindings").MidiHardwareState>
  midiHardwareConfigure(
    settings: import("@/bindings").MidiHardwareSettings
  ): Promise<import("@/bindings").MidiHardwareState>
  midiHardwareTarget(
    channel: ChannelId | null,
    generation: number,
    revision: number
  ): Promise<import("@/bindings").MidiHardwareState>
  midiHardwarePanic(): Promise<void>
  /** Controller links at work, learning, recording arms and the note log. */
  controllerState(): Promise<import("@/bindings").ControllerState>
  controllerSettingsSet(
    settings: import("@/bindings").ControllerSettings
  ): Promise<import("@/bindings").ControllerState>
  /**
   * Starts a pass of linking: the next hardware controls that move get
   * `targets`, in order. With `multi`, controls moved on screen join the
   * pass; with `global`, the links go to the app's settings.
   */
  controllerLearn(
    targets: import("@/bindings").LinkTarget[],
    global: boolean,
    multi: boolean,
    generation: number
  ): Promise<import("@/bindings").ControllerState>
  controllerLearnCancel(): Promise<import("@/bindings").ControllerState>
  noteRecordArm(armed: boolean): Promise<import("@/bindings").ControllerState>
  automationRecordArm(
    armed: boolean
  ): Promise<import("@/bindings").ControllerState>
  /** One undo step. `seconds` limits the dump to notes that recent. */
  scoreLogDump(
    pattern: number,
    seconds: number | null,
    generation: number
  ): Promise<DispatchResult>
  scoreLogClear(): Promise<import("@/bindings").ControllerState>
  audioEditorOpen(
    clip: number
  ): Promise<import("@/features/audio-editor/types").AudioEditPreview>
  audioEditorApply(
    request: import("@/features/audio-editor/types").AudioEditRequest
  ): Promise<DispatchResult>
  audioEditorDiscard(token: number): Promise<void>
  audioEditorSaveWavpack(token: number, start: number, end: number, path: string, depth: "int16" | "int24"): Promise<void>
  /** null means the installed OS AAC encoder can produce aligned M4A audio. */
  m4aCapability(): Promise<string | null>
  audioScriptPreview(input: import("@/features/audio-editor/types").AudioScriptRequest): Promise<import("@/features/audio-editor/types").AudioScriptPreview>
  audioScriptApply(token: number, request: number): Promise<DispatchResult>
  audioScriptAudition(token: number, request: number): Promise<void>
  audioEditorRegions(token: number, regions: import("@/features/audio-editor/types").AudioRegion[]): Promise<DispatchResult>
  audioEditorTransfer(request: import("@/features/audio-editor/transfer").AudioEditTransferRequest): Promise<DispatchResult>
  samplePlaybackDetect(channel: ChannelId, options: import("@/features/slicer/types").SliceOptions, generation: number, revision: number): Promise<DispatchResult>
  audioAnalysisDetect(token: number, polyphonic: boolean): Promise<import("@/features/audio-editor/analysis-types").AudioAnalysisPreview>
  audioAnalysisAudition(edit: import("@/features/audio-editor/analysis-types").AudioAnalysisEdit, dry: boolean): Promise<void>
  audioAnalysisApply(edit: import("@/features/audio-editor/analysis-types").AudioAnalysisEdit): Promise<DispatchResult>
  audioAnalysisSendMidi(edit: import("@/features/audio-editor/analysis-types").AudioAnalysisEdit, selected: number[]): Promise<DispatchResult>
  audioAnalysisCancel(): Promise<void>
  samplePlaybackExport(channel: ChannelId, generation: number, revision: number): Promise<void>
  sliceAnalyze(
    clip: number,
    options: import("@/features/slicer/types").SliceOptions
  ): Promise<import("@/features/slicer/types").SliceReview>
  sliceApply(token: number, markers: number[]): Promise<DispatchResult>
  sliceDiscard(token: number): Promise<void>
  audioLoggerState(): Promise<
    import("@/features/audio-logger/types").AudioLoggerState
  >
  audioLoggerConfigure(
    settings: import("@/features/audio-logger/types").AudioLoggerSettings
  ): Promise<import("@/features/audio-logger/types").AudioLoggerState>
  audioLoggerDump(
    request: import("@/features/audio-logger/types").AudioLoggerDumpRequest
  ): Promise<DispatchResult>
  recordingInputs(): Promise<import("@/bindings").RecordingInput[]>
  inputMonitorStart(): Promise<import("@/bindings").InputMonitorState>
  inputMonitorStop(): Promise<import("@/bindings").InputMonitorState>
  inputMonitorState(): Promise<import("@/bindings").InputMonitorState>
  recordingState(): Promise<import("@/bindings").RecordingState>
  recordingStart(
    source: import("@/bindings").RecordingSource,
    start: number,
    track: number | null
  ): Promise<import("@/bindings").RecordingState>
  recordingStop(takes?: import("@/bindings").RecordingTakeSelection): Promise<DispatchResult>
  recordingCancel(): Promise<void>

  /** "tauri" inside the app, "mock" in a plain browser. */
  readonly kind: "tauri" | "mock"
  pluginsState(): Promise<PluginManagerState>
  pluginsScan(retry?: string): Promise<void>
  pluginsAddFolder(folder: string): Promise<void>
  pluginsAdd(path: string, id: string, track?: TrackId): Promise<DispatchResult>
  pluginEditor(target: PluginTarget, open: boolean): Promise<void>
  pluginEditorScale(application: number): Promise<void>

  documentSnapshot(): Promise<DocumentSnapshot>
  proposalStamp(): Promise<import("@/bindings").ProposalStamp>
  assistantRespond(prompt: string): Promise<import("@/bindings").AssistantReply>
  proposalApply(proposal: import("@/bindings").CommandProposal): Promise<DispatchResult>
  proposalUndo(stamp: import("@/bindings").ProposalStamp): Promise<ProjectPatch>
  scriptBegin(): Promise<number>
  scriptCancel(request: number): Promise<void>
  scriptParameters(source: string): Promise<import("@/bindings").ScriptParameter[]>
  scriptPreview(request: number, input: import("@/bindings").PianoScriptRequest): Promise<import("@/bindings").PianoScriptPreview>
  assistantConnection(): Promise<import("@/bindings").AssistantConnection>
  assistantConfigure(update: import("@/bindings").AssistantConnectionUpdate): Promise<import("@/bindings").AssistantConnection>
  assistantRequestBegin(): Promise<number>
  assistantRequestCancel(request: number): Promise<void>
  assistantRemote(request: number, prompt: string): Promise<import("@/bindings").AssistantReply>
  dispatch(command: Command, gesture?: number): Promise<DispatchResult>
  prepareClipCommand(command: Command): Promise<DispatchResult>
  samplerPreparationBegin(): Promise<number>
  samplerPreparationCancel(request: number): Promise<void>
  samplerPreparationProgress(
    request: number
  ): Promise<import("@/bindings").SamplerPreparationProgress>
  prepareSamplerCommand(
    command: Command,
    request: number
  ): Promise<DispatchResult>
  /**
   * Renders a sampler channel's sample through offline treatments into a new
   * sample and gives it to the channel, as one undo step. `created` holds
   * the new sample. The original sample and its file are kept. Rejects when
   * nothing would change, when the project changed meanwhile, and in the
   * browser, which cannot render audio.
   */
  samplerPreprocess(
    request: import("@/bindings").SamplerPreprocessRequest
  ): Promise<DispatchResult>
  samplerPreprocessPreview(request: import("@/bindings").SamplerPreprocessRequest): Promise<import("@/bindings").SampleInfo>
  collectDerivedAssets(): Promise<number>
  detectClipTempo(
    sample: SampleId
  ): Promise<{ bpm: number; confidence: number }[]>
  undo(): Promise<ProjectPatch | null>
  redo(): Promise<ProjectPatch | null>
  historyJump(cursor: number): Promise<ProjectPatch>

  /** Prepares a checked FL import for review without replacing the project. */
  flpPreview(path: string, options: FlpImportOptions): Promise<FlpImportPreview>
  flpOpen(token: number): Promise<DocumentSnapshot>
  flpCancel(token: number): Promise<void>
  pickFlpFile(): Promise<string | null>

  projectNew(): Promise<DocumentSnapshot>
  projectOpen(path: string): Promise<DocumentSnapshot>
  /** Resolves to the saved path. Rejects when there is no path yet. */
  projectSave(path?: string): Promise<string>
  /** Native atomic numbered save. An unsaved project supplies a base path. */
  projectSaveNewVersion(path?: string): Promise<string>
  /** Native portable ZIP; rejects all missing audio and existing targets. */
  projectArchiveSave(path: string): Promise<string>
  projectArchiveCancel(): Promise<void>
  pickProjectArchivePath(suggestedName: string): Promise<string | null>
  recentProjects(): Promise<string[]>
  midiPreview(
    path: string,
    options: MidiImportOptions
  ): Promise<MidiImportPreview>
  importMidi(token: number): Promise<DispatchResult>
  midiDiscard(token: number): Promise<void>
  exportMidi(path: string, options: MidiExportOptions): Promise<string>
  /** Native save dialog, then MusicXML write. Cancellation writes nothing. */
  /**
   * Asks where to save a score and writes it as UTF-8 text. `format` picks
   * the extension and file filter: MusicXML when omitted, or an ASCII PDF.
   */
  sheetMusicSave(
    suggestedName: string,
    xml: string,
    format?: "musicxml" | "pdf"
  ): Promise<string | null>

  transportPlay(
    guard?: import("@/bindings").TimelinePlaybackState
  ): Promise<TransportState>
  transportStop(): Promise<TransportState>
  transportToggle(): Promise<TransportState>
  transportSeek(
    tick: number,
    guard?: import("@/bindings").TimelinePlaybackState
  ): Promise<void>
  transportSet(
    patch: TransportPatch,
    guard?: import("@/bindings").TimelinePlaybackState
  ): Promise<TransportState>
  transportState(): Promise<TransportState>

  engineStatus(): Promise<EngineStatus>
  /** Desktop host resident bytes; excludes webviews/helpers. Null if unavailable. */
  processMemory(): Promise<number | null>
  engineDevices(): Promise<AudioHost[]>
  engineConfigure(settings: AudioSettings): Promise<EngineStatus>
  /**
   * The audio output the user asked for, as stored. A field that is null or
   * missing means "the default"; what the engine made of it is in
   * `engineStatus`.
   */
  engineSettings(): Promise<StoredAudioSettings>

  auditionNoteOn(
    channel: ChannelId,
    key: number,
    velocity: number
  ): Promise<void>
  auditionNoteOff(channel: ChannelId, key: number): Promise<void>
  previewPlay(path: string, browser?: LibraryFileToken): Promise<void>
  previewStop(): Promise<void>

  browserRoots(): Promise<BrowserRoot[]>
  librarySearch(search: LibrarySearch): Promise<LibraryResults>
  libraryRefresh(): Promise<void>
  libraryCancel(generation: number): Promise<void>
  libraryFile(path: string): Promise<LibraryFileToken>
  libraryMetadata(path: string): Promise<LibraryMetadata>
  librarySetMetadata(
    path: string,
    metadata: LibraryMetadata
  ): Promise<LibraryMetadata>
  browserAddRoot(path: string): Promise<BrowserRoot[]>
  browserRemoveRoot(path: string): Promise<BrowserRoot[]>
  /** Folders first, then by name. */
  browserList(path: string): Promise<BrowserEntry[]>
  sampleInfo(path: string, browser?: LibraryFileToken): Promise<SampleInfo>
  /** Facts and waveform of a sample in the project's pool. */
  sampleInfoById(sample: SampleId): Promise<SampleInfo>
  /** One undo step: adds the sample to the pool and a channel that plays it. */
  addChannelFromFile(
    path: string,
    index?: number,
    browser?: LibraryFileToken
  ): Promise<DispatchResult>
  setChannelSampleFromFile(
    channel: ChannelId,
    path: string,
    browser?: LibraryFileToken
  ): Promise<DispatchResult>

  /**
   * One undo step, "Add audio clip": puts an audio file on the playlist as
   * a clip as long as the file lasts at the tempo the project has now.
   * `created` holds the sample (the one the project already had for the
   * file, if it had one), then the playlist track if one was made, then the
   * mixer track if one was made, and last the clip.
   */
  addAudioClipFromFile(
    path: string,
    place: AudioClipPlace,
    browser?: LibraryFileToken
  ): Promise<DispatchResult>
  /**
   * The same for a sample the project already has. `created` holds the
   * playlist track if one was made, the mixer track if one was made, and
   * last the clip. Rejects a sample whose audio is not loaded.
   */
  addAudioClipFromSample(
    sample: SampleId,
    place: AudioClipPlace
  ): Promise<DispatchResult>
  /** Render selected tracks' span from a copy, then add audio and mute sources in one undo batch. */
  bounceSelectedClips(clips: ClipId[]): Promise<DispatchResult>
  /**
   * Renders each of these playlist tracks' unmuted pattern and audio clips
   * to one audio clip on that track and mutes the sources: one undo batch
   * for any number of tracks. `created` holds a sample and then its clip
   * for each track, in playlist order.
   */
  consolidatePlaylistTracks(
    tracks: import("@/bindings").PlaylistTrackId[]
  ): Promise<DispatchResult>
  /**
   * One undo step, "Create automation clip": an automation of `target`
   * with one point at the value it has now, a new playlist track at the
   * end, and a clip of the automation on it from tick 0 for the length of
   * the song, at least four bars. `created` holds the automation, the
   * playlist track and the clip, in that order.
   */
  automate(target: AutomationTarget): Promise<DispatchResult>

  /**
   * Tries again to load the samples whose files were missing. Resolves to
   * how many still have no audio; which ones arrives through
   * `onProjectWarnings`.
   */
  samplesReload(): Promise<number>

  /** Returns at once. Progress arrives through `onExportProgress`. */
  exportAudio(options: ExportOptions): Promise<void>
  /**
   * Like `exportAudio`, with one file for each playlist track: the song
   * with only that track's pattern and audio clips placed, heard at the
   * output. Needs song mode, and `options.stems` must be absent.
   */
  exportPlaylistTracks(
    options: ExportOptions,
    stems: import("@/bindings").ExportPlaylistStems
  ): Promise<void>
  /** Stops the active export; its last progress event acknowledges cancellation. */
  exportCancel(): Promise<void>

  onProjectPatch(handler: (patch: ProjectPatch) => void): Unsubscribe
  onProjectLoaded(handler: (snapshot: DocumentSnapshot) => void): Unsubscribe
  onTransportState(handler: (state: TransportState) => void): Unsubscribe
  onEngineStatus(handler: (status: EngineStatus) => void): Unsubscribe
  onExportProgress(handler: (progress: ExportProgress) => void): Unsubscribe
  /** Notes a MIDI input played into the audition destination. */
  onMidiHardwareNote(
    handler: (note: import("@/bindings").MidiHardwareNote) => void
  ): Unsubscribe
  /** Problems found while loading a project, such as a missing sample. */
  onProjectWarnings(handler: (warnings: string[]) => void): Unsubscribe
  /** Frames arrive about 60 times a second while subscribed. */
  subscribeRealtime(handler: (frame: RealtimeFrame) => void): Unsubscribe

  /** File dialogs. Each resolves to `null` when the user cancels. */
  pickProjectToOpen(): Promise<string | null>
  pickProjectSavePath(suggestedName: string): Promise<string | null>
  pickExportPath(
    suggestedName: string,
    format?: ExportFormat
  ): Promise<string | null>
  pickFolder(): Promise<string | null>
  pickAudioFile(): Promise<string | null>
  pickMidiFile(): Promise<string | null>
  pickMidiExportPath(suggestedName: string): Promise<string | null>

  setWindowTitle(title: string): Promise<void>
  detachPanel(id: PanelId): Promise<void>
  dockPanel(id: PanelId): Promise<void>
  onPanelDocked(listener: (id: PanelId) => void): Unsubscribe
  /**
   * Asks `guard` before the window closes. The window closes only when it
   * resolves to true.
   */
  onCloseRequested(guard: () => Promise<boolean>): Unsubscribe
}

export const EVENTS = {
  panelDocked: "panel-docked",
  projectPatch: "project:patch",
  projectLoaded: "project:loaded",
  transportState: "transport:state",
  engineStatus: "engine:status",
  exportProgress: "export:progress",
  midiHardwareNote: "midi:note",
  projectWarnings: "project:warnings",
} as const

/** The audio files the engine can read, by extension. */
export const AUDIO_EXTENSIONS = [
  "wav",
  "wave",
  "aif",
  "aiff",
  "aifc",
  "flac",
  "mp3",
  "ogg",
  "oga",
  "ds",
  "wv",
]

function rejectionText(error: unknown): string {
  if (typeof error === "string") return error
  if (error instanceof Error) return error.message
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message
  }
  return "Something went wrong."
}

/**
 * Turns whatever a backend call rejected with into a readable message. The
 * document words a refused command as a phrase, such as "channel 7 does not
 * exist", so the message is given its capital here.
 */
export function errorMessage(error: unknown): string {
  const text = rejectionText(error)
  return text.charAt(0).toUpperCase() + text.slice(1)
}
