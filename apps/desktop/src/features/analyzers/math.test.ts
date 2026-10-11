import { describe, expect, it } from "vitest"

import {
  dbPosition,
  formatCents,
  formatDbfs,
  History,
  levelToDb,
  noteName,
  powerToDb,
  spectrumCurve,
  triggerOffset,
} from "./math"

describe("levels", () => {
  it("reads power and level in dB and silence as minus infinity", () => {
    expect(powerToDb(1)).toBe(0)
    expect(powerToDb(0.01)).toBeCloseTo(-20, 9)
    expect(powerToDb(0)).toBe(-Infinity)
    expect(levelToDb(0.5)).toBeCloseTo(-6.0206, 4)
    expect(levelToDb(0)).toBe(-Infinity)
  })

  it("places a level on a meter and holds it to the meter's range", () => {
    expect(dbPosition(-45, -90)).toBeCloseTo(0.5, 9)
    expect(dbPosition(6, -90)).toBe(1)
    expect(dbPosition(-200, -90)).toBe(0)
    expect(dbPosition(-Infinity, -90)).toBe(0)
  })

  it("writes silence and a missing measurement differently", () => {
    expect(formatDbfs(-12.34)).toBe("−12.3 dBFS")
    expect(formatDbfs(0)).toBe("0.0 dBFS")
    expect(formatDbfs(-Infinity)).toBe("−∞ dBFS")
    expect(formatDbfs(null)).toBe("–")
    expect(formatDbfs(undefined)).toBe("–")
  })
})

describe("the oscilloscope trigger", () => {
  it("starts on the first upward zero crossing that leaves a full span", () => {
    // Half a sample off the crossing, so no sample is a rounded zero.
    const wave = Array.from({ length: 64 }, (_, n) =>
      Math.sin((2 * Math.PI * (n + 5.5)) / 16)
    )
    // Sample 11 is the first above zero after a negative one.
    const at = triggerOffset(wave, 32)
    expect(at).toBe(11)
    expect(wave[at - 1]).toBeLessThan(0)
    expect(wave[at]).toBeGreaterThanOrEqual(0)
  })

  it("falls back to the start for silence, for DC and across a bad sample", () => {
    expect(triggerOffset(new Array(64).fill(0), 32)).toBe(0)
    expect(triggerOffset(new Array(64).fill(0.5), 32)).toBe(0)
    expect(triggerOffset([-1, null, 1, 1], 1)).toBe(0)
  })
})

describe("the spectrum curve", () => {
  const bins = 513
  const binHz = 48_000 / 1024

  it("puts a tone's power at the tone's place on the logarithmic axis", () => {
    const left = new Array<number>(bins).fill(0)
    // Bin 21 is 984 Hz.
    left[21] = 0.25
    const curve = spectrumCurve(left, [], binHz, 300, 20, 20_000)
    let peak = 0
    for (let point = 1; point < curve.length; point += 1) {
      if (curve[point] > curve[peak]) peak = point
    }
    const hz = 20 * 1000 ** ((peak + 0.5) / 300)
    expect(hz).toBeGreaterThan(984 / 1.03)
    expect(hz).toBeLessThan(984 * 1.03)
    expect(curve[peak]).toBeCloseTo(powerToDb(0.25), 5)
    expect(curve[0]).toBe(-Infinity)
  })

  it("averages the sides that have a spectrum and skips a side that has none", () => {
    const left = new Array<number>(bins).fill(0.5)
    const right = new Array<number>(bins).fill(0.1)
    const both = spectrumCurve(left, right, binHz, 50, 20, 20_000)
    expect(both[25]).toBeCloseTo(powerToDb(0.3), 5)
    const one = spectrumCurve(left, [], binHz, 50, 20, 20_000)
    expect(one[25]).toBeCloseTo(powerToDb(0.5), 5)
    const none = spectrumCurve([], [], binHz, 50, 20, 20_000)
    expect([...none].every((db) => db === -Infinity)).toBe(true)
  })

  it("draws nothing above half the sample rate", () => {
    const left = new Array<number>(bins).fill(1)
    // At 8 kHz, bins end at 4 kHz.
    const curve = spectrumCurve(left, [], 8_000 / 1024, 100, 20, 20_000)
    expect(curve[0]).toBe(0)
    expect(curve[99]).toBe(-Infinity)
  })
})

describe("the tuner's note", () => {
  it("names notes with middle C as C4", () => {
    expect(noteName(69)).toBe("A4")
    expect(noteName(60)).toBe("C4")
    expect(noteName(61)).toBe("C♯4")
    expect(noteName(0)).toBe("C-1")
    expect(noteName(127)).toBe("G9")
  })

  it("writes cents with a real minus sign", () => {
    expect(formatCents(31.77)).toBe("+31.8 ¢")
    expect(formatCents(-4)).toBe("−4.0 ¢")
    expect(formatCents(0.01)).toBe("0.0 ¢")
  })
})

describe("a bounded history", () => {
  it("keeps the newest entries, oldest first, and no more than its capacity", () => {
    const history = new History(3, 2)
    expect(history.length).toBe(0)
    for (let entry = 1; entry <= 5; entry += 1) history.push([entry, -entry])
    expect(history.length).toBe(3)
    expect([0, 1, 2].map((entry) => history.at(entry, 0))).toEqual([3, 4, 5])
    expect(history.at(2, 1)).toBe(-5)
    history.clear()
    expect(history.length).toBe(0)
  })

  it("stores what an entry lacks as not-a-number, never as zero", () => {
    const history = new History(2, 3)
    history.push([1])
    expect(history.at(0, 0)).toBe(1)
    expect(history.at(0, 1)).toBeNaN()
  })
})
