import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AutomationPoint, Command } from "@/bindings"
import { selectedClipLfoItems } from "@/features/playlist/automation/menu"
import {
  BAR,
  history,
  project,
  startPlaylist,
  ui,
} from "@/features/playlist/test-utils"
import { dispatch, redo, undo } from "@/lib/store/project"
import { settle } from "@/test/harness"

import { LfoDialog, closeLfoDialog, openAutomationLfo } from "./lfo-dialog"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

/*
 * The LFO tool of an automation clip, from its dialog to the document: the
 * mounted form sends one checked command, the real document writes the
 * points, and the edit is one undo step. What the written curve does to the
 * linked parameter while it plays is measured natively, in
 * crates/windfall-engine/tests/engine/playlist_layouts.rs.
 */

const point = (tick: number, value: number): AutomationPoint => ({
  tick,
  value,
  curve: 0,
  hold: false,
})

let app: Awaited<ReturnType<typeof startPlaylist>>
let automation: number
let clip: number

async function created(command: Command): Promise<number[]> {
  const result = await dispatch(command)
  if (!result) throw new Error("The command was refused")
  return result.created
}

const curve = () => {
  const found = project().automations.find((item) => item.id === automation)
  if (!found) throw new Error("The automation is gone")
  return found
}

beforeEach(async () => {
  app = await startPlaylist()
  ;[automation] = await created({
    type: "addAutomation",
    name: "Ride",
    target: { type: "tempo" },
    points: [point(0, 0.5)],
  })
  const [lane] = await created({ type: "addPlaylistTrack", name: "Curves" })
  // The clip shows the curve from its second beat for a bar and a half.
  ;[clip] = await created({
    type: "addClips",
    clips: [
      {
        track: lane,
        start: 2 * BAR,
        length: 5760,
        offset: 960,
        content: { type: "automation", automation },
      },
    ],
  })
})

afterEach(() => {
  act(() => closeLfoDialog())
  app.stop()
  vi.restoreAllMocks()
})

function open(run: () => void) {
  render(<LfoDialog />)
  act(run)
}

function type(label: string, value: string | number) {
  fireEvent.change(screen.getByLabelText(label), {
    target: { value: String(value) },
  })
}

const field = (label: string) =>
  screen.getByLabelText(label) as HTMLInputElement
const write = () => screen.getByRole("button", { name: "Write LFO" })

describe("automation clip LFO, dialog to document", () => {
  it("writes a sine into the captured curve as one undoable command", async () => {
    open(() => openAutomationLfo(automation))
    expect(screen.getByRole("dialog")).toHaveTextContent("LFO · Ride")
    // The whole curve is offered: from its start to the end of what a clip shows.
    expect(field("Start (source ticks)").value).toBe("0")
    expect(field("End (source ticks)").value).toBe("6720")
    expect(field("Period (ticks)").value).toBe("960")

    const captured = structuredClone(curve())
    type("End (source ticks)", 3840)
    type("Point spacing (ticks)", 120)
    type("Center (%)", 50)
    type("Depth (± %)", 25)
    expect(screen.getByRole("img", { name: "LFO shape preview" })).toBeInTheDocument()
    const send = vi.spyOn(app.backend, "dispatch")
    const before = history().cursor

    fireEvent.click(write())

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    expect(send).toHaveBeenCalledExactlyOnceWith(
      {
        type: "generateAutomationLfo",
        expected: captured,
        start: 0,
        end: 3840,
        resolution: 120,
        lfo: {
          wave: "sine",
          period: 960,
          phase: 0,
          center: 0.5,
          depth: 0.25,
          width: 0.5,
          seed: 1,
        },
      },
      undefined
    )
    expect(history().cursor).toBe(before + 1)
    expect(history().entries.at(-1)?.label).toBe("Write automation LFO")

    // The document holds the LFO as editable points: one every 120 ticks,
    // on the sine that was asked for, four cycles of it.
    const points = curve().points
    const written = points.filter((item) => item.tick <= 3840)
    expect(written.length).toBeGreaterThanOrEqual(33)
    for (const item of written) {
      if (item.tick % 120 !== 0) continue
      const ideal = 0.5 + 0.25 * Math.sin((item.tick / 960) * 2 * Math.PI)
      expect(item.value).toBeCloseTo(ideal, 5)
      expect(item.hold).toBe(false)
    }
    const at = (tick: number) =>
      points.find((item) => item.tick === tick)?.value
    expect(at(240)).toBeCloseTo(0.75, 5)
    expect(at(720)).toBeCloseTo(0.25, 5)
    expect(at(3120)).toBeCloseTo(0.75, 5)
    // The points are ordinary points: one of them can be moved afterwards.
    const edited = points.map((item) =>
      item.tick === 240 ? { ...item, value: 1 } : item
    )
    await act(async () => {
      await dispatch({ type: "setAutomationPoints", id: automation, points: edited })
    })
    expect(curve().points.find((item) => item.tick === 240)?.value).toBe(1)

    // One undo for the hand edit, one for the whole LFO.
    await act(async () => {
      await undo()
      await undo()
      await settle()
    })
    expect(curve()).toEqual(captured)
    await act(async () => {
      await redo()
      await settle()
    })
    expect(curve().points).toEqual(points)
  })

  it("opens on the part of the curve the selected clip shows", async () => {
    ui().select([clip])
    const items = selectedClipLfoItems()
    expect(items).toHaveLength(1)
    const [entry] = items
    if (typeof entry === "string" || !("title" in entry))
      throw new Error("The entry cannot be run")
    expect(entry.title).toBe("Write LFO…")
    open(() => void entry.run())
    expect(field("Start (source ticks)").value).toBe("960")
    expect(field("End (source ticks)").value).toBe("6720")

    type("Period (ticks)", 1920)
    type("Point spacing (ticks)", 240)
    fireEvent.click(write())
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    const points = curve().points
    // Before the clip's window the curve is as it was.
    expect(points[0]).toEqual(point(0, 0.5))
    expect(points.filter((item) => item.tick < 960)).toHaveLength(1)
    // Inside it, a cycle every two beats, counted from the window's start.
    const at = (tick: number) =>
      points.filter((item) => item.tick === tick).at(-1)?.value
    expect(at(960 + 480)).toBeCloseTo(1, 5)
    expect(at(960 + 1440)).toBeCloseTo(0, 5)

    // Nothing for a pattern clip, or for more than one clip.
    ui().clearSelection()
    expect(selectedClipLfoItems()).toEqual([])
  })

  it("refuses to write once the captured curve has changed", async () => {
    open(() => openAutomationLfo(automation))
    expect(write()).toBeEnabled()
    await act(async () => {
      await dispatch({
        type: "updateAutomation",
        id: automation,
        patch: { name: "Renamed" },
      })
      await settle()
    })
    expect(
      screen.getByText(/The project or selection changed/)
    ).toBeInTheDocument()
    expect(write()).toBeDisabled()
    const send = vi.spyOn(app.backend, "dispatch")
    fireEvent.submit(write().closest("form")!)
    expect(send).not.toHaveBeenCalled()
    expect(curve().points).toEqual([point(0, 0.5)])
  })

  it("validates the form and reports a curve that would be too long", async () => {
    open(() => openAutomationLfo(automation))
    type("Period (ticks)", 0)
    expect(screen.getByText(/Use finite values/)).toBeInTheDocument()
    expect(write()).toBeDisabled()
    type("Period (ticks)", 960)
    type("End (source ticks)", 0)
    expect(write()).toBeDisabled()

    // More than 4,096 points: the document refuses, the dialog says so and
    // stays open, and nothing was written.
    type("End (source ticks)", 100000)
    type("Point spacing (ticks)", 1)
    expect(write()).toBeEnabled()
    const before = history().cursor
    fireEvent.click(write())
    expect(
      await screen.findByText(/The LFO could not be applied/)
    ).toBeInTheDocument()
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    expect(curve().points).toEqual([point(0, 0.5)])
    expect(history().cursor).toBe(before)
  })

  it("closes on undo, so a stale capture is never left open", async () => {
    await act(async () => {
      await dispatch({ type: "updateAutomation", id: automation, patch: { color: 0x336699 } })
    })
    open(() => openAutomationLfo(automation))
    expect(screen.getByRole("dialog")).toBeInTheDocument()
    await act(async () => {
      await undo()
      await settle()
    })
    expect(screen.queryByRole("dialog")).toBeNull()
  })
})
