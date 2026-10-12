/**
 * The audio editor's IPC contract. It mirrors
 * `crates/windfall-ipc/src/audio_edit.rs`: operations without parameters
 * travel as their name, the others as an object with one key.
 */
export type AudioEditPreview = {
  token: number
  clip: number
  name: string
  frames: number
  sampleRate: number
  /** Stereo min/max envelope. */
  peaks: number[]
  spectral?: number[]
  regions?: AudioRegion[]
}
export type AudioRegion = { name: string; startFrame: number; endFrame: number }

/** A frame range of the clip view: [startFrame, endFrame). */
export type AudioEditRange = {
  startFrame: number
  endFrame: number
}

export type AudioNormalizeMode = "peak" | "rms"
export type AudioFadeDirection = "in" | "out"
export type AudioFadeShape = "equalPower" | "linear" | "sCurve"
export type AudioEditChannels = "both" | "left" | "right"
export type AudioStretchQuality = "fast" | "standard" | "high"

export type AudioEditOperation =
  | "trim"
  | "extract"
  | "normalize"
  | "reverse"
  | "fadeIn"
  | "fadeOut"
  | "silence"
  | "cut"
  | "removeDc"
  | "swapChannels"
  | "monoSum"
  | { declick: { strength: number } }
  | { reduceNoise: { reductionDb: number; profile: AudioEditRange | null } }
  | { trimSilence: { thresholdDb: number } }
  | { gain: { db: number } }
  | { normalizeTo: { targetDb: number; mode: AudioNormalizeMode } }
  | { fade: { direction: AudioFadeDirection; shape: AudioFadeShape } }
  | { invertPhase: { channels: AudioEditChannels } }
  | {
      stretch: {
        ratio: number
        semitones: number
        quality: AudioStretchQuality
        formants: boolean
      }
    }
  | { insertSilence: { frames: number } }
  | { equalize: { lowDb: number; midDb: number; highDb: number } }
  | { blur: { amount: number } }
  | { convolve: { impulse: AudioEditRange; wet: number } }
  | { repairClipping: { threshold: number } }
  | { loopTune: { crossfadeFrames: number } }
  | { stripSilence: { thresholdDb: number; minSilenceFrames: number } }
  | "script"

export type AudioEditRequest = {
  token: number
  operation: AudioEditOperation
  /** Inclusive first frame and exclusive end frame of the clip view. */
  startFrame: number
  endFrame: number
}
