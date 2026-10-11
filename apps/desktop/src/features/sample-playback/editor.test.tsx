import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { InstrumentKind } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { isInstrumentChannel } from "@/lib/channel-source"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { settle, startTestApp } from "@/test/harness"
import { instrumentDescriptor } from "@/features/params"
import { InstrumentSection } from "@/features/channel-rack/inspector/instrument-section"
import { announceProjectReplaced } from "@/lib/store/replaced"
import type { MockBackend } from "@/lib/ipc/mock"
import { defaultAsset } from "./settings"

let stop: () => void
let backend: MockBackend
beforeEach(async () => { ({ stop, backend } = await startTestApp()) })
afterEach(() => { stop(); vi.restoreAllMocks() })
const flush = () => act(() => settle())
function Panel({ id }: { id: number }) {
  const channel = useProjectStore((state) => state.project.channels.find((c) => c.id === id))
  return channel && isInstrumentChannel(channel) ? <InstrumentSection channel={channel} /> : null
}
async function open(kind: InstrumentKind) {
  const created = await dispatch({ type: "addChannel", instrument: kind })
  if (!created) throw new Error("No instrument")
  const id = created.created[0]
  const sample = useProjectStore.getState().project.samples[0].id
  await dispatch({ type: "setSamplePlayback", channel: id, asset: { ...defaultAsset(), sample } })
  render(<ValueContextMenus><Panel id={id} /></ValueContextMenus>)
  await flush()
  return id
}
const asset = (id: number) => useProjectStore.getState().project.channels.find((c) => c.id === id)!.samplePlayback!

describe("purpose-built sample instrument editors", () => {
  it("splits, edits, reverses and reorders slices with exact undo", async () => {
    const id = await open("sliceDeck")
    const cursor = useProjectStore.getState().history.cursor
    await userEvent.setup().click(screen.getByRole("button", { name: "Split slice" }))
    await flush()
    expect(asset(id).slices).toHaveLength(2)
    const gain = screen.getByRole("spinbutton", { name: "Slice gain" })
    fireEvent.change(gain, { target: { value: "0.4" } }); fireEvent.blur(gain)
    await flush()
    await userEvent.setup().click(screen.getByRole("switch", { name: "Reverse slice" }))
    await flush()
    expect(asset(id).slices[0]).toMatchObject({ gain: 0.4, reverse: true })
    await userEvent.setup().click(screen.getByRole("button", { name: "Move slice later" }))
    await flush()
    expect(asset(id).slices[1]).toMatchObject({ gain: 0.4, reverse: true, start: 0, end: 0.5 })
    await act(() => undo()); await flush()
    expect(asset(id).slices[0].reverse).toBe(true)
    expect(useProjectStore.getState().history.cursor).toBe(cursor + 3)
    expect(screen.getByRole("combobox", { name: "Source B" })).toBeVisible()
  })
  it("generates key-mapped notes in the chosen pattern as one history entry", async () => {
    const id = await open("sliceMap")
    const pattern = useProjectStore.getState().project.patterns[0].id
    useTransportStore.setState({ pattern })
    const before = useProjectStore.getState().history.cursor
    await userEvent.setup().click(screen.getByRole("button", { name: "Generate slice pattern" }))
    await flush()
    const notes = useProjectStore.getState().project.patterns[0].lanes.find((lane) => lane.channel === id)!.notes
    expect(notes.map((n) => [n.start, n.length, n.key])).toEqual([[0, 3840, 60]])
    expect(useProjectStore.getState().history.cursor).toBe(before + 1)
    await act(() => undo()); await flush()
    expect(useProjectStore.getState().project.patterns[0].lanes.some((lane) => lane.channel === id)).toBe(false)
  })
  it("mounts grain timing/randomization controls and persists a density gesture", async () => {
    const id = await open("grainCloud")
    const group = screen.getByRole("group", { name: "Grain controls" })
    const densityInfo = instrumentDescriptor("grainCloud").params[2]
    const control = within(group).getByRole("slider", { name: densityInfo.name })
    const before = useProjectStore.getState().history.cursor
    fireEvent.keyDown(control, { key: "End" }); fireEvent.keyUp(control, { key: "End" })
    await flush()
    const params = useProjectStore.getState().project.channels.find((c) => c.id === id)!.source
    expect(params).toMatchObject({ params: { type: "grainCloud", density: 128 } })
    expect(useProjectStore.getState().history.cursor).toBe(before + 1)
    expect(screen.getByText(/Grain spacing is/)).toBeVisible()
  })
  it("adds and edits drawn curve points through checked asset history", async () => {
    const id = await open("waveRide")
    const before = useProjectStore.getState().history.cursor
    await userEvent.setup().click(screen.getByRole("button", { name: "Add curve point" }))
    await flush()
    expect(asset(id).curve.points).toHaveLength(3)
    const middle = screen.getByRole("spinbutton", { name: "Point 2 value" })
    fireEvent.change(middle, { target: { value: "0.9" } }); fireEvent.blur(middle)
    await flush()
    expect(asset(id).curve.points[1]).toEqual({ time: 0.5, value: 0.9 })
    await act(() => undo()); await flush()
    expect(asset(id).curve.points[1].value).toBe(0.5)
    expect(useProjectStore.getState().history.cursor).toBe(before + 1)
    expect(screen.getByRole("img", { name: "Draw playback curve" })).toBeVisible()
  })
  it("stores a note-specific scratch curve separately from the shared curve", async () => {
    const id = await open("waveRide")
    const user = userEvent.setup()
    await user.click(screen.getByRole("combobox", { name: "Curve note" }))
    await user.click(await screen.findByRole("option", { name: "Key 60", exact: true }))
    const end = screen.getByRole("spinbutton", { name: "Point 2 value" })
    fireEvent.change(end, { target: { value: "0.2" } }); fireEvent.blur(end)
    await flush()
    expect(asset(id).curve.points[1].value).toBe(1)
    expect(asset(id).keyCurves).toEqual([{ key: 60, curve: { ...asset(id).curve,
      points: [{ time: 0, value: 0 }, { time: 1, value: 0.2 }] } }])
    await act(() => undo()); await flush()
    expect(asset(id).keyCurves).toEqual([])
  })
  it("remounts detection state on project replacement even when the channel ID repeats", async () => {
    await open("sliceMap")
    let rejectDetection: (error: Error) => void = () => {}
    vi.spyOn(backend, "samplePlaybackDetect").mockImplementation(() => new Promise((_resolve, reject) => { rejectDetection = reject }))
    await userEvent.setup().click(screen.getByRole("button", { name: "Detect transients" }))
    await flush()
    expect(screen.getByRole("button", { name: "Detect transients" })).toBeDisabled()
    act(() => announceProjectReplaced())
    expect(screen.getByRole("button", { name: "Detect transients" })).toBeEnabled()
    rejectDetection(new Error("The old document changed"))
    await flush()
    expect(screen.queryByText("The old document changed")).toBeNull()
  })
})
