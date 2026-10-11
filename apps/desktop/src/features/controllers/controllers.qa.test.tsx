import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { ControllerLink, ControllerState, LinkTarget } from "@/bindings"
import { setBackend } from "@/lib/ipc"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, undo, redo, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { startTestApp, settle } from "@/test/harness"
import { ControllerLinksPanel } from "./links-panel"
import { MidiRecordingControls } from "./recording-controls"
import { registerControllerActions } from "./actions"
import { runAction } from "@/lib/actions"

let app: Awaited<ReturnType<typeof startTestApp>>
let state: ControllerState
let unregister: () => void
beforeEach(async () => {
  app = await startTestApp()
  state = await app.backend.controllerState()
  state.error = null
  unregister = registerControllerActions()
})
afterEach(() => { unregister(); app.stop() })
const project = () => useProjectStore.getState().project
function native() {
  const learn = vi.fn(async (targets: LinkTarget[], global: boolean, multi: boolean) => {
    state = { ...state, learn: { targets, global, multi, linked: 0 } }
    return state
  })
  const settings = vi.fn(async (settings: ControllerState["settings"]) => {
    state = { ...state, settings }
    return state
  })
  const arm = vi.fn(async (armed: boolean) => {
    state = { ...state, noteRecord: { ...state.noteRecord, armed } }
    return state
  })
  setBackend({ ...app.backend, kind: "tauri", controllerState: async () => state,
    controllerLearn: learn, controllerSettingsSet: settings, noteRecordArm: arm,
    controllerLearnCancel: async () => { state = { ...state, learn: null }; return state },
  })
  return { learn, settings, arm }
}
const link = (): ControllerLink => ({
  source: { type: "midi", device: null, channel: 1, control: { type: "cc", number: 7 } },
  target: { type: "param", target: { type: "channelVolume", channel: project().channels[0]!.id } },
  map: { min: 0, max: 1, smoothingMs: 0, pickup: "inherit", mode: "absolute" },
})
function reopened() {
  const saved = SimDocument.create(structuredClone(project()))
  const opened = SimDocument.open(saved.fileText())
  const value = structuredClone(opened.project())
  saved.dispose(); opened.dispose()
  return value
}

it("mounts full link mapping against the WASM document with one history step and saved reopen", async () => {
  native()
  await dispatch({ type: "setControllerLinks", links: [link()] })
  const before = structuredClone(project())
  const cursor = useProjectStore.getState().history.cursor
  const user = userEvent.setup()
  render(<ControllerLinksPanel />)
  await user.click(screen.getByRole("button", { name: "Edit project link 1" }))
  fireEvent.change(screen.getByLabelText("Range start"), { target: { value: "0.8" } })
  fireEvent.change(screen.getByLabelText("Range end"), { target: { value: "0.2" } })
  fireEvent.change(screen.getByLabelText("Smoothing (ms)"), { target: { value: "250" } })
  fireEvent.change(screen.getByLabelText("Mapping formula"), { target: { value: "1 - x^2" } })
  await user.click(screen.getByRole("combobox", { name: "Controller mode" }))
  await user.click(await screen.findByRole("option", { name: "Relative: sign bit" }))
  await user.click(screen.getByRole("combobox", { name: "Link pickup" }))
  await user.click(await screen.findByRole("option", { name: "Off", exact: true }))
  await user.click(screen.getByRole("button", { name: "Apply controller link" }))
  await waitFor(() => expect(project().settings.controllerLinks?.[0]?.map.mode).toBe("relativeSignedBit"))
  expect(project().settings.controllerLinks?.[0]?.map).toEqual({
    min: 0.8, max: 0.2, smoothingMs: 250, formula: "1 - x^2", mode: "relativeSignedBit", pickup: "off",
  })
  expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
  expect(reopened().settings.controllerLinks).toEqual(project().settings.controllerLinks)
  const applied = structuredClone(project().settings.controllerLinks)
  await act(async () => { await undo(); await settle() })
  expect(project().settings.controllerLinks).toEqual(before.settings.controllerLinks)
  await act(async () => { await redo(); await settle() })
  expect(project().settings.controllerLinks).toEqual(applied)
})

it("refuses stale and invalid mapping drafts without changing document/history", async () => {
  native()
  await dispatch({ type: "setControllerLinks", links: [link()] })
  const user = userEvent.setup()
  render(<ControllerLinksPanel />)
  await user.click(screen.getByRole("button", { name: "Edit project link 1" }))
  await act(async () => { await dispatch({ type: "updateChannel", id: project().channels[0]!.id, patch: { pan: 0.2 } }) })
  expect(screen.getByRole("button", { name: "Apply controller link" })).toBeDisabled()
  await user.click(screen.getByRole("button", { name: "Cancel", exact: true }))
  await user.click(screen.getByRole("button", { name: "Edit project link 1" }))
  const before = structuredClone(project())
  const cursor = useProjectStore.getState().history.cursor
  fireEvent.change(screen.getByLabelText("Mapping formula"), { target: { value: "unknown(x)" } })
  await user.click(screen.getByRole("button", { name: "Apply controller link" }))
  await settle()
  expect(project()).toEqual(before)
  expect(useProjectStore.getState().history.cursor).toBe(cursor)
  await act(async () => { announceProjectReplaced(); await settle() })
  expect(screen.getByRole("button", { name: "Apply controller link" })).toBeDisabled()
})

it("shows pickup hold/cross/release and global-link learn/edit/removal", async () => {
  const calls = native()
  const target = { type: "channelVolume" as const, channel: project().channels[0]!.id }
  state = { ...state, waiting: [target], settings: { ...state.settings, links: [{ ...link(), target: { type: "slot", slot: { type: "masterVolume" } } }] } }
  const user = userEvent.setup()
  render(<ControllerLinksPanel />)
  await waitFor(() => expect(screen.getByLabelText("Controller pickup state")).toHaveTextContent("Pickup held"))
  state = { ...state, waiting: [] }
  await waitFor(() => expect(screen.getByLabelText("Controller pickup state")).toHaveTextContent("No controls held"))
  state = { ...state, waiting: [target] }
  await waitFor(() => expect(screen.getByLabelText("Controller pickup state")).toHaveTextContent("Pickup held"))
  state = { ...state, waiting: [] }
  await user.click(screen.getByRole("button", { name: "Learn global link" }))
  expect(calls.learn).toHaveBeenLastCalledWith([{ type: "slot", slot: { type: "masterVolume" } }], true, false, state.generation)
  await user.click(screen.getByRole("button", { name: "Edit global link 1" }))
  fireEvent.change(screen.getByLabelText("Smoothing (ms)"), { target: { value: "100" } })
  await user.click(screen.getByRole("button", { name: "Apply controller link" }))
  await waitFor(() => expect(calls.settings).toHaveBeenCalled())
  expect(state.settings.links[0]?.map.smoothingMs).toBe(100)
  await user.click(screen.getByRole("button", { name: "Edit global link 1" }))
  await user.click(screen.getByRole("button", { name: "Remove link" }))
  await waitFor(() => expect(state.settings.links).toEqual([]))
  expect(useProjectStore.getState().history.cursor).toBe(0)
})

it("mounts multilink and palette cancellation and the real MIDI-arm transport control", async () => {
  const calls = native()
  const user = userEvent.setup()
  render(<><ControllerLinksPanel /><MidiRecordingControls /></>)
  await waitFor(() => expect(screen.getByRole("button", { name: "Record MIDI notes" })).toBeEnabled())
  await user.click(screen.getByRole("button", { name: "Multi-link controllers" }))
  expect(calls.learn).toHaveBeenLastCalledWith([], false, true, state.generation)
  await runAction("controllers.multiLink")
  expect(state.learn).toBeNull()
  await user.click(screen.getByRole("button", { name: "Record MIDI notes" }))
  expect(calls.arm).toHaveBeenLastCalledWith(true)
  expect(screen.getByRole("button", { name: "Record MIDI notes" })).toHaveAttribute("aria-pressed", "true")
  await user.click(screen.getByRole("button", { name: "Record MIDI notes" }))
  expect(calls.arm).toHaveBeenLastCalledWith(false)
  expect(project().settings.controllerLinks).toBeUndefined()
})
