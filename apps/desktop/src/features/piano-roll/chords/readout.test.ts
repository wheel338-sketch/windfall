import { afterEach, describe, expect, it, vi } from "vitest"

import * as detection from "./detection"
import {
  chordReadout,
  chordReadoutKeys,
  createChordReadoutReader,
} from "./readout"

const notes = [60, 64, 67].map((key, id) => ({
  id,
  key,
  start: 10,
  length: 10,
}))

afterEach(() => vi.restoreAllMocks())

describe("chord readout", () => {
  it("names selected notes even without a playhead", () => {
    expect(chordReadout(notes, new Set([0, 1, 2]), null)).toBe("C major")
  })

  it("uses the floored playhead and excludes ended and future notes", () => {
    const sounding = notes.map((note) => ({
      ...note,
      key: note.key === 64 ? 63 : note.key,
    }))
    expect(
      chordReadout(
        [
          ...sounding,
          { id: 3, key: 64, start: 0, length: 10 },
          { id: 4, key: 71, start: 11, length: 10 },
        ],
        new Set(),
        10.9
      )
    ).toBe("C minor")
    expect(chordReadout(sounding, new Set(), 20)).toBe("—")
  })

  it("selection wins over sounding notes", () => {
    expect(
      chordReadout(
        [...notes, { id: 3, key: 63, start: 0, length: 10 }],
        new Set([0, 1, 2]),
        5
      )
    ).toBe("C major")
  })

  it("preserves the detector's note, unknown and empty labels when keys exist", () => {
    expect(chordReadout(notes, new Set([0]), null)).toBe("C")
    expect(
      chordReadout(
        [60, 61, 67].map((key, id) => ({ id, key, start: 0, length: 10 })),
        new Set([0, 1, 2]),
        null
      )
    ).toBe("Unknown chord (C, C♯, G)")
    expect(chordReadout([{ ...notes[0], key: NaN }], new Set([0]), null)).toBe(
      "No notes selected"
    )
  })

  it("does not call the detector when there are no keys", () => {
    const detect = vi.spyOn(detection, "detectChord")
    expect(chordReadout(notes, new Set(), null)).toBe("—")
    expect(chordReadout([], new Set(), 10)).toBe("—")
    expect(chordReadout(notes, new Set(), 20)).toBe("—")
    expect(detect).not.toHaveBeenCalled()
  })
})

describe("readout precedence", () => {
  // C major selected, C minor under the playhead, F major held.
  const lane = [
    ...notes,
    { id: 3, key: 60, start: 100, length: 10 },
    { id: 4, key: 63, start: 100, length: 10 },
    { id: 5, key: 67, start: 100, length: 10 },
  ]
  const selection = new Set([0, 1, 2])
  const held = [65, 69, 72]

  it("puts held keys before the selection, the selection before the playhead, then nothing", () => {
    expect(chordReadoutKeys(lane, selection, 105, held)).toEqual({
      source: "played",
      keys: [65, 69, 72],
    })
    expect(chordReadout(lane, selection, 105, held)).toBe("F major")
    expect(chordReadoutKeys(lane, selection, 105, [])).toEqual({
      source: "selected",
      keys: [60, 64, 67],
    })
    expect(chordReadout(lane, selection, 105)).toBe("C major")
    expect(chordReadoutKeys(lane, new Set(), 105)).toEqual({
      source: "playhead",
      keys: [60, 63, 67],
    })
    expect(chordReadout(lane, new Set(), 105)).toBe("C minor")
    expect(chordReadoutKeys(lane, new Set(), 50)).toEqual({
      source: "none",
      keys: [],
    })
    expect(chordReadout(lane, new Set(), 50)).toBe("—")
    expect(chordReadout(lane, new Set(), null)).toBe("—")
  })

  it("names held keys with no notes, no selection and no playhead", () => {
    expect(chordReadoutKeys([], new Set(), null, [64, 67, 72])).toEqual({
      source: "played",
      keys: [64, 67, 72],
    })
    expect(chordReadout([], new Set(), null, [64, 67, 72])).toBe("C major / E")
    expect(chordReadout([], new Set(), null, [60])).toBe("C")
  })

  it("falls through a selection that holds none of the open lane's notes", () => {
    expect(chordReadoutKeys(lane, new Set([99]), 105)).toEqual({
      source: "playhead",
      keys: [60, 63, 67],
    })
    expect(chordReadoutKeys(lane, new Set([99]), null).source).toBe("none")
  })

  it("floors the playhead and ends a note on its last tick", () => {
    const at = (tick: number | null) => chordReadoutKeys(lane, new Set(), tick)
    expect(at(99.99).source).toBe("none")
    expect(at(100).keys).toEqual([60, 63, 67])
    expect(at(109.99).keys).toEqual([60, 63, 67])
    expect(at(110).source).toBe("none")
    // The first chord runs from 10 up to, not including, 20.
    expect(at(9.9).source).toBe("none")
    expect(at(19.9).keys).toEqual([60, 64, 67])
    expect(at(20).source).toBe("none")
    expect(at(-0.5).source).toBe("none")
  })
})

describe("stable readout views", () => {
  it("returns the same view until what it shows changes", () => {
    const read = createChordReadoutReader()
    const lane = [
      ...notes,
      // The same chord an octave up and reordered, then a different one.
      { id: 3, key: 79, start: 20, length: 10 },
      { id: 4, key: 76, start: 20, length: 10 },
      { id: 5, key: 72, start: 20, length: 10 },
      { id: 6, key: 60, start: 30, length: 10 },
      { id: 7, key: 63, start: 30, length: 10 },
      { id: 8, key: 67, start: 30, length: 10 },
    ]
    const none = new Set<number>()
    const empty = read(lane, none, null)
    expect(empty).toEqual({ source: "none", label: "—", detection: null })
    expect(read(lane, none, 5)).toBe(empty)

    const major = read(lane, none, 10)
    expect(major).toMatchObject({ source: "playhead", label: "C major" })
    expect(major.detection).toMatchObject({ symbol: "C", bass: 0 })
    for (const tick of [10.5, 11, 15, 19.99, 20, 25, 29.9]) {
      expect(read(lane, none, tick)).toBe(major)
    }
    const minor = read(lane, none, 30)
    expect(minor).not.toBe(major)
    expect(minor.label).toBe("C minor")

    // The same chord from another source is a different view.
    const selected = read(lane, new Set([6, 7, 8]), 30)
    expect(selected).toMatchObject({ source: "selected", label: "C minor" })
    expect(selected).not.toBe(minor)
    const played = read(lane, new Set([6, 7, 8]), 30, [60, 63, 67])
    expect(played).toMatchObject({ source: "played", label: "C minor" })
    expect(read(lane, new Set([6, 7, 8]), 31, [60, 63, 67, 72])).toBe(played)
    // A new bass is a new view even with the same pitch classes.
    const inverted = read(lane, none, null, [63, 67, 72])
    expect(inverted.label).toBe("C minor / D♯")
    expect(read(lane, none, null)).toBe(empty)
  })

  it("does not run the detector again while the keys stay the same", () => {
    const detect = vi.spyOn(detection, "detectChord")
    const read = createChordReadoutReader()
    for (let tick = 10; tick < 20; tick += 0.25) read(notes, new Set(), tick)
    expect(detect).toHaveBeenCalledTimes(1)
    read(notes, new Set(), 20)
    read(notes, new Set(), null)
    expect(detect).toHaveBeenCalledTimes(1)
  })

  it("keeps readers apart", () => {
    const first = createChordReadoutReader()
    const second = createChordReadoutReader()
    expect(first(notes, new Set(), 10).label).toBe("C major")
    expect(second(notes, new Set(), null).label).toBe("—")
    expect(first(notes, new Set(), 12).label).toBe("C major")
  })
})
