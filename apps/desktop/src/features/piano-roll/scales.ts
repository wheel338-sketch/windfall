import { pianoRows, rowToKey, type RowStyle } from "@/lib/canvas"
import { translatedText as t } from "@/lib/i18n"

/** Public musical interval definitions, written here rather than imported presets. */
export const SCALES = [
  { id: "major", get label() { return t("Major") }, intervals: [0, 2, 4, 5, 7, 9, 11] },
  { id: "minor", get label() { return t("Natural minor") }, intervals: [0, 2, 3, 5, 7, 8, 10] },
  {
    id: "harmonic-minor",
    get label() { return t("Harmonic minor") },
    intervals: [0, 2, 3, 5, 7, 8, 11],
  },
  {
    id: "melodic-minor",
    get label() { return t("Melodic minor (ascending)") },
    intervals: [0, 2, 3, 5, 7, 9, 11],
  },
  { id: "dorian", get label() { return t("Dorian") }, intervals: [0, 2, 3, 5, 7, 9, 10] },
  { id: "phrygian", get label() { return t("Phrygian") }, intervals: [0, 1, 3, 5, 7, 8, 10] },
  { id: "lydian", get label() { return t("Lydian") }, intervals: [0, 2, 4, 6, 7, 9, 11] },
  { id: "mixolydian", get label() { return t("Mixolydian") }, intervals: [0, 2, 4, 5, 7, 9, 10] },
  { id: "locrian", get label() { return t("Locrian") }, intervals: [0, 1, 3, 5, 6, 8, 10] },
  {
    id: "major-pentatonic",
    get label() { return t("Major pentatonic") },
    intervals: [0, 2, 4, 7, 9],
  },
  {
    id: "minor-pentatonic",
    get label() { return t("Minor pentatonic") },
    intervals: [0, 3, 5, 7, 10],
  },
  { id: "blues", get label() { return t("Blues") }, intervals: [0, 3, 5, 6, 7, 10] },
  { id: "whole-tone", get label() { return t("Whole tone") }, intervals: [0, 2, 4, 6, 8, 10] },
  {
    id: "chromatic",
    get label() { return t("Chromatic") },
    intervals: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  },
] as const

export type ScaleId = (typeof SCALES)[number]["id"]
export type PitchScale = { root: number; id: ScaleId }
export const ROOT_NAMES = [
  "C",
  "C♯ / D♭",
  "D",
  "D♯ / E♭",
  "E",
  "F",
  "F♯ / G♭",
  "G",
  "G♯ / A♭",
  "A",
  "A♯ / B♭",
  "B",
] as const

export function isScaleId(value: unknown): value is ScaleId {
  return SCALES.some((scale) => scale.id === value)
}

export function isScaleRoot(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value < 12
  )
}

export function scaleDefinition(id: ScaleId) {
  return SCALES.find((scale) => scale.id === id)!
}

export function inScale(key: number, scale: PitchScale): boolean {
  const pitchClass = (((key - scale.root) % 12) + 12) % 12
  return scaleDefinition(scale.id).intervals.some(
    (interval) => interval === pitchClass
  )
}

/** Inclusive MIDI bounds. Equal distances choose the lower key. No candidate is null. */
export function nearestScaleKey(
  key: number,
  scale: PitchScale,
  low = 0,
  high = 127
): number | null {
  if (!Number.isFinite(key) || !Number.isFinite(low) || !Number.isFinite(high))
    return null
  let best: number | null = null
  let distance = Infinity
  for (
    let candidate = Math.max(0, Math.ceil(low));
    candidate <= Math.min(127, Math.floor(high));
    candidate++
  ) {
    if (!inScale(candidate, scale)) continue
    const next = Math.abs(candidate - key)
    if (next < distance) {
      best = candidate
      distance = next
    }
  }
  return best
}

/**
 * Snap one anchor and move every voice by that interval. Other voices can remain
 * outside the scale; intervals and unisons never collapse. A keyboard nudge must
 * advance in its requested direction, unless the group has reached a boundary.
 */
export function scaleMoveKeys(
  anchor: number,
  keys: number,
  limits: { minKeys: number; maxKeys: number },
  scale: PitchScale | null,
  directional = false
): number {
  const bounded = Math.max(
    limits.minKeys,
    Math.min(limits.maxKeys, Math.round(keys))
  )
  if (!scale || keys === 0) return bounded
  let low = anchor + limits.minKeys
  let high = anchor + limits.maxKeys
  if (directional) {
    if (keys > 0) low = Math.max(low, anchor + 1)
    else high = Math.min(high, anchor - 1)
  }
  const snapped = nearestScaleKey(anchor + bounded, scale, low, high)
  // If there is no allowed anchor inside the group's bounds, preserve the group.
  return snapped === null ? 0 : snapped - anchor
}

/** Default black-key shading is unchanged when highlighting is off. */
export function scaleRows(scale: PitchScale | null): RowStyle {
  if (!scale) return pianoRows()
  const shaded = new Uint8Array(128)
  const strong = new Uint8Array(128)
  for (let row = 0; row < 128; row++) {
    const key = rowToKey(row)
    shaded[row] = inScale(key, scale) ? 0 : 1
    // As with pianoRows' C boundary, the line sits just below each root.
    strong[row] = (key + 1) % 12 === scale.root ? 1 : 0
  }
  return { rowCount: 128, shaded, strong }
}
