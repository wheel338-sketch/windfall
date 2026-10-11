import { describe, expect, it } from "vitest"

import { MAX_PATTERN_TICKS } from "@/lib/units"

import { chordInsertCommand } from "./command"
import { detectChord, pitchClass } from "./detection"
import {
  advanceSeed,
  MAX_SEED,
  MIN_SEED,
  seededTriad,
  triadKeys,
} from "./generator"

describe("pitch-class chord detection", () => {
  it.each([
    [[60, 64, 67], "C major", "major"],
    [[69, 72, 76], "A minor", "minor"],
    [[71, 74, 77], "B diminished", "diminished"],
    [[60, 64, 68], "C augmented", "augmented"],
    [[60, 62, 67], "C suspended second", "sus2"],
    [[60, 65, 67], "C suspended fourth", "sus4"],
  ])("recognizes %j", (keys, label, quality) => {
    expect(detectChord(keys as number[])).toMatchObject({
      kind: "triad",
      label,
      quality,
    })
  })

  it("ignores input order and octave doublings without mutating input", () => {
    const keys = [79, 48, 72, 64, 60]
    expect(detectChord(keys)).toEqual(detectChord([60, 64, 67]))
    expect(keys).toEqual([79, 48, 72, 64, 60])
  })

  it("keeps the triad of an inversion and names its bass", () => {
    expect(detectChord([79, 52, 72, 64, 60])).toMatchObject({
      kind: "triad",
      label: "C major / E",
      symbol: "C/E",
      root: 0,
      bass: 4,
      quality: "major",
    })
  })

  it("detects transposed major and minor triads in every key", () => {
    for (let root = 48; root < 60; root++) {
      for (const quality of ["major", "minor"] as const) {
        expect(detectChord(triadKeys(root, quality))).toMatchObject({
          root: pitchClass(root),
          quality,
        })
      }
    }
  })

  it("uses a deterministic root for symmetric and suspended ambiguities", () => {
    // A symmetric triad is rooted on its lowest key.
    expect(detectChord([68, 64, 72])).toMatchObject({
      root: 4,
      quality: "augmented",
    })
    expect(detectChord([68, 64, 60])).toMatchObject({
      root: 0,
      quality: "augmented",
    })
    expect(detectChord([67, 62, 60])).toEqual(detectChord([60, 62, 67]))
  })

  it("names empty and unmatched selections, and keeps triad quality for triads", () => {
    expect(detectChord([])).toMatchObject({
      kind: "empty",
      label: "No notes selected",
    })
    expect(detectChord([60, 61, 67])).toMatchObject({
      kind: "unknown",
      label: "Unknown chord (C, C♯, G)",
    })
    expect(detectChord([60, 64, 67, 71])).toMatchObject({
      kind: "chord",
      quality: null,
    })
    expect(detectChord([60, 72])).toMatchObject({ kind: "note", quality: null })
  })
})

describe("seeded diatonic triads", () => {
  it("has stable notes for a seed and key, independent of intervening calls", () => {
    expect(seededTriad(0, 60)).toEqual({
      degree: "I",
      root: 60,
      quality: "major",
      keys: [60, 64, 67],
    })
    const first = seededTriad(-43, 62)
    seededTriad(1234, 65)
    expect(seededTriad(-43, 62)).toEqual(first)
    expect(seededTriad(0, 62).keys).not.toEqual(seededTriad(0, 60).keys)
  })

  it("visits seven diatonic choices and changes notes on every next seed", () => {
    for (const tonic of [0, 60, 61, 110]) {
      const choices = Array.from({ length: 7 }, (_, seed) =>
        seededTriad(seed, tonic)
      )
      expect(new Set(choices.map((choice) => choice.degree)).size).toBe(7)
      for (const choice of choices) {
        expect(
          choice.keys.every((key) =>
            [0, 2, 4, 5, 7, 9, 11].includes(pitchClass(key - tonic))
          )
        ).toBe(true)
        expect(choice.keys.every((key) => key >= 0 && key <= 127)).toBe(true)
      }
      for (const seed of [MIN_SEED, -1, 0, 1, 6, MAX_SEED]) {
        expect(seededTriad(advanceSeed(seed), tonic).keys).not.toEqual(
          seededTriad(seed, tonic).keys
        )
      }
    }
  })

  it("rejects invalid seed, tonic and overflowing manual roots", () => {
    for (const seed of [NaN, 0.5, Infinity, MAX_SEED + 1])
      expect(() => seededTriad(seed, 60)).toThrow()
    for (const tonic of [-1, 60.5, 111])
      expect(() => seededTriad(0, tonic)).toThrow()
    expect(() => triadKeys(121, "major")).toThrow(/Lower the root/)
  })
})

const target = {
  pattern: {
    id: 9,
    lengthSteps: 16,
    signature: { numerator: 4, denominator: 4 },
  },
  channel: 3,
}

describe("chord command payload", () => {
  it("inserts all three notes in one existing addNotes command", () => {
    expect(chordInsertCommand(target, [60, 64, 67], 240, 960)).toEqual({
      type: "addNotes",
      pattern: 9,
      channel: 3,
      notes: [60, 64, 67].map((key) => ({
        key,
        start: 240,
        length: 960,
        velocity: 0.8,
        pan: 0,
      })),
    })
  })

  it("includes pattern growth in the same undo command", () => {
    const command = chordInsertCommand(
      target,
      seededTriad(1, 60).keys,
      3840,
      960
    )
    expect(command).toMatchObject({
      type: "batch",
      label: "Insert chord",
      commands: [
        {
          type: "addNotes",
          pattern: 9,
          channel: 3,
          notes: [{ key: 69 }, { key: 72 }, { key: 76 }],
        },
        { type: "updatePattern", id: 9, patch: { lengthSteps: 32 } },
      ],
    })
  })

  it("rejects invalid notes and timing without clamping or shortening", () => {
    for (const keys of [
      [60, 64],
      [60, 60, 67],
      [60, 64, 128],
      [60, 64.5, 67],
    ]) {
      expect(() => chordInsertCommand(target, keys, 0, 240)).toThrow()
    }
    for (const [start, length] of [
      [-1, 240],
      [0.5, 240],
      [0, 0],
      [0, 1.5],
      [NaN, 240],
      [MAX_PATTERN_TICKS, 1],
    ]) {
      expect(() =>
        chordInsertCommand(target, [60, 64, 67], start, length)
      ).toThrow()
    }
    expect(() =>
      chordInsertCommand(target, [60, 64, 67], MAX_PATTERN_TICKS - 1, 1)
    ).not.toThrow()
  })
})
