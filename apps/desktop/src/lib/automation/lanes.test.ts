import { describe, expect, it } from "vitest"

import type {
  Automation,
  AutomationPoint,
  AutomationTarget,
  Clip,
  PlaylistTrack,
  Project,
} from "@/bindings"

import {
  automatedAt,
  compileLanes,
  holdSegments,
  laneCorners,
  laneValueAt,
  laneValueBefore,
  targetKey,
} from "./lanes"

const BAR = 3840

const point = (
  tick: number,
  value: number,
  more: Partial<AutomationPoint> = {}
): AutomationPoint => ({ tick, value, curve: 0, hold: false, ...more })

const FADER: AutomationTarget = { type: "trackVolume", track: 5 }

function automation(
  id: number,
  points: AutomationPoint[],
  target: AutomationTarget = FADER
): Automation {
  return { id, name: `A${id}`, color: 0x123456, target, points }
}

/** An automation clip. Track ids are 100 plus the row. */
function clip(
  id: number,
  row: number,
  start: number,
  length: number,
  shows: number,
  more: Partial<Clip> = {}
): Clip {
  return {
    id,
    track: 100 + row,
    start,
    length,
    offset: 0,
    muted: false,
    content: { type: "automation", automation: shows },
    ...more,
  }
}

function song(
  automations: Automation[],
  clips: Clip[],
  mutedRows: number[] = []
): Pick<Project, "playlist" | "automations"> {
  const tracks: PlaylistTrack[] = [0, 1, 2, 3].map((row) => ({
    id: 100 + row,
    name: `Track ${row + 1}`,
    muted: mutedRows.includes(row),
  }))
  return { automations, playlist: { tracks, clips } }
}

const ramp = automation(1, [point(0, 0), point(BAR, 1)])

describe("targetKey", () => {
  it("is the same for the same target and differs for another", () => {
    const targets: AutomationTarget[] = [
      { type: "channelVolume", channel: 3 },
      { type: "channelPan", channel: 3 },
      { type: "trackVolume", track: 3 },
      { type: "trackPan", track: 3 },
      { type: "sendGain", track: 3, target: 4 },
      { type: "sendGain", track: 4, target: 3 },
      { type: "effectParam", track: 1, effect: 9, param: 2 },
      { type: "effectParam", track: 1, effect: 9, param: 3 },
      { type: "effectMix", track: 1, effect: 9 },
      { type: "instrumentParam", channel: 3, param: 2 },
      { type: "tempo" },
    ]
    expect(new Set(targets.map(targetKey)).size).toBe(targets.length)
    expect(targetKey({ type: "trackVolume", track: 3 })).toBe(
      targetKey({ type: "trackVolume", track: 3 })
    )
    // An effect keeps its automations when it moves to another track.
    expect(targetKey({ type: "effectMix", track: 1, effect: 9 })).toBe(
      targetKey({ type: "effectMix", track: 7, effect: 9 })
    )
  })
})

describe("a lane", () => {
  it("joins repeated pattern events to playlist priority, offsets and end holds", () => {
    const project = { ...song([ramp], [clip(10, 1, 0, BAR * 2, 0, { content: { type: "pattern" as const, pattern: 7 }, offset: BAR / 4 }), clip(11, 0, BAR / 2, BAR / 2, 1)]),
      patterns: [{ id: 7, name: "Events", color: 0, lengthSteps: 16, lanes: [], events: [{ id: 2, target: FADER, points: [point(0, 0), point(BAR, 1)] }] }] }
    const [lane] = compileLanes(project)
    expect(laneValueAt(lane, 0)).toEqual({ automation: 2, value: .25 })
    expect(laneValueAt(lane, BAR / 2)).toEqual({ automation: 1, value: 0 })
    expect(laneValueAt(lane, BAR)).toEqual({ automation: 2, value: .25 })
    expect(laneValueAt(lane, BAR * 2 + 100)).toEqual({ automation: 2, value: .25 })
    expect(holdSegments([lane], BAR * 3).at(-1)?.automation).toBe(2)
  })
  it("has nothing to say before the target's first clip", () => {
    const [lane] = compileLanes(song([ramp], [clip(1, 0, BAR, BAR, 1)]))
    expect(laneValueAt(lane, 0)).toBeNull()
    expect(laneValueAt(lane, BAR - 1)).toBeNull()
    expect(laneValueAt(lane, BAR)).toEqual({ value: 0, automation: 1 })
  })

  it("follows the curve inside a clip, through the clip's window", () => {
    const windowed = clip(1, 0, 2 * BAR, BAR / 2, 1, { offset: BAR / 4 })
    const [lane] = compileLanes(song([ramp], [windowed]))
    // Song tick `start` is curve tick `offset`.
    expect(laneValueAt(lane, 2 * BAR)?.value).toBeCloseTo(0.25, 9)
    expect(laneValueAt(lane, 2 * BAR + BAR / 4)?.value).toBeCloseTo(0.5, 9)
  })

  it("holds the value the clip ended on until the next clip begins", () => {
    const first = clip(1, 0, 0, BAR / 2, 1)
    const second = clip(2, 0, 3 * BAR, BAR, 1)
    const [lane] = compileLanes(song([ramp], [first, second]))
    expect(laneValueAt(lane, BAR / 2)?.value).toBeCloseTo(0.5, 9)
    expect(laneValueAt(lane, 2 * BAR)?.value).toBeCloseTo(0.5, 9)
    expect(laneValueAt(lane, 3 * BAR)?.value).toBe(0)
    // After the last clip it holds for good.
    expect(laneValueAt(lane, 99 * BAR)?.value).toBe(1)
    // The song arrives at the second clip from the held value.
    expect(laneValueBefore(lane, 3 * BAR)).toBeCloseTo(0.5, 9)
  })

  it("lets the clip nearest the top win, then the later one, then the lower id", () => {
    const low = automation(1, [point(0, 0.1)])
    const high = automation(2, [point(0, 0.9)])
    const top = compileLanes(
      song([low, high], [clip(1, 2, 0, BAR, 1), clip(2, 0, 0, BAR, 2)])
    )
    expect(top).toHaveLength(1)
    expect(laneValueAt(top[0], 10)).toEqual({ value: 0.9, automation: 2 })

    const later = compileLanes(
      song([low, high], [clip(1, 1, 0, 2 * BAR, 1), clip(2, 1, BAR, BAR, 2)])
    )
    expect(laneValueAt(later[0], 10)?.automation).toBe(1)
    expect(laneValueAt(later[0], BAR + 10)?.automation).toBe(2)

    const lowerId = compileLanes(
      song([low, high], [clip(7, 1, 0, BAR, 1), clip(3, 1, 0, BAR, 2)])
    )
    expect(laneValueAt(lowerId[0], 10)?.automation).toBe(2)
  })

  it("goes back to a longer clip underneath when the winner ends", () => {
    const low = automation(1, [point(0, 0.1)])
    const high = automation(2, [point(0, 0.9)])
    const [lane] = compileLanes(
      song([low, high], [clip(1, 1, 0, 4 * BAR, 1), clip(2, 0, BAR, BAR, 2)])
    )
    expect(laneValueAt(lane, 0)?.automation).toBe(1)
    expect(laneValueAt(lane, BAR)?.automation).toBe(2)
    expect(laneValueAt(lane, 2 * BAR)?.automation).toBe(1)
    expect(lane.spans).toHaveLength(4)
  })

  it("counts muted clips, and clips on muted tracks, for nothing", () => {
    const muted = clip(1, 0, 0, BAR, 1, { muted: true })
    expect(compileLanes(song([ramp], [muted]))).toEqual([])
    expect(compileLanes(song([ramp], [clip(1, 2, 0, BAR, 1)], [2]))).toEqual([])
  })

  it("keeps one lane per target, in the order of the automations", () => {
    const pan = automation(2, [point(0, 0.5)], { type: "trackPan", track: 5 })
    const lanes = compileLanes(
      song([pan, ramp], [clip(1, 0, 0, BAR, 1), clip(2, 1, 0, BAR, 2)])
    )
    expect(lanes.map((lane) => lane.target.type)).toEqual([
      "trackPan",
      "trackVolume",
    ])
  })
})

describe("what the engine reports", () => {
  it("lists the automations that have their target in hand", () => {
    const pan = automation(2, [point(0, 0.25)], { type: "trackPan", track: 5 })
    const project = song(
      [ramp, pan],
      [clip(1, 0, 0, BAR, 1), clip(2, 1, 2 * BAR, BAR, 2)]
    )
    const lanes = compileLanes(project)
    const order = project.automations.map((item) => item.id)
    expect(automatedAt(lanes, order, BAR / 2)).toEqual([
      { automation: 1, value: 0.5 },
    ])
    // The fader holds its end value while the pan's clip plays.
    expect(automatedAt(lanes, order, 2 * BAR + 5)).toEqual([
      { automation: 1, value: 1 },
      { automation: 2, value: 0.25 },
    ])
  })
})

describe("holdSegments", () => {
  it("runs from a clip's end to the next clip of the target, or the song's end", () => {
    const first = clip(1, 0, 0, BAR / 2, 1)
    const second = clip(2, 2, 3 * BAR, BAR, 1)
    const lanes = compileLanes(song([ramp], [first, second]))
    expect(holdSegments(lanes, 10 * BAR)).toEqual([
      {
        clip: 1,
        track: 100,
        automation: 1,
        start: BAR / 2,
        end: 3 * BAR,
        value: 0.5,
      },
      {
        clip: 2,
        track: 102,
        automation: 1,
        start: 4 * BAR,
        end: 10 * BAR,
        value: 1,
      },
    ])
  })

  it("has none while clips of the target follow each other without a gap", () => {
    const lanes = compileLanes(
      song([ramp], [clip(1, 0, 0, BAR, 1), clip(2, 0, BAR, BAR, 1)])
    )
    expect(holdSegments(lanes, 2 * BAR)).toEqual([])
  })
})

describe("laneCorners", () => {
  it("has a corner at every span start and point, and cuts bends up", () => {
    const bent = automation(1, [
      point(0, 0, { curve: 0.5 }),
      point(240, 1),
      point(480, 0),
    ])
    const [lane] = compileLanes(song([bent], [clip(1, 0, 960, 960, 1)]))
    expect(laneCorners(lane, 4000, 60)).toEqual([
      0, 960, 1020, 1080, 1140, 1200, 1440, 1920, 4000,
    ])
  })
})
