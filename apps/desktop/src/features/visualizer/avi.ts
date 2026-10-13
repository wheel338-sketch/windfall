import type { VisualSettings } from "@/bindings"
import { offlineSignal } from "./offline"
import { frameHash, renderVisual, VISUAL_FPS } from "./render"
import { translatedText as t } from "@/lib/i18n"

const MAX_BYTES = 128 * 1024 * 1024
const encoder = new TextEncoder()
const ascii = (s: string) => encoder.encode(s)
function words(values: number[]): Uint8Array {
  const bytes = new Uint8Array(values.length * 4)
  const view = new DataView(bytes.buffer)
  values.forEach((value, index) => view.setUint32(index * 4, value, true))
  return bytes
}
function join(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0))
  let offset = 0
  for (const part of parts) { out.set(part, offset); offset += part.length }
  return out
}
function chunk(id: string, data: Uint8Array): Uint8Array {
  return join([ascii(id), words([data.length]), data, ...(data.length % 2 ? [new Uint8Array(1)] : [])])
}
function list(id: string, parts: Uint8Array[]): Uint8Array { return chunk("LIST", join([ascii(id), ...parts])) }
function streamHeader(type: string, handler: string, scale: number, rate: number, length: number, suggested: number, sampleSize: number, width = 0, height = 0): Uint8Array {
  const header = new Uint8Array(56)
  header.set(ascii(type)); header.set(ascii(handler), 4)
  const view = new DataView(header.buffer)
  view.setUint32(20, scale, true); view.setUint32(24, rate, true)
  view.setUint32(32, length, true); view.setUint32(36, suggested, true)
  view.setUint32(40, 0xffffffff, true); view.setUint32(44, sampleSize, true)
  view.setUint16(52, width, true); view.setUint16(54, height, true)
  return header
}

export type AviMetadata = { width: number; height: number; fps: number; firstVisualFrame: number; videoFrames: number; audioFrames: number; sampleRate: number; frameHashes: string[] }
export type AviExport = { bytes: Uint8Array; metadata: AviMetadata }

/** RIFF AVI 1.0: bottom-up padded BGR24 BI_RGB frames and interleaved
 * signed 16-bit stereo PCM. No compressed codec or native library. */
export async function exportVisualAvi(samples: ArrayLike<number>, sampleRate: number, settings: VisualSettings, width = 320, height = 180, cancelled: () => boolean = () => false, progress: (fraction: number) => void = () => {}, firstVisualFrame = 0): Promise<AviExport> {
  if (!Number.isSafeInteger(firstVisualFrame) || firstVisualFrame < 0) throw new Error(t("Invalid visual phase"))
  if (samples.length % 2 !== 0 || !Number.isInteger(sampleRate) || sampleRate < 8000 || sampleRate > 192000 || samples.length === 0) throw new Error(t("Export needs nonempty stereo PCM at a supported sample rate"))
  if (samples.length / 2 > sampleRate * 30 || width < 1 || height < 1 || width > 640 || height > 360 || !Number.isInteger(width) || !Number.isInteger(height)) throw new Error(t("Export exceeds the 30-second or 640×360 limit"))
  const audioFrames = samples.length / 2
  const videoFrames = Math.ceil(audioFrames * VISUAL_FPS / sampleRate)
  const stride = Math.ceil(width * 3 / 4) * 4
  const frameBytes = stride * height
  if (videoFrames * (frameBytes + 40) + samples.length * 2 + 4096 > MAX_BYTES) throw new Error(t("Uncompressed AVI exceeds the 128 MiB export budget; choose a shorter range"))
  const videoFormat = words([40, width, height, 24 * 65536 + 1, 0, frameBytes, 0, 0, 0, 0])
  const audioFormat = new Uint8Array(16)
  const af = new DataView(audioFormat.buffer)
  af.setUint16(0, 1, true); af.setUint16(2, 2, true); af.setUint32(4, sampleRate, true)
  af.setUint32(8, sampleRate * 4, true); af.setUint16(12, 4, true); af.setUint16(14, 16, true)
  const header = list("hdrl", [
    chunk("avih", words([Math.round(1000000 / VISUAL_FPS), frameBytes * VISUAL_FPS + sampleRate * 4, 0, 0x10, videoFrames, 0, 2, frameBytes, width, height, 0, 0, 0, 0])),
    list("strl", [chunk("strh", streamHeader("vids", "DIB ", 1, VISUAL_FPS, videoFrames, frameBytes, 0, width, height)), chunk("strf", videoFormat)]),
    list("strl", [chunk("strh", streamHeader("auds", "\0\0\0\0", 4, sampleRate * 4, audioFrames, Math.ceil(sampleRate / VISUAL_FPS) * 4, 4)), chunk("strf", audioFormat)]),
  ])
  const chunks: Uint8Array[] = []
  const indexes: Uint8Array[] = []
  const frameHashes: string[] = []
  let offset = 4
  const add = (id: string, data: Uint8Array, flags: number) => {
    const packed = chunk(id, data)
    indexes.push(join([ascii(id), words([flags, offset, data.length])]))
    chunks.push(packed); offset += packed.length
  }
  for (let frame = 0; frame < videoFrames; frame += 1) {
    if (cancelled()) throw new Error(t("Video export cancelled"))
    const first = Math.floor(frame * sampleRate / VISUAL_FPS)
    const end = Math.min(audioFrames, Math.floor((frame + 1) * sampleRate / VISUAL_FPS))
    const rgba = renderVisual(width, height, settings, offlineSignal(samples, first), firstVisualFrame + frame)
    frameHashes.push(frameHash(rgba))
    const bgr = new Uint8Array(frameBytes)
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const source = (y * width + x) * 4; const target = (height - 1 - y) * stride + x * 3
        bgr[target] = rgba[source + 2]; bgr[target + 1] = rgba[source + 1]; bgr[target + 2] = rgba[source]
      }
    }
    add("00db", bgr, 0x10)
    const pcm = new Uint8Array((end - first) * 4)
    const view = new DataView(pcm.buffer)
    for (let i = first * 2; i < end * 2; i += 1) {
      const value = samples[i]
      if (!Number.isFinite(value)) throw new Error(t("Audio contains an invalid sample"))
      view.setInt16((i - first * 2) * 2, Math.round(Math.max(-1, Math.min(1, value)) * (value < 0 ? 32768 : 32767)), true)
    }
    add("01wb", pcm, 0)
    progress((frame + 1) / videoFrames)
    // Yield through the webview's rendering lifecycle; no wall-clock timing.
    if (frame % 8 === 7) await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  }
  const body = join([ascii("AVI "), header, list("movi", chunks), chunk("idx1", join(indexes))])
  return { bytes: chunk("RIFF", body), metadata: { width, height, fps: VISUAL_FPS, firstVisualFrame, videoFrames, audioFrames, sampleRate, frameHashes } }
}
