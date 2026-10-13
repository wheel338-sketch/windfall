import { translatedText as t } from "@/lib/i18n"
export const LOOP_CROSSFADE_PRESETS = [
  { get label() { return t("None") }, crossfade: 0 },
  { get label() { return t("Short") }, crossfade: 0.25 },
  { get label() { return t("Medium") }, crossfade: 0.5 },
  { get label() { return t("Long") }, crossfade: 1 },
] as const

export function nextLoopCrossfade(
  current: number,
  preset: number
): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
