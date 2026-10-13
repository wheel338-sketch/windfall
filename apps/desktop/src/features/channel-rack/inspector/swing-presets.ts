import { translatedText as t } from "@/lib/i18n"
export const SWING_PRESETS = [
  { get label() { return t("Straight") }, value: 0 },
  { get label() { return t("Light") }, value: 0.25 },
  { get label() { return t("Half") }, value: 0.5 },
  { get label() { return t("Full") }, value: 1 },
] as const

export function nextSwing(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
