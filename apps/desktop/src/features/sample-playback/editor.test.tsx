import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { InstrumentKind } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { isInstrumentChannel } from "@/lib/channel-source"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { settle, startTestApp } from "@/test/harness"
import { instrumentDescriptor } from "@/features/params"
import { InstrumentSection } from "@/features/channel-rack/inspector/instrument-section"
import { KeyboardSection } from "@/features/channel-rack/inspector/keyboard-section"
import { ExportDialog } from "@/features/export/export-dialog"
import { useUiStore } from "@/lib/store/ui"
import { announceProjectReplaced } from "@/lib/store/replaced"
import type { MockBackend } from "@/lib/ipc/mock"
import { defaultAsset } from "./settings"

let stop: () => void
let backend: MockBackend
beforeEach(async () => { ({ stop, backend } = await startTestApp()) })
afterEach(() => { stop(); vi.restoreAllMocks() })
const flush = () => act(() => settle())
function Panel({ id, keyboard = false }: { id: number; keyboard?: boolean }) {
  const channel = useProjectStore((state) => state.project.channels.find((c) => c.id === id))
  return channel && isInstrumentChannel(channel) ? <>
    <InstrumentSection channel={channel} />
    {keyboard && <KeyboardSection channel={channel} />}
  </> : null
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
  it.each(["sliceMap", "sliceDeck", "grainCloud", "waveRide"] as const)(
    "%s joins imported-source selection, editing, keyboard, history, reopen and export controls",
    async (kind) => {
      // This mounted case uses the shared Rust/WASM document and the browser
      // backend. The companion native Session case measures the actual PCM;
      // neither case claims an actual native-window run.
      const path = await backend.pickAudioFile()
      if (path === null) throw new Error("No import fixture")
      await act(() => backend.addChannelFromFile(path))
      const imported = useProjectStore.getState().project.channels.find((c) =>
        c.source.type === "sampler" && c.name === "Kick 02")
      if (!imported || imported.source.type !== "sampler" || imported.source.sample === null)
        throw new Error("Imported source missing")
      const sampleId = imported.source.sample
      const sample = useProjectStore.getState().project.samples.find((s) => s.id === sampleId)!
      const created = await dispatch({ type: "addChannel", instrument: kind })
      if (!created) throw new Error("No instrument")
      const id = created.created[0]
      const user = userEvent.setup()
      const on = vi.spyOn(backend, "auditionNoteOn")
      const off = vi.spyOn(backend, "auditionNoteOff")
      const detect = vi.spyOn(backend, "samplePlaybackDetect")
      render(<ValueContextMenus><Panel id={id} keyboard /><ExportDialog /></ValueContextMenus>)
      await user.click(screen.getByRole("combobox", { name: "Source A" }))
      await user.click(await screen.findByRole("option", { name: sample.name }))
      await flush()
      expect(asset(id).sample).toBe(sample.id)
      if (kind === "sliceMap" || kind === "sliceDeck") {
        await user.click(screen.getByRole("button", { name: "Detect beat grid" }))
        await flush()
        expect(detect).toHaveBeenCalledWith(id, { mode: "grid", gridTicks: 960 }, expect.any(Number), expect.any(Number))
        await user.click(screen.getByRole("button", { name: "Generate slice pattern" }))
        await flush()
        const lane = useProjectStore.getState().project.patterns[0].lanes.find((l) => l.channel === id)!
        expect(lane.notes.map((n) => n.key)).toEqual(asset(id).slices.map((_, i) => 60 + i))
      } else {
        await act(() => dispatch({ type: "addNotes", pattern: useProjectStore.getState().project.patterns[0].id,
          channel: id, notes: [{ start: 0, length: 1920, key: 60, velocity: 0.8 }] }))
        await flush()
      }
      const before = useProjectStore.getState().project.channels.find((c) => c.id === id)!
      const cursor = useProjectStore.getState().history.cursor
      if (kind === "grainCloud") {
        const control = within(screen.getByRole("group", { name: "Grain controls" }))
          .getByRole("slider", { name: instrumentDescriptor(kind).params[2].name })
        fireEvent.keyDown(control, { key: "End" }); fireEvent.keyUp(control, { key: "End" })
      } else if (kind === "waveRide") {
        const value = screen.getByRole("spinbutton", { name: "Point 2 value" })
        fireEvent.change(value, { target: { value: "0.2" } }); fireEvent.blur(value)
      } else {
        await user.click(screen.getByRole("button", { name: "Split slice" }))
      }
      await flush()
      const edited = useProjectStore.getState().project.channels.find((c) => c.id === id)!
      expect(edited).not.toEqual(before)
      expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
      await act(() => undo()); await flush()
      expect(useProjectStore.getState().project.channels.find((c) => c.id === id)).toEqual(before)
      await act(() => redo()); await flush()
      expect(useProjectStore.getState().project.channels.find((c) => c.id === id)).toEqual(edited)

      const key = within(screen.getByRole("group", { name: "Piano keyboard" })).getByRole("button", { name: "C5" })
      fireEvent.keyDown(key, { key: "Enter" })
      fireEvent.keyUp(key, { key: "Enter" })
      await flush()
      expect(on).toHaveBeenCalledWith(id, 60, 0.8)
      expect(off).toHaveBeenCalledWith(id, 60)

      const saved = `/sample-workflow-${kind}.windfall`
      await act(() => backend.projectSave(saved)); await flush()
      await act(() => backend.projectNew()); await flush()
      await act(() => backend.projectOpen(saved)); await flush()
      expect(useProjectStore.getState().project.channels.find((c) => c.id === id)).toEqual(edited)
      expect(screen.getByRole("combobox", { name: "Source A" })).toHaveTextContent(sample.name)
      const exported = vi.spyOn(backend, "exportAudio")
      act(() => useUiStore.getState().openDialog("export"))
      await user.click(screen.getByRole("button", { name: "Choose…" }))
      await user.click(screen.getByRole("button", { name: "Export" }))
      await flush()
      expect(exported).toHaveBeenCalledOnce()
      expect(exported).toHaveBeenCalledWith(expect.objectContaining({ mode: "pattern", format: "wav" }))
    }
  )
  it("retains migrated source A when assigning the second deck", async () => {
    const id = await open("sliceDeck")
    const source = { sampleRate: 8000, data: [0.2, 0.4, 0.6, 0.8] }
    await act(() => dispatch({ type: "setSamplePlayback", channel: id,
      asset: { ...defaultAsset(), inlineSource: source } }))
    await flush()
    const sample = useProjectStore.getState().project.samples[0]
    const user = userEvent.setup()
    await user.click(screen.getByRole("combobox", { name: "Source B" }))
    await user.click(await screen.findByRole("option", { name: sample.name }))
    await flush()
    expect(asset(id).inlineSource).toEqual(source)
    expect(asset(id).sample).toBeNull()
    expect(asset(id).sampleB).toBe(sample.id)
  })
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
    await user.click(await screen.findByRole("option", { name: "Key 60" }))
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
