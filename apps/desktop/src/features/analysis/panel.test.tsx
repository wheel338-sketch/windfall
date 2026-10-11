import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import type { MockBackend } from "@/lib/ipc/mock"
import { useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import { disabledReason, getAppState, registry, runAction } from "@/lib/actions"
import { registerAnalysisActions, useAnalysisDialog } from "./actions"
import { reserveRecovery, useAnalysisRecovery } from "./recovery"
import { useRecordingStore } from "@/features/transport/recording-store"
import { CommandPalette } from "@/features/palette/command-palette"
import userEvent from "@testing-library/user-event"
import { AnalysisButton, AnalysisPanel } from "./index"
import type { AnalysisJob, AnalysisModel, AnalysisReview } from "./types"
import { useRemixLoader } from "./remix"

const model: AnalysisModel = {
  id: "test-only-copy",
  version: "1",
  revision: "1",
  sha256: "07".repeat(32),
  bytes: "135",
  maxBytes: "4096",
  sampleRate: 48000,
  inputChannels: 2,
  maxInputFrames: "48000",
  outputs: [{ role: "copy", channels: 2 }],
  provenance: {
    origin: "authored:test",
    sourceRevision: "1",
    author: "fixture",
    licenseSpdx: "CC0-1.0",
    licenseReference: "fixture",
    adapterId: "test-only-copy",
    adapterVersion: "1",
    device: "cpu",
  },
}
const job: AnalysisJob = {
  job: "9007199254740993",
  ticket: "9007199254740994",
  request: "9007199254740995",
  sequence: "4",
  status: "ready",
  completedWork: "100",
  maximumWork: "1000",
  failure: null,
}
const review: AnalysisReview = {
  amount: 1,
  job,
  clip: 42,
  generation: "1",
  editRevision: "1",
  sourceSha256: "01".repeat(32),
  bindingSha256: "02".repeat(32),
  startFrame: "0",
  endFrame: "100",
  inputFrames: "100",
  model,
  artifacts: [
    {
      name: "copy.wav",
      role: "copy",
      frames: "100",
      channels: 2,
      sampleRate: 48000,
      frameOrigin: "0",
      bytes: "800",
      sha256: "03".repeat(32),
    },
  ],
}
let backend: MockBackend
let stop: () => void
let unregister: () => void
let clip: number
beforeEach(async () => {
  useAnalysisRecovery.setState(useAnalysisRecovery.getInitialState(), true)
  useRecordingStore.setState(useRecordingStore.getInitialState(), true)
  useRemixLoader.setState({ busy: false })
  ;({ backend, stop } = await startTestApp())
  unregister = registerAnalysisActions()
  await selectClip()
})
afterEach(() => {
  unregister()
  stop()
})
async function selectClip() {
  const result = await backend.addAudioClipFromFile(
    "/factory/Loops/Drum loop 128.wav",
    { start: 0 }
  )
  const id = result.created.at(-1)
  if (id === undefined) throw new Error("Fixture did not create an audio clip")
  clip = id
  usePlaylistStore.getState().select([clip])
}
async function nativePanel(pinned = model) {
  if (
    !useProjectStore
      .getState()
      .project.playlist.clips.some((item) => item.id === clip)
  )
    await selectClip()
  vi.spyOn(backend, "analysisCapability").mockResolvedValue({
    native: true,
    available: true,
    reason: null,
    models: [pinned],
  })
  vi.spyOn(backend, "analysisStatus").mockResolvedValue({
    ...job,
    status: "cancelled",
  })
  vi.spyOn(backend, "analysisCancel").mockResolvedValue({
    ...job,
    status: "cancelled",
  })
  vi.spyOn(backend, "analysisForget").mockResolvedValue()
  const view = render(<AnalysisPanel clip={clip} />)
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Submit analysis" })
    ).toBeDisabled()
  )
  await screen.findByRole("option", { name: /test-only-copy/ })
  fireEvent.change(screen.getByLabelText("End frame exclusive"), {
    target: { value: "100" },
  })
  return view
}
describe("analysis app controls", () => {
  it("drops the old dialog capture on actual project replacement and keeps cleanup recovery across inspector remount", async () => {
    vi.spyOn(backend, "analysisCapability").mockResolvedValue({
      native: true,
      available: true,
      reason: null,
      models: [model],
    })
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    vi.spyOn(backend, "analysisCancel").mockResolvedValue({
      ...job,
      status: "cancelled",
    })
    vi.spyOn(backend, "analysisStatus").mockResolvedValue({
      ...job,
      status: "cancelled",
    })
    vi.spyOn(backend, "analysisForget").mockRejectedValue(
      new Error("owned cleanup refused")
    )
    const inspector = render(<AnalysisButton />)
    await act(() => runAction("analysis.open"))
    await screen.findByRole("dialog", { name: "Native analysis" })
    await screen.findByRole("option", { name: /test-only-copy/ })
    fireEvent.change(screen.getByLabelText("End frame exclusive"), {
      target: { value: "100" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await screen.findByText(/Job 9007199254740993: ready/)
    const captured = useAnalysisDialog.getState().target
    expect(captured).not.toBeNull()
    await act(() => backend.projectNew())
    await selectClip()
    inspector.unmount()
    render(<AnalysisButton />)
    expect(useAnalysisDialog.getState().target).toBeNull()
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(
      disabledReason(registry.get("analysis.open")!, getAppState())
    ).toBeUndefined()
    await waitFor(() =>
      expect(backend.analysisForget).toHaveBeenCalledWith(job.job)
    )
    expect(useAnalysisRecovery.getState().jobs).toHaveLength(1)
    await act(() => runAction("analysis.open"))
    expect(useAnalysisDialog.getState().target?.generation).not.toBe(
      captured?.generation
    )
    expect(
      await screen.findByText(/Retained job 9007199254740993/)
    ).toBeVisible()
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Analysis job 9007199254740993 remains retained: Owned cleanup refused"
    )
    expect(
      screen.getByRole("button", { name: "Retry retained job cleanup" })
    ).toBeEnabled()
    expect(
      screen.queryByRole("button", { name: "Apply reviewed outputs" })
    ).toBeNull()
    expect(backend.analysisSubmit).toHaveBeenCalledTimes(1)
  })
  it("opens the canonical selected source through keymap and palette", async () => {
    const user = userEvent.setup()
    render(
      <>
        <AnalysisButton />
        <CommandPalette />
      </>
    )
    await user.keyboard("{Control>}{Shift>}a{/Shift}{/Control}")
    expect(
      await screen.findByRole("dialog", { name: "Native analysis" })
    ).toBeVisible()
    expect(await screen.findByRole("status")).toHaveTextContent(
      "No inference runs in the browser"
    )
    await user.keyboard("{Escape}")
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    await user.keyboard("{Control>}k{/Control}")
    const input = await screen.findByRole("combobox")
    await user.type(input, "analysis")
    await user.click(screen.getByRole("option", { name: /^Analysis/ }))
    expect(
      await screen.findByRole("dialog", { name: "Native analysis" })
    ).toBeVisible()
  })
  it("retires a late submit from a replaced dialog without restoring its source lifetime", async () => {
    let finish!: (next: AnalysisJob) => void
    vi.spyOn(backend, "analysisCapability").mockResolvedValue({
      native: true,
      available: true,
      reason: null,
      models: [model],
    })
    vi.spyOn(backend, "analysisSubmit").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve
        })
    )
    vi.spyOn(backend, "analysisCancel").mockResolvedValue({
      ...job,
      status: "cancelled",
    })
    vi.spyOn(backend, "analysisStatus").mockResolvedValue({
      ...job,
      status: "cancelled",
    })
    vi.spyOn(backend, "analysisForget").mockRejectedValue(
      new Error("owned cleanup refused")
    )
    render(<AnalysisButton />)
    await act(() => runAction("analysis.open"))
    await screen.findByRole("option", { name: /test-only-copy/ })
    fireEvent.change(screen.getByLabelText("End frame exclusive"), {
      target: { value: "100" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    expect(useAnalysisRecovery.getState().pending).toBe(1)
    await act(() => backend.projectNew())
    await selectClip()
    const replaced = useProjectStore.getState()
    expect(useAnalysisDialog.getState().target).toBeNull()
    expect(screen.queryByRole("dialog")).toBeNull()
    await act(async () => finish(job))
    await waitFor(() =>
      expect(backend.analysisForget).toHaveBeenCalledWith(job.job)
    )
    expect(useProjectStore.getState()).toBe(replaced)
    expect(useAnalysisRecovery.getState().pending).toBe(0)
    expect(useAnalysisRecovery.getState().jobs).toHaveLength(1)
    await act(() => runAction("analysis.open"))
    expect(
      await screen.findByText(/Retained job 9007199254740993/)
    ).toBeVisible()
    expect(screen.queryByText(/Job 9007199254740993: ready/)).toBeNull()
    expect(
      screen.queryByRole("button", { name: "Apply reviewed outputs" })
    ).toBeNull()
    expect(
      disabledReason(registry.get("analysis.apply")!, getAppState())
    ).toContain("Review")
  })
  it("invalidates registry enablement for input, recording, busy work and source edits", async () => {
    let finish!: (job: AnalysisJob) => void
    const submit = vi.spyOn(backend, "analysisSubmit").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve
        })
    )
    await nativePanel()
    const command = registry.get("analysis.submit")!
    expect(disabledReason(command, getAppState())).toBeUndefined()
    const version = registry.stateVersion()
    act(() =>
      useRecordingStore.setState((state) => ({
        state: { ...state.state, active: true },
      }))
    )
    expect(registry.stateVersion()).toBeGreaterThan(version)
    expect(
      screen.getByRole("button", { name: "Submit analysis" })
    ).toBeDisabled()
    expect(disabledReason(command, getAppState())).toContain("Finish recording")
    await runAction(command.id)
    expect(submit).not.toHaveBeenCalled()
    act(() =>
      useRecordingStore.setState((state) => ({
        state: { ...state.state, active: false },
      }))
    )
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    expect(disabledReason(command, getAppState())).toBe(
      "Analysis operation in progress"
    )
    expect(
      screen.getByRole("button", { name: "Cancel active analysis preparation" })
    ).toBeEnabled()
    await act(async () => finish(job))
    await act(() =>
      backend.dispatch({
        type: "updateAudioClips",
        updates: [{ id: clip, patch: { gain: 0.5 } }],
      })
    )
    expect(
      screen.getByRole("button", { name: "Review outputs" })
    ).toBeDisabled()
    expect(
      disabledReason(registry.get("analysis.review")!, getAppState())
    ).toContain("source changed")
    expect(screen.getByRole("button", { name: "Cancel job" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: "Cancel job" }))
    await screen.findByText(/Job 9007199254740993: cancelled/)
  })
  it("bounds retained and in-flight UI recovery before submitting", async () => {
    await nativePanel()
    let reservations: ReturnType<typeof reserveRecovery>[] = []
    act(() => {
      reservations = Array.from({ length: 8 }, () => reserveRecovery())
    })
    expect(
      screen.getByRole("button", { name: "Submit analysis" })
    ).toBeDisabled()
    expect(() => reserveRecovery()).toThrow("limit 8")
    expect(useAnalysisRecovery.getState().pending).toBe(8)
    act(() =>
      reservations.forEach((finish, index) =>
        finish({ ...job, job: String(index + 1) })
      )
    )
    expect(useAnalysisRecovery.getState().jobs).toHaveLength(8)
    expect(useAnalysisRecovery.getState().pending).toBe(0)
    expect(
      screen.getByRole("button", { name: "Submit analysis" })
    ).toBeDisabled()
  })
  it("keeps refused cleanup recoverable after dismissal, selection and actual project replacement", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {})
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    const view = await nativePanel()
    vi.mocked(backend.analysisForget).mockRejectedValue(
      new Error("owned cleanup refused")
    )
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await screen.findByText(/Job 9007199254740993: ready/)
    view.unmount()
    await waitFor(() =>
      expect(backend.analysisForget).toHaveBeenCalledWith(job.job)
    )
    await selectClip()
    const reopened = render(<AnalysisPanel clip={clip} />)
    expect(
      await screen.findByText(/Retained job 9007199254740993/)
    ).toBeVisible()
    reopened.unmount()
    await backend.projectNew()
    await selectClip()
    render(<AnalysisPanel clip={clip} />)
    expect(
      await screen.findByText(/Retained job 9007199254740993/)
    ).toBeVisible()
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Analysis job 9007199254740993 remains retained: Owned cleanup refused"
    )
    expect(
      screen.getByRole("button", { name: "Retry retained job cleanup" })
    ).toBeEnabled()
    expect(
      screen.queryByRole("button", { name: "Apply reviewed outputs" })
    ).toBeNull()
    vi.spyOn(backend, "analysisRetryCleanup").mockResolvedValue()
    fireEvent.click(
      screen.getByRole("button", { name: "Retry retained job cleanup" })
    )
    await waitFor(() =>
      expect(backend.analysisRetryCleanup).toHaveBeenCalledWith(job.job)
    )
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Forget retained job" })
      ).toBeEnabled()
    )
    vi.mocked(backend.analysisForget).mockResolvedValue()
    fireEvent.click(screen.getByRole("button", { name: "Forget retained job" }))
    await waitFor(() =>
      expect(screen.queryByText(/Retained job 9007199254740993/)).toBeNull()
    )
    expect(useAnalysisRecovery.getState().jobs).toHaveLength(0)
  })
  it("exposes every Analysis command through the canonical registry", () => {
    render(<AnalysisPanel clip={clip} />)
    expect(
      registry
        .list()
        .filter((action) => action.section === "Analysis")
        .map((action) => action.id)
    ).toEqual([
      "analysis.open",
      "analysis.chooseModel",
      "analysis.importModel",
      "analysis.submit",
      "analysis.cancel",
      "analysis.review",
      "analysis.retryCleanup",
      "analysis.forget",
      "analysis.apply",
      "analysis.cancelPreparation",
      "analysis.recoveryCancel",
      "analysis.recoveryRetry",
      "analysis.recoveryForget",
      "analysis.recoveryNext",
      "analysis.remix",
    ])
  })
  it("shows honest browser unavailability and preserves document/history", async () => {
    const before = useProjectStore.getState()
    render(<AnalysisPanel clip={clip} />)
    expect(await screen.findByRole("status")).toHaveTextContent(
      "No inference runs in the browser"
    )
    expect(
      screen.getByRole("button", { name: "Submit analysis" })
    ).toBeDisabled()
    expect(
      screen.getByRole("button", { name: "Verify and import local model" })
    ).toBeDisabled()
    expect(useProjectStore.getState()).toBe(before)
  })
  it("retires a late submit after unmount instead of retaining a hidden job", async () => {
    let resolve!: (value: AnalysisJob) => void
    const submit = vi.spyOn(backend, "analysisSubmit").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    const view = await nativePanel()
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    expect(submit).toHaveBeenCalledWith({
      clip,
      modelId: model.id,
      modelVersion: "1",
      modelRevision: "1",
      startFrame: "0",
      endFrame: "100",
    })
    view.unmount()
    await act(async () => resolve(job))
    expect(backend.analysisForget).toHaveBeenCalledWith(job.job)
  })
  it("discards a late review after the selected clip context changes", async () => {
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    let resolve!: (value: AnalysisReview) => void
    vi.spyOn(backend, "analysisReview").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    await nativePanel()
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await screen.findByText(/Job 9007199254740993: ready/)
    fireEvent.click(screen.getByRole("button", { name: "Review outputs" }))
    act(() => usePlaylistStore.setState({ selection: new Set([99]) }))
    await act(async () => resolve(review))
    expect(screen.queryByText(/Source SHA256:/)).toBeNull()
    expect(screen.getByRole("alert")).toHaveTextContent("selection changed")
  })
  it("shows provenance and ignores a late apply patch after actual project replacement", async () => {
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    vi.spyOn(backend, "analysisReview").mockImplementation(async () => ({
      ...review,
      clip,
    }))
    const result = await backend.addAudioClipFromFile(
      "/factory/Loops/Drum loop 128.wav",
      { start: 0 }
    )
    await backend.projectNew()
    let resolve!: (value: typeof result) => void
    vi.spyOn(backend, "analysisApply").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done
        })
    )
    await nativePanel()
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await screen.findByText(/Job 9007199254740993: ready/)
    fireEvent.click(screen.getByRole("button", { name: "Review outputs" }))
    await screen.findByText(/License: CC0-1.0/)
    fireEvent.click(
      screen.getByRole("button", { name: "Apply reviewed outputs" })
    )
    expect(backend.analysisApply).toHaveBeenCalledWith({
      ticket: job.ticket,
      request: job.request,
      replaceOriginal: false,
    })
    await act(() => backend.projectNew())
    const before = useProjectStore.getState()
    await act(async () => resolve(result))
    expect(useProjectStore.getState()).toBe(before)
  })
  it("reviews selected stems, auditions without applying, and requests one aligned remix import", async () => {
    const stemModel: AnalysisModel = { ...model, outputs: ["drums", "bass", "vocals", "other"]
      .map((role) => ({ role, channels: 2 })) }
    const stemReview: AnalysisReview = { ...review, task: "stemSeparation", model: stemModel,
      artifacts: stemModel.outputs.map((output, i) => ({ ...review.artifacts[0],
        name: `stem-${i}.wav`, role: output.role })) }
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    vi.spyOn(backend, "analysisReview").mockImplementation(async () => ({ ...stemReview, clip }))
    vi.spyOn(backend, "analysisPreview").mockResolvedValue()
    vi.spyOn(backend, "previewStop").mockResolvedValue()
    const result = await backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav", { start: 100 })
    vi.spyOn(backend, "analysisApply").mockResolvedValue(result)
    await nativePanel(stemModel)
    fireEvent.change(screen.getByLabelText("Analysis operation"), { target: { value: "stemSeparation" } })
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await screen.findByText(/Job 9007199254740993: ready/)
    expect(backend.analysisSubmit).toHaveBeenCalledWith(expect.objectContaining({ task: "stemSeparation" }))
    fireEvent.click(screen.getByRole("button", { name: "Review outputs" }))
    await screen.findByRole("button", { name: "Audition vocals" })
    const before = useProjectStore.getState()
    fireEvent.click(screen.getByRole("button", { name: "Audition vocals" }))
    await waitFor(() => expect(backend.analysisPreview).toHaveBeenCalledWith(job.ticket, "vocals"))
    expect(useProjectStore.getState()).toBe(before)
    for (const role of ["bass", "other"]) fireEvent.click(screen.getByLabelText(`Import ${role}`))
    fireEvent.click(screen.getByRole("button", { name: "Apply reviewed outputs" }))
    await waitFor(() => expect(backend.analysisApply).toHaveBeenCalledWith({
      ticket: job.ticket, request: job.request, replaceOriginal: false,
      selectedRoles: ["drums", "vocals"], separateTracks: true, muteOriginal: true,
    }))
  })
  it.each(["dereverbGeneral", "dereverbVocal"] as const)("submits the distinct %s mode only with matching model roles", async (task) => {
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    const role = task === "dereverbGeneral" ? "dereverb-general" : "dereverb-vocal"
    await nativePanel({ ...model, outputs: [{ role, channels: 2 }] })
    fireEvent.change(screen.getByLabelText("Analysis operation"), { target: { value: task } })
    expect(screen.getByRole("button", { name: "Submit analysis" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await waitFor(() => expect(backend.analysisSubmit).toHaveBeenCalledWith(expect.objectContaining({ task })))
  })
  it("refuses mismatched stem roles through the registry", async () => {
    const submit = vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    await nativePanel()
    fireEvent.change(screen.getByLabelText("Analysis operation"), { target: { value: "stemSeparation" } })
    expect(screen.getByRole("button", { name: "Submit analysis" })).toBeDisabled()
    expect(disabledReason(registry.get("analysis.submit")!, getAppState())).toContain("exact roles")
    await act(() => runAction("analysis.submit"))
    expect(submit).not.toHaveBeenCalled()
  })
  it.each(["dereverbGeneral", "dereverbVocal"] as const)("uses built-in %s without a pack and pins reviewed wet/dry", async (task) => {
    vi.spyOn(backend, "analysisCapability").mockResolvedValue({ native: true, available: true,
      classicalDereverb: true, reason: null, models: [] })
    vi.spyOn(backend, "analysisSubmit").mockResolvedValue(job)
    vi.spyOn(backend, "analysisStatus").mockResolvedValue(job)
    vi.spyOn(backend, "analysisCancel").mockResolvedValue({ ...job, status: "cancelled" })
    vi.spyOn(backend, "analysisForget").mockResolvedValue()
    const role = task === "dereverbVocal" ? "dereverb-vocal" : "dereverb-general"
    vi.spyOn(backend, "analysisReview").mockImplementation(async () => ({ ...review, clip, task, amount: 0.4,
      model: { ...model, id: "windfall-classical-dereverb", bytes: "0", outputs: [{ role, channels: 2 }] },
      artifacts: [{ ...review.artifacts[0], role }] }))
    vi.spyOn(backend, "analysisPreview").mockResolvedValue()
    render(<AnalysisPanel clip={clip} initialTask={task} />)
    await screen.findByRole("option", { name: /Built-in classical/ })
    fireEvent.change(screen.getByLabelText("End frame exclusive"), { target: { value: "100" } })
    fireEvent.change(screen.getByLabelText("Dereverb wet/dry amount"), { target: { value: "1.2" } })
    expect(screen.getByRole("button", { name: "Submit analysis" })).toBeDisabled()
    fireEvent.change(screen.getByLabelText("Dereverb wet/dry amount"), { target: { value: "0.4" } })
    fireEvent.click(screen.getByRole("button", { name: "Submit analysis" }))
    await waitFor(() => expect(backend.analysisSubmit).toHaveBeenCalledWith(expect.objectContaining({
      task, amount: 0.4, modelId: "windfall-classical-dereverb", modelVersion: "1", modelRevision: "1" })))
    await screen.findByText(/Job 9007199254740993: ready/)
    fireEvent.click(screen.getByRole("button", { name: "Review outputs" }))
    await screen.findByText(/No model weights/)
    expect(screen.getByRole("button", { name: "Apply reviewed outputs" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: `Audition ${role}` }))
    await waitFor(() => expect(backend.analysisPreview).toHaveBeenCalledWith(job.ticket, role))
  })
  it("loads a song from the remix action and opens the honest stem wizard", async () => {
    vi.spyOn(backend, "pickAudioFile").mockResolvedValue("/factory/Loops/Drum loop 128.wav")
    render(<AnalysisButton />)
    await act(() => runAction("analysis.remix"))
    expect(await screen.findByRole("dialog", { name: "Native analysis" })).toBeVisible()
    expect(screen.getByRole("region", { name: "Remix song workflow" })).toHaveTextContent("Song loaded")
    expect(screen.getByLabelText("Analysis operation")).toHaveValue("stemSeparation")
    expect(usePlaylistStore.getState().selection.size).toBe(1)
    expect(screen.getByRole("button", { name: "Submit analysis" })).toBeDisabled()
    expect(useRemixLoader.getState().busy).toBe(false)
  })
  it("does not import a song chosen after project replacement", async () => {
    let finish!: (path: string | null) => void
    vi.spyOn(backend, "pickAudioFile").mockImplementation(() => new Promise((done) => { finish = done }))
    const add = vi.spyOn(backend, "addAudioClipFromFile")
    render(<AnalysisButton />)
    const loading = runAction("analysis.remix")
    expect(useRemixLoader.getState().busy).toBe(true)
    await act(() => backend.projectNew())
    await act(async () => { finish("/factory/Loops/Drum loop 128.wav"); await loading })
    expect(add).not.toHaveBeenCalled()
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(useRemixLoader.getState().busy).toBe(false)
  })
})
