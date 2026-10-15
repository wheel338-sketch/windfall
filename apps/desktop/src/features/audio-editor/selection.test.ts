import { describe, expect, it } from "vitest"
import { clockTime, frameAt, rulerTicks, selectionBetween, validSelection } from "./selection"

describe("audio editor frame selection", () => {
  it("maps and clamps the pointer and sorts backward drags with exclusive ends", () => {
    expect(frameAt(35, 10, 100, 1000)).toBe(250)
    expect(frameAt(-1, 10, 100, 1000)).toBe(0)
    expect(frameAt(200, 10, 100, 1000)).toBe(1000)
    expect(frameAt(35, 10, 0, 1000)).toBe(0)
    expect(selectionBetween(900, 100, 1000)).toEqual({ start: 100, end: 900 })
    expect(selectionBetween(1000, 1000, 1000)).toEqual({
      start: 999,
      end: 1000,
    })
  })
  it("refuses empty, fractional, non-finite and out-of-bounds selections", () => {
    expect(validSelection({ start: 0, end: 1000 }, 1000)).toBe(true)
    for (const selection of [
      { start: 0, end: 0 },
      { start: -1, end: 20 },
      { start: 0, end: 1001 },
      { start: 0.5, end: 2 },
      { start: NaN, end: 2 },
    ]) {
      expect(validSelection(selection, 1000)).toBe(false)
    }
  })
  it("reads positions as minutes and seconds and rounds into the next minute", () => {
    expect(clockTime(0)).toBe("0:00.000")
    expect(clockTime(1.25)).toBe("0:01.250")
    expect(clockTime(62.5, 1)).toBe("1:02.5")
    expect(clockTime(59.9996)).toBe("1:00.000")
    expect(clockTime(5, 0)).toBe("0:05")
    expect(clockTime(NaN)).toBe("")
  })
  it("marks a ruler with a round step and at most eight intervals", () => {
    expect(rulerTicks(4).map((tick) => tick.seconds)).toEqual([0, 0.5, 1, 1.5, 2, 2.5, 3, 3.5, 4])
    expect(rulerTicks(4)[1].decimals).toBe(1)
    expect(rulerTicks(120).map((tick) => tick.seconds)).toEqual([0, 15, 30, 45, 60, 75, 90, 105, 120])
    expect(rulerTicks(0.03).every((tick) => tick.decimals === 3)).toBe(true)
    expect(rulerTicks(0)).toEqual([])
  })
})
