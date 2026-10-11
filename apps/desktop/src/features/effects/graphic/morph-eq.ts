import type { MorphBandParams, MorphEqParams } from "@/bindings"

import { IDENTITY, peak, type Section } from "../response/curve"

/*
 * The morphing graphic equalizer, as the engine builds it
 * (`crates/windfall-dsp/src/eqbank/morph.rs` and `graphic.rs`): fifteen
 * fixed bells whose gains move in straight lines of dB along the stored
 * curves, then four movable bells that glide from one snapshot to the other.
 */

export const MORPH_CURVE_HZ = [
  25, 40, 63, 100, 160, 250, 400, 630, 1_000, 1_600, 2_500, 4_000, 6_300,
  10_000, 16_000,
] as const
export const MORPH_CURVE_Q = 2.1
export const MORPH_CURVE_NAMES = ["A", "B", "C", "D"] as const

/** What each band is called on its fader: "25", "1k", "16k". */
export const MORPH_CURVE_LABELS = MORPH_CURVE_HZ.map((hz) =>
  hz >= 1_000 ? `${hz / 1_000}k` : String(hz)
)

/** The id of one band's gain in one stored curve. */
export function curveGainId(curve: number, band: number): string {
  return `curves.${curve}.gainsDb.${band}`
}

/**
 * The band gains at `morph` (0 to 1) along the first `count` curves. A
 * position on a curve gives that curve exactly.
 */
export function interpolatedCurve(
  curves: readonly (readonly number[])[],
  count: number,
  morph: number
): number[] {
  const used = Math.min(curves.length, Math.max(2, Math.round(count)))
  const position = Math.min(1, Math.max(0, morph)) * (used - 1)
  const from = Math.min(used - 2, Math.floor(position))
  const share = position - from
  const [a, b] = [curves[from], curves[from + 1]]
  if (share <= 0) return [...a]
  if (share >= 1) return [...b]
  return a.map((gain, band) => gain + share * (b[band] - gain))
}

/** Where the morph is among the curves in use: "Curve B", "40% from A to B". */
export function describeMorph(morph: number, count: number): string {
  const used = Math.min(MORPH_CURVE_NAMES.length, Math.max(2, Math.round(count)))
  const position = Math.min(1, Math.max(0, morph)) * (used - 1)
  const from = Math.min(used - 2, Math.floor(position))
  const share = position - from
  if (share <= 0) return `Curve ${MORPH_CURVE_NAMES[from]}`
  if (share >= 1) return `Curve ${MORPH_CURVE_NAMES[from + 1]}`
  return `${Math.round(share * 100)}% from ${MORPH_CURVE_NAMES[from]} to ${MORPH_CURVE_NAMES[from + 1]}`
}

/** Where curve `index` sits on the morph's travel when `count` are in use. */
export function curvePosition(index: number, count: number): number {
  return index / (Math.max(2, count) - 1)
}

/** One bell per graphic band. A band at 0 dB is left out of the signal. */
export function curveSections(
  gainsDb: readonly number[],
  sampleRate: number
): Section[] {
  return MORPH_CURVE_HZ.map((hz, band) => {
    const gain = gainsDb[band] ?? 0
    return gain === 0 ? IDENTITY : peak(hz, MORPH_CURVE_Q, gain, sampleRate)
  })
}

/** The four movable bells at `morph`: Hz and Q move in ratios, dB in lines. */
export function interpolatedBells(
  a: readonly MorphBandParams[],
  b: readonly MorphBandParams[],
  morph: number
): MorphBandParams[] {
  if (morph <= 0) return [...a]
  if (morph >= 1) return [...b]
  const ratio = (from: number, to: number) =>
    Math.exp(Math.log(from) + morph * (Math.log(to) - Math.log(from)))
  return a.map((band, index) => ({
    frequencyHz: ratio(band.frequencyHz, b[index].frequencyHz),
    gainDb: band.gainDb + morph * (b[index].gainDb - band.gainDb),
    q: ratio(band.q, b[index].q),
  }))
}

/** Everything the effect does with these settings, as sections in series. */
export function morphEqSections(
  params: Pick<
    MorphEqParams,
    "snapshotA" | "snapshotB" | "morph" | "curves" | "curveCount"
  >,
  sampleRate: number
): Section[] {
  const gains = interpolatedCurve(
    params.curves.map((curve) => curve.gainsDb),
    params.curveCount,
    params.morph
  )
  const bells = interpolatedBells(
    params.snapshotA.bands,
    params.snapshotB.bands,
    params.morph
  ).map((band) =>
    band.gainDb === 0
      ? IDENTITY
      : peak(band.frequencyHz, band.q, band.gainDb, sampleRate)
  )
  return [...curveSections(gains, sampleRate), ...bells]
}
