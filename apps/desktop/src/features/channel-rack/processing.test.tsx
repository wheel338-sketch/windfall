import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { Backend } from "@/lib/ipc"
import {
  dispatch,
  loadSnapshot,
  useProjectStore,
  undo,
} from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"
import { ChannelInspector } from "./inspector"
import { channel, history, samplerOf, startRack } from "./test-utils"

let backend: Backend
let stop: () => void
beforeEach(async () => {
  ;({ backend, stop } = await startRack())
  useUiStore.getState().selectChannel(channel("Kick").id)
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})
async function open() {
  render(<ChannelInspector />)
  await settle()
}
async function independent() {
  await userEvent.click(screen.getByRole("button", { name: "Independent" }))
}

it("shows the browser DSP limit and refuses Apply without changing settings or history", async () => {
  await open()
  const before = useProjectStore.getState().project
  const cursor = history().cursor
  await independent()
  expect(
    screen.getByText(/Browser mode cannot prepare or audition/)
  ).toBeVisible()
  expect(
    screen.getByRole("spinbutton", { name: "First prepared MIDI key" })
  ).toHaveValue(54)
  expect(
    screen.getByRole("spinbutton", { name: "Last prepared MIDI key" })
  ).toHaveValue(65)
  await userEvent.click(screen.getByRole("button", { name: "Apply stretch" }))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    /desktop audio engine/
  )
  expect(useProjectStore.getState().project).toBe(before)
  expect(history().cursor).toBe(cursor)
})

it("validates range boundaries before starting preparation", async () => {
  await open()
  await independent()
  const begin = vi.spyOn(backend, "samplerPreparationBegin")
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "First prepared MIDI key" }),
    { target: { value: "127" } }
  )
  await userEvent.click(screen.getByRole("button", { name: "Apply stretch" }))
  expect(screen.getByRole("alert")).toHaveTextContent("ordered MIDI key range")
  expect(begin).not.toHaveBeenCalled()
})

it("reports unavailable audition and export for spectral settings loaded in browser mode", async () => {
  await act(async () => {
    await dispatch({
      type: "updateSampler",
      id: channel("Kick").id,
      patch: {
        stretch: {
          mode: "spectral",
          ratio: 1,
          quality: "standard",
          formants: false,
          range: { first: 60, last: 61 },
        },
      },
    })
  })
  await open()
  const before = useProjectStore.getState().project
  const cursor = history().cursor
  expect(screen.getByText(/Other keys are silent/)).toBeVisible()
  await expect(
    backend.auditionNoteOn(channel("Kick").id, 60, 1)
  ).rejects.toThrow("desktop audio engine")
  await expect(
    backend.exportAudio({
      path: "mock-sampler.wav",
      format: "wav",
      bitDepth: "float32",
      sampleRate: 48_000,
      mode: "pattern",
      patternLoops: 1,
      tailSecs: 0,
      autoTail: false,
    })
  ).rejects.toThrow("desktop audio engine")
  expect(useProjectStore.getState().project).toBe(before)
  expect(history().cursor).toBe(cursor)
})

it("applies native preparation once and undo restores tape", async () => {
  await open()
  vi.spyOn(backend, "samplerPreparationBegin").mockResolvedValue(41)
  vi.spyOn(backend, "prepareSamplerCommand").mockImplementation((command) =>
    backend.dispatch(command)
  )
  await independent()
  const cursor = history().cursor
  await userEvent.click(screen.getByRole("button", { name: "Apply stretch" }))
  await waitFor(() => expect(samplerOf("Kick").stretch?.mode).toBe("spectral"))
  expect(history().cursor).toBe(cursor + 1)
  expect(screen.getByText(/Other keys are silent/)).toBeVisible()
  await act(async () => {
    await undo()
  })
  expect(samplerOf("Kick").stretch?.mode ?? "tape").toBe("tape")
})

it.each(["cancel", "channel", "project"])(
  "cancels pending native work on %s and ignores its late failure",
  async (change) => {
    await open()
    vi.spyOn(backend, "samplerPreparationBegin").mockResolvedValue(42)
    let reject: (reason: Error) => void = () => {}
    vi.spyOn(backend, "prepareSamplerCommand").mockImplementation(
      () =>
        new Promise((_, fail) => {
          reject = fail
        })
    )
    const cancel = vi.spyOn(backend, "samplerPreparationCancel")
    await independent()
    const cursor = history().cursor
    await userEvent.click(screen.getByRole("button", { name: "Apply stretch" }))
    expect(screen.getByRole("status", { name: "Sampler preparation" })).toHaveTextContent(
      "Preparing key variants"
    )
    if (change === "cancel")
      await userEvent.click(
        screen.getByRole("button", { name: "Cancel preparation" })
      )
    if (change === "channel")
      act(() => useUiStore.getState().selectChannel(channel("Snare").id))
    if (change === "project")
      act(() =>
        loadSnapshot({
          ...useProjectStore.getState(),
          project: { ...useProjectStore.getState().project },
        })
      )
    await waitFor(() => expect(cancel).toHaveBeenCalledWith(42))
    await act(async () => {
      reject(new Error("obsolete preparation"))
      await settle()
    })
    expect(screen.queryByText("obsolete preparation")).toBeNull()
    expect(screen.queryByRole("status", { name: "Sampler preparation" })).toBeNull()
    expect(history().cursor).toBe(cursor)
  }
)
