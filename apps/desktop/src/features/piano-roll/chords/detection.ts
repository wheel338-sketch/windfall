import { translatedText as t, message } from "@/lib/i18n"
export const PITCH_NAMES = [
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

export const TRIADS = [
  { quality: "major", get label() { return t("Major") }, intervals: [0, 4, 7] },
  { quality: "minor", get label() { return t("Minor") }, intervals: [0, 3, 7] },
  { quality: "diminished", get label() { return t("Diminished") }, intervals: [0, 3, 6] },
  { quality: "augmented", get label() { return t("Augmented") }, intervals: [0, 4, 8] },
  { quality: "sus2", get label() { return t("Suspended second") }, intervals: [0, 2, 7] },
  { quality: "sus4", get label() { return t("Suspended fourth") }, intervals: [0, 5, 7] },
] as const

export type TriadQuality = (typeof TRIADS)[number]["quality"]

type ChordTemplate = {
  /** Stable id, also reported as `ChordDetection.chord`. */
  id: string
  /** What follows the root in the compact symbol. */
  suffix: string
  /** What follows the root in the long name. */
  name: string
  /** Semitones above the root in chord-tone order, the root first. */
  intervals: readonly number[]
  /** Scale-degree names, aligned with `intervals`. */
  degrees: readonly string[]
  /** Tones a voicing may leave out. Never the root or the naming tone. */
  optional?: readonly number[]
  /** Altered fifths and ninths, which rank a reading behind plainer ones. */
  alterations?: number
}

const TRIAD_FORMS: Record<
  TriadQuality,
  { suffix: string; degrees: readonly string[] }
> = {
  major: { suffix: "", degrees: ["1", "3", "5"] },
  minor: { suffix: "m", degrees: ["1", "♭3", "5"] },
  diminished: { suffix: "dim", degrees: ["1", "♭3", "♭5"] },
  augmented: { suffix: "aug", degrees: ["1", "3", "♯5"] },
  sus2: { suffix: "sus2", degrees: ["1", "2", "5"] },
  sus4: { suffix: "sus4", degrees: ["1", "4", "5"] },
}

const FIFTH = [7] as const

/**
 * Every chord the detector names, after the six triads. A seventh or larger
 * chord with a third may leave out its perfect fifth; elevenths may also
 * leave out the ninth, and thirteenths the ninth and the eleventh. The order
 * is the last tie-break between two readings on one root.
 */
export const CHORDS: readonly ChordTemplate[] = [
  ...TRIADS.map((triad) => ({
    id: triad.quality,
    name: triad.label.toLowerCase(),
    intervals: triad.intervals,
    ...TRIAD_FORMS[triad.quality],
  })),
  {
    id: "6",
    suffix: "6",
    name: "major sixth",
    intervals: [0, 4, 7, 9],
    degrees: ["1", "3", "5", "6"],
  },
  {
    id: "m6",
    suffix: "m6",
    name: "minor sixth",
    intervals: [0, 3, 7, 9],
    degrees: ["1", "♭3", "5", "6"],
  },
  {
    id: "69",
    suffix: "69",
    name: "six-nine",
    intervals: [0, 4, 7, 9, 2],
    degrees: ["1", "3", "5", "6", "9"],
  },
  {
    id: "maj7",
    suffix: "maj7",
    name: "major seventh",
    intervals: [0, 4, 7, 11],
    degrees: ["1", "3", "5", "7"],
    optional: FIFTH,
  },
  {
    id: "7",
    suffix: "7",
    name: "dominant seventh",
    intervals: [0, 4, 7, 10],
    degrees: ["1", "3", "5", "♭7"],
    optional: FIFTH,
  },
  {
    id: "m7",
    suffix: "m7",
    name: "minor seventh",
    intervals: [0, 3, 7, 10],
    degrees: ["1", "♭3", "5", "♭7"],
    optional: FIFTH,
  },
  {
    id: "m(maj7)",
    suffix: "m(maj7)",
    name: "minor major seventh",
    intervals: [0, 3, 7, 11],
    degrees: ["1", "♭3", "5", "7"],
    optional: FIFTH,
  },
  {
    id: "dim7",
    suffix: "dim7",
    name: "diminished seventh",
    intervals: [0, 3, 6, 9],
    degrees: ["1", "♭3", "♭5", "♭♭7"],
  },
  {
    id: "m7♭5",
    suffix: "m7♭5",
    name: "half-diminished seventh",
    intervals: [0, 3, 6, 10],
    degrees: ["1", "♭3", "♭5", "♭7"],
  },
  {
    // The same four tones as a dominant seventh with a sharp fifth (7♯5).
    id: "aug7",
    suffix: "aug7",
    name: "augmented seventh",
    intervals: [0, 4, 8, 10],
    degrees: ["1", "3", "♯5", "♭7"],
    alterations: 1,
  },
  {
    id: "maj7♯5",
    suffix: "maj7♯5",
    name: "augmented major seventh",
    intervals: [0, 4, 8, 11],
    degrees: ["1", "3", "♯5", "7"],
    alterations: 1,
  },
  {
    id: "7sus4",
    suffix: "7sus4",
    name: "dominant seventh suspended fourth",
    intervals: [0, 5, 7, 10],
    degrees: ["1", "4", "5", "♭7"],
  },
  {
    id: "7♭5",
    suffix: "7♭5",
    name: "dominant seventh flat five",
    intervals: [0, 4, 6, 10],
    degrees: ["1", "3", "♭5", "♭7"],
    alterations: 1,
  },
  {
    id: "add9",
    suffix: "add9",
    name: "added ninth",
    intervals: [0, 4, 7, 2],
    degrees: ["1", "3", "5", "9"],
  },
  {
    id: "m(add9)",
    suffix: "m(add9)",
    name: "minor added ninth",
    intervals: [0, 3, 7, 2],
    degrees: ["1", "♭3", "5", "9"],
  },
  {
    id: "add11",
    suffix: "add11",
    name: "added eleventh",
    intervals: [0, 4, 7, 5],
    degrees: ["1", "3", "5", "11"],
  },
  {
    id: "m(add11)",
    suffix: "m(add11)",
    name: "minor added eleventh",
    intervals: [0, 3, 7, 5],
    degrees: ["1", "♭3", "5", "11"],
  },
  {
    id: "maj9",
    suffix: "maj9",
    name: "major ninth",
    intervals: [0, 4, 7, 11, 2],
    degrees: ["1", "3", "5", "7", "9"],
    optional: FIFTH,
  },
  {
    id: "9",
    suffix: "9",
    name: "dominant ninth",
    intervals: [0, 4, 7, 10, 2],
    degrees: ["1", "3", "5", "♭7", "9"],
    optional: FIFTH,
  },
  {
    id: "m9",
    suffix: "m9",
    name: "minor ninth",
    intervals: [0, 3, 7, 10, 2],
    degrees: ["1", "♭3", "5", "♭7", "9"],
    optional: FIFTH,
  },
  {
    id: "7♭9",
    suffix: "7♭9",
    name: "dominant seventh flat nine",
    intervals: [0, 4, 7, 10, 1],
    degrees: ["1", "3", "5", "♭7", "♭9"],
    optional: FIFTH,
    alterations: 1,
  },
  {
    id: "7♯9",
    suffix: "7♯9",
    name: "dominant seventh sharp nine",
    intervals: [0, 4, 7, 10, 3],
    degrees: ["1", "3", "5", "♭7", "♯9"],
    optional: FIFTH,
    alterations: 1,
  },
  {
    id: "11",
    suffix: "11",
    name: "dominant eleventh",
    intervals: [0, 4, 7, 10, 2, 5],
    degrees: ["1", "3", "5", "♭7", "9", "11"],
    optional: [7, 2],
  },
  {
    id: "m11",
    suffix: "m11",
    name: "minor eleventh",
    intervals: [0, 3, 7, 10, 2, 5],
    degrees: ["1", "♭3", "5", "♭7", "9", "11"],
    optional: [7, 2],
  },
  {
    id: "13",
    suffix: "13",
    name: "dominant thirteenth",
    intervals: [0, 4, 7, 10, 2, 5, 9],
    degrees: ["1", "3", "5", "♭7", "9", "11", "13"],
    optional: [7, 2, 5],
  },
]

/** Semitones above the lower note, for a pair of pitch classes. */
export const INTERVAL_NAMES = [
  "unison",
  "minor second",
  "major second",
  "minor third",
  "major third",
  "perfect fourth",
  "tritone",
  "perfect fifth",
  "minor sixth",
  "major sixth",
  "minor seventh",
  "major seventh",
] as const

// How a tone above the bass is named when no chord gives it a function.
const PLAIN_DEGREES = [
  "1",
  "♭2",
  "2",
  "♭3",
  "3",
  "4",
  "♭5",
  "5",
  "♭6",
  "6",
  "♭7",
  "7",
] as const

export type ChordDetection = {
  kind: "empty" | "note" | "interval" | "triad" | "chord" | "unknown"
  /** The long name, such as "C major seventh / E". */
  label: string
  /** The compact symbol, such as "Cmaj7/E". Empty without notes. */
  symbol: string
  /** Pitch class of the root; the lower note of a pair. Null when unknown. */
  root: number | null
  /** Pitch class of the lowest key. */
  bass: number | null
  /** One of the six triads, for the chord tools. Null for anything else. */
  quality: TriadQuality | null
  /** Id of the recognised quality: a triad's, "maj7", "m7♭5", "5" and so on. */
  chord: string | null
  /** The distinct pitch classes, ascending from C. */
  pitchClasses: number[]
  /**
   * Semitones above the root in chord-tone order; above the bass, ascending,
   * when there is no root.
   */
  intervals: number[]
  /** Scale-degree names, aligned with `intervals`. */
  degrees: string[]
  /** Tones of the recognised chord that are not sounding, above the root. */
  omitted: number[]
}

export function pitchClass(key: number): number {
  return ((key % 12) + 12) % 12
}

const mask = (intervals: readonly number[]) =>
  intervals.reduce((bits, interval) => bits | (1 << interval), 0)

const TEMPLATE_MASKS = CHORDS.map((template) => {
  const full = mask(template.intervals)
  return { full, required: full & ~mask(template.optional ?? []) }
})

function bitCount(bits: number): number {
  let count = 0
  for (; bits; bits &= bits - 1) count += 1
  return count
}

/**
 * Names the chord of a set of MIDI keys from its pitch classes and its lowest
 * key. Octave doublings, voicing above the bass and input order do not
 * matter. Sharps name every black key.
 *
 * One pitch class is a note and two are an interval measured up from the
 * bass, a perfect fifth being a power chord. Three or more are matched
 * against `CHORDS` on every sounding root. A reading is a root whose tones
 * all belong to a template and include all it requires. Among several
 * readings the first of these decides:
 *
 * 1. the root is the bass;
 * 2. fewest omitted and altered tones together;
 * 3. the lowest root pitch class, counting up from C;
 * 4. the order of `CHORDS`.
 *
 * So C–E–G–A is C major sixth over C, E or G and A minor seventh over A. A
 * bass other than the root is written as a slash chord.
 */
export function detectChord(keys: readonly number[]): ChordDetection {
  let lowest = Infinity
  let present = 0
  for (const key of keys) {
    if (!Number.isInteger(key)) continue
    lowest = Math.min(lowest, key)
    present |= 1 << pitchClass(key)
  }
  const pitchClasses: number[] = []
  for (let pc = 0; pc < 12; pc++) if (present & (1 << pc)) pitchClasses.push(pc)

  if (!pitchClasses.length) {
    return {
      kind: "empty",
      label: t("No notes selected"),
      symbol: "",
      root: null,
      bass: null,
      quality: null,
      chord: null,
      pitchClasses,
      intervals: [],
      degrees: [],
      omitted: [],
    }
  }
  const bass = pitchClass(lowest)
  const bassName = PITCH_NAMES[bass]

  if (pitchClasses.length === 1) {
    return {
      kind: "note",
      label: bassName,
      symbol: bassName,
      root: bass,
      bass,
      quality: null,
      chord: null,
      pitchClasses,
      intervals: [0],
      degrees: ["1"],
      omitted: [],
    }
  }

  if (pitchClasses.length === 2) {
    const other = pitchClasses[0] === bass ? pitchClasses[1] : pitchClasses[0]
    const interval = pitchClass(other - bass)
    const power = interval === 7
    return {
      kind: "interval",
      label: power
        ? message("{bassName} power chord", { bassName })
        : message("{low}–{high} {interval}", { low: bassName, high: PITCH_NAMES[other], interval: t(INTERVAL_NAMES[interval]) }),
      symbol: power ? `${bassName}5` : `${bassName}–${PITCH_NAMES[other]}`,
      root: bass,
      bass,
      quality: null,
      chord: power ? "5" : null,
      pitchClasses,
      intervals: [0, interval],
      degrees: ["1", PLAIN_DEGREES[interval]],
      omitted: [],
    }
  }

  let best: {
    root: number
    template: number
    bits: number
    inverted: number
    penalty: number
  } | null = null
  for (const root of pitchClasses) {
    // The sounding pitch classes as intervals above this root.
    const bits = ((present >> root) | (present << (12 - root))) & 0xfff
    for (let template = 0; template < CHORDS.length; template++) {
      const { full, required } = TEMPLATE_MASKS[template]
      if ((bits & ~full) !== 0 || (required & ~bits) !== 0) continue
      const inverted = root === bass ? 0 : 1
      const penalty =
        bitCount(full & ~bits) + (CHORDS[template].alterations ?? 0)
      // Roots ascend and templates keep their order, so only a strictly
      // better reading replaces an earlier one.
      if (
        best === null ||
        inverted < best.inverted ||
        (inverted === best.inverted && penalty < best.penalty)
      ) {
        best = { root, template, bits, inverted, penalty }
      }
    }
  }

  if (best === null) {
    const intervals = pitchClasses
      .map((pc) => pitchClass(pc - bass))
      .sort((a, b) => a - b)
    return {
      kind: "unknown",
      label: message("Unknown chord ({map})", { map: pitchClasses.map((pc) => PITCH_NAMES[pc]).join(", ") }),
      symbol: "?",
      root: null,
      bass,
      quality: null,
      chord: null,
      pitchClasses,
      intervals,
      degrees: intervals.map((interval) => PLAIN_DEGREES[interval]),
      omitted: [],
    }
  }

  const template = CHORDS[best.template]
  const { root, bits } = best
  const rootName = PITCH_NAMES[root]
  const triad = TRIADS.find((item) => item.quality === template.id)
  const sounding = template.intervals
    .map((interval, index) => ({ interval, degree: template.degrees[index] }))
    .filter(({ interval }) => bits & (1 << interval))
  return {
    kind: triad ? "triad" : "chord",
    label: root === bass
      ? message("{root} {quality}", { root: rootName, quality: t(template.name) })
      : message("{root} {quality} / {bass}", { root: rootName, quality: t(template.name), bass: bassName }),
    symbol: `${rootName}${template.suffix}${root === bass ? "" : `/${bassName}`}`,
    root,
    bass,
    quality: triad?.quality ?? null,
    chord: template.id,
    pitchClasses,
    intervals: sounding.map(({ interval }) => interval),
    degrees: sounding.map(({ degree }) => degree),
    omitted: template.intervals.filter((interval) => !(bits & (1 << interval))),
  }
}
