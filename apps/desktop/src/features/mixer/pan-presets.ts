import { translatedText as t } from "@/lib/i18n"
export const PAN_PRESETS = [
  { get label() { return t("Hard left") }, value: -1 },
  { get label() { return t("Left") }, value: -0.5 },
  { get label() { return t("Center") }, value: 0 },
  { get label() { return t("Right") }, value: 0.5 },
  { get label() { return t("Hard right") }, value: 1 },
] as const

export function nextPan(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
