import { translatedText as t } from "@/lib/i18n"
import { formatDb, formatHz } from "@/components/audio"

import { IDENTITY, peak, type Section } from "../response/curve"

/*
 * The fixed-band graphic equalizer, as the engine builds it
 * (`crates/windfall-dsp/src/eqbank/seven.rs`): seven bells in series at
 * fixed centers, each with the same Q.
 */

export const SEVEN_BAND_HZ = [63, 160, 400, 1_000, 2_500, 6_300, 16_000] as const
export const SEVEN_BAND_Q = 1.4

/** The ids of the seven gains, low band first. */
export const SEVEN_BAND_IDS = SEVEN_BAND_HZ.map(
  (_, index) => `gainsDb.${index}`
)

/** What each band is called on its fader: "63", "1k", "16k". */
export const SEVEN_BAND_LABELS = SEVEN_BAND_HZ.map((hz) =>
  hz >= 1_000 ? `${hz / 1_000}k` : String(hz)
)

/** One section per band. A band at 0 dB is left out of the signal. */
export function sevenBandSections(
  gainsDb: readonly number[],
  sampleRate: number
): Section[] {
  return SEVEN_BAND_HZ.map((hz, index) => {
    const gain = gainsDb[index] ?? 0
    return gain === 0 ? IDENTITY : peak(hz, SEVEN_BAND_Q, gain, sampleRate)
  })
}

/** The bands that are not flat, in words: "63 Hz +6.0 dB, 1.00 kHz −3.0 dB". */
export function describeBands(
  frequenciesHz: readonly number[],
  gainsDb: readonly number[]
): string {
  const moved = frequenciesHz
    .map((hz, index) => ({ hz, gain: gainsDb[index] ?? 0 }))
    .filter((band) => band.gain !== 0)
  if (moved.length === 0) return t("flat")
  return moved
    .map((band) => `${formatHz(band.hz)} ${formatDb(band.gain, 1)}`)
    .join(", ")
}
