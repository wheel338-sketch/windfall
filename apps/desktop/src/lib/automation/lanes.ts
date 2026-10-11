import type {
  AutomationId,
  AutomationPoint,
  AutomationTarget,
  ClipId,
  PlaylistTrackId,
  Project,
} from "@/bindings"

import { curveValue, curveValueBefore } from "./curve"

/*
 * What every automated target does along the song. This mirrors the lanes
 * the engine compiles (`windfall-engine/src/automation.rs`): one lane per
 * target, a row of spans that follow each other, each a stretch of some
 * clip's curve or the value the last clip left behind. Which clip wins
 * where several overlap is settled here.
 */

/** A target as text: two automations with the same key move the same thing. */
export function targetKey(target: AutomationTarget): string {
  switch (target.type) {
    case "channelVolume":
    case "channelPan":
      return `${target.type}:${target.channel}`
    case "trackVolume":
    case "trackPan":
      return `${target.type}:${target.track}`
    case "trackParam":
      return `trackParam:${target.track}:${target.param}`
    case "sendGain":
    case "sidechainGain":
      return `${target.type}:${target.track}:${target.target}`
    case "effectParam":
      return `effectParam:${target.effect}:${target.param}`
    case "effectMix":
      return `effectMix:${target.effect}`
    case "instrumentParam":
      return `instrumentParam:${target.channel}:${target.param}`
    case "tempo":
      return "tempo"
    default: {
      const _exhaustive: never = target
      return _exhaustive
    }
  }
}

export type LaneSpan = {
  /** The song tick the span begins on. It lasts until the next one. */
  readonly start: number
  /** The automation whose clip made the span. */
  readonly automation: AutomationId
  /** The clip that plays here, or the one whose end value is held. */
  readonly clip: ClipId
  readonly track: PlaylistTrackId
  /** The clip's curve, read `shift` ticks ahead of the song. Null on a hold. */
  readonly points: readonly AutomationPoint[] | null
  readonly shift: number
  /** Pattern event curves repeat in source ticks. */
  readonly repeat?: number
  /** On a hold: the value the clip ended on. */
  readonly held: number
}

export type Lane = {
  readonly key: string
  readonly target: AutomationTarget
  /** From the start of the target's first clip on. */
  readonly spans: readonly LaneSpan[]
}

type Piece = {
  start: number
  end: number
  offset: number
  /** Position of the clip's playlist track, from the top. */
  row: number
  id: ClipId
  track: PlaylistTrackId
  automation: AutomationId
  points: readonly AutomationPoint[]
  repeat?: number
}

/**
 * Whether one clip has its way over another where both play: the one on
 * the playlist track nearer the top, then the one that starts later, then
 * the one with the lower id.
 */
function beats(a: Piece, b: Piece): boolean {
  if (a.row !== b.row) return a.row < b.row
  if (a.start !== b.start) return a.start > b.start
  return a.id < b.id
}

function flatten(pieces: readonly Piece[]): LaneSpan[] {
  const edges = [
    ...new Set(pieces.flatMap((piece) => [piece.start, piece.end])),
  ].sort((a, b) => a - b)
  const spans: LaneSpan[] = []
  let playing: ClipId | null = null
  for (const edge of edges) {
    let winner: Piece | null = null
    for (const piece of pieces) {
      if (piece.start > edge || edge >= piece.end) continue
      if (!winner || beats(piece, winner)) winner = piece
    }
    if (winner) {
      // One clip that wins over several edges in a row stays one span.
      if (playing === winner.id) continue
      playing = winner.id
      spans.push({
        start: edge,
        automation: winner.automation,
        clip: winner.id,
        track: winner.track,
        points: winner.points,
        shift: winner.offset - winner.start,
        repeat: winner.repeat,
        held: 0,
      })
      continue
    }
    playing = null
    // Of the clips that have ended, the one that ended last.
    let last: Piece | null = null
    for (const piece of pieces) {
      if (piece.end > edge) continue
      const later = !last || piece.end > last.end
      const wins = last !== null && piece.end === last.end && beats(piece, last)
      if (later || wins) last = piece
    }
    if (!last) continue
    spans.push({
      start: edge,
      automation: last.automation,
      clip: last.id,
      track: last.track,
      points: null,
      shift: 0,
      held: curveValue(last.points, last.repeat ? ((last.offset + last.end - last.start) % last.repeat || last.repeat) : last.offset + (last.end - last.start)),
    })
  }
  return spans
}

/**
 * The lanes of a project, in the order their targets first turn up among
 * its automations. Clips that are muted, or on a muted playlist track,
 * count for nothing.
 */
export function compileLanes(
  project: Pick<Project, "playlist" | "automations"> & Partial<Pick<Project, "patterns">>
): Lane[] {
  const { playlist, automations } = project
  const rows = new Map(playlist.tracks.map((track, row) => [track.id, row]))
  const muted = new Set(
    playlist.tracks.filter((track) => track.muted).map((track) => track.id)
  )
  const byId = new Map(automations.map((item) => [item.id, item]))
  const pieces = new Map<string, Piece[]>()

  for (const clip of playlist.clips) {
    if (clip.content.type !== "automation") continue
    const row = rows.get(clip.track)
    if (row === undefined || clip.muted || muted.has(clip.track)) continue
    if (clip.length <= 0) continue
    const automation = byId.get(clip.content.automation)
    if (!automation || automation.points.length === 0) continue
    const key = targetKey(automation.target)
    const list = pieces.get(key) ?? []
    list.push({
      start: clip.start,
      end: clip.start + clip.length,
      offset: clip.offset,
      row,
      id: clip.id,
      track: clip.track,
      automation: automation.id,
      points: automation.points,
    })
    pieces.set(key, list)
  }

  const eventTargets: { key: string; target: AutomationTarget }[] = []
  for (const pattern of project.patterns ?? []) for (const lane of pattern.events ?? []) {
    const key = targetKey(lane.target)
    eventTargets.push({ key, target: lane.target })
    const list = pieces.get(key) ?? []
    for (const clip of playlist.clips) {
      const row = rows.get(clip.track)
      if (clip.content.type !== "pattern" || clip.content.pattern !== pattern.id || row === undefined || clip.muted || muted.has(clip.track) || clip.length <= 0 || !lane.points.length) continue
      list.push({ start: clip.start, end: clip.start + clip.length, offset: clip.offset, row, id: clip.id, track: clip.track, automation: lane.id, points: lane.points, repeat: pattern.lengthSteps * 240 })
    }
    pieces.set(key, list)
  }

  const lanes: Lane[] = []
  const seen = new Set<string>()
  for (const automation of automations) {
    const key = targetKey(automation.target)
    if (seen.has(key)) continue
    seen.add(key)
    const list = pieces.get(key)
    if (list) {
      lanes.push({ key, target: automation.target, spans: flatten(list) })
    }
  }
  for (const { key, target } of eventTargets) {
    if (seen.has(key)) continue
    seen.add(key)
    const list = pieces.get(key)
    if (list?.length) lanes.push({ key, target, spans: flatten(list) })
  }
  return lanes
}

function spanAt(lane: Lane, tick: number): LaneSpan | null {
  const spans = lane.spans
  let low = 0
  let high = spans.length
  while (low < high) {
    const middle = (low + high) >> 1
    if (spans[middle].start <= tick) low = middle + 1
    else high = middle
  }
  return low > 0 ? spans[low - 1] : null
}

/**
 * The value, 0 to 1, that automation gives a lane's target at `tick` of
 * the song and the automation it comes from, or null before the target's
 * first clip, where the target has its stored value.
 */
export function laneValueAt(
  lane: Lane,
  tick: number
): { value: number; automation: AutomationId } | null {
  const span = spanAt(lane, tick)
  if (!span) return null
  const source = span.repeat ? ((tick + span.shift) % span.repeat + span.repeat) % span.repeat : tick + span.shift
  const value = span.points
    ? curveValue(span.points, source)
    : span.held
  return { value, automation: span.automation }
}

/**
 * What the target is coming from as the song arrives at `tick`. It differs
 * from `laneValueAt` where the lane jumps.
 */
export function laneValueBefore(lane: Lane, tick: number): number | null {
  const spans = lane.spans
  let index = -1
  for (let at = 0; at < spans.length && spans[at].start < tick; at += 1) {
    index = at
  }
  if (index < 0) return null
  const span = spans[index]
  return span.points
    ? curveValueBefore(span.points, span.repeat ? ((tick + span.shift) % span.repeat || span.repeat) : tick + span.shift)
    : span.held
}

/**
 * What the engine reports as `RealtimeFrame.automated` at a tick of the
 * song: one entry per lane that has its target in hand, in the order of the
 * project's automations, at most 128.
 */
export function automatedAt(
  lanes: readonly Lane[],
  order: readonly AutomationId[],
  tick: number
): { automation: AutomationId; value: number }[] {
  const found = new Map<AutomationId, number>()
  for (const lane of lanes) {
    const at = laneValueAt(lane, tick)
    if (at) found.set(at.automation, at.value)
  }
  const listed: { automation: AutomationId; value: number }[] = []
  for (const automation of order) {
    const value = found.get(automation)
    if (value === undefined) continue
    listed.push({ automation, value })
    if (listed.length === 128) break
  }
  return listed
}

/**
 * The ticks at which a lane's course has a corner or a jump, from tick 0
 * up to `end`: where a span begins and where a curve has a point, with the
 * bent stretches cut into pieces of `grain` ticks. Between two neighbours
 * the value moves in a straight line, or near enough.
 */
export function laneCorners(lane: Lane, end: number, grain: number): number[] {
  const corners = [0]
  lane.spans.forEach((span, index) => {
    const until = Math.min(lane.spans[index + 1]?.start ?? end, end)
    if (span.start >= until) return
    corners.push(span.start)
    const points = span.points
    if (!points) return
    const pass = (origin: number, passEnd: number) => {
      if (origin > span.start && origin < until) corners.push(origin)
      points.forEach((point, at) => {
        const tick = origin + point.tick
        if (tick >= passEnd) return
        if (tick > span.start && tick < until) corners.push(tick)
        const next = points[at + 1]
        if (!next || point.hold || point.curve === 0) return
        const stop = Math.min(origin + next.tick, passEnd, until)
        for (let inside = Math.max(tick, span.start) + Math.max(1, grain); inside < stop; inside += Math.max(1, grain)) corners.push(inside)
      })
    }
    if (span.repeat) {
      const first = Math.floor((span.start + span.shift) / span.repeat) * span.repeat - span.shift
      for (let origin = first; origin < until; origin += span.repeat) pass(origin, origin + span.repeat)
    } else pass(-span.shift, Infinity)
  })
  corners.push(end)
  return [...new Set(corners)].sort((a, b) => a - b)
}

export type HoldSegment = {
  /** The clip whose end value is held, and the lane it sits on. */
  readonly clip: ClipId
  readonly track: PlaylistTrackId
  readonly automation: AutomationId
  readonly start: number
  /** Where another clip of the target takes over, or the end of the song. */
  readonly end: number
  /** The held value, 0 to 1. */
  readonly value: number
}

/**
 * The stretches where a target stays on the value its last clip left: from
 * the end of that clip to the next clip of the same target, or to
 * `songEnd`. These are what make a knob stay where a curve put it.
 */
export function holdSegments(
  lanes: readonly Lane[],
  songEnd: number
): HoldSegment[] {
  const segments: HoldSegment[] = []
  for (const lane of lanes) {
    lane.spans.forEach((span, index) => {
      if (span.points) return
      const end = lane.spans[index + 1]?.start ?? songEnd
      if (end <= span.start) return
      segments.push({
        clip: span.clip,
        track: span.track,
        automation: span.automation,
        start: span.start,
        end,
        value: span.held,
      })
    })
  }
  return segments
}
