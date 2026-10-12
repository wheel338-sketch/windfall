import { afterEach, describe, expect, it, vi } from "vitest"
import { exportVisualAvi } from "@/features/visualizer/avi"
import { frameHash } from "@/features/visualizer/render"
import { readRawAvi } from "./avi"
afterEach(() => vi.unstubAllGlobals())
describe("prepared raw AVI picture reader", () => {
  it("decodes the repository AVI picture pixels and seeks by its exact stream rate", async () => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { callback(0); return 1 })
    const exported = await exportVisualAvi(new Float32Array(8000), 8000, { scene: "bars", intensity: 1, hue: 180 }, 16, 8)
    const avi = readRawAvi(exported.bytes.slice().buffer)!
    expect(avi.width).toBe(16); expect(avi.height).toBe(8)
    const pixels = new Uint8ClampedArray(16 * 8 * 4)
    for (const index of [0, 3, 1, 0]) {
      expect(avi.frameAt(index / exported.metadata.fps)).toBe(index)
      avi.drawFrame(index, pixels)
      expect(frameHash(pixels)).toBe(exported.metadata.frameHashes[index])
    }
    expect(avi.frameAt(-1)).toBe(-1)
    expect(avi.frameAt(avi.duration)).toBe(-1)
  })
  it("leaves other containers to the webview and refuses truncated AVI", () => {
    expect(readRawAvi(new ArrayBuffer(16))).toBeNull()
    const bytes = new Uint8Array(12)
    bytes.set(new TextEncoder().encode("RIFF")); bytes.set(new TextEncoder().encode("AVI "), 8)
    new DataView(bytes.buffer).setUint32(4, 1000, true)
    expect(() => readRawAvi(bytes.buffer)).toThrow("truncated")
  })
})
