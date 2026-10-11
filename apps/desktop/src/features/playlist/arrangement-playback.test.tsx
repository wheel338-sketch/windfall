import { act, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

import { emptyProject } from "@/lib/ipc/sim/project"
import { dispatch } from "@/lib/store/project"
import { settle } from "@/test/harness"

import { arrangementPlaylist } from "./arrangement-view"
import { GridMetrics } from "./metrics"
import { PlaylistScene } from "./scene"
import { BAR, FakeSurface, project, startPlaylist } from "./test-utils"
import { TrackHeaders } from "./track-headers"
import { buildTrackRows, documentRowFor } from "./track-rows"

function fixture() {
  const value = emptyProject()
  value.nextId = 120
  value.playlist = {
    tracks: [100, 101, 102].map((id) => ({
      id,
      name: `Track ${id}`,
      muted: false,
    })),
    clips: [100, 101, 102, 102].map((track, index) => ({
      id: 103 + index,
      track,
      start: index * BAR,
      length: BAR,
      offset: 120,
      muted: false,
      content: { type: "pattern", pattern: value.patterns[0].id },
    })),
    arrangementBook: {
      arrangements: [
        { id: 110, name: "Verse", tracks: [102, 100], clips: [103, 104, 105] },
        { id: 111, name: "Chorus", tracks: [101], clips: [104] },
        { id: 112, name: "No clips", tracks: [100], clips: [] },
        { id: 113, name: "No tracks", tracks: [], clips: [103] },
      ],
      active: 110,
      trackGroups: [],
      groupParents: {},
      trackParents: {},
      clipGroups: [],
      linkedTracks: {},
    },
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

function canvasClips() {
  const batch = surface.items!.batch
  return Array.from({ length: batch.count }, (_, index) => ({
    id: batch.ids[index],
    row: batch.geometry[index * 4 + 2],
  }))
}

describe("active arrangement playlist view", () => {
  it("lists only the arrangement tracks in reference order and filters canvas clips", () => {
    render(<TrackHeaders metrics={metrics} />)
    expect(screen.getByRole("group", { name: "Track 102" })).toHaveStyle({
      top: "0px",
    })
    expect(screen.getByRole("group", { name: "Track 100" })).toHaveStyle({
      top: "20px",
    })
    expect(screen.queryByRole("group", { name: "Track 101" })).toBeNull()
    // 104 is listed but its track is excluded; 106 is on a visible track
    // but is not listed in the arrangement.
    expect(canvasClips()).toEqual([
      { id: 103, row: 1 },
      { id: 105, row: 0 },
    ])
    expect(documentRowFor(0)).toBe(2)
    expect(documentRowFor(1)).toBe(0)
  })

  it("switches the visible tracks and clips without changing stored positions", async () => {
    render(<TrackHeaders metrics={metrics} />)
    const saved = structuredClone(project().playlist.clips)
    await act(async () => {
      await dispatch({ type: "switchArrangement", id: 111 })
      await settle()
    })
    expect(screen.getByRole("group", { name: "Track 101" })).toHaveStyle({
      top: "0px",
    })
    expect(screen.queryByRole("group", { name: "Track 100" })).toBeNull()
    expect(screen.queryByRole("group", { name: "Track 102" })).toBeNull()
    expect(canvasClips()).toEqual([{ id: 104, row: 0 }])
    expect(project().playlist.clips).toEqual(saved)
  })

  it("shows the lanes of an arrangement that has no clips yet, and nothing on them", async () => {
    render(<TrackHeaders metrics={metrics} />)
    await act(async () => {
      await dispatch({ type: "switchArrangement", id: 112 })
      await settle()
    })
    // A new layout has to offer its lanes, or nothing could be placed on it.
    expect(screen.getByRole("group", { name: "Track 100" })).toHaveStyle({
      top: "0px",
    })
    for (const id of [101, 102]) {
      expect(screen.queryByRole("group", { name: `Track ${id}` })).toBeNull()
    }
    expect(canvasClips()).toEqual([])
    expect(scene.songEnd).toBe(0)
  })

  it("shows nothing for an arrangement that lists no tracks", async () => {
    render(<TrackHeaders metrics={metrics} />)
    await act(async () => {
      await dispatch({ type: "switchArrangement", id: 113 })
      await settle()
    })
    for (const id of [100, 101, 102]) {
      expect(screen.queryByRole("group", { name: `Track ${id}` })).toBeNull()
    }
    expect(canvasClips()).toEqual([])
    expect(scene.songEnd).toBe(0)
  })

  it("keeps every clip and track for a missing book, empty book or no active id", () => {
    for (const mode of ["missing", "empty", "inactive"]) {
      const value = fixture().playlist
      if (mode === "missing") delete value.arrangementBook
      else if (mode === "empty") value.arrangementBook!.arrangements = []
      else value.arrangementBook!.active = null
      const view = arrangementPlaylist(value)
      expect(view.tracks.map((track) => track.id)).toEqual([100, 101, 102])
      expect(view.clips).toHaveLength(4)
      expect(buildTrackRows(value, new Set()).trackRows.size).toBe(3)
    }
  })

  it("keeps arrangement order across group boundaries and honors collapse", () => {
    const value = fixture().playlist
    const book = value.arrangementBook!
    book.trackGroups = [{ id: 114, name: "Rhythm" }]
    book.trackParents = { 100: 114, 102: 114 }
    book.arrangements[0].tracks = [102, 101, 100]
    const layout = buildTrackRows(value, new Set())
    expect(
      layout.rows
        .filter((row) => row.kind === "track")
        .map((row) => row.track.id)
    ).toEqual([102, 101, 100])
    expect(
      buildTrackRows(value, new Set([114]))
        .rows.filter((row) => row.kind === "track")
        .map((row) => row.track.id)
    ).toEqual([101])
  })
})
