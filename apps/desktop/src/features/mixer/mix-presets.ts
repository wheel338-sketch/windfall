import { translatedText as t } from "@/lib/i18n"
export const MIX_PRESETS = [
  { get label() { return t("Dry") }, value: 0 },
  { label: "25%", value: 0.25 },
  { get label() { return t("Half") }, value: 0.5 },
  { label: "75%", value: 0.75 },
  { get label() { return t("Wet") }, value: 1 },
] as const

export function nextMix(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
