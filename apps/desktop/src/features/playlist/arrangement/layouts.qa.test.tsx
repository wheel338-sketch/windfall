import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

import type { ClipId } from "@/bindings"
import { emptyProject } from "@/lib/ipc/sim/project"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { settle } from "@/test/harness"

import { GridMetrics } from "../metrics"
import { PlaylistScene } from "../scene"
import { BAR, FakeSurface, project, startPlaylist } from "../test-utils"
import { TrackHeaders } from "../track-headers"
import { ArrangementControls } from "./controls"

/*
 * Alternative timeline layouts through the mounted arrangement controls and
 * the real document: a layout owns its clips, so what is moved, placed or
 * deleted in one leaves the others as they were.
 */

/** A project from before arrangements: two lanes, a pattern clip on each. */
function fixture() {
  const value = emptyProject()
  value.nextId = 120
  value.playlist = {
    tracks: [100, 101].map((id) => ({ id, name: `Track ${id}`, muted: false })),
    clips: [100, 101].map((track, index) => ({
      id: 103 + index,
      track,
      start: index * BAR,
      length: BAR,
      offset: 0,
      muted: false,
      content: { type: "pattern" as const, pattern: value.patterns[0].id },
    })),
  }
  return value
}

let app: Awaited<ReturnType<typeof startPlaylist>>
let scene: PlaylistScene
let surface: FakeSurface
let metrics: GridMetrics
let detach: () => void

beforeEach(async () => {
  app = await startPlaylist({ project: fixture() })
  surface = new FakeSurface()
  metrics = new GridMetrics()
  detach = metrics.attach(surface)
  scene = new PlaylistScene(surface, metrics)
})

afterEach(() => {
  scene.destroy()
  detach()
  app.stop()
})

const book = () => project().playlist.arrangementBook
const clip = (id: ClipId) => {
  const found = project().playlist.clips.find((item) => item.id === id)
  if (!found) throw new Error(`No clip ${id}`)
  return found
}
/** The clips the playlist canvas shows, in timeline order. */
const shown = () => scene.clips.map((item) => item.id)
const headers = () =>
  screen.queryAllByRole("group").map((group) => group.getAttribute("aria-label"))
const history = () => useProjectStore.getState().history

async function send(command: Parameters<typeof dispatch>[0]) {
  const box: { result: Awaited<ReturnType<typeof dispatch>> } = { result: null }
  await act(async () => {
    box.result = await dispatch(command)
    await settle()
  })
  if (!box.result) throw new Error("The command was refused")
  return box.result.created
}

function mount() {
  render(
    <>
      <ArrangementControls />
      <TrackHeaders metrics={metrics} />
    </>
  )
  fireEvent.click(screen.getByRole("button", { name: /^Arrangements/ }))
}

async function addLayout(name: string, button: string) {
  const count = book()?.arrangements.length ?? 0
  fireEvent.change(screen.getByLabelText("New layout name"), {
    target: { value: name },
  })
  fireEvent.click(screen.getByRole("button", { name: button }))
  await waitFor(() =>
    expect(book()?.arrangements.length ?? 0).toBeGreaterThan(count)
  )
  await act(async () => {
    await settle()
  })
}

describe("independent arrangement layouts", () => {
  it("duplicates the timeline into a layout whose clips move on their own", async () => {
    mount()
    const before = history().cursor
    await addLayout("Variation", "Duplicate layout")

    // The timeline as it was became the first layout; the copy is active.
    const [first, copy] = book()!.arrangements
    expect(first).toMatchObject({
      name: "Arrangement 1",
      clips: [103, 104],
      tracks: [100, 101],
    })
    expect(copy.name).toBe("Variation")
    expect(book()!.active).toBe(copy.id)
    expect(copy.tracks).toEqual([100, 101])
    expect(copy.clips).toHaveLength(2)
    expect(copy.clips).not.toContain(103)
    expect(copy.clips).not.toContain(104)
    for (const [index, id] of copy.clips.entries()) {
      expect(clip(id)).toEqual({ ...clip(103 + index), id })
    }
    expect(history().cursor).toBe(before + 1)
    expect(history().entries.at(-1)?.label).toBe("Add arrangement")
    expect(shown()).toEqual(copy.clips)

    // Moving, resizing and deleting in the copy leaves the first layout alone.
    const [movedId, removedId] = copy.clips
    await send({
      type: "updateClips",
      updates: [
        { id: movedId, patch: { start: 3 * BAR, length: BAR / 2, track: 101 } },
      ],
    })
    await send({ type: "removeClips", clips: [removedId] })
    expect(clip(103)).toMatchObject({ start: 0, length: BAR, track: 100 })
    expect(clip(104)).toMatchObject({ start: BAR, length: BAR, track: 101 })
    expect(clip(movedId)).toMatchObject({
      start: 3 * BAR,
      length: BAR / 2,
      track: 101,
    })
    expect(shown()).toEqual([movedId])
    expect(scene.songEnd).toBe(3 * BAR + BAR / 2)

    // The first layout is still there, as it was, one switch away.
    await send({ type: "switchArrangement", id: first.id })
    expect(shown()).toEqual([103, 104])
    expect(scene.songEnd).toBe(2 * BAR)
    await send({ type: "updateClips", updates: [{ id: 104, patch: { start: 4 * BAR } }] })
    expect(clip(movedId).start).toBe(3 * BAR)

    // Both layouts are saved with the project and come back on reopen.
    const saved = structuredClone(project().playlist)
    const path = await app.backend.projectSave("/projects/layouts.windfall")
    await act(async () => {
      await app.backend.projectNew()
      await settle()
    })
    expect(project().playlist.arrangementBook).toBeUndefined()
    await act(async () => {
      await app.backend.projectOpen(path)
      await settle()
    })
    expect(project().playlist).toEqual(saved)
    expect(shown()).toEqual([103, 104])
  })

  it("starts an empty layout with the same lanes, and what is placed there stays there", async () => {
    mount()
    await addLayout("Bridge", "New empty layout")
    const [first, empty] = book()!.arrangements
    expect(empty).toMatchObject({ name: "Bridge", clips: [], tracks: [100, 101] })
    expect(book()!.active).toBe(empty.id)
    // Nothing plays yet, and the lanes are there to place on.
    expect(shown()).toEqual([])
    expect(scene.songEnd).toBe(0)
    expect(headers()).toEqual(expect.arrayContaining(["Track 100", "Track 101"]))
    expect(project().playlist.clips.map((item) => item.id)).toEqual([103, 104])

    const [placed] = await send({
      type: "addClips",
      clips: [
        {
          track: 101,
          start: BAR,
          length: BAR,
          content: { type: "pattern", pattern: project().patterns[0].id },
        },
      ],
    })
    expect(book()!.arrangements[1].clips).toEqual([placed])
    expect(book()!.arrangements[0].clips).toEqual([103, 104])
    expect(shown()).toEqual([placed])

    await send({ type: "switchArrangement", id: first.id })
    expect(shown()).toEqual([103, 104])

    // Deleting a layout takes the clips only it placed, in one undo step.
    await send({ type: "removeArrangement", id: empty.id })
    expect(project().playlist.clips.map((item) => item.id)).toEqual([103, 104])
    expect(book()!.arrangements).toHaveLength(1)
    await act(async () => {
      await undo()
      await settle()
    })
    expect(book()!.arrangements).toHaveLength(2)
    expect(clip(placed)).toMatchObject({ track: 101, start: BAR })
  })

  it("one undo takes a new layout and its copied clips back out", async () => {
    mount()
    const saved = structuredClone(project().playlist)
    await addLayout("Variation", "Duplicate layout")
    expect(project().playlist.clips).toHaveLength(4)
    await act(async () => {
      await undo()
      await settle()
    })
    expect(project().playlist).toEqual(saved)
    expect(shown()).toEqual([103, 104])
    expect(headers()).toEqual(expect.arrayContaining(["Track 100", "Track 101"]))
  })

  it("refuses a blank name and changes nothing", async () => {
    mount()
    const saved = structuredClone(project().playlist)
    fireEvent.change(screen.getByLabelText("New layout name"), {
      target: { value: "   " },
    })
    fireEvent.click(screen.getByRole("button", { name: "New empty layout" }))
    expect(
      await screen.findByText("The arrangement edit was not applied.")
    ).toBeInTheDocument()
    expect(project().playlist).toEqual(saved)
  })
})
