import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { PluginBinding, PluginManagerState } from "@/bindings"
import { targetState } from "@/lib/automation/targets"
import { useProjectStore, useUiStore } from "@/lib/store"
import { dispatch, undo } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { createMockBackend } from "@/lib/ipc/mock"
import { simulatedLatencyFrames } from "@/lib/ipc/sim/effects"
import { TransportSim } from "@/lib/ipc/sim/transport"
import { useRackStore } from "@/features/channel-rack/rack-store"
import { useEffectsUi } from "@/features/mixer/effects-ui"
import { PluginControls } from "./controls"
import { PluginManager } from "./manager"
import { openPluginManager, usePluginUi } from "./store"

const binding = (): PluginBinding => ({
  target: { type: "instrument", channel: 0 },
  format: "clap",
  path: "/plugins/native.clap",
  id: "native.synth",
  name: "Native Synth",
  state: [1, 2],
  parameters: [
    {
      id: 43,
      name: "Native cutoff",
      min: 100,
      max: 8000,
      value: 400,
      stepped: false,
      readOnly: false,
      automatable: true,
    },
  ],
})
const catalog = (): PluginManagerState => ({
  folders: ["/plugins"],
  entries: [
    {
      path: "/plugins/native.clap",
      format: "clap",
      id: "native.synth",
      name: "Native Synth",
      vendor: "Fixture",
      instrument: true,
      usable: true,
      error: null,
    },
    {
      path: "/plugins/gain.clap",
      format: "clap",
      id: "native.gain",
      name: "Native Gain",
      vendor: "Fixture",
      instrument: false,
      usable: true,
      error: null,
    },
  ],
  blocked: [
    {
      path: "/plugins/broken.clap",
      format: "clap",
      id: "",
      name: "Broken",
      vendor: "",
      instrument: false,
      usable: false,
      error: "Scanner timed out",
    },
  ],
  scanning: false,
  completed: 3,
  total: 3,
  current: null,
  error: null,
  instances: [],
})
let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
  usePluginUi.setState({ open: false, track: undefined })
  Object.defineProperty(app.backend, "kind", { value: "tauri" })
  vi.spyOn(app.backend, "pluginsState").mockResolvedValue(catalog())
  vi.spyOn(app.backend, "pluginsScan").mockResolvedValue()
  vi.spyOn(app.backend, "pluginEditor").mockResolvedValue()
  vi.spyOn(app.backend, "pluginsAdd").mockImplementation((_path, id, track) =>
    app.backend.dispatch(
      track === undefined
        ? { type: "addPluginInstrument", plugin: binding() }
        : {
            type: "addPluginEffect",
            track,
            plugin: {
              ...binding(),
              id,
              name: "Native Gain",
              target: { type: "effect", effect: 0 },
            },
          }
    )
  )
})
afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})
it("retains unavailable native references and reports silent browser placeholders", async () => {
  await act(async () => {
    await dispatch({ type: "addPluginInstrument", plugin: binding() })
  })
  const project = (await app.backend.documentSnapshot()).project
  const plugin = project.plugins![0]
  const mock = createMockBackend({ project, storage: null })
  try {
    expect((await mock.pluginsState()).instances).toEqual([
      {
        target: plugin.target,
        error: "Native plugins require the Windows desktop app",
      },
    ])
    expect(simulatedLatencyFrames(project, 48000)).toBe(0)
    const transport = new TransportSim(project.patterns[0].id)
    if (plugin.target.type !== "instrument")
      throw new Error("instrument expected")
    transport.noteOn(plugin.target.channel, 60, 1)
    expect(
      transport.advance(project, 0.01).meters.every((level) => level === 0)
    ).toBe(true)
  } finally {
    mock.dispose()
  }
})

it("searches scanned identities, retries blocked files, and selects an added instrument", async () => {
  act(() => openPluginManager())
  render(<PluginManager />)
  await screen.findByText("Native Synth · Fixture · CLAP")
  await userEvent.click(screen.getByRole("button", { name: "Retry Broken" }))
  expect(app.backend.pluginsScan).toHaveBeenCalledWith("/plugins/broken.clap")
  await userEvent.type(screen.getByLabelText("Search plugins"), "Synth")
  expect(
    screen.queryByText("Native Gain · Fixture · CLAP")
  ).not.toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Add instrument" }))
  await waitFor(() => expect(usePluginUi.getState().open).toBe(false))
  const plugin = useProjectStore.getState().project.plugins?.[0]
  expect(plugin?.name).toBe("Native Synth")
  expect(useUiStore.getState().selectedChannel).toBe(
    plugin?.target.type === "instrument" ? plugin.target.channel : -1
  )
  expect(useRackStore.getState().inspectorOpen).toBe(true)
})

it("adds an effect to the requested track and opens its actual inspector", async () => {
  act(() => openPluginManager(0))
  render(<PluginManager />)
  await screen.findByText("Native Gain · Fixture · CLAP")
  await userEvent.click(screen.getByRole("button", { name: "Add effect" }))
  await waitFor(() => expect(usePluginUi.getState().open).toBe(false))
  const plugin = useProjectStore.getState().project.plugins?.[0]
  expect(app.backend.pluginsAdd).toHaveBeenCalledWith(
    "/plugins/gain.clap",
    "native.gain",
    0
  )
  expect(useEffectsUi.getState().selectedEffect).toBe(
    plugin?.target.type === "effect" ? plugin.target.effect : -1
  )
})

it("names the destination track the way the mixer numbers it, not by its id", async () => {
  // Removing an earlier track leaves the last one's id ahead of its place.
  const inserts = () =>
    useProjectStore.getState().project.mixer.tracks.filter((track) => !track.current)
  await dispatch({ type: "removeMixerTrack", id: inserts()[1].id })
  const last = inserts().at(-1)!
  const place = inserts().length - 1
  expect(last.id).not.toBe(place)
  act(() => useUiStore.getState().selectTrack(last.id))
  act(() => openPluginManager())
  render(<PluginManager />)
  expect(
    await screen.findByText(new RegExp(`effects to mixer track ${place}, ${last.name}\\.`))
  ).toBeInTheDocument()
  await screen.findByText("Native Gain · Fixture · CLAP")
  await userEvent.click(screen.getByRole("button", { name: "Add effect" }))
  expect(app.backend.pluginsAdd).toHaveBeenCalledWith("/plugins/gain.clap", "native.gain", last.id)
})

it("keeps one search for the scanned files and the library", async () => {
  act(() => openPluginManager(0))
  render(<PluginManager />)
  await screen.findByText("Native Synth · Fixture · CLAP")
  expect(screen.getAllByRole("textbox", { name: /search/i })).toHaveLength(1)
  expect(screen.getByRole("region", { name: "Plugin files on this computer" })).toBeInTheDocument()
  expect(screen.getByRole("region", { name: "Plugin library" })).toBeInTheDocument()
})

it("retains a failed VST3 scan and refuses additions", async () => {
  const state = catalog()
  state.entries = [
    {
      ...state.entries[0],
      path: "/plugins/native.vst3",
      format: "vst3",
      usable: false,
      error: "Plugin rejected its initialized layout",
    },
  ]
  vi.mocked(app.backend.pluginsState).mockResolvedValue(state)
  act(() => openPluginManager())
  render(<PluginManager />)
  await screen.findByText("Native Synth · Fixture · VST3")
  expect(screen.getByRole("button", { name: "Add instrument" })).toBeDisabled()
  await userEvent.click(screen.getByRole("button", { name: "Add instrument" }))
  expect(app.backend.pluginsAdd).not.toHaveBeenCalled()
  expect(
    screen.getByText(/Plugin rejected its initialized layout/)
  ).toBeVisible()
})

it("adds a usable scanned VST3 instrument through the desktop backend", async () => {
  const state = catalog()
  state.entries = [
    { ...state.entries[0], path: "/plugins/native.vst3", format: "vst3" },
  ]
  vi.mocked(app.backend.pluginsState).mockResolvedValue(state)
  act(() => openPluginManager())
  render(<PluginManager />)
  await screen.findByText("Native Synth · Fixture · VST3")
  await userEvent.click(screen.getByRole("button", { name: "Add instrument" }))
  await waitFor(() => expect(usePluginUi.getState().open).toBe(false))
  expect(app.backend.pluginsAdd).toHaveBeenCalledWith(
    "/plugins/native.vst3",
    "native.synth",
    undefined
  )
  expect(useRackStore.getState().inspectorOpen).toBe(true)
})

it("edits stable native IDs in their real range with undo and editor routing", async () => {
  await act(async () => {
    await dispatch({ type: "addPluginInstrument", plugin: binding() })
  })
  const plugin = useProjectStore.getState().project.plugins![0]
  render(<PluginControls binding={plugin} />)
  const cutoff = screen.getByLabelText("Native cutoff")
  expect(cutoff).toHaveAttribute("min", "100")
  expect(cutoff).toHaveAttribute("max", "8000")
  fireEvent.focus(cutoff)
  fireEvent.change(cutoff, { target: { value: "9000" } })
  fireEvent.blur(cutoff)
  await waitFor(() =>
    expect(
      useProjectStore.getState().project.plugins![0].parameters[0].value
    ).toBe(8000)
  )
  const target =
    plugin.target.type === "instrument"
      ? {
          type: "instrumentParam" as const,
          channel: plugin.target.channel,
          param: 0,
        }
      : { type: "tempo" as const }
  expect(
    targetState(useProjectStore.getState().project, target)?.range
  ).toEqual({ min: 100, max: 8000, taper: "linear" })
  await act(async () => {
    await undo()
  })
  expect(
    useProjectStore.getState().project.plugins![0].parameters[0].value
  ).toBe(400)
  await userEvent.click(
    screen.getByRole("button", { name: "Open native editor" })
  )
  expect(app.backend.pluginEditor).toHaveBeenCalledWith(plugin.target, true)
})
