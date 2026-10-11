/*
 * The notation model: one channel of a pattern turned into measures of
 * notes, rests, ties, tuplets, beams and accidentals. Pure data, no layout
 * and no DOM. Ticks are project ticks; positions inside a measure count from
 * the measure's start.
 */

import type { ChannelId, Pattern, TimeSignature } from "@/bindings"
import { ticksPerBar } from "@/lib/time"
import { MAX_PATTERN_TICKS, PPQ, TICKS_PER_STEP } from "@/lib/units"

import { detectKey, keyAlter, spell, type KeySignature } from "./keys"

export const WHOLE = PPQ * 4
/** A 1/32 note, the finest straight position the model can write. */
const FINE = WHOLE / 32
const QUARTER = PPQ
const EIGHTH = PPQ / 2
const TRIPLET_EIGHTH = QUARTER / 3
const TRIPLET_SIXTEENTH = EIGHTH / 3
/** Notes are cut here so a corrupt length cannot produce an endless score. */
const SCORE_LIMIT = MAX_PATTERN_TICKS * 2
/** Middle C: this key and above go to the treble staff of a grand staff. */
export const SPLIT_KEY = 60

export type StaffKind = "treble" | "bass"
export type StaffLayout = "grand" | "treble" | "bass"
export type SmallestValue = 8 | 16 | 32
/** The denominator of a note value: 1 is a whole note, 32 a 1/32 note. */
export type NoteValue = 1 | 2 | 4 | 8 | 16 | 32
export type Accidental = "sharp" | "flat" | "natural"

export type ScoreOptions = {
  /** The straight grid note starts and ends snap to. Defaults to 32. */
  smallest?: SmallestValue
  staff?: StaffLayout
  key?: KeySignature | "auto"
  /** The project's base meter, used when the pattern sets none. */
  baseSignature?: TimeSignature
}

export type ScoreHead = {
  id: number
  key: number
  step: number
  alter: -1 | 0 | 1
  octave: number
  /** Staff position: 0 is the bottom line, 8 the top line, 1 per step. */
  position: number
  accidental: Accidental | null
  tieStart: boolean
  tieStop: boolean
}

export type ScoreEvent = {
  start: number
  ticks: number
  value: NoteValue
  dots: 0 | 1 | 2
  /** Index into the voice's tuplets, or -1. */
  tuplet: number
  rest: boolean
  wholeMeasure: boolean
  stemUp: boolean
  /** Lowest first. Empty for a rest. */
  heads: ScoreHead[]
}

export type ScoreTuplet = {
  start: number
  end: number
  /** The written value of one of the three slots. */
  unit: 8 | 16
  first: number
  last: number
}

export type ScoreVoice = {
  voice: 0 | 1
  stems: "auto" | "up" | "down"
  events: ScoreEvent[]
  tuplets: ScoreTuplet[]
  /** Runs of event indices joined by a beam. */
  beams: number[][]
}

export type ScoreMeasure = {
  index: number
  number: number
  start: number
  length: number
  signature: TimeSignature
  /** True where the time signature is drawn: bar one and every change. */
  showSignature: boolean
  /** A bar cut short by a meter change that does not land on a barline. */
  irregular: boolean
  /** One per staff, in the order of `Score.staves`. */
  staves: { voices: ScoreVoice[] }[]
}

export type ScoreStats = {
  /** Notes read from the lane. */
  notes: number
  /** Notes whose start or end moved to reach a writable position. */
  quantised: number
  /** Notes merged, shortened or lengthened to fit two voices per staff. */
  simplified: number
}

export type Score = {
  key: KeySignature
  keyDetected: boolean
  staves: StaffKind[]
  measures: ScoreMeasure[]
  stats: ScoreStats
}

const COMMON_TIME: TimeSignature = { numerator: 4, denominator: 4 }

function validSignature(signature: TimeSignature | undefined) {
  return (
    !!signature &&
    Number.isInteger(signature.numerator) &&
    signature.numerator >= 1 &&
    signature.numerator <= 64 &&
    [2, 4, 8, 16].includes(signature.denominator)
  )
}

// ---------------------------------------------------------------------------
// Meter
// ---------------------------------------------------------------------------

type Metric = {
  length: number
  /** Beats are whole denominator units; otherwise they are groups of them. */
  simple: boolean
  unit: number
  /** Beat boundaries from 0 to the measure length, both included. */
  beats: number[]
  half: number | null
  /** Beam group boundaries from 0 to the measure length. */
  groups: number[]
  numerator: number
}

/** Eighth or sixteenth meters count in groups: 6/8 is 3+3, 7/8 is 2+2+3. */
function groupSizes(numerator: number): number[] {
  if (numerator <= 3) return [numerator]
  if (numerator % 3 === 0) return Array<number>(numerator / 3).fill(3)
  const sizes = Array<number>(Math.floor(numerator / 2)).fill(2)
  if (numerator % 2) sizes[sizes.length - 1] = 3
  return sizes
}

function boundaries(sizes: number[], unit: number, length: number): number[] {
  const result = [0]
  for (const size of sizes) {
    const next = Math.min(length, result[result.length - 1] + size * unit)
    if (next > result[result.length - 1]) result.push(next)
  }
  if (result[result.length - 1] < length) result.push(length)
  return result
}

function metricOf(signature: TimeSignature, length: number): Metric {
  const unit = WHOLE / signature.denominator
  const full = signature.numerator * unit
  const simple = signature.denominator <= 4
  const beats = boundaries(
    simple
      ? Array<number>(signature.numerator).fill(1)
      : groupSizes(signature.numerator),
    unit,
    length
  )
  const halved = simple ? signature.numerator === 4 : signature.numerator === 12
  return {
    length,
    simple,
    unit,
    beats,
    half: halved && length === full ? full / 2 : null,
    groups: simple
      ? boundaries(
          Array<number>(Math.ceil(length / QUARTER)).fill(1),
          QUARTER,
          length
        )
      : beats,
    numerator: signature.numerator,
  }
}

function isBeat(metric: Metric, position: number): boolean {
  return metric.beats.includes(position)
}

/**
 * How strong a position is: 0 the barline, 1 the half bar, 2 a beat, then one
 * more for each halving below the beat. Smaller is stronger.
 */
function levelAt(metric: Metric, position: number): number {
  if (position === 0 || position === metric.length) return 0
  if (position === metric.half) return 1
  let beat = 0
  for (const boundary of metric.beats) if (boundary <= position) beat = boundary
  if (beat === position) return 2
  const offset = position - beat
  let level = 3
  for (
    let step = metric.simple ? metric.unit / 2 : metric.unit;
    step >= FINE;
    step /= 2
  ) {
    if (offset % step === 0) return level
    level += 1
  }
  return level
}

type Bar = {
  index: number
  start: number
  length: number
  signature: TimeSignature
  metric: Metric
  /** The straight grid of this bar. */
  grid: number
  showSignature: boolean
  irregular: boolean
}

/** The pattern's bars, made on demand as far as the notes reach. */
class Bars {
  readonly list: Bar[] = []
  private readonly changes: { tick: number; signature: TimeSignature }[]

  constructor(
    private readonly grid: number,
    base: TimeSignature,
    meters: readonly { tick: number; signature: TimeSignature }[]
  ) {
    this.changes = [{ tick: 0, signature: base }]
    for (const meter of [...meters].sort((a, b) => a.tick - b.tick)) {
      if (!validSignature(meter.signature) || !Number.isFinite(meter.tick))
        continue
      // A change lands on the grid so the bar before it can be filled.
      const tick = Math.max(0, Math.round(meter.tick / grid) * grid)
      const last = this.changes[this.changes.length - 1]
      if (tick === last.tick) last.signature = meter.signature
      else this.changes.push({ tick, signature: meter.signature })
    }
  }

  private add(): Bar {
    const previous = this.list[this.list.length - 1]
    const start = previous ? previous.start + previous.length : 0
    let segment = 0
    while (
      segment + 1 < this.changes.length &&
      this.changes[segment + 1].tick <= start
    )
      segment += 1
    const { signature } = this.changes[segment]
    const full = ticksPerBar(signature)
    const next = this.changes[segment + 1]?.tick ?? Infinity
    const length = Math.min(full, next - start)
    const bar: Bar = {
      index: this.list.length,
      start,
      length,
      signature,
      metric: metricOf(signature, length),
      grid: Math.min(this.grid, WHOLE / signature.denominator),
      showSignature:
        !previous ||
        previous.signature.numerator !== signature.numerator ||
        previous.signature.denominator !== signature.denominator,
      irregular: length !== full,
    }
    this.list.push(bar)
    return bar
  }

  /** Makes sure the bars cover every tick before `end`. */
  cover(end: number): void {
    let last = this.list[this.list.length - 1] ?? this.add()
    while (last.start + last.length < end) last = this.add()
  }

  at(tick: number): Bar {
    this.cover(tick + 1)
    let low = 0
    let high = this.list.length - 1
    while (low < high) {
      const middle = (low + high + 1) >> 1
      if (this.list[middle].start <= tick) low = middle
      else high = middle - 1
    }
    return this.list[low]
  }
}

// ---------------------------------------------------------------------------
// Quantisation
// ---------------------------------------------------------------------------

/** Which beats of a bar one staff writes as triplets. */
type TupletGrid = { eighths: Set<number>; sixteenths: Set<number> }
type StaffGrid = Map<number, TupletGrid>

const NO_TUPLETS: TupletGrid = { eighths: new Set(), sixteenths: new Set() }

function distance(position: number, base: number, step: number): number {
  const offset = (((position - base) % step) + step) % step
  return Math.min(offset, step - offset)
}

/**
 * Decides, per quarter of each bar, whether the note starts of a staff read
 * better as triplets. A quarter becomes an eighth-note triplet when its
 * starts sit closer to thirds of the quarter than to the 1/32 grid; failing
 * that, each half of it may become a sixteenth-note triplet the same way.
 * Ties keep the straight grid.
 */
function detectTuplets(
  bars: Bars,
  starts: readonly number[],
  smallest: SmallestValue
): StaffGrid {
  const byBar = new Map<number, number[]>()
  for (const start of starts) {
    const bar = bars.at(start)
    const list = byBar.get(bar.index)
    if (list) list.push(start - bar.start)
    else byBar.set(bar.index, [start - bar.start])
  }
  const result: StaffGrid = new Map()
  for (const [index, onsets] of byBar) {
    const bar = bars.list[index]
    const denominator = bar.signature.denominator
    const eighthsAllowed = denominator <= 4
    const sixteenthsAllowed = smallest >= 16 && denominator <= 8
    if (!eighthsAllowed && !sixteenthsAllowed) continue
    const grid: TupletGrid = { eighths: new Set(), sixteenths: new Set() }
    for (let quarter = 0; quarter * QUARTER < bar.length; quarter++) {
      const from = quarter * QUARTER
      const inside = onsets.filter(
        (onset) => onset >= from && onset < from + QUARTER
      )
      if (!inside.length) continue
      const sum = (values: number[], base: number, step: number) =>
        values.reduce((total, value) => total + distance(value, base, step), 0)
      const straight = sum(inside, 0, FINE)
      const halves = [0, 1].map((half) => {
        const base = from + half * EIGHTH
        const part = inside.filter(
          (onset) => onset >= base && onset < base + EIGHTH
        )
        const plain = sum(part, 0, FINE)
        const triplet = sum(part, base, TRIPLET_SIXTEENTH)
        const fits =
          sixteenthsAllowed && base + EIGHTH <= bar.length && triplet < plain
        return { fits, error: fits ? triplet : plain }
      })
      const whole = sum(inside, from, TRIPLET_EIGHTH)
      if (
        eighthsAllowed &&
        from + QUARTER <= bar.length &&
        whole < straight &&
        whole <= halves[0].error + halves[1].error
      ) {
        grid.eighths.add(quarter)
        continue
      }
      halves.forEach((half, at) => {
        if (half.fits) grid.sixteenths.add(quarter * 2 + at)
      })
    }
    if (grid.eighths.size || grid.sixteenths.size) result.set(index, grid)
  }
  return result
}

/** The writable positions either side of a position inside a bar. */
function neighbours(
  bar: Bar,
  grid: TupletGrid,
  position: number
): [number, number] {
  const quarter = Math.floor(position / QUARTER)
  let base = Math.floor(position / bar.grid) * bar.grid
  let step = bar.grid
  if (grid.eighths.has(quarter)) {
    base = quarter * QUARTER
    step = TRIPLET_EIGHTH
  } else if (grid.sixteenths.has(Math.floor(position / EIGHTH))) {
    base = Math.floor(position / EIGHTH) * EIGHTH
    step = TRIPLET_SIXTEENTH
  }
  const low = base + Math.floor((position - base) / step) * step
  return [low, low + step]
}

type QuantNote = { id: number; key: number; start: number; end: number }

// ---------------------------------------------------------------------------
// Voices
// ---------------------------------------------------------------------------

type Chord = { start: number; end: number; notes: QuantNote[] }

/**
 * Notes that start and end together are one chord. Chords go to the first
 * free voice of two. When both are busy, a chord that starts with another
 * takes that chord's length, and otherwise the voice that would finish
 * first is cut short at the new start.
 */
function assignVoices(
  notes: readonly QuantNote[],
  simplified: Set<number>
): [Chord[], Chord[]] {
  const chords: Chord[] = []
  const byShape = new Map<string, Chord>()
  for (const note of notes) {
    const shape = `${note.start}:${note.end}`
    const chord = byShape.get(shape)
    if (chord) chord.notes.push(note)
    else {
      const made = { start: note.start, end: note.end, notes: [note] }
      byShape.set(shape, made)
      chords.push(made)
    }
  }
  const top = (chord: Chord) => Math.max(...chord.notes.map((note) => note.key))
  chords.sort(
    (a, b) => a.start - b.start || top(b) - top(a) || b.end - a.end
  )
  const voices: [Chord[], Chord[]] = [[], []]
  for (const chord of chords) {
    const last = voices.map((voice) => voice[voice.length - 1])
    const free = last.findIndex((item) => !item || item.end <= chord.start)
    if (free >= 0) {
      voices[free].push(chord)
      continue
    }
    const together = last
      .map((item, voice) => ({ item: item!, voice }))
      .filter(({ item }) => item.start === chord.start)
      .sort(
        (a, b) =>
          Math.abs(a.item.end - chord.end) - Math.abs(b.item.end - chord.end) ||
          a.voice - b.voice
      )[0]
    if (together) {
      for (const note of chord.notes) {
        note.end = together.item.end
        simplified.add(note.id)
        together.item.notes.push(note)
      }
      continue
    }
    const voice = last[1]!.end < last[0]!.end ? 1 : last[0]!.end < last[1]!.end ? 0 : 1
    const cut = last[voice]!
    cut.end = chord.start
    for (const note of cut.notes) {
      note.end = chord.start
      simplified.add(note.id)
    }
    voices[voice].push(chord)
  }
  return voices
}

// ---------------------------------------------------------------------------
// Durations
// ---------------------------------------------------------------------------

const VALUES: readonly NoteValue[] = [1, 2, 4, 8, 16, 32]

/** A single written value for a tick length, with up to two dots. */
export function writtenValue(
  ticks: number
): { value: NoteValue; dots: 0 | 1 | 2 } | null {
  for (const value of VALUES) {
    const base = WHOLE / value
    if (ticks === base) return { value, dots: 0 }
    if (base / 2 >= FINE && ticks === base * 1.5) return { value, dots: 1 }
    if (base / 4 >= FINE && ticks === base * 1.75) return { value, dots: 2 }
  }
  return null
}

function fitsAsOne(
  metric: Metric,
  from: number,
  to: number,
  rest: boolean
): { value: NoteValue; dots: 0 | 1 | 2 } | null {
  const written = writtenValue(to - from)
  if (!written) return null
  const level = levelAt(metric, from)
  let crossesBeat = false
  for (let position = from + FINE; position < to; position += FINE) {
    const inner = levelAt(metric, position)
    // A note may not hide a position stronger than where it starts; a rest
    // may not hide one as strong either.
    if (rest ? inner <= level : inner < level) return null
    if (inner <= 2) crossesBeat = true
  }
  if (!crossesBeat) return rest && written.dots === 2 ? null : written
  // In grouped meters a value that crosses a beat must also end on one.
  if (!metric.simple && !isBeat(metric, to)) return null
  if (rest) {
    if (written.dots === 2 || !isBeat(metric, from) || !isBeat(metric, to))
      return null
    if (metric.simple && (written.dots || metric.numerator === 3)) return null
  }
  return written
}

type Piece = { start: number; ticks: number; value: NoteValue; dots: 0 | 1 | 2 }

/** Cuts a straight span into legal values that show the beat structure. */
function splitSpan(
  metric: Metric,
  from: number,
  to: number,
  rest: boolean,
  out: Piece[]
): void {
  const written = fitsAsOne(metric, from, to, rest)
  if (written) {
    out.push({ start: from, ticks: to - from, ...written })
    return
  }
  let strongest = Infinity
  const inner: number[] = []
  for (let position = from + FINE; position < to; position += FINE) {
    const level = levelAt(metric, position)
    if (level < strongest) {
      strongest = level
      inner.length = 0
    }
    if (level === strongest) inner.push(position)
  }
  if (!inner.length) {
    // Shorter than a 1/32 note: cannot happen on the grid; keep it visible.
    out.push({ start: from, ticks: to - from, value: 32, dots: 0 })
    return
  }
  const cut =
    inner.findLast((position) => fitsAsOne(metric, from, position, rest)) ??
    inner[0]
  splitSpan(metric, from, cut, rest, out)
  splitSpan(metric, cut, to, rest, out)
}

// ---------------------------------------------------------------------------
// Measures
// ---------------------------------------------------------------------------

/** Diatonic index of the bottom staff line: E4 for treble, G2 for bass. */
const BOTTOM_LINE: Record<StaffKind, number> = {
  treble: 4 * 7 + 2,
  bass: 2 * 7 + 4,
}

function buildVoice(
  bar: Bar,
  chords: readonly Chord[],
  grid: TupletGrid,
  voice: 0 | 1,
  staff: StaffKind,
  fifths: number
): ScoreVoice {
  const end = bar.start + bar.length
  type Atom = { from: number; to: number; chord: Chord | null }
  const atoms: Atom[] = []
  let cursor = 0
  for (const chord of chords) {
    const from = Math.max(bar.start, chord.start) - bar.start
    const to = Math.min(end, chord.end) - bar.start
    if (from > cursor) atoms.push({ from: cursor, to: from, chord: null })
    atoms.push({ from, to, chord })
    cursor = to
  }
  if (cursor < bar.length)
    atoms.push({ from: cursor, to: bar.length, chord: null })

  // A voice writes a triplet only where one of its own notes needs it.
  const windows: { from: number; to: number; unit: 8 | 16 }[] = []
  const used = (from: number, to: number) =>
    atoms.some((atom) => atom.from > from && atom.from < to)
  for (const quarter of grid.eighths) {
    const from = quarter * QUARTER
    if (used(from, from + QUARTER))
      windows.push({ from, to: from + QUARTER, unit: 8 })
  }
  for (const eighth of grid.sixteenths) {
    const from = eighth * EIGHTH
    if (used(from, from + EIGHTH))
      windows.push({ from, to: from + EIGHTH, unit: 16 })
  }
  windows.sort((a, b) => a.from - b.from)

  const events: ScoreEvent[] = []
  const tuplets: ScoreTuplet[] = windows.map((window) => ({
    start: window.from,
    end: window.to,
    unit: window.unit,
    first: -1,
    last: -1,
  }))
  for (const atom of atoms) {
    const pieces: (Piece & { tuplet: number })[] = []
    let position = atom.from
    while (position < atom.to) {
      const inside = windows.findIndex(
        (window) => position >= window.from && position < window.to
      )
      if (inside >= 0) {
        const window = windows[inside]
        const slot = (window.to - window.from) / 3
        const to = Math.min(atom.to, window.to)
        // One or two slots: the unit value, or the next longer one.
        const single = to - position <= slot
        pieces.push({
          start: position,
          ticks: to - position,
          value: (single ? window.unit : window.unit / 2) as NoteValue,
          dots: 0,
          tuplet: inside,
        })
        position = to
        continue
      }
      const next = windows.find((window) => window.from > position)
      const to = Math.min(atom.to, next?.from ?? atom.to)
      const straight: Piece[] = []
      splitSpan(bar.metric, position, to, !atom.chord, straight)
      for (const piece of straight) pieces.push({ ...piece, tuplet: -1 })
      position = to
    }
    for (const piece of pieces) {
      const chord = atom.chord
      const from = bar.start + piece.start
      const to = from + piece.ticks
      if (piece.tuplet >= 0) {
        const tuplet = tuplets[piece.tuplet]
        if (tuplet.first < 0) tuplet.first = events.length
        tuplet.last = events.length
      }
      events.push({
        start: piece.start,
        ticks: piece.ticks,
        value: piece.value,
        dots: piece.dots,
        tuplet: piece.tuplet,
        rest: !chord,
        wholeMeasure: false,
        stemUp: true,
        heads: chord
          ? [...chord.notes]
              .sort((a, b) => a.key - b.key || a.id - b.id)
              .map((note) => {
                const spelled = spell(note.key, fifths)
                return {
                  id: note.id,
                  key: note.key,
                  ...spelled,
                  position:
                    spelled.octave * 7 + spelled.step - BOTTOM_LINE[staff],
                  accidental: null,
                  tieStart: to < chord.end,
                  tieStop: from > chord.start,
                }
              })
          : [],
      })
    }
  }

  // Eighths and shorter are beamed inside one beam group; a rest ends a beam.
  const beams: number[][] = []
  const groupOf = (position: number) =>
    bar.metric.groups.findLastIndex((boundary) => boundary <= position)
  let run: number[] = []
  const flush = () => {
    if (run.length > 1) beams.push(run)
    run = []
  }
  events.forEach((event, index) => {
    if (event.rest || event.value < 8) return flush()
    if (
      run.length &&
      groupOf(events[run[0]].start) !== groupOf(event.start)
    )
      flush()
    run.push(index)
  })
  flush()
  return { voice, stems: "auto", events, tuplets, beams }
}

function wholeRest(bar: Bar): ScoreVoice {
  return {
    voice: 0,
    stems: "auto",
    events: [
      {
        start: 0,
        ticks: bar.length,
        value: 1,
        dots: 0,
        tuplet: -1,
        rest: true,
        wholeMeasure: true,
        stemUp: true,
        heads: [],
      },
    ],
    tuplets: [],
    beams: [],
  }
}

/** Stems: two voices point apart; one voice follows its average position. */
function setStems(voices: ScoreVoice[]): void {
  const keys = (voice: ScoreVoice) =>
    voice.events.flatMap((event) => event.heads.map((head) => head.key))
  const mean = (values: number[]) =>
    values.reduce((total, value) => total + value, 0) / values.length
  if (voices.length === 2) {
    const upper = mean(keys(voices[1])) > mean(keys(voices[0])) ? 1 : 0
    voices.forEach((voice, index) => {
      voice.stems = index === upper ? "up" : "down"
      for (const event of voice.events) event.stemUp = index === upper
    })
    return
  }
  for (const voice of voices) {
    const beamed = new Set(voice.beams.flat())
    const direct = (indices: number[]) => {
      const positions = indices.flatMap((index) =>
        voice.events[index].heads.map((head) => head.position)
      )
      // The middle line and above take a down stem.
      const up = positions.length > 0 && mean(positions) < 4
      for (const index of indices) voice.events[index].stemUp = up
    }
    voice.beams.forEach(direct)
    voice.events.forEach((event, index) => {
      if (!event.rest && !beamed.has(index)) direct([index])
    })
  }
}

/**
 * An accidental is written when a head differs from what its line holds: the
 * signature at first, then whatever was last written on that line and octave
 * in the bar. A note tied from before repeats nothing.
 */
function setAccidentals(voices: ScoreVoice[], fifths: number): void {
  const state = new Map<number, number>()
  const order = voices
    .flatMap((voice, at) => voice.events.map((event) => ({ event, at })))
    .filter(({ event }) => !event.rest)
    .sort((a, b) => a.event.start - b.event.start || a.at - b.at)
  for (const { event } of order) {
    for (const head of event.heads) {
      if (head.tieStop) continue
      const line = head.octave * 7 + head.step
      const held = state.get(line) ?? keyAlter(head.step, fifths)
      if (held === head.alter) continue
      head.accidental =
        head.alter > 0 ? "sharp" : head.alter < 0 ? "flat" : "natural"
      state.set(line, head.alter)
    }
  }
}

export function buildScore(
  pattern: Pattern,
  channel: ChannelId,
  options: ScoreOptions = {}
): Score {
  const smallest = options.smallest ?? 32
  const layout = options.staff ?? "grand"
  const grid = WHOLE / smallest
  const base = [pattern.timeSignature, options.baseSignature].find(
    validSignature
  ) ?? COMMON_TIME
  const bars = new Bars(grid, base, pattern.timeline?.meters ?? [])

  const source = (
    pattern.lanes.find((lane) => lane.channel === channel)?.notes ?? []
  )
    .filter(
      (note) =>
        Number.isInteger(note.key) &&
        note.key >= 0 &&
        note.key <= 127 &&
        Number.isFinite(note.start) &&
        note.start >= 0 &&
        note.start < SCORE_LIMIT &&
        note.length >= 1
    )
    .sort(
      (a, b) =>
        a.start - b.start || a.key - b.key || b.length - a.length || a.id - b.id
    )

  const weights = Array<number>(12).fill(0)
  for (const note of source) weights[note.key % 12] += note.length
  const key =
    !options.key || options.key === "auto" ? detectKey(weights) : options.key
  const staves: StaffKind[] =
    layout === "grand" ? ["treble", "bass"] : [layout]
  const staffOf = (note: { key: number }) =>
    layout === "grand" && note.key < SPLIT_KEY ? 1 : 0

  const quantised = new Set<number>()
  const simplified = new Set<number>()
  let scoreEnd = pattern.lengthSteps * TICKS_PER_STEP
  const perStaff = staves.map((staff, index) => {
    const raw = source.filter((note) => staffOf(note) === index)
    const tuplets = detectTuplets(
      bars,
      raw.map((note) => note.start),
      smallest
    )
    const snap = (tick: number): number => {
      const bar = bars.at(tick)
      const position = tick - bar.start
      const [low, high] = neighbours(
        bar,
        tuplets.get(bar.index) ?? NO_TUPLETS,
        position
      )
      return bar.start + (position - low <= high - position ? low : high)
    }
    const notes: QuantNote[] = []
    for (const note of raw) {
      const rawEnd = Math.min(SCORE_LIMIT, note.start + note.length)
      const start = snap(note.start)
      let end = snap(rawEnd)
      if (end <= start) {
        // Too short to write: it takes the shortest value at its position.
        const bar = bars.at(start)
        end =
          bar.start +
          neighbours(
            bar,
            tuplets.get(bar.index) ?? NO_TUPLETS,
            start - bar.start
          )[1]
      }
      if (start !== note.start || end !== note.start + note.length)
        quantised.add(note.id)
      notes.push({ id: note.id, key: note.key, start, end })
    }
    // One pitch sounds once: duplicates merge, an overlap ends at the next.
    notes.sort((a, b) => a.key - b.key || a.start - b.start || b.end - a.end)
    const kept: QuantNote[] = []
    for (const note of notes) {
      const previous = kept[kept.length - 1]
      if (previous?.key === note.key) {
        if (previous.start === note.start) {
          simplified.add(note.id)
          continue
        }
        if (previous.end > note.start) {
          previous.end = note.start
          simplified.add(previous.id)
        }
      }
      kept.push(note)
    }
    kept.sort((a, b) => a.start - b.start || a.key - b.key)
    const voices = assignVoices(kept, simplified)
    for (const note of kept) scoreEnd = Math.max(scoreEnd, note.end)
    return { staff, voices, tuplets }
  })

  bars.cover(Math.max(1, scoreEnd))
  // Bars made while snapping past the last note are not part of the score.
  const last = bars.at(Math.max(1, scoreEnd) - 1).index
  const cursors = perStaff.map(() => [0, 0])
  const measures: ScoreMeasure[] = bars.list.slice(0, last + 1).map((bar) => ({
    index: bar.index,
    number: bar.index + 1,
    start: bar.start,
    length: bar.length,
    signature: bar.signature,
    showSignature: bar.showSignature,
    irregular: bar.irregular,
    staves: perStaff.map(({ staff, voices, tuplets }, at) => {
      const end = bar.start + bar.length
      const built = voices
        .map((chords, voice) => {
          const cursor = cursors[at]
          while (
            cursor[voice] < chords.length &&
            chords[cursor[voice]].end <= bar.start
          )
            cursor[voice] += 1
          const inside: Chord[] = []
          for (
            let index = cursor[voice];
            index < chords.length && chords[index].start < end;
            index++
          )
            inside.push(chords[index])
          return inside.length
            ? buildVoice(
                bar,
                inside,
                tuplets.get(bar.index) ?? NO_TUPLETS,
                voice as 0 | 1,
                staff,
                key.fifths
              )
            : null
        })
        .filter((voice) => voice !== null)
      const present = built.length ? built : [wholeRest(bar)]
      setStems(present)
      setAccidentals(present, key.fifths)
      return { voices: present }
    }),
  }))

  return {
    key,
    keyDetected: !options.key || options.key === "auto",
    staves,
    measures,
    stats: {
      notes: source.length,
      quantised: quantised.size,
      simplified: simplified.size,
    },
  }
}
