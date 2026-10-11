import type {
  AutomationPoint,
  AutomationTarget,
  Pattern,
  PatternEventLane,
  Project,
} from "@/bindings"
import { targetKey } from "@/lib/automation/lanes"
import { describeTarget, targetState } from "@/lib/automation/targets"
import { MAX_PATTERN_TICKS, TICKS_PER_STEP } from "@/lib/units"

/*
 * Event automation: curves a pattern stores for itself. A lane is one
 * control's curve in pattern ticks, and the control follows it wherever the
 * pattern plays. Everything here is plain data; the document commands
 * `setPatternEvents` and `generatePatternEventLfo` make the edits.
 */

/** The most points a lane holds, as the document enforces. */
export const MAX_EVENT_POINTS = 4096

/** The lanes of a pattern. A pattern without any omits the field. */
export function eventLanes(
  pattern: Pick<Pattern, "events"> | undefined
): readonly PatternEventLane[] {
  return pattern?.events ?? []
}

/** The lane a pattern has for a target, if it has one. */
export function eventLane(
  pattern: Pick<Pattern, "events"> | undefined,
  target: AutomationTarget
): PatternEventLane | undefined {
  const key = targetKey(target)
  return eventLanes(pattern).find((lane) => targetKey(lane.target) === key)
}

/** The points a pattern holds for a target: none when it has no lane. */
export function lanePoints(
  pattern: Pick<Pattern, "events"> | undefined,
  target: AutomationTarget
): AutomationPoint[] {
  return (eventLane(pattern, target)?.points ?? []).map((point) => ({
    ...point,
  }))
}

/** The pattern's length in ticks: where its lanes go around. */
export function patternTicks(pattern: Pick<Pattern, "lengthSteps">): number {
  return Math.max(1, pattern.lengthSteps) * TICKS_PER_STEP
}

export type EventTarget = {
  key: string
  target: AutomationTarget
  label: string
  /** The pattern already has a lane for it. */
  used: boolean
}

/**
 * What the editor's picker offers for a pattern: every control that has a
 * lane there, then the level and pan of each channel and mixer track, each
 * effect's mix and each send. Settings of an instrument or effect are
 * reached from the control itself ("Edit events in pattern"), and appear
 * here once they have a lane. The tempo is never offered: it belongs to the
 * song.
 */
export function eventTargets(
  project: Project,
  pattern: Pick<Pattern, "events"> | undefined,
  also: AutomationTarget | null = null
): EventTarget[] {
  const targets: AutomationTarget[] = []
  for (const lane of eventLanes(pattern)) targets.push(lane.target)
  if (also) targets.push(also)
  for (const channel of project.channels) {
    targets.push({ type: "channelVolume", channel: channel.id })
    targets.push({ type: "channelPan", channel: channel.id })
  }
  for (const track of project.mixer.tracks) {
    targets.push({ type: "trackVolume", track: track.id })
    targets.push({ type: "trackPan", track: track.id })
    for (const effect of track.effects) {
      targets.push({ type: "effectMix", track: track.id, effect: effect.id })
    }
    for (const send of track.sends ?? []) {
      targets.push({ type: "sendGain", track: track.id, target: send.target })
    }
  }
  const used = new Set(
    eventLanes(pattern).map((lane) => targetKey(lane.target))
  )
  const seen = new Set<string>()
  const list: EventTarget[] = []
  for (const target of targets) {
    const key = targetKey(target)
    if (target.type === "tempo" || seen.has(key)) continue
    // A lane whose control is gone is still listed, so it can be cleared.
    if (!targetState(project, target) && !used.has(key)) continue
    seen.add(key)
    list.push({
      key,
      target,
      label: describeTarget(project, target),
      used: used.has(key),
    })
  }
  return list
}

const clamp = (value: number, low: number, high: number) =>
  Math.min(high, Math.max(low, value))

/** A point as the document will store it: whole tick, value and bend in range. */
export function cleanPoint(point: AutomationPoint, length: number): AutomationPoint {
  const limit = Math.min(MAX_PATTERN_TICKS, Math.max(0, Math.round(length)))
  return {
    tick: clamp(Math.round(Number.isFinite(point.tick) ? point.tick : 0), 0, limit),
    value: clamp(Number.isFinite(point.value) ? point.value : 0, 0, 1),
    curve: clamp(Number.isFinite(point.curve) ? point.curve : 0, -1, 1),
    hold: !!point.hold,
  }
}

/**
 * The points with one more at `tick`, kept in order. A point that lands on
 * a tick that already has one goes after it, which makes the curve jump
 * there. Returns the new list and where the point is in it.
 */
export function insertPoint(
  points: readonly AutomationPoint[],
  tick: number,
  value: number,
  length: number
): { points: AutomationPoint[]; index: number } {
  const point = cleanPoint({ tick, value, curve: 0, hold: false }, length)
  let index = points.length
  for (let at = 0; at < points.length; at += 1) {
    if (points[at].tick > point.tick) {
      index = at
      break
    }
  }
  const next = points.map((item) => ({ ...item }))
  next.splice(index, 0, point)
  return { points: next, index }
}

/**
 * The points with the one at `index` changed. It cannot pass its
 * neighbours: its tick stays between theirs, so the order holds.
 */
export function changePoint(
  points: readonly AutomationPoint[],
  index: number,
  change: Partial<AutomationPoint>,
  length: number
): AutomationPoint[] {
  const current = points[index]
  if (!current) return points.map((item) => ({ ...item }))
  const low = index > 0 ? points[index - 1].tick : 0
  const high =
    index < points.length - 1
      ? points[index + 1].tick
      : Math.min(MAX_PATTERN_TICKS, Math.max(low, Math.round(length)))
  const cleaned = cleanPoint({ ...current, ...change }, length)
  cleaned.tick = clamp(cleaned.tick, low, Math.max(low, high))
  return points.map((item, at) => (at === index ? cleaned : { ...item }))
}

/** The points without the one at `index`. */
export function removePoint(
  points: readonly AutomationPoint[],
  index: number
): AutomationPoint[] {
  return points
    .filter((_, at) => at !== index)
    .map((item) => ({ ...item }))
}

export function samePoints(
  a: readonly AutomationPoint[],
  b: readonly AutomationPoint[]
): boolean {
  return (
    a.length === b.length &&
    a.every(
      (point, index) =>
        point.tick === b[index].tick &&
        point.value === b[index].value &&
        point.curve === b[index].curve &&
        point.hold === b[index].hold
    )
  )
}
