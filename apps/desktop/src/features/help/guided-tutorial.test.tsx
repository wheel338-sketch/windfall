import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
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
import type { MockBackend } from "@/lib/ipc/mock"
let backend: MockBackend; let stop: () => void
beforeEach(async () => {
  const app = await startTestApp({ project: starterProject(), dialogs: { ...TEST_DIALOGS, audioFile: async () => null } })
  backend = app.backend
  const unregister = registerChannelRackActions()
  stop = () => { unregister(); app.stop() }
})
afterEach(() => { closeGettingStarted(); stop() })
describe("real-state guided tutorials", () => {
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
