import { describe, expect, it } from "vitest"
import {
  declick,
  fade,
  gain,
  insertSilence,
  invertPhase,
  normalizeTo,
  operationName,
  reduceNoise,
  stretch,
  trimSilence,
} from "./tools"

describe("audio editor tool requests", () => {
  it("sends de-click strength as a fraction and refuses what is out of range", () => {
    expect(declick(50)).toEqual({ declick: { strength: 0.5 } })
    expect(declick(0)).toEqual({ declick: { strength: 0 } })
    expect(declick(100)).toEqual({ declick: { strength: 1 } })
    expect(declick(101)).toBeNull()
    expect(declick(-1)).toBeNull()
    expect(declick(Number.NaN)).toBeNull()
  })

  it("sends noise reduction with its captured profile, or null for automatic", () => {
    expect(reduceNoise(12, null)).toEqual({
      reduceNoise: { reductionDb: 12, profile: null },
    })
    expect(reduceNoise(20, { start: 4, end: 900 })).toEqual({
      reduceNoise: {
        reductionDb: 20,
        profile: { startFrame: 4, endFrame: 900 },
      },
    })
    expect(reduceNoise(0, null)).toBeNull()
    expect(reduceNoise(48.5, null)).toBeNull()
  })

  it("keeps thresholds, gains and targets inside what the editor accepts", () => {
    expect(trimSilence(-60)).toEqual({ trimSilence: { thresholdDb: -60 } })
    expect(trimSilence(-5)).toBeNull()
    expect(trimSilence(-121)).toBeNull()
    expect(gain(-6)).toEqual({ gain: { db: -6 } })
    expect(gain(0)).toBeNull()
    expect(gain(97)).toBeNull()
    expect(normalizeTo(-18, "rms")).toEqual({
      normalizeTo: { targetDb: -18, mode: "rms" },
    })
    expect(normalizeTo(0.5, "peak")).toBeNull()
    expect(normalizeTo(-61, "peak")).toBeNull()
  })

  it("names fades and phase inversion by their parameters", () => {
    expect(fade("out", "sCurve")).toEqual({
      fade: { direction: "out", shape: "sCurve" },
    })
    expect(invertPhase("left")).toEqual({ invertPhase: { channels: "left" } })
  })

  it("sends stretch as a ratio and refuses a stretch that changes nothing", () => {
    expect(stretch(150, 0)).toEqual({
      stretch: {
        ratio: 1.5,
        semitones: 0,
        quality: "standard",
        formants: false,
      },
    })
    expect(stretch(100, -3)).toEqual({
      stretch: {
        ratio: 1,
        semitones: -3,
        quality: "standard",
        formants: false,
      },
    })
    expect(stretch(100, 0)).toBeNull()
    expect(stretch(24, 0)).toBeNull()
    expect(stretch(401, 0)).toBeNull()
    expect(stretch(100, 25)).toBeNull()
  })

  it("turns milliseconds of silence into whole frames at the clip's rate", () => {
    expect(insertSilence(100, 48_000)).toEqual({
      insertSilence: { frames: 4800 },
    })
    expect(insertSilence(1, 44_100)).toEqual({ insertSilence: { frames: 44 } })
    expect(insertSilence(0, 48_000)).toBeNull()
    expect(insertSilence(0.001, 100)).toBeNull()
    expect(insertSilence(Number.NaN, 48_000)).toBeNull()
  })

  it("names an operation the way its request does", () => {
    expect(operationName("removeDc")).toBe("removeDc")
    expect(operationName({ gain: { db: 3 } })).toBe("gain")
  })
})
