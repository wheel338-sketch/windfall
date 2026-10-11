import { describe, expect, it } from "vitest"

import { describeReading } from "./tuner-editor"

const reading = (cents: number | null, midiNote: number | null = 69) =>
  ({
    type: "tuner",
    firstFrame: "0",
    frames: 256,
    frequencyHz: cents === null ? null : 440,
    midiNote,
    cents,
    confidence: 0.98,
  }) as const

describe("what the tuner shows for a reading", () => {
  it("names the note and says how far off it is, and which way", () => {
    const sharp = describeReading(reading(31.8))
    expect(sharp.note).toBe("A4")
    expect(sharp.cents).toBe("+31.8 ¢")
    expect(sharp.frequency).toBe("440.0 Hz")
    expect(sharp.spoken).toBe("A4, +31.8 ¢, sharp")
    expect(sharp.inTune).toBe(false)
    // 31.8 of 50 cents to the right of the middle.
    expect(sharp.needle).toBeCloseTo(0.818, 9)

    const flat = describeReading(reading(-12, 60))
    expect(flat.spoken).toBe("C4, −12.0 ¢, flat")
    expect(flat.needle).toBeCloseTo(0.38, 9)
  })

  it("counts five cents either way as in tune and stops the needle at the ends", () => {
    expect(describeReading(reading(5)).inTune).toBe(true)
    expect(describeReading(reading(-5)).spoken).toBe("A4, −5.0 ¢, in tune")
    expect(describeReading(reading(5.1)).inTune).toBe(false)
    expect(describeReading(reading(80)).needle).toBe(1)
    expect(describeReading(reading(-80)).needle).toBe(0)
  })

  it("shows no note at all without a steady pitch", () => {
    for (const none of [null, reading(null), reading(3, null)]) {
      const shown = describeReading(none)
      expect(shown.note).toBe("–")
      expect(shown.needle).toBeNull()
      expect(shown.inTune).toBe(false)
      expect(shown.spoken).toBe("No steady pitch")
    }
  })
})
