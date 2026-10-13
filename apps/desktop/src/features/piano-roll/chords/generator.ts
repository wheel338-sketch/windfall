import { pitchClass, TRIADS, type TriadQuality } from "./detection"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export const MIN_SEED = -2_147_483_648
export const MAX_SEED = 2_147_483_647

// The seven diatonic triads of a major key, in scale-degree order.
const DIATONIC_TRIADS: readonly {
  degree: string
  offset: number
  quality: TriadQuality
}[] = [
  { degree: "I", offset: 0, quality: "major" },
  { degree: "ii", offset: 2, quality: "minor" },
  { degree: "iii", offset: 4, quality: "minor" },
  { degree: "IV", offset: 5, quality: "major" },
  { degree: "V", offset: 7, quality: "major" },
  { degree: "vi", offset: 9, quality: "minor" },
  { degree: "vii°", offset: 11, quality: "diminished" },
]

export function triadKeys(root: number, quality: TriadQuality): number[] {
  const triad = TRIADS.find((item) => item.quality === quality)
  if (!Number.isInteger(root) || root < 0 || root > 127 || !triad)
    throw new Error(
      t("Choose a whole MIDI root from 0 to 127 and a triad quality.")
    )
  const keys = triad.intervals.map((interval) => root + interval)
  if (keys.some((key) => key > 127))
    throw new Error(
      t("Lower the root so every chord note fits within MIDI keys 0–127.")
    )
  return keys
}

/** A small reproducible permutation rather than runtime randomness. Seven
 * consecutive seeds visit all seven choices; adjacent seeds always differ. */
export function seededTriad(seed: number, tonic: number) {
  if (!Number.isInteger(seed) || seed < MIN_SEED || seed > MAX_SEED)
    throw new Error(message("Use a whole seed from {minSeed} to {maxSeed}.", { minSeed: plainNumber(MIN_SEED), maxSeed: plainNumber(MAX_SEED) }))
  if (!Number.isInteger(tonic) || tonic < 0 || tonic > 110)
    throw new Error(
      t("Use a MIDI key tonic from 0 to 110 so all seven generated triads fit.")
    )
  const residue = ((seed % 7) + 7) % 7
  const choice = DIATONIC_TRIADS[(residue * 5 + pitchClass(tonic) * 3) % 7]
  const root = tonic + choice.offset
  return {
    degree: choice.degree,
    root,
    quality: choice.quality,
    keys: triadKeys(root, choice.quality),
  }
}

export function advanceSeed(seed: number): number {
  // Going past the UI's signed integer range wraps and still changes choice.
  return seed === MAX_SEED ? MIN_SEED : seed + 1
}
