/** AVI 1.0 BI_RGB reader for the repository's raw visualizer exports.
 * Holds file bytes, frame offsets and one caller-owned pixel surface.
 * Audio belongs to the native mixer. */
import { translatedText as t } from "@/lib/i18n"
export type RawAvi = {
  width: number; height: number; duration: number
  frameAt(seconds: number): number
  drawFrame(index: number, pixels: Uint8ClampedArray): void
}
export function readRawAvi(buffer: ArrayBuffer): RawAvi | null {
  const bytes = new Uint8Array(buffer)
  const view = new DataView(buffer)
  const text = (at: number) => String.fromCharCode(...bytes.subarray(at, at + 4))
  if (bytes.length < 12 || text(0) !== "RIFF" || text(8) !== "AVI ") return null
  if (bytes.length > 64 * 1024 * 1024) throw new Error(t("AVI exceeds the 64 MiB picture budget"))
  const end = view.getUint32(4, true) + 8
  if (end > bytes.length || end < 12) throw new Error(t("The AVI is truncated"))
  let streamCount = 0; let stream = -1; let width = 0; let signedHeight = 0; let bits = 0; let fps = 0; let start = 0
  const frames: { at: number; length: number }[] = []
  const walk = (first: number, limit: number, depth: number) => {
    if (depth > 8) throw new Error(t("AVI contains too many nested lists"))
    let at = first
    while (at < limit) {
      if (at + 8 > limit) throw new Error(t("Truncated AVI chunk"))
      const id = text(at); const size = view.getUint32(at + 4, true); const data = at + 8
      if (data + size > limit) throw new Error(t("Truncated AVI payload"))
      if (id === "LIST") {
        if (size < 4) throw new Error(t("Invalid AVI list"))
        walk(data + 4, data + size, depth + 1)
      } else if (id === "strh") {
        if (size < 56) throw new Error(t("Truncated AVI stream header"))
        if (stream < 0 && text(data) === "vids") {
          stream = streamCount
          fps = view.getUint32(data + 24, true) / view.getUint32(data + 20, true)
          start = view.getUint32(data + 28, true) / fps
        }
        streamCount += 1
      } else if (id === "strf" && stream >= 0 && stream === streamCount - 1 && width === 0) {
        if (size < 40 || view.getUint32(data, true) < 40) throw new Error(t("Invalid AVI picture format"))
        width = view.getInt32(data + 4, true); signedHeight = view.getInt32(data + 8, true); bits = view.getUint16(data + 14, true)
        if (view.getUint16(data + 12, true) !== 1 || view.getUint32(data + 16, true) !== 0 || (bits !== 24 && bits !== 32)) throw new Error(t("AVI picture decoder unavailable: choose uncompressed RGB AVI or a webview-supported file"))
      } else if (/^\d\d(?:db|dc)$/.test(id) && Number(id.slice(0, 2)) === stream) {
        if (frames.length >= 16384) throw new Error(t("AVI exceeds the frame index budget"))
        frames.push({ at: data, length: size })
      }
      at = data + size + size % 2
      if (at > limit) throw new Error(t("Missing AVI chunk padding"))
    }
  }
  walk(12, end, 0)
  const height = Math.abs(signedHeight)
  if (width < 1 || width > 1920 || height < 1 || height > 1080 || !Number.isFinite(fps) || fps <= 0 || fps > 240 || frames.length === 0) throw new Error(t("AVI has no bounded supported picture stream"))
  const stride = Math.ceil(width * bits / 8 / 4) * 4
  if (frames.some((frame) => frame.length < stride * height)) throw new Error(t("Truncated raw AVI picture"))
  return {
    width, height, duration: start + frames.length / fps,
    frameAt: (seconds) => Number.isFinite(seconds) && seconds >= start && seconds < start + frames.length / fps ? Math.min(frames.length - 1, Math.floor((seconds - start) * fps)) : -1,
    drawFrame: (index, pixels) => {
      if (!Number.isInteger(index) || index < 0 || index >= frames.length || pixels.length !== width * height * 4) throw new Error(t("Invalid AVI frame or pixel surface"))
      const first = frames[index].at
      for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          const source = first + (signedHeight > 0 ? height - 1 - y : y) * stride + x * bits / 8
          const target = (y * width + x) * 4
          pixels[target] = bytes[source + 2]; pixels[target + 1] = bytes[source + 1]; pixels[target + 2] = bytes[source]; pixels[target + 3] = 255
        }
      }
    },
  }
}
