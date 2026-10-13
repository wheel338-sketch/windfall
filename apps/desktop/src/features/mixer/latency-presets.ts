import { translatedText as t } from "@/lib/i18n"
export const LATENCY_PRESETS = [
  { get label() { return t("None") }, value: 0 },
  { label: "1 ms", value: 1 },
  { label: "5 ms", value: 5 },
  { label: "10 ms", value: 10 },
] as const

export function nextLatency(current: number, preset: number): number | null {
  return Math.abs(current - preset) < 0.001 ? null : preset
}
