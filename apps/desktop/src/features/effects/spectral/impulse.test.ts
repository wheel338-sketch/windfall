import { describe, expect, it } from "vitest"
import { linearPhaseImpulse, readImpulseWav } from "./impulse"
import { wav } from "./wav-fixture"

describe("portable IR control-side preparation", () => {
  it("loads mono and independent stereo without normalizing", () => {
    expect(readImpulseWav(wav([1, .5]), "Own IR.wav").frames).toEqual([[1, 1], [.5, .5]])
    expect(readImpulseWav(wav([1, .25, -.5, 0], 2), "Stereo.wav").frames).toEqual([[1, .25], [-.5, 0]])
  })
  it("rejects damaged, nonfinite, oversized and partial PCM", () => {
    expect(() => readImpulseWav(new ArrayBuffer(9), "bad")).toThrow()
    expect(() => readImpulseWav(wav([NaN]), "bad")).toThrow()
    expect(() => readImpulseWav(wav(Array(65537).fill(0)), "long")).toThrow(/65536/)
    expect(() => readImpulseWav(wav([1], 2), "partial")).toThrow(/alignment/)
  })
  it("generates an exact delayed identity and symmetric nonneutral FIR EQ", () => {
    const neutral = linearPhaseImpulse(0, 0, 0)
    expect(neutral.frames[512]).toEqual([1, 1])
    expect(neutral.frames.filter((frame) => frame[0] !== 0)).toHaveLength(1)
    const eq = linearPhaseImpulse(6, -3, 2)
    eq.frames.forEach((frame, i) => expect(Math.abs(frame[0] - eq.frames[1024 - i][0])).toBeLessThan(1e-12))
    expect(eq.frames).not.toEqual(neutral.frames)
    expect(() => linearPhaseImpulse(NaN, 0, 0)).toThrow()
  })
})
