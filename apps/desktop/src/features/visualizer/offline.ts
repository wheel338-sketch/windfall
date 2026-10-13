import type { VisualSignal } from "./render"

const N = 1024
const HOP = 256

// Geometry is fixed at 1024 samples. These retain the original expression
// order, while each stereo window reuses its Hann and butterfly coefficients.
const HANN = Float64Array.from({ length: N }, (_, i) => 0.5 - 0.5 * Math.cos(2 * Math.PI * i / N))
const TWIDDLES = Array.from({ length: Math.log2(N) }, (_, stage) => {
  const size = 2 ** (stage + 1)
  const angle = -2 * Math.PI / size
  return {
    real: Float64Array.from({ length: size / 2 }, (_, j) => Math.cos(angle * j)),
    imag: Float64Array.from({ length: size / 2 }, (_, j) => Math.sin(angle * j)),
  }
})

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
  for (let size = 2, stage = 0; size <= N; size *= 2, stage += 1) {
    const twiddles = TWIDDLES[stage]
    for (let start = 0; start < N; start += size) {
      for (let j = 0; j < size / 2; j += 1) {
        const a = start + j; const b = a + size / 2
        const cos = twiddles.real[j]; const sin = twiddles.imag[j]
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
  let leftSquare = 0; let rightSquare = 0
  for (let i = 0; i < N; i += 1) {
    const l = samples[(end - N + i) * 2]; const r = samples[(end - N + i) * 2 + 1]
    if (!Number.isFinite(l) || !Number.isFinite(r)) throw new Error("Audio contains an invalid sample")
    leftSquare += l * l; rightSquare += r * r
    const window = HANN[i]
    left[i] = l * window; right[i] = r * window
  }
  const lp = powers(left); const rp = powers(right)
  const bands = new Array<number>(64).fill(0)
  // Native service uses floor-partitioned contiguous FFT-bin groups.
  for (let band = 0; band < bands.length; band += 1) {
    const from = Math.floor(band * lp.length / bands.length)
    const to = Math.floor((band + 1) * lp.length / bands.length)
    for (let bin = from; bin < to; bin += 1) bands[band] += lp[bin] + rp[bin]
    bands[band] = Math.fround(bands[band] / 2)
  }
  // Match the f32 wire quantization of each native level and band.
  const lr = Math.fround(Math.sqrt(leftSquare / N)); const rr = Math.fround(Math.sqrt(rightSquare / N))
  return { rms: Math.sqrt((lr * lr + rr * rr) / 2), bands }
}
