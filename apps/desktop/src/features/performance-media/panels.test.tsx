import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { RealtimeFrame } from "@/bindings"
import { resetAnalyzerFeed } from "@/features/analyzers/feed"
import type { AnalyzerEvent, AnalyzerFrame, AnalyzerSelect, AnalyzerStatus } from "@/features/analyzers/types"
import { PerformancePanel } from "@/features/performance/grid"
import { VideoPanel } from "@/features/video/panel"
import { VideoPlayer } from "@/features/video/player"
import { NoteCharacter } from "@/features/visualizer/character"
import { VisualizerPanel } from "@/features/visualizer/panel"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced, getProjectGeneration } from "@/lib/store/replaced"
import { startTestApp } from "@/test/harness"
import { changeStageMedia, DEFAULT_STAGE_MEDIA, mediaDocumentGuard, useMediaWindows } from "./store"

let backend: MockBackend
let stop: () => void
const status = (select: AnalyzerSelect): AnalyzerStatus => ({ slot: select.slot, request: select.request, phase: "preparing", reason: null, ticket: null, source: select.source, effect: null, sourceName: null, sampleRate: null })
function analyzerFrame(request: number): AnalyzerFrame {
  const level = { valid: true, peak: 0.25, rms: 0.1767767, peakDb: -12, rmsDb: -15, silent: false, clipped: 0, invalid: 0 }
  return {
    type: "frame", slot: "visualizer", request, sequence: "1",
    ticket: { service: "1", subscription: "1", stream: "1", selection: "1", project: "0" },
    clock: { epoch: "1", firstFrame: "0", endFrame: "1024", alignedFirstFrame: "0", sampleRate: 48000, pdcFrames: 0, deviceLatencyFrames: null, timeLabel: "00:00:00.021" },
    gap: false, levels: [level, level], stereo: { correlation: 1, midRms: level.rms, sideRms: 0 },
    spectrum: { binHz: 46.875, enbwBins: 1.5, left: [], right: [], bands: new Array<number>(64).fill(0.001) },
    history: [], scope: null, vector: null, envelope: null, reductionDb: null, observations: [],
    counters: { droppedFrames: "0", invalidSamples: "0", resets: "0", analyzedWindows: "1", droppedWindows: "0", droppedObservations: "0", missingSourceBlocks: "0", skippedEvents: "0" },
    playing: true, tick: 40,
  }
}
beforeEach(async () => {
  resetAnalyzerFeed()
  useMediaWindows.setState({ performance: false, video: false, visualizer: false })
  ;({ backend, stop } = await startTestApp())
})
afterEach(() => {
  useMediaWindows.setState({ performance: false, video: false, visualizer: false })
  resetAnalyzerFeed(); stop(); vi.restoreAllMocks(); vi.unstubAllGlobals()
})

describe("mounted performance and media editors", () => {
  it("launches a saved pad and a scene through current document tokens, including keyboard activation", async () => {
    const pattern = useProjectStore.getState().project.patterns[0].id
    await act(async () => { await changeStageMedia((media) => ({ ...media, pads: [{ track: 0, scene: 0, pattern }] })) })
    vi.spyOn(backend, "performanceState").mockResolvedValue({ available: true, tick: 2, droppedNotes: "0", tracks: Array.from({ length: 32 }, () => ({ active: null, queued: null, queuedStop: false })) })
    vi.spyOn(backend, "performancePad").mockResolvedValue()
    vi.spyOn(backend, "performanceScene").mockResolvedValue()
    // A reloaded window can be on cancellation counter 0 while native is 55.
    vi.spyOn(backend, "timelineState").mockResolvedValue({ generation: 55, revision: useProjectStore.getState().revision, request: 0, region: null, navigationOverflows: 0 })
    render(<PerformancePanel />)
    act(() => useMediaWindows.setState({ performance: true }))
    const user = userEvent.setup()
    const pad = await screen.findByRole("button", { name: "Launch track 1 scene 1" })
    await user.click(pad)
    const revision = useProjectStore.getState().revision
    expect(backend.performancePad).toHaveBeenCalledWith(0, 0, 55, revision)
    pad.focus(); await user.keyboard("{Enter}")
    expect(backend.performancePad).toHaveBeenCalledTimes(2)
    await user.click(screen.getByRole("button", { name: "Scene 1" }))
    expect(backend.performanceScene).toHaveBeenCalledWith(0, 55, revision)
    await user.click(screen.getByRole("button", { name: "Stop 1" }))
    expect(backend.performancePad).toHaveBeenLastCalledWith(0, null, 55, revision)
    act(() => announceProjectReplaced())
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Performance grid" })).toBeNull())
  })

  it("reports unavailable launch audio instead of presenting mock playback as live", async () => {
    render(<PerformancePanel />)
    act(() => useMediaWindows.setState({ performance: true }))
    expect(await screen.findByText(/Launching needs the desktop audio engine/)).toBeVisible()
    expect(screen.getByRole("button", { name: "Launch track 1 scene 1" })).toBeDisabled()
  })

  it("selects the native analyzer spectrum and persists scene parameters through checked commands", async () => {
    const stopped = vi.fn()
    let deliver!: (event: AnalyzerEvent) => void
    vi.spyOn(backend, "analyzerSubscribe").mockImplementation((slot, handler) => {
      deliver = handler
      return { status: Promise.resolve({ slot, request: 0, phase: "idle", reason: null, ticket: null, source: null, effect: null, sourceName: null, sampleRate: null }), stop: stopped }
    })
    vi.spyOn(backend, "analyzerSelect").mockImplementation((select) => Promise.resolve(status(select)))
    vi.spyOn(backend, "analyzerAck").mockImplementation(() => {})
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
    render(<VisualizerPanel />)
    act(() => useMediaWindows.setState({ visualizer: true }))
    await waitFor(() => expect(backend.analyzerSelect).toHaveBeenCalled())
    expect(vi.mocked(backend.analyzerSelect).mock.calls.at(-1)?.[0]).toMatchObject({ slot: "visualizer", source: { type: "mixOutput" }, want: { spectrum: true, scope: false, vector: false, envelope: false } })
    expect(screen.getByRole("img", { name: "Audio reactive scene" })).toHaveAttribute("data-live", "false")
    const selection = vi.mocked(backend.analyzerSelect).mock.calls.at(-1)![0]
    act(() => deliver(analyzerFrame(selection.request)))
    await waitFor(() => expect(screen.getByRole("img", { name: "Audio reactive scene" })).toHaveAttribute("data-live", "true"))
    expect(backend.analyzerAck).toHaveBeenCalledWith("visualizer", "1")
    act(() => deliver({ type: "status", ...status(selection), phase: "stalled", reason: "audioStalled" }))
    await waitFor(() => expect(screen.getByRole("img", { name: "Audio reactive scene" })).toHaveAttribute("data-live", "false"))
    const user = userEvent.setup()
    const hue = screen.getByRole("spinbutton", { name: "Hue" })
    await user.clear(hue); await user.type(hue, "240"); await user.tab()
    await waitFor(() => expect(useProjectStore.getState().project.settings.stageMedia?.visual.hue).toBe(240))
    expect(screen.getByRole("button", { name: "Export synchronized AVI" })).toBeDisabled()
    act(() => useMediaWindows.setState({ visualizer: false }))
    await waitFor(() => expect(stopped).toHaveBeenCalledTimes(1))
  })

  it("edits mute and source offset and removes saved timeline video with undo", async () => {
    await act(async () => { await changeStageMedia((media) => ({ ...media, videos: [{ path: "C:/clip.webm", startTick: 0, lengthTicks: 3840, offsetSeconds: 0, muted: true }] })) })
    vi.spyOn(backend, "mediaVideoRead").mockRejectedValue(new Error("Decoder unavailable in harness"))
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {})
    render(<VideoPanel />); act(() => useMediaWindows.setState({ video: true }))
    const user = userEvent.setup()
    await user.click(await screen.findByRole("switch", { name: "Mute video 1" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.stageMedia?.videos[0].muted).toBe(false))
    const offset = screen.getByRole("spinbutton", { name: "Video 1 source offset seconds" })
    await user.clear(offset); await user.type(offset, "1.25"); await user.tab()
    await waitFor(() => expect(useProjectStore.getState().project.settings.stageMedia?.videos[0].offsetSeconds).toBe(1.25))
    await user.click(screen.getByRole("button", { name: "Remove video 1" }))
    await waitFor(() => expect(screen.getByText("No timeline video")).toBeVisible())
    await act(async () => { await backend.undo() })
    await waitFor(() => expect(useProjectStore.getState().project.settings.stageMedia?.videos[0].offsetSeconds).toBe(1.25))
  })

  it("drops a media read that completes after unmount without allocating a Blob URL", async () => {
    let resolve!: (bytes: ArrayBuffer) => void
    vi.spyOn(backend, "mediaVideoRead").mockImplementation(() => new Promise((done) => { resolve = done }))
    const create = vi.fn(() => "blob:fixture")
    vi.stubGlobal("URL", class extends URL { static createObjectURL = create; static revokeObjectURL = vi.fn() })
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {})
    const view = render(<VideoPlayer placement={{ path: "C:/clip.webm", startTick: 0, lengthTicks: 3840, offsetSeconds: 0, muted: true }} />)
    await waitFor(() => expect(backend.mediaVideoRead).toHaveBeenCalled())
    view.unmount()
    await act(async () => resolve(new ArrayBuffer(8)))
    expect(create).not.toHaveBeenCalled()
  })

  it("refuses media work when the native revision or window lifetime changes during guard acquisition", async () => {
    const revision = useProjectStore.getState().revision
    vi.spyOn(backend, "timelineState").mockResolvedValue({ generation: 55, revision: revision + 1, request: 0, region: null, navigationOverflows: 0 })
    await expect(mediaDocumentGuard()).rejects.toThrow("changed")
    const generation = getProjectGeneration()
    act(() => announceProjectReplaced())
    await expect(mediaDocumentGuard(generation, revision)).rejects.toThrow("changed")
  })

  it("animates the selected channel from the real note-pose feed and clears on replacement", async () => {
    let receive!: (frame: RealtimeFrame) => void
    vi.spyOn(backend, "subscribeRealtime").mockImplementation((handler) => { receive = handler; return () => {} })
    const channel = useProjectStore.getState().project.channels[0].id
    render(<NoteCharacter channel={channel} />)
    const frame: RealtimeFrame = { playing: false, tick: 0, meters: [], cpu: 0, xruns: 0, voices: 1, gainReductions: [], automated: [], audioClips: 0, droppedClips: 0, activeNotePoses: [{ channel, key: 62, velocity: 1 }], mediaClock: { epoch: "1", outputFrame: "12000", sampleRate: 48000, pdcFrames: 0, deviceLatencyFrames: null, playing: false, mode: "song", tick: 0 } }
    act(() => receive(frame))
    await waitFor(() => expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", "jump"))
    act(() => announceProjectReplaced())
    await waitFor(() => expect(document.querySelector("[data-pose]")).toHaveAttribute("data-pose", "rest"))
    expect(DEFAULT_STAGE_MEDIA.danceChannel).toBeNull()
  })
})
