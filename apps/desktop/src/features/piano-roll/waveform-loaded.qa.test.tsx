import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { TimeGridView, type IndexedBatch } from "@/lib/canvas"
import { createFake2D } from "@/lib/canvas/test-utils"
import { clearSampleInfo } from "@/features/channel-rack/inspector/sample-info"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { settle } from "@/test/harness"
import { channel, currentPattern, notesOf, startRoll, undoSteps } from "./test-utils"
import { useWaveformHelper } from "./waveform-helper"
import { Workspace } from "./workspace"

let roll: Awaited<ReturnType<typeof startRoll>>
let view: TimeGridView | null
let underlay: IndexedBatch | null
const frames = new Map<number, FrameRequestCallback>()
beforeEach(async () => {
  clearSampleInfo()
  useWaveformHelper.setState(useWaveformHelper.getInitialState(), true)
  view = null; underlay = null
  let frame = 0
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.set(++frame, callback); return frame })
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id))
  vi.spyOn(HTMLDivElement.prototype, "clientWidth", "get").mockReturnValue(800)
  vi.spyOn(HTMLDivElement.prototype, "clientHeight", "get").mockReturnValue(320)
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (_kind) {
    if (_kind !== "2d") return null
    const fake = createFake2D()
    Object.assign(fake.context, { setTransform() {}, clearRect() {}, fillText() {}, beginPath() {}, moveTo() {}, lineTo() {}, closePath() {}, fill() {} })
    return fake.context
  })
  const create = TimeGridView.create.bind(TimeGridView)
  vi.spyOn(TimeGridView, "create").mockImplementation(async (container, options) => {
    const created = await create(container, { ...options, renderer: "canvas2d", autoRender: false })
    const set = created.setUnderlay.bind(created)
    vi.spyOn(created, "setUnderlay").mockImplementation((batch) => { underlay = batch; set(batch) })
    view = created
    return created
  })
  roll = await startRoll()
  await dispatch({ type: "removeNotes", pattern: currentPattern().id, channel: channel("Kick").id,
    notes: notesOf("Kick").map((note) => note.id) })
  await dispatch({ type: "addNotes", pattern: currentPattern().id, channel: channel("Kick").id,
    notes: [{ start: 960, length: 480, key: 60 }] })
})
afterEach(() => { cleanup(); roll.stop(); clearSampleInfo(); frames.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals() })
async function choose(label: string, value: string) {
  const user = userEvent.setup()
  await user.click(screen.getByRole("combobox", { name: label }))
  await act(async () => { const pending = [...frames.values()]; frames.clear(); pending.forEach((frame) => frame(performance.now())); await settle() })
  await user.click(await screen.findByRole("option", { name: value }))
}
const waveform = () => {
  const batch = underlay?.batch
  if (!batch) return []
  return Array.from({ length: batch.count }, (_, index) => index)
    .filter((index) => batch.ids[index] <= -1_000_000)
    .map((index) => Array.from(batch.geometry.slice(index * 4, index * 4 + 4)))
}

it("loads actual source peaks into the mounted note grid and aligns seconds/fit/offset without affecting history", async () => {
  const project = useProjectStore.getState().project
  const sample = project.samples[0]
  const info = await roll.backend.sampleInfoById(sample.id)
  const before = undoSteps()
  render(<Workspace patternId={currentPattern().id} channelId={channel("Kick").id} />)
  await waitFor(() => expect(view?.items?.batch.count).toBeGreaterThan(0))
  await userEvent.setup().click(screen.getByRole("button", { name: "Waveform" }))
  await choose("Waveform helper audio", sample.name)
  await waitFor(() => expect(waveform().length).toBeGreaterThan(0))
  const originalTicks = Math.max(1, Math.round(info.durationSecs * project.settings.tempoBpm * 960 / 60))
  const rows = waveform()
  expect(rows[0][0]).toBe(0)
  expect(rows.at(-1)![0] + rows.at(-1)![1]).toBe(originalTicks)
  expect(view!.items!.batch.ids[0]).toBe(notesOf("Kick")[0].id)
  await choose("Waveform helper time mapping", "Fit to pattern")
  expect(waveform().at(-1)![0] + waveform().at(-1)![1]).toBe(currentPattern().lengthSteps * 240)
  act(() => useWaveformHelper.getState().configure({ start: -480 }))
  expect(waveform()[0][0]).toBe(-480)
  await userEvent.setup().click(screen.getByRole("button", { name: "Hide waveform" }))
  expect(waveform()).toEqual([])
  await userEvent.setup().click(screen.getByRole("button", { name: "Show waveform" }))
  expect(waveform().length).toBeGreaterThan(0)
  expect(undoSteps()).toBe(before)
  // The mounted workspace owns the real session/view; edit its note against
  // the Rust document while the loaded reference remains composed underneath.
  await act(async () => {
    view!.setViewport({ ...view!.viewport, scrollTick: 0, scrollRow: 57, pxPerTick: 0.05, rowHeight: 16 })
    const pointer = (type: string, x: number) => {
      const event = new MouseEvent(type, { bubbles: true, button: 0, clientX: x, clientY: 168 })
      Object.defineProperty(event, "pointerId", { value: 1 })
      fireEvent(view!.element, event)
    }
    pointer("pointerdown", 55)
    pointer("pointermove", 67)
    pointer("pointerup", 67)
    await settle()
  })
  expect(view!.items!.batch.geometry[0]).toBe(1200)
  expect(waveform()[0][0]).toBe(-480)
  await act(async () => { await undo(); await settle() })
  expect(view!.items!.batch.geometry[0]).toBe(960)
  await roll.backend.projectSave("/projects/waveform-grid.windfall")
  act(() => announceProjectReplaced())
  expect(useWaveformHelper.getState().sample).toBeNull()
  expect(waveform()).toEqual([])
})

it("refuses an old pending source result after replacement even when the new project reuses its id/path", async () => {
  const sample = useProjectStore.getState().project.samples[0]
  const info = await roll.backend.sampleInfoById(sample.id)
  let resolveOld!: (value: typeof info) => void
  const pending = new Promise<typeof info>((resolve) => { resolveOld = resolve })
  vi.spyOn(roll.backend, "sampleInfoById").mockImplementationOnce(() => pending).mockResolvedValue({ ...info, durationSecs: 2 })
  render(<Workspace patternId={currentPattern().id} channelId={channel("Kick").id} />)
  act(() => useWaveformHelper.getState().configure({ sample: sample.id, visible: true }))
  await settle()
  act(() => announceProjectReplaced())
  act(() => useWaveformHelper.getState().configure({ sample: sample.id, visible: true }))
  await waitFor(() => expect(waveform().length).toBeGreaterThan(0))
  const latest = waveform()
  await act(async () => { resolveOld(info); await pending; await settle() })
  expect(waveform()).toEqual(latest)
  expect(waveform().at(-1)![0] + waveform().at(-1)![1]).toBe(Math.round(2 * useProjectStore.getState().project.settings.tempoBpm * 960 / 60))
})
