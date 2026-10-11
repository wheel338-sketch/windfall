import { describe, expect, it } from "vitest"

import type { AutomationPoint, AutomationTarget, Pattern, Project } from "@/bindings"
import { MAX_PATTERN_TICKS } from "@/lib/units"

import {
  changePoint,
  cleanPoint,
  eventLane,
  eventTargets,
  insertPoint,
  lanePoints,
  patternTicks,
  removePoint,
  samePoints,
} from "./model"

const point = (tick: number, value: number): AutomationPoint => ({
  tick,
  value,
  curve: 0,
  hold: false,
})

const VOLUME: AutomationTarget = { type: "channelVolume", channel: 5 }
const PAN: AutomationTarget = { type: "channelPan", channel: 5 }

function project(events: Pattern["events"] = undefined): Project {
  return {
    channels: [{ id: 5, name: "Lead", volume: 0.8, pan: 0 }],
    mixer: {
      tracks: [
        {
          id: 0,
          name: "Master",
          volume: 1,
          pan: 0,
          effects: [{ id: 9, mix: 1, params: { type: "delay" } }],
          sends: [],
        },
        {
          id: 3,
          name: "Bus",
          volume: 1,
          pan: 0,
          effects: [],
          sends: [{ target: 0, gain: 0.5 }],
        },
      ],
    },
    patterns: [{ id: 1, name: "Pattern 1", lengthSteps: 16, lanes: [], events }],
    settings: { timeSignature: { numerator: 4, denominator: 4 } },
    automations: [],
  } as unknown as Project
}

describe("event lanes", () => {
  it("reads a pattern's lanes, and none from a pattern without the field", () => {
    const empty = project().patterns[0]
    expect(lanePoints(empty, VOLUME)).toEqual([])
    expect(eventLane(empty, VOLUME)).toBeUndefined()
    expect(lanePoints(undefined, VOLUME)).toEqual([])

    const lanes = [{ id: 40, target: VOLUME, points: [point(0, 0.5), point(960, 1)] }]
    const pattern = project(lanes).patterns[0]
    expect(eventLane(pattern, { ...VOLUME })?.id).toBe(40)
    expect(eventLane(pattern, PAN)).toBeUndefined()
    const read = lanePoints(pattern, VOLUME)
    expect(read).toEqual(lanes[0].points)
    // A copy: editing it does not touch the project.
    read[0].value = 0
    expect(lanes[0].points[0].value).toBe(0.5)
    expect(patternTicks(pattern)).toBe(3840)
  })

  it("offers lanes first, then levels, pans, effect mixes and sends, never the tempo", () => {
    const ghost: AutomationTarget = { type: "channelPan", channel: 777 }
    const lanes = [
      { id: 40, target: PAN, points: [point(0, 0.5)] },
      // Its channel is gone; it is still listed so it can be cleared.
      { id: 41, target: ghost, points: [point(0, 0.5)] },
    ]
    const source = project(lanes)
    const offered = eventTargets(source, source.patterns[0])
    expect(offered.map((item) => item.key)).toEqual([
      "channelPan:5",
      "channelPan:777",
      "channelVolume:5",
      "trackVolume:0",
      "trackPan:0",
      "effectMix:9",
      "trackVolume:3",
      "trackPan:3",
      "sendGain:3:0",
    ])
    expect(offered.filter((item) => item.used).map((item) => item.key)).toEqual([
      "channelPan:5",
      "channelPan:777",
    ])
    expect(offered[2].label).toBe("Lead → volume")

    // A control reached from its own menu is offered while it is open.
    const setting: AutomationTarget = { type: "trackParam", track: 3, param: 0 }
    const withSetting = eventTargets(source, source.patterns[0], setting)
    expect(withSetting.some((item) => item.key === "trackParam:3:0")).toBe(true)
    const withTempo = eventTargets(source, source.patterns[0], { type: "tempo" })
    expect(withTempo.some((item) => item.key === "tempo")).toBe(false)
    // A control the project does not have, with no lane, is not offered.
    const missing = eventTargets(source, source.patterns[0], {
      type: "channelVolume",
      channel: 999,
    })
    expect(missing.some((item) => item.key === "channelVolume:999")).toBe(false)
  })
})

describe("editing points", () => {
  it("cleans a point into whole ticks inside the pattern and values in range", () => {
    expect(
      cleanPoint({ tick: 10.6, value: 1.5, curve: -3, hold: true }, 3840)
    ).toEqual({ tick: 11, value: 1, curve: -1, hold: true })
    expect(cleanPoint(point(-5, -1), 3840)).toEqual(point(0, 0))
    expect(cleanPoint(point(9999, 0.5), 3840).tick).toBe(3840)
    expect(cleanPoint(point(Number.NaN, Number.NaN), 3840)).toEqual(point(0, 0))
    expect(cleanPoint(point(MAX_PATTERN_TICKS * 2, 0.5), MAX_PATTERN_TICKS * 2).tick).toBe(
      MAX_PATTERN_TICKS
    )
  })

  it("inserts in order, after a point that shares the tick", () => {
    const start = [point(0, 0.2), point(960, 0.8)]
    const middle = insertPoint(start, 480, 0.5, 3840)
    expect(middle.index).toBe(1)
    expect(middle.points.map((item) => item.tick)).toEqual([0, 480, 960])
    const jump = insertPoint(start, 960, 0.1, 3840)
    expect(jump.index).toBe(2)
    expect(jump.points.map((item) => item.value)).toEqual([0.2, 0.8, 0.1])
    const last = insertPoint(start, 5000, 2, 3840)
    expect(last.points[2]).toEqual(point(3840, 1))
    expect(insertPoint([], 100, 0.5, 3840)).toEqual({
      points: [point(100, 0.5)],
      index: 0,
    })
    // The list handed in is left alone.
    expect(start).toHaveLength(2)
  })

  it("changes a point without letting it pass its neighbours", () => {
    const start = [point(0, 0.2), point(480, 0.5), point(960, 0.8)]
    expect(changePoint(start, 1, { tick: 100, value: 0.9 }, 3840)[1]).toEqual(
      point(100, 0.9)
    )
    expect(changePoint(start, 1, { tick: 2000 }, 3840)[1].tick).toBe(960)
    expect(changePoint(start, 1, { tick: -50 }, 3840)[1].tick).toBe(0)
    expect(changePoint(start, 2, { tick: 9999 }, 3840)[2].tick).toBe(3840)
    expect(changePoint(start, 0, { tick: 700 }, 3840)[0].tick).toBe(480)
    expect(changePoint(start, 1, { curve: 5, hold: true }, 3840)[1]).toEqual({
      tick: 480,
      value: 0.5,
      curve: 1,
      hold: true,
    })
    // An index that is not there changes nothing.
    expect(changePoint(start, 7, { value: 0 }, 3840)).toEqual(start)
    expect(start[1]).toEqual(point(480, 0.5))
  })

  it("removes a point and compares lists", () => {
    const start = [point(0, 0.2), point(480, 0.5)]
    expect(removePoint(start, 0)).toEqual([point(480, 0.5)])
    expect(removePoint(start, 5)).toEqual(start)
    expect(samePoints(start, [point(0, 0.2), point(480, 0.5)])).toBe(true)
    expect(samePoints(start, [point(0, 0.2)])).toBe(false)
    expect(samePoints(start, [point(0, 0.2), { ...point(480, 0.5), hold: true }])).toBe(
      false
    )
    expect(samePoints([], [])).toBe(true)
  })
})
