import type { Note, NoteUpdate } from "@/bindings"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"

import { clampPan, clampVelocity } from "./edit-math"
import { translatedText as t } from "@/lib/i18n"

/*
 * The value lane under the grid shows one bar per note: its velocity, or
 * its pan. This is the arithmetic of drawing and editing those bars.
 */

export type LaneKind = "velocity" | "pan" | "release" | "finePitchCents" | "modulationX" | "modulationY"

export const LANE_KINDS: { id: LaneKind; label: string }[] = [
  { id: "velocity", get label() { return t("Velocity") } },
  { id: "pan", get label() { return t("Pan") } },
  { id: "release", get label() { return t("Release") } },
  { id: "finePitchCents", get label() { return t("Fine pitch") } },
  { id: "modulationX", get label() { return t("Modulation X") } },
  { id: "modulationY", get label() { return t("Modulation Y") } },
]

type Range = { min: number; max: number }

const RANGES: Record<LaneKind, Range> = {
  velocity: { min: 0, max: 1 },
  pan: { min: -1, max: 1 },
  release: { min: 0, max: 1 },
  finePitchCents: { min: -1200, max: 1200 },
  modulationX: { min: 0, max: 1 },
  modulationY: { min: 0, max: 1 },
}

/** Space kept above and below the bars so a full bar's cap stays in view. */
export const LANE_PAD_PX = 5

export function laneValue(note: Note, kind: LaneKind): number {
  if (kind === "velocity" || kind === "pan") return note[kind]
  return (note.expression ?? DEFAULT_NOTE_EXPRESSION)[kind]
}

export function clampLaneValue(value: number, kind: LaneKind): number {
  // Two decimals are finer than anyone can drag and keep the file readable.
  const rounded = Math.round(value * 100) / 100
  if (kind === "velocity") return clampVelocity(rounded)
  if (kind === "pan") return clampPan(rounded)
  if (kind === "finePitchCents") return Math.min(1200, Math.max(-1200, Math.round(value)))
  return Math.min(1, Math.max(0, rounded))
}

/** The value a y coordinate stands for. The top of the lane is the maximum. */
export function valueAtY(y: number, height: number, kind: LaneKind): number {
  const { min, max } = RANGES[kind]
  const travel = Math.max(1, height - 2 * LANE_PAD_PX)
  const share = 1 - (y - LANE_PAD_PX) / travel
  return clampLaneValue(min + share * (max - min), kind)
}

export function yOfValue(
  value: number,
  height: number,
  kind: LaneKind
): number {
  const { min, max } = RANGES[kind]
  const travel = Math.max(1, height - 2 * LANE_PAD_PX)
  return LANE_PAD_PX + (1 - (value - min) / (max - min)) * travel
}

/** The y a bar grows from: the bottom for velocity, the middle for pan. */
export function baselineY(height: number, kind: LaneKind): number {
  return yOfValue(0, height, kind)
}

/** First index whose note starts at or after `tick`. Notes are sorted by start. */
export function lowerBound(notes: readonly Note[], tick: number): number {
  let low = 0
  let high = notes.length
  while (low < high) {
    const mid = (low + high) >>> 1
    if (notes[mid].start < tick) low = mid + 1
    else high = mid
  }
  return low
}

/**
 * The note whose bar is nearest to `tick`, no further than `reach` ticks
 * away. Among notes on the same tick a selected one wins, so a chord's
 * selected note can be picked out.
 */
export function nearestBar(
  notes: readonly Note[],
  tick: number,
  reach: number,
  selected: ReadonlySet<number>
): Note | null {
  let best: Note | null = null
  let bestDistance = Infinity
  const from = lowerBound(notes, tick - reach)
  for (let i = from; i < notes.length; i++) {
    const note = notes[i]
    if (note.start > tick + reach) break
    const distance = Math.abs(note.start - tick)
    const better =
      distance < bestDistance ||
      (distance === bestDistance &&
        selected.has(note.id) &&
        best !== null &&
        !selected.has(best.id))
    if (better) {
      best = note
      bestDistance = distance
    }
  }
  return best
}

/**
 * Painting: every bar between two pointer positions takes the value on the
 * straight line between them, so a fast drag draws a ramp and skips nothing.
 * Bars within `reach` ticks of either end count as under the pointer, so a
 * straight up-and-down drag keeps hold of its bar. `only` limits the bars
 * that can be painted. Adds to `into`.
 */
export function paintValues(
  notes: readonly Note[],
  kind: LaneKind,
  from: { tick: number; value: number },
  to: { tick: number; value: number },
  reach: number,
  into: Map<number, number>,
  only?: ReadonlySet<number>
): void {
  const left = Math.min(from.tick, to.tick) - reach
  const right = Math.max(from.tick, to.tick) + reach
  const span = to.tick - from.tick
  for (let i = lowerBound(notes, left); i < notes.length; i++) {
    const note = notes[i]
    if (note.start > right) break
    if (only && !only.has(note.id)) continue
    const share =
      span === 0 ? 1 : Math.min(1, Math.max(0, (note.start - from.tick) / span))
    into.set(
      note.id,
      clampLaneValue(from.value + (to.value - from.value) * share, kind)
    )
  }
}

/**
 * Scaling: dragging one selected bar changes every selected bar with it.
 * Velocities keep their proportions; pans keep their distances.
 */
export function scaleValues(
  notes: readonly Note[],
  kind: LaneKind,
  grabbed: number,
  target: number
): Map<number, number> {
  const values = new Map<number, number>()
  for (const note of notes) {
    const value = laneValue(note, kind)
    const next =
      kind === "velocity" && grabbed > 0
        ? value * (target / grabbed)
        : value + (target - grabbed)
    values.set(note.id, clampLaneValue(next, kind))
  }
  return values
}

/** The updates for the bars whose value really changed. */
export function laneUpdates(
  notes: readonly Note[],
  kind: LaneKind,
  values: ReadonlyMap<number, number>
): NoteUpdate[] {
  const updates: NoteUpdate[] = []
  for (const note of notes) {
    const value = values.get(note.id)
    if (value === undefined || value === laneValue(note, kind)) continue
    updates.push({
      id: note.id,
      patch: kind === "velocity" || kind === "pan" ? { [kind]: value } : { expression: { ...(note.expression ?? DEFAULT_NOTE_EXPRESSION), [kind]: value } },
    })
  }
  return updates
}
