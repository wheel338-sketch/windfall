import { translatedText as t } from "@/lib/i18n"
export const SAMPLE_GAIN_PRESETS = [
  { get label() { return t("Quiet") }, gain: 0.5 },
  { get label() { return t("Unity") }, gain: 1 },
  { get label() { return t("Loud") }, gain: 1.5 },
] as const

export function nextSampleGain(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
