import type { EffectId, TrackId } from "@/bindings"

/*
 * The live analyzer protocol, as `crates/windfall-ipc/src/analyzer.rs` sends
 * it. Tickets, sequences and frame positions are native 64-bit counts and
 * stay decimal strings. A value that could not be measured is `null`, never
 * a zero that would read as a healthy measurement.
 */

export type AnalyzerSource =
  | { type: "mixOutput" }
  | { type: "trackPostFader"; track: TrackId }

export type AnalyzerWant = {
  scope: boolean
  spectrum: boolean
  vector: boolean
  envelope: boolean
}

export type AnalyzerSelect = {
  slot: string
  request: number
  generation: number
  source: AnalyzerSource
  effect?: EffectId
  want: AnalyzerWant
}

export type AnalyzerPhase =
  | "idle"
  | "preparing"
  | "awaitingAudio"
  | "live"
  | "waitingForStream"
  | "stalled"
  | "unavailable"
  | "refused"

export type AnalyzerReason =
  | "desktopRequired"
  | "notSubscribed"
  | "invalidSlot"
  | "subscriptionCapacity"
  | "sourceCapacity"
  | "staleProject"
  | "sourceMissing"
  | "effectMissing"
  | "effectNotOnSource"
  | "rateUnsupported"
  | "latencyUnsupported"
  | "preparation"
  | "exhausted"
  | "audioStalled"
  | "viewStalled"
  | "projectReplaced"

export type AnalyzerTicket = {
  service: string
  subscription: string
  stream: string
  selection: string
  project: string
}

export type AnalyzerStatus = {
  slot: string
  request: number
  phase: AnalyzerPhase
  reason: AnalyzerReason | null
  ticket: AnalyzerTicket | null
  source: AnalyzerSource | null
  effect: EffectId | null
  sourceName: string | null
  sampleRate: number | null
}

export type AnalyzerCapability = {
  native: boolean
  reason: AnalyzerReason | null
  captures: number
  subscriptions: number
  fftSize: number
  hopSize: number
  bins: number
  bands: number
  window: string
  scopeFrames: number
  vectorPoints: number
  envelopeBins: number
  historyRows: number
  observations: number
  maxEventBytes: number
  maxEventsPerSecond: number
  sampleRate: number | null
  minSampleRate: number
  maxSampleRate: number
}

export type AnalyzerClock = {
  epoch: string
  firstFrame: string
  endFrame: string
  sampleRate: number
  pdcFrames: number
  alignedFirstFrame: string
  timeLabel: string
  deviceLatencyFrames: number | null
}

export type AnalyzerLevel = {
  valid: boolean
  peak: number | null
  rms: number | null
  peakDb: number | null
  rmsDb: number | null
  silent: boolean
  clipped: number
  invalid: number
}

export type AnalyzerStereo = {
  correlation: number | null
  midRms: number | null
  sideRms: number | null
}

export type AnalyzerSpectrum = {
  binHz: number
  enbwBins: number
  left: number[]
  right: number[]
  bands: number[]
}

export type AnalyzerRow = {
  firstFrame: string
  endFrame: string
  bands: number[]
}

export type AnalyzerScope = {
  firstFrame: string
  left: (number | null)[]
  right: (number | null)[]
}

export type AnalyzerEnvelope = {
  firstFrame: string
  endFrame: string
  bins: [number | null, number | null, number | null, number | null][]
}

export type AnalyzerObservation =
  | {
      type: "dynamics"
      firstFrame: string
      frames: number
      inputPeak: number
      outputPeak: number
      reductionDb: number
      gateDb: number
    }
  | {
      type: "tuner"
      firstFrame: string
      frames: number
      frequencyHz: number | null
      midiNote: number | null
      cents: number | null
      confidence: number
    }

export type AnalyzerCounters = {
  droppedFrames: string
  invalidSamples: string
  resets: string
  analyzedWindows: string
  droppedWindows: string
  droppedObservations: string
  missingSourceBlocks: string
  skippedEvents: string
}

export type AnalyzerFrame = {
  slot: string
  request: number
  ticket: AnalyzerTicket
  sequence: string
  clock: AnalyzerClock
  gap: boolean
  levels: [AnalyzerLevel, AnalyzerLevel]
  stereo: AnalyzerStereo
  spectrum: AnalyzerSpectrum | null
  history: AnalyzerRow[]
  scope: AnalyzerScope | null
  vector: [number, number][] | null
  envelope: AnalyzerEnvelope | null
  reductionDb: number | null
  observations: AnalyzerObservation[]
  counters: AnalyzerCounters
  playing: boolean
  tick: number
}

export type AnalyzerEvent =
  | ({ type: "status" } & AnalyzerStatus)
  | ({ type: "frame" } & AnalyzerFrame)

/** The analyzer calls of the backend. */
export interface AnalyzerBackend {
  analyzerCapability(): Promise<AnalyzerCapability>
  /**
   * Starts a slot. Events for it arrive at `handler` until the returned
   * function is called. The promise is the slot's first standing.
   */
  analyzerSubscribe(
    slot: string,
    handler: (event: AnalyzerEvent) => void
  ): { status: Promise<AnalyzerStatus>; stop(): void }
  analyzerSelect(select: AnalyzerSelect): Promise<AnalyzerStatus>
  /** Lets the next frame be sent. Never rejects. */
  analyzerAck(slot: string, sequence: string): void
}

export const ANALYZER_DESKTOP_REASON =
  "Audio analysis needs the desktop app's audio engine. The browser plays no audio to analyze."

function unavailableStatus(slot: string, request: number): AnalyzerStatus {
  return {
    slot,
    request,
    phase: "unavailable",
    reason: "desktopRequired",
    ticket: null,
    source: null,
    effect: null,
    sourceName: null,
    sampleRate: null,
  }
}

/**
 * The browser's analyzer: it says it cannot analyze, and sends nothing. No
 * frame is ever made up for a view to draw.
 */
export function unavailableAnalyzer(): AnalyzerBackend {
  return {
    analyzerCapability: () =>
      Promise.resolve({
        native: false,
        reason: "desktopRequired",
        captures: 0,
        subscriptions: 0,
        fftSize: 0,
        hopSize: 0,
        bins: 0,
        bands: 0,
        window: "",
        scopeFrames: 0,
        vectorPoints: 0,
        envelopeBins: 0,
        historyRows: 0,
        observations: 0,
        maxEventBytes: 0,
        maxEventsPerSecond: 0,
        sampleRate: null,
        minSampleRate: 0,
        maxSampleRate: 0,
      }),
    analyzerSubscribe: (slot) => ({
      status: Promise.resolve(unavailableStatus(slot, 0)),
      stop: () => {},
    }),
    analyzerSelect: (select) =>
      Promise.resolve(unavailableStatus(select.slot, select.request)),
    analyzerAck: () => {},
  }
}
