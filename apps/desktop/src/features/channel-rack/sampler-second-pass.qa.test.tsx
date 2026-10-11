import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { SimDocument } from "@/lib/ipc/sim/document"
import { redo, undo } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"
import { ChannelInspector } from "./inspector"
import { channel, history, project, samplerOf, startRack } from "./test-utils"

let app: Awaited<ReturnType<typeof startRack>>
beforeEach(async () => {
  app = await startRack()
  useUiStore.getState().selectChannel(channel("Kick").id)
})
afterEach(() => { app.stop(); vi.restoreAllMocks() })
function reopen() {
  const doc = SimDocument.create(project())
  const loaded = SimDocument.open(doc.fileText())
  const value = loaded.project()
  doc.dispose(); loaded.dispose()
  return value
}

it("groups every filter-envelope and target-LFO knob drag into one saved undo step", async () => {
  render(<ChannelInspector />)
  await settle()
  await userEvent.click(screen.getByRole("switch", { name: "Enable filter envelope" }))
  await userEvent.click(screen.getByRole("switch", { name: "Enable volume LFO" }))
  for (const name of ["Filter envelope attack", "Volume LFO depth"]) {
    const before = structuredClone(channel("Kick").voice)
    const cursor = history().cursor
    const knob = screen.getByRole("slider", { name })
    fireEvent.pointerDown(knob, { button: 0, pointerId: 41, clientY: 200 })
    for (const clientY of [190, 180, 160, 140]) {
      fireEvent.pointerMove(knob, { pointerId: 41, clientY })
      await act(settle)
    }
    fireEvent.pointerUp(knob, { pointerId: 41, clientY: 140 })
    await act(settle)
    const changed = structuredClone(channel("Kick").voice)
    expect(changed).not.toEqual(before)
    expect(history().cursor).toBe(cursor + 1)
    expect(reopen().channels.find((item) => item.id === channel("Kick").id)?.voice).toEqual(changed)
    await act(async () => { await undo() })
    expect(channel("Kick").voice).toEqual(before)
    await act(async () => { await redo() })
    expect(channel("Kick").voice).toEqual(changed)
  }
})

it("clicks Fit to tempo then publishes its exact spectral ratio in one persisted edit", async () => {
  vi.spyOn(app.backend, "sampleInfoById").mockResolvedValue({
    path: "fixture.wav", name: "Two seconds", sampleRate: 48000,
    channels: 2, frames: 96000, durationSecs: 2, peaks: [-0.5, 0.5],
  })
  vi.spyOn(app.backend, "samplerPreparationBegin").mockResolvedValue(71)
  const prepare = vi.spyOn(app.backend, "prepareSamplerCommand").mockImplementation((command) => app.backend.dispatch(command))
  render(<ChannelInspector />)
  await settle()
  await userEvent.click(screen.getByRole("button", { name: "Independent" }))
  fireEvent.change(screen.getByRole("spinbutton", { name: "Beats to fit" }), { target: { value: "2" } })
  const cursor = history().cursor
  await userEvent.click(screen.getByRole("button", { name: "Fit to tempo" }))
  expect(history().cursor).toBe(cursor)
  await userEvent.click(screen.getByRole("button", { name: "Apply stretch" }))
  await waitFor(() => expect(prepare).toHaveBeenCalledOnce())
  await waitFor(() => expect(samplerOf("Kick").stretch?.mode).toBe("spectral"))
  const stretch = samplerOf("Kick").stretch
  expect(stretch).toMatchObject({ mode: "spectral", ratio: 2 * 60 / project().settings.tempoBpm / 2 })
  expect(history().cursor).toBe(cursor + 1)
  expect(reopen().channels[0].source).toEqual(channel("Kick").source)
  await act(async () => { await undo() })
  expect(samplerOf("Kick").stretch?.mode ?? "tape").toBe("tape")
  await act(async () => { await redo() })
  expect(samplerOf("Kick").stretch).toEqual(stretch)
})

it("auditions a prepared result without changing the WASM document and exposes conservative cleanup", async () => {
  const preview = vi.spyOn(app.backend, "samplerPreprocessPreview").mockResolvedValue({
    path: "", name: "Prepared", frames: 1600, sampleRate: 48000, channels: 2,
    durationSecs: 1600 / 48000, peaks: [-0.8912509, 0.8912509],
  })
  const cleanup = vi.spyOn(app.backend, "collectDerivedAssets").mockResolvedValue(2)
  render(<ChannelInspector />)
  await settle()
  const before = structuredClone(project())
  const cursor = history().cursor
  await userEvent.click(screen.getByRole("checkbox", { name: "Normalize to −1 dBFS" }))
  await userEvent.click(screen.getByRole("button", { name: "Preview prepared sample" }))
  await waitFor(() => expect(preview).toHaveBeenCalledWith(expect.objectContaining({ channel: channel("Kick").id, normalize: true })))
  expect(screen.getByLabelText("Prepared sample duration")).toHaveTextContent("1600 frames")
  expect(screen.getByRole("img", { name: "Prepared sample waveform" })).toBeVisible()
  expect(project()).toEqual(before)
  expect(history().cursor).toBe(cursor)
  await userEvent.click(screen.getByRole("button", { name: "Collect unused sample edits" }))
  await waitFor(() => expect(cleanup).toHaveBeenCalledOnce())
  expect(screen.getByLabelText("Collected sample edits")).toHaveTextContent("Removed 2 unused files")
  expect(history().cursor).toBe(cursor)
})
