import { describe, expect, it } from "vitest"
import { exportVisualAvi } from "./avi"
import { offlineSignal } from "./offline"
import { frameHash, renderVisual, visualSongFrame } from "./render"

type Entry = { id: string; at: number; size: number; kind?: string; children: Entry[] }
const text = (bytes: Uint8Array, at: number) => String.fromCharCode(...bytes.subarray(at, at + 4))
function entries(bytes: Uint8Array, begin: number, end: number): Entry[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const found: Entry[] = []
  for (let at = begin; at < end;) {
    const id = text(bytes, at); const size = view.getUint32(at + 4, true)
    expect(at + 8 + size).toBeLessThanOrEqual(end)
    const container = id === "LIST" || id === "RIFF"
    found.push({ id, at, size, kind: container ? text(bytes, at + 8) : undefined, children: container ? entries(bytes, at + 12, at + 8 + size) : [] })
    at += 8 + size + size % 2
  }
  return found
}
const settings = { scene: "bars", hue: 180, intensity: 1 } as const

describe("raw AVI audiovisual export", () => {
  it("retains song-time animation phase for a range export instead of restarting the scene", async () => {
    const pcm = Float32Array.from({ length: 4801 * 2 }, (_, i) => Math.sin(i / 19) * 0.2)
    const visual = { scene: "orbit", hue: 180, intensity: 1 } as const
    const phase = visualSongFrame(7 / 24)
    const result = await exportVisualAvi(pcm, 48000, visual, 32, 18, () => false, () => {}, phase)
    expect(result.metadata.firstVisualFrame).toBe(7)
    expect(result.metadata.frameHashes[2]).toBe(frameHash(renderVisual(32, 18, visual, offlineSignal(pcm, 4000), 9)))
    expect(result.metadata.frameHashes[2]).not.toBe(frameHash(renderVisual(32, 18, visual, offlineSignal(pcm, 4000), 2)))
    await expect(exportVisualAvi(pcm, 48000, visual, 32, 18, () => false, () => {}, -1)).rejects.toThrow("phase")
  })
  it("declares exact stream rates, ceil frame count, PCM length and index offsets", async () => {
    // A partial final frame must retain all audio, never silently truncate it.
    const pcm = Float32Array.from({ length: 4801 * 2 }, (_, i) => Math.sin(i / 19) * 0.2)
    const result = await exportVisualAvi(pcm, 48000, settings, 32, 18)
    const { bytes, metadata } = result
    expect(metadata).toMatchObject({ width: 32, height: 18, fps: 24, videoFrames: 3, audioFrames: 4801, sampleRate: 48000 })
    expect(metadata.frameHashes).toHaveLength(3)
    const root = entries(bytes, 0, bytes.length)[0]
    expect(root.id).toBe("RIFF"); expect(root.kind).toBe("AVI ")
    const headers = root.children.find((entry) => entry.kind === "hdrl")!
    const streams = headers.children.filter((entry) => entry.kind === "strl")
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    expect(streams).toHaveLength(2)
    const video = streams[0].children[0].at + 8
    expect(text(bytes, video)).toBe("vids")
    expect(view.getUint32(video + 20, true)).toBe(1)
    expect(view.getUint32(video + 24, true)).toBe(24)
    expect(view.getUint32(video + 32, true)).toBe(3)
    const audio = streams[1].children[0].at + 8
    expect(text(bytes, audio)).toBe("auds")
    expect(view.getUint32(audio + 20, true)).toBe(4)
    expect(view.getUint32(audio + 24, true)).toBe(48000 * 4)
    expect(view.getUint32(audio + 32, true)).toBe(4801)
    const movie = root.children.find((entry) => entry.kind === "movi")!
    expect(movie.children.filter((entry) => entry.id === "00db")).toHaveLength(3)
    const audioChunks = movie.children.filter((entry) => entry.id === "01wb")
    expect(audioChunks.reduce((sum, entry) => sum + entry.size, 0)).toBe(4801 * 4)
    let sample = 0
    for (const chunk of audioChunks) for (let at = chunk.at + 8; at < chunk.at + 8 + chunk.size; at += 2) {
      const original = pcm[sample++]
      expect(view.getInt16(at, true)).toBe(Math.round(original * (original < 0 ? 32768 : 32767)))
    }
    const index = root.children.find((entry) => entry.id === "idx1")!
    expect(index.size).toBe(movie.children.length * 16)
    for (let i = 0; i < movie.children.length; i += 1) {
      const at = index.at + 8 + i * 16; const chunk = movie.children[i]
      expect(text(bytes, at)).toBe(chunk.id)
      expect(movie.at + 8 + view.getUint32(at + 8, true)).toBe(chunk.at)
      expect(view.getUint32(at + 12, true)).toBe(chunk.size)
    }
    expect(await exportVisualAvi(pcm, 48000, settings, 32, 18)).toEqual(result)
  })

  it("rejects malformed, over-budget or cancelled inputs before publishing a file", async () => {
    await expect(exportVisualAvi([0], 48000, settings)).rejects.toThrow("stereo")
    await expect(exportVisualAvi([NaN, 0], 48000, settings)).rejects.toThrow("invalid")
    await expect(exportVisualAvi([0, 0], 48000, settings, 0, 1)).rejects.toThrow("limit")
    await expect(exportVisualAvi([0, 0], 48000, settings, 32, 18, () => true)).rejects.toThrow("cancelled")
    await expect(exportVisualAvi(new Float32Array(48000 * 31 * 2), 48000, settings)).rejects.toThrow("30-second")
    await expect(exportVisualAvi(new Float32Array(48000 * 30 * 2), 48000, settings, 640, 360)).rejects.toThrow("budget")
  })
})
