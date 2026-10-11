import { describe, expect, it } from "vitest"

import {
  describeBands,
  SEVEN_BAND_HZ,
  SEVEN_BAND_Q,
  sevenBandSections,
} from "./graphic/seven-band"
import {
  cycleSeconds,
  lfoWave,
  motionGain,
  motionPan,
  panReach,
} from "./motion/lfo"
import {
  bandPass,
  curveExtreme,
  IDENTITY,
  notch,
  responseGrid,
  seriesGainAt,
  seriesResponseDb,
} from "./response/curve"
import { describeOffset, fieldExtent } from "./stereo/stereo"

/*
 * The arithmetic behind the editors' pictures, against references written
 * here: the analog prototype of each section at the frequency the bilinear
 * transform maps a digital frequency to.
 */

const RATE = 48_000

/** Analog frequency, relative to the corner, of a digital frequency. */
function warped(hz: number, cornerHz: number): number {
  return Math.tan((Math.PI * hz) / RATE) / Math.tan((Math.PI * cornerHz) / RATE)
}

function db(power: number): number {
  return 10 * Math.log10(power)
}

/** |H|^2 of the bell `(s^2 + sA/Q + 1) / (s^2 + s/(AQ) + 1)` at s = jw. */
function bellPower(w: number, q: number, gainDb: number): number {
  const a = 10 ** (gainDb / 40)
  const real = (1 - w * w) ** 2
  return (real + ((w * a) / q) ** 2) / (real + (w / (a * q)) ** 2)
}

describe("response sections", () => {
  it("passes the center of a band-pass at unity and follows its skirts", () => {
    for (const q of [0.7, 2, 8]) {
      const section = bandPass(1_000, q, RATE)
      expect(seriesGainAt([section], 1_000, RATE)).toBeCloseTo(0, 6)
      for (const hz of [100, 500, 2_000, 9_000]) {
        const w = warped(hz, 1_000)
        const power = (w / q) ** 2 / ((1 - w * w) ** 2 + (w / q) ** 2)
        expect(seriesGainAt([section], hz, RATE)).toBeCloseTo(db(power), 5)
      }
    }
  })

  it("removes the center of a notch and leaves the rest", () => {
    const section = notch(2_000, 4, RATE)
    expect(seriesGainAt([section], 2_000, RATE)).toBeLessThan(-100)
    for (const hz of [60, 1_000, 3_000, 15_000]) {
      const w = warped(hz, 2_000)
      const power = (1 - w * w) ** 2 / ((1 - w * w) ** 2 + (w / 4) ** 2)
      expect(seriesGainAt([section], hz, RATE)).toBeCloseTo(db(power), 5)
    }
  })

  it("adds sections in series and skips the ones that change nothing", () => {
    const grid = responseGrid([100, 1_000, 10_000], RATE)
    expect([...seriesResponseDb([IDENTITY, IDENTITY], grid, 3)]).toEqual([
      3, 3, 3,
    ])
    const one = seriesResponseDb([bandPass(1_000, 1, RATE)], grid)
    const two = seriesResponseDb(
      [bandPass(1_000, 1, RATE), bandPass(1_000, 1, RATE)],
      grid
    )
    for (let index = 0; index < 3; index += 1) {
      expect(two[index]).toBeCloseTo(2 * one[index], 4)
    }
  })

  it("finds the largest swing of a curve", () => {
    expect(curveExtreme([0, 2, -7, 3], [10, 20, 30, 40])).toEqual({
      gainDb: -7,
      frequencyHz: 30,
    })
    expect(curveExtreme([], [])).toEqual({ gainDb: 0, frequencyHz: 0 })
  })
})

describe("the Seven Band curve", () => {
  it("is seven bells at the fixed centers with one Q", () => {
    const gains = [6, -4, 3, -9, 12, -6, 5]
    const sections = sevenBandSections(gains, RATE)
    for (const hz of [40, 160, 700, 1_000, 3_300, 12_000]) {
      const expected = SEVEN_BAND_HZ.reduce(
        (sum, center, index) =>
          sum + db(bellPower(warped(hz, center), SEVEN_BAND_Q, gains[index])),
        0
      )
      expect(seriesGainAt(sections, hz, RATE)).toBeCloseTo(expected, 4)
    }
  })

  it("leaves a flat band out and says so", () => {
    expect(sevenBandSections([0, 0, 0, 0, 0, 0, 0], RATE)).toEqual(
      SEVEN_BAND_HZ.map(() => IDENTITY)
    )
    expect(describeBands(SEVEN_BAND_HZ, [0, 0, 0, 0, 0, 0, 0])).toBe("flat")
    expect(describeBands([100, 2_000], [3, -6])).toBe(
      "100 Hz +3.0 dB, 2.00 kHz −6.0 dB"
    )
  })
})

describe("the pan motion", () => {
  it("draws each shape between -1 and 1 from its start", () => {
    expect(lfoWave("sine", 0.25)).toBeCloseTo(1, 9)
    expect(lfoWave("sine", 0.75)).toBeCloseTo(-1, 9)
    for (const phase of [0, 0.1, 0.25, 0.4, 0.6, 0.75, 0.9]) {
      // A triangle is the arcsine of a sine, scaled to reach 1.
      const expected = (Math.asin(Math.sin(2 * Math.PI * phase)) * 2) / Math.PI
      expect(lfoWave("triangle", phase)).toBeCloseTo(expected, 9)
      expect(lfoWave("saw", phase)).toBeCloseTo(2 * phase - 1, 9)
      expect(lfoWave("square", phase)).toBe(phase < 0.5 ? 1 : -1)
    }
    // One level for a whole cycle, another for the next.
    expect(lfoWave("random", 0.1)).toBe(lfoWave("random", 0.9))
    expect(lfoWave("random", 0.1)).not.toBe(lfoWave("random", 1.1))
    expect(lfoWave("saw", 3.25)).toBeCloseTo(-0.5, 9)
  })

  it("turns a wave into pan and gain the way the engine does", () => {
    const settings = { pan: 0.5, depth: 1, gain: 0.8, volumeDepth: 0.5 }
    expect(motionPan(settings, 1)).toBe(1)
    expect(motionPan(settings, -1)).toBe(-0.5)
    expect(panReach(settings)).toEqual([-0.5, 1])
    expect(motionGain(settings, -1)).toBeCloseTo(0.8, 9)
    expect(motionGain(settings, 1)).toBeCloseTo(0.4, 9)
    expect(motionGain(settings, 0)).toBeCloseTo(0.6, 9)
  })

  it("times a cycle from the rate, or from the tempo when synced", () => {
    expect(cycleSeconds({ sync: false, beats: 4, rateHz: 2 }, 120)).toBe(0.5)
    expect(cycleSeconds({ sync: true, beats: 2, rateHz: 9 }, 120)).toBe(1)
    expect(cycleSeconds({ sync: true, beats: 2, rateHz: 9 }, 60)).toBe(2)
  })
})

describe("the stereo picture", () => {
  it("grows from the center with width and leans with balance", () => {
    expect(fieldExtent(0)).toEqual({ left: 50, right: 50 })
    expect(fieldExtent(1)).toEqual({ left: 25, right: 75 })
    expect(fieldExtent(2)).toEqual({ left: 0, right: 100 })
    expect(fieldExtent(9)).toEqual({ left: 0, right: 100 })
    // Favoring the right turns the left down.
    expect(fieldExtent(2, 1)).toEqual({ left: 50, right: 100 })
    expect(fieldExtent(2, -0.5)).toEqual({ left: 0, right: 75 })
  })

  it("names the side a phase offset holds back", () => {
    expect(describeOffset(0)).toBe("Both sides in time")
    expect(describeOffset(5)).toBe("Right side 5 ms late")
    expect(describeOffset(-2.5)).toBe("Left side 2.5 ms late")
  })
})
