import { PPQ, TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export const GATE_PRESETS = [
  { get label() { return t("Written") }, ticks: 0 },
  { get label() { return t("16th") }, ticks: TICKS_PER_STEP },
  { get label() { return t("8th") }, ticks: TICKS_PER_STEP * 2 },
  { get label() { return t("Quarter") }, ticks: PPQ },
] as const

export function nextGate(current: number, preset: number): number | null {
  return current === preset ? null : preset
}
