import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { ControllerState, Command } from "@/bindings"
import { ChannelRow } from "@/features/channel-rack/channel-row"
import { ValueContextMenus } from "@/components/value-context-menu"
import { TransportBar } from "@/features/transport/transport-bar"
import { useAutomationRecordStore } from "@/features/automation/record"
import { setBackend } from "@/lib/ipc"
import { dispatch, undo, redo, useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { SimDocument } from "@/lib/ipc/sim/document"
import { startTestApp, settle } from "@/test/harness"

let app: Awaited<ReturnType<typeof startTestApp>>
let state: ControllerState
beforeEach(async () => {
  app = await startTestApp(); state = await app.backend.controllerState(); state.error = null
  useAutomationRecordStore.setState({ recordArmed: false })
})
afterEach(() => { useAutomationRecordStore.setState({ recordArmed: false }); vi.restoreAllMocks(); app.stop() })
const project = () => useProjectStore.getState().project
function saveReopen() {
  const saved = SimDocument.create(structuredClone(project()))
  const opened = SimDocument.open(saved.fileText())
  expect(opened.project()).toEqual(project()); saved.dispose(); opened.dispose()
}

// Session-capability replies are fixtures. Commands, subscriptions, mounted
// controls and history execute against the real WASM document. Rust tests prove
// the native capture and timing algorithm separately; this is not Rust IPC.
it("mounts MIDI arm, configured count-in, play, quantized stop commit, undo/reopen and replacement refusal", async () => {
  const pattern = project().patterns[0]!.id, channel = project().channels[0]!.id
  const arm = vi.fn(async (armed: boolean) => { state = { ...state, noteRecord: { ...state.noteRecord, armed } }; return state })
  const toggle = vi.fn(() => app.backend.transportToggle())
  let captured: Command | null = null
  const stop = vi.fn(async () => {
    if (captured) { await app.backend.dispatch(captured, 887001); captured = null }
    state = { ...state, noteRecord: { ...state.noteRecord, pending: 0 } }
    return app.backend.transportStop()
  })
  setBackend({ ...app.backend, kind: "tauri", controllerState: async () => state, noteRecordArm: arm, transportToggle: toggle, transportStop: stop })
  await app.backend.transportSet({ countInBars: 1 })
  const before = structuredClone(project()), cursor = useProjectStore.getState().history.cursor
  const user = userEvent.setup()
  render(<TransportBar />)
  const record = screen.getByRole("button", { name: "Record MIDI notes" })
  await waitFor(() => expect(record).toBeEnabled())
  expect(record).toHaveAttribute("title", "Record MIDI notes; 1 count-in bars")
  await user.click(record); expect(arm).toHaveBeenCalledWith(true)
  await user.click(screen.getByRole("button", { name: "Play or stop" }))
  expect(toggle).toHaveBeenCalledTimes(1)
  expect(useTransportStore.getState().playing).toBe(true)
  // A native quantized capture reply at stop: start 251->240; the sounding duration stays 245.
  captured = { type: "addNotes", pattern, channel, notes: [{ start: 240, length: 245, key: 110, velocity: 0.75 }] }
  state = { ...state, noteRecord: { ...state.noteRecord, pending: 1 } }
  await user.click(screen.getByRole("button", { name: "Stop" }))
  await waitFor(() => expect(useProjectStore.getState().history.cursor).toBe(cursor + 1))
  expect(project().patterns[0]!.lanes.flatMap((lane) => lane.notes).find((note) => note.key === 110)).toMatchObject({ start: 240, length: 245 })
  expect(useTransportStore.getState().playing).toBe(false)
  saveReopen()
  await act(async () => { await undo(); await settle() }); expect(project()).toEqual(before)
  await act(async () => { await redo(); await settle() }); saveReopen()
  await app.backend.projectNew()
  await act(async () => { announceProjectReplaced(); await settle() })
  await expect(app.backend.scoreLogDump(pattern, null, state.generation)).rejects.toThrow(/changed/)
})

it("mounts automation arm and control movements with a pattern-lane commit grouped into one WASM history gesture", async () => {
  const pattern = project().patterns[0]!.id, channel = project().channels[0]!.id
  const target = { type: "channelVolume" as const, channel }
  const initial = [{ tick: 0, value: 0.5, curve: 0, hold: true }, { tick: 3000, value: 0.5, curve: 0, hold: true }]
  await dispatch({ type: "setPatternEvents", pattern, target, expected: [], points: initial })
  const before = structuredClone(project()), cursor = useProjectStore.getState().history.cursor
  const arm = vi.fn(async (armed: boolean) => { state = { ...state, automationRecord: { ...state.automationRecord, armed } }; return state })
  const send = vi.fn((command: Command) => app.backend.dispatch(command, 887002))
  const stop = vi.fn(async () => {
    await app.backend.dispatch({ type: "setPatternEvents", pattern, target, expected: initial, points: [initial[0]!, { tick: 120, value: 0, curve: 0, hold: true }, initial[1]!] }, 887002)
    return app.backend.transportStop()
  })
  setBackend({ ...app.backend, kind: "tauri", controllerState: async () => state, automationRecordArm: arm, dispatch: send, transportStop: stop })
  const user = userEvent.setup()
  render(<ValueContextMenus><TransportBar /><ChannelRow id={channel} pattern={pattern} lengthSteps={16} groupSize={4} anySolo={false} drop={null} /></ValueContextMenus>)
  await user.click(screen.getByRole("button", { name: "Record automation" }))
  await waitFor(() => expect(arm).toHaveBeenCalledWith(true))
  await user.click(screen.getByRole("button", { name: "Play or stop" }))
  const control = screen.getByRole("slider", { name: `${project().channels[0]!.name} channel volume` })
  for (const key of ["ArrowRight", "ArrowLeft", "Home"]) { fireEvent.keyDown(control, { key }); fireEvent.keyUp(control, { key }); await act(settle) }
  await user.click(screen.getByRole("button", { name: "Stop" }))
  await waitFor(() => expect(project().patterns[0]!.events[0]!.points.some((point) => point.tick === 120)).toBe(true))
  expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
  expect(project().channels[0]!.volume).toBe(0)
  saveReopen()
  await act(async () => { await undo(); await settle() }); expect(project()).toEqual(before)
})
