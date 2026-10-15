import { describe, expect, it } from "vitest"

import type { Command, Note, TimeSignature } from "@/bindings"

import {
  cellsBetween,
  copyNotes,
  duplicateRight,
  endAfterUpdates,
  extendedLengthSteps,
  extensionSteps,
  initsFit,
  laneEndAfterUpdates,
  lengthStepsAt,
  MAX_PATTERN_TICKS,
  moveDelta,
  moveLimits,
  moveUpdates,
  movedCopies,
  notesExtent,
  paintSpacing,
  pasteInits,
  pasteStart,
  quantizeEnds,
  quantizeStarts,
  resizeDelta,
  resizeUpdates,
  rowsAlongSegment,
  shrunkLengthSteps,
  updatesFit,
  withExtension,
  withLength,
} from "./edit-math"

const FOUR_FOUR: TimeSignature = { numerator: 4, denominator: 4 }
const SIX_EIGHT: TimeSignature = { numerator: 6, denominator: 8 }

let nextId = 1
function note(start: number, key: number, length = 240, extra = {}): Note {
  return { id: nextId++, start, length, key, velocity: 0.8, pan: 0, ...extra }
}

describe("extent and limits", () => {
  it("boxes notes in time and pitch", () => {
    expect(notesExtent([])).toBeNull()
    expect(
      notesExtent([note(240, 60), note(0, 72, 960), note(480, 48)])
    ).toEqual({ start: 0, end: 960, lowKey: 48, highKey: 72 })
  })

  it("lets a group move until its first note would leave", () => {
    const limits = moveLimits([note(480, 2), note(960, 120)])
    expect(limits).toEqual({ minTicks: -480, minKeys: -2, maxKeys: 7 })
  })
})

describe("moving", () => {
  const limits = { minTicks: -480, minKeys: -12, maxKeys: 12 }

  it("snaps the distance, not the position", () => {
    expect(moveDelta(250, 0, 240, limits)).toEqual({ ticks: 240, keys: 0 })
    expect(moveDelta(119, 0, 240, limits).ticks).toBe(0)
    expect(moveDelta(-130, 3, 240, limits)).toEqual({ ticks: -240, keys: 3 })
  })

  it("rounds to whole ticks with snap off", () => {
    expect(moveDelta(37.6, 0, 0, limits).ticks).toBe(38)
  })

  it("stops at the start of the pattern and the ends of the keyboard", () => {
    expect(moveDelta(-5000, 40, 240, limits)).toEqual({ ticks: -480, keys: 12 })
    expect(moveDelta(0, -40, 240, limits).keys).toBe(-12)
  })

  it("patches only the fields that changed", () => {
    const a = note(480, 60)
    expect(moveUpdates([a], { ticks: 0, keys: 0 })).toEqual([])
    expect(moveUpdates([a], { ticks: 240, keys: 0 })).toEqual([
      { id: a.id, patch: { start: 720 } },
    ])
    expect(moveUpdates([a], { ticks: 0, keys: -2 })).toEqual([
      { id: a.id, patch: { key: 58 } },
    ])
    expect(moveUpdates([a], { ticks: -240, keys: 5 })).toEqual([
      { id: a.id, patch: { start: 240, key: 65 } },
    ])
  })

  it("copies keep length, velocity and pan", () => {
    const a = note(0, 60, 480, { velocity: 0.3, pan: -0.5 })
    expect(movedCopies([a], { ticks: 960, keys: 7 })).toEqual([
      { start: 960, length: 480, key: 67, velocity: 0.3, pan: -0.5 },
    ])
  })

  it("never produces a value the backend would reject", () => {
    const wild = note(0, 127, 240, { velocity: 1.4, pan: -3 })
    const [copy] = movedCopies([wild], { ticks: -50, keys: 9 })
    expect(copy).toEqual({
      start: 0,
      length: 240,
      key: 127,
      velocity: 1,
      pan: -1,
    })
  })
})

describe("resizing", () => {
  it("moves the end by the snapped distance", () => {
    const a = note(0, 60, 480)
    const delta = resizeDelta("end", 250, 240, [a])
    expect(delta).toEqual({ start: 0, end: 240, minLength: 240 })
    expect(resizeUpdates([a], delta)).toEqual([
      { id: a.id, patch: { length: 720 } },
    ])
  })

  it("moves the start and keeps the end", () => {
    const a = note(960, 60, 480)
    const delta = resizeDelta("start", -250, 240, [a])
    expect(resizeUpdates([a], delta)).toEqual([
      { id: a.id, patch: { start: 720, length: 720 } },
    ])
  })

  it("cannot pull a start before tick 0", () => {
    const delta = resizeDelta("start", -5000, 240, [
      note(480, 60),
      note(960, 62),
    ])
    expect(delta.start).toBe(-480)
  })

  it("stops at one snap interval, or one tick with snap off", () => {
    const a = note(0, 60, 480)
    expect(resizeUpdates([a], resizeDelta("end", -9999, 240, [a]))).toEqual([
      { id: a.id, patch: { length: 240 } },
    ])
    expect(resizeUpdates([a], resizeDelta("end", -9999, 0, [a]))).toEqual([
      { id: a.id, patch: { length: 1 } },
    ])
    expect(resizeUpdates([a], resizeDelta("start", 9999, 240, [a]))).toEqual([
      { id: a.id, patch: { start: 240, length: 240 } },
    ])
  })

  it("gives every selected note the same change and skips the unchanged", () => {
    const long = note(0, 60, 960)
    const short = note(0, 64, 240)
    const delta = resizeDelta("end", -480, 240, [long, short])
    expect(resizeUpdates([long, short], delta)).toEqual([
      { id: long.id, patch: { length: 480 } },
    ])
  })
})

describe("resizing onto the grid", () => {
  const BEAT = 960

  it("lands a dragged end on a beat line, not a beat past where it was", () => {
    // A sixteenth stretched by about a beat on the beat grid. Snapping the
    // distance made it 1¼ beats long; the end goes to the beat line instead.
    const a = note(0, 60, 240)
    const delta = resizeDelta("end", 900, BEAT, [a], { anchor: 240, origin: 0 })
    expect(delta.end).toBe(720)
    expect(resizeUpdates([a], delta)).toEqual([
      { id: a.id, patch: { length: BEAT } },
    ])
    // The old rule, without an anchor, keeps the end off the grid.
    expect(resizeUpdates([a], resizeDelta("end", 900, BEAT, [a]))).toEqual([
      { id: a.id, patch: { length: 1200 } },
    ])
  })

  it("rounds to the nearest line and keeps a short note until the next one is nearer", () => {
    const a = note(0, 60, 240)
    expect(resizeUpdates([a], resizeDelta("end", 200, BEAT, [a], { anchor: 240, origin: 0 }))).toEqual([])
    expect(resizeUpdates([a], resizeDelta("end", 250, BEAT, [a], { anchor: 240, origin: 0 }))).toEqual([
      { id: a.id, patch: { length: BEAT } },
    ])
  })

  it("puts a dragged start on a line and keeps the end", () => {
    const a = note(130, 60, 830)
    const delta = resizeDelta("start", -100, 240, [a], { anchor: 130, origin: 0 })
    expect(resizeUpdates([a], delta)).toEqual([
      { id: a.id, patch: { start: 0, length: 960 } },
    ])
  })

  it("counts the lines from the origin of the meter in force", () => {
    const a = note(480, 60, 240)
    // Lines every beat from tick 120: 1080 is the nearest to 720 + 300.
    const delta = resizeDelta("end", 300, BEAT, [a], { anchor: 720, origin: 120 })
    expect(delta.end).toBe(360)
  })

  it("gives the other selected notes the grabbed note's change", () => {
    const grabbed = note(0, 60, 240)
    const other = note(480, 64, 300)
    const delta = resizeDelta("end", 900, BEAT, [grabbed, other], { anchor: 240, origin: 0 })
    expect(resizeUpdates([grabbed, other], delta)).toEqual([
      { id: grabbed.id, patch: { length: 960 } },
      { id: other.id, patch: { length: 1020 } },
    ])
  })

  it("ignores the grid with snap off", () => {
    const a = note(0, 60, 240)
    expect(resizeDelta("end", 333, 0, [a], { anchor: 240, origin: 0 }).end).toBe(333)
  })
})

describe("pattern length that follows the notes", () => {
  const info = { id: 7, lengthSteps: 16, signature: FOUR_FOUR }
  const edit: Command = {
    type: "updateNotes",
    pattern: 7,
    channel: 2,
    updates: [{ id: 1, patch: { length: 480 } }],
  }

  it("grows to the end of the bar a note reaches", () => {
    expect(extensionSteps(info, 3840)).toBeNull()
    expect(extensionSteps(info, 3841)).toBe(32)
    expect(withLength(edit, "Resize notes", 7, 32)).toEqual({
      type: "batch",
      label: "Resize notes",
      commands: [edit, { type: "updatePattern", id: 7, patch: { lengthSteps: 32 } }],
    })
  })

  it("gives back bars no note reaches any more, down to the length before growing", () => {
    // Grown from one bar to two; the last note is back inside the first.
    expect(shrunkLengthSteps(16, 32, 3600, FOUR_FOUR)).toBe(16)
    // Something still reaches into the second bar.
    expect(shrunkLengthSteps(16, 32, 3900, FOUR_FOUR)).toBeNull()
    // Grown from three bars to five, with notes now ending in the second:
    // never shorter than the three bars it had.
    expect(shrunkLengthSteps(48, 80, 4000, FOUR_FOUR)).toBe(48)
    // Grown from one bar to four, with notes ending in the third.
    expect(shrunkLengthSteps(16, 64, 9000, FOUR_FOUR)).toBe(48)
    // In 6/8 a bar is 12 steps.
    expect(shrunkLengthSteps(12, 24, 2000, SIX_EIGHT)).toBe(12)
  })

  it("finds where a lane ends once an update has moved or resized notes", () => {
    const short = note(0, 60, 240)
    const long = note(3000, 62, 1200)
    expect(laneEndAfterUpdates([short, long], [])).toBe(4200)
    expect(
      laneEndAfterUpdates([short, long], [{ id: long.id, patch: { length: 600 } }])
    ).toBe(3600)
    expect(
      laneEndAfterUpdates([short, long], [{ id: long.id, patch: { start: 0 } }])
    ).toBe(1200)
  })
})

describe("quantize", () => {
  it("moves starts to the nearest snap line and keeps lengths", () => {
    const early = note(230, 60, 300)
    const late = note(370, 62)
    const onGrid = note(480, 64)
    expect(quantizeStarts([early, late, onGrid], 240)).toEqual([
      { id: early.id, patch: { start: 240 } },
      { id: late.id, patch: { start: 480 } },
    ])
  })

  it("moves ends to the nearest snap line and never to nothing", () => {
    const loose = note(0, 60, 250)
    const tiny = note(240, 60, 20)
    expect(quantizeEnds([loose, tiny], 240)).toEqual([
      { id: loose.id, patch: { length: 240 } },
      { id: tiny.id, patch: { length: 240 } },
    ])
  })

  it("does nothing with snap off", () => {
    expect(quantizeStarts([note(13, 60)], 0)).toEqual([])
    expect(quantizeEnds([note(13, 60)], 0)).toEqual([])
  })
})

describe("duplicate", () => {
  it("places the copy right after the group, on the snap", () => {
    const { inits, offset } = duplicateRight(
      [note(0, 60, 240), note(480, 64, 300)],
      960
    )
    // The group spans 780 ticks, which rounds up to one beat.
    expect(offset).toBe(960)
    expect(inits.map((init) => [init.start, init.key])).toEqual([
      [960, 60],
      [1440, 64],
    ])
  })

  it("uses the exact length with snap off", () => {
    expect(duplicateRight([note(100, 60, 50)], 0).offset).toBe(50)
  })
})

describe("clipboard", () => {
  it("stores notes relative to the first one", () => {
    const clip = copyNotes([note(960, 60), note(1200, 67, 480)])
    expect(clip?.origin).toBe(960)
    expect(clip?.notes.map((item) => item.start)).toEqual([0, 240])
    expect(copyNotes([])).toBeNull()
  })

  it("pastes at the playhead, snapped", () => {
    const clip = copyNotes([note(960, 60), note(1200, 67)])
    if (!clip) throw new Error("nothing copied")
    const start = pasteStart(
      clip,
      { at: "playhead", tick: 4000 },
      240,
      FOUR_FOUR
    )
    expect(start).toBe(4080)
    expect(pasteInits(clip, start).map((init) => init.start)).toEqual([
      4080, 4320,
    ])
  })

  it("pastes into the first bar in view, at the same place in the bar", () => {
    const clip = copyNotes([note(960, 60)])
    if (!clip) throw new Error("nothing copied")
    const view = (leftTick: number) =>
      pasteStart(clip, { at: "view", leftTick }, 240, FOUR_FOUR)
    expect(view(0)).toBe(960)
    expect(view(1)).toBe(3840 + 960)
    expect(view(7000)).toBe(7680 + 960)
  })
})

describe("pattern length", () => {
  it("grows to the end of the bar a note reaches into", () => {
    expect(extendedLengthSteps(16, 3840, FOUR_FOUR)).toBeNull()
    expect(extendedLengthSteps(16, 3841, FOUR_FOUR)).toBe(32)
    expect(extendedLengthSteps(16, 9000, FOUR_FOUR)).toBe(48)
    expect(extendedLengthSteps(12, 2881, SIX_EIGHT)).toBe(24)
  })

  it("stops at the longest pattern there is", () => {
    expect(extendedLengthSteps(1000, 400_000, FOUR_FOUR)).toBe(1024)
    expect(extendedLengthSteps(1024, 400_000, FOUR_FOUR)).toBeNull()
  })

  it("only counts notes an update moves or resizes", () => {
    const hanging = note(3600, 60, 960)
    expect(
      endAfterUpdates([hanging], [{ id: hanging.id, patch: { velocity: 1 } }])
    ).toBe(0)
    expect(
      endAfterUpdates([hanging], [{ id: hanging.id, patch: { start: 3840 } }])
    ).toBe(4800)
  })

  it("wraps an edit and the longer pattern into one undo step", () => {
    const pattern = { id: 1, lengthSteps: 16, signature: FOUR_FOUR }
    const add: Command = {
      type: "addNotes",
      pattern: 1,
      channel: 2,
      notes: [{ start: 3840, length: 240, key: 60 }],
    }
    expect(withExtension(add, "Add note", pattern, 3000)).toBe(add)
    expect(withExtension(add, "Add note", pattern, 4080)).toEqual({
      type: "batch",
      label: "Add note",
      commands: [
        add,
        { type: "updatePattern", id: 1, patch: { lengthSteps: 32 } },
      ],
    })
  })

  it("reads a ruler drag as whole bars, or whole steps", () => {
    expect(lengthStepsAt(7000, FOUR_FOUR, false)).toBe(32)
    expect(lengthStepsAt(7000, FOUR_FOUR, true)).toBe(29)
    expect(lengthStepsAt(-50, FOUR_FOUR, false)).toBe(16)
    expect(lengthStepsAt(-50, FOUR_FOUR, true)).toBe(1)
    expect(lengthStepsAt(9_999_999, FOUR_FOUR, false)).toBe(1024)
  })
})

describe("the end of the longest pattern", () => {
  const note = (id: number, start: number, length = 240): Note => ({
    id,
    start,
    length,
    key: 60,
    velocity: 0.8,
    pan: 0,
  })

  it("is 1,024 steps in", () => {
    expect(MAX_PATTERN_TICKS).toBe(1024 * 240)
  })

  it("takes new notes up to it and none past it", () => {
    const last = MAX_PATTERN_TICKS - 240
    expect(initsFit([{ start: last, length: 240, key: 60 }])).toBe(true)
    expect(initsFit([{ start: last, length: 241, key: 60 }])).toBe(false)
    expect(
      initsFit([
        { start: 0, length: 240, key: 60 },
        { start: MAX_PATTERN_TICKS, length: 1, key: 62 },
      ])
    ).toBe(false)
  })

  it("lets no update move or stretch a note past it", () => {
    const notes = [note(1, MAX_PATTERN_TICKS - 480)]
    const move = (start: number) => [{ id: 1, patch: { start } }]
    expect(updatesFit(notes, move(MAX_PATTERN_TICKS - 240))).toBe(true)
    expect(updatesFit(notes, move(MAX_PATTERN_TICKS - 239))).toBe(false)
    expect(updatesFit(notes, [{ id: 1, patch: { length: 480 } }])).toBe(true)
    expect(updatesFit(notes, [{ id: 1, patch: { length: 481 } }])).toBe(false)
    // A change that leaves the note's place in time alone always fits.
    expect(updatesFit(notes, [{ id: 1, patch: { velocity: 1 } }])).toBe(true)
  })

  it("lets a note that is already out there come back, but not go further", () => {
    const stray = [note(1, MAX_PATTERN_TICKS + 9600)]
    const move = (start: number) => [{ id: 1, patch: { start } }]
    expect(updatesFit(stray, move(MAX_PATTERN_TICKS + 4800))).toBe(true)
    expect(updatesFit(stray, move(0))).toBe(true)
    expect(updatesFit(stray, move(MAX_PATTERN_TICKS + 9840))).toBe(false)
  })

  it("grows the pattern up to 1,024 steps and no further", () => {
    expect(extendedLengthSteps(16, MAX_PATTERN_TICKS, FOUR_FOUR)).toBe(1024)
    expect(extendedLengthSteps(16, MAX_PATTERN_TICKS * 4, FOUR_FOUR)).toBe(1024)
    expect(
      extendedLengthSteps(1024, MAX_PATTERN_TICKS * 4, FOUR_FOUR)
    ).toBeNull()
  })
})

describe("painting", () => {
  it("spaces notes by the snap, or by their length when that is longer", () => {
    expect(paintSpacing(240, 240)).toBe(240)
    expect(paintSpacing(240, 120)).toBe(240)
    expect(paintSpacing(240, 500)).toBe(720)
    expect(paintSpacing(0, 300)).toBe(300)
  })

  it("lists every cell a drag passed, in either direction", () => {
    expect(cellsBetween(2, 5)).toEqual([2, 3, 4, 5])
    expect(cellsBetween(1, -2)).toEqual([1, 0, -1, -2])
    expect(cellsBetween(3, 3)).toEqual([3])
  })
})

describe("erasing", () => {
  it("covers one row for a level move", () => {
    expect(rowsAlongSegment(500, 10.2, 100, 10.9)).toEqual([
      { row: 10, tick0: 100, tick1: 500 },
    ])
  })

  it("splits a slanted move by row", () => {
    const spans = rowsAlongSegment(0, 10, 300, 13)
    expect(spans.map((span) => span.row)).toEqual([10, 11, 12, 13])
    expect(spans[0]).toEqual({ row: 10, tick0: 0, tick1: 100 })
    expect(spans[2]).toEqual({ row: 12, tick0: 200, tick1: 300 })
  })
})
