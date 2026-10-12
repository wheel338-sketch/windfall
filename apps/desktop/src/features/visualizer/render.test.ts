import { describe, expect, it } from "vitest"
import { characterPose } from "./character"
import { offlineSignal } from "./offline"
import { frameHash, renderVisual, visualFrameNumber } from "./render"

function sine() {
  return Float64Array.from({ length: 2048 }, (_, i) => 0.25 * Math.sin(2 * Math.PI * 16 * Math.floor(i / 2) / 1024))
}
function referenceHash(bytes: Uint8ClampedArray) {
  let value = 2166136261n
  for (const byte of bytes) value = ((value ^ BigInt(byte)) * 16777619n) & 0xffffffffn
  return value.toString(16).padStart(8, "0")
}

describe("deterministic Windfall scene frames", () => {
  it("measures a bin-centred audio fixture against analytic RMS and Parseval energy", () => {
    const pcm = sine()
    expect(offlineSignal(pcm, 1023)).toBeNull()
    const signal = offlineSignal(pcm, 1024)!
    expect(signal.rms).toBeCloseTo(0.25 / Math.SQRT2, 7)
    expect(signal.bands.reduce((a, b) => a + b, 0)).toBeCloseTo(0.03125, 8)
    // Hann spreads bin 16 into bins 15..17, which cross groups 1 and 2.
    expect(signal.bands[1]).toBeCloseTo(0.03125 / 6, 8)
    expect(signal.bands[2]).toBeCloseTo(0.03125 * 5 / 6, 8)
    expect(() => offlineSignal(pcm, 2048)).toThrow("beyond")
    pcm[0] = NaN
    expect(() => offlineSignal(pcm, 1024)).toThrow("invalid")
  })

  it("matches independently specified bar pixels and checksums", () => {
    const bands = new Array<number>(64).fill(0); bands[0] = 1 / 9
    const actual = renderVisual(32, 4, { scene: "bars", hue: 0, intensity: 1 }, { rms: 0, bands }, 0)
    const expected = new Uint8ClampedArray(32 * 4 * 4)
    for (let y = 0; y < 4; y += 1) for (let x = 0; x < 32; x += 1) {
      const at = (y * 32 + x) * 4
      expected.set([x === 0 ? Math.round(255 * (1 - y / 8)) : 8, 12, 20, 255], at)
    }
    expect(actual).toEqual(expected)
    expect(frameHash(actual)).toBe(referenceHash(expected))
    expect(renderVisual(32, 4, { scene: "bars", hue: 360, intensity: 1 }, { rms: 0, bands }, 0)).toEqual(expected)
  })

  it("renders every authored scene reproducibly from deterministic audio", () => {
    const signal = offlineSignal(sine(), 1024)
    const hashes = ["orbit", "bars", "ribbon"].map((scene) => {
      if (scene !== "orbit" && scene !== "bars" && scene !== "ribbon") throw new Error("fixture scene")
      const settings = { scene, hue: 180, intensity: 2 } satisfies import("@/bindings").VisualSettings
      const pixels = renderVisual(80, 45, settings, signal, 6)
      expect(frameHash(pixels)).toBe(referenceHash(pixels))
      expect(renderVisual(80, 45, settings, offlineSignal(sine(), 1024), 6)).toEqual(pixels)
      return frameHash(pixels)
    })
    expect(new Set(hashes).size).toBe(3)
    expect(visualFrameNumber("9007199254740993", 48000)).toBe(Number((9007199254740993n * 24n / 48000n) % 1000000n))
  })

  it("maps real note keys and velocities to newly authored character poses", () => {
    expect(characterPose(null, 6).name).toBe("rest")
    expect([60, 61, 62, 63].map((key) => characterPose({ key, velocity: 1 }, 6).name)).toEqual(["step", "wave", "jump", "lean"])
    expect(characterPose({ key: 62, velocity: 1 }, 6).lift).toBe(-18)
    expect(characterPose({ key: 62, velocity: 0 }, 6).lift).toBe(-0)
    expect(characterPose({ key: 60, velocity: 0.5 }, 6).leftLeg).toBe(12.5)
  })
})
