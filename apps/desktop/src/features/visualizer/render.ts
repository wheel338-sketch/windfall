import type { VisualSettings } from "@/bindings"
import type { AnalyzerFrame } from "@/features/analyzers/types"

export type VisualSignal = { rms: number; bands: readonly number[] }
export const VISUAL_FPS = 24

export function signalFromAnalyzer(frame: AnalyzerFrame | null): VisualSignal | null {
  if (!frame || frame.gap || frame.levels.some((level) => !level.valid || level.rms === null || level.invalid !== 0)) return null
  const rms = Math.sqrt(((frame.levels[0].rms ?? 0) ** 2 + (frame.levels[1].rms ?? 0) ** 2) / 2)
  const bands = frame.spectrum?.bands ?? []
  if (!Number.isFinite(rms) || bands.some((power) => !Number.isFinite(power) || power < 0)) return null
  return { rms, bands }
}

/** Integer sample counts remain integers even past JavaScript's safe range. */
export function visualFrameNumber(endFrame: string, rate: number): number {
  return Number((BigInt(endFrame) * BigInt(VISUAL_FPS) / BigInt(rate)) % 1000000n)
}

function color(hue: number): [number, number, number] {
  const h = ((hue % 360) + 360) / 60
  const x = 1 - Math.abs(h % 2 - 1)
  const rgb = h < 1 ? [1, x, 0] : h < 2 ? [x, 1, 0] : h < 3 ? [0, 1, x] : h < 4 ? [0, x, 1] : h < 5 ? [x, 0, 1] : [1, 0, x]
  return [Math.round(255 * rgb[0]), Math.round(255 * rgb[1]), Math.round(255 * rgb[2])]
}

/** Authored software rasterizer shared by live canvas and offline export.
 * No canvas text/font/GPU state enters an exported pixel or its hash. */
export function renderVisual(width: number, height: number, settings: VisualSettings, signal: VisualSignal | null, frame: number): Uint8ClampedArray {
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1 || width > 1920 || height > 1080) throw new Error("Invalid visual dimensions")
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let i = 0; i < pixels.length; i += 4) {
    pixels[i] = 8; pixels[i + 1] = 12; pixels[i + 2] = 20; pixels[i + 3] = 255
  }
  if (!signal) return pixels
  const rgb = color(settings.hue)
  const energy = Math.min(1, Math.max(0, signal.rms * settings.intensity))
  const put = (x: number, y: number, strength = 1) => {
    x = Math.round(x); y = Math.round(y)
    if (x < 0 || x >= width || y < 0 || y >= height) return
    const i = (y * width + x) * 4
    for (let c = 0; c < 3; c += 1) pixels[i + c] = Math.max(pixels[i + c], Math.round(rgb[c] * strength))
  }
  const circle = (cx: number, cy: number, radius: number) => {
    for (let y = Math.floor(cy - radius); y <= Math.ceil(cy + radius); y += 1) {
      for (let x = Math.floor(cx - radius); x <= Math.ceil(cx + radius); x += 1) {
        if ((x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2) put(x, y)
      }
    }
  }
  if (settings.scene === "bars") {
    const count = 16
    for (let bar = 0; bar < count; bar += 1) {
      const start = Math.floor(bar * signal.bands.length / count)
      const end = Math.floor((bar + 1) * signal.bands.length / count)
      let power = 0
      for (let i = start; i < end; i += 1) power += signal.bands[i]
      const level = Math.min(1, Math.sqrt(power) * settings.intensity * 3)
      const left = Math.floor(bar * width / count)
      const right = Math.floor((bar + 1) * width / count) - 1
      for (let y = height - Math.ceil(level * height); y < height; y += 1) {
        for (let x = left; x < right; x += 1) put(x, y, 0.5 + 0.5 * (height - y) / height)
      }
    }
  } else if (settings.scene === "orbit") {
    const angle = frame * Math.PI / (VISUAL_FPS * 2)
    const radius = Math.min(width, height) * (0.15 + 0.25 * energy)
    for (let orb = 0; orb < 6; orb += 1) {
      const at = angle + orb * Math.PI / 3
      circle(width / 2 + Math.cos(at) * radius, height / 2 + Math.sin(at) * radius, 2 + energy * Math.min(width, height) * 0.06)
    }
  } else {
    for (let x = 0; x < width; x += 1) {
      const band = signal.bands[Math.floor(x * signal.bands.length / width)] ?? 0
      const y = height / 2 + Math.sin(x / width * Math.PI * 6 + frame / VISUAL_FPS) * height * energy * 0.4
      const thick = 1 + Math.min(8, Math.sqrt(band) * settings.intensity * 12)
      for (let dy = -thick; dy <= thick; dy += 1) put(x, y + dy)
    }
  }
  return pixels
}

/** Portable FNV-1a pixel checksum for fixtures and export manifests. */
export function frameHash(pixels: Uint8ClampedArray): string {
  let hash = 2166136261
  for (const byte of pixels) hash = Math.imul(hash ^ byte, 16777619) >>> 0
  return hash.toString(16).padStart(8, "0")
}
