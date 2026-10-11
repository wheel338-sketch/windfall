import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import { AnalysisTools } from "./analysis-tools"
import type { AudioAnalysisPreview } from "./analysis-types"

let backend: MockBackend
let stop: () => void
let review: AudioAnalysisPreview
beforeEach(async () => {
  ;({ backend, stop } = await startTestApp())
  const result = await backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav", { start: 0 })
  const id = result.created.at(-1)!
  usePlaylistStore.getState().select([id])
  review = { token: 7, recipe: {
    sourceClip: useProjectStore.getState().project.playlist.clips.find((c) => c.id === id)!,
    analysis: { frames: 16000, sampleRate: 16000, polyphonic: false,
      notes: [{ startFrame: 2000, endFrame: 10000, midiPitch: 69, confidence: 0.95, rms: 0.1 }],
      transients: [2000] },
    overrides: [], markers: [{ sourceFrame: 0, targetFrame: 0 }, { sourceFrame: 16000, targetFrame: 16000 }], denoise: null,
  } }
  vi.spyOn(backend, "previewStop").mockResolvedValue()
  vi.spyOn(backend, "audioAnalysisCancel").mockResolvedValue()
})
afterEach(() => stop())
async function mount() {
  vi.spyOn(backend, "audioAnalysisDetect").mockResolvedValue(review)
  const onCommit = vi.fn(), onBusy = vi.fn()
  const view = render(<AnalysisTools preview={{ token: 7, clip: review.recipe.sourceClip.id,
    name: "Take", frames: 16000, sampleRate: 16000, peaks: [-0.2, 0.2] }}
    selection={{ start: 0, end: 2000 }} disabled={false} onBusy={onBusy} onCommit={onCommit} />)
  fireEvent.click(screen.getByRole("button", { name: "Detect monophonic notes" }))
  await screen.findByRole("button", { name: "Note 1 pitch 69.00" })
  return { ...view, onCommit, onBusy }
}
describe("classical audio editors (written, not executed)", () => {
  it("keeps estimates independent from keyboard overrides, draft undo and audition payload", async () => {
    vi.spyOn(backend, "audioAnalysisAudition").mockResolvedValue()
    await mount()
    const note = screen.getByRole("button", { name: "Note 1 pitch 69.00" })
    fireEvent.keyDown(note, { key: "ArrowUp" })
    expect(screen.getByLabelText("Target MIDI pitch")).toHaveValue(70)
    expect(screen.getByText(/estimate 69.00 → 70.00/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Audition edited audio" }))
    await waitFor(() => expect(backend.audioAnalysisAudition).toHaveBeenCalledWith({
      token: 7, recipe: { ...review.recipe, overrides: [{ note: 0, midiPitch: 70, startFrame: 2000, endFrame: 10000 }] },
    }, false))
    fireEvent.click(screen.getByRole("button", { name: "Undo draft edit" }))
    expect(screen.getByLabelText("Target MIDI pitch")).toHaveValue(69)
  })
  it("refuses crossing warp markers and learns a noise-only profile for wet preview", async () => {
    vi.spyOn(backend, "audioAnalysisAudition").mockResolvedValue()
    await mount()
    fireEvent.click(screen.getByRole("tab", { name: "Time and warp" }))
    fireEvent.change(screen.getByLabelText("Output duration (frames)"), { target: { value: "1" } })
    expect(screen.getByRole("alert")).toHaveTextContent(/ordered|ratio/)
    expect(screen.getByLabelText("Output duration (frames)")).toHaveValue(16000)
    fireEvent.click(screen.getByRole("tab", { name: "Noise profile" }))
    fireEvent.click(screen.getByRole("button", { name: "Learn noise profile from selection" }))
    fireEvent.change(screen.getByLabelText("Denoise wet mix"), { target: { value: "0.5" } })
    fireEvent.click(screen.getByRole("button", { name: "Audition edited audio" }))
    await waitFor(() => expect(backend.audioAnalysisAudition).toHaveBeenCalledWith({
      token: 7, recipe: { ...review.recipe, denoise: { profileStart: 0, profileEnd: 2000, reductionDb: 24, wet: 0.5 } },
    }, false))
  })
  it("inserts only selected reviewed notes and forwards the native one-step result", async () => {
    const result = await backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav", { start: 100 })
    vi.spyOn(backend, "audioAnalysisSendMidi").mockResolvedValue(result)
    const { onCommit } = await mount()
    fireEvent.click(screen.getByRole("button", { name: "Insert reviewed MIDI score" }))
    await waitFor(() => expect(onCommit).toHaveBeenCalledWith(result))
    expect(backend.audioAnalysisSendMidi).toHaveBeenCalledWith(review, [0])
  })
  it("ignores late detection after project replacement", async () => {
    let resolve!: (v: AudioAnalysisPreview) => void
    vi.spyOn(backend, "audioAnalysisDetect").mockImplementation(() => new Promise((done) => { resolve = done }))
    render(<AnalysisTools preview={{ token: 7, clip: review.recipe.sourceClip.id, name: "Take",
      frames: 16000, sampleRate: 16000, peaks: [] }} selection={{ start: 0, end: 2000 }}
      disabled={false} onBusy={vi.fn()} onCommit={vi.fn()} />)
    fireEvent.click(screen.getByRole("button", { name: "Detect monophonic notes" }))
    await act(() => backend.projectNew())
    await act(async () => resolve(review))
    expect(screen.queryByRole("group", { name: "Detected note pitch editor" })).toBeNull()
    expect(screen.getByRole("alert")).toHaveTextContent("Project or source changed")
    expect(backend.audioAnalysisCancel).toHaveBeenCalled()
  })
  it("uses preserved source frames when a reopened derived clip has a different duration", async () => {
    review.sourcePeaks = [-0.3, 0.3]
    review.recipe.markers[1].targetFrame = 24000
    vi.spyOn(backend, "audioAnalysisDetect").mockResolvedValue(review)
    vi.spyOn(backend, "audioAnalysisAudition").mockResolvedValue()
    render(<AnalysisTools preview={{ token: 7, clip: review.recipe.sourceClip.id, name: "Edited take",
      frames: 24000, sampleRate: 16000, peaks: [-0.1, 0.1] }}
      selection={{ start: 18000, end: 22000 }} disabled={false} onBusy={vi.fn()} onCommit={vi.fn()} />)
    fireEvent.click(screen.getByRole("button", { name: "Detect monophonic notes" }))
    await screen.findByRole("group", { name: "Detected note pitch editor" })
    expect(screen.getByLabelText("Analysis source selection end")).toHaveValue(16000)
    fireEvent.change(screen.getByLabelText("Analysis source selection start"), { target: { value: "0" } })
    fireEvent.change(screen.getByLabelText("Analysis source selection end"), { target: { value: "2000" } })
    fireEvent.click(screen.getByRole("tab", { name: "Noise profile" }))
    fireEvent.click(screen.getByRole("button", { name: "Learn noise profile from selection" }))
    fireEvent.click(screen.getByRole("button", { name: "Audition edited audio" }))
    await waitFor(() => expect(backend.audioAnalysisAudition).toHaveBeenCalledWith({
      token: 7, recipe: { ...review.recipe, denoise: { profileStart: 0, profileEnd: 2000, reductionDb: 24, wet: 1 } },
    }, false))
  })
  it("shows explicit native-only refusal in the browser backend", async () => {
    render(<AnalysisTools preview={{ token: 7, clip: review.recipe.sourceClip.id, name: "Take",
      frames: 16000, sampleRate: 16000, peaks: [] }} selection={{ start: 0, end: 2000 }}
      disabled={false} onBusy={vi.fn()} onCommit={vi.fn()} />)
    fireEvent.click(screen.getByRole("button", { name: "Detect monophonic notes" }))
    expect(await screen.findByRole("alert")).toHaveTextContent(/desktop|native|available/i)
    expect(screen.queryByRole("group", { name: "Detected note pitch editor" })).toBeNull()
  })
})
