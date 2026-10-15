import { fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { Backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { RecordingDialog } from "./recording-dialog"
import { openRecording, useRecordingStore } from "./recording-store"
let backend: Backend
let cleanup: () => void
beforeEach(async () => {
  const app = await startTestApp()
  backend = app.backend
  cleanup = app.stop
  useRecordingStore.setState(useRecordingStore.getInitialState(), true)
  useRecordingStore.setState({
    open: false,
    busy: false,
    inputs: [],
    device: 0,
    left: 0,
    right: -1,
    start: 960,
    track: -1,
    error: "",
    state: {
      active: false,
      frames: 0,
      sampleRate: 0,
      startTick: 0,
      error: null,
    },
  })
})
afterEach(() => cleanup())
it("selects native channels and shows capture errors without losing the discard control", async () => {
  vi.spyOn(backend, "recordingInputs").mockResolvedValue([
    { host: "Fake", device: "Synthetic", channels: 2, sampleRates: [48000] },
  ])
  const state = {
    active: false,
    frames: 0,
    sampleRate: 48000,
    startTick: 960,
    error: null as string | null,
  }
  vi.spyOn(backend, "recordingState").mockImplementation(async () => ({
    ...state,
  }))
  const start = vi
    .spyOn(backend, "recordingStart")
    .mockImplementation(async () => {
      state.active = true
      return { ...state }
    })
  const cancel = vi
    .spyOn(backend, "recordingCancel")
    .mockImplementation(async () => {
      state.active = false
    })
  render(<RecordingDialog />)
  await openRecording()
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Start recording" }))
  expect(start).toHaveBeenCalledWith(
    {
      host: "Fake",
      device: "Synthetic",
      left: 0,
      right: null,
      alignment: {
        synchronize: true,
        driftCorrection: true,
        offsetMs: 0,
        inputSampleRate: null,
      },
      loopRecording: undefined,
      monitor: undefined,
      armedTracks: false,
    },
    960,
    null
  )
  expect(screen.getByRole("button", { name: "Start recording" })).toBeDisabled()
  useRecordingStore.setState({
    state: { ...state, error: "Synthetic input dropout" },
  })
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Synthetic input dropout"
  )
  await user.click(screen.getByRole("button", { name: "Discard" }))
  expect(cancel).toHaveBeenCalledOnce()
})
it("explains that browser preview cannot capture audio", async () => {
  render(<RecordingDialog />)
  await openRecording()
  expect(screen.getByRole("button", { name: "Start recording" })).toBeDisabled()
  expect(
    screen.getByText(
      /No audio inputs found\. Recording needs the native desktop app/
    )
  ).toBeVisible()
  expect(
    screen.getByText(
      /Driver timestamps supply automatic input\/output latency alignment/
    )
  ).toBeVisible()
})
it("takes the playlist start as a song position and keeps the record controls out of the scrolling setup", async () => {
  const { project } = useProjectStore.getState()
  useProjectStore.setState({
    project: {
      ...project,
      settings: {
        ...project.settings,
        timeSignature: { numerator: 4, denominator: 4 },
      },
      playlist: { ...project.playlist, timeline: undefined },
    },
  })
  render(<RecordingDialog />)
  await openRecording()
  const start = screen.getByLabelText("Playlist start position")
  expect(start).toHaveValue("1:2")
  fireEvent.change(start, { target: { value: "3:1" } })
  expect(useRecordingStore.getState().start).toBe(7680)
  fireEvent.change(start, { target: { value: "3:9" } })
  expect(start).toHaveAttribute("aria-invalid", "true")
  expect(useRecordingStore.getState().start).toBe(7680)
  fireEvent.blur(start)
  expect(start).toHaveValue("3:1")
  const setup = start.closest(".overflow-y-auto")
  expect(setup).not.toBeNull()
  expect(setup).not.toContainElement(
    screen.getByRole("button", { name: "Start recording" })
  )
  expect(setup).not.toContainElement(screen.getByRole("status"))
})
