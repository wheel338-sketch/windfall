import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { setBackend } from "@/lib/ipc"
import { createMockBackend } from "@/lib/ipc/mock"
import { undo, useProjectStore } from "@/lib/store/project"
import { startTestApp, settle } from "@/test/harness"
import { MidiRecordingControls } from "./recording-controls"
import { SimDocument } from "@/lib/ipc/sim/document"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => { app = await startTestApp() })
afterEach(() => { vi.restoreAllMocks(); app.stop() })
it("logs browser audition, mounts retained dump/history/save-reopen and refuses replaced generations", async () => {
  let now = Date.now()
  vi.spyOn(Date, "now").mockImplementation(() => now)
  const channel = useProjectStore.getState().project.channels[0]!.id
  await app.backend.auditionNoteOn(channel, 110, 0.8)
  now += 250
  await app.backend.auditionNoteOff(channel, 110)
  const state = await app.backend.controllerState()
  expect(state.scoreLog.notes).toBe(1)
  const user = userEvent.setup()
  render(<MidiRecordingControls />)
  await user.click(screen.getByRole("button", { name: "Dump note log" }))
  await waitFor(() => expect(useProjectStore.getState().history.cursor).toBe(1))
  const recorded = structuredClone(useProjectStore.getState().project)
  expect(recorded.patterns[0]!.lanes.flatMap((lane) => lane.notes).some((note) => note.key === 110)).toBe(true)
  const document = SimDocument.create(recorded)
  const reopened = SimDocument.open(document.fileText())
  expect(reopened.project()).toEqual(recorded)
  document.dispose(); reopened.dispose()
  await act(async () => { await undo(); await settle() })
  expect((await app.backend.controllerState()).scoreLog.notes).toBe(1)
  await app.backend.projectNew()
  await expect(app.backend.scoreLogDump(recorded.patterns[0]!.id, null, state.generation)).rejects.toThrow(/changed/)
  expect((await app.backend.controllerState()).scoreLog.notes).toBe(0)
})
it("restores the bounded browser note log from storage across backend restart", async () => {
  const data = new Map<string, string>()
  const storage = { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value) } }
  const project = structuredClone(useProjectStore.getState().project)
  const channel = project.channels[0]!.id
  let now = Date.now()
  vi.spyOn(Date, "now").mockImplementation(() => now)
  const first = createMockBackend({ project, storage })
  await first.auditionNoteOn(channel, 110, 0.5)
  now += 125
  await first.auditionNoteOff(channel, 110)
  first.dispose()
  const second = createMockBackend({ project, storage })
  setBackend(second)
  expect((await second.controllerState()).scoreLog.notes).toBe(1)
  const state = await second.controllerState()
  await second.scoreLogDump(project.patterns[0]!.id, null, state.generation)
  expect((await second.documentSnapshot()).history.cursor).toBe(1)
  expect((await second.controllerState()).scoreLog.notes).toBe(1)
  now += 30 * 60 * 1000 + 1
  expect((await second.controllerState()).scoreLog.notes).toBe(0)
  second.dispose()
  setBackend(app.backend)
})

it("surfaces browser log persistence failures in the mounted transport status", async () => {
  const project = structuredClone(useProjectStore.getState().project)
  const backend = createMockBackend({ project, storage: { getItem: () => null, setItem: () => { throw new Error("quota") } } })
  setBackend(backend)
  const channel = project.channels[0]!.id
  await backend.auditionNoteOn(channel, 110, 0.5)
  await backend.auditionNoteOff(channel, 110)
  render(<MidiRecordingControls />)
  await screen.findByText("The note log could not be saved in browser storage.")
  expect((await backend.controllerState()).scoreLog.notes).toBe(1)
  backend.dispose(); setBackend(app.backend)
})

it("mounts native-capability dump replies into the real document and retains native log state through history", async () => {
  const channel = useProjectStore.getState().project.channels[0]!.id
  await app.backend.auditionNoteOn(channel, 111, 0.75)
  await app.backend.auditionNoteOff(channel, 111)
  const state = await app.backend.controllerState()
  state.error = null
  const dump = vi.fn(app.backend.scoreLogDump.bind(app.backend))
  setBackend({ ...app.backend, kind: "tauri", controllerState: async () => state, scoreLogDump: dump })
  const user = userEvent.setup()
  render(<MidiRecordingControls />)
  await user.click(screen.getByRole("button", { name: "Dump note log" }))
  await waitFor(() => expect(useProjectStore.getState().history.cursor).toBe(1))
  expect(dump).toHaveBeenCalledWith(useProjectStore.getState().project.patterns[0]!.id, null, state.generation)
  expect(useProjectStore.getState().project.patterns[0]!.lanes.flatMap((lane) => lane.notes).some((note) => note.key === 111)).toBe(true)
  await act(async () => { await undo(); await settle() })
  expect((await app.backend.controllerState()).scoreLog.notes).toBe(1)
})
