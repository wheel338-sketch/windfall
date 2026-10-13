/*
 * Layout: a notation model placed on pages. Measures get a width from their
 * rhythm, fill systems that are stretched to the staff width, and systems
 * fill pages. The result is a list of drawing primitives per page. Pure and
 * deterministic: the same score and options always give the same pages.
 */

import {
  boundsOf,
  translateOp,
  type DrawOp,
  type PathCommand,
  type ScorePage,
} from "./draw"
import { fitText } from "./fonts"
import {
  accidental,
  ACCIDENTAL_HALF,
  bassClef,
  BEAM_GAP,
  BEAM_THICKNESS,
  blockRest,
  brace,
  CLEF_WIDTH,
  DIGIT_WIDTH,
  dot,
  flags,
  HEAD_HALF,
  hookRest,
  notehead,
  numerals,
  quarterRest,
  STEM_WIDTH,
  tie,
  trebleClef,
  WHOLE_HEAD_HALF,
} from "./glyphs"
import type {
  Score,
  ScoreEvent,
  ScoreMeasure,
  ScoreVoice,
  StaffKind,
} from "./model"
import { translatedText as t } from "@/lib/i18n"

export type PaperSize = "a4" | "letter"

/** Portrait sizes in points, with the name CSS `@page` uses. */
export const PAPERS: Record<
  PaperSize,
  { width: number; height: number; label: string; css: string }
> = {
  a4: { width: 595.28, height: 841.89, label: "A4", css: "A4" },
  letter: { width: 612, height: 792, get label() { return t("US Letter") }, css: "letter" },
}

export type LayoutOptions = {
  paper?: PaperSize
  title?: string
  subtitle?: string
  composer?: string
  /** Drawn as a quarter note and a number above the first system. */
  tempoBpm?: number
}

export type SystemInfo = {
  page: number
  /** Indices into `Score.measures`. */
  measures: number[]
  left: number
  right: number
  /** The top line of the top staff and the bottom line of the bottom one. */
  top: number
  bottom: number
}

export type ScoreLayout = {
  paper: PaperSize
  width: number
  height: number
  pages: ScorePage[]
  systems: SystemInfo[]
}

/** One staff space in points: a five-line staff is 20 pt, about 7 mm, high. */
export const STAFF_SPACE = 5
const SP = STAFF_SPACE
export const MARGIN_X = 42
export const MARGIN_TOP = 42
export const MARGIN_BOTTOM = 50

const STAFF_LINE = 0.1 * SP
const BAR_LINE = 0.16 * SP
const STAFF_HEIGHT = 4 * SP
const MIN_STAFF_GAP = 7 * SP
const SYSTEM_GAP = 3.5 * SP
const STEM_LENGTH = 3.5 * SP
const ACCIDENTAL_STEP = 1.2 * SP
const PAD_LEFT = 1.1 * SP
const PAD_RIGHT = 0.6 * SP

/** Staff positions of the key signature accidentals on a treble staff. */
const SHARP_POSITIONS = [8, 5, 9, 6, 3, 7, 4]
const FLAT_POSITIONS = [4, 7, 3, 6, 2, 5, 1]

function yOf(position: number): number {
  return ((8 - position) * SP) / 2
}

function headHalf(event: ScoreEvent): number {
  return (event.value === 1 ? WHOLE_HEAD_HALF : HEAD_HALF) * SP
}

/** The room a duration asks for before the next note, in points. */
function idealGap(ticks: number): number {
  return SP * Math.max(1.7, 1.7 + Math.log2(ticks / 120))
}

// ---------------------------------------------------------------------------
// Measure plans
// ---------------------------------------------------------------------------

type Geometry = {
  staff: number
  voice: ScoreVoice
  event: ScoreEvent
  beamed: boolean
  /** Sideways move of the whole chord to clear the other voice. */
  shift: number
  /** Sideways move of each head: a second sits across the stem. */
  offsets: number[]
  /** Centre of each head's accidental from the column, or null. */
  accidentals: (number | null)[]
  left: number
  right: number
}

type Column = { tick: number; left: number; right: number; ideal: number }

type Plan = {
  measure: ScoreMeasure
  columns: Column[]
  geometry: Map<ScoreEvent, Geometry>
  /** Room for a time signature at the start of the measure. */
  time: number
}

function headOffsets(event: ScoreEvent): number[] {
  const offsets = event.heads.map(() => 0)
  const across = 2 * headHalf(event) - STEM_WIDTH * SP
  if (event.value === 1 || event.stemUp) {
    for (let index = 1; index < offsets.length; index++)
      if (
        event.heads[index].position - event.heads[index - 1].position === 1 &&
        offsets[index - 1] === 0
      )
        offsets[index] = across
  } else {
    for (let index = offsets.length - 2; index >= 0; index--)
      if (
        event.heads[index + 1].position - event.heads[index].position === 1 &&
        offsets[index + 1] === 0
      )
        offsets[index] = -across
  }
  return offsets
}

function planMeasure(measure: ScoreMeasure): Plan {
  const geometry = new Map<ScoreEvent, Geometry>()
  const columns = new Map<number, Column>()
  const column = (tick: number) => {
    let found = columns.get(tick)
    if (!found) {
      found = { tick, left: 0, right: 0, ideal: 0 }
      columns.set(tick, found)
    }
    return found
  }

  measure.staves.forEach((staff, staffIndex) => {
    const byTick = new Map<number, Geometry[]>()
    for (const voice of staff.voices) {
      const beamed = new Set(voice.beams.flat())
      voice.events.forEach((event, index) => {
        if (event.wholeMeasure) return
        const half = headHalf(event)
        const offsets = headOffsets(event)
        const dots = event.dots ? (0.55 + 0.45 * event.dots) * SP : 0
        const flag =
          !event.rest && !beamed.has(index) && event.value >= 8 && event.stemUp
            ? SP
            : 0
        const made: Geometry = {
          staff: staffIndex,
          voice,
          event,
          beamed: beamed.has(index),
          shift: 0,
          offsets,
          accidentals: event.heads.map(() => null),
          left: event.rest
            ? 0.65 * SP
            : half + (offsets.some((offset) => offset < 0) ? 2 * half : 0),
          right: event.rest
            ? 0.65 * SP + dots
            : half +
              (offsets.some((offset) => offset > 0) ? 2 * half : 0) +
              Math.max(dots, flag),
        }
        geometry.set(event, made)
        const list = byTick.get(event.start)
        if (list) list.push(made)
        else byTick.set(event.start, [made])
      })
    }
    for (const [tick, together] of byTick) {
      // Two voices a step apart cannot share a column: the second moves over.
      const [first, second] = together
      if (
        second &&
        !first.event.rest &&
        !second.event.rest &&
        first.event.heads.some((a) =>
          second.event.heads.some((b) => Math.abs(a.position - b.position) <= 1)
        )
      ) {
        second.shift = first.right + headHalf(second.event) + 0.1 * SP
        second.right += second.shift
      }
      // Accidentals stack leftward in as many files as it takes to keep
      // those closer than a seventh apart from touching.
      let left = Math.max(...together.map((item) => item.left))
      const right = Math.max(...together.map((item) => item.right))
      const marked = together
        .flatMap((item) =>
          item.event.heads.map((head, index) => ({ item, head, index }))
        )
        .filter(({ head }) => head.accidental)
        .sort((a, b) => b.head.position - a.head.position)
      const files: number[] = []
      for (const { item, head, index } of marked) {
        let file = files.findIndex((lowest) => lowest - head.position >= 6)
        if (file < 0) file = files.length
        files[file] = head.position
        item.accidentals[index] =
          -(left + (0.3 + ACCIDENTAL_HALF) * SP + file * ACCIDENTAL_STEP)
      }
      if (files.length) left += 0.3 * SP + files.length * ACCIDENTAL_STEP
      const target = column(tick)
      target.left = Math.max(target.left, left)
      target.right = Math.max(target.right, right)
    }
  })

  const ordered = [...columns.values()].sort((a, b) => a.tick - b.tick)
  ordered.forEach((item, index) => {
    const next = ordered[index + 1]?.tick ?? measure.length
    item.ideal = idealGap(next - item.tick)
  })
  const digits = Math.max(
    String(measure.signature.numerator).length,
    String(measure.signature.denominator).length
  )
  return {
    measure,
    columns: ordered,
    geometry,
    time: measure.showSignature ? (digits * DIGIT_WIDTH + 1.2) * SP : 0,
  }
}

/** Where a measure's columns fall, from its left barline, for a stretch. */
function place(plan: Plan, stretch: number) {
  const xs = new Map<number, number>()
  let x = plan.time + PAD_LEFT
  const start = x
  const { columns } = plan
  if (!columns.length) {
    x += Math.max(6 * SP, 6 * SP * stretch)
    return { xs, start, width: x }
  }
  x += columns[0].left
  columns.forEach((item, index) => {
    xs.set(item.tick, x)
    const next = columns[index + 1]
    x += Math.max(
      item.right + (next ? next.left + 0.35 * SP : PAD_RIGHT),
      item.ideal * stretch * (next ? 1 : 0.9)
    )
  })
  return { xs, start, width: Math.max(x, plan.time + 6 * SP) }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

type Placed = {
  plan: Plan
  /** Left barline of the measure. */
  x: number
  /** Where the notes of the measure may begin. */
  start: number
  end: number
  xs: Map<number, number>
}

function keyAccidentals(fifths: number, kind: StaffKind, x: number): DrawOp[] {
  const positions = fifths > 0 ? SHARP_POSITIONS : FLAT_POSITIONS
  const drop = kind === "bass" ? 2 : 0
  return positions
    .slice(0, Math.abs(fifths))
    .flatMap((position, index) =>
      accidental(
        fifths > 0 ? "sharp" : "flat",
        x + (index + 0.5) * SP,
        yOf(position - drop),
        SP
      )
    )
}

function prefixWidth(fifths: number): number {
  const count = Math.abs(fifths)
  return (0.6 + CLEF_WIDTH + 0.4) * SP + (count ? (count + 0.5) * SP : 0)
}

function line(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  width: number
): DrawOp {
  return { type: "line", x1, y1, x2, y2, width }
}

function polygon(points: [number, number][]): DrawOp {
  return {
    type: "path",
    fill: true,
    commands: [
      ...points.map(([x, y], index): PathCommand => [index ? "L" : "M", x, y]),
      ["Z"],
    ],
  }
}

type Stem = {
  geometry: Geometry
  x: number
  up: boolean
  /** The head end and the far end of the stem. */
  base: number
  tip: number
  top: number
  bottom: number
}

/** Everything one staff of one system draws, with y from its top line. */
function drawStaff(
  score: Score,
  staffIndex: number,
  placed: readonly Placed[],
  left: number,
  right: number
): DrawOp[] {
  const kind = score.staves[staffIndex]
  const ops: DrawOp[] = []
  for (let index = 0; index < 5; index++)
    ops.push(line(left, index * SP, right, index * SP, STAFF_LINE))
  const clefX = left + 0.6 * SP
  ops.push(
    ...(kind === "treble"
      ? trebleClef(clefX, 0, SP)
      : bassClef(clefX, 0, SP))
  )
  ops.push(
    ...keyAccidentals(score.key.fifths, kind, clefX + (CLEF_WIDTH + 0.4) * SP)
  )

  /** Ties still open, by voice and key. */
  const open = new Map<string, { x: number; y: number; above: boolean }>()

  placed.forEach((item, measureIndex) => {
    const { measure } = item.plan
    const staff = measure.staves[staffIndex]
    if (measure.showSignature) {
      const cx = item.x + item.plan.time / 2
      ops.push(
        ...numerals(measure.signature.numerator, cx, 0, SP),
        ...numerals(measure.signature.denominator, cx, 2 * SP, SP)
      )
    }
    const twoVoices = staff.voices.length === 2
    for (const voice of staff.voices) {
      const stems = new Map<ScoreEvent, Stem>()
      const extents = new Map<ScoreEvent, { top: number; bottom: number; x: number }>()
      const restShift = twoVoices ? (voice.stems === "up" ? 4 : -4) : 0

      for (const event of voice.events) {
        if (event.wholeMeasure) {
          ops.push(blockRest((item.start + item.end) / 2, yOf(6), SP, true))
          continue
        }
        const geometry = item.plan.geometry.get(event)!
        const x = (item.xs.get(event.start) ?? item.start) + geometry.shift
        if (event.rest) {
          const centre = yOf(4 + restShift)
          if (event.value === 1) ops.push(blockRest(x, yOf(6 + restShift), SP, true))
          else if (event.value === 2) ops.push(blockRest(x, centre, SP, false))
          else if (event.value === 4) ops.push(...quarterRest(x, centre, SP))
          else
            ops.push(
              ...hookRest(x, centre, SP, event.value === 8 ? 1 : event.value === 16 ? 2 : 3)
            )
          for (let index = 0; index < event.dots; index++)
            ops.push(dot(x + (1.0 + index * 0.45) * SP, yOf(5 + restShift), SP))
          extents.set(event, {
            top: centre - 2 * SP,
            bottom: centre + 2 * SP,
            x,
          })
          continue
        }

        const half = headHalf(event)
        const low = event.heads[0].position
        const high = event.heads[event.heads.length - 1].position
        // Ledger lines, each as wide as the heads that need it.
        const ledger = (position: number) => {
          const xs = event.heads
            .map((head, index) => ({ head, x: x + geometry.offsets[index] }))
            .filter(({ head }) =>
              position < 0 ? head.position <= position : head.position >= position
            )
            .map((entry) => entry.x)
          ops.push(
            line(
              Math.min(...xs) - half - 0.3 * SP,
              yOf(position),
              Math.max(...xs) + half + 0.3 * SP,
              yOf(position),
              0.14 * SP
            )
          )
        }
        for (let position = -2; position >= low; position -= 2) ledger(position)
        for (let position = 10; position <= high; position += 2) ledger(position)

        const dotted = new Set<number>()
        const dotX =
          x + Math.max(0, ...geometry.offsets) + half + 0.45 * SP
        event.heads.forEach((head, index) => {
          const hx = x + geometry.offsets[index]
          const hy = yOf(head.position)
          ops.push(notehead(hx, hy, SP, event.value))
          const mark = geometry.accidentals[index]
          if (head.accidental && mark !== null)
            ops.push(
              ...accidental(
                head.accidental,
                x - geometry.shift + mark,
                hy,
                SP
              )
            )
          if (event.dots) {
            // A dot sits in a space: a head on a line dots the space above.
            const space = head.position % 2 ? head.position : head.position + 1
            if (!dotted.has(space)) {
              dotted.add(space)
              for (let dots = 0; dots < event.dots; dots++)
                ops.push(dot(dotX + dots * 0.45 * SP, yOf(space), SP))
            }
          }

          const above = twoVoices ? event.stemUp : !event.stemUp
          const ty = hy + (above ? -0.6 : 0.6) * SP
          const id = `${voice.voice}:${head.key}`
          if (head.tieStop) {
            const from = open.get(id)
            const to = hx - half - 0.15 * SP
            const begin = from ? from.x : item.start - 0.6 * SP
            open.delete(id)
            ops.push(
              tie(Math.min(begin, to - 1.2 * SP), to, ty, SP, from?.above ?? above)
            )
          }
          if (head.tieStart)
            open.set(id, { x: hx + half + 0.15 * SP, y: ty, above })
        })

        let top = yOf(high) - 0.5 * SP
        let bottom = yOf(low) + 0.5 * SP
        if (event.value >= 2) {
          const up = event.stemUp
          const count = event.value >= 8 ? Math.log2(event.value) - 2 : 0
          const extra = geometry.beamed ? 0 : Math.max(0, count - 1) * 0.8 * SP
          let tip = up
            ? yOf(high) - STEM_LENGTH - extra
            : yOf(low) + STEM_LENGTH + extra
          // A stem from far outside the staff reaches the middle line.
          if (!twoVoices) tip = up ? Math.min(tip, yOf(4)) : Math.max(tip, yOf(4))
          stems.set(event, {
            geometry,
            x: up
              ? x + half - (STEM_WIDTH * SP) / 2
              : x - half + (STEM_WIDTH * SP) / 2,
            up,
            base: up ? yOf(low) : yOf(high),
            tip,
            top,
            bottom,
          })
          if (up) top = Math.min(top, tip)
          else bottom = Math.max(bottom, tip)
        }
        extents.set(event, { top, bottom, x })
      }

      // Beams: one straight line per group, tilted a little toward the
      // melody and moved until the shortest stem is long enough.
      for (const run of voice.beams) {
        const group = run
          .map((index) => stems.get(voice.events[index]))
          .filter((stem) => stem !== undefined)
        if (group.length < 2) continue
        const first = group[0]
        const last = group[group.length - 1]
        const up = first.up
        const most = Math.max(...group.map((stem) => Math.log2(stem.geometry.event.value) - 2))
        const reach = (2.6 + (most - 1) * BEAM_GAP) * SP
        const wanted = group.map((stem) => {
          const heads = stem.geometry.event.heads
          return up
            ? yOf(heads[heads.length - 1].position) - Math.max(reach, STEM_LENGTH)
            : yOf(heads[0].position) + Math.max(reach, STEM_LENGTH)
        })
        const span = last.x - first.x || 1
        const slope =
          Math.max(-SP, Math.min(SP, wanted[wanted.length - 1] - wanted[0])) /
          span /
          2
        const lineAt = (x: number, origin: number) =>
          origin + slope * (x - first.x)
        const origins = group.map(
          (stem, index) => wanted[index] - slope * (stem.x - first.x)
        )
        const origin = up ? Math.min(...origins) : Math.max(...origins)
        const thick = BEAM_THICKNESS * SP * (up ? 1 : -1)
        const step = BEAM_GAP * SP * (up ? 1 : -1)
        const halfStem = (STEM_WIDTH * SP) / 2
        const segment = (from: number, to: number, level: number) => {
          const y1 = lineAt(from, origin) + level * step
          const y2 = lineAt(to, origin) + level * step
          ops.push(
            polygon([
              [from, y1],
              [to, y2],
              [to, y2 + thick],
              [from, y1 + thick],
            ])
          )
        }
        group.forEach((stem, index) => {
          stem.tip = lineAt(stem.x, origin)
          const extent = extents.get(stem.geometry.event)!
          if (up) extent.top = Math.min(extent.top, stem.tip)
          else extent.bottom = Math.max(extent.bottom, stem.tip)
          const beams = Math.log2(stem.geometry.event.value) - 2
          const next = group[index + 1]
          const previous = group[index - 1]
          const count = (other: Stem | undefined) =>
            other ? Math.log2(other.geometry.event.value) - 2 : 0
          for (let level = 0; level < beams; level++) {
            if (count(next) > level)
              segment(stem.x - halfStem, next.x + halfStem, level)
            else if (count(previous) <= level) {
              // No neighbour shares this beam: a stub toward the group.
              const toward = previous ? -1 : 1
              const from = stem.x + (toward < 0 ? halfStem : -halfStem)
              const to = from + toward * 1.1 * SP
              segment(Math.min(from, to), Math.max(from, to), level)
            }
          }
        })
      }

      for (const stem of stems.values()) {
        const { event } = stem.geometry
        ops.push(line(stem.x, stem.base, stem.x, stem.tip, STEM_WIDTH * SP))
        if (!stem.geometry.beamed && event.value >= 8)
          ops.push(
            ...flags(
              stem.x,
              stem.tip,
              SP,
              stem.up,
              Math.log2(event.value) - 2
            )
          )
      }

      // Triplets: a bracket on the stem side with a 3 in its gap.
      for (const tuplet of voice.tuplets) {
        if (tuplet.first < 0) continue
        const members = voice.events
          .slice(tuplet.first, tuplet.last + 1)
          .map((event) => ({ event, extent: extents.get(event) }))
          .filter((member) => member.extent !== undefined)
        if (!members.length) continue
        const notes = members.filter((member) => !member.event.rest)
        const above =
          voice.stems === "auto"
            ? notes.filter((member) => member.event.stemUp).length * 2 >=
              notes.length
            : voice.stems === "up"
        const y = above
          ? Math.min(-1.2 * SP, ...members.map((m) => m.extent!.top - 0.9 * SP))
          : Math.max(
              STAFF_HEIGHT + 1.2 * SP,
              ...members.map((m) => m.extent!.bottom + 0.9 * SP)
            )
        let x1 = members[0].extent!.x - HEAD_HALF * SP
        let x2 = members[members.length - 1].extent!.x + HEAD_HALF * SP
        if (x2 - x1 < 3 * SP) {
          const grow = (3 * SP - (x2 - x1)) / 2
          x1 -= grow
          x2 += grow
        }
        const middle = (x1 + x2) / 2
        const hook = (above ? 0.6 : -0.6) * SP
        const thin = 0.12 * SP
        ops.push(
          line(x1, y + hook, x1, y, thin),
          line(x1, y, middle - 0.8 * SP, y, thin),
          line(middle + 0.8 * SP, y, x2, y, thin),
          line(x2, y, x2, y + hook, thin),
          ...numerals(3, middle, y - 0.7 * SP, SP, 0.7)
        )
      }
    }

    // A tie that never met its note on this system runs to the system end.
    if (measureIndex === placed.length - 1) {
      for (const pending of open.values())
        ops.push(
          tie(
            pending.x,
            Math.max(pending.x + 1.2 * SP, right - 0.3 * SP),
            pending.y,
            SP,
            pending.above
          )
        )
      open.clear()
    }
  })

  return ops
}

type System = {
  ops: DrawOp[]
  measures: number[]
  left: number
  right: number
  /** Extent above the top staff line (negative) and below it. */
  top: number
  bottom: number
  /** The bottom line of the bottom staff, from the top line of the top one. */
  staffBottom: number
}

function drawSystem(
  score: Score,
  plans: readonly Plan[],
  left: number,
  width: number,
  lastOfScore: boolean
): System {
  const grand = score.staves.length > 1
  const prefix = prefixWidth(score.key.fifths)
  const room = width - prefix
  const total = (stretch: number) =>
    plans.reduce((sum, plan) => sum + place(plan, stretch).width, 0)

  // Stretch the rhythmic spacing until the measures fill the system. The
  // last system of a score is only filled when it is already mostly full.
  let stretch = 1.15
  let scale = 1
  const least = total(0)
  const justified = !lastOfScore || least > room || total(1) >= room * 0.6
  if (least > room) {
    stretch = 0
    scale = room / least
  } else if (justified) {
    let low = 0
    let high = 64
    for (let turn = 0; turn < 48; turn++) {
      const middle = (low + high) / 2
      if (total(middle) > room) high = middle
      else low = middle
    }
    stretch = low
  }

  const placed: Placed[] = []
  let x = left + prefix
  for (const plan of plans) {
    const laid = place(plan, stretch)
    const xs = new Map<number, number>()
    for (const [tick, offset] of laid.xs) xs.set(tick, x + offset * scale)
    placed.push({
      plan,
      x,
      start: x + laid.start * scale,
      end: x + laid.width * scale,
      xs,
    })
    x += laid.width * scale
  }
  const right = justified ? left + width : x
  if (justified && placed.length) placed[placed.length - 1].end = right

  const staffOps = score.staves.map((_, index) =>
    drawStaff(score, index, placed, left, right)
  )
  const ops: DrawOp[] = [...staffOps[0]]
  let staffBottom = STAFF_HEIGHT
  if (grand) {
    const upper = boundsOf(staffOps[0])
    const lower = boundsOf(staffOps[1])
    const offset = Math.max(
      STAFF_HEIGHT + MIN_STAFF_GAP,
      upper.y1 - lower.y0 + 0.6 * SP
    )
    ops.push(...staffOps[1].map((op) => translateOp(op, 0, offset)))
    staffBottom = offset + STAFF_HEIGHT
    ops.push(
      line(left, 0, left, staffBottom, BAR_LINE),
      brace(left - 0.5 * SP, 0, staffBottom, SP)
    )
  }
  placed.forEach((item, index) => {
    const final = lastOfScore && index === placed.length - 1
    if (final) {
      ops.push(
        line(item.end - 0.95 * SP, 0, item.end - 0.95 * SP, staffBottom, BAR_LINE),
        line(item.end - 0.25 * SP, 0, item.end - 0.25 * SP, staffBottom, 0.5 * SP)
      )
    } else ops.push(line(item.end, 0, item.end, staffBottom, BAR_LINE))
  })
  const first = plans[0].measure
  if (first.number > 1)
    ops.push({
      type: "text",
      x: left,
      y: -2.6 * SP,
      text: String(first.number),
      size: 7,
      font: "sans",
      anchor: "start",
    })
  const bounds = boundsOf(ops)
  return {
    ops,
    measures: plans.map((plan) => plan.measure.index),
    left: grand ? left - 2 * SP : left,
    right,
    top: Math.min(0, bounds.y0),
    bottom: Math.max(staffBottom, bounds.y1),
    staffBottom,
  }
}

function formatTempo(bpm: number): string {
  return String(Math.round(bpm * 100) / 100)
}

/** Lays a score out on pages of one paper size. */
export function layoutScore(
  score: Score,
  options: LayoutOptions = {}
): ScoreLayout {
  const paper = options.paper ?? "a4"
  const { width, height } = PAPERS[paper]
  const grand = score.staves.length > 1
  // A grand staff keeps room for its brace left of the staves.
  const left = MARGIN_X + (grand ? 2 * SP : 0)
  const staffWidth = width - MARGIN_X - left
  const room = staffWidth - prefixWidth(score.key.fifths)

  // Break into systems: measures are added while their natural widths fit.
  const plans = score.measures.map(planMeasure)
  const groups: Plan[][] = []
  let current: Plan[] = []
  let used = 0
  for (const plan of plans) {
    const natural = place(plan, 1).width
    if (current.length && used + natural > room) {
      groups.push(current)
      current = []
      used = 0
    }
    current.push(plan)
    used += natural
  }
  if (current.length) groups.push(current)

  const pages: ScorePage[] = [{ width, height, ops: [] }]
  const systems: SystemInfo[] = []
  let y = MARGIN_TOP
  const text = (
    value: string,
    x: number,
    baseline: number,
    size: number,
    font: "serif" | "serif-bold" | "serif-italic" | "sans",
    anchor: "start" | "middle" | "end"
  ) => {
    const fitted = fitText(value, font, size, width - 2 * MARGIN_X)
    pages[pages.length - 1].ops.push({
      type: "text",
      x,
      y: baseline,
      text: fitted.text,
      size: fitted.size,
      font,
      anchor,
    })
  }
  const title = options.title?.trim()
  const subtitle = options.subtitle?.trim()
  const composer = options.composer?.trim()
  if (title) {
    text(title, width / 2, y + 20, 20, "serif-bold", "middle")
    y += 28
  }
  if (subtitle) {
    text(subtitle, width / 2, y + 12, 12, "serif", "middle")
    y += 18
  }
  if (composer) {
    text(composer, width - MARGIN_X, y + 10, 10, "serif-italic", "end")
    y += 14
  }
  if (options.tempoBpm !== undefined && Number.isFinite(options.tempoBpm)) {
    const scale = 3.2
    const cx = left + HEAD_HALF * scale
    const cy = y + 11
    const stemX = cx + (HEAD_HALF - STEM_WIDTH / 2) * scale
    pages[0].ops.push(
      notehead(cx, cy, scale, 4),
      line(stemX, cy, stemX, cy - 3.2 * scale, STEM_WIDTH * scale)
    )
    text(
      `= ${formatTempo(options.tempoBpm)}`,
      cx + 2 * scale,
      y + 13.5,
      10,
      "serif-bold",
      "start"
    )
    y += 18
  }
  if (y > MARGIN_TOP) y += 4

  const limit = height - MARGIN_BOTTOM
  let onPage = 0
  groups.forEach((group, index) => {
    const system = drawSystem(
      score,
      group,
      left,
      staffWidth,
      index === groups.length - 1
    )
    if (onPage > 0 && y - system.top + system.bottom > limit) {
      pages.push({ width, height, ops: [] })
      y = MARGIN_TOP
      onPage = 0
    }
    const origin = y - system.top
    const page = pages[pages.length - 1]
    for (const op of system.ops) page.ops.push(translateOp(op, 0, origin))
    systems.push({
      page: pages.length - 1,
      measures: system.measures,
      left: system.left,
      right: system.right,
      top: origin,
      bottom: origin + system.staffBottom,
    })
    y = origin + system.bottom + SYSTEM_GAP
    onPage += 1
  })

  pages.forEach((page, index) => {
    page.ops.push({
      type: "text",
      x: width / 2,
      y: height - 28,
      text: `${index + 1} / ${pages.length}`,
      size: 9,
      font: "sans",
      anchor: "middle",
    })
  })
  return { paper, width, height, pages, systems }
}
