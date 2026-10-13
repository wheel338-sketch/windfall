import { translatedText as t } from "@/lib/i18n"
export const SEND_PRESETS = [
  { get label() { return t("Off") }, value: 0 },
  { get label() { return t("Quiet") }, value: 0.5 },
  { get label() { return t("Unity") }, value: 1 },
  { get label() { return t("Loud") }, value: 1.5 },
] as const

export function nextSendGain(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
