import { translatedText as t } from "@/lib/i18n"
export const CHANNEL_VOLUME_PRESETS = [
  { get label() { return t("Quiet") }, value: 0.5 },
  { get label() { return t("Default") }, value: 0.8 },
  { get label() { return t("Unity") }, value: 1 },
  { get label() { return t("Loud") }, value: 1.5 },
] as const

export function nextChannelVolume(
  current: number,
  preset: number
): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
