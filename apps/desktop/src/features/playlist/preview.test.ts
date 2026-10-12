import { describe, expect, it } from "vitest"

import type { Note, Pattern } from "@/bindings"

import {
  buildPreview,
  firstPassStart,
  loopPoints,
  previewOf,
  previewSpans,
  SEGMENT_STRIDE,
  type ClipSpan,
  type PatternPreview,
} from "./preview"

const BAR = 3840
const STEP = 240

it("shows event-only patterns as bounded silhouettes at their real source ticks", () => {
  const source = pattern([])
  source.events = [{ id: 9, target: { type: "trackPan", track: 0 }, points: [{ tick: 0, value: 0, curve: 0, hold: false }, { tick: BAR, value: 1, curve: 0, hold: false }] }]
  const preview = buildPreview(source)
  const segments = rows(preview)
  expect(preview.count).toBe(32)
  expect(segments[0][0]).toBe(0)
  expect(segments.at(-1)![0] + segments.at(-1)![1]).toBe(BAR)
  expect(segments[0][2]).toBeGreaterThan(segments.at(-1)![2])
})

let nextNote = 1
function note(start: number, key = 60, length = STEP): Note {
  return { id: nextNote++, start, length, key, velocity: 0.8, pan: 0 }
}

function pattern(lanes: Note[][], lengthSteps = 16): Pattern {
  return {
    id: 1,
    name: "Pattern",
    color: 0xe5484d,
    lengthSteps,
    lanes: lanes.map((notes, index) => ({ channel: index + 1, notes })),
  }
}

/** The segments as [start, length, top, height] rows. */
function rows(preview: PatternPreview): number[][] {
  const out: number[][] = []
  for (let index = 0; index < preview.count; index++) {
    out.push([
      ...preview.segments.slice(
        index * SEGMENT_STRIDE,
        (index + 1) * SEGMENT_STRIDE
      ),
    ])
  }
  return out
}

function spans(preview: PatternPreview, clip: ClipSpan, from = 0, to = 1e9) {
  const out: [number, number][] = []
  const count = previewSpans(preview, clip, from, to, (start, end) =>
    out.push([start, end])
  )
  expect(count).toBe(out.length)
  return out
}

describe("buildPreview", () => {
  it("gives each drum its own line", () => {
    const preview = buildPreview(
      pattern([[note(0), note(4 * STEP)], [note(2 * STEP)]])
    )
    expect(preview.length).toBe(BAR)
    expect(rows(preview)).toEqual([
      [0, STEP, 0, 0.5],
      [2 * STEP, STEP, 0.5, 0.5],
      [4 * STEP, STEP, 0, 0.5],
    ])
  })

  it("lays a melody out by pitch, highest on top", () => {
    const preview = buildPreview(
      pattern([[note(0, 60), note(STEP, 64), note(2 * STEP, 62)]])
    )
    const [low, high, middle] = rows(preview)
    expect(high[2]).toBe(0)
    expect(low[2]).toBeCloseTo(4 / 5)
    expect(middle[2]).toBeCloseTo(2 / 5)
    expect(low[3]).toBeCloseTo(1 / 5)
  })

  it("gives a wide-ranging channel more room than a drum", () => {
    const preview = buildPreview(
      pattern([[note(0, 60)], [note(0, 48), note(STEP, 50)]])
    )
    const heights = rows(preview).map((row) => row[3])
    expect(heights.every((height) => Math.abs(height - 1 / 4) < 1e-6)).toBe(
      true
    )
    // The drum has the top quarter, the melody the three below it.
    expect(rows(preview).map((row) => row[2])).toEqual([0, 0.75, 0.25])
  })

  it("leaves out notes that start after the pattern ends", () => {
    const preview = buildPreview(pattern([[note(0), note(BAR), note(2 * BAR)]]))
    expect(preview.count).toBe(1)
  })

  it("cuts a note that runs past the end of the pattern", () => {
    const preview = buildPreview(pattern([[note(BAR - STEP, 60, 4 * STEP)]]))
    expect(rows(preview)[0][1]).toBe(STEP)
  })

  it("is empty for an empty pattern", () => {
    expect(buildPreview(pattern([])).count).toBe(0)
    expect(buildPreview(pattern([[]])).count).toBe(0)
  })

  it("draws a busy pattern coarsely instead of note by note", () => {
    const notes: Note[] = []
    for (let step = 0; step < 64; step++) {
      for (let key = 40; key < 80; key += 2) notes.push(note(step * STEP, key))
    }
    const preview = buildPreview(pattern([notes], 64), 192)
    expect(notes.length).toBeGreaterThan(192)
    expect(preview.count).toBeLessThanOrEqual(192)
    expect(preview.count).toBeGreaterThan(0)
    const starts = rows(preview).map((row) => row[0])
    expect([...starts].sort((a, b) => a - b)).toEqual(starts)
    for (const [start, length, top, height] of rows(preview)) {
      expect(start + length).toBeLessThanOrEqual(64 * STEP)
      expect(top + height).toBeLessThanOrEqual(1 + 1e-6)
    }
  })

  it("is worked out once per pattern object", () => {
    const first = pattern([[note(0)]])
    expect(previewOf(first)).toBe(previewOf(first))
    expect(previewOf({ ...first })).not.toBe(previewOf(first))
  })
})

describe("loop points", () => {
  it("finds where the first pass began, before the clip when it has an offset", () => {
    expect(
      firstPassStart({ start: 2 * BAR, length: BAR, offset: 0 }, BAR)
    ).toBe(2 * BAR)
    expect(
      firstPassStart({ start: 2 * BAR, length: BAR, offset: 960 }, BAR)
    ).toBe(2 * BAR - 960)
    expect(
      firstPassStart({ start: 2 * BAR, length: BAR, offset: BAR + 960 }, BAR)
    ).toBe(2 * BAR - 960)
  })

  it("has none in a clip that plays the pattern once", () => {
    expect(loopPoints({ start: BAR, length: BAR, offset: 0 }, BAR)).toEqual([])
  })

  it("has one at every repeat of a longer clip", () => {
    expect(loopPoints({ start: BAR, length: 3 * BAR, offset: 0 }, BAR)).toEqual(
      [2 * BAR, 3 * BAR]
    )
  })

  it("moves with the offset", () => {
    expect(
      loopPoints({ start: BAR, length: 2 * BAR, offset: 960 }, BAR)
    ).toEqual([2 * BAR - 960, 3 * BAR - 960])
  })

  it("only reports the ones in view", () => {
    const long = { start: 0, length: 1000 * BAR, offset: 0 }
    expect(loopPoints(long, BAR, 500 * BAR - 10, 502 * BAR + 10)).toEqual([
      500 * BAR,
      501 * BAR,
      502 * BAR,
    ])
  })
})

describe("previewSpans", () => {
  const kick = buildPreview(pattern([[note(0), note(8 * STEP)]]))

  it("places one pass of notes at the clip's start", () => {
    expect(spans(kick, { start: 2 * BAR, length: BAR, offset: 0 })).toEqual([
      [2 * BAR, 2 * BAR + STEP],
      [2 * BAR + 8 * STEP, 2 * BAR + 9 * STEP],
    ])
  })

  it("repeats the pattern for as long as the clip is", () => {
    const starts = spans(kick, { start: 0, length: 2 * BAR + 960, offset: 0 })
    expect(starts.map(([start]) => start)).toEqual([
      0,
      8 * STEP,
      BAR,
      BAR + 8 * STEP,
      2 * BAR,
    ])
  })

  it("starts inside the pattern when the clip has an offset", () => {
    // Four steps in: the first kick is behind the clip's start.
    expect(spans(kick, { start: BAR, length: BAR, offset: 4 * STEP })).toEqual([
      [BAR + 4 * STEP, BAR + 5 * STEP],
      [BAR + 12 * STEP, BAR + 13 * STEP],
    ])
  })

  it("cuts notes at the clip's edges", () => {
    const half = { start: BAR, length: 120, offset: 0 }
    expect(spans(kick, half)).toEqual([[BAR, BAR + 120]])
    // A clip that starts halfway through the first kick shows its tail.
    expect(spans(kick, { start: BAR, length: BAR, offset: 120 })[0]).toEqual([
      BAR,
      BAR + 120,
    ])
  })

  it("only visits the part of a long clip that is in view", () => {
    const long = { start: 0, length: 5000 * BAR, offset: 0 }
    const seen = spans(kick, long, 3000 * BAR, 3001 * BAR)
    expect(seen).toEqual([
      [3000 * BAR, 3000 * BAR + STEP],
      [3000 * BAR + 8 * STEP, 3000 * BAR + 9 * STEP],
    ])
  })

  it("passes on where each note sits in the clip's height", () => {
    const two = buildPreview(pattern([[note(0)], [note(0)]]))
    const tops: number[] = []
    previewSpans(two, { start: 0, length: BAR, offset: 0 }, 0, BAR, (...args) =>
      tops.push(args[2])
    )
    expect(tops).toEqual([0, 0.5])
  })

  it("draws nothing for an empty pattern or outside the clip", () => {
    const empty = buildPreview(pattern([]))
    expect(spans(empty, { start: 0, length: BAR, offset: 0 })).toEqual([])
    expect(
      spans(kick, { start: 0, length: BAR, offset: 0 }, 2 * BAR, 3 * BAR)
    ).toEqual([])
  })
})
