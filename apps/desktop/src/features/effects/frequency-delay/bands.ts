import type { ParamInfo } from "@/bindings"
import { clampParam } from "@/features/params"

/** The frequency delay always has this many bands. */
export const BAND_COUNT = 16

/** What one row of bars edits: one setting of every band. */
export const LANES = [
  {
    id: "delayMs",
    name: "Delay",
    hint: "How late each band is heard. Draw across the bars to shape an echo that sweeps through the spectrum",
  },
  {
    id: "level",
    name: "Level",
    hint: "How loud each band's echo is",
  },
  {
    id: "pan",
    name: "Pan",
    hint: "Where each band's echo sits between left and right. The middle of the plot is the center",
  },
] as const

export type Lane = (typeof LANES)[number]["id"]

/** The id of one band's setting, as the descriptor spells it. */
export function bandParam(band: number, field: Lane | "enabled"): string {
  return `bands.${band}.${field}`
}

/**
 * The nominal edges of a band in Hz, before the Bandwidth control scales
 * them. The core puts crossover `i` at `20 * 1000^((i + 1) / 16)`, so the
 * bands divide 20 Hz to 20 kHz into equal ratios. The first band reaches
 * down to DC and the last up to half the sample rate.
 */
export function bandEdges(band: number): { from: number; to: number } {
  const edge = (index: number) => 20 * 1000 ** (index / BAND_COUNT)
  return { from: edge(band), to: edge(band + 1) }
}

/** The geometric middle of a band, for its label. */
export function bandCenter(band: number): number {
  const { from, to } = bandEdges(band)
  return Math.sqrt(from * to)
}

/** A short frequency for a narrow column: 27, 150, 1.5k, 12k. */
export function shortHz(hz: number): string {
  if (hz >= 10_000) return `${Math.round(hz / 1000)}k`
  if (hz >= 1000) return `${(hz / 1000).toFixed(1).replace(/\.0$/, "")}k`
  return String(Math.round(hz))
}

/** How far up its range a value is, 0 at the minimum and 1 at the maximum. */
export function fractionOf(info: ParamInfo, value: number): number {
  const span = info.max - info.min
  if (span <= 0) return 0
  return Math.min(1, Math.max(0, (value - info.min) / span))
}

/** The value a share of the range stands for, inside the range. */
export function valueAt(info: ParamInfo, fraction: number): number {
  const share = Math.min(1, Math.max(0, fraction))
  return clampParam(info, info.min + (info.max - info.min) * share)
}

/** A place in the plot: which band's column, and how far up it. */
export type PlotPoint = { band: number; fraction: number }

/**
 * Where a pointer is in the plot. `left`, `top`, `width` and `height` are
 * the plot's box, in the same pixels as `x` and `y`. A pointer outside the
 * box is taken to its nearest edge, so a stroke that leaves the plot keeps
 * drawing along it. Returns null while the plot has no size.
 */
export function plotPoint(
  x: number,
  y: number,
  box: { left: number; top: number; width: number; height: number }
): PlotPoint | null {
  if (!(box.width > 0) || !(box.height > 0)) return null
  const across = (x - box.left) / box.width
  const up = 1 - (y - box.top) / box.height
  return {
    band: Math.min(BAND_COUNT - 1, Math.max(0, Math.floor(across * BAND_COUNT))),
    fraction: Math.min(1, Math.max(0, up)),
  }
}

/**
 * Every band a stroke passes from one point to the next, with the height
 * of the straight line between them. A fast stroke skips columns between
 * two pointer events; this fills them in, so the drawn shape has no gaps.
 * `from` is left out of the result, since it was drawn already.
 */
export function strokeBands(from: PlotPoint | null, to: PlotPoint): PlotPoint[] {
  if (!from || from.band === to.band) return [to]
  const step = to.band > from.band ? 1 : -1
  const points: PlotPoint[] = []
  for (let band = from.band + step; band !== to.band + step; band += step) {
    const along = (band - from.band) / (to.band - from.band)
    points.push({
      band,
      fraction: from.fraction + (to.fraction - from.fraction) * along,
    })
  }
  return points
}

/**
 * The time a band's echo really takes, in ms, which the Scale and Short
 * range controls make of its Delay. The core's rule: a positive scale
 * multiplies the delay, a negative one multiplies what is left of a
 * second, and the short range is a tenth of either.
 */
export function effectiveDelayMs(
  delayMs: number,
  scale: number,
  shortRange: boolean
): number {
  const scaled = scale >= 0 ? scale * delayMs : -scale * (1000 - delayMs)
  return shortRange ? scaled * 0.1 : scaled
}
