import type { CurveLfo, CurveLfoWave } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const LFO_WAVES: { value: CurveLfoWave; label: string }[] = [
  { value: "sine", get label() { return t("Sine") } }, { value: "triangle", get label() { return t("Triangle") } },
  { value: "sawUp", get label() { return t("Rising saw") } }, { value: "sawDown", get label() { return t("Falling saw") } },
  { value: "square", get label() { return t("Pulse") } }, { value: "sampleHold", get label() { return t("Sample and hold") } },
]

/** UI shape preview only. Native Rust owns emitted points and note edits. */
export function lfoPreviewValue(lfo: CurveLfo, tick: number): number {
  const phase = lfo.phase + tick / lfo.period
  const cycle = Math.floor(phase)
  const part = ((phase % 1) + 1) % 1
  let value: number
  switch (lfo.wave) {
    case "sine": value = Math.sin(part * Math.PI * 2); break
    case "triangle": value = 1 - Math.abs(part * 4 - 2); break
    case "sawUp": value = part * 2 - 1; break
    case "sawDown": value = 1 - part * 2; break
    case "square": value = part < lfo.width ? 1 : -1; break
    case "sampleHold": {
      let word = lfo.seed ^ (cycle >>> 0) ^ (Math.floor(cycle / 4294967296) >>> 0) ^ 0x9e3779b9
      word = Math.imul(word ^ word >>> 16, 0x7feb352d)
      word = Math.imul(word ^ word >>> 15, 0x846ca68b)
      word ^= word >>> 16
      value = (word >>> 0) / 4294967296 * 2 - 1
      break
    }
  }
  return Math.min(1, Math.max(0, lfo.center + lfo.depth * value))
}
