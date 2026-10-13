import { translatedText as t } from "@/lib/i18n"
export const SAMPLE_TRIM_PRESETS = [
  { get label() { return t("Whole") }, start: 0, end: 1 },
  { get label() { return t("First half") }, start: 0, end: 0.5 },
  { get label() { return t("Second half") }, start: 0.5, end: 1 },
  { get label() { return t("Middle") }, start: 0.25, end: 0.75 },
] as const

export function nextSampleTrim(
  start: number,
  end: number,
  preset: { start: number; end: number }
): { start: number; end: number } | null {
  return Math.abs(start - preset.start) < 0.001 &&
    Math.abs(end - preset.end) < 0.001
    ? null
    : preset
}
