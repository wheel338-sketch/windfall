import type { VisualSignal } from "./render"

const N = 1024
const HOP = 256

/** Independently authored radix-two FFT. Periodic Hann / Parseval power
 * normalization matches ANALYZER-TAPS; no codec, FFT or native dependency. */
function powers(input: Float64Array): Float64Array {
  const real = input.slice()
  const imag = new Float64Array(N)
  for (let i = 1, j = 0; i < N; i += 1) {
    let bit = N >> 1
    for (; j & bit; bit >>= 1) j ^= bit
    j ^= bit
    if (i < j) { const value = real[i]; real[i] = real[j]; real[j] = value }
  }
  for (let size = 2; size <= N; size *= 2) {
    const angle = -2 * Math.PI / size
    for (let start = 0; start < N; start += size) {
      for (let j = 0; j < size / 2; j += 1) {
        const a = start + j; const b = a + size / 2
        const cos = Math.cos(angle * j); const sin = Math.sin(angle * j)
        const tr = real[b] * cos - imag[b] * sin
        const ti = real[b] * sin + imag[b] * cos
        real[b] = real[a] - tr; imag[b] = imag[a] - ti
        real[a] += tr; imag[a] += ti
      }
    }
  }
  const result = new Float64Array(N / 2 + 1)
  for (let bin = 0; bin < result.length; bin += 1) {
    const factor = bin === 0 || bin === N / 2 ? 1 : 2
    result[bin] = factor * (real[bin] ** 2 + imag[bin] ** 2) / (N * N * 0.375)
  }
  return result
}

/** Latest *completed* analyzer window at this video frame's audio position.
 * Startup before N frames is explicitly absent, just like the native feed. */
export function offlineSignal(samples: ArrayLike<number>, endFrame: number): VisualSignal | null {
  if (endFrame < N) return null
  const end = N + Math.floor((endFrame - N) / HOP) * HOP
  if (end * 2 > samples.length) throw new Error("Analysis extends beyond the audio")
  const left = new Float64Array(N); const right = new Float64Array(N)
  let sum = 0
  for (let i = 0; i < N; i += 1) {
    const l = samples[(end - N + i) * 2]; const r = samples[(end - N + i) * 2 + 1]
    if (!Number.isFinite(l) || !Number.isFinite(r)) throw new Error("Audio contains an invalid sample")
    sum += (l * l + r * r) / 2
    const window = 0.5 - 0.5 * Math.cos(2 * Math.PI * i / N)
    left[i] = l * window; right[i] = r * window
  }
  const lp = powers(left); const rp = powers(right)
  const bands = new Array<number>(64).fill(0)
  // Native service uses floor-partitioned contiguous FFT-bin groups.
  for (let band = 0; band < bands.length; band += 1) {
    const from = Math.floor(band * lp.length / bands.length)
    const to = Math.floor((band + 1) * lp.length / bands.length)
    for (let bin = from; bin < to; bin += 1) bands[band] += (lp[bin] + rp[bin]) / 2
  }
  return { rms: Math.sqrt(sum / N), bands }
}
