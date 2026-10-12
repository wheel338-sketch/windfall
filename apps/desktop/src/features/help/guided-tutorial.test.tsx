import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { starterProject } from "@/lib/ipc/sim/project"
import { registerChannelRackActions } from "@/features/channel-rack/actions"
import { TEST_DIALOGS, startTestApp } from "@/test/harness"
import { GuidedTutorial } from "./guided-tutorial"
import { baseline, TUTORIALS, type TutorialEvidence } from "./tutorials"
import { ContextHelp } from "./context-help"
import { Overlays } from "@/features/layout/app-shell"
import { PanelFrame } from "@/features/layout/panel-frame"
import { closeGettingStarted } from "./getting-started-store"
import { openGettingStarted } from "./getting-started-store"
import { GettingStartedDialog } from "./getting-started-dialog"
import userEvent from "@testing-library/user-event"
import type { MockBackend } from "@/lib/ipc/mock"
import type { ExportRequestProgress } from "@/lib/ipc/backend"
import { ExportDialog } from "@/features/export/export-dialog"
import { useExportRequestStore } from "@/features/export/requests"
import { useRecordingStore } from "@/features/transport/recording-store"
let backend: MockBackend; let stop: () => void
beforeEach(async () => {
  const app = await startTestApp({ project: starterProject(), dialogs: { ...TEST_DIALOGS, audioFile: async () => null } })
  backend = app.backend
  useExportRequestStore.setState({ intent: null, request: null })
  useRecordingStore.setState(useRecordingStore.getInitialState(), true)
  const unregister = registerChannelRackActions()
  stop = () => { unregister(); app.stop() }
})
afterEach(() => { closeGettingStarted(); stop(); vi.restoreAllMocks() })
describe("real-state guided tutorials", () => {
  it("explicitly discards an active failed capture before retrying device setup", async () => {
    const active = { active: true, frames: 64, sampleRate: 48000, startTick: 0, error: "Input failed. Stop to discard this take." }
    const idle = { ...active, active: false, frames: 0, error: null }
    const state = vi.spyOn(backend, "recordingState").mockResolvedValue(active)
    const cancelled = vi.spyOn(backend, "recordingCancel").mockImplementation(async () => { state.mockResolvedValue(idle) })
    const inputs = [{ host: "Fixture", device: "Replacement", channels: 2, sampleRates: [48000] }]
    vi.spyOn(backend, "recordingInputs").mockResolvedValue(inputs)
    useRecordingStore.setState({ open: true, inputs, state: active })
    const before = useProjectStore.getState()
    render(<TooltipProvider><GuidedTutorial kind="record" /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Next" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    await user.click(screen.getByRole("button", { name: "Discard failed take and retry setup" }))
    expect(cancelled).toHaveBeenCalledOnce()
    expect(useRecordingStore.getState().state.active).toBe(false)
    expect(screen.getByRole("button", { name: "Record audio input" })).toBeVisible()
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled()
    expect(useProjectStore.getState()).toBe(before)
  })
  it("walks through the real export dialog, ignores foreign completion, and recovers from cancellation and errors", async () => {
    const listeners = new Set<(event: ExportRequestProgress) => void>()
    vi.spyOn(backend, "onExportProgress").mockImplementation((listener) => { listeners.add(listener); return () => { listeners.delete(listener) } })
    const exported = vi.spyOn(backend, "exportAudio").mockResolvedValue(undefined)
    render(<TooltipProvider><GuidedTutorial kind="export" /><ExportDialog /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Export" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    const submit = async () => {
      const dialog = within(screen.getByRole("dialog", { name: "Export audio" }))
      await user.click(dialog.getByRole("button", { name: "Choose…" }))
      await user.click(dialog.getByRole("button", { name: "Export" }))
      const [options, request] = exported.mock.calls.at(-1)!
      expect(request).toMatch(/^[0-9a-f-]{36}$/)
      return { path: options.path, request, fraction: 1, done: true, error: null, droppedClips: 0 }
    }
    const first = await submit()
    const emit = (event: ExportRequestProgress) => act(() => { for (const listener of listeners) listener(event) })
    emit({ ...first, request: "another-window" })
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
    emit({ ...first, cancelled: true })
    expect(screen.getByText(/Only this guide's successful request counts/)).toBeVisible()
    const retry = await submit()
    expect(retry.request).not.toBe(first.request)
    emit({ ...retry, error: "Disk full" })
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
    emit(first)
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
    const final = await submit()
    emit(final)
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Next" }))
    expect(screen.getByRole("status")).toHaveTextContent("Completed: Export audio")
  })
  it("recording setup recovers after device loss and a focus refresh, then finishes on a kept document clip", async () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
    const state = { active: false, frames: 0, sampleRate: 48000, startTick: 0, error: null }
    const recordingState = vi.spyOn(backend, "recordingState").mockResolvedValue(state)
    const inputs = vi.spyOn(backend, "recordingInputs").mockResolvedValue([{ host: "Fixture", device: "Input", channels: 2, sampleRates: [48000] }])
    vi.spyOn(backend, "recordingStart").mockImplementation(async () => {
      const active = { ...state, active: true, frames: 64 }
      recordingState.mockResolvedValue(active)
      return active
    })
    vi.spyOn(backend, "recordingStop").mockImplementation(async () => {
      const sample = useProjectStore.getState().project.samples[0].id
      const result = await backend.addAudioClipFromSample(sample, { start: 0 })
      recordingState.mockResolvedValue(state)
      return result
    })
    const mounted = render(<TooltipProvider><GuidedTutorial kind="record" /></TooltipProvider>)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Record audio input" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    await user.click(screen.getByRole("button", { name: "Start recording" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    recordingState.mockResolvedValue({ ...state, error: "Input failed. Stop to discard this take." })
    await act(async () => { window.dispatchEvent(new Event("focus")) })
    expect(screen.getByText("Input failed. Stop to discard this take.")).toBeVisible()
    await user.click(screen.getByRole("button", { name: "Retry recording setup" }))
    expect(inputs).toHaveBeenCalledTimes(2)
    recordingState.mockResolvedValue(state)
    await user.click(screen.getByRole("button", { name: "Record audio input" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    await user.click(screen.getByRole("button", { name: "Start recording" }))
    await user.click(screen.getByRole("button", { name: "Next" }))
    await user.click(screen.getByRole("button", { name: "Stop recording and keep take" }))
    expect(useProjectStore.getState().project.playlist.clips.some((clip) => clip.content.type === "audio")).toBe(true)
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Next" }))
    expect(screen.getByRole("status")).toHaveTextContent("Completed: Record audio")
    mounted.unmount()
    const calls = recordingState.mock.calls.length
    window.dispatchEvent(new Event("focus"))
    expect(recordingState).toHaveBeenCalledTimes(calls)
  })
  it("keeps help open while focusing the application and provides a bundled manual", async () => {
    render(<TooltipProvider><button>Song editor control</button><GettingStartedDialog /></TooltipProvider>)
    act(() => openGettingStarted())
    const user = userEvent.setup()
    const editor = screen.getByRole("button", { name: "Song editor control" })
    await user.click(editor)
    expect(editor).toHaveFocus()
    expect(screen.getByRole("dialog", { name: "Getting started" })).toBeVisible()
    expect(document.querySelector('[data-slot="dialog-overlay"]')).toBeNull()
    await user.click(screen.getByText("Read manual"))
    expect(screen.getByRole("article", { name: "Windfall manual" })).toBeVisible()
    expect(screen.getByText("Patterns and songs")).toBeVisible()
  })
  it("opens real contextual help from a detached header using the shared window overlays", async () => {
    render(<TooltipProvider>
      <PanelFrame title="Mixer" panel="mixer" detached><p>Detached mixer</p></PanelFrame>
      <Overlays />
    </TooltipProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Help" }))
    expect(screen.getByRole("dialog", { name: "Getting started" })).toBeVisible()
    expect(screen.getByText(/Each channel feeds a mixer track/)).toBeVisible()
    await act(async () => { await backend.projectNew() })
    expect(screen.queryByRole("dialog", { name: "Getting started" })).toBeNull()
  })
  it("keeps Next disabled until real notes exist, then places an undoable pattern clip", async () => {
    render(<TooltipProvider><GuidedTutorial kind="beat" /></TooltipProvider>)
    // The existing playable factory kit satisfies the sound-channel step.
    fireEvent.click(screen.getByRole("button", { name: "Next" }))
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
    const project = useProjectStore.getState().project
    await act(async () => { await dispatch({ type: "toggleStep", pattern: project.patterns[0].id, channel: project.channels[0].id, step: 0 }) })
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: "Next" }))
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open the playlist" })))
    const clip = useProjectStore.getState().project.playlist.clips[0]
    expect(clip.content).toEqual({ type: "pattern", pattern: project.patterns[0].id })
    expect(useUiStore.getState().centerTab).toBe("playlist")
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled()
    const patch = await backend.undo()
    expect(patch?.playlist?.clips).toEqual([])
  })
  it("an import picker cancellation cannot advance the mounted guide", async () => {
    render(<TooltipProvider><GuidedTutorial kind="import" /></TooltipProvider>)
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Import audio" })))
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled()
  })
  it("recording and export gates require real state and successful completion", () => {
    const project = useProjectStore.getState().project
    const evidence: TutorialEvidence = { project, before: baseline(project), playing: false, song: false, recording: false, inputs: 0, recordingOpen: false, exportOpen: false, exported: false, dirty: true }
    expect(TUTORIALS.record.steps.every((step) => !step.complete(evidence))).toBe(true)
    expect(TUTORIALS.export.steps.every((step) => !step.complete(evidence))).toBe(true)
    expect(TUTORIALS.record.steps[0].complete({ ...evidence, recordingOpen: true, inputs: 1 })).toBe(true)
    expect(TUTORIALS.record.steps[1].complete({ ...evidence, recording: true })).toBe(true)
    expect(TUTORIALS.export.steps[1].complete({ ...evidence, exported: true })).toBe(true)
  })
  it("contextual help follows the real active panel and supports a detached-panel override", () => {
    act(() => useUiStore.getState().setActiveScope("playlist"))
    const { rerender } = render(<ContextHelp />)
    expect(screen.getByText(/Switch the transport to Song/)).toBeVisible()
    rerender(<ContextHelp panel="mixer" />)
    expect(screen.getByText(/Each channel feeds a mixer track/)).toBeVisible()
  })
})
