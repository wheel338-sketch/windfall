import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"
import { ValueContextMenus } from "@/components/value-context-menu"
import { InstrumentSection } from "../inspector/instrument-section"
import { isInstrumentChannel } from "@/lib/channel-source"
import { setBackend } from "@/lib/ipc"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { SimDocument } from "@/lib/ipc/sim/document"
import { startTestApp, settle } from "@/test/harness"

let app: Awaited<ReturnType<typeof startTestApp>> | null = null
afterEach(() => { app?.stop(); app = null })
function Panel({ id }: { id: number }) {
  const channel = useProjectStore((state) => state.project.channels.find((item) => item.id === id))
  return channel && isInstrumentChannel(channel) ? <InstrumentSection channel={channel} /> : null
}
it("mounts named hosted and hardware MIDI destinations with checked history, file roundtrip and target-removal disconnect", async () => {
  app = await startTestApp()
  const source = (await dispatch({ type: "addChannel", instrument: "midiOut" }))!.created[0]!
  const target = (await dispatch({ type: "addChannel", instrument: "tripleOsc" }))!.created[0]!
  const project = (await app.backend.documentSnapshot()).project
  project.plugins = [{ target: { type: "instrument", channel: target }, format: "clap", path: "synthetic-midi", id: "org.windfall.test.midi", name: "Hosted destination", state: [], parameters: [], auxiliaryInputs: [] }]
  app.stop(); app = await startTestApp({ project })
  const backend = app.backend
  const issued = vi.spyOn(backend, "dispatch")
  const hardware = await backend.midiHardwareState()
  setBackend({ ...backend, midiHardwareState: async () => ({ ...hardware, settings: { ...hardware.settings, output: "main", extraOutputs: ["synth"] }, outputs: [{ id: "main", name: "Main synth" }, { id: "synth", name: "Second synth" }] }) })
  const user = userEvent.setup()
  const cursor = useProjectStore.getState().history.cursor
  render(<ValueContextMenus><Panel id={source} /></ValueContextMenus>)
  await user.click(screen.getByRole("combobox", { name: "MIDI destination" }))
  await user.click(await screen.findByRole("option", { name: "Hosted destination" }))
  const params = () => {
    const channel = useProjectStore.getState().project.channels.find((item) => item.id === source)!
    if (!isInstrumentChannel(channel) || channel.source.params.type !== "midiOut") throw new Error("MIDI output missing")
    return channel.source.params
  }
  await waitFor(() => expect(params().pluginDestination).toBe(target))
  expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
  const snapshot = (await app.backend.documentSnapshot()).project
  expect(snapshot).toEqual({ ...useProjectStore.getState().project, nextId: snapshot.nextId })
  const saved = SimDocument.create(snapshot)
  const opened = SimDocument.open(saved.fileText()); expect(opened.project()).toEqual(snapshot); saved.dispose(); opened.dispose()
  await act(async () => { await undo(); await settle() })
  await user.click(screen.getByRole("combobox", { name: "MIDI output port" }))
  await user.click(await screen.findByRole("option", { name: "Output slot 1: Second synth" }))
  await waitFor(() => expect(params().outputPort).toBe(1))
  await user.click(screen.getByRole("combobox", { name: "MIDI destination" }))
  await user.click(await screen.findByRole("option", { name: "Hosted destination" }))
  await waitFor(() => expect(params().pluginDestination).toBe(target))
  await act(async () => { await dispatch({ type: "removeChannel", id: target }); await settle() })
  expect(params().pluginDestination, JSON.stringify(issued.mock.calls.map(([command]) => command))).toBe(-1)
  await act(async () => { await undo(); await settle() })
  expect(params().pluginDestination).toBe(target)
  expect(await dispatch({ type: "setInstrumentParams", channel: source, params: { ...params(), pluginDestination: source } })).toBeNull()
})
