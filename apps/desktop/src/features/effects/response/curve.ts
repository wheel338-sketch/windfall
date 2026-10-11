import {
  highPass,
  highShelf,
  lowPass,
  lowShelf,
  peak,
  responseGrid,
  sectionGainDb,
  type ResponseGrid,
  type Section,
} from "../eq/eq-response"

/*
 * Response curves for the filter and equalizer editors that are not the
 * parametric equalizer. The second-order sections are the engine's
 * (`crates/windfall-dsp/src/blocks/biquad.rs`): the same cookbook formulas,
 * so a display made from them shows what is heard.
 */

export { highPass, highShelf, lowPass, lowShelf, peak, responseGrid }
export type { ResponseGrid, Section }

/** A section that changes nothing. */
export const IDENTITY: Section = { b0: 1, b1: 0, b2: 0, a1: 0, a2: 0 }

/** Lowest and highest Q a section is built with, as in the engine. */
const Q_MIN = 0.025
const Q_MAX = 40

function prototype(frequencyHz: number, q: number, sampleRate: number) {
  const rate = Math.max(1, sampleRate)
  const frequency = Math.min(0.49 * rate, Math.max(1, frequencyHz))
  const omega = (2 * Math.PI * frequency) / rate
  return {
    cos: Math.cos(omega),
    alpha: Math.sin(omega) / (2 * Math.min(Q_MAX, Math.max(Q_MIN, q))),
  }
}

function normalized(b: number[], a: number[]): Section {
  const scale = 1 / a[0]
  return {
    b0: b[0] * scale,
    b1: b[1] * scale,
    b2: b[2] * scale,
    a1: a[1] * scale,
    a2: a[2] * scale,
  }
}

/** Passes a band around the frequency at unity and attenuates the rest. */
export function bandPass(
  frequencyHz: number,
  q: number,
  sampleRate: number
): Section {
  const { cos, alpha } = prototype(frequencyHz, q, sampleRate)
  return normalized([alpha, 0, -alpha], [1 + alpha, -2 * cos, 1 - alpha])
}

/** Removes the frequency and leaves the rest. */
export function notch(
  frequencyHz: number,
  q: number,
  sampleRate: number
): Section {
  const { cos, alpha } = prototype(frequencyHz, q, sampleRate)
  return normalized([1, -2 * cos, 1], [1 + alpha, -2 * cos, 1 - alpha])
}

/**
 * What sections in series do, in dB at each frequency of the grid, on top
 * of `offsetDb`.
 */
export function seriesResponseDb(
  sections: readonly Section[],
  grid: ResponseGrid,
  offsetDb = 0,
  out = new Float32Array(grid.phi.length)
): Float32Array {
  out.fill(offsetDb)
  for (const section of sections) {
    if (section === IDENTITY) continue
    for (let index = 0; index < out.length; index += 1) {
      out[index] += sectionGainDb(section, grid.phi[index])
    }
  }
  return out
}

/** Gain of sections in series at one frequency, in dB. */
export function seriesGainAt(
  sections: readonly Section[],
  frequencyHz: number,
  sampleRate: number
): number {
  return seriesResponseDb(sections, responseGrid([frequencyHz], sampleRate))[0]
}

/** The largest swing of a curve from 0 dB, and where it is. */
export function curveExtreme(
  gainsDb: ArrayLike<number>,
  frequenciesHz: ArrayLike<number>
): { gainDb: number; frequencyHz: number } {
  let at = 0
  for (let index = 1; index < gainsDb.length; index += 1) {
    if (Math.abs(gainsDb[index]) > Math.abs(gainsDb[at])) at = index
  }
  return { gainDb: gainsDb[at] ?? 0, frequencyHz: frequenciesHz[at] ?? 0 }
}
