import type {
  Command,
  ChannelId,
  NoteCurveInsert,
  NoteExpressionCurve,
  Note,
  NoteInit,
  NoteUpdate,
  PatternId,
  TimeSignature,
  Timeline,
} from "@/bindings"
import { resizedSpan } from "@/lib/canvas"
import { ticksPerBar } from "@/lib/time"
import { meterSegments } from "@/lib/timeline"
import { clamp, MAX_PATTERN_STEPS, TICKS_PER_STEP } from "@/lib/units"

import { snapCeil, snapRound } from "./snap"

/*
 * The arithmetic of every edit, with no pointer, canvas or store in it. The
 * Rust backend clamps values that are out of range and the browser mock
 * rejects them, so everything here returns values that are already valid.
 */

export const MAX_KEY = 127
/** The key the piano roll centers on when a lane is empty. FL calls it C5. */
export const HOME_KEY = 60

export function clampKey(key: number): number {
  return clamp(Math.round(key), 0, MAX_KEY)
}

export function clampVelocity(velocity: number): number {
  return clamp(velocity, 0, 1)
}

export function clampPan(pan: number): number {
  return clamp(pan, -1, 1)
}

/**
 * The last tick a note may reach: the end of the longest pattern there can
 * be. A note past it could never play, in any pattern.
 */
export const MAX_PATTERN_TICKS = MAX_PATTERN_STEPS * TICKS_PER_STEP

export function clampStart(start: number): number {
  return Math.max(0, Math.round(start))
}

export function clampLength(length: number): number {
  return Math.max(1, Math.round(length))
}

export function noteEnd(note: Pick<Note, "start" | "length">): number {
  return note.start + note.length
}

export type Extent = {
  start: number
  end: number
  lowKey: number
  highKey: number
}

/** The box around some notes, or null when there are none. */
export function notesExtent(
  notes: Iterable<Pick<Note, "start" | "length" | "key">>
): Extent | null {
  let extent: Extent | null = null
  for (const note of notes) {
    if (extent === null) {
      extent = {
        start: note.start,
        end: noteEnd(note),
        lowKey: note.key,
        highKey: note.key,
      }
      continue
    }
    extent.start = Math.min(extent.start, note.start)
    extent.end = Math.max(extent.end, noteEnd(note))
    extent.lowKey = Math.min(extent.lowKey, note.key)
    extent.highKey = Math.max(extent.highKey, note.key)
  }
  return extent
}

export type MoveLimits = {
  /** The furthest left the group can go, as a tick delta (zero or less). */
  minTicks: number
  minKeys: number
  maxKeys: number
}

/**
 * How far a group of notes can move before one of them leaves the keyboard
 * or starts before the pattern does. A group moves as one, so it stops when
 * its first note would.
 */
export function moveLimits(notes: readonly Note[]): MoveLimits {
  const extent = notesExtent(notes)
  if (!extent) return { minTicks: 0, minKeys: 0, maxKeys: 0 }
  return {
    minTicks: -extent.start,
    minKeys: -extent.lowKey,
    maxKeys: MAX_KEY - extent.highKey,
  }
}

export type MoveDelta = { ticks: number; keys: number }

/**
 * The move a drag stands for. Time snaps by distance moved, so notes that
 * sit off the grid keep their offset. `rawTicks` may be fractional.
 */
export function moveDelta(
  rawTicks: number,
  rawKeys: number,
  snap: number,
  limits: MoveLimits
): MoveDelta {
  return {
    ticks: Math.max(limits.minTicks, snapRound(rawTicks, snap)),
    keys: clamp(Math.round(rawKeys), limits.minKeys, limits.maxKeys),
  }
}

export function moveUpdates(
  notes: readonly Note[],
  delta: MoveDelta
): NoteUpdate[] {
  if (delta.ticks === 0 && delta.keys === 0) return []
  return notes.map((note) => ({
    id: note.id,
    patch: {
      ...(delta.ticks !== 0 && { start: clampStart(note.start + delta.ticks) }),
      ...(delta.keys !== 0 && { key: clampKey(note.key + delta.keys) }),
    },
  }))
}

/** Copies of notes at their moved places, for a move that duplicates. */
export function movedCopies(
  notes: readonly Note[],
  delta: MoveDelta
): NoteInit[] {
  return notes.map((note) => ({
    start: clampStart(note.start + delta.ticks),
    length: clampLength(note.length),
    key: clampKey(note.key + delta.keys),
    velocity: clampVelocity(note.velocity),
    pan: clampPan(note.pan),
    expression: note.expression ? { ...note.expression } : undefined,
  }))
}

export type ResizeEdge = "start" | "end"

export type ResizeDelta = {
  /** Ticks added to the start edge. The end stays. */
  start: number
  /** Ticks added to the end edge. */
  end: number
  /** No note is made shorter than this, or than it already is. */
  minLength: number
}

/** The shortest a resize makes a note: one snap interval, or a tick. */
export function resizeMinLength(snap: number): number {
  return snap > 0 ? snap : 1
}

/**
 * The resize a drag of one edge stands for. Every selected note gets the
 * same change, and the start edge cannot pull a note before tick 0.
 *
 * With `absolute`, the grabbed edge (`anchor`) is snapped to the grid lines
 * counted from `origin`, so a sixteenth dragged on the beat grid ends on a
 * beat. Without it the distance is snapped, which keeps an edge that is off
 * the grid off it by the same amount.
 */
export function resizeDelta(
  edge: ResizeEdge,
  rawTicks: number,
  snap: number,
  notes: readonly Note[],
  absolute?: { anchor: number; origin: number }
): ResizeDelta {
  const ticks = absolute ? snapRound(absolute.anchor + rawTicks, snap, absolute.origin) - absolute.anchor : snapRound(rawTicks, snap)
  const minLength = resizeMinLength(snap)
  if (edge === "end") return { start: 0, end: ticks, minLength }
  const earliest = notes.reduce(
    (least, note) => Math.min(least, note.start),
    Infinity
  )
  const floor = Number.isFinite(earliest) ? -earliest : 0
  return { start: Math.max(floor, ticks), end: 0, minLength }
}

export function resizeUpdates(
  notes: readonly Note[],
  delta: ResizeDelta
): NoteUpdate[] {
  const updates: NoteUpdate[] = []
  for (const note of notes) {
    const span = resizedSpan(
      note.start,
      note.length,
      delta.start,
      delta.end,
      delta.minLength
    )
    const start = clampStart(span.start)
    const length = clampLength(span.length)
    if (start === note.start && length === note.length) continue
    updates.push({
      id: note.id,
      patch: {
        ...(start !== note.start && { start }),
        ...(length !== note.length && { length }),
      },
    })
  }
  return updates
}

/** Moves each start to the nearest snap line. Lengths stay. */
export function quantizeStarts(
  notes: readonly Note[],
  snap: number
): NoteUpdate[] {
  if (snap <= 0) return []
  const updates: NoteUpdate[] = []
  for (const note of notes) {
    const start = clampStart(snapRound(note.start, snap))
    if (start !== note.start) updates.push({ id: note.id, patch: { start } })
  }
  return updates
}

/** Moves each end to the nearest snap line that leaves the note a length. */
export function quantizeEnds(
  notes: readonly Note[],
  snap: number
): NoteUpdate[] {
  if (snap <= 0) return []
  const updates: NoteUpdate[] = []
  for (const note of notes) {
    let end = snapRound(noteEnd(note), snap)
    if (end <= note.start) end = snapCeil(note.start + 1, snap)
    const length = clampLength(end - note.start)
    if (length !== note.length) updates.push({ id: note.id, patch: { length } })
  }
  return updates
}

/**
 * Copies of the notes placed right after them. The gap between the
 * original and the copy is the group's length rounded up to the snap, so a
 * one-bar phrase repeats on the next bar. Returns the copies and how far
 * they were moved.
 */
export function duplicateRight(
  notes: readonly Note[],
  snap: number
): { inits: NoteInit[]; offset: number } {
  const extent = notesExtent(notes)
  if (!extent) return { inits: [], offset: 0 }
  const offset = Math.max(1, snapCeil(extent.end - extent.start, snap))
  return { inits: movedCopies(notes, { ticks: offset, keys: 0 }), offset }
}

/** A note as the clipboard keeps it: its start counts from the group's. */
export type ClipNote = Omit<Note, "id">

export type ClipContents = {
  curves?: NoteCurveInsert[]
  notes: ClipNote[]
  /** Where the group started in the pattern it was copied from. */
  origin: number
}

export function curveInserts(notes: readonly Note[], curves: readonly NoteExpressionCurve[] = []): NoteCurveInsert[] {
  const indices = new Map(notes.map((note, index) => [note.id, index]))
  return curves.flatMap((curve) => {
    const noteIndex = indices.get(curve.note)
    return noteIndex === undefined ? [] : [{ noteIndex, parameter: curve.parameter, points: curve.points.map((point) => ({ ...point })) }]
  })
}

export function noteInsertionCommand(target: { pattern: PatternId; channel: ChannelId }, notes: NoteInit[], curves: NoteCurveInsert[] = []): Command {
  return curves.length ? { type: "addNotesWithCurves", ...target, notes, curves } : { type: "addNotes", ...target, notes }
}

export function copyNotes(notes: readonly Note[], sourceCurves: readonly NoteExpressionCurve[] = []): ClipContents | null {
  const extent = notesExtent(notes)
  if (!extent) return null
  const curves = curveInserts(notes, sourceCurves)
  return {
    ...(curves.length ? { curves } : {}),
    origin: extent.start,
    notes: notes.map((note) => ({
      start: note.start - extent.start,
      length: note.length,
      key: note.key,
      velocity: note.velocity,
      pan: note.pan,
      expression: note.expression ? { ...note.expression } : undefined,
    })),
  }
}

export type PasteTarget =
  /** The first note lands exactly here. */
  | { at: "playhead"; tick: number }
  /** The group keeps its place in the bar, in the first bar in view. */
  | { at: "view"; leftTick: number }

/** The tick the first pasted note starts on. */
export function pasteStart(
  clip: ClipContents,
  target: PasteTarget,
  snap: number,
  signature: TimeSignature,
  timeline?: Timeline,
  settingsAt?: (tick: number) => { snap: number; snapOrigin?: number }
): number {
  if (timeline?.meters.length) {
    const raw = target.at === "playhead" ? target.tick : target.leftTick
    const segments = meterSegments(signature, timeline.meters)
    const segment = segments.findLast((item) => item.start <= raw) ?? segments[0]
    if (target.at === "playhead") {
      const settings = settingsAt?.(raw)
      return clampStart(Math.min(segment.end, snapRound(raw, settings?.snap ?? snap, settings?.snapOrigin ?? segment.start)))
    }
    const bar = ticksPerBar(segment.signature)
    const first = segment.start + Math.ceil((Math.max(segment.start, raw) - segment.start) / bar) * bar
    const destination = first >= segment.end ? segments.find((item) => item.start === segment.end) ?? segment : segment
    const origin = segments.findLast((item) => item.start <= clip.origin) ?? segments[0]
    const offset = (clip.origin - origin.start) % ticksPerBar(origin.signature)
    const start = first >= segment.end ? destination.start : first
    return clampStart(start + Math.min(offset, ticksPerBar(destination.signature) - 1))
  }
  if (target.at === "playhead") {
    return clampStart(snapRound(target.tick, snap))
  }
  const bar = ticksPerBar(signature)
  const firstBar = Math.ceil(Math.max(0, target.leftTick) / bar) * bar
  return clampStart(firstBar + (clip.origin % bar))
}

export function pasteInits(clip: ClipContents, start: number): NoteInit[] {
  return clip.notes.map((note) => ({
    start: clampStart(start + note.start),
    length: clampLength(note.length),
    key: clampKey(note.key),
    velocity: clampVelocity(note.velocity),
    pan: clampPan(note.pan),
    expression: note.expression ? { ...note.expression } : undefined,
  }))
}

/**
 * The pattern length that holds a note ending at `endTick`: the end of the
 * bar it reaches into. Null when the pattern is already long enough or
 * cannot grow any more.
 */
export function extendedLengthSteps(
  lengthSteps: number,
  endTick: number,
  signature: TimeSignature
): number | null {
  if (endTick <= lengthSteps * TICKS_PER_STEP) return null
  const bar = ticksPerBar(signature)
  const steps = Math.min(
    MAX_PATTERN_STEPS,
    Math.ceil((Math.ceil(endTick / bar) * bar) / TICKS_PER_STEP)
  )
  return steps > lengthSteps ? steps : null
}

/**
 * The latest end among the notes an update moves or resizes. Notes whose
 * place in time is untouched do not count: changing the velocity of a note
 * that hangs past the end of the pattern must not make the pattern longer.
 */
export function endAfterUpdates(
  notes: readonly Note[],
  updates: readonly NoteUpdate[]
): number {
  const byId = new Map(updates.map((update) => [update.id, update.patch]))
  let end = 0
  for (const note of notes) {
    const patch = byId.get(note.id)
    if (!patch) continue
    if (patch.start === undefined && patch.length === undefined) continue
    end = Math.max(
      end,
      (patch.start ?? note.start) + (patch.length ?? note.length)
    )
  }
  return end
}

export function endOfInits(inits: readonly NoteInit[]): number {
  return inits.reduce((end, init) => Math.max(end, init.start + init.length), 0)
}

/** Whether new notes all end inside the longest pattern there can be. */
export function initsFit(inits: readonly NoteInit[]): boolean {
  return endOfInits(inits) <= MAX_PATTERN_TICKS
}

/**
 * Whether an update leaves every note it moves or resizes inside the
 * longest pattern there can be. A note that is out there already (from a
 * file made before this was checked) may still be brought back, or moved
 * without going further out.
 */
export function updatesFit(
  notes: readonly Note[],
  updates: readonly NoteUpdate[]
): boolean {
  const byId = new Map(updates.map((update) => [update.id, update.patch]))
  for (const note of notes) {
    const patch = byId.get(note.id)
    if (!patch) continue
    const end = (patch.start ?? note.start) + (patch.length ?? note.length)
    if (end > MAX_PATTERN_TICKS && end > noteEnd(note)) return false
  }
  return true
}

export type PatternInfo = {
  noteCurves?: readonly NoteExpressionCurve[]
  id: PatternId
  lengthSteps: number
  signature: TimeSignature
  timeline?: Timeline
}

/**
 * The length a pattern grows to so that it holds a note ending at
 * `endTick`, to the end of the bar in the meter there. Null when it is
 * long enough already.
 */
export function extensionSteps(
  pattern: PatternInfo,
  endTick: number
): number | null {
  let lengthSteps = extendedLengthSteps(
    pattern.lengthSteps,
    endTick,
    pattern.signature
  )
  if (lengthSteps !== null && pattern.timeline?.meters.length) {
    const segment = meterSegments(pattern.signature, pattern.timeline.meters).findLast((item) => item.start < endTick)
    if (segment) {
      const bar = ticksPerBar(segment.signature)
      const end = segment.start + Math.ceil((endTick - segment.start) / bar) * bar
      lengthSteps = clamp(Math.ceil(end / TICKS_PER_STEP), 1, MAX_PATTERN_STEPS)
    }
  }
  return lengthSteps
}

/** An edit and a new pattern length, as one undo step. */
export function withLength(
  command: Command,
  label: string,
  pattern: PatternId,
  lengthSteps: number
): Command {
  return {
    type: "batch",
    label,
    commands: [
      command,
      { type: "updatePattern", id: pattern, patch: { lengthSteps } },
    ],
  }
}

/**
 * One command for an edit. When the edit puts a note past the end of the
 * pattern, the pattern grows to the next bar in the same undo step.
 */
export function withExtension(
  command: Command,
  label: string,
  pattern: PatternInfo,
  endTick: number
): Command {
  const lengthSteps = extensionSteps(pattern, endTick)
  if (lengthSteps === null) return command
  return withLength(command, label, pattern.id, lengthSteps)
}

/** The latest end among a lane's notes once an update has moved or resized them. */
export function laneEndAfterUpdates(
  notes: readonly Note[],
  updates: readonly NoteUpdate[]
): number {
  const byId = new Map(updates.map((update) => [update.id, update.patch]))
  let end = 0
  for (const note of notes) {
    const patch = byId.get(note.id)
    end = Math.max(
      end,
      (patch?.start ?? note.start) + (patch?.length ?? note.length)
    )
  }
  return end
}

/**
 * The length a pattern that an edit grew from `fromSteps` can go back to
 * once its content ends at `contentEnd`: the end of the bar the content
 * reaches, never shorter than the pattern was before it grew. Null when it
 * stays as it is.
 */
export function shrunkLengthSteps(
  fromSteps: number,
  lengthSteps: number,
  contentEnd: number,
  signature: TimeSignature
): number | null {
  const bar = ticksPerBar(signature)
  const steps = Math.max(
    fromSteps,
    Math.ceil((Math.ceil(contentEnd / bar) * bar) / TICKS_PER_STEP)
  )
  return steps < lengthSteps ? steps : null
}

/** The pattern length a ruler drag asks for: whole bars, or whole steps. */
export function lengthStepsAt(
  tick: number,
  signature: TimeSignature,
  bySteps: boolean
): number {
  const unit = bySteps ? TICKS_PER_STEP : ticksPerBar(signature)
  const ticks = Math.max(unit, Math.round(tick / unit) * unit)
  return clamp(Math.round(ticks / TICKS_PER_STEP), 1, MAX_PATTERN_STEPS)
}

/**
 * How far apart the paint tool places notes: the snap, or the note length
 * rounded up to the snap when the note is longer, so painted notes on one
 * key never overlap.
 */
export function paintSpacing(snap: number, noteLength: number): number {
  if (snap <= 0) return Math.max(1, noteLength)
  return Math.max(snap, snapCeil(noteLength, snap))
}

/** The paint cells between two cells, both included, in drag order. */
export function cellsBetween(from: number, to: number): number[] {
  const cells: number[] = []
  const step = to >= from ? 1 : -1
  for (let cell = from; cell !== to + step; cell += step) cells.push(cell)
  return cells
}

export type RowSpan = { row: number; tick0: number; tick1: number }

/**
 * The stretch of each row a straight pointer move passes over, so an erase
 * drag catches every note it crosses however fast the pointer went.
 */
export function rowsAlongSegment(
  tick0: number,
  row0: number,
  tick1: number,
  row1: number
): RowSpan[] {
  const first = Math.floor(Math.min(row0, row1))
  const last = Math.floor(Math.max(row0, row1))
  if (first === last) {
    return [
      {
        row: first,
        tick0: Math.min(tick0, tick1),
        tick1: Math.max(tick0, tick1),
      },
    ]
  }
  const spans: RowSpan[] = []
  const slope = (tick1 - tick0) / (row1 - row0)
  const low = Math.min(row0, row1)
  const high = Math.max(row0, row1)
  for (let row = first; row <= last; row++) {
    const enter = Math.max(low, row)
    const leave = Math.min(high, row + 1)
    const a = tick0 + (enter - row0) * slope
    const b = tick0 + (leave - row0) * slope
    spans.push({ row, tick0: Math.min(a, b), tick1: Math.max(a, b) })
  }
  return spans
}
