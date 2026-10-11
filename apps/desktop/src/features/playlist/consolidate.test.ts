import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { Project } from "@/bindings"
import { getAppState, isEnabled, registry, runAction } from "@/lib/actions"
import { SimDocument } from "@/lib/ipc/sim/document"
import { consolidatePlan } from "@/lib/ipc/sim/playlist"
import { demoProject } from "@/lib/ipc/sim/project"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"

import { registerPlaylistActions } from "./actions"
import { consolidatableTracks, consolidateTracks } from "./consolidate"
import { usePlaylistStore } from "./store"

/*
 * Track consolidation through the playlist actions and the real document.
 * A fixture file stands in for the native render; the batch, the muting,
 * the undo step and every refusal are the real thing.
 */

const FILE = "/factory/Drums/Kicks/Kick 02.wav"

let app: Awaited<ReturnType<typeof startTestApp>>
let unregister: () => void
let source: Project
const render =
  vi.fn<
    (
      project: Project,
      track: number,
      region: { start: number; end: number }
    ) => Promise<string>
  >()

beforeEach(async () => {
  source = demoProject()
  source.nextId = 1000
  source.playlist.tracks = [
    { id: 900, name: "Top", muted: true, solo: false },
    { id: 901, name: "Bottom", muted: false, solo: true },
    { id: 902, name: "Spare", muted: false, solo: false },
  ]
  const pattern = { type: "pattern" as const, pattern: source.patterns[0].id }
  source.playlist.clips = [
    { id: 910, track: 901, start: 960, length: 960, offset: 0, muted: false, content: pattern },
    {
      id: 911,
      track: 900,
      start: 1440,
      length: 960,
      offset: 0,
      muted: false,
      content: {
        type: "audio",
        sample: source.samples[0].id,
        mixerTrack: source.mixer.tracks[0].id,
        gain: 1,
        pan: 0,
        fadeIn: 0,
        fadeOut: 0,
        reverse: false,
        pitch: 0,
      },
    },
    { id: 912, track: 901, start: 2880, length: 960, offset: 0, muted: true, content: pattern },
    { id: 914, track: 901, start: 4800, length: 960, offset: 0, muted: false, content: pattern },
  ]
  render.mockReset().mockResolvedValue(FILE)
  app = await startTestApp({ project: source, renderPlaylistTrack: render })
  unregister = registerPlaylistActions()
  usePlaylistStore.setState(usePlaylistStore.getInitialState(), true)
  useUiStore.getState().showCenterTab("playlist")
})

afterEach(() => {
  unregister()
  app.stop()
  vi.restoreAllMocks()
})

const project = () => useProjectStore.getState().project
const clip = (id: number) => project().playlist.clips.find((item) => item.id === id)!
const enabled = (id: string) => isEnabled(registry.get(id)!, getAppState())

it("prints one track from its header, mutes its sources and selects the print", async () => {
  usePlaylistStore.getState().setTargetTrack(901)
  expect(enabled("playlist.consolidateTrack")).toBe(true)
  const send = vi.spyOn(SimDocument.prototype, "dispatch")
  const tracks = structuredClone(project().playlist.tracks)

  await runAction("playlist.consolidateTrack")

  // The render copy: the lane's unmuted sound alone, with no solo in use.
  expect(render).toHaveBeenCalledOnce()
  const [copy, lane, span] = render.mock.calls[0]
  expect(lane).toBe(901)
  expect(span).toEqual({ start: 960, end: 5760 })
  expect(copy.playlist.tracks.map((track) => [track.solo, track.muted])).toEqual([
    [false, true],
    [false, false],
    [false, false],
  ])
  expect(copy.playlist.clips.map((item) => [item.id, item.muted])).toEqual([
    [910, false],
    [911, true],
    [912, true],
    [914, false],
  ])

  expect(send).toHaveBeenCalledOnce()
  expect(send.mock.calls[0][0]).toMatchObject({
    type: "batch",
    label: "Consolidate track",
    commands: [
      { type: "addSample", name: "Bottom (consolidated)" },
      {
        type: "addClips",
        clips: [
          {
            track: 901,
            start: 960,
            length: 4800,
            muted: false,
            content: { type: "audio", output: "direct" },
          },
        ],
      },
      {
        type: "updateClips",
        updates: [
          { id: 910, patch: { muted: true } },
          { id: 914, patch: { muted: true } },
        ],
      },
    ],
  })

  const { history } = useProjectStore.getState()
  expect(history.entries.at(-1)?.label).toBe("Consolidate track")
  expect(project().playlist.tracks).toEqual(tracks)
  expect([910, 912, 914].map((id) => clip(id).muted)).toEqual([true, true, true])
  expect(clip(911)).toEqual(source.playlist.clips[1])
  const print = project().playlist.clips.find(
    (item) => ![910, 911, 912, 914].includes(item.id)
  )!
  expect(print).toMatchObject({
    track: 901,
    start: 960,
    length: 4800,
    muted: false,
    content: { type: "audio", output: "direct" },
  })
  expect(usePlaylistStore.getState().selection).toEqual(new Set([print.id]))

  await app.backend.undo()
  expect(project().playlist.clips).toEqual(source.playlist.clips)
})

it("prints the tracks of the selected clips apart, in one undo step", async () => {
  usePlaylistStore.getState().select([910, 911])
  expect(enabled("playlist.consolidateSelectedTracks")).toBe(true)
  const send = vi.spyOn(SimDocument.prototype, "dispatch")
  const before = useProjectStore.getState().history.cursor

  await runAction("playlist.consolidateSelectedTracks")

  // One render for each lane, in playlist order.
  expect(render.mock.calls.map(([, lane, span]) => [lane, span])).toEqual([
    [900, { start: 1440, end: 2400 }],
    [901, { start: 960, end: 5760 }],
  ])
  expect(send).toHaveBeenCalledOnce()
  const batch = send.mock.calls[0][0]
  expect(batch).toMatchObject({ type: "batch", label: "Consolidate tracks" })
  if (batch.type !== "batch") throw new Error("not a batch")
  const placed = batch.commands.flatMap((command) =>
    command.type === "addClips" ? command.clips : []
  )
  expect(placed).toMatchObject([
    { track: 900, start: 1440, length: 960, content: { output: "direct" } },
    { track: 901, start: 960, length: 4800, content: { output: "direct" } },
  ])
  expect(batch.commands.at(-1)).toEqual({
    type: "updateClips",
    updates: [911, 910, 914].map((id) => ({ id, patch: { muted: true } })),
  })
  expect(useProjectStore.getState().history.cursor).toBe(before + 1)
  expect([910, 911, 914].map((id) => clip(id).muted)).toEqual([true, true, true])
  const prints = project().playlist.clips.filter(
    (item) => ![910, 911, 912, 914].includes(item.id)
  )
  expect(prints.map((item) => item.track).sort()).toEqual([900, 901])
  expect(usePlaylistStore.getState().selection).toEqual(
    new Set(prints.map((item) => item.id))
  )

  await app.backend.undo()
  expect(project().playlist.clips).toEqual(source.playlist.clips)
})

it("offers nothing for a track with no unmuted pattern or audio clip", async () => {
  usePlaylistStore.getState().setTargetTrack(902)
  expect(enabled("playlist.consolidateTrack")).toBe(false)
  expect(consolidatableTracks([900, 901, 902])).toEqual([900, 901])
  const send = vi.spyOn(SimDocument.prototype, "dispatch")
  await consolidateTracks([902])
  await runAction("playlist.consolidateTrack")
  expect(render).not.toHaveBeenCalled()
  expect(send).not.toHaveBeenCalled()
  // The backend refuses the same request, for every track asked with it.
  await expect(
    app.backend.consolidatePlaylistTracks([901, 902])
  ).rejects.toThrow('"Spare" has no unmuted pattern or audio clip to consolidate.')
  await expect(app.backend.consolidatePlaylistTracks([])).rejects.toThrow(
    "Choose at least one playlist track to consolidate."
  )
  await expect(app.backend.consolidatePlaylistTracks([999])).rejects.toThrow(
    "A chosen playlist track no longer exists."
  )
  expect(send).not.toHaveBeenCalled()
})

it("changes nothing after a failed render or an edit made while rendering", async () => {
  const before = await app.backend.documentSnapshot()
  render.mockRejectedValueOnce(new Error("Renderer failed"))
  const send = vi.spyOn(SimDocument.prototype, "dispatch")
  await consolidateTracks([901])
  expect(send).not.toHaveBeenCalled()
  expect(await app.backend.documentSnapshot()).toEqual(before)

  render.mockImplementationOnce(async () => {
    await app.backend.dispatch({ type: "addPlaylistTrack" })
    return FILE
  })
  await consolidateTracks([901])
  expect(send).toHaveBeenCalledExactlyOnceWith(
    { type: "addPlaylistTrack" },
    undefined
  )
  expect(project().playlist.clips).toEqual(source.playlist.clips)
})

it("follows the active arrangement and enrolls the print in it", async () => {
  await app.backend.dispatch({
    type: "addArrangementLayout",
    name: "Short",
    copy: true,
  })
  await vi.waitFor(() =>
    expect(project().playlist.arrangementBook?.arrangements).toHaveLength(2)
  )
  const layout = project().playlist.arrangementBook!.arrangements[1]
  // The copy of clip 914 is deleted in the new layout only.
  const late = project().playlist.clips.find(
    (item) => layout.clips.includes(item.id) && item.start === 4800
  )!
  await app.backend.dispatch({ type: "removeClips", clips: [late.id] })
  await vi.waitFor(() => expect(project().playlist.clips).toHaveLength(7))

  const plan = consolidatePlan(project(), [901])
  expect(plan).toHaveLength(1)
  expect(plan[0]).toMatchObject({ track: 901, start: 960, end: 1920 })
  expect(plan[0].sources).toHaveLength(1)
  expect(layout.clips).toContain(plan[0].sources[0])

  await consolidateTracks([901])
  expect(render.mock.calls[0][2]).toEqual({ start: 960, end: 1920 })
  const book = project().playlist.arrangementBook!
  const [print] = [...usePlaylistStore.getState().selection]
  expect(book.arrangements[1].clips).toContain(print)
  expect(book.arrangements[0].clips).not.toContain(print)
  // The first layout's own clips were neither rendered nor muted.
  expect([910, 914].map((id) => clip(id).muted)).toEqual([false, false])
})

it("browser preview refuses consolidation without a native render fixture", async () => {
  unregister()
  app.stop()
  app = await startTestApp({ project: source })
  unregister = registerPlaylistActions()
  const before = await app.backend.documentSnapshot()
  await expect(app.backend.consolidatePlaylistTracks([901])).rejects.toThrow(
    "offline renderer"
  )
  expect(await app.backend.documentSnapshot()).toEqual(before)
})
