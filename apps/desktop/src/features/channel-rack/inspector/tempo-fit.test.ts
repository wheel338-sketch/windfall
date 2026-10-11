import { describe, expect, it } from "vitest"

import {
  beatsSeconds,
  regionSeconds,
  suggestedBeats,
  tempoFitRatio,
  tempoFitTune,
} from "./tempo-fit"

describe("fitting a sample to the tempo", () => {
  it("measures the trimmed region", () => {
    expect(regionSeconds(4, 0, 1)).toBe(4)
    expect(regionSeconds(4, 0.25, 0.75)).toBe(2)
    expect(regionSeconds(4, 0.75, 0.25)).toBe(0)
    expect(regionSeconds(Number.NaN, 0, 1)).toBe(0)
    expect(regionSeconds(4, Number.NaN, 2)).toBe(4)
  })

  it("turns beats into seconds at a tempo", () => {
    expect(beatsSeconds(4, 120)).toBe(2)
    expect(beatsSeconds(1, 90)).toBeCloseTo(2 / 3)
    expect(beatsSeconds(0, 120)).toBe(0)
    expect(beatsSeconds(4, 0)).toBe(0)
  })

  it("suggests the beat count that changes the sample least", () => {
    // 1.9 s at 120 BPM is 3.8 beats: four is nearest.
    expect(suggestedBeats(1.9, 120)).toBe(4)
    // 0.26 s is about half a beat.
    expect(suggestedBeats(0.26, 120)).toBe(0.5)
    // Three beats is nearer four than two as a ratio.
    expect(suggestedBeats(1.5, 120)).toBe(4)
    expect(suggestedBeats(1000, 120)).toBe(64)
    expect(suggestedBeats(0, 120)).toBe(4)
  })

  it("fits with a duration ratio inside the stretch range", () => {
    // A 1.6 s loop into four beats at 120 BPM: 2 s, a ratio of 1.25.
    expect(tempoFitRatio(1.6, 4, 120)).toBeCloseTo(1.25)
    expect(tempoFitRatio(2, 4, 120)).toBe(1)
    expect(tempoFitRatio(8, 4, 120)).toBe(0.25)
    expect(tempoFitRatio(8.1, 4, 120)).toBeNull()
    expect(tempoFitRatio(0.4, 4, 120)).toBeNull()
    expect(tempoFitRatio(0, 4, 120)).toBeNull()
    expect(tempoFitRatio(2, 0, 120)).toBeNull()
    expect(tempoFitRatio(2, 4, Number.NaN)).toBeNull()
  })

  it("fits tape playback with the tune", () => {
    // Twice too long plays an octave up; half as long, an octave down.
    expect(tempoFitTune(4, 4, 120)).toBeCloseTo(12)
    expect(tempoFitTune(1, 4, 120)).toBeCloseTo(-12)
    expect(tempoFitTune(2, 4, 120)).toBeCloseTo(0)
    // 2 s sped up by the tune lasts 2 / 2^(tune / 12) seconds.
    const tune = tempoFitTune(2.5, 4, 140)
    expect(tune).not.toBeNull()
    expect(2.5 / 2 ** ((tune ?? 0) / 12)).toBeCloseTo(beatsSeconds(4, 140))
    expect(tempoFitTune(2 * 2 ** 5, 4, 120)).toBeNull()
    expect(tempoFitTune(0, 4, 120)).toBeNull()
  })
})
