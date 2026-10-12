import type { Pattern } from "@/bindings"
import { curveValue } from "@/lib/automation/curve"

import { patternTicks, wrapTicks } from "./edit"

/*
 * The small picture of a pattern's notes drawn inside each of its clips.
 * The picture is worked out once per pattern and reused by every clip and
 * every frame; only placing it on the timeline happens while drawing.
 */

export const SEGMENT_STRIDE = 4

/** A pattern's notes laid out in a box one pass long and 1 tall. */
export type PatternPreview = {
  /** One pass of the pattern in ticks. */
  readonly length: number
  /** start tick, length in ticks, top, height per segment. Top and height are 0 to 1. */
  readonly segments: Float32Array
  readonly count: number
}

/** More notes than this are drawn as a coarse picture instead of one by one. */
export const MAX_PREVIEW_SEGMENTS = 192
const COARSE_COLUMNS = 64
const COARSE_ROWS = 16

type Placed = { start: number; length: number; unit: number }

/**
 * Lays out a pattern's notes. Each channel gets a band as tall as its range
 * of keys, so a drum pattern shows one line per drum and a melody keeps its
 * shape. Notes past the end of the pattern never play and are left out.
 */
export function buildPreview(
  pattern: Pattern,
  maxSegments = MAX_PREVIEW_SEGMENTS
): PatternPreview {
  const length = patternTicks(pattern)
  const placed: Placed[] = []
  let units = 0
  for (const lane of pattern.lanes) {
    let low = Infinity
    let high = -Infinity
    for (const note of lane.notes) {
      if (note.start >= length) continue
      low = Math.min(low, note.key)
      high = Math.max(high, note.key)
    }
    if (high < low) continue
    for (const note of lane.notes) {
      if (note.start >= length) continue
      placed.push({
        start: note.start,
        length: Math.max(1, Math.min(note.length, length - note.start)),
        unit: units + (high - note.key),
      })
    }
    units += high - low + 1
  }
  // A bounded curve silhouette in its own band makes event-only patterns
  // visible too. Pattern offsets/repeats use the same previewSpans mapping.
  for (const lane of (pattern.events ?? []).slice(0, 8)) {
    if (!lane.points.length) continue
    for (let column = 0; column < 32; column++) {
      const start = column * length / 32
      placed.push({ start, length: length / 32, unit: units + (1 - curveValue(lane.points, start)) * 7 })
    }
    units += 8
  }
  if (placed.length === 0) {
    return { length, segments: new Float32Array(0), count: 0 }
  }
  if (placed.length > maxSegments) return coarsePreview(placed, units, length)

  placed.sort((a, b) => a.start - b.start || a.unit - b.unit)
  const segments = new Float32Array(placed.length * SEGMENT_STRIDE)
  placed.forEach((note, index) => {
    const s = index * SEGMENT_STRIDE
    segments[s] = note.start
    segments[s + 1] = note.length
    segments[s + 2] = note.unit / units
    segments[s + 3] = 1 / units
  })
  return { length, segments, count: placed.length }
}

/** Marks which cells of a small grid hold notes and joins them into runs. */
function coarsePreview(
  placed: readonly Placed[],
  units: number,
  length: number
): PatternPreview {
  const rows = Math.min(units, COARSE_ROWS)
  const cell = length / COARSE_COLUMNS
  const filled = new Uint8Array(rows * COARSE_COLUMNS)
  for (const note of placed) {
    const row = Math.min(rows - 1, Math.floor((note.unit / units) * rows))
    const first = Math.min(COARSE_COLUMNS - 1, Math.floor(note.start / cell))
    const last = Math.min(
      COARSE_COLUMNS - 1,
      Math.floor((note.start + note.length - 1) / cell)
    )
    for (let column = first; column <= last; column++) {
      filled[row * COARSE_COLUMNS + column] = 1
    }
  }
  const runs: number[] = []
  for (let column = 0; column < COARSE_COLUMNS; column++) {
    for (let row = 0; row < rows; row++) {
      const at = row * COARSE_COLUMNS + column
      if (filled[at] !== 1) continue
      let end = column
      while (
        end + 1 < COARSE_COLUMNS &&
        filled[at + (end + 1 - column)] === 1
      ) {
        end++
      }
      // Cells of a run are cleared so each run is reported once.
      for (let c = column; c <= end; c++) filled[row * COARSE_COLUMNS + c] = 2
      runs.push(column * cell, (end - column + 1) * cell, row / rows, 1 / rows)
    }
  }
  return {
    length,
    segments: Float32Array.from(runs),
    count: runs.length / SEGMENT_STRIDE,
  }
}

const previews = new WeakMap<Pattern, PatternPreview>()

/**
 * The preview of a pattern. The store keeps a pattern's object until the
 * pattern changes, so the object itself is the cache key.
 */
export function previewOf(pattern: Pattern): PatternPreview {
  let preview = previews.get(pattern)
  if (!preview) {
    preview = buildPreview(pattern)
    previews.set(pattern, preview)
  }
  return preview
}

export type ClipSpan = {
  readonly start: number
  readonly length: number
  readonly offset: number
}

/**
 * Tick where the pass of the loop that is playing at the clip's start
 * began. It is at or before the clip's start.
 */
export function firstPassStart(clip: ClipSpan, passTicks: number): number {
  return clip.start - wrapTicks(clip.offset, passTicks)
}

/**
 * Ticks inside a clip where the pattern starts over. The clip's own start
 * is not one, even when a pass begins there.
 */
export function loopPoints(
  clip: ClipSpan,
  passTicks: number,
  from = -Infinity,
  to = Infinity
): number[] {
  if (passTicks <= 0) return []
  const end = Math.min(clip.start + clip.length, to)
  const begin = Math.max(clip.start, from)
  const first = firstPassStart(clip, passTicks)
  const points: number[] = []
  // Skips straight to the first pass that can be in range.
  let k = Math.max(1, Math.ceil((begin - first) / passTicks))
  for (; first + k * passTicks < end; k++) {
    const tick = first + k * passTicks
    if (tick > clip.start && tick >= begin) points.push(tick)
  }
  return points
}

/**
 * Calls `emit` for every piece of a note that shows inside a clip, between
 * `from` and `to` on the timeline. The pattern repeats for as long as the
 * clip is, starting `offset` ticks in, and notes are cut at the clip's
 * edges. Returns how many pieces there were.
 */
export function previewSpans(
  preview: PatternPreview,
  clip: ClipSpan,
  from: number,
  to: number,
  emit: (start: number, end: number, top: number, height: number) => void
): number {
  const pass = preview.length
  if (pass <= 0 || preview.count === 0) return 0
  const begin = Math.max(clip.start, from)
  const end = Math.min(clip.start + clip.length, to)
  if (end <= begin) return 0
  const first = firstPassStart(clip, pass)
  const segments = preview.segments
  let emitted = 0
  let k = Math.max(0, Math.floor((begin - first) / pass))
  for (; first + k * pass < end; k++) {
    const passStart = first + k * pass
    for (let index = 0; index < preview.count; index++) {
      const s = index * SEGMENT_STRIDE
      const noteStart = passStart + segments[s]
      // Segments are sorted by start, so the rest of this pass is out too.
      if (noteStart >= end) break
      const noteEnd = noteStart + segments[s + 1]
      if (noteEnd <= begin) continue
      emit(
        Math.max(noteStart, begin),
        Math.min(noteEnd, end),
        segments[s + 2],
        segments[s + 3]
      )
      emitted++
    }
  }
  return emitted
}
