import { translatedText as t } from "@/lib/i18n"
export const CUT_GROUP_PRESETS = [
  { get label() { return t("None") }, cutGroup: 0 },
  { label: "1", cutGroup: 1 },
  { label: "2", cutGroup: 2 },
  { label: "3", cutGroup: 3 },
] as const

export function nextCutGroup(current: number, preset: number): number | null {
  return current === preset ? null : preset
}
