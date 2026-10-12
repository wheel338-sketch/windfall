import { describe, expect, it } from "vitest"

import type { AutomationPoint, AutomationRange } from "@/bindings"
import fixtures from "@/bindings/automation-fixtures.json"

import {
  curveShape,
  curveValue,
  curveValueBefore,
  GAIN_RANGE,
  paramRange,
  rangeNormalized,
  rangeValue,
  TEMPO_RANGE,
} from "./curve"

/*
 * The fixtures are written by the engine's own functions. Every number in
 * them is checked, so the curves drawn here are the curves that play.
 */

const CURVE_TOLERANCE = 1e-5
const RANGE_TOLERANCE = 1e-4

function relative(got: number, wanted: number): number {
  return Math.abs(got - wanted) / Math.max(1, Math.abs(wanted))
}

describe("the bend of a segment", () => {
  it("matches the engine at every tension and position", () => {
    let checked = 0
    let worst = 0
    for (const shape of fixtures.shapes) {
      shape.parts.forEach((part, index) => {
        const error = Math.abs(
          curveShape(part, shape.curve) - shape.values[index]
        )
        worst = Math.max(worst, error)
        checked += 1
      })
    }
    expect(checked).toBe(77)
    expect(worst).toBeLessThan(1e-9)
  })

  it("holds what is out of range to what makes sense", () => {
    expect(curveShape(0.5, Number.NaN)).toBe(0.5)
    expect(curveShape(0.5, 9)).toBe(curveShape(0.5, 1))
    expect(curveShape(7, 0.5)).toBe(1)
    expect(curveShape(-1, 0.5)).toBe(0)
  })
})

describe("the value of a curve", () => {
  it("matches the engine before, on, between and after its points", () => {
    let checked = 0
    let worst = 0
    for (const curve of fixtures.curves) {
      const points: AutomationPoint[] = curve.points
      curve.ticks.forEach((tick, index) => {
        const error = Math.abs(curveValue(points, tick) - curve.values[index])
        worst = Math.max(worst, error)
        checked += 1
      })
    }
    expect(fixtures.curves).toHaveLength(5)
    expect(checked).toBeGreaterThan(150)
    expect(worst).toBeLessThan(CURVE_TOLERANCE)
  })

  it("is 0 with no points and flat with one", () => {
    expect(curveValue([], 10)).toBe(0)
    const one = [{ tick: 50, value: 0.7, curve: 0, hold: false }]
    expect(curveValue(one, 0)).toBe(0.7)
    expect(curveValue(one, 900)).toBe(0.7)
  })

  it("leaves a jump from the later of two points on one tick", () => {
    const jump: AutomationPoint[] = [
      { tick: 0, value: 0, curve: 0, hold: false },
      { tick: 100, value: 0.5, curve: 0, hold: false },
      { tick: 100, value: 1, curve: 0, hold: false },
      { tick: 200, value: 0, curve: 0, hold: false },
    ]
    expect(curveValue(jump, 99)).toBeCloseTo(0.495, 9)
    expect(curveValue(jump, 100)).toBe(1)
    expect(curveValueBefore(jump, 100)).toBe(0.5)
    expect(curveValueBefore(jump, 150)).toBe(curveValue(jump, 150))
    expect(curveValueBefore(jump, 0)).toBe(0)
  })
})

describe("the range of a target", () => {
  it("maps every range to real values and back like the engine", () => {
    let checked = 0
    let worstForward = 0
    let worstBack = 0
    for (const entry of fixtures.ranges) {
      const range = entry.range as AutomationRange
      entry.normalized.forEach((normalized, index) => {
        const value = rangeValue(range, normalized)
        const forward = relative(value, entry.values[index])
        expect(forward, `${entry.name} at ${normalized}`).toBeLessThan(
          RANGE_TOLERANCE
        )
        worstForward = Math.max(worstForward, forward)
        // Back from the engine's own value, as a point typed in real units.
        const back = Math.abs(
          rangeNormalized(range, entry.values[index]) -
            entry.normalizedBack[index]
        )
        expect(back, `${entry.name} back from ${value}`).toBeLessThan(
          RANGE_TOLERANCE
        )
        worstBack = Math.max(worstBack, back)
        checked += 1
      })
    }
    // All generated effect and instrument ranges, nine samples per range.
    // 1,071 effect + 1,547 instrument controls, plus four shared ranges.
    expect(fixtures.ranges).toHaveLength(2622)
    expect(checked).toBe(2622 * 9)
    expect(worstForward).toBeLessThan(RANGE_TOLERANCE)
    expect(worstBack).toBeLessThan(RANGE_TOLERANCE)
  })

  it("has 0 dB at 0.7071 and 120 bpm at 0.2148", () => {
    expect(rangeValue(GAIN_RANGE, Math.SQRT1_2)).toBeCloseTo(1, 9)
    expect(rangeNormalized(GAIN_RANGE, 1)).toBeCloseTo(Math.SQRT1_2, 9)
    expect(rangeValue(TEMPO_RANGE, 0.21484375)).toBe(120)
    expect(rangeNormalized(TEMPO_RANGE, 120)).toBe(0.21484375)
  })

  it("holds values outside 0 to 1, and outside the range on the way back", () => {
    expect(rangeValue(GAIN_RANGE, 7)).toBe(2)
    expect(rangeValue(GAIN_RANGE, -1)).toBe(0)
    expect(rangeValue(GAIN_RANGE, Number.NaN)).toBe(0)
    expect(rangeNormalized(GAIN_RANGE, 9)).toBe(1)
    expect(rangeNormalized(GAIN_RANGE, Number.NaN)).toBe(0)
  })

  it("picks the taper from a setting's description", () => {
    const base = { min: 20, max: 20000, scale: "logarithmic" } as const
    expect(paramRange({ ...base, kind: "float" }).taper).toBe("logarithmic")
    expect(paramRange({ ...base, kind: "float", min: 0 }).taper).toBe("linear")
    expect(paramRange({ ...base, kind: "integer" }).taper).toBe("stepped")
    expect(paramRange({ ...base, kind: "choice" }).taper).toBe("stepped")
    expect(paramRange({ ...base, kind: "toggle" }).taper).toBe("toggle")
  })
})
