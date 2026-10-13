import type { EqParams } from "@/bindings"
import { effectDescriptor, paramInfo } from "@/features/params"

import {
  EQ_BAND_IDS,
  EQ_BAND_SHAPES,
  isCutBand,
  type EqBandId,
  type EqBandShape,
} from "./eq-response"
import { message } from "@/lib/i18n"

export type BandSpec = {
  id: EqBandId
  shape: EqBandShape
  /** The name the descriptors give the band: "Low cut", "Peak 1". */
  name: string
  /** The band's color, as a CSS color the display's root defines. */
  color: string
  /** Ids of the band's settings in the descriptor table. */
  enabled: string
  frequency: string
  q: string
  /** Shelves and bells have a gain, cuts a slope. */
  gain: string | null
  slope: string | null
}

// One hue per band, running through the spectrum from the lows to the
// highs, at one lightness and strength so no band outweighs another. The
// two numbers come from the editor's root, which sets them per theme.
const HUES: Record<EqBandId, number> = {
  lowCut: 35,
  lowShelf: 70,
  peak1: 115,
  peak2: 160,
  peak3: 210,
  highShelf: 260,
  highCut: 305,
}

/** Sets the lightness and strength the band colors use, for both themes. */
export const BAND_COLOR_SCOPE =
  "[--eq-l:0.56] [--eq-c:0.17] dark:[--eq-l:0.78] dark:[--eq-c:0.14]"

const descriptor = effectDescriptor("eq")

export const EQ_BANDS: readonly BandSpec[] = EQ_BAND_IDS.map((id) => {
  const cut = isCutBand(id)
  return {
    id,
    shape: EQ_BAND_SHAPES[id],
    name: paramInfo(descriptor, `${id}.enabled`).name,
    color: `oklch(var(--eq-l) var(--eq-c) ${HUES[id]})`,
    enabled: `${id}.enabled`,
    frequency: `${id}.frequencyHz`,
    q: `${id}.q`,
    gain: cut ? null : `${id}.gainDb`,
    slope: cut ? `${id}.slope` : null,
  }
})

export function bandSpec(id: EqBandId): BandSpec {
  const spec = EQ_BANDS.find((band) => band.id === id)
  if (!spec) throw new Error(message("The equaliser has no band \"{id}\"", { id }))
  return spec
}

/** What a band's node shows: its settings, with 0 dB for a cut. */
export function bandPoint(params: EqParams, id: EqBandId) {
  const band = params[id]
  return {
    enabled: band.enabled,
    frequencyHz: band.frequencyHz,
    gainDb: "gainDb" in band ? band.gainDb : 0,
    q: band.q,
  }
}
