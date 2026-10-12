import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { MidiHardwareSettings, MidiHardwareState, ControllerSettings } from "@/bindings"
import { setBackend } from "@/lib/ipc"
import { startTestApp } from "@/test/harness"
import { MidiHardwareSettingsSection } from "./midi-hardware"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => { app = await startTestApp() })
afterEach(() => { app.stop() })

it("mounts independent extra inputs, each profile, offset and both clock controls against the real document", async () => {
  let state: MidiHardwareState = {
    ...(await app.backend.midiHardwareState()),
    inputs: [{ id: "keys", name: "Keys" }, { id: "surface", name: "Surface" }],
    generation: 1, error: null,
    settings: { ...(await app.backend.midiHardwareState()).settings, input: "keys" },
  }
  let controllers = await app.backend.controllerState()
  controllers.error = null
  controllers.profiles = [
    { id: "mackie-control", name: "Mackie compatible", ports: [], controls: [] },
    { id: "korg-nanokontrol2", name: "Small surface", ports: [], controls: [] },
    { id: "general-midi-mixer", name: "Standard mixer", ports: [], controls: [] },
  ]
  const configure = vi.fn(async (settings: MidiHardwareSettings) => { state = { ...state, settings }; return state })
  const settings = vi.fn(async (settings: ControllerSettings) => { controllers = { ...controllers, settings }; return controllers })
  setBackend({ ...app.backend, kind: "tauri", midiHardwareState: async () => state, midiHardwareConfigure: configure,
    controllerState: async () => controllers, controllerSettingsSet: settings,
  })
  const before = await app.backend.documentSnapshot()
  const user = userEvent.setup()
  render(<MidiHardwareSettingsSection />)
  await waitFor(() => expect(screen.getByRole("button", { name: "Add MIDI input" })).toBeEnabled())
  await user.click(screen.getByRole("button", { name: "Add MIDI input" }))
  expect(state.settings.extraInputs).toEqual([{ id: "surface", profile: null }])
  for (const profile of controllers.profiles) {
    await user.click(screen.getByRole("combobox", { name: "Additional input 1 profile" }))
    await user.click(await screen.findByRole("option", { name: profile.name }))
    expect(state.settings.extraInputs[0]?.profile).toBe(profile.id)
    expect(state.settings.input).toBe("keys")
  }
  for (const label of ["Send MIDI clock", "Follow MIDI clock"]) {
    await user.click(screen.getByRole("combobox", { name: label }))
    await user.click(await screen.findByRole("option", { name: "On" }))
  }
  expect(state.settings.sendClock && state.settings.followClock).toBe(true)
  const offset = screen.getByLabelText("Recording offset (ms)")
  fireEvent.change(offset, { target: { value: "-35" } })
  fireEvent.blur(offset)
  await waitFor(() => expect(settings).toHaveBeenCalledWith(expect.objectContaining({ recordOffsetMs: -35 })))
  await user.click(screen.getByRole("button", { name: "Remove additional input 1" }))
  expect(state.settings.extraInputs).toEqual([])
  expect(await app.backend.documentSnapshot()).toEqual(before)
  expect(configure.mock.calls.length).toBe(7)
})

it("mounts independent MIDI output slots and keeps the later slot fixed when an earlier port is disabled", async () => {
  let state: MidiHardwareState = { ...(await app.backend.midiHardwareState()), outputs: [{ id: "main", name: "Main" }, { id: "aux1", name: "First synth" }, { id: "aux2", name: "Second synth" }], settings: { ...(await app.backend.midiHardwareState()).settings, output: "main" }, error: null }
  const configure = vi.fn(async (settings: MidiHardwareSettings) => { state = { ...state, settings }; return state })
  setBackend({ ...app.backend, kind: "tauri", midiHardwareState: async () => state, midiHardwareConfigure: configure })
  const user = userEvent.setup()
  render(<MidiHardwareSettingsSection />)
  const add = await screen.findByRole("button", { name: "Add MIDI output" })
  await waitFor(() => expect(add).toBeEnabled())
  await user.click(add); await waitFor(() => expect(state.settings.extraOutputs).toEqual(["aux1"]))
  await user.click(add); await waitFor(() => expect(state.settings.extraOutputs).toEqual(["aux1", "aux2"]))
  await user.click(screen.getByRole("combobox", { name: "MIDI output slot 1" }))
  await user.click(await screen.findByRole("option", { name: "Disabled" }))
  expect(state.settings.extraOutputs).toEqual(["", "aux2"])
  expect(state.settings.output).toBe("main")
  await user.click(add)
  expect(state.settings.extraOutputs).toEqual(["aux1", "aux2"])
})
