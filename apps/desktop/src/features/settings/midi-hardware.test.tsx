import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { MidiHardwareState } from "@/bindings"
import { setBackend, type Backend } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store"
import { startTestApp } from "@/test/harness"
import { MidiHardwareSettingsSection, portOptions } from "./midi-hardware"
import { hardwareHeldKey, heldKeys } from "@/features/piano-roll/chords/held-keys"

describe("MIDI hardware settings", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  beforeEach(async () => {
    app = await startTestApp()
  })
  afterEach(() => {
    app.stop()
  })

  /** The settings added with controller profiles and MIDI clock. */
  const MORE_SETTINGS = {
    inputProfile: null,
    extraInputs: [],
    sendClock: false,
    followClock: false,
  }
  const hardware = (): MidiHardwareState => ({
    settings: {
      input: null,
      output: null,
      inputChannel: null,
      outputChannel: 1,
      ...MORE_SETTINGS,
    },
    inputs: [
      { id: "input-1", name: "Keyboard" },
      { id: "input-2", name: "Keyboard" },
    ],
    outputs: [{ id: "output", name: "Synth" }],
    inputConnected: false,
    outputConnected: false,
    target: null,
    generation: 42,
    droppedEvents: 0,
    error: null,
    connectedInputs: [],
    clockRunning: false,
    outputDropped: 0,
    externalBpm: null,
  })
  function native(overrides: Partial<Backend> = {}) {
    let state = hardware()
    const calls = {
      midiHardwareState: vi.fn(async () => state),
      midiHardwareRefresh: vi.fn(async () => state),
      midiHardwareConfigure: vi.fn(
        async (settings: MidiHardwareState["settings"]) => {
          state = {
            ...state,
            settings,
            inputConnected: settings.input !== null,
            outputConnected: settings.output !== null,
          }
          return state
        }
      ),
      midiHardwareTarget: vi.fn(async (channel: number | null) => {
        state = { ...state, target: channel }
        return state
      }),
      midiHardwarePanic: vi.fn(async () => {}),
    }
    setBackend({ ...app.backend, kind: "tauri", ...calls, ...overrides })
    return calls
  }

  it("uses opaque IDs for duplicate labels and retains unavailable saved ports", () => {
    expect(portOptions(hardware().inputs, "missing")).toEqual([
      { value: "off", label: "Disabled" },
      { value: "port:input-1", label: "Keyboard" },
      { value: "port:input-2", label: "Keyboard" },
      { value: "port:missing", label: "Saved device unavailable" },
    ])
  })

  it("states the browser hardware limit and disables device changes", async () => {
    render(<MidiHardwareSettingsSection />)
    expect(
      await screen.findByText(/Browser simulation has no native MIDI ports/)
    ).toBeVisible()
    expect(screen.getByRole("combobox", { name: "MIDI input" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Refresh ports" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Panic MIDI" })).toBeEnabled()
  })

  it("configures live input/output independently and guards the explicit destination", async () => {
    const user = userEvent.setup()
    const calls = native()
    const before = await app.backend.documentSnapshot()
    render(<MidiHardwareSettingsSection />)
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "MIDI input" })).toBeEnabled()
    )
    await user.click(screen.getByRole("combobox", { name: "MIDI input" }))
    const keyboards = await screen.findAllByRole("option", { name: "Keyboard" })
    await user.click(keyboards[1])
    expect(calls.midiHardwareConfigure).toHaveBeenLastCalledWith({
      input: "input-2",
      output: null,
      inputChannel: null,
      outputChannel: 1,
      ...MORE_SETTINGS,
    })
    await user.click(screen.getByRole("combobox", { name: "Live MIDI output" }))
    await user.click(await screen.findByRole("option", { name: "Synth" }))
    expect(calls.midiHardwareConfigure).toHaveBeenLastCalledWith({
      input: "input-2",
      output: "output",
      inputChannel: null,
      outputChannel: 1,
      ...MORE_SETTINGS,
    })
    const channel = useProjectStore.getState().project.channels[0]
    await user.click(
      screen.getByRole("combobox", { name: "Audition destination" })
    )
    await user.click(await screen.findByRole("option", { name: channel.name }))
    expect(calls.midiHardwareTarget).toHaveBeenLastCalledWith(
      channel.id,
      42,
      useProjectStore.getState().revision
    )
    hardwareHeldKey(channel.id, 60, 100)
    await user.click(screen.getByRole("button", { name: "Panic MIDI" }))
    expect(calls.midiHardwarePanic).toHaveBeenCalledOnce()
    await waitFor(() => expect(heldKeys(channel.id)).toEqual([]))
    expect(await app.backend.documentSnapshot()).toEqual(before)
  })

  it("reports refused configuration and keeps panic available during pending work", async () => {
    const user = userEvent.setup()
    let reject!: (error: Error) => void
    const pending = new Promise<MidiHardwareState>((_, fail) => {
      reject = fail
    })
    const calls = native({ midiHardwareConfigure: () => pending })
    render(<MidiHardwareSettingsSection />)
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Reconnect devices" })
      ).toBeEnabled()
    )
    await user.click(screen.getByRole("button", { name: "Reconnect devices" }))
    expect(screen.getByRole("combobox", { name: "MIDI input" })).toBeDisabled()
    await user.click(screen.getByRole("button", { name: "Panic MIDI" }))
    expect(calls.midiHardwarePanic).toHaveBeenCalledOnce()
    reject(new Error("Stop or cancel recording first."))
    expect(
      await screen.findByText("Stop or cancel recording first.")
    ).toBeVisible()
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "MIDI input" })).toBeEnabled()
    )
  })
})
