import type { InstrumentAsset } from "@/bindings"

export type AssetRequest = { type: "layer"; bytes: ArrayBuffer } | { type: "snapshots"; a: number[]; b: number[] }
/** Bounded worker-side WAV decoding; no WebAudio device or audio callback. */
export function prepareInstrumentAsset(request: AssetRequest): InstrumentAsset {
  if (request.type === "snapshots") {
    if (request.a.length !== 32 || request.b.length !== 32 || [...request.a,...request.b].some(level => !Number.isFinite(level) || level<0 || level>1)) throw new Error("Snapshots need 32 harmonic levels from zero to one")
    return { type: "spectra", frameSeconds: 0.1, rootHz: 440, frames: [request.a,request.b].map(levels => ({ partials: levels.map((level,index) => ({ ratio: index+1,level,phase: 0,pan: 0 })) })) }
  }
  const view = new DataView(request.bytes)
  const text = (at: number, count: number) => Array.from({ length: count }, (_, i) => String.fromCharCode(view.getUint8(at + i))).join("")
  if (view.byteLength < 44 || text(0,4) !== "RIFF" || text(8,4) !== "WAVE") throw new Error("Choose a PCM or float WAV file")
  let format = 0, channels = 0, rate = 0, bits = 0, stride = 0, start = 0, length = 0
  for (let at = 12; at + 8 <= view.byteLength;) {
    const count = view.getUint32(at + 4,true), next = at + 8 + count
    if (next > view.byteLength) throw new Error("The WAV has a truncated chunk")
    const tag = text(at,4)
    if (tag === "fmt ") { if (count < 16) throw new Error("The WAV format chunk is too short"); format = view.getUint16(at+8,true); channels = view.getUint16(at+10,true); rate = view.getUint32(at+12,true); stride = view.getUint16(at+20,true); bits = view.getUint16(at+22,true) }
    if (tag === "data") { start = at+8; length = count }
    at = next + (count % 2)
  }
  if (![1,3].includes(format) || channels < 1 || channels > 8 || rate < 8000 || rate > 192000 || ![16,24,32].includes(bits) || (format === 3 && bits !== 32) || stride !== channels * (bits/8) || length < stride) throw new Error("Supported WAV: 8–192 kHz, 1–8 channels, PCM 16/24/32-bit or float 32-bit")
  const frames = Math.floor(length / stride), kept = Math.min(frames,32768)
  const samples = Array.from({ length: kept }, (_, frame) => {
    let sample = 0
    for (let channel = 0; channel < channels; channel++) {
      const at = start + frame*stride + channel*(bits/8)
      if (format === 3) sample += view.getFloat32(at,true)
      else if (bits === 16) sample += view.getInt16(at,true) / 32768
      else if (bits === 32) sample += view.getInt32(at,true) / 2147483648
      else { const raw = view.getUint8(at) | (view.getUint8(at+1)<<8) | (view.getUint8(at+2)<<16); sample += ((raw<<8)>>8)/8388608 }
    }
    sample /= channels
    const fade = frames > kept && frame >= kept - 64 ? (kept-frame)/64 : 1
    return Math.round(Math.max(-1,Math.min(1,Number.isFinite(sample) ? sample : 0)) * 32767 * fade)
  })
  return { type: "pcm", sampleRate: rate, samples }
}
