import type { Selection } from "./selection"
import type {
  AudioEditChannels,
  AudioEditOperation,
  AudioFadeDirection,
  AudioFadeShape,
  AudioNormalizeMode,
  AudioStretchQuality,
} from "./types"

/**
 * The ranges the native editor accepts, as `audio_edit/clean.rs` and
 * `audio_edit/process.rs` check them. A control that is outside its range
 * builds no operation, so its button is disabled instead of sending a
 * request that is known to be refused.
 */
export const LIMITS = {
  declickPercent: { min: 0, max: 100 },
  reductionDb: { min: 0.1, max: 48 },
  thresholdDb: { min: -120, max: -6 },
  gainDb: { min: -96, max: 96 },
  targetDb: { min: -60, max: 0 },
  stretchPercent: { min: 25, max: 400 },
  semitones: { min: -24, max: 24 },
} as const

function within(value: number, range: { min: number; max: number }) {
  return Number.isFinite(value) && value >= range.min && value <= range.max
}

/** Strength is shown as a percentage and sent as 0 to 1. */
export function declick(percent: number): AudioEditOperation | null {
  if (!within(percent, LIMITS.declickPercent)) return null
  return { declick: { strength: percent / 100 } }
}

/** `profile` is a noise-only range captured earlier, or null for automatic. */
export function reduceNoise(
  reductionDb: number,
  profile: Selection | null
): AudioEditOperation | null {
  if (!within(reductionDb, LIMITS.reductionDb)) return null
  return {
    reduceNoise: {
      reductionDb,
      profile: profile
        ? { startFrame: profile.start, endFrame: profile.end }
        : null,
    },
  }
}

export function trimSilence(thresholdDb: number): AudioEditOperation | null {
  if (!within(thresholdDb, LIMITS.thresholdDb)) return null
  return { trimSilence: { thresholdDb } }
}

/** A gain of zero changes nothing and is not an edit. */
export function gain(db: number): AudioEditOperation | null {
  if (!within(db, LIMITS.gainDb) || db === 0) return null
  return { gain: { db } }
}

export function normalizeTo(
  targetDb: number,
  mode: AudioNormalizeMode
): AudioEditOperation | null {
  if (!within(targetDb, LIMITS.targetDb)) return null
  return { normalizeTo: { targetDb, mode } }
}

export function fade(
  direction: AudioFadeDirection,
  shape: AudioFadeShape
): AudioEditOperation {
  return { fade: { direction, shape } }
}

export function invertPhase(channels: AudioEditChannels): AudioEditOperation {
  return { invertPhase: { channels } }
}

/**
 * Length is shown as a percentage of the selection's duration. A stretch
 * that changes neither length nor pitch is not an edit.
 */
export function stretch(
  percent: number,
  semitones: number,
  quality: AudioStretchQuality = "standard",
  formants = false
): AudioEditOperation | null {
  if (
    !within(percent, LIMITS.stretchPercent) ||
    !within(semitones, LIMITS.semitones) ||
    (percent === 100 && semitones === 0)
  )
    return null
  return {
    stretch: {
      ratio: percent / 100,
      semitones,
      quality,
      formants,
    },
  }
}

/** Silence is asked for in milliseconds and inserted in whole frames. */
export function insertSilence(
  milliseconds: number,
  sampleRate: number
): AudioEditOperation | null {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return null
  const frames = Math.round((milliseconds / 1000) * sampleRate)
  if (frames < 1) return null
  return { insertSilence: { frames } }
}

/** What an operation is called in the request, for keys and test names. */
export function operationName(operation: AudioEditOperation): string {
  return typeof operation === "string" ? operation : Object.keys(operation)[0]
}
