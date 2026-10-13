/*
 * Key signatures and pitch spelling for the engraved score.
 *
 * Pitches are placed on the line of fifths (F = -1, C = 0, G = 1, ... with
 * sharps continuing upward and flats downward). A key with `fifths` sharps
 * (negative for flats) owns the seven positions `fifths - 1 ... fifths + 5`.
 */

export type KeyMode = "major" | "minor"
export type KeySignature = { fifths: number; mode: KeyMode }
/** `step` counts C = 0 ... B = 6. The octave is the written one (C4 = 60). */
export type Spelling = { step: number; alter: -1 | 0 | 1; octave: number }

/** Steps in line-of-fifths order: F C G D A E B. */
const FIFTH_STEPS = [3, 0, 4, 1, 5, 2, 6] as const
const STEP_NAMES = ["C", "D", "E", "F", "G", "A", "B"]

function mod(value: number, by: number): number {
  return ((value % by) + by) % by
}

function nameAt(position: number): string {
  const alter = Math.floor((position + 1) / 7)
  const sign = alter > 0 ? "♯" : alter < 0 ? "♭" : ""
  return STEP_NAMES[FIFTH_STEPS[mod(position + 1, 7)]] + sign
}

export type KeyChoice = KeySignature & { id: string; label: string }

const FIFTHS_ORDER = [0, 1, 2, 3, 4, 5, 6, 7, -1, -2, -3, -4, -5, -6, -7]

/** All fifteen major and fifteen minor signatures, majors first. */
export const KEY_SIGNATURES: readonly KeyChoice[] = (
  ["major", "minor"] as const
).flatMap((mode) =>
  FIFTHS_ORDER.map((fifths) => ({
    id: `${mode}:${fifths}`,
    fifths,
    mode,
    label: `${nameAt(mode === "major" ? fifths : fifths + 3)} ${mode}`,
  }))
)

export function keyLabel(key: KeySignature): string {
  return (
    KEY_SIGNATURES.find(
      (item) => item.fifths === key.fifths && item.mode === key.mode
    )?.label ?? "C major"
  )
}

/** The alteration the signature gives a natural step: -1, 0 or 1. */
export function keyAlter(step: number, fifths: number): -1 | 0 | 1 {
  const index = FIFTH_STEPS.indexOf(step as (typeof FIFTH_STEPS)[number])
  if (fifths > 0 && index < fifths) return 1
  if (fifths < 0 && index >= 7 + fifths) return -1
  return 0
}

/**
 * Spells a MIDI key for a signature. The twelve pitch classes take the twelve
 * consecutive line-of-fifths positions `fifths - 3 ... fifths + 8`: the scale
 * itself, the lowered third and seventh below it, and the raised fourth,
 * first and fifth above it (which is also the minor leading tone). The window
 * is clamped so that no double sharp or double flat is ever needed.
 */
export function spell(key: number, fifths: number): Spelling {
  const low = Math.max(-8, Math.min(1, fifths - 3))
  const position = low + mod(mod(key, 12) * 7 - low, 12)
  const alter = Math.floor((position + 1) / 7) as -1 | 0 | 1
  return {
    step: FIFTH_STEPS[mod(position + 1, 7)],
    alter,
    octave: Math.floor((key - alter) / 12) - 1,
  }
}

/** Candidate signatures by preference: fewest accidentals, sharps before flats. */
const DETECT_ORDER = [0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6]

/**
 * Picks the signature whose seven notes cover the most weight (sounding time
 * per pitch class). Ties go to fewer accidentals, then to sharps. The mode is
 * a label only: minor is chosen when the relative minor's tonic, fifth, third
 * and leading tone outweigh the major tonic triad.
 */
export function detectKey(weights: readonly number[]): KeySignature {
  const at = (pitchClass: number) => weights[mod(pitchClass, 12)] ?? 0
  let best = 0
  let bestScore = -1
  for (const fifths of DETECT_ORDER) {
    let score = 0
    for (let position = fifths - 1; position <= fifths + 5; position++)
      score += at(position * 7)
    if (score > bestScore) {
      best = fifths
      bestScore = score
    }
  }
  if (bestScore <= 0) return { fifths: 0, mode: "major" }
  const tonic = best * 7
  const major = 2 * at(tonic) + at(tonic + 7) + at(tonic + 4)
  const minor =
    2 * at(tonic + 9) + at(tonic + 4) + at(tonic) + 2 * at(tonic + 8)
  return { fifths: best, mode: minor > major ? "minor" : "major" }
}
