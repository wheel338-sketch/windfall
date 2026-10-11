import { unavailableAnalysis } from "@/features/analysis/unavailable"
import { NATIVE_ONLY, unavailableLogger } from "@/features/audio-logger/types"
import type {
  FlpImportPreview,
  ImportReport,
  AudioHost,
  BrowserRoot,
  Command,
  DispatchResult,
  DocumentSnapshot,
  EngineStatus,
  ExportProgress,
  Project,
  ProjectPatch,
  RealtimeFrame,
  SampleAsset,
  TransportState,
} from "@/bindings"

import { DEFAULT_TRACK_PROCESSING } from "@/lib/track-processing"
import { isPanelId, type PanelId } from "@/lib/store/ui"
import { mixerTrackOfSample } from "@/lib/audio-clips"
import { ticksPerBar } from "@/lib/time"
import {
  MASTER_TRACK,
  MAX_MIXER_TRACKS,
  MAX_SONG_TICKS,
  PPQ,
} from "@/lib/units"

import type {
  AudioClipPlace,
  Backend,
  StoredAudioSettings,
  Unsubscribe,
} from "./backend"
import {
  baseName,
  defaultRoots,
  FACTORY_ROOT,
  isAudioPath,
  listFolder,
  sampleInfoFor,
  samplePathFor,
} from "./sim/browser"
import { sim } from "./sim/wasm"
import { SimDocument } from "./sim/document"
import { simulatedLatencyFrames } from "./sim/effects"
import { demoProject, starterProject } from "./sim/project"
import { TransportSim } from "./sim/transport"
import { createMidiMock, type MockMidiOptions } from "./sim/midi"
import { createMidiHardwareMock } from "./sim/midi-hardware"
import { createSlicerMock, type SlicerMockOptions } from "./sim/slicer"
import { createLibraryMock, storedBrowserRoots } from "./sim/library"

/** Stand-ins for the native file dialogs. Each resolves to null on cancel. */
export type MockDialogs = {
  openProject(saved: string[]): Promise<string | null>
  saveProject(suggestedPath: string): Promise<string | null>
  exportPath(suggestedPath: string): Promise<string | null>
  folder(): Promise<string | null>
  flpFile?(): Promise<string | null>
  audioFile(): Promise<string | null>
}

export type MockOptions = MockMidiOptions &
  SlicerMockOptions & {
    /** Original FL bytes supplied by tests, or selected with the browser picker. */
    flpFiles?: Record<string, Uint8Array>
    /** Where "saved" projects live. Pass null to keep them in memory only. */
    storage?: Pick<Storage, "getItem" | "setItem"> | null
    dialogs?: MockDialogs
    /** The project to start with. Defaults to the demo beat. */
    project?: Project
    /** Test render fixture. Browser previews have no offline renderer and reject bounce without it. */
    renderPlaylistBounce?: (
      source: Project,
      region: { start: number; end: number }
    ) => Promise<string>
  }

/** A backend that lives in the page, and what tidies it up. */
export type MockBackend = Backend & {
  /** Successful requests to open a panel window, in call order. */
  detachCalls: PanelId[]
  /**
   * Stops the mock's timers and frees its document in the WebAssembly
   * module. A call that needs the document rejects afterwards.
   */
  dispose(): void
}

// Each path maps to the text of a `.windfall` file.
const FILES_KEY = "windfall.mock.project-files"
const RECENT_KEY = "windfall.mock.recent-projects"
/**
 * How each saved project was being played: song or pattern, and whether the
 * song loops. The shell keeps this with the project file. Here it is kept
 * beside the files until the document itself carries it.
 */
const TRANSPORT_KEY = "windfall.mock.project-transport"

type SavedTransport = Pick<TransportState, "mode" | "loopSong">
const FILE_EXTENSION = ".windfall"
const FRAME_MS = 1000 / 60

const HOSTS: AudioHost[] = [
  {
    name: "WASAPI",
    isDefault: true,
    devices: [
      {
        name: "Speakers (simulated)",
        isDefault: true,
        sampleRates: [44_100, 48_000, 96_000],
        minBufferFrames: 64,
        maxBufferFrames: 4096,
      },
      {
        name: "Headphones (simulated)",
        isDefault: false,
        sampleRates: [44_100, 48_000],
        minBufferFrames: 128,
        maxBufferFrames: 2048,
      },
    ],
  },
  {
    name: "ASIO",
    isDefault: false,
    devices: [
      {
        name: "Audio interface (simulated)",
        isDefault: true,
        sampleRates: [44_100, 48_000, 88_200, 96_000, 192_000],
        minBufferFrames: 32,
        maxBufferFrames: 2048,
      },
    ],
  },
]

function promptDialogs(): MockDialogs {
  const ask = (message: string, suggestion: string) =>
    Promise.resolve(window.prompt(message, suggestion)?.trim() || null)
  return {
    openProject(saved) {
      if (saved.length === 0) {
        return Promise.reject(
          new Error("No projects are saved in this browser yet.")
        )
      }
      return ask(
        `Open project. Saved in this browser:\n${saved.join("\n")}`,
        saved[0]
      )
    },
    saveProject: (suggested) => ask("Save project as", suggested),
    exportPath: (suggested) => ask("Export audio to", suggested),
    folder: () => ask("Folder to add", "/samples/My samples"),
    audioFile: () =>
      ask("Audio file to load", `${FACTORY_ROOT}/Drums/Kicks/Kick 01.wav`),
  }
}

class Emitter<T> {
  private handlers = new Set<(value: T) => void>()

  on(handler: (value: T) => void): Unsubscribe {
    this.handlers.add(handler)
    return () => {
      this.handlers.delete(handler)
    }
  }

  emit(value: T) {
    for (const handler of [...this.handlers]) handler(value)
  }

  get size() {
    return this.handlers.size
  }
}

/**
 * The stored request the way the shell's JSON has it: all four fields, and
 * `null` for each one left to the system.
 */
function storedSettings(settings: StoredAudioSettings): StoredAudioSettings {
  return {
    host: settings.host ?? null,
    device: settings.device ?? null,
    sampleRate: settings.sampleRate ?? null,
    bufferFrames: settings.bufferFrames ?? null,
    outputChannels: settings.outputChannels ?? null,
  }
}

function describeStatus(settings: StoredAudioSettings): EngineStatus {
  const host =
    HOSTS.find((item) => item.name === settings.host) ??
    HOSTS.find((item) => item.isDefault) ??
    HOSTS[0]
  const device =
    settings.device == null
      ? host.devices.find((item) => item.isDefault)
      : host.devices.find((item) => item.name === settings.device)
  const sampleRate = settings.sampleRate ?? 48_000
  const bufferFrames = settings.bufferFrames ?? 256
  const base = {
    host: host.name,
    sampleRate,
    bufferFrames,
    latencyMs: (bufferFrames / sampleRate) * 1000,
    // Filled in from the project each time the status is asked for.
    latencyFrames: 0,
  }
  if (settings.host != null && host.name !== settings.host) {
    return {
      ...base,
      running: false,
      device: null,
      error: `The audio driver "${settings.host}" is not available.`,
    }
  }
  if (!device) {
    return {
      ...base,
      running: false,
      device: null,
      error: `The device "${settings.device}" was not found.`,
    }
  }
  if (!device.sampleRates.includes(sampleRate)) {
    return {
      ...base,
      running: false,
      device: device.name,
      error: `${device.name} does not support ${sampleRate} Hz.`,
    }
  }
  return { ...base, running: true, device: device.name, error: null }
}

/**
 * Starts a message from the document, which begins in lower case, with a
 * capital, as the shell does for what it shows about files.
 */
function sentence(error: unknown): Error {
  const text = error instanceof Error ? error.message : String(error)
  return new Error(text.charAt(0).toUpperCase() + text.slice(1))
}

/** Adds `.windfall` to a file name that does not end in it. */
function withProjectExtension(filePath: string): string {
  return filePath.toLowerCase().endsWith(FILE_EXTENSION)
    ? filePath
    : filePath + FILE_EXTENSION
}

/**
 * The whole backend, in the browser. The project is a real document: the
 * Rust `Document` compiled to WebAssembly, so every command, limit, label,
 * error and saved file is the app's own. What is made up here is the shell
 * around it and the engine: files that live in the browser's storage, a
 * playhead that follows the tempo and meters that move with the beat.
 */
export function createMockBackend(options: MockOptions = {}): MockBackend {
  const storage =
    options.storage === undefined
      ? typeof localStorage === "undefined"
        ? null
        : localStorage
      : options.storage
  const dialogs = options.dialogs ?? promptDialogs()

  let doc = SimDocument.create(options.project ?? demoProject())
  let path: string | null = null
  let transport = new TransportSim(doc.project().patterns[0].id)
  let timelineDocument = doc
  let timelineGeneration = 1
  let timelineRequest = 0
  let timelinePlayRequest = 0
  function timelineState() {
    if (timelineDocument !== doc) {
      timelineDocument = doc
      timelineGeneration += 1
    }
    return {
      request: timelineRequest,
      generation: timelineGeneration,
      revision: doc.snapshot(path).revision,
      region: transport.region,
      navigationOverflows: transport.navigationOverflows,
    }
  }
  function checkTimelineGuard(
    guard?: import("@/bindings").TimelinePlaybackState
  ) {
    if (!guard) return
    const current = timelineState()
    if (
      guard.generation !== current.generation ||
      guard.revision !== current.revision ||
      !Number.isSafeInteger(guard.request) ||
      guard.request <= 0 ||
      guard.request !== current.request ||
      !guard.region ||
      !current.region ||
      guard.region.start !== current.region.start ||
      guard.region.end !== current.region.end
    )
      throw new Error(
        "The project or timeline selection changed before playback could continue."
      )
  }
  let audioSettings = storedSettings({})
  let engine = describeStatus(audioSettings)
  let roots: BrowserRoot[] = [...defaultRoots(), ...storedBrowserRoots(storage)]
  const library = createLibraryMock(storage, () => roots)
  const flpFiles = new Map(Object.entries(options.flpFiles ?? {}))
  let flpSequence = 0
  let flpPending: {
    preview: FlpImportPreview
    next: SimDocument
    original: SimDocument
    originalRevision: number
  } | null = null
  let memoryFiles: Record<string, string> = {}
  let memoryRecent: string[] = []
  let memoryTransports: Record<string, SavedTransport> = {}

  const patches = new Emitter<ProjectPatch>()
  const panelDocked = new Emitter<PanelId>()
  const detachCalls: PanelId[] = []
  const loaded = new Emitter<DocumentSnapshot>()
  const transportStates = new Emitter<TransportState>()
  const engineStatuses = new Emitter<EngineStatus>()
  const exportProgress = new Emitter<ExportProgress>()
  const warnings = new Emitter<string[]>()
  const frames = new Emitter<RealtimeFrame>()

  let frameTimer: ReturnType<typeof setInterval> | null = null
  let lastFrameAt = 0
  let exporting: {
    timer: ReturnType<typeof setInterval>
    path: string
    fraction: number
  } | null = null

  function readJson<T>(key: string, fallback: T): T {
    try {
      const text = storage?.getItem(key)
      return text ? (JSON.parse(text) as T) : fallback
    } catch {
      return fallback
    }
  }

  function files(): Record<string, string> {
    return storage ? readJson(FILES_KEY, {}) : memoryFiles
  }

  function recent(): string[] {
    return storage ? readJson(RECENT_KEY, []) : memoryRecent
  }

  function savedTransports(): Record<string, SavedTransport> {
    return storage ? readJson(TRANSPORT_KEY, {}) : memoryTransports
  }

  /** Keeps how the project is being played with its file. */
  function rememberTransport(savedPath: string) {
    const { mode, loopSong } = transport.state
    const next = { ...savedTransports(), [savedPath]: { mode, loopSong } }
    if (!storage) {
      memoryTransports = next
      return
    }
    try {
      storage.setItem(TRANSPORT_KEY, JSON.stringify(next))
    } catch {
      // The project itself was saved. It opens in pattern mode.
    }
  }

  function remember(savedPath: string, fileText: string) {
    const nextFiles = { ...files(), [savedPath]: fileText }
    const nextRecent = [
      savedPath,
      ...recent().filter((item) => item !== savedPath),
    ].slice(0, 8)
    if (!storage) {
      memoryFiles = nextFiles
      memoryRecent = nextRecent
      return
    }
    try {
      storage.setItem(FILES_KEY, JSON.stringify(nextFiles))
      storage.setItem(RECENT_KEY, JSON.stringify(nextRecent))
    } catch {
      throw new Error(
        "The browser's storage is full, so the project was not saved."
      )
    }
  }

  /** The status with the delay the project's effects and instruments add. */
  function currentEngine(): EngineStatus {
    return {
      ...engine,
      latencyFrames: simulatedLatencyFrames(doc.project(), engine.sampleRate),
    }
  }

  function emitTransport(): TransportState {
    transportStates.emit(transport.state)
    return transport.state
  }

  /** The transport's pattern can vanish on a delete or an undo. */
  function repairTransportPattern() {
    const patterns = doc.project().patterns
    if (patterns.some((pattern) => pattern.id === transport.state.pattern)) {
      return
    }
    transport.set({ pattern: patterns[0].id }, doc.project())
    emitTransport()
  }

  /** Sends a patch the document just produced to everyone listening. */
  function publish(patch: ProjectPatch): ProjectPatch {
    repairTransportPattern()
    patches.emit(patch)
    return patch
  }

  function dispatchNow(command: Command, gesture?: number): DispatchResult {
    const result = doc.dispatch(command, gesture)
    publish(result.patch)
    return result
  }

  function sampleFile(sample: SampleAsset): string {
    return sample.path.kind === "factory"
      ? `${FACTORY_ROOT}/${sample.path.path}`
      : sample.path.path
  }

  /** One warning for each sample whose file the made-up disk does not have. */
  function missingSamples(): string[] {
    return doc.project().samples.flatMap((sample) => {
      try {
        sampleInfoFor(roots, sampleFile(sample))
        return []
      } catch {
        return [`The sample "${sample.name}" is missing: ${sampleFile(sample)}`]
      }
    })
  }

  /**
   * Swaps in a document that is ready, as the shell does after new and
   * open. A project that was saved comes back the way it was being played:
   * a song opens in song mode. The transport is announced after the
   * project, so whoever hears it already has the project it is for.
   */
  function load(next: SimDocument, loadedPath: string | null) {
    doc.dispose()
    doc = next
    path = loadedPath
    const saved =
      loadedPath === null ? undefined : savedTransports()[loadedPath]
    transport = new TransportSim(
      doc.project().patterns[0].id,
      saved?.loopSong ?? transport.state.loopSong
    )
    if (saved?.mode === "song") transport.set({ mode: "song" }, doc.project())
    const snapshot = doc.snapshot(path)
    loaded.emit(snapshot)
    emitTransport()
    const missing = missingSamples()
    if ((doc.project().retainedPlugins?.length ?? 0) > 0)
      missing.push(
        `${doc.project().retainedPlugins?.length} imported plugin states are retained; unsupported instruments stay silent and effects bypassed.`
      )
    if (missing.length > 0) warnings.emit(missing)
    return snapshot
  }

  /** The shell refuses to play a song that would be silent, and says why. */
  function startPlayback() {
    if (transport.state.mode === "song" && !transport.state.playing) {
      const { clips, tracks } = doc.project().playlist
      if (clips.length === 0) {
        throw new Error(
          "The playlist is empty. Add a clip to the playlist, or switch to pattern mode."
        )
      }
      const muted = new Set(
        tracks.filter((track) => track.muted).map((track) => track.id)
      )
      if (clips.every((clip) => clip.muted || muted.has(clip.track))) {
        throw new Error(
          "Every clip on the playlist is muted. Unmute a clip, or switch to pattern mode."
        )
      }
    }
    transport.play(doc.project())
  }

  function startFrames() {
    if (frameTimer !== null) return
    lastFrameAt = performance.now()
    frameTimer = setInterval(() => {
      const now = performance.now()
      const seconds = Math.min(0.25, (now - lastFrameAt) / 1000)
      lastFrameAt = now
      const wasPlaying = transport.state.playing
      frames.emit(transport.advance(doc.project(), seconds))
      // Song mode stops by itself at the end of the last clip.
      if (wasPlaying && !transport.state.playing) emitTransport()
    }, FRAME_MS)
  }

  function stopFrames() {
    if (frameTimer === null || frames.size > 0) return
    clearInterval(frameTimer)
    frameTimer = null
  }

  /** Runs a call the way IPC does: later, and failing with a plain message. */
  function ipc<T>(work: () => T): Promise<T> {
    return new Promise((resolve, reject) => {
      queueMicrotask(() => {
        try {
          resolve(work())
        } catch (error) {
          reject(error instanceof Error ? error : new Error(String(error)))
        }
      })
    })
  }

  function sampleCommands(filePath: string) {
    if (!isAudioPath(filePath)) {
      throw new Error(`"${filePath}" is not an audio file Windfall can read`)
    }
    const samplePath = samplePathFor(filePath)
    const project = doc.project()
    const existing = project.samples.find(
      (sample) =>
        sample.path.kind === samplePath.kind &&
        sample.path.path === samplePath.path
    )
    const name = baseName(filePath)
    return {
      name,
      // A batch cannot pass an id from one command to the next, so the id
      // the sample will get is worked out first, as the shell does: the one
      // it already has, or the next the project hands out.
      sampleId: existing?.id ?? project.nextId,
      addSample: { type: "addSample", name, path: samplePath } as const,
    }
  }

  /**
   * The commands that put a sample on the playlist as an audio clip, as the
   * shell builds them: a playlist track and a mixer track where the place
   * names none, then the clip, as long as the audio lasts at the tempo the
   * project has now. `nextId` is the first id these commands will be given.
   */
  function audioClipCommands(
    sample: number,
    durationSecs: number,
    name: string,
    place: AudioClipPlace,
    nextId: number
  ): Command[] {
    const project = doc.project()
    const commands: Command[] = []
    // Ids are handed out in the order the commands ask for them.
    let next = nextId
    const allocate = () => next++
    let track = place.track
    if (track === undefined) {
      commands.push({ type: "addPlaylistTrack" })
      track = allocate()
    }
    // With no track asked for, the clip joins the clips of the same sample
    // on theirs, as the shell does.
    let mixerTrack = place.mixerTrack ?? mixerTrackOfSample(project, sample)
    if (mixerTrack === undefined) {
      if (project.mixer.tracks.filter((track) => !track.current).length < MAX_MIXER_TRACKS) {
        commands.push({ type: "addMixerTrack", name })
        mixerTrack = allocate()
      } else {
        // A full mixer must not stop the user adding audio.
        mixerTrack = MASTER_TRACK
      }
    }
    const start = Math.max(0, Math.round(place.start))
    const ticks = (durationSecs * project.settings.tempoBpm * PPQ) / 60
    const room = Math.max(1, MAX_SONG_TICKS - start)
    commands.push({
      type: "addClips",
      clips: [
        {
          track,
          start,
          length: Math.min(Math.max(1, Math.ceil(ticks)), room),
          content: {
            type: "audio",
            sample,
            mixerTrack,
            gain: 1,
            pan: 0,
            fadeIn: 0,
            fadeOut: 0,
            reverse: false,
            pitch: 0,
          },
        },
      ],
    })
    return commands
  }

  return {
    kind: "mock",
    ...unavailableAnalysis(),
    mixerWaveformTracks: (tracks, generation, revision) => ipc(() => {
      const state = timelineState()
      if (state.generation !== generation || state.revision !== revision) throw new Error("The project changed before waveform selection could update")
      if (tracks.length > 128 || new Set(tracks).size !== tracks.length || tracks.some((id) => !doc.project().mixer.tracks.some((track) => track.id === id))) throw new Error("Invalid waveform track selection")
      // The browser has no native audio callback. It publishes no waveform PCM.
    }),
    mixerPresetCapture: async (id, generation, revision) => {
      const ticket = timelineState()
      if (generation !== ticket.generation || revision !== ticket.revision) throw new Error("The project changed before preset capture")
      const project = doc.project()
      const track = project.mixer.tracks.find((track) => track.id === id)
      if (!track) throw new Error("Preset source track no longer exists")
      return structuredClone({ version: 1, name: track.name, color: track.color, volume: track.volume, pan: track.pan, muted: track.muted, processing: track.processing ?? DEFAULT_TRACK_PROCESSING, latencyOffsetMs: track.latencyOffsetMs ?? 0, effects: track.effects, plugins: (project.plugins ?? []).filter((plugin) => { const target = plugin.target; return target.type === "effect" && track.effects.some((slot) => slot.id === target.effect) }) })
    },
    mixerPresetSave: async (preset) => {
      if (typeof document === "undefined") throw new Error("Preset downloads require a browser")
      const name = `${preset.name.replace(/[<>:"/\\|?*]/g, "_") || "Mixer track"}.wfmixer`
      const url = URL.createObjectURL(new Blob([JSON.stringify(preset, null, 2)], { type: "application/json" }))
      const anchor = document.createElement("a"); anchor.href = url; anchor.download = name
      document.body.append(anchor); anchor.click(); anchor.remove()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
      return name
    },
    sheetMusicSave: async (suggestedName, xml) => {
      const name = `${suggestedName.replace(/[<>:"/\\|?*]/g, "_") || "Score"}.musicxml`
      const path = await dialogs.exportPath(`/exports/${name}`)
      if (!path) return null
      if (typeof document === "undefined") throw new Error("Score downloads require a browser")
      const url = URL.createObjectURL(new Blob([xml], { type: "application/vnd.recordare.musicxml+xml" }))
      const anchor = document.createElement("a")
      anchor.href = url
      anchor.download = path.split(/[\\/]/).pop() || name
      document.body.append(anchor)
      anchor.click()
      anchor.remove()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
      return path
    },
    mixerPresetLoad: async () => {
      if (typeof document === "undefined") throw new Error("Preset import requires a browser")
      const file = await new Promise<File | null>((resolve) => {
        const input = document.createElement("input"); input.type = "file"; input.accept = ".wfmixer,application/json"; input.hidden = true
        const finish = (file: File | null) => { input.remove(); resolve(file) }
        input.addEventListener("change", () => finish(input.files?.[0] ?? null), { once: true })
        input.addEventListener("cancel", () => finish(null), { once: true })
        document.body.append(input); input.click()
      })
      if (!file) return null
      if (file.size > 256 * 1024 * 1024) throw new Error("Preset file exceeds 256 MiB")
      const preset = JSON.parse(await file.text()) as import("@/bindings").MixerTrackPreset
      if (preset.version !== 1 || !Array.isArray(preset.effects) || !Array.isArray(preset.plugins)) throw new Error("Invalid mixer preset")
      return preset
    },
    currentMixerTarget: (track, generation, revision) => ipc(() => {
      const state = timelineState()
      if (generation !== state.generation || revision !== doc.snapshot(null).revision) throw new Error("Project changed before Current could follow selection.")
      if (track !== null && !doc.project().mixer.tracks.some((item) => item.id === track && !item.current)) throw new Error("Current source no longer exists.")
    }),
    projectSaveNewVersion: async () => {
      throw new Error("Numbered saves require the desktop app.")
    },
    projectArchiveSave: async () => {
      throw new Error("Portable project archives require the desktop app.")
    },
    projectArchiveCancel: async () => {
      throw new Error("Portable project archives require the desktop app.")
    },
    pickProjectArchivePath: async () => {
      throw new Error("Portable project archives require the desktop app.")
    },
    ...createMidiHardwareMock(),
    ...createSlicerMock(
      options,
      () => doc,
      dispatchNow,
      (id) => {
        const asset = doc.project().samples.find((s) => s.id === id)
        if (!asset) throw new Error("The clip's source no longer exists.")
        return sampleInfoFor(roots, sampleFile(asset))
      }
    ),
    ...createMidiMock(options, () => doc, publish, dialogs.exportPath),
    pluginsState: async () => ({
      folders: [],
      entries: [],
      blocked: [],
      scanning: false,
      completed: 0,
      total: 0,
      current: null,
      error: "Native plugins require the Windows desktop app",
      instances: (doc.project().plugins ?? []).map((plugin) => ({
        target: plugin.target,
        error: "Native plugins require the Windows desktop app",
      })),
    }),
    pluginsScan: async () => {
      throw new Error("Native plugins require the Windows desktop app")
    },
    pluginsAddFolder: async () => {
      throw new Error("Native plugins require the Windows desktop app")
    },
    pluginsAdd: async () => {
      throw new Error("Native plugins require the Windows desktop app")
    },
    pluginEditor: async () => {
      throw new Error("Native editors require the Windows desktop app")
    },

    timelineState: () => ipc(timelineState),
    timelineRegion: (region, generation, revision, request, cancel) =>
      ipc(() => {
        const current = timelineState()
        if (generation !== current.generation || revision !== current.revision)
          throw new Error(
            "The project changed before the timeline region was applied."
          )
        sim.call("timeline_range", 0, region)
        request ??= timelineRequest + 1
        if (!Number.isSafeInteger(request) || request <= timelineRequest)
          throw new Error(
            "The timeline request is stale or exceeds the exact request limit."
          )
        if (cancel) {
          if (!Number.isSafeInteger(cancel.request) || cancel.request <= 0)
            throw new Error(
              "The cancelled timeline request exceeds the exact request limit."
            )
          if (
            cancel.generation === current.generation &&
            cancel.revision === current.revision &&
            cancel.request === current.request &&
            cancel.request === timelinePlayRequest &&
            cancel.region &&
            current.region &&
            cancel.region.start === current.region.start &&
            cancel.region.end === current.region.end
          ) {
            transport.stop()
            timelinePlayRequest = 0
            emitTransport()
          }
        }
        transport.setRegion(region, doc.project())
        timelineRequest = request
        return timelineState()
      }),
    documentSnapshot: () => ipc(() => doc.snapshot(path)),
    dispatch: (command, gesture) => ipc(() => dispatchNow(command, gesture)),
    undo: () =>
      ipc(() => {
        const patch = doc.undo()
        return patch ? publish(patch) : null
      }),
    redo: () =>
      ipc(() => {
        const patch = doc.redo()
        return patch ? publish(patch) : null
      }),
    historyJump: (cursor) => ipc(() => publish(doc.jump(cursor))),

    async pickFlpFile() {
      if (dialogs.flpFile) return dialogs.flpFile()
      return new Promise<string | null>((resolve) => {
        const input = document.createElement("input")
        input.type = "file"
        input.accept = ".flp"
        input.addEventListener("cancel", () => resolve(null), { once: true })
        input.addEventListener(
          "change",
          () => {
            const file = input.files?.[0]
            if (!file) {
              resolve(null)
              return
            }
            void file.arrayBuffer().then(
              (buffer) => {
                flpFiles.set(file.name, new Uint8Array(buffer))
                resolve(file.name)
              },
              () => resolve(null)
            )
          },
          { once: true }
        )
        input.click()
      })
    },
    flpPreview: (sourcePath, importOptions) =>
      ipc(() => {
        flpPending?.next.dispose()
        flpPending = null
        const bytes = flpFiles.get(sourcePath)
        if (!bytes)
          throw new Error(
            "Choose an FL project from this browser's file picker first."
          )
        const { project, report } = sim.call<{
          project: Project
          report: ImportReport
        }>("flp_convert", 0, {
          bytes: Array.from(bytes),
          options: {
            projectDir: sourcePath.includes("/")
              ? sourcePath.slice(0, sourcePath.lastIndexOf("/"))
              : undefined,
            fallbackName: baseName(sourcePath).replace(/\.flp$/i, ""),
            pathStyle: "posix",
            factoryDataDir: importOptions.factoryDataDir,
            userDataDir: importOptions.userDataDir,
          },
        })
        const missingSamples = project.samples.flatMap((sample) => {
          try {
            sampleInfoFor(roots, sample.path.path)
            return []
          } catch {
            return [
              { sample: sample.id, name: sample.name, path: sample.path.path },
            ]
          }
        })
        const preview: FlpImportPreview = {
          token: ++flpSequence,
          name: project.settings.name,
          report,
          missingSamples,
          retainedPlugins: project.retainedPlugins?.length ?? 0,
          warnings: missingSamples.map((s) => `Missing sample: ${s.path}`),
        }
        if (missingSamples.length > 0)
          preview.warnings.push(
            "The browser cannot read external sample folders. Open the desktop app to locate and load those samples."
          )
        const next = SimDocument.imported(project)
        flpPending = {
          preview,
          next,
          original: doc,
          originalRevision: doc.snapshot(null).revision,
        }
        return preview
      }),
    flpOpen: (token) =>
      ipc(() => {
        const pending = flpPending
        if (!pending || pending.preview.token !== token)
          throw new Error(
            "The import review expired. Choose the FL project again."
          )
        if (
          pending.original !== doc ||
          pending.originalRevision !== doc.snapshot(null).revision
        )
          throw new Error(
            "The current project changed. Review the FL project again before opening it."
          )
        flpPending = null
        return load(pending.next, null)
      }),
    flpCancel: (token) =>
      ipc(() => {
        if (flpPending?.preview.token === token) {
          flpPending.next.dispose()
          flpPending = null
        }
      }),

    projectNew: () =>
      ipc(() => load(SimDocument.create(starterProject()), null)),
    projectOpen: (openPath) =>
      ipc(() => {
        if (/\.zip$/i.test(openPath))
          throw new Error("Portable project archives require the desktop app.")
        const fileText = files()[openPath]
        if (typeof fileText !== "string") {
          throw new Error(
            `Could not read ${openPath}: no such file is saved in this browser.`
          )
        }
        let opened: SimDocument
        try {
          opened = SimDocument.open(fileText)
        } catch (error) {
          throw sentence(error)
        }
        const snapshot = load(opened, openPath)
        remember(openPath, fileText)
        return snapshot
      }),
    projectSave: (savePath) =>
      ipc(() => {
        const target = savePath ? withProjectExtension(savePath) : path
        if (!target) {
          throw new Error("This project has no file yet. Use Save as.")
        }
        let fileText: string
        try {
          fileText = doc.fileText()
        } catch (error) {
          throw sentence(error)
        }
        remember(target, fileText)
        rememberTransport(target)
        path = target
        // Changes nothing in the project. It carries the new dirty flag.
        publish(doc.markSaved())
        return target
      }),
    recentProjects: () => ipc(() => recent()),

    transportPlay: (guard) =>
      ipc(() => {
        checkTimelineGuard(guard)
        startPlayback()
        timelinePlayRequest = guard?.request ?? 0
        return emitTransport()
      }),
    transportStop: () =>
      ipc(() => {
        timelinePlayRequest = 0
        transport.stop()
        return emitTransport()
      }),
    transportToggle: () =>
      ipc(() => {
        if (transport.state.playing) transport.stop()
        else startPlayback()
        timelinePlayRequest = 0
        return emitTransport()
      }),
    transportSeek: (tick, guard) =>
      ipc(() => {
        checkTimelineGuard(guard)
        if (!guard) timelinePlayRequest = 0
        transport.seek(tick, doc.project())
      }),
    transportSet: (patch, guard) =>
      ipc(() => {
        checkTimelineGuard(guard)
        if (
          patch.pattern !== undefined &&
          !doc
            .project()
            .patterns.some((pattern) => pattern.id === patch.pattern)
        ) {
          throw new Error(`pattern ${patch.pattern} does not exist`)
        }
        if (!guard) timelinePlayRequest = 0
        transport.set(patch, doc.project())
        return emitTransport()
      }),
    transportState: () => ipc(() => transport.state),

    engineStatus: () => ipc(() => currentEngine()),
    processMemory: async () => null,
    // The browser preview keeps no audio history. It says so; it never
    // pretends to log or to add a clip.
    audioLoggerState: async () => unavailableLogger(),
    audioLoggerConfigure: () => Promise.reject(new Error(NATIVE_ONLY)),
    audioLoggerDump: () => Promise.reject(new Error(NATIVE_ONLY)),
    recordingInputs: async () => [],
    inputMonitorState: async () => ({ active: false, sampleRate: 0, tracks: [], error: null }),
    inputMonitorStop: async () => ({ active: false, sampleRate: 0, tracks: [], error: null }),
    inputMonitorStart: async () => { throw new Error("Input monitoring requires the native desktop app.") },
    recordingState: async () => ({
      active: false,
      frames: 0,
      sampleRate: 0,
      startTick: 0,
      error: null,
    }),
    recordingStart: async () => {
      throw new Error("Audio recording requires the native desktop app.")
    },
    recordingStop: async () => {
      throw new Error("No recording is active.")
    },
    recordingCancel: async () => {},
    engineDevices: () => ipc(() => HOSTS),
    engineConfigure: (settings) =>
      ipc(() => {
        audioSettings = storedSettings(settings)
        engine = describeStatus(audioSettings)
        engineStatuses.emit(currentEngine())
        return currentEngine()
      }),
    engineSettings: () => ipc(() => audioSettings),

    auditionNoteOn: (channel, key, velocity) =>
      ipc(() => {
        const source = doc
          .project()
          .channels.find((item) => item.id === channel)?.source
        if (source?.type === "sampler" && source.stretch?.mode === "spectral")
          throw new Error(
            "Spectral sampler audition requires the desktop audio engine."
          )
        transport.noteOn(channel, key, velocity)
      }),
    auditionNoteOff: (channel, key) =>
      ipc(() => transport.noteOff(channel, key)),
    previewPlay: (filePath, browser) =>
      ipc(() => {
        library.check(filePath, browser)
        sampleInfoFor(roots, filePath)
      }),
    previewStop: () => ipc(() => undefined),

    browserRoots: () => ipc(() => roots),
    librarySearch: (search) => ipc(() => library.search(search)),
    libraryRefresh: () => ipc(() => library.refresh()),
    libraryCancel: (generation) => ipc(() => library.cancel(generation)),
    libraryFile: (path) => ipc(() => library.file(path)),
    libraryMetadata: (path) => ipc(() => library.metadata(path)),
    librarySetMetadata: (path, metadata) =>
      ipc(() => library.setMetadata(path, metadata)),
    browserAddRoot: (rootPath) =>
      ipc(() => {
        const clean = rootPath.replace(/[\\/]+$/, "")
        if (!clean) throw new Error("Choose a folder to add.")
        if (roots.length >= 128 || clean.length > 4096)
          throw new Error(
            "The browser supports at most 128 folders and paths up to 4096 bytes."
          )
        if (roots.some((root) => root.path === clean)) {
          throw new Error(`"${clean}" is already in the browser.`)
        }
        const name = clean.split(/[\\/]/).pop() ?? clean
        const next: BrowserRoot[] = [
          ...roots,
          { name, path: clean, kind: "user" },
        ]
        library.changedRoots(next)
        roots = next
        return roots
      }),
    browserRemoveRoot: (rootPath) =>
      ipc(() => {
        const root = roots.find((item) => item.path === rootPath)
        if (!root) throw new Error(`"${rootPath}" is not in the browser.`)
        if (root.kind === "factory") {
          throw new Error("The factory library cannot be removed.")
        }
        const next = roots.filter((item) => item.path !== rootPath)
        library.changedRoots(next)
        roots = next
        return roots
      }),
    browserList: (folderPath) => ipc(() => listFolder(roots, folderPath)),
    sampleInfo: (filePath, browser) =>
      ipc(() => {
        library.check(filePath, browser)
        return sampleInfoFor(roots, filePath)
      }),
    prepareClipCommand: (command) => ipc(() => dispatchNow(command)),
    samplerPreparationBegin: () =>
      Promise.reject(
        new Error(
          "Spectral sampler preparation needs the desktop audio engine. Browser mode cannot prepare or audition key variants."
        )
      ),
    samplerPreparationCancel: () => Promise.resolve(),
    samplerPreparationProgress: (request) =>
      Promise.resolve({ request, completed: 0, total: 0, current: false }),
    prepareSamplerCommand: () =>
      Promise.reject(
        new Error(
          "Spectral sampler preparation needs the desktop audio engine. Settings and history were kept."
        )
      ),
    audioEditorOpen: () =>
      Promise.reject(
        new Error(
          "Audio editing requires the desktop app. This browser preview cannot decode clips or create derived WAV files."
        )
      ),
    audioEditorApply: () =>
      Promise.reject(
        new Error(
          "Audio editing requires the desktop app. No audio or project changes were made."
        )
      ),
    audioEditorDiscard: () => Promise.resolve(),
    detectClipTempo: () =>
      Promise.reject(
        new Error(
          "Tempo detection requires the desktop app. Enter a source BPM or beat count."
        )
      ),
    sampleInfoById: (sample) =>
      ipc(() => {
        const asset = doc.project().samples.find((item) => item.id === sample)
        if (!asset) throw new Error(`sample ${sample} does not exist`)
        return sampleInfoFor(roots, sampleFile(asset))
      }),

    addChannelFromFile: (filePath, index, browser) =>
      ipc(() => {
        library.check(filePath, browser)
        const { name, sampleId, addSample } = sampleCommands(filePath)
        return dispatchNow({
          type: "batch",
          label: "Add channel",
          commands: [
            addSample,
            { type: "addChannel", name, sample: sampleId, index },
          ],
        })
      }),
    setChannelSampleFromFile: (channel, filePath, browser) =>
      ipc(() => {
        library.check(filePath, browser)
        const { sampleId, addSample } = sampleCommands(filePath)
        return dispatchNow({
          type: "batch",
          label: "Change channel sample",
          commands: [
            addSample,
            { type: "setChannelSample", id: channel, sample: sampleId },
          ],
        })
      }),

    addAudioClipFromFile: (filePath, place, browser) =>
      ipc(() => {
        library.check(filePath, browser)
        const { name, sampleId, addSample } = sampleCommands(filePath)
        const info = sampleInfoFor(roots, filePath)
        const project = doc.project()
        // The sample takes an id only when the project does not have it.
        const nextId =
          sampleId === project.nextId ? project.nextId + 1 : project.nextId
        return dispatchNow({
          type: "batch",
          label: "Add audio clip",
          commands: [
            addSample,
            ...audioClipCommands(
              sampleId,
              info.durationSecs,
              name,
              place,
              nextId
            ),
          ],
        })
      }),
    addAudioClipFromSample: (sample, place) =>
      ipc(() => {
        const project = doc.project()
        const asset = project.samples.find((item) => item.id === sample)
        if (!asset) throw new Error(`sample ${sample} does not exist`)
        let durationSecs: number
        try {
          durationSecs = sampleInfoFor(roots, sampleFile(asset)).durationSecs
        } catch {
          throw new Error(
            `The audio of "${asset.name}" is not loaded, so a clip of it cannot be made. Check that its file is there, then reload the samples.`
          )
        }
        return dispatchNow({
          type: "batch",
          label: "Add audio clip",
          commands: audioClipCommands(
            sample,
            durationSecs,
            asset.name,
            place,
            project.nextId
          ),
        })
      }),
    async bounceSelectedClips(selection) {
      const { document, snapshot } = await ipc(() => ({
        document: doc,
        snapshot: doc.snapshot(path),
      }))
      const ids = new Set(selection)
      const clips = snapshot.project.playlist.clips.filter((clip) =>
        ids.has(clip.id)
      )
      if (ids.size === 0)
        throw new Error("Select at least one playlist clip to bounce.")
      if (clips.length !== ids.size)
        throw new Error("A selected clip no longer exists.")
      const tracks = new Set(clips.map((clip) => clip.track))
      const destination = snapshot.project.playlist.tracks.find((track) =>
        tracks.has(track.id)
      )
      if (!destination)
        throw new Error("A selected playlist track no longer exists.")
      const region = {
        start: Math.min(...clips.map((clip) => clip.start)),
        end: Math.max(...clips.map((clip) => clip.start + clip.length)),
      }
      if (!options.renderPlaylistBounce)
        throw new Error(
          "Bouncing requires the desktop app's offline renderer. No project changes were made."
        )
      const source = structuredClone(snapshot.project)
      for (const track of source.playlist.tracks) {
        track.solo = tracks.has(track.id)
        if (track.solo) track.muted = false
      }
      const renderedPath = await options.renderPlaylistBounce(source, region)
      return ipc(() => {
        if (
          doc !== document ||
          doc.snapshot(path).revision !== snapshot.revision
        )
          throw new Error(
            "The project changed while bouncing. Select the clips and try again."
          )
        const info = sampleInfoFor(roots, renderedPath)
        const { sampleId, addSample } = sampleCommands(renderedPath)
        const commands = audioClipCommands(
          sampleId,
          info.durationSecs,
          "Bounce",
          {
            track: destination.id,
            start: region.start,
            mixerTrack: MASTER_TRACK,
          },
          snapshot.project.nextId + 1
        )
        for (const command of commands) {
          if (command.type === "addClips") {
            for (const clip of command.clips) {
              clip.length = region.end - region.start
              clip.muted = false
            }
          }
        }
        return dispatchNow({
          type: "batch",
          label: "Bounce selected clips",
          commands: [
            addSample,
            ...commands,
            {
              type: "updateClips",
              updates: [...ids].map((id) => ({ id, patch: { muted: true } })),
            },
          ],
        })
      })
    },
    automate: (target) =>
      ipc(() => {
        const project = doc.project()
        const song = project.playlist.clips.reduce(
          (end, clip) => Math.max(end, clip.start + clip.length),
          0
        )
        const bars = ticksPerBar(project.settings.timeSignature) * 4
        // A batch cannot pass an id from one command to the next, so the
        // ids the automation and the track will get are worked out first.
        const automation = project.nextId
        const track = project.nextId + 1
        return dispatchNow({
          type: "batch",
          label: "Create automation clip",
          commands: [
            { type: "addAutomation", target },
            { type: "addPlaylistTrack" },
            {
              type: "addClips",
              clips: [
                {
                  track,
                  start: 0,
                  length: Math.min(Math.max(song, bars), MAX_SONG_TICKS),
                  content: { type: "automation", automation },
                },
              ],
            },
          ],
        })
      }),

    samplesReload: () =>
      ipc(() => {
        const missing = missingSamples()
        warnings.emit(missing)
        return missing.length
      }),

    exportAudio: (exportOptions) =>
      ipc(() => {
        if (exportOptions.region) {
          if (exportOptions.mode !== "song")
            throw new Error("A timeline export region requires song mode.")
          sim.call("timeline_range", 0, exportOptions.region)
        }
        if (
          exportOptions.regionGeneration !== undefined ||
          exportOptions.regionRevision !== undefined
        ) {
          const current = timelineState()
          if (
            exportOptions.regionGeneration !== current.generation ||
            exportOptions.regionRevision !== current.revision
          )
            throw new Error(
              "The project changed before the selected region could be exported."
            )
        }
        if (
          doc
            .project()
            .channels.some(
              (channel) =>
                channel.source.type === "sampler" &&
                channel.source.stretch?.mode === "spectral"
            )
        )
          throw new Error(
            "Spectral sampler export requires the desktop audio engine. Browser mode cannot render key variants."
          )
        if (exporting) throw new Error("An export is already running.")
        if (!exportOptions.path)
          throw new Error("Choose where to save the file.")
        if (exportOptions.patternLoops < 1) {
          throw new Error("Render the pattern at least once.")
        }
        const timer = setInterval(() => {
          if (!exporting) return
          const fraction = Math.min(1, exporting.fraction + 0.06)
          exporting.fraction = fraction
          const done = fraction >= 1
          if (done) {
            clearInterval(timer)
            exporting = null
          }
          exportProgress.emit({
            path: exportOptions.path,
            fraction,
            done,
            error: null,
            droppedClips: 0,
          })
        }, 60)
        exporting = { timer, path: exportOptions.path, fraction: 0 }
      }),

    exportCancel: () =>
      ipc(() => {
        if (!exporting) return
        const current = exporting
        clearInterval(current.timer)
        exporting = null
        exportProgress.emit({
          path: current.path,
          fraction: current.fraction,
          done: true,
          error: null,
          droppedClips: 0,
          cancelled: true,
        })
      }),

    onProjectPatch: (handler) => patches.on(handler),
    detachCalls,
    async detachPanel(id) {
      if (!isPanelId(id)) throw new Error(`Unknown panel: ${id}`)
      detachCalls.push(id)
    },
    async dockPanel(id) {
      if (!isPanelId(id)) throw new Error(`Unknown panel: ${id}`)
      panelDocked.emit(id)
    },
    onPanelDocked: (listener) => panelDocked.on(listener),
    onProjectLoaded: (handler) => loaded.on(handler),
    onTransportState: (handler) => transportStates.on(handler),
    onEngineStatus: (handler) => engineStatuses.on(handler),
    onExportProgress: (handler) => exportProgress.on(handler),
    onProjectWarnings: (handler) => warnings.on(handler),

    subscribeRealtime(handler) {
      const off = frames.on(handler)
      startFrames()
      return () => {
        off()
        stopFrames()
      }
    },

    pickProjectToOpen: () => dialogs.openProject(recent()),
    pickProjectSavePath: (suggestedName) =>
      dialogs.saveProject(`/projects/${suggestedName}.windfall`),
    pickExportPath: (suggestedName, format = "wav") =>
      dialogs.exportPath(`/exports/${suggestedName}.${format}`),
    pickFolder: () => dialogs.folder(),
    pickAudioFile: () => dialogs.audioFile(),

    setWindowTitle: (title) =>
      ipc(() => {
        if (typeof window !== "undefined") window.document.title = title
      }),

    // A browser tab cannot wait for an answer before it closes, so the mock
    // falls back to the browser's own "leave site?" prompt.
    onCloseRequested() {
      const handler = (event: BeforeUnloadEvent) => {
        if (doc.isDirty()) event.preventDefault()
      }
      window.addEventListener("beforeunload", handler)
      return () => window.removeEventListener("beforeunload", handler)
    },

    dispose() {
      if (exporting) clearInterval(exporting.timer)
      exporting = null
      if (frameTimer !== null) clearInterval(frameTimer)
      frameTimer = null
      flpPending?.next.dispose()
      flpPending = null
      doc.dispose()
    },
  }
}
