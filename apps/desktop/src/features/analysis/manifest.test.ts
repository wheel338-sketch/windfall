import { describe, expect, it } from "vitest"
import { parseManifest } from "./manifest"

const manifest = {
  id: "authored-onnx-contract", version: "1", revision: "1", sha256: "01".repeat(32),
  bytes: "32", maxBytes: "64", sampleRate: 44100, inputChannels: 2, maxInputFrames: "88200",
  provenance: { origin: "authored", sourceRevision: "1", author: "Windfall", licenseSpdx: "CC0-1.0",
    licenseReference: "authored fixture", adapterId: "future-onnx", adapterVersion: "1", device: "cpu" },
  outputs: ["drums", "bass", "vocals", "other"].map((role) => ({ role, channels: 2 })),
  onnx: { opset: 17, inputs: [{ name: "audio", shape: [1, 2, "frames"] }],
    outputs: [{ name: "stems", shape: [1, 4, 2, "frames"] }], sampleRate: 44100,
    segmentFrames: 88200, overlapFrames: 22050, stemRoles: ["drums", "bass", "vocals", "other"] },
}
describe("imported ONNX pack contract", () => {
  it("retains explicit tensor names, symbolic shapes, segment policy and stem order without executable extras", () => {
    expect(parseManifest(JSON.stringify({ ...manifest, executable: "ignored" }))).toEqual(manifest)
    expect(parseManifest(JSON.stringify({ ...manifest, onnx: null })).onnx).toBeUndefined()
  })
  it.each([0, -1, 1.5, "", "unbounded shape", 16777217])("rejects invalid tensor dimension %s", (dimension) => {
    const onnx = { ...manifest.onnx, inputs: [{ name: "audio", shape: [1, 2, dimension] }] }
    expect(() => parseManifest(JSON.stringify({ ...manifest, onnx }))).toThrow(/dimension|symbolic/)
  })
  it("refuses overlap at segment length and unbounded tensor collections", () => {
    expect(() => parseManifest(JSON.stringify({ ...manifest, onnx: { ...manifest.onnx, overlapFrames: 88200 } }))).toThrow(/segment/)
    expect(() => parseManifest(JSON.stringify({ ...manifest, onnx: { ...manifest.onnx, outputs: Array.from({ length: 17 }, () => ({ name: "output", shape: [1] })) } }))).toThrow(/tensors/)
  })
  it("validates production waveform layout, required context and hostile stem metadata", () => {
    const pack = { ...manifest, provenance: { ...manifest.provenance, adapterId: "windfall-onnx-separation" },
      onnx: { ...manifest.onnx, contextFrames: 2048, layout: "channelsFirst" } }
    expect(parseManifest(JSON.stringify(pack)).onnx).toMatchObject({ contextFrames: 2048, layout: "channelsFirst" })
    for (const onnx of [
      { ...pack.onnx, contextFrames: 2147483647 }, { ...pack.onnx, overlapFrames: 88199 },
      { ...pack.onnx, layout: "mask" }, { ...pack.onnx, stemRoles: ["drums", "bass", "vocals", "bass"] },
      { ...pack.onnx, outputs: [{ name: "stems", shape: [1, 8, 2, "frames"] }] },
      { ...pack.onnx, inputs: [{ name: "audio", shape: ["batch", 2, "frames"] }] },
    ]) expect(() => parseManifest(JSON.stringify({ ...pack, onnx }))).toThrow()
    expect(() => parseManifest(JSON.stringify({ ...pack, onnx: undefined }))).toThrow(/Separation/)
    const last = { ...pack, onnx: { ...pack.onnx, layout: "channelsLast",
      inputs: [{ name: "audio", shape: [1, "frames", 2] }], outputs: [{ name: "stems", shape: [1, 4, "frames", 2] }] } }
    expect(parseManifest(JSON.stringify(last)).onnx?.layout).toBe("channelsLast")
  })
})
