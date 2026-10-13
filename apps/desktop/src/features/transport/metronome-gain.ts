import { translatedText as t } from "@/lib/i18n"
export const METRONOME_GAINS = [
  { get label() { return t("Quiet") }, value: 0.25 },
  { get label() { return t("Medium") }, value: 0.5 },
  { get label() { return t("Loud") }, value: 1 },
] as const

export function nextMetronomeGain(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
