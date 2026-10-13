import type { ScaleId } from "../scales"
import { MAX_RIFF_SEED } from "./rng"
import { translatedText as t } from "@/lib/i18n"

/** Stages in the order they run. Each one reads the previous one's notes. */
export const RIFF_STAGES = [
  { id: "progression", get label() { return t("Progression") }, optional: false },
  { id: "chords", get label() { return t("Chords") }, optional: true },
  { id: "arpeggio", get label() { return t("Arpeggio") }, optional: true },
  { id: "mirror", get label() { return t("Mirror") }, optional: true },
  { id: "levels", get label() { return t("Levels") }, optional: true },
  { id: "articulation", get label() { return t("Length") }, optional: true },
  { id: "groove", get label() { return t("Groove") }, optional: true },
  { id: "fit", get label() { return t("Fit") }, optional: true },
] as const
export type RiffStageId = (typeof RIFF_STAGES)[number]["id"]

/** Most notes one riff may add. More is refused, never cut short. */
export const MAX_RIFF_NOTES = 1024
export const MAX_RIFF_CHORDS = 32

/**
 * `harmony` is the seven-note scale that degrees and stacked thirds are read
 * from. A pentatonic scale borrows its parent's and then pulls every chord
 * tone onto the nearest of its own five notes.
 */
export const RIFF_SCALES = [
  { value: "major", get label() { return t("Major") }, harmony: "major" },
  { value: "minor", get label() { return t("Natural minor") }, harmony: "minor" },
  {
    value: "harmonic-minor",
    get label() { return t("Harmonic minor") },
    harmony: "harmonic-minor",
  },
  { value: "melodic-minor", get label() { return t("Melodic minor") }, harmony: "melodic-minor" },
  { value: "dorian", get label() { return t("Dorian") }, harmony: "dorian" },
  { value: "phrygian", get label() { return t("Phrygian") }, harmony: "phrygian" },
  { value: "lydian", get label() { return t("Lydian") }, harmony: "lydian" },
  { value: "mixolydian", get label() { return t("Mixolydian") }, harmony: "mixolydian" },
  { value: "major-pentatonic", get label() { return t("Pentatonic major") }, harmony: "major" },
  { value: "minor-pentatonic", get label() { return t("Pentatonic minor") }, harmony: "minor" },
] as const satisfies readonly {
  value: ScaleId
  label: string
  harmony: ScaleId
}[]
export type RiffScaleId = (typeof RIFF_SCALES)[number]["value"]

export const RIFF_CHORD_SIZES = [
  { value: "triad", get label() { return t("Triad") }, steps: [0, 2, 4] },
  { value: "seventh", get label() { return t("Seventh") }, steps: [0, 2, 4, 6] },
  { value: "ninth", get label() { return t("Ninth") }, steps: [0, 2, 4, 6, 8] },
] as const
export type RiffChordSize = (typeof RIFF_CHORD_SIZES)[number]["value"]

export const RIFF_BARS_PER_CHORD = [
  { value: "1/4", get label() { return t("¼ bar") }, bars: 0.25 },
  { value: "1/2", get label() { return t("½ bar") }, bars: 0.5 },
  { value: "1", get label() { return t("1 bar") }, bars: 1 },
  { value: "2", get label() { return t("2 bars") }, bars: 2 },
] as const
export type RiffBarsPerChord = (typeof RIFF_BARS_PER_CHORD)[number]["value"]

export const RIFF_INVERSIONS = [
  { value: "root", get label() { return t("Root") } },
  { value: "first", label: "1st" },
  { value: "second", get label() { return t("2nd") } },
  { value: "auto", get label() { return t("Smooth") } },
] as const
export type RiffInversion = (typeof RIFF_INVERSIONS)[number]["value"]

export const RIFF_SPREADS = [
  { value: "close", get label() { return t("Close") } },
  { value: "open", get label() { return t("Open") } },
  { value: "drop2", get label() { return t("Drop 2") } },
] as const
export type RiffSpread = (typeof RIFF_SPREADS)[number]["value"]

export const RIFF_BASS = [
  { value: "none", get label() { return t("None") }, semitones: 0 },
  { value: "octave", get label() { return t("1 octave below") }, semitones: 12 },
  { value: "two-octaves", get label() { return t("2 octaves below") }, semitones: 24 },
] as const
export type RiffBass = (typeof RIFF_BASS)[number]["value"]

export const RIFF_ARP_PATTERNS = [
  { value: "up", get label() { return t("Up") } },
  { value: "down", get label() { return t("Down") } },
  { value: "up-down", get label() { return t("Up-down") } },
  { value: "down-up", get label() { return t("Down-up") } },
  { value: "random", get label() { return t("Random") } },
  { value: "played", get label() { return t("As played") } },
] as const
export type RiffArpPattern = (typeof RIFF_ARP_PATTERNS)[number]["value"]

/** Note values in ticks at 960 per quarter note, whatever the meter. */
export const RIFF_ARP_RATES = [
  { value: "1/4", label: "1/4", ticks: 960 },
  { value: "1/8", label: "1/8", ticks: 480 },
  { value: "1/8T", label: "1/8T", ticks: 320 },
  { value: "1/16", label: "1/16", ticks: 240 },
  { value: "1/16T", label: "1/16T", ticks: 160 },
  { value: "1/32", label: "1/32", ticks: 120 },
] as const
export type RiffArpRate = (typeof RIFF_ARP_RATES)[number]["value"]

export const RIFF_INVERTS = [
  { value: "off", get label() { return t("Off") } },
  { value: "chord", get label() { return t("Around each chord") } },
  { value: "key", get label() { return t("Around a key") } },
] as const
export type RiffInvert = (typeof RIFF_INVERTS)[number]["value"]

export const RIFF_PAN_MODES = [
  { value: "centre", get label() { return t("Centre") } },
  { value: "alternate", get label() { return t("Alternate") } },
  { value: "random", get label() { return t("Random") } },
] as const
export type RiffPanMode = (typeof RIFF_PAN_MODES)[number]["value"]

export const RIFF_LENGTH_MODES = [
  { value: "scale", get label() { return t("Multiply") } },
  { value: "legato", get label() { return t("Legato") } },
  { value: "staccato", get label() { return t("Staccato") } },
] as const
export type RiffLengthMode = (typeof RIFF_LENGTH_MODES)[number]["value"]

export const RIFF_PLACEMENTS = [
  { value: "after", get label() { return t("After the last note") } },
  { value: "start", get label() { return t("At the pattern start") } },
  { value: "selection", get label() { return t("Replace the selection") } },
] as const
export type RiffPlacement = (typeof RIFF_PLACEMENTS)[number]["value"]

/** Percentages are whole numbers of percent, the way their fields show them. */
export type RiffSettings = {
  /** 0 to 4294967295. Every random stage draws from its own stream of it. */
  seed: number
  placement: RiffPlacement
  /** Grow the pattern to the bar that holds the riff's last note. */
  extendPattern: boolean
  progression: {
    /** Pitch class of the key, 0 (C) to 11 (B). */
    root: number
    scale: RiffScaleId
    /** A preset id, or "custom" to read `custom`. */
    preset: string
    custom: string
    chordSize: RiffChordSize
    barsPerChord: RiffBarsPerChord
    /** 1 to 16. */
    bars: number
    /** 1 to 8. Octave 5 puts the tonic between MIDI 60 and 71. */
    octave: number
    /** 0 to 100: the chance that a chord after the first is swapped. */
    variation: number
  }
  chords: {
    enabled: boolean
    inversion: RiffInversion
    spread: RiffSpread
    bass: RiffBass
    /** -480 to 480 ticks between chord notes. Negative strums downward. */
    strum: number
  }
  arpeggio: {
    enabled: boolean
    pattern: RiffArpPattern
    rate: RiffArpRate
    /** 1 to 3. */
    octaves: number
    /** 1 to 100 percent of the rate. */
    gate: number
    /** Start the pattern and its grid again at every chord. */
    restart: boolean
  }
  mirror: {
    enabled: boolean
    invert: RiffInvert
    /** MIDI key the "key" inversion turns around. */
    axis: number
    keepInScale: boolean
    reverse: boolean
  }
  levels: {
    enabled: boolean
    /** 0 to 100. */
    base: number
    /** 0 is off, otherwise every 1 to 16 steps. */
    accentEvery: number
    /** -100 to 100 percentage points. */
    accentAmount: number
    /** 0 to 100 percentage points either way. */
    random: number
    /** -100 to 100 percentage points from the first tick to the last. */
    ramp: number
    panMode: RiffPanMode
    /** 0 to 100. */
    panAmount: number
    /** 0 to 100, 50 is neutral. */
    release: number
    modX: number
    modY: number
  }
  articulation: {
    enabled: boolean
    mode: RiffLengthMode
    /** 10 to 400 percent, for the "scale" mode. */
    multiplier: number
    tie: boolean
    /** 1 to 960 ticks. */
    minLength: number
  }
  groove: {
    enabled: boolean
    /** 0 to 100. */
    swing: number
    /** 0 to 120 ticks either way. */
    humanize: number
  }
  fit: {
    enabled: boolean
    low: number
    high: number
    keepInScale: boolean
    dropDuplicates: boolean
  }
}

export const DEFAULT_RIFF_SETTINGS: RiffSettings = {
  seed: 0,
  placement: "after",
  extendPattern: true,
  progression: {
    root: 0,
    scale: "major",
    preset: "pop",
    custom: "",
    chordSize: "triad",
    barsPerChord: "1",
    bars: 4,
    octave: 5,
    variation: 0,
  },
  chords: {
    enabled: true,
    inversion: "auto",
    spread: "close",
    bass: "none",
    strum: 0,
  },
  arpeggio: {
    enabled: true,
    pattern: "up",
    rate: "1/8",
    octaves: 1,
    gate: 80,
    restart: true,
  },
  mirror: {
    enabled: false,
    invert: "off",
    axis: 60,
    keepInScale: true,
    reverse: false,
  },
  levels: {
    enabled: true,
    base: 80,
    accentEvery: 4,
    accentAmount: 15,
    random: 0,
    ramp: 0,
    panMode: "centre",
    panAmount: 30,
    release: 50,
    modX: 50,
    modY: 50,
  },
  articulation: {
    enabled: false,
    mode: "scale",
    multiplier: 100,
    tie: false,
    minLength: 1,
  },
  groove: { enabled: false, swing: 0, humanize: 0 },
  fit: {
    enabled: true,
    low: 36,
    high: 96,
    keepInScale: true,
    dropDuplicates: true,
  },
}

/** The chord a note belongs to. Notes of one chord share one span. */
export type RiffSpan = {
  index: number
  start: number
  /** Exclusive. */
  end: number
  /** MIDI key of the chord's root before any voicing. */
  root: number
}

export type RiffNote = {
  start: number
  length: number
  key: number
  /** 0 to 1. */
  velocity: number
  /** -1 to 1. */
  pan: number
  /** 0 to 1 with 0.5 neutral, as in a note's expression. */
  release: number
  modX: number
  modY: number
  /** A bass note is held under its chord and is never arpeggiated. */
  role: "chord" | "bass"
  /** Position in the sequence: the chord index, or the arpeggio step. */
  step: number
  span: RiffSpan
}

export type RiffContext = {
  /** Tick the riff starts at. Nothing is placed before it. */
  origin: number
  /** Bar length at `origin`. */
  barTicks: number
  /** Velocity notes start with, 0 to 1. */
  velocity: number
  scale: { root: number; id: RiffScaleId }
  /** The arpeggio's step in ticks, or 0 when chords are left as blocks. */
  grid: number
}

/** A setting the user can correct. Its message is shown as written. */
export class RiffError extends Error {
  readonly stage: RiffStageId
  constructor(stage: RiffStageId, message: string) {
    super(message)
    this.name = "RiffError"
    this.stage = stage
  }
}

function whole(value: number, low: number, high: number): boolean {
  return Number.isInteger(value) && value >= low && value <= high
}

function oneOf<T extends string>(
  options: readonly { value: T }[],
  value: unknown
): value is T {
  return options.some((option) => option.value === value)
}

/** Ranges only. The progression's degrees are checked where they are read. */
export function validateRiffSettings(settings: RiffSettings): void {
  const fail = (stage: RiffStageId, message: string): never => {
    throw new RiffError(stage, message)
  }
  const {
    progression,
    chords,
    arpeggio,
    mirror,
    levels,
    articulation,
    groove,
    fit,
  } = settings
  if (!whole(settings.seed, 0, MAX_RIFF_SEED))
    fail("progression", "Use a whole seed from 0 to 4294967295.")
  if (!oneOf(RIFF_PLACEMENTS, settings.placement))
    fail("progression", "Choose where the riff goes.")

  if (!whole(progression.root, 0, 11) || !oneOf(RIFF_SCALES, progression.scale))
    fail("progression", "Choose a key and a scale.")
  if (
    !oneOf(RIFF_CHORD_SIZES, progression.chordSize) ||
    !oneOf(RIFF_BARS_PER_CHORD, progression.barsPerChord)
  )
    fail("progression", "Choose a chord size and how long each chord lasts.")
  if (!whole(progression.bars, 1, 16))
    fail("progression", "Use a length of 1 to 16 whole bars.")
  if (!whole(progression.octave, 1, 8))
    fail("progression", "Use an octave from 1 to 8.")
  if (!whole(progression.variation, 0, 100))
    fail("progression", "Use a variation from 0 to 100%.")

  if (chords.enabled) {
    if (
      !oneOf(RIFF_INVERSIONS, chords.inversion) ||
      !oneOf(RIFF_SPREADS, chords.spread) ||
      !oneOf(RIFF_BASS, chords.bass)
    )
      fail("chords", "Choose an inversion, a spread and a bass note.")
    if (!whole(chords.strum, -480, 480))
      fail("chords", "Use a strum of -480 to 480 whole ticks.")
  }

  if (arpeggio.enabled) {
    if (
      !oneOf(RIFF_ARP_PATTERNS, arpeggio.pattern) ||
      !oneOf(RIFF_ARP_RATES, arpeggio.rate)
    )
      fail("arpeggio", "Choose an arpeggio pattern and rate.")
    if (!whole(arpeggio.octaves, 1, 3))
      fail("arpeggio", "Use an octave range of 1 to 3.")
    if (!whole(arpeggio.gate, 1, 100))
      fail("arpeggio", "Use a gate of 1 to 100%.")
  }

  if (mirror.enabled) {
    if (!oneOf(RIFF_INVERTS, mirror.invert))
      fail("mirror", "Choose how pitches are mirrored.")
    if (mirror.invert === "key" && !whole(mirror.axis, 0, 127))
      fail("mirror", "Use a mirror key from MIDI 0 to 127.")
  }

  if (levels.enabled) {
    if (!whole(levels.base, 0, 100))
      fail("levels", "Use a base velocity of 0 to 100%.")
    if (!whole(levels.accentEvery, 0, 16))
      fail("levels", "Accent every 1 to 16 steps, or 0 for no accent.")
    if (!whole(levels.accentAmount, -100, 100))
      fail("levels", "Use an accent of -100 to 100 percentage points.")
    if (!whole(levels.random, 0, 100))
      fail("levels", "Use a random velocity amount of 0 to 100.")
    if (!whole(levels.ramp, -100, 100))
      fail("levels", "Use a ramp of -100 to 100 percentage points.")
    if (!oneOf(RIFF_PAN_MODES, levels.panMode))
      fail("levels", "Choose a pan mode.")
    if (!whole(levels.panAmount, 0, 100))
      fail("levels", "Use a pan amount of 0 to 100%.")
    if (
      !whole(levels.release, 0, 100) ||
      !whole(levels.modX, 0, 100) ||
      !whole(levels.modY, 0, 100)
    )
      fail("levels", "Use 0 to 100% for release and both modulation values.")
  }

  if (articulation.enabled) {
    if (!oneOf(RIFF_LENGTH_MODES, articulation.mode))
      fail("articulation", "Choose how note lengths change.")
    if (
      articulation.mode === "scale" &&
      !whole(articulation.multiplier, 10, 400)
    )
      fail("articulation", "Use a length multiplier of 10 to 400%.")
    if (!whole(articulation.minLength, 1, 960))
      fail("articulation", "Use a minimum length of 1 to 960 ticks.")
  }

  if (groove.enabled) {
    if (!whole(groove.swing, 0, 100))
      fail("groove", "Use a swing of 0 to 100%.")
    if (!whole(groove.humanize, 0, 120))
      fail("groove", "Use a timing variation of 0 to 120 ticks.")
  }

  if (fit.enabled) {
    if (!whole(fit.low, 0, 127) || !whole(fit.high, 0, 127))
      fail("fit", "Use lowest and highest keys from MIDI 0 to 127.")
    if (fit.high - fit.low < 11)
      fail(
        "fit",
        "Leave at least an octave between the lowest and highest key."
      )
  }
}
