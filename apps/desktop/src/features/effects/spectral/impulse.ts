import type { ConvolverImpulse } from "@/bindings"
const MAX_FRAMES = 65_536
/** Strict mono/stereo PCM or float WAV reader, called before document dispatch. */
export function readImpulseWav(buffer: ArrayBuffer, name: string): ConvolverImpulse {
  const view = new DataView(buffer)
  const tag = (at: number) => String.fromCharCode(...new Uint8Array(buffer, at, 4))
  if (buffer.byteLength < 12 || tag(0) !== "RIFF" || tag(8) !== "WAVE") throw new Error("Choose a PCM or float WAV impulse response")
  const limit = view.getUint32(4, true) + 8
  if (limit > buffer.byteLength) throw new Error("Truncated WAV file")
  let format = 0, channels = 0, sampleRate = 0, bits = 0, alignment = 0, dataAt = 0, dataLength = 0
  for (let at = 12; at + 8 <= limit;) {
    const size = view.getUint32(at + 4, true), start = at + 8
    if (start + size > limit) throw new Error("Truncated WAV chunk")
    if (tag(at) === "fmt ") {
      if (size < 16) throw new Error("Invalid WAV format")
      format = view.getUint16(start, true); channels = view.getUint16(start + 2, true)
      sampleRate = view.getUint32(start + 4, true); alignment = view.getUint16(start + 12, true); bits = view.getUint16(start + 14, true)
    } else if (tag(at) === "data") { dataAt = start; dataLength = size }
    at = start + size + (size & 1)
  }
  if (![1, 2].includes(channels) || sampleRate < 8000 || sampleRate > 384000) throw new Error("IR must be mono or stereo, 8000-384000 Hz")
  if (!(format === 1 && [16, 24, 32].includes(bits)) && !(format === 3 && bits === 32)) throw new Error("Supported WAV encodings: PCM 16/24/32 or float 32")
  if (alignment !== channels * bits / 8 || !dataAt || dataLength % alignment) throw new Error("Invalid WAV frame alignment")
  const length = dataLength / alignment
  if (!length || length > MAX_FRAMES) throw new Error("IR must contain 1-65536 frames; trim a longer file before import")
  const read = (at: number): number => {
    if (format === 3) return view.getFloat32(at, true)
    if (bits === 16) return view.getInt16(at, true) / 32768
    if (bits === 32) return view.getInt32(at, true) / 2147483648
    const value = view.getUint8(at) | view.getUint8(at + 1) << 8 | view.getUint8(at + 2) << 16
    return (value << 8 >> 8) / 8388608
  }
  const frames: [number, number][] = Array.from({ length }, (_, n) => {
    const left = read(dataAt + n * alignment), right = channels === 1 ? left : read(dataAt + n * alignment + bits / 8)
    if (![left, right].every((v) => Number.isFinite(v) && Math.abs(v) <= 8)) throw new Error("IR contains damaged or excessive samples")
    return [left, right]
  })
  // The model's limit is UTF-8 bytes, rather than JavaScript code units.
  let savedName = "", bytes = 0
  for (const character of name) {
    const size = new TextEncoder().encode(character).length
    if (bytes + size > 256) break
    savedName += character; bytes += size
  }
  return { name: savedName || "Impulse", sampleRate, frames }
}
/** Windfall-generated symmetric FIR: 512 samples intentional group delay. */
export function linearPhaseImpulse(lowDb: number, midDb: number, highDb: number): ConvolverImpulse {
  if (![lowDb, midDb, highDb].every((v) => Number.isFinite(v) && Math.abs(v) <= 24)) throw new Error("EQ gain must be within -24 to 24 dB")
  const gains = [lowDb, midDb, highDb].map((v) => 10 ** (v / 20))
  const lowpass = (n: number, hz: number) => n === 0 ? 2 * hz / 48000 : Math.sin(2 * Math.PI * hz * n / 48000) / (Math.PI * n)
  const frames: [number, number][] = Array.from({ length: 1025 }, (_, i) => {
    const n = i - 512, window = 0.42 - 0.5 * Math.cos(2 * Math.PI * i / 1024) + 0.08 * Math.cos(4 * Math.PI * i / 1024)
    const tap = ((gains[0] - gains[1]) * lowpass(n, 500) + (gains[1] - gains[2]) * lowpass(n, 2000)) * window + (n === 0 ? gains[2] : 0)
    if (Math.abs(tap) > 8) throw new Error("Combined EQ gain exceeds the IR headroom limit")
    return [tap, tap]
  })
  return { name: `Linear-phase EQ ${lowDb}/${midDb}/${highDb} dB`, sampleRate: 48000, groupDelay: 512, eqGains: [lowDb, midDb, highDb], frames }
}
