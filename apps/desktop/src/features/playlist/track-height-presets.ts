import { DEFAULT_ROW_HEIGHT, MIN_ROW_HEIGHT, TALL_ROW_HEIGHT } from "./layout"
import { translatedText as t } from "@/lib/i18n"

export const TRACK_HEIGHT_PRESETS = [
  { get label() { return t("Follow") }, height: 0 },
  { get label() { return t("Short") }, height: MIN_ROW_HEIGHT },
  { get label() { return t("Normal") }, height: DEFAULT_ROW_HEIGHT },
  { get label() { return t("Tall") }, height: TALL_ROW_HEIGHT },
] as const

export function nextTrackHeight(
  current: number,
  preset: number
): number | null {
  return current === preset ? null : preset
}
