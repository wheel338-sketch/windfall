import { formatHz } from "@/components/audio"

/*
 * The bands of the splitter, as the engine makes them
 * (`crates/windfall-dsp/src/multiband/crossover.rs`): fourth-order
 * Linkwitz-Riley pairs, whose low and high sides are each 6 dB down where
 * they meet and add back up to a flat response.
 */

export type SplitSettings = {
  /** Two bands, or three. */
  three: boolean
  lowCrossoverHz: number
  highCrossoverHz: number
  lowGainDb: number
  midGainDb: number
  highGainDb: number
}

export const BAND_NAMES = ["Low", "Mid", "High"] as const

/** The crossover frequencies in effect, held where the engine holds them. */
export function crossovers(
  settings: Pick<SplitSettings, "lowCrossoverHz" | "highCrossoverHz">,
  sampleRate: number
): [number, number] {
  const low = Math.min(0.35 * sampleRate, Math.max(1, settings.lowCrossoverHz))
  const high = Math.min(
    0.45 * sampleRate,
    Math.max(1.25, settings.highCrossoverHz, low * 1.25)
  )
  return [low, high]
}

/** Low and high sides of one crossover at `hz`, as linear gains. */
function pair(cornerHz: number, hz: number, sampleRate: number) {
  const warped =
    Math.tan((Math.PI * hz) / sampleRate) /
    Math.tan((Math.PI * cornerHz) / sampleRate)
  const fourth = warped ** 4
  return { low: 1 / (1 + fourth), high: fourth / (1 + fourth) }
}

const FLOOR = 1e-10

/**
 * The level of each band in dB at each frequency: low, mid, high. In two
 * bands the mid is absent (all at the floor) and the high band carries
 * everything above the one crossover, at the high band's gain.
 */
export function bandResponsesDb(
  settings: SplitSettings,
  frequenciesHz: ArrayLike<number>,
  sampleRate: number
): [Float32Array, Float32Array, Float32Array] {
  const [lowHz, highHz] = crossovers(settings, sampleRate)
  const out: [Float32Array, Float32Array, Float32Array] = [
    new Float32Array(frequenciesHz.length),
    new Float32Array(frequenciesHz.length),
    new Float32Array(frequenciesHz.length),
  ]
  const db = (gain: number, offset: number) =>
    20 * Math.log10(Math.max(FLOOR, gain)) + offset
  for (let index = 0; index < frequenciesHz.length; index += 1) {
    const hz = Math.min(0.499 * sampleRate, frequenciesHz[index])
    const first = pair(lowHz, hz, sampleRate)
    const second = pair(highHz, hz, sampleRate)
    out[0][index] = db(first.low, settings.lowGainDb)
    if (settings.three) {
      out[1][index] = db(first.high * second.low, settings.midGainDb)
      out[2][index] = db(first.high * second.high, settings.highGainDb)
    } else {
      out[1][index] = db(0, 0)
      out[2][index] = db(first.high, settings.highGainDb)
    }
  }
  return out
}

/** Where the bands divide, in words. */
export function describeSplit(
  settings: Pick<SplitSettings, "three" | "lowCrossoverHz" | "highCrossoverHz">,
  sampleRate: number
): string {
  const [low, high] = crossovers(settings, sampleRate)
  return settings.three
    ? `Low below ${formatHz(low)}, mid up to ${formatHz(high)}, high above`
    : `Low below ${formatHz(low)}, high above`
}
