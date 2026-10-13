import { TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export const SHIFT_PRESETS = [
  { get label() { return t("16th early") }, ticks: -TICKS_PER_STEP },
  { get label() { return t("On grid") }, ticks: 0 },
  { get label() { return t("16th late") }, ticks: TICKS_PER_STEP },
  { get label() { return t("8th late") }, ticks: TICKS_PER_STEP * 2 },
] as const

export function nextShift(current: number, preset: number): number | null {
  return current === preset ? null : preset
}
