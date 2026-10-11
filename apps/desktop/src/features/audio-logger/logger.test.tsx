import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { Backend } from "@/lib/ipc"
import { runAction } from "@/lib/actions"
import { startTestApp } from "@/test/harness"
import {
  AudioLoggerDialog,
  formatMemory,
  formatSeconds,
} from "./logger-dialog"
import { openAudioLogger, useAudioLoggerStore } from "./logger-store"
import {
  NATIVE_ONLY,
  sameSource,
  sourceKey,
  type AudioLoggerSettings,
  type AudioLoggerState,
} from "./types"

let backend: Backend
let cleanup: () => void

function nativeState(
  settings: Partial<AudioLoggerSettings> = {}
): AudioLoggerState {
  return {
    settings: {
      enabled: true,
      seconds: 120,
      master: true,
      tracks: [],
      input: null,
      ...settings,
    },
    running: true,
    sources: [
      {
        source: { type: "master" },
        label: "Master output",
        sampleRate: 48000,
        capacitySeconds: 120,
        availableSeconds: 42,
        availableFrames: 42 * 48000,
        droppedFrames: 0,
        memoryBytes: 46 * 1024 * 1024,
        active: true,
        error: null,
      },
    ],
    memoryBytes: 46 * 1024 * 1024,
    maxSourceBytes: 256 * 1024 * 1024,
    minSeconds: 30,
    maxSeconds: 600,
    maxTracks: 4,
    error: null,
  }
}

beforeEach(async () => {
  const app = await startTestApp()
  backend = app.backend
  cleanup = app.stop
  useAudioLoggerStore.setState(useAudioLoggerStore.getInitialState(), true)
})
afterEach(() => cleanup())

it("names seconds and memory the way the dialog shows them", () => {
  expect(formatSeconds(42)).toBe("42 s")
  expect(formatSeconds(120)).toBe("2 min")
  expect(formatSeconds(150)).toBe("2 min 30 s")
  expect(formatMemory(46 * 1024 * 1024)).toBe("46.0 MB")
})

it("tells sources apart by kind and by track", () => {
  expect(sameSource({ type: "master" }, { type: "master" })).toBe(true)
  expect(sameSource({ type: "master" }, { type: "input" })).toBe(false)
  expect(
    sameSource({ type: "track", track: 3 }, { type: "track", track: 4 })
  ).toBe(false)
  expect(sourceKey({ type: "track", track: 3 })).toBe("track-3")
  expect(sourceKey({ type: "input" })).toBe("input")
})

it("says plainly that the browser preview keeps no audio history", async () => {
  render(<AudioLoggerDialog />)
  await openAudioLogger()
  expect(await screen.findByRole("alert")).toHaveTextContent(NATIVE_ONLY)
  expect(screen.getByText("The audio logger is off.")).toBeInTheDocument()
  await expect(
    backend.audioLoggerDump({
      source: { type: "master" },
      seconds: null,
      start: null,
      track: null,
    })
  ).rejects.toThrow(NATIVE_ONLY)
  await expect(
    backend.audioLoggerConfigure(nativeState().settings)
  ).rejects.toThrow(NATIVE_ONLY)
})

it("opens from the command registry and shows what each source holds", async () => {
  vi.spyOn(backend, "audioLoggerState").mockResolvedValue(nativeState())
  render(<AudioLoggerDialog />)
  await runAction("audioLogger.open")
  const sources = await screen.findByRole("list", { name: "Logged sources" })
  const row = within(sources).getByText("Master output").closest("li")
  expect(row).toHaveTextContent("Logging")
  expect(row).toHaveTextContent("42 s of 2 min at 48000 Hz")
  expect(row).toHaveTextContent("46.0 MB")
  expect(screen.getByRole("checkbox", { name: /Keep audio/ })).toBeChecked()
})

it("sends the whole settings with only the changed field different", async () => {
  vi.spyOn(backend, "audioLoggerState").mockResolvedValue(nativeState())
  const configure = vi
    .spyOn(backend, "audioLoggerConfigure")
    .mockImplementation(async (settings) => nativeState(settings))
  render(<AudioLoggerDialog />)
  await openAudioLogger()
  await userEvent.click(
    await screen.findByRole("checkbox", { name: "Master output" })
  )
  expect(configure).toHaveBeenLastCalledWith({
    enabled: true,
    seconds: 120,
    master: false,
    tracks: [],
    input: null,
  })
  await userEvent.click(screen.getByRole("checkbox", { name: /Keep audio/ }))
  expect(configure).toHaveBeenLastCalledWith({
    enabled: false,
    seconds: 120,
    master: false,
    tracks: [],
    input: null,
  })
})

it("adds the chosen trailing length of a source to the playlist", async () => {
  vi.spyOn(backend, "audioLoggerState").mockResolvedValue(nativeState())
  const dump = vi.spyOn(backend, "audioLoggerDump").mockResolvedValue({
    created: [],
    patch: { revision: 0, dirty: false },
  } as never)
  render(<AudioLoggerDialog />)
  await openAudioLogger()
  await userEvent.click(
    await screen.findByRole("button", {
      name: "Add logged Master output to playlist",
    })
  )
  expect(dump).toHaveBeenLastCalledWith({
    source: { type: "master" },
    seconds: 30,
    start: null,
    track: null,
  })
  useAudioLoggerStore.setState({ dumpSeconds: 0 })
  await userEvent.click(
    screen.getByRole("button", { name: "Add logged Master output to playlist" })
  )
  expect(dump).toHaveBeenLastCalledWith({
    source: { type: "master" },
    seconds: null,
    start: null,
    track: null,
  })
})

it("shows a native refusal and a source's own error, and keeps the controls", async () => {
  const state = nativeState()
  state.sources[0].error = "The audio input was disconnected."
  vi.spyOn(backend, "audioLoggerState").mockResolvedValue(state)
  vi.spyOn(backend, "audioLoggerDump").mockRejectedValue(
    new Error("Stop or cancel recording first.")
  )
  render(<AudioLoggerDialog />)
  await openAudioLogger()
  expect(
    await screen.findByText("The audio input was disconnected.")
  ).toBeInTheDocument()
  await userEvent.click(
    screen.getByRole("button", { name: "Add logged Master output to playlist" })
  )
  expect(
    await screen.findByText("Stop or cancel recording first.")
  ).toBeInTheDocument()
  expect(
    screen.getByRole("button", { name: "Add logged Master output to playlist" })
  ).toBeEnabled()
})

it("does not offer to add a history that holds nothing", async () => {
  const state = nativeState()
  state.sources[0].availableFrames = 0
  state.sources[0].availableSeconds = 0
  vi.spyOn(backend, "audioLoggerState").mockResolvedValue(state)
  render(<AudioLoggerDialog />)
  await openAudioLogger()
  expect(
    await screen.findByRole("button", {
      name: "Add logged Master output to playlist",
    })
  ).toBeDisabled()
})
