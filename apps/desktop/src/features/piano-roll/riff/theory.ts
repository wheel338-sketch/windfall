import { inScale, scaleDefinition } from "../scales"
import {
  MAX_RIFF_CHORDS,
  RIFF_CHORD_SIZES,
  RIFF_SCALES,
  RiffError,
  type RiffChordSize,
  type RiffScaleId,
  type RiffSettings,
} from "./model"

/** A chord by scale degree, 0 for the tonic to 6 for the seventh degree. */
export type RiffDegree = { degree: number; size?: RiffChordSize }

/** Named progressions, written as degrees 1 to 7 of whichever scale is chosen. */
export const RIFF_PRESETS = [
  { id: "pop", name: "Pop", degrees: [1, 5, 6, 4] },
  { id: "fifties", name: "Fifties", degrees: [1, 6, 4, 5] },
  { id: "sixth-first", name: "Sixth first", degrees: [6, 4, 1, 5] },
  { id: "three-chord", name: "Three chord", degrees: [1, 4, 5, 4] },
  { id: "lift", name: "Lift", degrees: [1, 4, 6, 5] },
  { id: "folk", name: "Folk", degrees: [1, 4, 1, 5] },
  { id: "climb", name: "Climb", degrees: [1, 3, 4, 5] },
  { id: "two-five-one", name: "Two five one", degrees: [2, 5, 1] },
  { id: "circle", name: "Circle", degrees: [6, 2, 5, 1] },
  { id: "canon", name: "Canon", degrees: [1, 5, 6, 3, 4, 1, 4, 5] },
  { id: "epic", name: "Epic", degrees: [1, 6, 3, 7] },
  { id: "descent", name: "Descent", degrees: [1, 7, 6, 5] },
  { id: "cadence", name: "Cadence", degrees: [1, 4, 5, 1] },
  {
    id: "twelve-bar",
    name: "Twelve bar",
    degrees: [1, 1, 1, 1, 4, 4, 1, 1, 5, 4, 1, 5],
  },
] as const

export const CUSTOM_PRESET = "custom"

const NUMERALS = ["I", "II", "III", "IV", "V", "VI", "VII"] as const
const ROMAN = /^(i{1,3}|iv|vi{0,2})°?(7|9)?$/i
const PITCH_NAMES = [
  "C",
  "C♯",
  "D",
  "D♯",
  "E",
  "F",
  "F♯",
  "G",
  "G♯",
  "A",
  "A♯",
  "B",
] as const

function scaleOption(id: RiffScaleId) {
  return RIFF_SCALES.find((scale) => scale.value === id)!
}

/** Semitones above the tonic of the scale's own notes. */
export function scaleIntervals(id: RiffScaleId): readonly number[] {
  return scaleDefinition(id).intervals
}

/** Semitones above the tonic of the seven notes chords are stacked from. */
export function harmonyIntervals(id: RiffScaleId): readonly number[] {
  return scaleDefinition(scaleOption(id).harmony).intervals
}

/**
 * Reads "I V vi IV", "1-5-6-4" or "ii7, V7, I". Case is not significant: a
 * chord's quality comes from the scale. A roman numeral may end in 7 or 9 to
 * set that chord's size.
 */
export function parseDegrees(text: string): RiffDegree[] {
  const tokens = text.split(/[\s,;|>\-–—]+/).filter(Boolean)
  if (tokens.length === 0)
    throw new RiffError(
      "progression",
      "Type at least one scale degree, such as I V vi IV."
    )
  if (tokens.length > MAX_RIFF_CHORDS)
    throw new RiffError(
      "progression",
      `Use at most ${MAX_RIFF_CHORDS} chords in a progression.`
    )
  return tokens.map((token) => {
    if (/^[1-7]$/.test(token)) return { degree: Number(token) - 1 }
    const roman = ROMAN.exec(token)
    if (!roman)
      throw new RiffError(
        "progression",
        `“${token}” is not a scale degree. Use I to VII or 1 to 7.`
      )
    const degree = NUMERALS.findIndex(
      (numeral) => numeral === roman[1].toUpperCase()
    )
    if (roman[2] === "7") return { degree, size: "seventh" }
    if (roman[2] === "9") return { degree, size: "ninth" }
    return { degree }
  })
}

/** The degrees a progression setting asks for, before any variation. */
export function progressionDegrees(
  progression: Pick<RiffSettings["progression"], "preset" | "custom">
): RiffDegree[] {
  if (progression.preset === CUSTOM_PRESET)
    return parseDegrees(progression.custom)
  const preset = RIFF_PRESETS.find((item) => item.id === progression.preset)
  if (!preset)
    throw new RiffError("progression", "Choose a chord progression.")
  return preset.degrees.map((degree) => ({ degree: degree - 1 }))
}

/**
 * The nearest key of the scale inside the inclusive bounds, the lower one
 * when two are as near. A key with no scale note within an octave is kept.
 */
export function nearestInScale(
  key: number,
  scale: { root: number; id: RiffScaleId },
  low = -Infinity,
  high = Infinity
): number {
  for (let distance = 0; distance < 12; distance++) {
    for (const candidate of [key - distance, key + distance]) {
      if (candidate >= low && candidate <= high && inScale(candidate, scale))
        return candidate
    }
  }
  return key
}

function harmonyStep(intervals: readonly number[], step: number): number {
  return intervals[step % 7] + 12 * Math.floor(step / 7)
}

/**
 * The keys of one chord in root position, lowest first: thirds stacked on the
 * degree. Tones outside a pentatonic scale move to its nearest note, the
 * lower one when two are as near, and tones that land together merge.
 */
export function chordKeys(
  chord: RiffDegree,
  progression: Pick<
    RiffSettings["progression"],
    "root" | "scale" | "octave" | "chordSize"
  >
): number[] {
  const harmony = harmonyIntervals(progression.scale)
  const size = RIFF_CHORD_SIZES.find(
    (item) => item.value === (chord.size ?? progression.chordSize)
  )!
  const tonic = 12 * progression.octave + progression.root
  const keys = size.steps.map(
    (step) => tonic + harmonyStep(harmony, chord.degree + step)
  )
  if (scaleOption(progression.scale).harmony === progression.scale) return keys
  const scale = { root: progression.root, id: progression.scale }
  const snapped = keys.map((key) => nearestInScale(key, scale))
  return snapped.filter((key, index) => snapped.indexOf(key) === index)
}

/** "vi" or "V7": lower case for a minor third, ° for a diminished fifth. */
export function degreeNumeral(chord: RiffDegree, scale: RiffScaleId): string {
  const harmony = harmonyIntervals(scale)
  const root = harmonyStep(harmony, chord.degree)
  const third = harmonyStep(harmony, chord.degree + 2) - root
  const fifth = harmonyStep(harmony, chord.degree + 4) - root
  const numeral = NUMERALS[chord.degree]
  const suffix =
    chord.size === "seventh" ? "7" : chord.size === "ninth" ? "9" : ""
  return (
    (third === 3 ? numeral.toLowerCase() : numeral) +
    (third === 3 && fifth === 6 ? "°" : third === 4 && fifth === 8 ? "+" : "") +
    suffix
  )
}

/** A short name for a root-position chord, such as "Am" or "G7". */
export function chordName(keys: readonly number[]): string {
  if (keys.length === 0) return ""
  const above = new Set(keys.map((key) => (((key - keys[0]) % 12) + 12) % 12))
  const quality =
    above.has(4) && above.has(8)
      ? "aug"
      : above.has(4)
        ? ""
        : above.has(3) && above.has(6)
          ? "dim"
          : above.has(3)
            ? "m"
            : above.has(5)
              ? "sus4"
              : above.has(2)
                ? "sus2"
                : ""
  const seventh = above.has(11) ? "maj7" : above.has(10) ? "7" : ""
  return PITCH_NAMES[((keys[0] % 12) + 12) % 12] + quality + seventh
}
