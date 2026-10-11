import {
  RIFF_ARP_RATES,
  RIFF_BARS_PER_CHORD,
  RIFF_BASS,
  type RiffContext,
  type RiffNote,
  type RiffSettings,
  type RiffSpan,
  type RiffStageId,
} from "./model"
import type { RiffRng } from "./rng"
import { chordKeys, nearestInScale, progressionDegrees } from "./theory"

/**
 * One step of the pipeline. A stage never changes the notes it is given and
 * draws only from the generator it is handed, so the same notes, settings,
 * context and seed always give the same notes back.
 */
export type RiffStage<Id extends RiffStageId> = (
  notes: readonly RiffNote[],
  settings: RiffSettings[Id],
  context: RiffContext,
  rng: RiffRng
) => RiffNote[]

const NEUTRAL = 0.5

/** Three decimals, and never negative zero. */
function round3(value: number): number {
  return Math.round(value * 1000) / 1000 + 0
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value))
}

/** The notes of each chord, in the order the chords first appear. */
export function bySpan(notes: readonly RiffNote[]): RiffNote[][] {
  const groups = new Map<number, RiffNote[]>()
  for (const note of notes) {
    const group = groups.get(note.span.index)
    if (group) group.push(note)
    else groups.set(note.span.index, [note])
  }
  return [...groups.values()]
}

/**
 * Stage 1. Block chords from the chosen degrees, one per chord span, cycling
 * until the length is filled. It is the source: any notes passed in are
 * ignored. The last chord is cut short when the length is not a multiple.
 */
export const progressionStage: RiffStage<"progression"> = (
  _notes,
  settings,
  context,
  rng
) => {
  const degrees = progressionDegrees(settings)
  const perChord = RIFF_BARS_PER_CHORD.find(
    (item) => item.value === settings.barsPerChord
  )!.bars
  const chordTicks = Math.max(1, Math.round(context.barTicks * perChord))
  const total = settings.bars * context.barTicks
  const vary = rng.fork("variation")
  const notes: RiffNote[] = []
  for (
    let index = 0, offset = 0;
    offset < total;
    index++, offset += chordTicks
  ) {
    let chord = degrees[index % degrees.length]
    // Two draws per chord whatever the amount, so turning the amount up
    // adds substitutions without moving the ones already there.
    const chance = vary.next()
    const direction = vary.next()
    if (index > 0 && chance < settings.variation / 100)
      chord = {
        ...chord,
        // A third up or a third down: the two chords that share two tones.
        degree: (chord.degree + (direction < 0.5 ? 2 : 5)) % 7,
      }
    const keys = chordKeys(chord, settings)
    const span: RiffSpan = {
      index,
      start: context.origin + offset,
      end: context.origin + Math.min(total, offset + chordTicks),
      root: keys[0],
    }
    for (const key of keys)
      notes.push({
        start: span.start,
        length: span.end - span.start,
        key,
        velocity: context.velocity,
        pan: 0,
        release: NEUTRAL,
        modX: NEUTRAL,
        modY: NEUTRAL,
        role: "chord",
        step: index,
        span,
      })
  }
  return notes
}

/** Raises the lowest key an octave, `turns` times. */
function invert(keys: readonly number[], turns: number): number[] {
  const next = [...keys]
  for (let turn = 0; turn < Math.min(turns, next.length - 1); turn++)
    next[next.indexOf(Math.min(...next))] += 12
  return next
}

/** How far two chords are apart: each key's distance to the other's nearest. */
function movement(from: readonly number[], to: readonly number[]): number {
  const nearest = (key: number, keys: readonly number[]) =>
    Math.min(...keys.map((other) => Math.abs(other - key)))
  return (
    to.reduce((sum, key) => sum + nearest(key, from), 0) +
    from.reduce((sum, key) => sum + nearest(key, to), 0)
  )
}

/**
 * The inversion and octave of `keys` that moves least from `previous`. Ties
 * keep the earlier candidate: the same octave before the one below and the
 * one above, and fewer turns before more.
 */
function smoothest(
  keys: readonly number[],
  previous: readonly number[]
): number[] {
  let best = [...keys]
  let cost = Infinity
  for (const shift of [0, -12, 12]) {
    for (let turns = 0; turns < keys.length; turns++) {
      const candidate = invert(keys, turns).map((key) => key + shift)
      const next = movement(previous, candidate)
      if (next < cost) {
        best = candidate
        cost = next
      }
    }
  }
  return best
}

/**
 * Stage 2. Voices each chord: inversion, spread, strum and an optional bass
 * note under the root. Notes keep their order, so "as played" still means
 * root, third, fifth. Smooth voicing compares inversions before the spread.
 */
export const chordsStage: RiffStage<"chords"> = (notes, settings) => {
  const bass = RIFF_BASS.find((item) => item.value === settings.bass)!
  const result: RiffNote[] = []
  let previous: number[] | null = null
  for (const group of bySpan(notes)) {
    const chord = group.filter((note) => note.role === "chord")
    if (chord.length === 0) {
      result.push(...group)
      continue
    }
    const source = chord.map((note) => note.key)
    let keys: number[] =
      settings.inversion === "auto"
        ? previous
          ? smoothest(source, previous)
          : source
        : invert(source, settings.inversion === "first" ? 1 : settings.inversion === "second" ? 2 : 0)
    previous = keys
    if (keys.length >= 3 && settings.spread !== "close") {
      const rising = keys
        .map((key, index) => ({ key, index }))
        .sort((a, b) => a.key - b.key)
      keys = [...keys]
      if (settings.spread === "open") keys[rising[1].index] += 12
      else keys[rising[rising.length - 2].index] -= 12
    }
    // Strum in pitch order: upward for a positive offset, downward otherwise.
    const order = keys
      .map((key, index) => ({ key, index }))
      .sort((a, b) => (settings.strum < 0 ? b.key - a.key : a.key - b.key))
    const delay = new Map(
      order.map((item, place) => [item.index, place * Math.abs(settings.strum)])
    )
    const span = chord[0].span
    if (bass.semitones > 0)
      result.push({
        ...chord[0],
        key: span.root - bass.semitones,
        start: span.start,
        length: span.end - span.start,
        role: "bass",
      })
    result.push(...group.filter((note) => note.role !== "chord"))
    chord.forEach((note, index) => {
      const offset = Math.min(delay.get(index) ?? 0, note.length - 1)
      result.push({
        ...note,
        key: keys[index],
        start: note.start + offset,
        length: note.length - offset,
      })
    })
  }
  return result
}

/** Index into a pool of `size` keys for step `step` of a pattern. */
function patternIndex(
  pattern: RiffSettings["arpeggio"]["pattern"],
  step: number,
  size: number,
  rng: RiffRng
): number {
  if (pattern === "random") return Math.min(size - 1, Math.floor(rng.next() * size))
  if (pattern === "down") return size - 1 - (step % size)
  if (pattern === "up" || pattern === "played" || size < 2) return step % size
  // There and back without repeating either end.
  const place = step % (2 * size - 2)
  const rising = place < size ? place : 2 * size - 2 - place
  return pattern === "up-down" ? rising : size - 1 - rising
}

/**
 * Stage 3. Replaces each chord with single notes on a grid of the chosen
 * note value. Bass notes are left held. A step takes its velocity and other
 * levels from the chord note it plays, and is cut at the end of its chord.
 */
export const arpeggioStage: RiffStage<"arpeggio"> = (
  notes,
  settings,
  context,
  rng
) => {
  const rate = RIFF_ARP_RATES.find((item) => item.value === settings.rate)!.ticks
  const length = Math.max(1, Math.round((rate * settings.gate) / 100))
  const result: RiffNote[] = []
  for (const group of bySpan(notes)) {
    const chord = group.filter((note) => note.role === "chord")
    result.push(...group.filter((note) => note.role !== "chord"))
    if (chord.length === 0) continue
    const ordered =
      settings.pattern === "played"
        ? [...chord].sort((a, b) => a.start - b.start)
        : [...chord].sort((a, b) => a.key - b.key)
    const tones = ordered.filter(
      (note, index) =>
        ordered.findIndex((other) => other.key === note.key) === index
    )
    const pool = Array.from({ length: settings.octaves }, (_, octave) =>
      tones.map((note) => ({ note, key: note.key + 12 * octave }))
    ).flat()
    const span = chord[0].span
    // A free-running arpeggio keeps one grid and one count from the origin.
    const base = settings.restart ? span.start : context.origin
    for (
      let step = Math.max(0, Math.ceil((span.start - base) / rate));
      base + step * rate < span.end;
      step++
    ) {
      const start = base + step * rate
      const tone = pool[patternIndex(settings.pattern, step, pool.length, rng)]
      result.push({
        ...tone.note,
        key: tone.key,
        start,
        length: Math.min(length, span.end - start),
        step,
      })
    }
  }
  return result
}

/**
 * Stage 4. Turns pitches upside down around each chord's middle or one key,
 * and plays each chord span backwards. Bass notes keep their pitch.
 */
export const mirrorStage: RiffStage<"mirror"> = (notes, settings, context) => {
  const result = new Map<RiffNote, RiffNote>()
  for (const group of bySpan(notes)) {
    const chord = group.filter((note) => note.role === "chord")
    const keys = chord.map((note) => note.key)
    const steps = chord.map((note) => note.step)
    const turn =
      settings.invert === "chord"
        ? Math.min(...keys) + Math.max(...keys)
        : 2 * settings.axis
    const stepTurn = Math.min(...steps) + Math.max(...steps)
    for (const note of group) {
      let next = note
      if (settings.invert !== "off" && note.role === "chord") {
        const key = turn - note.key
        next = {
          ...next,
          key: settings.keepInScale ? nearestInScale(key, context.scale) : key,
        }
      }
      if (settings.reverse)
        next = {
          ...next,
          start: note.span.start + note.span.end - (note.start + note.length),
          step: note.role === "chord" ? stepTurn - note.step : note.step,
        }
      result.set(note, next)
    }
  }
  return notes.map((note) => result.get(note)!)
}

/**
 * Stage 5. Velocity, pan and the three expression values. Velocity is the
 * base plus an accent on every Nth step, a ramp across the whole phrase and
 * a random offset, held to 0–1. Bass notes take no accent and stay centred.
 */
export const levelsStage: RiffStage<"levels"> = (
  notes,
  settings,
  _context,
  rng
) => {
  const velocityRng = rng.fork("velocity")
  const panRng = rng.fork("pan")
  const first = Math.min(...notes.map((note) => note.span.start))
  const phrase = Math.max(...notes.map((note) => note.span.end)) - first
  const amount = settings.panAmount / 100
  return notes.map((note) => {
    // One draw per note from each stream, used or not.
    const jitter = velocityRng.next() * 2 - 1
    const side = panRng.next() * 2 - 1
    const chord = note.role === "chord"
    const accent =
      chord &&
      settings.accentEvery > 0 &&
      note.step % settings.accentEvery === 0
        ? settings.accentAmount
        : 0
    const position = phrase > 0 ? (note.start - first) / phrase : 0
    const pan =
      !chord || settings.panMode === "centre"
        ? 0
        : settings.panMode === "alternate"
          ? note.step % 2 === 0
            ? -amount
            : amount
          : amount * side
    return {
      ...note,
      velocity: round3(
        clamp(
          (settings.base +
            accent +
            settings.ramp * position +
            settings.random * jitter) /
            100,
          0,
          1
        )
      ),
      pan: round3(clamp(pan, -1, 1)),
      release: settings.release / 100,
      modX: settings.modX / 100,
      modY: settings.modY / 100,
    }
  })
}

/**
 * Stage 6. Note lengths, one chord and one voice (chord notes or bass) at a
 * time. Ties come first: a note that repeats the pitch before it is merged
 * into that note. Then the length mode, then the minimum length.
 */
export const articulationStage: RiffStage<"articulation"> = (
  notes,
  settings
) => {
  const result = new Map<RiffNote, RiffNote | null>()
  for (const group of bySpan(notes)) {
    for (const role of ["bass", "chord"] as const) {
      const voice = group
        .filter((note) => note.role === role)
        .sort((a, b) => a.start - b.start)
      const kept: { note: RiffNote; end: number }[] = []
      for (const note of voice) {
        const last = kept.at(-1)
        if (settings.tie && last && last.note.key === note.key) {
          last.end = Math.max(last.end, note.start + note.length)
          result.set(note, null)
        } else kept.push({ note, end: note.start + note.length })
      }
      kept.forEach(({ note, end }, index) => {
        const room = note.span.end - note.start
        let length = end - note.start
        if (settings.mode === "legato") {
          const next = kept
            .slice(index + 1)
            .find((other) => other.note.start > note.start)
          // Only ever longer, so the notes of a strummed chord keep ringing.
          length = Math.max(
            length,
            (next ? next.note.start : note.span.end) - note.start
          )
        } else if (settings.mode === "staccato") {
          length = Math.round(length / 2)
        } else {
          length = Math.min(
            Math.round((length * settings.multiplier) / 100),
            Math.max(room, length)
          )
        }
        result.set(note, {
          ...note,
          length: Math.max(1, settings.minLength, length),
        })
      })
    }
  }
  return notes.flatMap((note) => result.get(note) ?? [])
}

/**
 * Stage 7. Timing. Swing delays arpeggio steps with an odd count by up to a
 * third of the grid, the triplet feel. Humanise then moves each start by a
 * random whole number of ticks. Both move only the start: the end stays, the
 * note stays inside its own chord span and never comes before the origin.
 */
export const grooveStage: RiffStage<"groove"> = (
  notes,
  settings,
  context,
  rng
) => {
  const humanize = rng.fork("humanize")
  const swing = Math.round(((settings.swing / 100) * context.grid) / 3)
  return notes.map((note) => {
    const drift = Math.round((humanize.next() * 2 - 1) * settings.humanize)
    const end = note.start + note.length
    const earliest = Math.max(context.origin, note.span.start)
    const latest = Math.max(earliest, note.span.end - 1)
    const swung =
      note.role === "chord" && note.step % 2 === 1 ? note.start + swing : note.start
    const start = clamp(swung + drift, earliest, latest)
    return { ...note, start, length: Math.max(1, end - start) }
  })
}

/** Moves a key by octaves until it is inside the inclusive range. */
function fold(key: number, low: number, high: number): number {
  let next = key
  while (next < low) next += 12
  while (next > high) next -= 12
  return next < low ? low : next
}

/**
 * Stage 8. Brings every key into the chosen range by octaves, pulls it onto
 * the scale, and drops a note that starts on the same tick and key as a
 * longer one. With `enabled` false only the MIDI range 0–127 is applied.
 */
export const fitStage: RiffStage<"fit"> = (notes, settings, context) => {
  const low = settings.enabled ? settings.low : 0
  const high = settings.enabled ? settings.high : 127
  const fitted = notes.map((note) => {
    const key = fold(Math.round(note.key), low, high)
    return {
      ...note,
      key:
        settings.enabled && settings.keepInScale
          ? nearestInScale(key, context.scale, low, high)
          : key,
    }
  })
  if (!settings.enabled || !settings.dropDuplicates) return fitted
  const longest = new Map<string, RiffNote>()
  for (const note of fitted) {
    const id = `${note.start}:${note.key}`
    const other = longest.get(id)
    if (!other || note.length > other.length) longest.set(id, note)
  }
  return fitted.filter(
    (note) => longest.get(`${note.start}:${note.key}`) === note
  )
}
