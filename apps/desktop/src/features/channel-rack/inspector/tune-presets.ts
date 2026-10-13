import { joinTune } from "../steps"
import { translatedText as t } from "@/lib/i18n"

export const TUNE_PRESETS = [
  { get label() { return t("Octave down") }, semitones: -12 },
  { get label() { return t("Fifth down") }, semitones: -7 },
  { get label() { return t("Unison") }, semitones: 0 },
  { get label() { return t("Fifth up") }, semitones: 7 },
  { get label() { return t("Octave up") }, semitones: 12 },
] as const

export function nextSamplerTune(
  current: number,
  cents: number,
  presetSemitones: number
): number | null {
  const joined = joinTune(presetSemitones, cents, 48)
  return Math.abs(joined - current) < 0.001 ? null : joined
}
