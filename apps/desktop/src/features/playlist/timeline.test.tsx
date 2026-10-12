import userEvent from "@testing-library/user-event"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { logicalDelta, UI_SCALE_EVENT } from "@/lib/ui-scale"
import { tickToX } from "@/lib/canvas"
import { settle } from "@/test/harness"
import type { ExportOptions } from "@/bindings"
import { startPlaylist } from "./test-utils"
import PlaylistPanel from "./index"
import { GridMetrics } from "./metrics"
import { Ruler } from "./ruler"
import { usePlaylistStore } from "./store"
import { addClips } from "./ops"
import { editTimeline } from "./timeline-store"
import { registry } from "@/lib/actions"
import { useTransportStore } from "@/lib/store/transport"
import { TimelineGesture } from "./timeline-gesture"
import {
  applyPlaybackRegion,
  selectTimelineRegion,
  useTimelineStore,
} from "./timeline-store"

vi.mock("@/lib/canvas/react", () => ({ TimeGridCanvas: () => null }))
let rig: Awaited<ReturnType<typeof startPlaylist>>
beforeEach(async () => {
  rig = await startPlaylist()
  announceProjectReplaced()
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
})
afterEach(() => {
  rig.stop()
  delete document.documentElement.dataset.uiScale
})

it("adds, edits, deletes and undoes meters/markers through accessible menus and real WASM", async () => {
  const user = userEvent.setup()
  render(<PlaylistPanel />)
  await user.click(screen.getByRole("button", { name: "Timeline" }))
  expect(
    await screen.findByRole("menuitem", { name: /^Play selected song region/ })
  ).toHaveAttribute("aria-disabled", "true")
  await user.click(
    screen.getByRole("menuitem", { name: "Add song meter change…" })
  )
  fireEvent.change(screen.getByLabelText("Song tick"), {
    target: { value: "4001" },
  })
  fireEvent.change(screen.getByLabelText("Beats per bar"), {
    target: { value: "7" },
  })
  fireEvent.change(screen.getByLabelText("Beat unit"), {
    target: { value: "8" },
  })
  await user.click(screen.getByRole("button", { name: "Save" }))
  await act(async () => {
    await settle()
  })
  const meter = useProjectStore.getState().project.playlist.timeline!.meters[0]
  expect(meter.tick).toBe(4001)
  screen.getByRole("button", { name: "Timeline" }).focus()
  await user.keyboard("{ArrowDown}")
  await user.click(
    await screen.findByRole("menuitem", { name: "7/8 at tick 4001" })
  )
  fireEvent.change(screen.getByLabelText("Song tick"), {
    target: { value: "4003" },
  })
  await user.click(screen.getByRole("button", { name: "Save" }))
  await act(async () => {
    await settle()
  })
  expect(
    useProjectStore.getState().project.playlist.timeline!.meters[0]
  ).toMatchObject({ id: meter.id, tick: 4003 })
  await act(async () => {
    await undo()
  })
  expect(
    useProjectStore.getState().project.playlist.timeline!.meters[0].tick
  ).toBe(4001)
  await act(async () => {
    await dispatch({
      type: "addTimelineMarker",
      tick: 17,
      name: "Verse",
      kind: { type: "named" },
    })
  })
  const marker =
    useProjectStore.getState().project.playlist.timeline!.markers[0]
  useTimelineStore.setState({ selected: { type: "marker", id: marker.id } })
  await act(async () => {
    await dispatch({ type: "removeTimelineMarker", id: marker.id })
  })
  expect(useTimelineStore.getState().selected).toBeNull()
})

it("fits dragged bounds in logical coordinates at 75–200% scale, DPR and scroll", () => {
  for (const scale of [75, 100, 125, 150, 175, 200]) {
    for (const dpr of [1, 1.5, 2]) {
      document.documentElement.dataset.uiScale = String(scale)
      const metrics = new GridMetrics()
      metrics.setViewport({
        ...metrics.viewport,
        width: 800,
        scrollTick: 1000,
        pxPerTick: 0.1,
        dpr,
      })
      const drag = new TimelineGesture(
        metrics,
        logicalDelta((10 * scale) / 100),
        "zoom"
      )
      drag.update(logicalDelta((300 * scale) / 100))
      expect(useTimelineStore.getState().draft).toEqual({
        start: 1100,
        end: 4000,
      })
      drag.finish(logicalDelta((300 * scale) / 100))
      expect(tickToX(metrics.viewport, 1100)).toBeCloseTo(24)
      expect(tickToX(metrics.viewport, 4000)).toBeCloseTo(776)
      expect(useTimelineStore.getState().selection).toBeNull()
    }
  }
})

it("keeps typed timeline identities numeric through actual WASM commands, refusal, history and save/open", async () => {
  const first = useProjectStore.getState().project.nextId
  const reply = await rig.backend.dispatch({
    type: "batch",
    commands: [
      {
        type: "addMeterChange",
        tick: 4001,
        signature: { numerator: 7, denominator: 8 },
      },
      {
        type: "addTimelineMarker",
        tick: 17,
        name: "Verse",
        kind: { type: "named" },
      },
    ],
  })
  expect(reply.created).toEqual([first, first + 1])
  const snapshot = await rig.backend.documentSnapshot()
  const timeline = snapshot.project.playlist.timeline!
  expect(timeline.meters[0].id).toBe(first)
  expect(timeline.markers[0].id).toBe(first + 1)
  expect(typeof timeline.meters[0].id).toBe("number")
  const saved = await rig.backend.projectSave("/typed-timeline")
  const savedSnapshot = await rig.backend.documentSnapshot()
  for (const id of [0, -1, 1.5, 2 ** 32, first + 1]) {
    await expect(
      rig.backend.dispatch({ type: "removeMeterChange", id })
    ).rejects.toThrow()
    expect(await rig.backend.documentSnapshot()).toEqual(savedSnapshot)
  }
  await expect(
    rig.backend.dispatch({ type: "removeTimelineMarker", id: first })
  ).rejects.toThrow()
  await expect(
    rig.backend.dispatch({
      type: "updateTimelineMarker",
      marker: { ...timeline.markers[0], id: first },
    })
  ).rejects.toThrow()
  expect(await rig.backend.documentSnapshot()).toEqual(savedSnapshot)
  await rig.backend.dispatch({
    type: "updateMeterChange",
    id: first,
    tick: 4201,
    signature: { numerator: 3, denominator: 4 },
  })
  await rig.backend.dispatch({
    type: "updateTimelineMarker",
    marker: { ...timeline.markers[0], tick: 29, name: "Chorus" },
  })
  const changed = (await rig.backend.documentSnapshot()).project.playlist
    .timeline
  await rig.backend.dispatch({
    type: "batch",
    commands: [
      { type: "removeMeterChange", id: first },
      { type: "removeTimelineMarker", id: first + 1 },
    ],
  })
  await rig.backend.undo()
  expect(
    (await rig.backend.documentSnapshot()).project.playlist.timeline
  ).toEqual(changed)
  await rig.backend.redo()
  expect(
    (await rig.backend.documentSnapshot()).project.playlist.timeline
  ).toBeUndefined()
  await rig.backend.projectOpen(saved)
  expect(
    (await rig.backend.documentSnapshot()).project.playlist.timeline
  ).toEqual(timeline)
  const next = (await rig.backend.documentSnapshot()).project.nextId
  expect(
    (
      await rig.backend.dispatch({
        type: "addMeterChange",
        tick: 0,
        signature: { numerator: 5, denominator: 8 },
      })
    ).created
  ).toEqual([next])
})

it("cancels preview drags and refuses late region requests after edits/replacement", async () => {
  const metrics = new GridMetrics()
  const before = { ...metrics.viewport }
  const cancelled = new TimelineGesture(metrics, 10, "zoom")
  cancelled.update(200)
  cancelled.cancel()
  cancelled.finish(200)
  expect(metrics.viewport).toEqual(before)
  const edited = new TimelineGesture(metrics, 10, "select")
  await dispatch({
    type: "addMeterChange",
    tick: 17,
    signature: { numerator: 7, denominator: 8 },
  })
  edited.finish(200)
  expect(useTimelineStore.getState().selection).toBeNull()
  const state = await rig.backend.timelineState()
  await dispatch({
    type: "addTimelineMarker",
    tick: 9,
    name: "Pause",
    kind: { type: "pause" },
  })
  await expect(
    rig.backend.timelineRegion(
      { start: 1, end: 30 },
      state.generation,
      state.revision
    )
  ).rejects.toThrow(/changed/)
  await selectTimelineRegion({ start: 17, end: 80 })
  expect(await applyPlaybackRegion({ start: 17, end: 80 })).toBe(true)
  const fresh = await rig.backend.timelineState()
  await rig.backend.projectNew()
  await expect(
    rig.backend.timelineRegion(
      { start: 1, end: 30 },
      fresh.generation,
      fresh.revision
    )
  ).rejects.toThrow(/changed/)
  expect(useTimelineStore.getState().selection).toBeNull()
  expect((await rig.backend.timelineState()).region).toBeNull()
})

it("cancels actual ruler pointer gestures on Escape, focus, scale and replacement", async () => {
  await addClips(
    [
      {
        row: 0,
        start: 0,
        length: 3840,
        offset: 0,
        muted: false,
        content: {
          type: "pattern",
          pattern: useProjectStore.getState().project.patterns[0].id,
        },
      },
    ],
    "Seed selected clip"
  )
  const metrics = new GridMetrics()
  metrics.setViewport({ ...metrics.viewport, width: 800, pxPerTick: 0.1 })
  useTimelineStore.setState({ tool: "select" })
  render(<Ruler metrics={metrics} />)
  const ruler = screen.getByRole("slider", { name: "Song position" })
  vi.spyOn(ruler, "hasPointerCapture").mockReturnValue(true)
  const [clip] = useProjectStore.getState().project.playlist.clips
  act(() => usePlaylistStore.getState().select([clip.id]))
  // jsdom has no PointerEvent constructor; a MouseEvent still supplies the
  // pointer handler's coordinate/modifier fields, unlike its generic Event.
  const pointer = (type: string, x: number) =>
    fireEvent(
      ruler,
      new MouseEvent(type, { bubbles: true, clientX: x, button: 0 })
    )
  const cancellations = [
    () => {
      fireEvent.keyDown(ruler, { key: "Escape" })
      expect(usePlaylistStore.getState().selection.has(clip.id)).toBe(true)
    },
    () => fireEvent.blur(ruler),
    () => window.dispatchEvent(new Event(UI_SCALE_EVENT)),
    () => announceProjectReplaced(),
  ]
  for (const cancel of cancellations) {
    act(() => useTimelineStore.setState({ tool: "select" }))
    act(() => {
      pointer("pointerdown", 10)
      pointer("pointermove", 300)
    })
    expect(useTimelineStore.getState().draft).toEqual({ start: 100, end: 3000 })
    act(() => {
      cancel()
      pointer("pointerup", 300)
    })
    expect(useTimelineStore.getState().selection).toBeNull()
    expect(useTimelineStore.getState().draft).toBeNull()
  }
  act(() => useTimelineStore.setState({ tool: "select" }))
  act(() => {
    pointer("pointerdown", 10)
    pointer("pointermove", 300)
    pointer("pointerup", 300)
  })
  expect(useTimelineStore.getState().selection).toEqual({
    start: 100,
    end: 3000,
  })
  await settle()
})

it("exposes region actions and clears an editor when its project is replaced", async () => {
  render(<PlaylistPanel />)
  // The action registry is the command palette/context-menu authority.
  expect(registry.get("playlist.loopSelection")?.title).toBe(
    "Loop selected song region"
  )
  act(() => editTimeline({ type: "meter" }))
  expect(screen.getByRole("dialog", { name: "Add meter change" })).toBeVisible()
  await act(async () => {
    await rig.backend.projectNew()
  })
  expect(screen.queryByRole("dialog", { name: "Add meter change" })).toBeNull()
})

it("shows the native navigation overflow counter when song playback stops", async () => {
  render(<PlaylistPanel />)
  const native = await rig.backend.timelineState()
  vi.spyOn(rig.backend, "timelineState").mockResolvedValue({
    ...native,
    navigationOverflows: 1,
  })
  await act(async () => {
    useTransportStore.setState({ playing: true, mode: "song" })
    useTransportStore.setState({ playing: false })
    await settle()
  })
  expect(
    screen.getByText(/Playback reached the navigation limit/)
  ).toBeVisible()
})

it("checks canonical export pairs after edit/New/Open and keeps absent guards compatible", async () => {
  const saved = await rig.backend.projectSave("/saved-timeline")
  const legacy: ExportOptions = {
    path: "/region.wav",
    region: { start: 17, end: 839 },
    format: "wav",
    bitDepth: "float32",
    sampleRate: 48000,
    mode: "song",
    patternLoops: 1,
    tailSecs: 0,
    autoTail: false,
  }
  const guarded = async () => {
    const source = await rig.backend.timelineState()
    return {
      ...legacy,
      regionGeneration: source.generation,
      regionRevision: source.revision,
    }
  }
  await rig.backend.exportAudio(await guarded())
  await rig.backend.exportCancel()
  const changes = [
    () =>
      dispatch({
        type: "addMeterChange",
        tick: 4001,
        signature: { numerator: 7, denominator: 8 },
      }),
    () => rig.backend.projectNew(),
    () => rig.backend.projectOpen(saved),
  ]
  for (const change of changes) {
    const before = await guarded()
    await change()
    await expect(rig.backend.exportAudio(before)).rejects.toThrow(/changed/)
  }
  const source = await guarded()
  await expect(
    rig.backend.exportAudio({
      ...legacy,
      regionGeneration: source.regionGeneration,
    })
  ).rejects.toThrow(/changed/)
  await rig.backend.exportAudio(legacy)
  await rig.backend.exportCancel()
})
