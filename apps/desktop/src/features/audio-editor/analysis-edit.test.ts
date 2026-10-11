import { describe, expect, it } from "vitest"
import { mappedFrame, recipeProblem } from "./analysis-edit"
import type { AudioEditRecipe } from "./analysis-types"
import type { Clip } from "@/bindings"

function recipe(): AudioEditRecipe {
  return { sourceClip: {} as Clip, analysis: { frames: 16000, sampleRate: 16000, polyphonic: false,
    notes: [], transients: [] }, markers: [{ sourceFrame: 0, targetFrame: 0 },
      { sourceFrame: 8000, targetFrame: 12000 }, { sourceFrame: 16000, targetFrame: 24000 }],
    overrides: [], denoise: null }
}
describe("analysis edit constraints", () => {
  it("maps frames independently of playback seconds and rejects crossing or excessive ratios", () => {
    const r = recipe()
    expect(mappedFrame(r, 4000)).toBe(6000)
    expect(recipeProblem(r)).toBeNull()
    r.markers[1].targetFrame = 25000
    expect(recipeProblem(r)).toMatch(/ordered/)
    r.markers[1].targetFrame = 1
    expect(recipeProblem(r)).toMatch(/ratio/)
  })
})
