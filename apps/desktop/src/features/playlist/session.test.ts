import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { dispatch, redo, undo } from "@/lib/store/project"
import { setTransportPattern, useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import { addAudioFile } from "./audio/ops"
import { MIN_ROWS, SPARE_ROWS } from "./layout"
import { addClips } from "./ops"
import type { PlaylistSession } from "./session"
import {
  at,
  BAR,
  BEAT,
  click,
  clips,
  drag,
  FakeSurface,
  history,
  labels,
  layout,
  project,
  selection,
  startPlaylist,
  startSession,
  STEP,
  tracks,
  ui,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let session: PlaylistSession
let surface: FakeSurface
let clock: { time: number }
let stop: () => void

beforeEach(async () => {
  const app = await startPlaylist()
  const started = startSession()
  ;({ session, surface, clock } = started)
  stop = () => {
    started.stop()
    app.stop()
  }
})
afterEach(() => stop())

const pattern = () => project().patterns[0]

/** Puts clips of the first pattern on the timeline, outside the history under test. */
async function seed(...spots: [row: number, start: number, length?: number][]) {
  const created = await addClips(
    spots.map(([row, start, length = BAR]) => ({
      row,
      start,
      length,
      offset: 0,
      muted: false,
      content: { type: "pattern", pattern: pattern().id },
    })),
    "Seed"
  )
  await settle()
  return created ?? []
}

const steps = () => history().entries.length

describe("the grid it sets up", () => {
  it("has rows to place clips on in a project with no tracks", () => {
    expect(tracks()).toHaveLength(0)
    expect(surface.limits.rowCount).toBe(
      Math.max(MIN_ROWS, Math.ceil(400 / 20))
    )
    expect(surface.rows?.shaded?.every((flag) => flag === 1)).toBe(true)
  })

  it("keeps spare rows below the tracks and shades them and muted tracks", async () => {
    for (let index = 0; index < 30; index++) {
      await dispatch({ type: "addPlaylistTrack" })
    }
    await dispatch({
      type: "updatePlaylistTrack",
      id: tracks()[3].id,
      patch: { muted: true },
    })
    expect(surface.limits.rowCount).toBe(30 + SPARE_ROWS)
    const shaded = [...(surface.rows?.shaded ?? [])]
    expect(shaded.slice(0, 5)).toEqual([0, 0, 0, 1, 0])
    expect(shaded.slice(30)).toEqual(Array(SPARE_ROWS).fill(1))
  })

  it("extends the timeline past the last clip", async () => {
    await seed([0, 200 * BAR])
    expect(surface.limits.contentTicks).toBeGreaterThan(201 * BAR)
  })

  it("draws grid lines that match the snap", () => {
    expect(surface.timeGrid?.ticksPerStep).toBe(BAR)
    ui().setSnap("beat")
    expect(surface.timeGrid?.ticksPerStep).toBe(BEAT)
  })

  it("rebuilds the batch once per edit and not for a selection", async () => {
    const [first] = await seed([0, 0], [1, BAR])
    const before = surface.rebuilds
    ui().select([first])
    expect(surface.rebuilds).toBe(before)
    expect(surface.items?.batch.selectedCount).toBe(1)
    await dispatch({
      type: "updateClips",
      updates: [{ id: first, patch: { start: 2 * BAR } }],
    })
    expect(surface.rebuilds).toBe(before + 1)
  })
})

describe("Draw tool", () => {
  it("places the selected pattern in the cell that was clicked", async () => {
    await click(session, at(2 * BAR + 900, 0))
    expect(clips()).toEqual([
      expect.objectContaining({
        row: 0,
        start: 2 * BAR,
        length: BAR,
        offset: 0,
        pattern: pattern().id,
      }),
    ])
  })

  it("makes the track and the clip in one undo step", async () => {
    await click(session, at(BAR, 0))
    expect(tracks().map((track) => track.name)).toEqual(["Track 1"])
    expect(labels()).toEqual(["Add clip"])
    await undo()
    expect(tracks()).toHaveLength(0)
    expect(clips()).toHaveLength(0)
    await redo()
    expect(layout()).toEqual([`0:${BAR}+${BAR}`])
  })

  it("makes every missing track down to the row, still in one step", async () => {
    await click(session, at(0, 3))
    expect(tracks().map((track) => track.name)).toEqual([
      "Track 1",
      "Track 2",
      "Track 3",
      "Track 4",
    ])
    expect(layout()).toEqual([`3:0+${BAR}`])
    expect(steps()).toBe(1)
    await undo()
    expect(tracks()).toHaveLength(0)
  })

  it("adds no track when the row has one", async () => {
    await seed([1, 0])
    const before = steps()
    await click(session, at(4 * BAR, 0))
    expect(tracks()).toHaveLength(2)
    expect(steps()).toBe(before + 1)
  })

  it("places the pattern that is selected app-wide, at its own length", async () => {
    const added = await dispatch({ type: "addPattern", name: "Long" })
    const long = added!.created[0]
    await dispatch({
      type: "updatePattern",
      id: long,
      patch: { lengthSteps: 32 },
    })
    await setTransportPattern(long)
    await click(session, at(BAR, 1))
    expect(clips()[0]).toMatchObject({ pattern: long, length: 2 * BAR })
  })

  it("follows the pointer until the button is released", async () => {
    session.pointerDown(at(0, 0))
    session.pointerMove(at(5 * BAR + 10, 2))
    expect(session.ghosts).toEqual([
      expect.objectContaining({ row: 2, start: 5 * BAR }),
    ])
    expect(clips()).toHaveLength(0)
    await session.pointerUp(at(5 * BAR + 10, 2))
    expect(layout()).toEqual([`2:${5 * BAR}+${BAR}`])
    expect(session.ghosts).toEqual([])
  })

  it("places next to a clip when pressed just past its end", async () => {
    await seed([0, 0])
    await click(session, at(BAR + 40, 0))
    expect(layout()).toEqual([`0:0+${BAR}`, `0:${BAR}+${BAR}`])
  })

  it("places off the grid with Alt held", async () => {
    await click(session, at(BAR + 500, 0, { alt: true }))
    expect(clips()[0].start).toBe(BAR + 500)
  })

  it("moves a clip, also to another track, in one undo step", async () => {
    await seed([0, 0], [2, 0])
    const before = steps()
    await drag(session, at(BAR / 2, 0), at(BAR / 2 + 3 * BAR + 300, 1))
    expect(layout()).toEqual([`2:0+${BAR}`, `1:${3 * BAR}+${BAR}`])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Move clip")
    await undo()
    expect(layout()).toEqual([`0:0+${BAR}`, `2:0+${BAR}`])
  })

  it("shows a move as an offset on the canvas until the drop", async () => {
    await seed([0, 0])
    const rebuilds = surface.rebuilds
    session.pointerDown(at(BAR / 2, 0))
    session.pointerMove(at(BAR / 2 + 2 * BAR, 3))
    expect(surface.drag).toEqual({ ticks: 2 * BAR, rows: 3 })
    expect(surface.rebuilds).toBe(rebuilds)
    expect(layout()).toEqual([`0:0+${BAR}`])
    await session.pointerUp(at(BAR / 2 + 2 * BAR, 3))
    expect(surface.drag).toEqual({ ticks: 0, rows: 0 })
    expect(layout()).toEqual([`3:${2 * BAR}+${BAR}`])
  })

  it("moves the whole selection and makes tracks for rows without one", async () => {
    const ids = await seed([0, 0], [1, BAR])
    ui().select(ids)
    const before = steps()
    await drag(session, at(BAR / 2, 0), at(BAR / 2 + BAR, 4))
    expect(layout()).toEqual([`4:${BAR}+${BAR}`, `5:${2 * BAR}+${BAR}`])
    expect(tracks()).toHaveLength(6)
    expect(steps()).toBe(before + 1)
    await undo()
    expect(tracks()).toHaveLength(2)
    expect(layout()).toEqual([`0:0+${BAR}`, `1:${BAR}+${BAR}`])
  })

  it("does not move on a click that barely moves", async () => {
    await seed([0, BAR])
    const before = steps()
    session.pointerDown(at(BAR + 200, 0))
    session.pointerMove({ ...at(BAR + 200, 0), x: (BAR + 200) * 0.025 + 2 })
    await session.pointerUp(at(BAR + 200, 0))
    expect(steps()).toBe(before)
  })

  it("stops a move at the start of the song and at the top row", async () => {
    await seed([1, BAR])
    await drag(session, at(BAR + BAR / 2, 1), at(-9 * BAR, -5))
    expect(layout()).toEqual([`0:0+${BAR}`])
  })

  it("moves the selection on Shift+drag and copies nothing", async () => {
    const [first, second] = await seed([0, 0], [1, 2 * BAR])
    ui().select([first])
    const before = steps()
    // Shift adds the pressed clip to the selection. It never means "copy".
    await drag(
      session,
      at(2 * BAR + BAR / 2, 1, { shift: true }),
      at(3 * BAR + BAR / 2, 1, { shift: true })
    )
    expect(clips()).toHaveLength(2)
    expect(layout()).toEqual([`0:${BAR}+${BAR}`, `1:${3 * BAR}+${BAR}`])
    expect(selection()).toEqual([first, second])
    expect(labels().at(-1)).toBe("Move clips")
    expect(steps()).toBe(before + 1)
  })

  it("copies only the clip that is dragged with Ctrl, not the rest of the selection", async () => {
    const [first, second] = await seed([0, 0], [1, 2 * BAR])
    ui().select([first])
    // Ctrl from the press on: the clip under the pointer is the one copied.
    await drag(
      session,
      at(2 * BAR + BAR / 2, 1, { mod: true }),
      at(5 * BAR + BAR / 2, 1, { mod: true })
    )
    expect(layout()).toEqual([
      `0:0+${BAR}`,
      `1:${2 * BAR}+${BAR}`,
      `1:${5 * BAR}+${BAR}`,
    ])
    expect(labels().at(-1)).toBe("Clone clip")
    const copy = clips().find((clip) => clip.start === 5 * BAR)
    expect(selection()).toEqual([copy?.id])
    expect(selection()).not.toContain(second)
  })

  it("copies the whole selection when one of its clips is dragged with Ctrl", async () => {
    const ids = await seed([0, 0], [1, 2 * BAR])
    ui().select(ids)
    await drag(
      session,
      at(BAR / 2, 0, { mod: true }),
      at(BAR / 2 + 4 * BAR, 0, { mod: true })
    )
    expect(clips()).toHaveLength(4)
    expect(labels().at(-1)).toBe("Clone clips")
  })

  it("copies instead of moving when Ctrl is held at the drop", async () => {
    const [original] = await seed([0, 0, 2 * BAR])
    await dispatch({
      type: "updateClips",
      updates: [{ id: original, patch: { offset: STEP, muted: true } }],
    })
    const before = steps()
    await drag(session, at(BAR / 2, 0), at(BAR / 2 + 4 * BAR, 1, { mod: true }))
    expect(clips()).toEqual([
      expect.objectContaining({ id: original, row: 0, start: 0 }),
      expect.objectContaining({
        row: 1,
        start: 4 * BAR,
        length: 2 * BAR,
        offset: STEP,
        muted: true,
      }),
    ])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Clone clip")
    expect(selection()).toEqual([clips()[1].id])
    await undo()
    expect(clips()).toHaveLength(1)
  })

  it("resizes from the right edge, past the pattern's length", async () => {
    await seed([0, 0])
    const before = steps()
    await drag(session, at(BAR - 40, 0), at(4 * BAR + 100, 0))
    expect(layout()).toEqual([`0:0+${4 * BAR}`])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Resize clip")
  })

  it("shows a resize on the canvas until the drop", async () => {
    await seed([0, 0, 4 * BAR])
    session.pointerDown(at(4 * BAR - 40, 0))
    session.pointerMove(at(2 * BAR + 10, 0))
    expect(surface.resize).toEqual({ start: 0, end: -2 * BAR, minLength: BAR })
    expect(layout()).toEqual([`0:0+${4 * BAR}`])
    await session.pointerUp(at(2 * BAR + 10, 0))
    expect(surface.resize).toEqual({ start: 0, end: 0, minLength: 0 })
    expect(layout()).toEqual([`0:0+${2 * BAR}`])
  })

  it("does not shrink a clip below one grid cell", async () => {
    await seed([0, 0, 4 * BAR])
    await drag(session, at(4 * BAR - 40, 0), at(100, 0))
    expect(layout()).toEqual([`0:0+${BAR}`])
  })

  it("trims from the left edge and keeps the notes in place", async () => {
    await seed([0, 2 * BAR, 4 * BAR])
    ui().setSnap("beat")
    const before = steps()
    await drag(session, at(2 * BAR + 40, 0), at(2 * BAR + 3 * BEAT + 30, 0))
    expect(clips()[0]).toMatchObject({
      start: 2 * BAR + 3 * BEAT,
      length: 4 * BAR - 3 * BEAT,
      offset: 3 * BEAT,
    })
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Trim clip")
  })

  it("wraps the offset when the left edge is pulled out", async () => {
    await seed([0, 2 * BAR, BAR])
    ui().setSnap("beat")
    await drag(session, at(2 * BAR + 40, 0), at(2 * BAR - BEAT - 20, 0))
    expect(clips()[0]).toMatchObject({
      start: 2 * BAR - BEAT,
      length: BAR + BEAT,
      offset: BAR - BEAT,
    })
  })

  it("resizes every selected clip together", async () => {
    const ids = await seed([0, 0], [1, 0, 2 * BAR])
    ui().select(ids)
    const before = steps()
    await drag(session, at(BAR - 40, 0), at(2 * BAR + 20, 0))
    expect(layout()).toEqual([`0:0+${2 * BAR}`, `1:0+${3 * BAR}`])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Resize clips")
  })

  it("deletes with a right-click", async () => {
    await seed([0, 0], [0, BAR])
    const before = steps()
    await click(session, at(BAR + 100, 0, { button: 2 }))
    expect(layout()).toEqual([`0:0+${BAR}`])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Delete clip")
  })

  it("opens the clip menu instead when right-click is set not to delete", async () => {
    const [first] = await seed([0, 0], [0, BAR])
    ui().toggleRightClickDeletes()
    const before = steps()
    await click(session, at(100, 0, { button: 2 }))
    expect(layout()).toEqual([`0:0+${BAR}`, `0:${BAR}+${BAR}`])
    expect(steps()).toBe(before)
    expect(selection()).toEqual([first])
    expect(ui().menuOnClips).toBe(true)
  })

  it("deletes everything a right-drag crosses, in one undo step", async () => {
    await seed([0, 0], [0, BAR], [0, 2 * BAR], [1, 0], [3, 0])
    const before = steps()
    await drag(
      session,
      at(100, 0, { button: 2 }),
      at(2 * BAR + 100, 0, { button: 2 })
    )
    expect(layout()).toEqual([`1:0+${BAR}`, `3:0+${BAR}`])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Delete clips")
    await undo()
    expect(clips()).toHaveLength(5)
  })

  it("leaves overlapped clips alone", async () => {
    await seed([0, 0, 4 * BAR], [1, BAR])
    await drag(session, at(BAR + BAR / 2, 1), at(BAR + BAR / 2, 0))
    expect(layout()).toEqual([`0:0+${4 * BAR}`, `0:${BAR}+${BAR}`])
  })

  it("selects with a box on Ctrl+drag from empty grid", async () => {
    const ids = await seed([0, 0], [1, 2 * BAR], [5, 9 * BAR])
    await drag(
      session,
      at(3 * BAR + 10, 2, { mod: true }),
      at(100, 0, { mod: true })
    )
    expect(selection()).toEqual(ids.slice(0, 2))
    expect(clips()).toHaveLength(3)
  })
})

describe("brush meter length", () => {
  async function pickBarBrush() {
    const made = await dispatch({
      type: "addAutomation",
      target: { type: "trackVolume", track: project().mixer.tracks[0].id },
      points: [{ tick: 0, value: 0.5, curve: 0, hold: false }],
    })
    ui().setBrush({ type: "automation", automation: made!.created[0] })
  }

  it.each(["draw", "paint"] as const)(
    "uses a 7/8 bar at the later paint tick with %s",
    async (tool) => {
      await pickBarBrush()
      await dispatch({
        type: "addMeterChange",
        tick: 2 * BAR,
        signature: { numerator: 7, denominator: 8 },
      })
      ui().setTool(tool)
      const later = at(2 * BAR, 0)
      session.pointerDown(later)
      expect(session.ghosts[0]).toMatchObject({
        start: 2 * BAR,
        length: (7 * BEAT) / 2,
      })
      await session.pointerUp(later)
      await settle()
      expect(clips()[0]).toMatchObject({
        start: 2 * BAR,
        length: (7 * BEAT) / 2,
      })
    }
  )

  it("uses the project signature before the first meter change", async () => {
    await pickBarBrush()
    await dispatch({
      type: "addMeterChange",
      tick: 2 * BAR,
      signature: { numerator: 7, denominator: 8 },
    })
    ui().setTool("paint")
    await click(session, at(BAR, 0))
    expect(clips()[0]).toMatchObject({ start: BAR, length: BAR })
  })

  it("keeps the old bar length and spacing with no meter changes", async () => {
    await pickBarBrush()
    ui().setTool("paint")
    await drag(session, at(BAR + 100, 0), at(3 * BAR + 100, 0))
    expect(layout()).toEqual([
      `0:${BAR}+${BAR}`,
      `0:${2 * BAR}+${BAR}`,
      `0:${3 * BAR}+${BAR}`,
    ])
  })

  it.each(["right", "left"] as const)(
    "uses each clip's local bar length in a stroke crossing meters to the %s",
    async (direction) => {
      await pickBarBrush()
      await dispatch({
        type: "addMeterChange",
        tick: 2 * BAR,
        signature: { numerator: 7, denominator: 8 },
      })
      ui().setTool("paint")
      ui().setSnap("none")
      const shortBar = (7 * BEAT) / 2
      const first = at(BAR, 0)
      const last = at(2 * BAR + 2 * shortBar, 0)
      await drag(
        session,
        direction === "right" ? first : last,
        direction === "right" ? last : first
      )
      expect(layout()).toEqual([
        `0:${BAR}+${BAR}`,
        `0:${2 * BAR}+${shortBar}`,
        `0:${2 * BAR + shortBar}+${shortBar}`,
        `0:${2 * BAR + 2 * shortBar}+${shortBar}`,
      ])
    }
  )
})

describe("Paint tool", () => {
  beforeEach(() => ui().setTool("paint"))

  it("paints clips back to back in one undo step", async () => {
    await drag(session, at(BAR + 100, 1), at(4 * BAR + 100, 1))
    expect(layout()).toEqual([
      `1:${BAR}+${BAR}`,
      `1:${2 * BAR}+${BAR}`,
      `1:${3 * BAR}+${BAR}`,
      `1:${4 * BAR}+${BAR}`,
    ])
    expect(tracks()).toHaveLength(2)
    expect(labels()).toEqual(["Paint clips"])
    await undo()
    expect(clips()).toHaveLength(0)
    expect(tracks()).toHaveLength(0)
  })

  it("steps by the pattern's length, not by the grid", async () => {
    await dispatch({
      type: "updatePattern",
      id: pattern().id,
      patch: { lengthSteps: 8 },
    })
    await drag(session, at(BAR + 10, 0), at(2 * BAR + 10, 0))
    expect(layout()).toEqual([
      `0:${BAR}+${BAR / 2}`,
      `0:${BAR + BAR / 2}+${BAR / 2}`,
      `0:${2 * BAR}+${BAR / 2}`,
    ])
  })

  it("stays on the row the stroke started on", async () => {
    await drag(session, at(10, 2), at(BAR + 10, 5))
    expect(clips().map((clip) => clip.row)).toEqual([2, 2])
  })

  it("does not stack a clip on one that is already exactly there", async () => {
    await seed([0, BAR])
    await drag(session, at(10, 0), at(2 * BAR + 10, 0))
    expect(layout()).toEqual([
      `0:0+${BAR}`,
      `0:${BAR}+${BAR}`,
      `0:${2 * BAR}+${BAR}`,
    ])
  })

  it("moves a clip it is pressed on, like the Draw tool", async () => {
    await seed([0, 0])
    await drag(session, at(BAR / 2, 0), at(BAR / 2 + BAR, 0))
    expect(layout()).toEqual([`0:${BAR}+${BAR}`])
  })
})

describe("Select tool", () => {
  beforeEach(() => ui().setTool("select"))

  it("selects what the box touches and edits nothing", async () => {
    const ids = await seed([0, 0], [1, BAR], [3, 6 * BAR])
    const before = steps()
    await drag(session, at(2 * BAR + 500, 0), at(BAR / 2, 1))
    expect(selection()).toEqual(ids.slice(0, 2))
    expect(steps()).toBe(before)
    expect(surface.marquee).toBeNull()
  })

  it("shows the box and the clips in it while dragging", async () => {
    await seed([0, 0], [1, BAR])
    session.pointerDown(at(2 * BAR + 500, 0))
    session.pointerMove(at(BAR / 2, 1))
    expect(surface.marquee).toMatchObject({
      tick0: 2 * BAR + 500,
      tick1: BAR / 2,
    })
    expect(surface.items?.batch.selectedCount).toBe(2)
    expect(selection()).toEqual([])
    await session.pointerUp(at(BAR / 2, 1))
  })

  it("adds to the selection with Shift", async () => {
    const ids = await seed([0, 0], [3, 6 * BAR])
    ui().select([ids[0]])
    await drag(
      session,
      at(6 * BAR - 100, 3, { shift: true }),
      at(6 * BAR + 100, 3, { shift: true })
    )
    expect(selection()).toEqual(ids)
  })

  it("clears the selection on a click in empty space", async () => {
    const ids = await seed([0, 0])
    ui().select(ids)
    await click(session, at(5 * BAR, 4))
    expect(selection()).toEqual([])
  })

  it("selects a clicked clip, and Shift+click adds or removes one", async () => {
    const [first, second] = await seed([0, 0], [1, 2 * BAR])
    await click(session, at(BAR / 2, 0))
    expect(selection()).toEqual([first])
    await click(session, at(2 * BAR + BAR / 2, 1, { shift: true }))
    expect(selection()).toEqual([first, second])
    clock.time += 1000
    await click(session, at(BAR / 2, 0, { shift: true }))
    expect(selection()).toEqual([second])
  })

  it("moves the selection when one of its clips is dragged", async () => {
    const ids = await seed([0, 0], [1, 2 * BAR])
    ui().select(ids)
    await drag(session, at(BAR / 2, 0), at(BAR / 2 + BAR, 0))
    expect(layout()).toEqual([`0:${BAR}+${BAR}`, `1:${3 * BAR}+${BAR}`])
    expect(selection()).toEqual(ids)
  })

  it("opens the menu for the clips on a right-click, selecting the clip", async () => {
    const [first] = await seed([0, 0])
    const before = steps()
    await click(session, at(BAR / 2, 0, { button: 2 }))
    expect(selection()).toEqual([first])
    expect(ui().menuOnClips).toBe(true)
    await click(session, at(5 * BAR, 3, { button: 2 }))
    expect(ui().menuOnClips).toBe(false)
    expect(steps()).toBe(before)
    expect(clips()).toHaveLength(1)
  })
})

describe("Erase tool", () => {
  beforeEach(() => ui().setTool("erase"))

  it("deletes a clicked clip", async () => {
    await seed([0, 0], [1, 0])
    await click(session, at(BAR / 2, 1))
    expect(layout()).toEqual([`0:0+${BAR}`])
  })

  it("deletes what a drag crosses, across tracks, in one undo step", async () => {
    await seed([0, 0], [1, 0], [2, 0], [2, 6 * BAR])
    const before = steps()
    session.pointerDown(at(BAR / 2, 0))
    session.pointerMove(at(BAR / 2, 1))
    session.pointerMove(at(BAR / 2, 2))
    expect(session.marked.size).toBe(3)
    expect(clips()).toHaveLength(4)
    await session.pointerUp(at(BAR / 2, 2))
    expect(layout()).toEqual([`2:${6 * BAR}+${BAR}`])
    expect(steps()).toBe(before + 1)
  })

  it("does nothing in empty space", async () => {
    await seed([0, 0])
    const before = steps()
    await click(session, at(5 * BAR, 3))
    expect(steps()).toBe(before)
  })
})

describe("Mute tool", () => {
  beforeEach(() => ui().setTool("mute"))

  it("mutes a clicked clip and unmutes it on the next click", async () => {
    await seed([0, 0])
    await click(session, at(BAR / 2, 0))
    expect(clips()[0].muted).toBe(true)
    expect(labels().at(-1)).toBe("Mute clip")
    await click(session, at(BAR / 2, 0))
    expect(clips()[0].muted).toBe(false)
    expect(labels().at(-1)).toBe("Unmute clip")
  })

  it("gives every clip a drag crosses the state of the first, in one step", async () => {
    const ids = await seed([0, 0], [0, BAR], [0, 2 * BAR])
    await dispatch({
      type: "updateClips",
      updates: [{ id: ids[1], patch: { muted: true } }],
    })
    const before = steps()
    await drag(session, at(100, 0), at(2 * BAR + 100, 0))
    expect(clips().map((clip) => clip.muted)).toEqual([true, true, true])
    expect(steps()).toBe(before + 1)
    expect(labels().at(-1)).toBe("Mute clips")
  })

  it("deletes with the right button", async () => {
    await seed([0, 0])
    await click(session, at(BAR / 2, 0, { button: 2 }))
    expect(clips()).toHaveLength(0)
  })
})

describe("double-click", () => {
  it("selects the clip's pattern and shows the channel rack", async () => {
    const added = await dispatch({ type: "addPattern", name: "Other" })
    const other = added!.created[0]
    await seed([0, 0])
    await dispatch({
      type: "addClips",
      clips: [
        {
          track: tracks()[0].id,
          start: 4 * BAR,
          content: { type: "pattern", pattern: other },
        },
      ],
    })
    await setTransportPattern(pattern().id)
    const before = steps()
    session.pointerDown(at(4 * BAR + 500, 0))
    await session.pointerUp(at(4 * BAR + 500, 0))
    clock.time += 200
    session.pointerDown(at(4 * BAR + 500, 0))
    await settle()
    expect(useTransportStore.getState().pattern).toBe(other)
    expect(useUiStore.getState().centerTab).toBe("channelRack")
    expect(steps()).toBe(before)
  })

  it("brings back the hidden settings of an audio clip", async () => {
    const id = await addAudioFile("/factory/Loops/Drum loop 128.wav", {
      start: 0,
    })
    await settle()
    ui().clearSelection()
    ui().toggleInspector()
    expect(ui().inspectorOpen).toBe(false)
    const before = steps()
    session.pointerDown(at(BAR / 2, 0))
    await session.pointerUp(at(BAR / 2, 0))
    clock.time += 200
    session.pointerDown(at(BAR / 2, 0))
    await settle()
    expect(ui().inspectorOpen).toBe(true)
    expect(selection()).toEqual([id])
    expect(steps()).toBe(before)
  })

  it("does not count a click on a clip and then a grab of its edge", async () => {
    await seed([0, 0])
    await click(session, at(BAR / 2, 0))
    clock.time += 100
    await drag(session, at(BAR - 40, 0), at(3 * BAR + 20, 0))
    expect(useUiStore.getState().centerTab).toBe("playlist")
    expect(layout()).toEqual([`0:0+${3 * BAR}`])
  })

  it("does not count two slow clicks, or clicks on empty grid", async () => {
    await seed([0, 0])
    await click(session, at(BAR / 2, 0))
    clock.time += 900
    await click(session, at(BAR / 2, 0))
    expect(useUiStore.getState().centerTab).toBe("playlist")
    // Two quick clicks on empty grid place one clip and then press on it.
    await click(session, at(6 * BAR + 100, 2))
    clock.time += 100
    await click(session, at(6 * BAR + 100, 2))
    expect(useUiStore.getState().centerTab).toBe("playlist")
    expect(clips()).toHaveLength(2)
  })
})

describe("tracks going away", () => {
  it("scroll the view back up to the last track", async () => {
    for (let index = 0; index < 30; index++)
      await dispatch({ type: "addPlaylistTrack" })
    await settle()
    surface.setViewport({ ...surface.viewport, scrollRow: 100 })
    expect(surface.viewport.scrollRow).toBe(30 + SPARE_ROWS - 20)
    for (const track of tracks().slice(20))
      await dispatch({ type: "removePlaylistTrack", id: track.id })
    await settle()
    // Twenty tracks and one spare row under them, in a view twenty rows tall.
    expect(tracks()).toHaveLength(20)
    expect(surface.viewport.scrollRow).toBe(1)
  })
})

describe("what happens around a gesture", () => {
  it("drops clips from the selection when their pattern is deleted", async () => {
    const added = await dispatch({ type: "addPattern", name: "Other" })
    const other = added!.created[0]
    const [kept] = await seed([0, 0])
    const made = await dispatch({
      type: "addClips",
      clips: [
        {
          track: tracks()[0].id,
          start: 4 * BAR,
          content: { type: "pattern", pattern: other },
        },
      ],
    })
    ui().select([kept, made!.created[0]])
    await dispatch({ type: "removePattern", id: other })
    expect(clips().map((clip) => clip.id)).toEqual([kept])
    expect(selection()).toEqual([kept])
    expect(surface.items?.batch.count).toBe(1)
  })

  it("drops a drag on cancel without editing", async () => {
    await seed([0, 0])
    const before = steps()
    session.pointerDown(at(BAR / 2, 0))
    session.pointerMove(at(3 * BAR, 2))
    expect(session.busy).toBe(true)
    session.cancel()
    expect(session.busy).toBe(false)
    expect(surface.drag).toEqual({ ticks: 0, rows: 0 })
    await session.pointerUp(at(3 * BAR, 2))
    expect(steps()).toBe(before)
  })

  it("ignores a second press while an edit is on its way", async () => {
    session.pointerDown(at(0, 0))
    const first = session.pointerUp(at(0, 0))
    expect(session.pointerDown(at(4 * BAR, 0))).toBe(false)
    await first
    await settle()
    expect(clips()).toHaveLength(1)
  })

  it("scrolls when a drag is held at the edge, and drops where that leads", async () => {
    await seed([0, 0])
    const held = { ...at(0, 0), x: surface.viewport.width - 4 }
    session.pointerDown(at(BAR / 2, 0))
    session.pointerMove(held)
    await vi.waitFor(() =>
      expect(surface.viewport.scrollTick).toBeGreaterThan(BAR)
    )
    // The clip follows the timeline as it scrolls under the still pointer.
    expect(surface.drag.ticks).toBeGreaterThanOrEqual(10 * BAR)
    const scrolled = surface.viewport.scrollTick
    await session.pointerUp(held)
    await settle()
    expect(clips()[0].start).toBeGreaterThanOrEqual(10 * BAR)
    expect(clips()[0].start % BAR).toBe(0)
    await new Promise((resolve) => setTimeout(resolve, 60))
    expect(surface.viewport.scrollTick).toBe(scrolled)
  })

  it("does not scroll for a press that stays put near the edge", async () => {
    const edge = { ...at(0, 0), x: surface.viewport.width - 4 }
    session.pointerDown(edge)
    session.pointerMove(edge)
    await new Promise((resolve) => setTimeout(resolve, 60))
    expect(surface.viewport.scrollTick).toBe(0)
    await session.pointerUp(edge)
  })

  it("can press a sliver of a clip from just beside it", async () => {
    ui().setTool("erase")
    await seed([0, 0, 60])
    surface.setViewport({ ...surface.viewport, pxPerTick: 0.002 })
    // The clip is a tenth of a pixel wide here. One pixel to its right.
    await click(session, { ...at(0, 0), x: 1.1 })
    expect(clips()).toHaveLength(0)
  })
})
