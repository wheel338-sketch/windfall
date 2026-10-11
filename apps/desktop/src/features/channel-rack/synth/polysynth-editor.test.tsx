import { act, fireEvent, render, screen } from "@testing-library/react"
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
