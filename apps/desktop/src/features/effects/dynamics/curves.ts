/*
 * What the dynamics editors draw and say, worked out the way the processors
 * in `crates/windfall-dsp/src/multiband` and `mastering` do it.
 */

export const DISPLAY_MIN_HZ = 20
export const DISPLAY_MAX_HZ = 20_000
/** The quietest level a band display shows, in dB. The loudest is 0. */
export const DISPLAY_FLOOR_DB = -60

export const BANDS = [
  { key: "low", title: "Low" },
  { key: "mid", title: "Mid" },
  { key: "high", title: "High" },
] as const
export type BandKey = (typeof BANDS)[number]["key"]

/**
 * The two crossover frequencies in effect, as `Crossover::set` constrains
 * them: the lower edge at most 0.35 of the sample rate, and the upper edge
 * at least a quarter above it and at most 0.45 of the sample rate.
 */
export function effectiveCrossovers(
  lowHz: number,
  highHz: number,
  sampleRate: number
): [number, number] {
  const low = Math.min(Math.max(lowHz, 1), sampleRate * 0.35)
  const high = Math.min(Math.max(highHz, low * 1.25, 1.25), sampleRate * 0.45)
  return [low, high]
}

/** A frequency as its share of a logarithmic axis, 0 to 1. */
export function frequencyPosition(hz: number): number {
  const clamped = Math.min(Math.max(hz, DISPLAY_MIN_HZ), DISPLAY_MAX_HZ)
  return (
    Math.log(clamped / DISPLAY_MIN_HZ) /
    Math.log(DISPLAY_MAX_HZ / DISPLAY_MIN_HZ)
  )
}

/** A level as its share of the display's height from the floor up, 0 to 1. */
export function levelPosition(db: number): number {
  return Math.min(1, Math.max(0, 1 - db / DISPLAY_FLOOR_DB))
}

/**
 * Reduction in dB, zero or more, of one band for a steady level in that
 * band: a hard knee, and a limiter from a ratio of 100.
 */
export function bandReductionDb(
  levelDb: number,
  thresholdDb: number,
  ratio: number
): number {
  const slope = ratio >= 100 ? 1 : 1 - 1 / Math.max(1, ratio)
  return Math.max(0, levelDb - thresholdDb) * slope
}

/** The settings the one-knob enhancer gives its maximizer for an amount. */
export function oneKnobRecipe(amount: number) {
  const share = Math.min(1, Math.max(0, amount))
  const band = (thresholdDb: number, ratio: number, makeupDb: number) => ({
    thresholdDb: thresholdDb * share,
    ratio: 1 + (ratio - 1) * share,
    makeupDb: makeupDb * share,
  })
  return {
    inputGainDb: 8 * share,
    ceilingDb: -0.3 * share,
    low: band(-24, 3, 3),
    mid: band(-18, 2.5, 2),
    high: band(-22, 2, 4),
  }
}

/** The most a transient shaper boosts or cuts either part, in dB. */
export const TRANSIENT_RANGE_DB = 18

/**
 * A hit as the transient shaper hears it, before and after: a level
 * envelope over `points` steps that rises at once and falls away, and the
 * same with the attack and the sustain turned up or down. The share of a
 * step that counts as attack falls from one to nothing over the first
 * fifth of the hit.
 */
export function transientShape(
  attack: number,
  sustain: number,
  points: number
): { dry: number[]; shaped: number[] } {
  const dry: number[] = []
  const shaped: number[] = []
  for (let index = 0; index < points; index += 1) {
    const time = index / Math.max(1, points - 1)
    const level = Math.exp(-3 * time)
    const share = Math.max(0, 1 - time * 5)
    const gainDb =
      TRANSIENT_RANGE_DB * (attack * share + sustain * (1 - share))
    dry.push(level)
    shaped.push(level * 10 ** (gainDb / 20))
  }
  return { dry, shaped }
}

/**
 * The frames a Mastering Limiter delays its track by: 1.5 ms of look-ahead
 * and the six frames its intersample detector needs. This is
 * `master_limiter_latency` in `mastering/master_limiter.rs`.
 */
export function masterLimiterLatencyFrames(sampleRate: number): number {
  const rate = Math.min(384_000, Math.max(1, sampleRate))
  return Math.max(1, Math.round((1.5 * rate) / 1000)) + 6
}

/** Frames of the maximizer's two fixed 1 ms look-ahead stages. */
export function maximizerLatencyFrames(sampleRate: number): number {
  const rate = Math.min(384_000, Math.max(8, sampleRate))
  return 2 * Math.max(1, Math.round(rate / 1000))
}

/** Frames the bass harmonics' oversampled path delays the track by. */
export const HARMONICS_LATENCY_FRAMES = 13

/**
 * Reduction of the Mastering Limiter's level stage in dB for an average
 * level in dB RMS: 0.6 of every dB over -16 at full leveling, through a
 * 6 dB knee. This is `level_reduction_db`.
 */
export function levelStageReductionDb(rmsDb: number, leveling: number): number {
  const slope = 0.6 * Math.min(1, Math.max(0, leveling))
  const over = rmsDb + 16
  const knee = 6
  if (2 * over <= -knee) return 0
  if (2 * over < knee) {
    const into = over + knee / 2
    return (slope * into * into) / (2 * knee)
  }
  return slope * over
}
