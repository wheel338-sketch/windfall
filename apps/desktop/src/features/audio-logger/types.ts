/**
 * The audio logger's IPC contract. It mirrors
 * `crates/windfall-ipc/src/audio_logger.rs` field for field, as the audio
 * editor's contract mirrors its own, so the feature compiles against the
 * shape the generated bindings will have.
 */
export type AudioLoggerInput = {
  host: string
  device: string
  left: number
  right: number | null
}

export type AudioLoggerSettings = {
  enabled: boolean
  /** History length in seconds. */
  seconds: number
  master: boolean
  /** Mixer track ids logged after their effects. */
  tracks: number[]
  input: AudioLoggerInput | null
}

export type AudioLoggerSource =
  | { type: "master" }
  | { type: "track"; track: number }
  | { type: "input" }

export type AudioLoggerSourceStatus = {
  source: AudioLoggerSource
  label: string
  sampleRate: number
  capacitySeconds: number
  availableSeconds: number
  availableFrames: number
  droppedFrames: number
  memoryBytes: number
  active: boolean
  error: string | null
}

export type AudioLoggerState = {
  settings: AudioLoggerSettings
  running: boolean
  sources: AudioLoggerSourceStatus[]
  memoryBytes: number
  maxSourceBytes: number
  minSeconds: number
  maxSeconds: number
  maxTracks: number
  error: string | null
}

export type AudioLoggerDumpRequest = {
  source: AudioLoggerSource
  /** Trailing seconds to keep; null keeps everything the history holds. */
  seconds: number | null
  /** Tick the clip starts on; null uses the playhead. */
  start: number | null
  /** Playlist track; null adds one. */
  track: number | null
}

/** What the browser preview reports: it has no audio history to keep. */
export const NATIVE_ONLY =
  "The audio logger requires the desktop app. This browser preview keeps no audio history."

/** The logger as a backend without one describes it. */
export function unavailableLogger(): AudioLoggerState {
  return {
    settings: {
      enabled: false,
      seconds: 120,
      master: true,
      tracks: [],
      input: null,
    },
    running: false,
    sources: [],
    memoryBytes: 0,
    maxSourceBytes: 0,
    minSeconds: 30,
    maxSeconds: 600,
    maxTracks: 4,
    error: NATIVE_ONLY,
  }
}

export function sameSource(a: AudioLoggerSource, b: AudioLoggerSource) {
  if (a.type !== b.type) return false
  return a.type !== "track" || (b.type === "track" && a.track === b.track)
}

export function sourceKey(source: AudioLoggerSource): string {
  return source.type === "track" ? `track-${source.track}` : source.type
}
