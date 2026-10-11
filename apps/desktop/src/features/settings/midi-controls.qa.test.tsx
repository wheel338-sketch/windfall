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
    await user.click(await screen.findByRole("option", { name: "On", exact: true }))
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
