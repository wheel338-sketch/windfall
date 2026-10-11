import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"
import type { ChannelId, InstrumentKind } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { instrumentDescriptor, paramIndex, readParam } from "@/features/params"
import { isInstrumentChannel } from "@/lib/channel-source"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"
import { InstrumentSection } from "../inspector/instrument-section"

let stop: () => void
beforeEach(async () => { ({ stop } = await startTestApp()) })
afterEach(() => stop())
function Panel({ id }: { id: ChannelId }) {
  const channel = useProjectStore((s) => s.project.channels.find((c) => c.id === id))
  return channel && isInstrumentChannel(channel) ? <InstrumentSection channel={channel} /> : null
}
const cases: [InstrumentKind, string, string][] = [["packSynth", "Windfall patch browser", "tone"], ["matrixSynth", "FM routing matrix", "fm.0.1"], ["hybridSeq", "FM and subtractive graph", "cutoffHz"], ["grooveSynth", "Groove engine", "cutoffHz"], ["pocketSynth", "Oscillator", "cutoffHz"], ["vintageSaw", "Charging saw oscillator", "cutoffHz"], ["compactSynth", "Quick oscillators", "cutoffHz"], ["dualPoly", "Dual oscillator banks", "cutoffHz"], ["vintagePoly", "DCO and divider", "cutoffHz"]]
async function mount(kind: InstrumentKind) {
  const added = await dispatch({ type: "addChannel", instrument: kind })
  if (!added) throw new Error("Could not add instrument")
  const channel = added.created[0]
  await dispatch({ type: "setInstrumentParams", channel, params: instrumentDescriptor(kind).defaults })
  render(<ValueContextMenus><Panel id={channel} /></ValueContextMenus>)
  await act(() => settle())
  return channel
}
function paramsOf(channel: ChannelId) {
  const source = useProjectStore.getState().project.channels.find((c) => c.id === channel)?.source
  if (source?.type !== "instrument") throw new Error("Missing instrument")
  return source.params
}
async function maximum(name: string) {
  const control = screen.getByRole("slider", { name })
  fireEvent.keyDown(control, { key: "End" }); fireEvent.keyUp(control, { key: "End" })
  await act(() => settle())
}
it.each(cases)("%s mounts its signal-flow editor and makes undoable parameter edits", async (kind, title, path) => {
  const added = await dispatch({ type: "addChannel", instrument: kind })
  if (!added) throw new Error("Could not add instrument")
  const channel = added.created[0]
  const descriptor = instrumentDescriptor(kind)
  await dispatch({ type: "setInstrumentParams", channel, params: descriptor.defaults })
  render(<ValueContextMenus><Panel id={channel} /></ValueContextMenus>)
  await act(() => settle())
  expect(screen.getByRole("region", { name: title })).toBeVisible()
  if (kind === "pocketSynth") { fireEvent.click(screen.getByRole("tab", { name: "Filter" })); await act(() => settle()) }
  const param = paramIndex(descriptor, path)
  const info = descriptor.params[param]
  const control = screen.getByRole("slider", { name: info.name })
  const cursor = useProjectStore.getState().history.cursor
  fireEvent.keyDown(control, { key: "End" }); fireEvent.keyUp(control, { key: "End" })
  await act(() => settle())
  expect(control).toHaveAttribute("aria-valuenow", String(info.max))
  expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
  const source = useProjectStore.getState().project.channels.find((c) => c.id === channel)?.source
  if (source?.type !== "instrument") throw new Error("Missing instrument")
  expect(readParam(source.params, info)).toBe(info.max)
  await act(() => undo()); await act(() => settle())
  expect(control).toHaveAttribute("aria-valuenow", String(info.default))
})

it("edits ring routing, operator feedback and live modulation in the matrix", async () => {
  const channel = await mount("matrixSynth")
  expect(screen.getByRole("table", { name: "FM operator routing" })).toBeVisible()
  expect(screen.getByRole("table", { name: "RING operator routing" })).toBeVisible()
  for (const [path, name] of [["ring.1.2", "RING 3 to 2"], ["operators.0.feedback", "Operator 1 feedback"], ["lfoIndex", "LFO phase depth"]]) {
    const info = instrumentDescriptor("matrixSynth").params[paramIndex(instrumentDescriptor("matrixSynth"), path)]
    await maximum(name)
    expect(readParam(paramsOf(channel), info)).toBe(info.max)
    await act(() => undo()); await act(() => settle())
    expect(readParam(paramsOf(channel), info)).toBe(info.default)
  }
})

it("edits the hybrid sequence rather than merely choosing an FM patch", async () => {
  const channel = await mount("hybridSeq")
  const table = screen.getByRole("table", { name: "Hybrid note sequence" })
  expect(within(table).getAllByRole("row")).toHaveLength(17)
  await maximum("Step 2 pitch")
  let params = paramsOf(channel)
  if (params.type !== "hybridSeq") throw new Error("Wrong instrument")
  expect(params.steps[1].pitch).toBe(24)
  await act(() => undo()); await act(() => settle())
  params = paramsOf(channel)
  if (params.type !== "hybridSeq") throw new Error("Wrong instrument")
  expect(params.steps[1].pitch).toBe(0)
})

it("searches own packs, selects a multisample patch and restores it with undo", async () => {
  const channel = await mount("packSynth")
  fireEvent.change(screen.getByRole("textbox", { name: "Search patches" }), { target: { value: "Soft keys" } })
  expect(screen.queryByRole("button", { name: "Prism / High chime" })).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: "Ember / Soft keys" }))
  await act(() => settle())
  let params = paramsOf(channel)
  if (params.type !== "packSynth") throw new Error("Wrong instrument")
  expect(params.patch).toBe("emberKeys")
  expect(screen.getByRole("button", { name: "Ember / Soft keys" })).toHaveAttribute("aria-pressed", "true")
  await act(() => undo()); await act(() => settle())
  params = paramsOf(channel)
  if (params.type !== "packSynth") throw new Error("Wrong instrument")
  expect(params.patch).toBe("emberBass")
})
