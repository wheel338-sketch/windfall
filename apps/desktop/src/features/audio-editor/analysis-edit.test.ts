import { describe, expect, it } from "vitest"
import { mappedFrame, noteEdit, recipeProblem, renderMarkers } from "./analysis-edit"
import type { AudioEditRecipe } from "./analysis-types"

function recipe(): AudioEditRecipe {
  return { sourceClip: { id: 8, track: 7, start: 0, length: 1920, offset: 0, muted: false,
    content: { type: "audio", sample: 6, mixerTrack: 0, gain: 1, pan: 0, fadeIn: 0,
      fadeOut: 0, reverse: false, pitch: 0 } },
    analysis: { frames: 16000, sampleRate: 16000, polyphonic: false,
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
  it("maps untouched notes through edited edges and refuses conflicting adjacent overrides", () => {
    const r = recipe()
    r.analysis.notes = [2000, 6000].map((startFrame) => ({ startFrame, endFrame: startFrame + 4000,
      midiPitch: 69, confidence: 1, rms: 0.1 }))
    r.overrides = [{ note: 0, midiPitch: 70, startFrame: 4000, endFrame: 10000 }]
    expect(recipeProblem(r)).toBeNull()
    expect(noteEdit(r, 1).startFrame).toBe(10000)
    expect(renderMarkers(r).find((m) => m.sourceFrame === 6000)?.targetFrame).toBe(10000)
    r.overrides.push({ note: 1, midiPitch: 69, startFrame: 11000, endFrame: 16000 })
    expect(recipeProblem(r)).toMatch(/shared boundary/)
    r.overrides[1].startFrame = 10000
    expect(recipeProblem(r)).toBeNull()
    r.overrides[0].endFrame = 14000
    expect(recipeProblem(r)).not.toBeNull()
  })
})
