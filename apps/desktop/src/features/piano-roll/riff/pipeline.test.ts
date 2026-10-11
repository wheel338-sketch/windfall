import { describe, expect, it } from "vitest"

import { MAX_PATTERN_TICKS } from "@/lib/units"

import {
  DEFAULT_RIFF_SETTINGS,
  RiffError,
  type RiffNote,
  type RiffSettings,
} from "./model"
import { riffNoteInits, runRiff, type RiffPlace } from "./pipeline"
import { parseDegrees, progressionDegrees } from "./theory"

/*
 * The staged riff generator as a pure pipeline: what each stage adds to
 * the one before, that one seed gives one riff, and that a riff is refused
 * whole rather than shortened. The dialog is not mounted here.
 */

const BAR = 3840
const PLACE: RiffPlace = { origin: 0, barTicks: BAR, velocity: 0.7 }

type Patch = { [Key in keyof RiffSettings]?: Partial<RiffSettings[Key]> }

/** The defaults with every optional stage off, then `patch` on top. */
function settings(patch: Patch = {}): RiffSettings {
  const base: RiffSettings = {
    ...DEFAULT_RIFF_SETTINGS,
    chords: { ...DEFAULT_RIFF_SETTINGS.chords, enabled: false },
    arpeggio: { ...DEFAULT_RIFF_SETTINGS.arpeggio, enabled: false },
    mirror: { ...DEFAULT_RIFF_SETTINGS.mirror, enabled: false },
    levels: { ...DEFAULT_RIFF_SETTINGS.levels, enabled: false },
    articulation: { ...DEFAULT_RIFF_SETTINGS.articulation, enabled: false },
    groove: { ...DEFAULT_RIFF_SETTINGS.groove, enabled: false },
    fit: { ...DEFAULT_RIFF_SETTINGS.fit, enabled: false },
  }
  const merged = { ...base } as Record<string, unknown>
  for (const [key, value] of Object.entries(patch)) {
    const current = (base as Record<string, unknown>)[key]
    merged[key] =
      typeof value === "object" && value !== null
        ? { ...(current as object), ...value }
        : value
  }
  return merged as RiffSettings
}

const pitchClasses = (notes: readonly RiffNote[]) =>
  [...new Set(notes.map((note) => note.key % 12))].sort((a, b) => a - b)

function chordsOf(notes: readonly RiffNote[]): RiffNote[][] {
  const starts = [...new Set(notes.map((note) => note.span.index))].sort(
    (a, b) => a - b
  )
  return starts.map((index) => notes.filter((note) => note.span.index === index))
}

function sound(notes: readonly RiffNote[]) {
  for (const note of notes) {
    expect(Number.isInteger(note.start)).toBe(true)
    expect(Number.isInteger(note.length)).toBe(true)
    expect(note.length).toBeGreaterThanOrEqual(1)
    expect(note.key).toBeGreaterThanOrEqual(0)
    expect(note.key).toBeLessThanOrEqual(127)
    expect(note.velocity).toBeGreaterThanOrEqual(0)
    expect(note.velocity).toBeLessThanOrEqual(1)
    expect(note.pan).toBeGreaterThanOrEqual(-1)
    expect(note.pan).toBeLessThanOrEqual(1)
  }
}

describe("progression stage", () => {
  it("writes one block chord per bar from the chosen degrees of the key", () => {
    const result = runRiff(settings(), PLACE)
    expect(result.start).toBe(0)
    expect(result.end).toBe(4 * BAR)
    expect(result.counts.progression).toBe(12)
    expect(result.counts.chords).toBeNull()
    expect(result.counts.arpeggio).toBeNull()
    expect(result.chords).toHaveLength(4)
    const chords = chordsOf(result.notes)
    expect(chords).toHaveLength(4)
    // I V vi IV in C major.
    expect(chords.map(pitchClasses)).toEqual([
      [0, 4, 7],
      [2, 7, 11],
      [0, 4, 9],
      [0, 5, 9],
    ])
    chords.forEach((chord, index) => {
      for (const note of chord) {
        expect(note.start).toBe(index * BAR)
        expect(note.length).toBe(BAR)
        expect(note.velocity).toBeCloseTo(0.7)
        expect(note.pan).toBe(0)
        expect(note.role).toBe("chord")
      }
    })
    sound(result.notes)
  })

  it("moves with the key, the octave, the chord size and the bars per chord", () => {
    const c = runRiff(settings(), PLACE).notes
    const d = runRiff(settings({ progression: { root: 2 } }), PLACE).notes
    expect(d.map((note) => note.key)).toEqual(c.map((note) => note.key + 2))
    const low = runRiff(settings({ progression: { octave: 4 } }), PLACE).notes
    expect(low.map((note) => note.key)).toEqual(c.map((note) => note.key - 12))

    const sevenths = runRiff(
      settings({ progression: { chordSize: "seventh" } }),
      PLACE
    )
    expect(sevenths.counts.progression).toBe(16)
    expect(pitchClasses(chordsOf(sevenths.notes)[0])).toEqual([0, 4, 7, 11])

    const minor = runRiff(settings({ progression: { scale: "minor" } }), PLACE)
    expect(pitchClasses(chordsOf(minor.notes)[0])).toEqual([0, 3, 7])

    const halves = runRiff(
      settings({ progression: { barsPerChord: "1/2", bars: 2 } }),
      PLACE
    )
    const starts = chordsOf(halves.notes).map((chord) => chord[0].start)
    expect(starts).toEqual([0, BAR / 2, BAR, (BAR * 3) / 2])
    expect(halves.end).toBe(2 * BAR)
  })

  it("starts at the origin and follows the meter there", () => {
    const waltz = runRiff(settings(), { origin: 2880, barTicks: 2880, velocity: 0.5 })
    expect(waltz.start).toBe(2880)
    expect(waltz.end).toBe(2880 + 4 * 2880)
    const starts = chordsOf(waltz.notes).map((chord) => chord[0].start)
    expect(starts).toEqual([2880, 5760, 8640, 11520])
    expect(waltz.notes.every((note) => note.start >= 2880)).toBe(true)
  })

  it("reads typed degrees and says what is wrong with them", () => {
    expect(parseDegrees("I V vi IV").map((item) => item.degree)).toEqual([0, 4, 5, 3])
    expect(parseDegrees("1-5-6-4").map((item) => item.degree)).toEqual([0, 4, 5, 3])
    expect(parseDegrees("ii7, V7, I")).toEqual([
      { degree: 1, size: "seventh" },
      { degree: 4, size: "seventh" },
      { degree: 0 },
    ])
    expect(() => parseDegrees("")).toThrow(RiffError)
    expect(() => parseDegrees("I H IV")).toThrow(/not a scale degree/)
    expect(
      progressionDegrees({ preset: "custom", custom: "vi IV I V" }).map(
        (item) => item.degree
      )
    ).toEqual([5, 3, 0, 4])
    expect(() => progressionDegrees({ preset: "no-such", custom: "" })).toThrow(
      RiffError
    )
    const typed = runRiff(
      settings({ progression: { preset: "custom", custom: "IV I", bars: 2 } }),
      PLACE
    )
    expect(chordsOf(typed.notes).map(pitchClasses)).toEqual([
      [0, 5, 9],
      [0, 4, 7],
    ])
  })
})

describe("later stages", () => {
  it("an arpeggio replaces each block chord with steps on its grid", () => {
    const result = runRiff(
      settings({
        arpeggio: { enabled: true, pattern: "up", rate: "1/8", octaves: 1, gate: 50 },
      }),
      PLACE
    )
    // Eight eighth notes to the bar, four bars.
    expect(result.counts.arpeggio).toBe(32)
    expect(result.notes).toHaveLength(32)
    expect(result.context.grid).toBe(480)
    result.notes.forEach((note, index) => {
      expect(note.start).toBe(index * 480)
      expect(note.length).toBe(240)
    })
    // Up: the first bar climbs through C E G and starts again.
    const first = result.notes.slice(0, 6).map((note) => note.key % 12)
    expect(first).toEqual([0, 4, 7, 0, 4, 7])
    const keys = result.notes.slice(0, 3).map((note) => note.key)
    expect(keys[0]).toBeLessThan(keys[1])
    expect(keys[1]).toBeLessThan(keys[2])

    const down = runRiff(
      settings({ arpeggio: { enabled: true, pattern: "down", rate: "1/8" } }),
      PLACE
    )
    expect(down.notes.slice(0, 3).map((note) => note.key % 12)).toEqual([7, 4, 0])
    sound(result.notes)
  })

  it("a stage that is switched off changes nothing and reports no count", () => {
    const plain = runRiff(settings(), PLACE)
    const withLevels = runRiff(settings({ levels: { enabled: true, base: 40 } }), PLACE)
    expect(plain.counts.levels).toBeNull()
    expect(withLevels.counts.levels).toBe(12)
    expect(withLevels.notes.map((note) => note.key)).toEqual(
      plain.notes.map((note) => note.key)
    )
    expect(withLevels.notes.map((note) => note.start)).toEqual(
      plain.notes.map((note) => note.start)
    )
    // The draw velocity when levels are off, the stage's own when on.
    expect(plain.notes.every((note) => Math.abs(note.velocity - 0.7) < 1e-9)).toBe(true)
    expect(withLevels.notes.some((note) => Math.abs(note.velocity - 0.7) > 0.05)).toBe(
      true
    )
    sound(withLevels.notes)
  })

  it("fit keeps every note inside the chosen range", () => {
    const fitted = runRiff(
      settings({
        arpeggio: { enabled: true, octaves: 3 },
        fit: { enabled: true, low: 60, high: 72, keepInScale: true },
      }),
      PLACE
    )
    expect(fitted.notes.length).toBeGreaterThan(0)
    for (const note of fitted.notes) {
      expect(note.key).toBeGreaterThanOrEqual(60)
      expect(note.key).toBeLessThanOrEqual(72)
      // C major has no black keys.
      expect([0, 2, 4, 5, 7, 9, 11]).toContain(note.key % 12)
    }
  })
})

describe("one seed, one riff", () => {
  const random = (seed: number, patch: Patch = {}) =>
    runRiff(
      settings({
        seed,
        arpeggio: { enabled: true, pattern: "random", rate: "1/16" },
        levels: { enabled: true, random: 60 },
        ...patch,
      } as Patch),
      PLACE
    ).notes

  it("is the same every time and differs with the seed", () => {
    const first = random(11)
    expect(random(11)).toEqual(first)
    const other = random(12)
    expect(other.map((note) => [note.key, note.velocity])).not.toEqual(
      first.map((note) => [note.key, note.velocity])
    )
    sound(first)
  })

  it("gives each random stage a stream of its own", () => {
    const base = random(5)
    // Turning the levels' randomness off leaves the arpeggio's order alone.
    const steady = random(5, { levels: { enabled: true, random: 0 } })
    expect(steady.map((note) => note.key)).toEqual(base.map((note) => note.key))
    expect(steady.map((note) => note.start)).toEqual(base.map((note) => note.start))
    // Humanised timing leaves the keys and the velocities alone.
    const loose = random(5, { groove: { enabled: true, swing: 0, humanize: 40 } })
    const byStep = (notes: RiffNote[]) =>
      [...notes].sort((a, b) => a.step - b.step || a.key - b.key)
    expect(byStep(loose).map((note) => note.key)).toEqual(
      byStep(base).map((note) => note.key)
    )
    expect(byStep(loose).map((note) => note.velocity)).toEqual(
      byStep(base).map((note) => note.velocity)
    )
    expect(loose.every((note) => note.start >= PLACE.origin)).toBe(true)
  })
})

describe("limits and the command's notes", () => {
  it("refuses a riff that does not fit instead of shortening it", () => {
    const late: RiffPlace = { ...PLACE, origin: MAX_PATTERN_TICKS - BAR }
    expect(() => runRiff(settings(), late)).toThrow(RiffError)
    expect(() => runRiff(settings({ progression: { bars: 0 } }), PLACE)).toThrow(
      RiffError
    )
    expect(() => runRiff(settings({ progression: { root: 12 } }), PLACE)).toThrow(
      RiffError
    )
    expect(() => runRiff(settings({ seed: -1 }), PLACE)).toThrow(RiffError)
    expect(() => runRiff(settings(), { ...PLACE, barTicks: 0 })).toThrow(RiffError)
    expect(() => runRiff(settings(), { ...PLACE, velocity: 2 })).toThrow(RiffError)
  })

  it("hands plain notes to addNotes and expression only where it is set", () => {
    const plain = riffNoteInits(runRiff(settings(), PLACE).notes)
    expect(plain).toHaveLength(12)
    expect(plain.every((note) => note.expression === undefined)).toBe(true)
    expect(plain[0]).toMatchObject({ start: 0, length: BAR, pan: 0 })

    const shaped = riffNoteInits(
      runRiff(settings({ levels: { enabled: true, modX: 80, release: 50, modY: 50 } }), PLACE)
        .notes
    )
    expect(shaped.every((note) => note.expression !== undefined)).toBe(true)
    expect(shaped[0].expression?.modulationX).toBeCloseTo(0.8)
    expect(shaped[0].expression?.modulationY).toBeCloseTo(0.5)
    expect(shaped[0].expression?.finePitchCents).toBe(0)
  })
})
