import type {
  ChannelId,
  Command,
  Note,
  NoteInit,
  PatternId,
  TimeSignature,
  Timeline,
} from "@/bindings"
import {
  clamp,
  DEFAULT_KEY,
  MAX_PATTERN_STEPS,
  TICKS_PER_STEP,
} from "@/lib/units"
import { meterSegments, musicalPosition } from "@/lib/timeline"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"
import { message, plainNumber, historyLabel } from "@/lib/i18n"

/*
 * The step sequencer and the piano roll edit the same notes. These helpers
 * turn a lane's notes into the row of steps the rack shows, and the rack's
 * row edits back into commands. They are pure so they can be tested without
 * a backend.
 */

/** The note a lit step stands for. */
export function stepNote(step: number): NoteInit {
  return {
    start: step * TICKS_PER_STEP,
    length: TICKS_PER_STEP,
    key: DEFAULT_KEY,
  }
}

/** True for a note that is exactly what clicking a step makes. */
export function isStepNote(note: Note): boolean {
  return (
    note.start % TICKS_PER_STEP === 0 &&
    note.key === DEFAULT_KEY &&
    note.length === TICKS_PER_STEP
  )
}

/** One entry per step: lit when a note starts exactly on it. */
export function litSteps(
  notes: readonly Note[] | undefined,
  lengthSteps: number
): boolean[] {
  const steps = new Array<boolean>(lengthSteps).fill(false)
  for (const note of notes ?? []) {
    if (note.start % TICKS_PER_STEP !== 0) continue
    const step = note.start / TICKS_PER_STEP
    if (step < lengthSteps) steps[step] = true
  }
  return steps
}

/**
 * Steps that hold something a step toggle cannot show or put back: a note
 * off the grid, on another key, of another length, or several notes at once.
 * Sorted, each step once.
 */
export function detailSteps(
  notes: readonly Note[] | undefined,
  lengthSteps: number
): number[] {
  const onGrid = new Map<number, number>()
  const found = new Set<number>()
  for (const note of notes ?? []) {
    const step = Math.floor(note.start / TICKS_PER_STEP)
    if (step >= lengthSteps) continue
    if (!isStepNote(note)) found.add(step)
    if (note.start % TICKS_PER_STEP === 0) {
      const count = (onGrid.get(step) ?? 0) + 1
      onGrid.set(step, count)
      if (count > 1) found.add(step)
    }
  }
  return [...found].sort((a, b) => a - b)
}

function notesInPattern(
  notes: readonly Note[] | undefined,
  lengthSteps: number
): Note[] {
  const end = lengthSteps * TICKS_PER_STEP
  return (notes ?? []).filter((note) => note.start < end)
}

/** The steps "fill every N" lights: 0, N, 2N and so on. */
export function fillSteps(every: number, lengthSteps: number): number[] {
  const steps: number[] = []
  for (let step = 0; step < lengthSteps; step += Math.max(1, every)) {
    steps.push(step)
  }
  return steps
}

/**
 * Replaces what the channel plays inside the pattern with a hit on every
 * `every`th step. One undo step.
 */
export function fillCommand(
  pattern: PatternId,
  channel: ChannelId,
  notes: readonly Note[] | undefined,
  lengthSteps: number,
  every: number
): Command {
  const old = notesInPattern(notes, lengthSteps).map((note) => note.id)
  const commands: Command[] = []
  if (old.length > 0) {
    commands.push({ type: "removeNotes", pattern, channel, notes: old })
  }
  commands.push({
    type: "addNotes",
    pattern,
    channel,
    notes: fillSteps(every, lengthSteps).map(stepNote),
  })
  return {
    type: "batch",
    label: historyLabel("Fill every {every} steps", { every }),
    commands,
  }
}

/** Where a note lands when the row moves by `by` steps and wraps around. */
export function rotateStart(
  start: number,
  by: number,
  lengthSteps: number
): number {
  const length = lengthSteps * TICKS_PER_STEP
  return (((start + by * TICKS_PER_STEP) % length) + length) % length
}

/** A row of steps moved by `by` steps, wrapping around. */
export function rotateSteps<T>(steps: readonly T[], by: number): T[] {
  const count = steps.length
  if (count === 0) return []
  return steps.map((_, step) => steps[(((step - by) % count) + count) % count])
}

/**
 * Moves every note inside the pattern one step left (-1) or right (1). Notes
 * that fall off one end come back at the other, and each keeps its key,
 * length and place between steps. Null when there is nothing to move.
 */
export function shiftCommand(
  pattern: PatternId,
  channel: ChannelId,
  notes: readonly Note[] | undefined,
  lengthSteps: number,
  by: -1 | 1
): Command | null {
  const inside = notesInPattern(notes, lengthSteps)
  if (inside.length === 0 || lengthSteps < 2) return null
  return {
    type: "batch",
    label: by < 0 ? historyLabel("Shift steps left") : historyLabel("Shift steps right"),
    commands: [
      {
        type: "updateNotes",
        pattern,
        channel,
        updates: inside.map((note) => ({
          id: note.id,
          patch: { start: rotateStart(note.start, by, lengthSteps) },
        })),
      },
    ],
  }
}

/** Sixteenth-note steps in one beat of the time signature. */
export function stepsPerBeat(signature: TimeSignature): number {
  return Math.max(1, Math.round(16 / signature.denominator))
}

export function stepsPerBar(signature: TimeSignature): number {
  return stepsPerBeat(signature) * signature.numerator
}

export type RulerMark = {
  step: number
  /** The bar number on a bar line, "bar.beat" on the beats inside a bar. */
  label: string
  bar: boolean
}

/** One mark per beat. Bars and beats count from 1. */
export function rulerMarks(
  lengthSteps: number,
  signature: TimeSignature,
  timeline?: Timeline
): RulerMark[] {
  if (timeline?.meters.length) {
    const marks: RulerMark[] = []
    const length = lengthSteps * TICKS_PER_STEP
    for (const segment of meterSegments(signature, timeline.meters)) {
      const beat = ticksPerBeat(segment.signature)
      const bar = ticksPerBar(segment.signature)
      for (let tick = segment.start; tick < Math.min(length, segment.end); tick += beat) {
        const offset = tick - segment.start
        const number = segment.bar + Math.floor(offset / bar)
        const downbeat = offset % bar === 0
        marks.push({ step: tick / TICKS_PER_STEP, label: downbeat ? String(number) : `${number}.${Math.floor((offset % bar) / beat) + 1}`, bar: downbeat })
      }
    }
    return marks
  }
  const beat = stepsPerBeat(signature)
  const bar = stepsPerBar(signature)
  const marks: RulerMark[] = []
  for (let step = 0; step < lengthSteps; step += beat) {
    const inBar = step % bar
    const number = Math.floor(step / bar) + 1
    marks.push(
      inBar === 0
        ? { step, label: String(number), bar: true }
        : { step, label: `${number}.${inBar / beat + 1}`, bar: false }
    )
  }
  return marks
}

/** Saved meter changes can begin between step columns. Shade at each onset. */
export function beatShades(lengthSteps: number, signature: TimeSignature, timeline?: Timeline): readonly boolean[] | undefined {
  if (!timeline?.meters.length) return undefined
  const segments = meterSegments(signature, timeline.meters)
  return Array.from({ length: lengthSteps }, (_, step) => {
    const tick = step * TICKS_PER_STEP
    const segment = segments.findLast((item) => item.start <= tick) ?? segments[0]
    return Math.floor((tick - segment.start) / ticksPerBeat(segment.signature)) % 2 === 1
  })
}

/** "1 bar", "2 bars", "1 bar and 4 steps", "12 steps". */
export function describeLength(
  lengthSteps: number,
  signature: TimeSignature,
  timeline?: Timeline
): string {
  if (timeline?.meters.length) {
    const position = musicalPosition(lengthSteps * TICKS_PER_STEP, signature, timeline.meters)
    if (position.beat === 1 && position.tick === 0) return message("{shown} bars", { count: position.bar - 1, shown: plainNumber(position.bar - 1) })
    return position.tick
      ? message("ends at {bar}.{beat} + {tick} ticks", { bar: plainNumber(position.bar), beat: plainNumber(position.beat), tick: plainNumber(position.tick) })
      : message("ends at {bar}.{beat}", { bar: plainNumber(position.bar), beat: plainNumber(position.beat) })
  }
  const bar = stepsPerBar(signature)
  const bars = Math.floor(lengthSteps / bar)
  const rest = lengthSteps % bar
  const stepsText = message("{shown} steps", { count: rest, shown: plainNumber(rest) })
  if (bars === 0) return stepsText
  const barsText = message("{shown} bars", { count: bars, shown: plainNumber(bars) })
  return rest === 0 ? barsText : message("{barsText} and {stepsText}", { barsText, stepsText })
}

export function clampPatternLength(steps: number): number {
  return clamp(Math.round(steps), 1, MAX_PATTERN_STEPS)
}

/**
 * The index to give `moveChannel` when the row at `from` is dropped into the
 * gap before row `gap` (0 is above the first row, `count` below the last).
 * Null when the row would stay where it is.
 */
export function moveIndex(
  from: number,
  gap: number,
  count: number
): number | null {
  const target = clamp(gap > from ? gap - 1 : gap, 0, Math.max(0, count - 1))
  return target === from ? null : target
}

/**
 * Splits a tuning into whole semitones and the cents left over, from -50
 * to 50. A tuning exactly between two semitones reads either way: 0.5 is
 * +50 cents on 0, and -50 cents on 1. `prefer` is the semitone on show,
 * which keeps the tuning for as long as it is within 50 cents of it, so
 * Fine can be turned to either end without Tune jumping to the neighbour.
 * With nothing to prefer, the half goes to the semitone nearer zero.
 */
export function splitTune(
  tune: number,
  prefer?: number
): { semitones: number; cents: number } {
  const split = (semitones: number) => ({
    // Plain zero, never the negative one rounding can leave.
    semitones: semitones + 0,
    cents: Math.round((tune - semitones) * 100) + 0,
  })
  if (prefer !== undefined && Math.abs(split(prefer).cents) <= 50) {
    return split(prefer)
  }
  const nearest = Math.round(tune)
  const halfway = Math.abs(tune - nearest) === 0.5
  return split(halfway ? Math.trunc(tune) : nearest)
}

export function joinTune(semitones: number, cents: number, max: number) {
  return clamp(semitones + cents / 100, -max, max)
}
