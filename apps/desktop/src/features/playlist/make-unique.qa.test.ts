import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { AutomationPoint, Command } from "@/bindings"
import { getAppState, isEnabled, registry, runAction } from "@/lib/actions"
import { dispatch, undo } from "@/lib/store/project"

import { BAR, history, project, startPlaylist, ui } from "./test-utils"

/*
 * Make unique on an automation clip, through the playlist action and the
 * real document: the chosen clip gets a curve of its own, which can then be
 * edited without changing the clips that still show the original.
 */

const point = (tick: number, value: number): AutomationPoint => ({
  tick,
  value,
  curve: 0,
  hold: false,
})

let app: Awaited<ReturnType<typeof startPlaylist>>
let automation: number
let lane: number
let first: number
let second: number
let pattern: number

async function created(command: Command): Promise<number[]> {
  const result = await dispatch(command)
  if (!result) throw new Error("The command was refused")
  return result.created
}

const automationOf = (clip: number) => {
  const found = project().playlist.clips.find((item) => item.id === clip)
  if (found?.content.type !== "automation") throw new Error("Not a curve clip")
  return found.content.automation
}
const curve = (id: number) => {
  const found = project().automations.find((item) => item.id === id)
  if (!found) throw new Error(`No automation ${id}`)
  return found
}

beforeEach(async () => {
  app = await startPlaylist()
  ;[automation] = await created({
    type: "addAutomation",
    name: "Tempo ride",
    target: { type: "tempo" },
    points: [point(0, 0.25), point(BAR, 0.75)],
  })
  ;[lane] = await created({ type: "addPlaylistTrack", name: "Curves" })
  ;[first, second, pattern] = await created({
    type: "addClips",
    clips: [
      { track: lane, start: 0, length: BAR, content: { type: "automation", automation } },
      {
        track: lane,
        start: 2 * BAR,
        length: BAR / 2,
        offset: 240,
        content: { type: "automation", automation },
      },
      {
        track: lane,
        start: 4 * BAR,
        length: BAR,
        content: { type: "pattern", pattern: project().patterns[0].id },
      },
    ],
  })
})

afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

const enabled = () =>
  isEnabled(registry.get("playlist.makeUnique")!, getAppState())

describe("make unique for automation clips", () => {
  it("gives the selected automation clip a curve of its own in one undo step", async () => {
    const original = structuredClone(curve(automation))
    const other = structuredClone(
      project().playlist.clips.find((item) => item.id === first)
    )
    const chosen = structuredClone(
      project().playlist.clips.find((item) => item.id === second)!
    )
    ui().select([second])
    expect(enabled()).toBe(true)
    const send = vi.spyOn(app.backend, "dispatch")
    const before = history().cursor
    const curves = project().automations.length

    await runAction("playlist.makeUnique")

    expect(send).toHaveBeenCalledTimes(1)
    expect(send.mock.calls[0][0]).toEqual({
      type: "batch",
      label: "Make unique",
      commands: [{ type: "makeUnique", clip: second }],
    })
    expect(history().cursor).toBe(before + 1)
    expect(history().entries.at(-1)?.label).toBe("Make unique")

    const copy = automationOf(second)
    expect(copy).not.toBe(automation)
    // The copy sits right after its source in the automation list.
    const ids = project().automations.map((item) => item.id)
    expect(ids.indexOf(copy)).toBe(ids.indexOf(automation) + 1)
    expect(ids).toHaveLength(curves + 1)
    expect(curve(copy)).toEqual({
      ...original,
      id: copy,
      name: "Tempo ride #2",
    })
    // Only the chosen clip was redirected, and it shows the same window.
    expect(project().playlist.clips.find((item) => item.id === second)).toEqual({
      ...chosen,
      content: { type: "automation", automation: copy },
    })
    expect(project().playlist.clips.find((item) => item.id === first)).toEqual(
      other
    )

    // The copy is edited on its own.
    await dispatch({
      type: "setAutomationPoints",
      id: copy,
      points: [point(0, 1), point(480, 0)],
    })
    expect(curve(automation)).toEqual(original)
    expect(curve(copy).points).toEqual([point(0, 1), point(480, 0)])
    // And so is the original.
    await dispatch({
      type: "setAutomationPoints",
      id: automation,
      points: [point(0, 0.5)],
    })
    expect(curve(copy).points).toEqual([point(0, 1), point(480, 0)])

    // Undo back through both edits and the copy.
    await undo()
    await undo()
    await undo()
    expect(project().automations).toHaveLength(curves)
    expect(curve(automation)).toEqual(original)
    expect(automationOf(second)).toBe(automation)
    expect(history().cursor).toBe(before)
  })

  it("copies the sources of a mixed selection in one batch", async () => {
    ui().select([first, second, pattern])
    const curves = project().automations.length
    const patterns = project().patterns.length
    const source = project().playlist.clips.find((item) => item.id === pattern)
    const send = vi.spyOn(app.backend, "dispatch")
    await runAction("playlist.makeUnique")
    expect(send).toHaveBeenCalledTimes(1)
    expect(send.mock.calls[0][0]).toEqual({
      type: "batch",
      label: "Make unique",
      commands: [first, second, pattern].map((clip) => ({
        type: "makeUnique",
        clip,
      })),
    })
    // Two curve clips of one automation each get a copy of their own.
    const copies = [automationOf(first), automationOf(second)]
    expect(new Set([automation, ...copies]).size).toBe(3)
    expect(project().automations).toHaveLength(curves + 2)
    const made = project().playlist.clips.find((item) => item.id === pattern)
    expect(made?.content).not.toEqual(source?.content)
    expect(project().patterns).toHaveLength(patterns + 1)
    await undo()
    expect(project().automations).toHaveLength(curves)
    expect(project().patterns).toHaveLength(patterns)
  })

  it("is disabled with nothing selected", async () => {
    ui().clearSelection()
    expect(enabled()).toBe(false)
    const send = vi.spyOn(app.backend, "dispatch")
    await runAction("playlist.makeUnique")
    expect(send).not.toHaveBeenCalled()
  })
})
