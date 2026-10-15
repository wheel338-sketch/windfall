import { render, screen } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { NoteInit } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { isStepNote, litSteps } from "@/features/channel-rack/steps"
import { runAction, shortcutLabel } from "@/lib/actions"
import { emptyProject } from "@/lib/ipc/sim/project"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { setTransportPattern } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import { readClipboard } from "./clipboard"
import { applyNoteTool, closeNoteTools, useNoteTools } from "./note-tools"
import { MAX_PATTERN_TICKS } from "./edit-math"
import PianoRollPanel from "./index"
import { laneUpdates, paintValues } from "./lane-math"
import { usePianoRollStore } from "./store"
import {
  at,
  brief,
  channel,
  currentPattern,
  history,
  notesOf,
  startRoll,
  undoSteps,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

type Roll = Awaited<ReturnType<typeof startRoll>>

let roll: Roll

/** Adds an empty channel called Lead and opens it. */
async function openLead(notes: NoteInit[] = []) {
  const result = await dispatch({ type: "addChannel", name: "Lead" })
  if (!result) throw new Error("could not add a channel")
  if (notes.length > 0) {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: result.created[0],
      notes,
    })
  }
  roll.show("Lead")
}

const lead = () => brief(notesOf("Lead"))
const n = (start: number, key: number, length = 240): NoteInit => ({
  start,
  length,
  key,
})
const selectedBrief = () => brief(roll.editor.selectedNotes())

beforeEach(async () => {
  roll = await startRoll()
})
afterEach(() => {
  closeNoteTools()
  roll.stop()
})

it("selects and moves imported edit-group members together across MIDI colors", async () => {
  await openLead([
    { ...n(0, 64), expression: { ...DEFAULT_NOTE_EXPRESSION, colorGroup: 0 } },
    { ...n(480, 67), expression: { ...DEFAULT_NOTE_EXPRESSION, colorGroup: 1 } },
    n(960, 60),
  ])
  const [first, second] = notesOf("Lead")
  const state = useProjectStore.getState()
  useProjectStore.setState({
    project: {
      ...state.project,
      noteEditGroups: [
        {
          pattern: currentPattern().id,
          channel: channel("Lead").id,
          group: 7,
          notes: [first.id, second.id],
        },
      ],
    },
  })
  roll.editor.setSelection([first.id])
  expect(roll.editor.selection).toEqual(new Set([first.id, second.id]))
  roll.editor.pointerDown(at(100, 64), "left")
  roll.editor.pointerMove(at(340, 65))
  roll.editor.pointerUp(at(340, 65))
  await settle()
  expect(notesOf("Lead").map((note) => [note.start, note.key])).toEqual([
    [240, 65],
    [720, 68],
    [960, 60],
  ])
})

describe("draw tool", () => {
  beforeEach(() => openLead())

  it("adds a note where the click lands, snapped, as one undo step", async () => {
    const before = undoSteps()
    await roll.click(at(500, 64))
    expect(notesOf("Lead")).toMatchObject([
      { start: 480, length: 240, key: 64, velocity: 0.8, pan: 0 },
    ])
    expect(undoSteps()).toBe(before + 1)
    expect(roll.editor.selection.size).toBe(0)
  })

  it("auditions the note while the button is down", async () => {
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    const id = channel("Lead").id
    roll.editor.pointerDown(at(500, 64), "left")
    expect(on).toHaveBeenCalledWith(id, 64, 0.8)
    expect(off).not.toHaveBeenCalled()
    roll.editor.pointerUp(at(500, 64))
    expect(off).toHaveBeenCalledWith(id, 64)
  })

  it("lets the new note be placed before the button comes up", async () => {
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    const before = undoSteps()
    roll.editor.pointerDown(at(500, 64), "left")
    roll.editor.pointerMove(at(1300, 67))
    // The preview moves the note; nothing is in the project yet.
    expect(roll.surface.dragOffset).toEqual({ ticks: 720, rows: -3 })
    expect(lead()).toEqual([])
    roll.editor.pointerUp(at(1300, 67))
    await settle()
    expect(lead()).toEqual(["1200:67:240"])
    expect(undoSteps()).toBe(before + 1)
    expect(roll.surface.dragOffset).toEqual({ ticks: 0, rows: 0 })
    // The key changed under the pointer: the old note stops, the new one starts.
    expect(on.mock.calls.map((call) => call[1])).toEqual([64, 67])
    expect(off.mock.calls.map((call) => call[1])).toEqual([64, 67])
  })

  it("gives a new note the length and velocity of the last note touched", async () => {
    await roll.click(at(0, 60))
    // Drag the end from one step to three steps.
    await roll.drag(at(200, 60), at(200 + 480, 60))
    expect(lead()).toEqual(["0:60:720"])
    await dispatch({
      type: "updateNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      updates: [{ id: notesOf("Lead")[0].id, patch: { velocity: 0.4 } }],
    })
    // Touching the note is what makes it the model for the next one.
    await roll.click(at(300, 60))
    await roll.click(at(1000, 72))
    expect(notesOf("Lead")[1]).toMatchObject({
      start: 960,
      key: 72,
      length: 720,
      velocity: 0.4,
    })
  })

  it("ignores snap while Alt is held, and only then", async () => {
    await roll.click(at(500, 64, { alt: true }))
    expect(lead()).toEqual(["500:64:240"])
    // Shift is for the selection. It leaves the snap on.
    await roll.click(at(1300, 66, { shift: true }))
    expect(lead()).toEqual(["500:64:240", "1200:66:240"])
  })

  it("grows the pattern to the next bar in the same undo step", async () => {
    const before = undoSteps()
    await roll.click(at(4000, 64))
    expect(lead()).toEqual(["3840:64:240"])
    expect(currentPattern().lengthSteps).toBe(32)
    expect(undoSteps()).toBe(before + 1)
    expect(history().entries[history().cursor - 1].label).toBe("Add note")
    await undo()
    expect(lead()).toEqual([])
    expect(currentPattern().lengthSteps).toBe(16)
  })

  it("ends a resized note on the beat grid and gives back the bar it grew", async () => {
    usePianoRollStore.getState().setSnap("beat")
    await roll.click(at(2900, 60))
    expect(lead()).toEqual(["2880:60:240"])
    // About a beat to the right puts the end on the next beat line. Snapping
    // the distance instead made it 1¼ beats, across the bar line.
    roll.editor.pointerDown(at(3100, 60), "left")
    roll.editor.pointerMove(at(3100 + 900, 60))
    expect(roll.editor.resizedLength).toBe(960)
    roll.editor.pointerUp(at(3100 + 900, 60))
    await settle()
    expect(roll.editor.resizedLength).toBeNull()
    expect(lead()).toEqual(["2880:60:960"])
    expect(currentPattern().lengthSteps).toBe(16)
    // Past the bar line, the pattern grows to two bars.
    await roll.drag(at(3820, 60), at(3820 + 960, 60))
    expect(lead()).toEqual(["2880:60:1920"])
    expect(currentPattern().lengthSteps).toBe(32)
    // Back inside the first bar, the bar it grew goes again, in the same undo step.
    const before = undoSteps()
    await roll.drag(at(4780, 60), at(4780 - 960, 60))
    expect(lead()).toEqual(["2880:60:960"])
    expect(currentPattern().lengthSteps).toBe(16)
    expect(undoSteps()).toBe(before + 1)
    await undo()
    expect(lead()).toEqual(["2880:60:1920"])
    expect(currentPattern().lengthSteps).toBe(32)
  })

  it("keeps a pattern length set by hand when the notes come back in", async () => {
    usePianoRollStore.getState().setSnap("beat")
    await roll.click(at(2900, 60))
    await roll.drag(at(3100, 60), at(3100 + 1680, 60))
    expect(lead()).toEqual(["2880:60:1920"])
    expect(currentPattern().lengthSteps).toBe(32)
    await dispatch({
      type: "updatePattern",
      id: currentPattern().id,
      patch: { lengthSteps: 48 },
    })
    await settle()
    await roll.drag(at(4780, 60), at(4780 - 960, 60))
    expect(lead()).toEqual(["2880:60:960"])
    expect(currentPattern().lengthSteps).toBe(48)
  })

  it("deletes with the right button, one undo step per drag", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(0, 60), n(480, 60), n(960, 60), n(960, 64)],
    })
    const before = undoSteps()
    await roll.click(at(100, 60), "right")
    expect(lead()).toEqual(["480:60:240", "960:60:240", "960:64:240"])
    await roll.drag(at(300, 60), at(1300, 60), "right")
    expect(lead()).toEqual(["960:64:240"])
    expect(undoSteps()).toBe(before + 2)
    // A right-click on nothing is not an edit.
    await roll.click(at(2000, 70), "right")
    expect(undoSteps()).toBe(before + 2)
  })

  it("selects with Ctrl+drag instead of drawing", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(0, 60), n(480, 62), n(2400, 72)],
    })
    const before = undoSteps()
    await roll.drag(at(0, 63, { ctrl: true }, 0.1), at(900, 59, { ctrl: true }))
    expect(selectedBrief()).toEqual(["0:60:240", "480:62:240"])
    expect(undoSteps()).toBe(before)
    expect(roll.surface.marquee).toBeNull()
  })
})

describe("moving and resizing", () => {
  beforeEach(() => openLead([n(0, 60), n(480, 64), n(960, 67, 480)]))

  it("moves a note by dragging its body", async () => {
    const before = undoSteps()
    await roll.drag(at(100, 60), at(100 + 500, 62))
    expect(lead()).toEqual(["480:62:240", "480:64:240", "960:67:480"])
    expect(undoSteps()).toBe(before + 1)
    expect(selectedBrief()).toEqual(["480:62:240"])
  })

  it("moves the whole selection as one command", async () => {
    roll.editor.selectAll()
    const before = undoSteps()
    roll.editor.pointerDown(at(600, 64), "left")
    roll.editor.pointerMove(at(600 + 240, 65))
    expect(roll.surface.dragOffset).toEqual({ ticks: 240, rows: -1 })
    roll.editor.pointerUp(at(600 + 240, 65))
    await settle()
    expect(lead()).toEqual(["240:61:240", "720:65:240", "1200:68:480"])
    expect(undoSteps()).toBe(before + 1)
    expect(roll.editor.selection.size).toBe(3)
    await undo()
    expect(lead()).toEqual(["0:60:240", "480:64:240", "960:67:480"])
  })

  it("stops the group where its first note reaches the start", async () => {
    roll.editor.selectAll()
    await roll.drag(at(600, 64), at(600 - 2000, 64))
    expect(lead()).toEqual(["0:60:240", "480:64:240", "960:67:480"])
    // On its own the last note can go all the way to the start.
    roll.editor.setSelection([])
    await roll.drag(at(1200, 67), at(1200 - 2000, 67))
    expect(lead()).toEqual(["0:60:240", "0:67:480", "480:64:240"])
  })

  it("moves freely with Alt held", async () => {
    await roll.drag(at(100, 60), at(100 + 130, 60, { alt: true }))
    expect(lead()[0]).toBe("130:60:240")
  })

  it("moves on the snap with Shift held, and copies nothing", async () => {
    const count = lead().length
    await roll.drag(
      at(100, 60, { shift: true }),
      at(100 + 250, 60, { shift: true })
    )
    expect(lead()[0]).toBe("240:60:240")
    expect(lead()).toHaveLength(count)
  })

  it("moves the selection on Shift+drag of another note, as the playlist does", async () => {
    roll.editor.setSelection([notesOf("Lead")[0].id])
    await roll.drag(
      at(600, 64, { shift: true }),
      at(600 + 240, 64, { shift: true })
    )
    // Shift added the pressed note to the selection, and both moved.
    expect(lead()).toEqual(["240:60:240", "720:64:240", "960:67:480"])
    expect(roll.editor.selection.size).toBe(2)
  })

  it("copies on Ctrl+drag of a note, with Ctrl held from the press on", async () => {
    const before = undoSteps()
    await roll.drag(
      at(100, 60, { ctrl: true }),
      at(100 + 480, 60, { ctrl: true })
    )
    expect(lead()).toEqual([
      "0:60:240",
      "480:60:240",
      "480:64:240",
      "960:67:480",
    ])
    expect(selectedBrief()).toEqual(["480:60:240"])
    expect(undoSteps()).toBe(before + 1)
  })

  it("duplicates when Ctrl is held at the drop, and selects the copies", async () => {
    const before = undoSteps()
    roll.editor.pointerDown(at(100, 60), "left")
    roll.editor.pointerMove(at(100, 72, { ctrl: true }))
    expect(roll.editor.drag?.duplicate).toBe(true)
    roll.editor.pointerUp(at(100, 72, { ctrl: true }))
    await settle()
    expect(lead()).toEqual(["0:60:240", "0:72:240", "480:64:240", "960:67:480"])
    expect(selectedBrief()).toEqual(["0:72:240"])
    expect(undoSteps()).toBe(before + 1)
  })

  it("treats a press without movement as a click that selects", async () => {
    roll.editor.selectAll()
    const before = undoSteps()
    await roll.click(at(600, 64))
    expect(selectedBrief()).toEqual(["480:64:240"])
    await roll.click(at(100, 60, { shift: true }))
    expect(selectedBrief()).toEqual(["0:60:240", "480:64:240"])
    await roll.click(at(100, 60, { shift: true }))
    expect(selectedBrief()).toEqual(["480:64:240"])
    expect(undoSteps()).toBe(before)
  })

  it("resizes from the right edge", async () => {
    const before = undoSteps()
    roll.editor.pointerDown(at(1420, 67), "left")
    roll.editor.pointerMove(at(1420 + 480, 67))
    expect(roll.surface.dragResize).toEqual({ start: 0, end: 480 })
    roll.editor.pointerUp(at(1420 + 480, 67))
    await settle()
    expect(lead()).toEqual(["0:60:240", "480:64:240", "960:67:960"])
    expect(undoSteps()).toBe(before + 1)
    expect(roll.surface.dragResize).toEqual({ start: 0, end: 0 })
  })

  it("resizes from the left edge and keeps the end", async () => {
    await roll.drag(at(970, 67), at(970 - 480, 67))
    expect(lead()).toContain("480:67:960")
    await roll.drag(at(490, 67), at(490 + 5000, 67))
    // It cannot pass its own end: one snap interval is left.
    expect(lead()).toContain("1200:67:240")
  })

  it("resizes every selected note together", async () => {
    roll.editor.selectAll()
    const before = undoSteps()
    await roll.drag(at(1420, 67), at(1420 + 240, 67))
    expect(lead()).toEqual(["0:60:480", "480:64:480", "960:67:720"])
    expect(undoSteps()).toBe(before + 1)
  })

  it("puts everything back when a drag is cancelled", async () => {
    const before = undoSteps()
    roll.editor.pointerDown(at(100, 60), "left")
    roll.editor.pointerMove(at(900, 70))
    roll.editor.cancel()
    await settle()
    expect(roll.surface.dragOffset).toEqual({ ticks: 0, rows: 0 })
    expect(roll.editor.drag).toBeNull()
    expect(roll.editor.busy).toBe(false)
    expect(lead()).toEqual(["0:60:240", "480:64:240", "960:67:480"])
    expect(undoSteps()).toBe(before)
  })

  it("never leaves a note sounding", async () => {
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    roll.editor.pointerDown(at(100, 60), "left")
    roll.editor.pointerMove(at(100, 65))
    roll.editor.cancel()
    roll.editor.pointerDown(at(2000, 70), "left")
    roll.editor.dispose()
    expect(on).toHaveBeenCalledTimes(3)
    expect(off.mock.calls.map((call) => call[1])).toEqual([60, 65, 70])
  })
})

describe("paint, erase and select tools", () => {
  beforeEach(() => openLead([n(480, 62)]))

  it("paints a note per snap interval as one command", async () => {
    usePianoRollStore.getState().setTool("paint")
    const before = undoSteps()
    await roll.drag(at(10, 62), [at(300, 62), at(700, 62), at(1100, 62)])
    // The step that already held a note on this key was left alone.
    expect(lead()).toEqual([
      "0:62:240",
      "240:62:240",
      "480:62:240",
      "720:62:240",
      "960:62:240",
    ])
    expect(undoSteps()).toBe(before + 1)
  })

  it("paints notes end to end when they are longer than the snap", async () => {
    usePianoRollStore.getState().setTool("paint")
    usePianoRollStore.getState().rememberNote(480, 0.6)
    await roll.drag(at(10, 70), at(1300, 70))
    expect(brief(notesOf("Lead").filter((note) => note.key === 70))).toEqual([
      "0:70:480",
      "480:70:480",
      "960:70:480",
    ])
  })

  it("erases everything a drag crosses as one command", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(0, 60), n(960, 64), n(1920, 67), n(1920, 50)],
    })
    usePianoRollStore.getState().setTool("erase")
    const before = undoSteps()
    // The slanted first leg passes over the note on key 62 as well.
    await roll.drag(at(50, 60), [at(1000, 64), at(2000, 67)])
    expect(lead()).toEqual(["1920:50:240"])
    expect(undoSteps()).toBe(before + 1)
    await undo()
    expect(lead()).toHaveLength(5)
  })

  it("selects with a marquee, adds with Shift and clears on a click", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(0, 60), n(1920, 67)],
    })
    usePianoRollStore.getState().setTool("select")
    const before = undoSteps()
    await roll.drag(at(0, 63, {}, 0.1), at(900, 59))
    expect(selectedBrief()).toEqual(["0:60:240", "480:62:240"])
    await roll.drag(
      at(1800, 68, { shift: true }, 0.1),
      at(2300, 66, { shift: true })
    )
    expect(roll.editor.selection.size).toBe(3)
    await roll.click(at(3000, 70))
    expect(roll.editor.selection.size).toBe(0)
    expect(lead()).toHaveLength(3)
    expect(undoSteps()).toBe(before)
  })

  it("keeps the right button for the menu in the select tool", async () => {
    usePianoRollStore.getState().setTool("select")
    const before = undoSteps()
    const intent = roll.editor.pointerDown(at(500, 62), "right")
    expect(intent).toEqual({ kind: "menu" })
    expect(roll.editor.busy).toBe(false)
    // The note under the pointer is what the menu will act on.
    expect(selectedBrief()).toEqual(["480:62:240"])
    await settle()
    expect(lead()).toEqual(["480:62:240"])
    expect(undoSteps()).toBe(before)
  })
})

describe("clipboard and keyboard edits", () => {
  beforeEach(() => openLead([n(960, 60), n(1200, 64, 480), n(1920, 67)]))

  it("copies, cuts and pastes into another channel and pattern", async () => {
    roll.editor.setSelection(
      notesOf("Lead")
        .slice(0, 2)
        .map((note) => note.id)
    )
    await runAction("pianoRoll.copy")
    expect(readClipboard()?.notes).toHaveLength(2)
    expect(lead()).toHaveLength(3)

    roll.show("Clap")
    const clapBefore = notesOf("Clap").length
    const before = undoSteps()
    await runAction("pianoRoll.paste")
    await settle()
    // Nothing is playing, so the phrase lands in the first bar in view, at
    // the place in the bar it was copied from.
    const pasted = roll.editor.selectedNotes()
    expect(brief(pasted)).toEqual(["960:60:240", "1200:64:480"])
    expect(notesOf("Clap")).toHaveLength(clapBefore + 2)
    expect(undoSteps()).toBe(before + 1)

    const added = await dispatch({ type: "addPattern" })
    if (!added) throw new Error("no pattern")
    await setTransportPattern(added.created[0])
    roll.show("Lead")
    expect(lead()).toEqual([])
    await runAction("pianoRoll.paste")
    await settle()
    expect(lead()).toEqual(["960:60:240", "1200:64:480"])

    await runAction("pianoRoll.cut")
    await settle()
    expect(lead()).toEqual([])
    expect(readClipboard()?.notes).toHaveLength(2)
  })

  it("pastes at the playhead when it is showing", async () => {
    roll.editor.setSelection([notesOf("Lead")[0].id])
    await runAction("pianoRoll.copy")
    roll.session.setPlayhead(2500)
    await runAction("pianoRoll.paste")
    await settle()
    expect(brief(roll.editor.selectedNotes())).toEqual(["2400:60:240"])
  })

  it("duplicates the selection right after itself", async () => {
    roll.editor.setSelection(
      notesOf("Lead")
        .slice(0, 2)
        .map((note) => note.id)
    )
    const before = undoSteps()
    await runAction("pianoRoll.duplicate")
    await settle()
    // The phrase is 720 ticks long, so the copy starts 720 later.
    expect(selectedBrief()).toEqual(["1680:60:240", "1920:64:480"])
    expect(lead()).toHaveLength(5)
    expect(undoSteps()).toBe(before + 1)
  })

  it("nudges by one snap step and one semitone, one undo step each", async () => {
    roll.editor.selectAll()
    const before = undoSteps()
    await runAction("pianoRoll.nudgeRight")
    await runAction("pianoRoll.transposeUp")
    await settle()
    expect(lead()).toEqual(["1200:61:240", "1440:65:480", "2160:68:240"])
    await runAction("pianoRoll.nudgeLeft")
    await runAction("pianoRoll.transposeDown")
    await settle()
    expect(lead()).toEqual(["960:60:240", "1200:64:480", "1920:67:240"])
    expect(undoSteps()).toBe(before + 4)
  })

  it("nudges by the chosen snap", async () => {
    usePianoRollStore.getState().setSnap("beat")
    roll.editor.selectAll()
    await runAction("pianoRoll.nudgeRight")
    await settle()
    expect(lead()[0]).toBe("1920:60:240")
    usePianoRollStore.getState().setSnap("none")
    await runAction("pianoRoll.nudgeLeft")
    await settle()
    expect(lead()[0]).toBe("1919:60:240")
  })

  it("transposes by octaves and stops at the top of the keyboard", async () => {
    roll.editor.selectAll()
    await runAction("pianoRoll.octaveUp")
    await settle()
    expect(lead()).toEqual(["960:72:240", "1200:76:480", "1920:79:240"])
    for (let i = 0; i < 6; i++) await runAction("pianoRoll.octaveUp")
    await settle()
    // The highest note is on key 127 and the chord kept its shape.
    expect(lead()).toEqual(["960:120:240", "1200:124:480", "1920:127:240"])
  })

  it("quantizes starts and ends to the snap", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(250, 50, 200), n(490, 52, 500)],
    })
    const before = undoSteps()
    roll.editor.setSelection(
      notesOf("Lead")
        .slice(0, 2)
        .map((note) => note.id)
    )
    await runAction("pianoRoll.quantize")
    const starts = useNoteTools.getState().request!
    await applyNoteTool(starts, {
      type: "quantize",
      grid: 240,
      strength: 1,
      edge: "start",
      groove: "straight",
    })
    await settle()
    expect(lead().slice(0, 2)).toEqual(["240:50:200", "480:52:500"])
    await runAction("pianoRoll.quantizeEnds")
    const ends = useNoteTools.getState().request!
    await applyNoteTool(ends, {
      type: "quantize",
      grid: 240,
      strength: 1,
      edge: "end",
      groove: "straight",
    })
    await settle()
    expect(lead().slice(0, 2)).toEqual(["240:50:240", "480:52:480"])
    expect(undoSteps()).toBe(before + 2)
  })

  it("deletes and selects through actions", async () => {
    await runAction("pianoRoll.selectAll")
    expect(roll.editor.selection.size).toBe(3)
    expect(usePianoRollStore.getState().selectionCount).toBe(3)
    await runAction("pianoRoll.deselect")
    expect(roll.editor.selection.size).toBe(0)
    roll.editor.setSelection([notesOf("Lead")[1].id])
    const before = undoSteps()
    await runAction("pianoRoll.delete")
    await settle()
    expect(lead()).toEqual(["960:60:240", "1920:67:240"])
    expect(undoSteps()).toBe(before + 1)
    await undo()
    await redo()
    expect(lead()).toHaveLength(2)
  })

  it("drops notes from the selection when an undo removes them", async () => {
    await roll.click(at(300, 70))
    const added = notesOf("Lead").find((note) => note.key === 70)
    if (!added) throw new Error("no note drawn")
    roll.editor.setSelection([added.id, notesOf("Lead")[1].id])
    await undo()
    await settle()
    expect(roll.editor.selection.size).toBe(1)
    expect(selectedBrief()).toEqual(["960:60:240"])
  })

  it("commits a velocity drag as one command and leaves the pattern length alone", async () => {
    await dispatch({
      type: "addNotes",
      pattern: currentPattern().id,
      channel: channel("Lead").id,
      notes: [n(3600, 60, 960)],
    })
    const notes = roll.editor.notes
    const values = new Map<number, number>()
    paintValues(
      notes,
      "velocity",
      { tick: 0, value: 0.3 },
      { tick: 4000, value: 0.3 },
      0,
      values
    )
    const before = undoSteps()
    await roll.editor.update(
      notes,
      laneUpdates(notes, "velocity", values),
      "Change velocity"
    )
    expect(notesOf("Lead").map((note) => note.velocity)).toEqual([
      0.3, 0.3, 0.3, 0.3,
    ])
    expect(undoSteps()).toBe(before + 1)
    // One note hangs past the end; changing how loud it is does not move it.
    expect(currentPattern().lengthSteps).toBe(16)
  })
})

describe("the end of the longest pattern", () => {
  const END = MAX_PATTERN_TICKS
  const refused = () =>
    expect(toast.error).toHaveBeenLastCalledWith(
      "Notes cannot go past step 1,024",
      expect.objectContaining({
        description: expect.stringContaining("could never play"),
      })
    )

  beforeEach(() => vi.mocked(toast.error).mockClear())

  it("refuses to duplicate notes past it, and says so", async () => {
    // A phrase 500 steps long: one copy fits, the next would end at 2,000.
    await openLead([n(0, 60), n(499 * 240, 64)])
    roll.editor.selectAll()
    await runAction("pianoRoll.duplicate")
    await settle()
    expect(lead()).toHaveLength(4)
    expect(toast.error).not.toHaveBeenCalled()
    // The pattern grew to the bar that holds the copy.
    expect(currentPattern().lengthSteps).toBe(1008)

    roll.editor.selectAll()
    const before = undoSteps()
    await runAction("pianoRoll.duplicate")
    await settle()
    refused()
    expect(lead()).toHaveLength(4)
    expect(undoSteps()).toBe(before)
    expect(Math.max(...notesOf("Lead").map((note) => note.start))).toBeLessThan(
      END
    )
  })

  it("refuses to paste past it", async () => {
    await openLead([n(0, 60), n(480, 64)])
    roll.editor.selectAll()
    await runAction("pianoRoll.copy")
    roll.session.setPlayhead(END - 240)
    const before = undoSteps()
    await runAction("pianoRoll.paste")
    await settle()
    refused()
    expect(lead()).toHaveLength(2)
    expect(undoSteps()).toBe(before)
  })

  it("refuses to nudge or drag a note past it, and puts the note back", async () => {
    await openLead([n(END - 240, 60)])
    roll.editor.selectAll()
    const before = undoSteps()
    await runAction("pianoRoll.nudgeRight")
    await settle()
    refused()
    expect(lead()).toEqual([`${END - 240}:60:240`])

    vi.mocked(toast.error).mockClear()
    await roll.drag(at(END - 120, 60), at(END + 600, 60))
    refused()
    expect(lead()).toEqual([`${END - 240}:60:240`])
    expect(roll.editor.drag).toBeNull()
    // Dragging the end of the note out is no way round it either.
    vi.mocked(toast.error).mockClear()
    await roll.drag(at(END - 2, 60), at(END + 480, 60))
    refused()
    expect(lead()).toEqual([`${END - 240}:60:240`])
    expect(undoSteps()).toBe(before)

    // Back towards the start is fine.
    await runAction("pianoRoll.nudgeLeft")
    await settle()
    expect(lead()).toEqual([`${END - 480}:60:240`])
  })

  it("refuses to draw a note past it", async () => {
    await openLead()
    const before = undoSteps()
    await roll.click(at(END + 10, 60))
    refused()
    expect(lead()).toEqual([])
    expect(undoSteps()).toBe(before)

    // The last step of the longest pattern still takes a note.
    await roll.click(at(END - 230, 60))
    expect(lead()).toEqual([`${END - 240}:60:240`])
    expect(currentPattern().lengthSteps).toBe(1024)
  })

  it("paints up to it, leaves the rest out and says why", async () => {
    await openLead()
    usePianoRollStore.getState().setTool("paint")
    await roll.drag(at(END - 470, 62), [at(END - 200, 62), at(END + 500, 62)])
    refused()
    expect(lead()).toEqual([`${END - 480}:62:240`, `${END - 240}:62:240`])
  })
})

describe("shortcuts", () => {
  const otherDelete = vi.fn()
  let root: HTMLElement
  let otherPanel: HTMLElement
  let outside: HTMLElement

  function press(init: KeyboardEventInit, target: Element = root) {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init })
    )
  }

  beforeEach(async () => {
    roll.stop()
    otherDelete.mockClear()
    // Another panel gives Delete to an action of its own, as the channel
    // rack does.
    roll = await startRoll({
      earlier: [
        {
          id: "other.delete",
          title: "Delete in another panel",
          section: "Test",
          scope: "channelRack",
          defaultShortcut: "Delete",
          run: otherDelete,
        },
      ],
    })
    await openLead([n(960, 60), n(1200, 64)])
    root = document.createElement("div")
    root.tabIndex = 0
    root.dataset.shortcutScope = "pianoRoll"
    otherPanel = document.createElement("div")
    otherPanel.dataset.shortcutScope = "channelRack"
    outside = document.createElement("button")
    otherPanel.append(outside)
    document.body.append(root, otherPanel)
    roll.editor.selectAll()
  })
  afterEach(() => {
    root.remove()
    otherPanel.remove()
  })

  it("gives Delete to the notes while the piano roll has the keyboard", async () => {
    root.focus()
    press({ key: "Delete", code: "Delete" })
    await settle()
    expect(lead()).toEqual([])
    expect(otherDelete).not.toHaveBeenCalled()
  })

  it("leaves the key to the other panel when the focus is there", async () => {
    outside.focus()
    press({ key: "Delete", code: "Delete" }, outside)
    await settle()
    expect(lead()).toHaveLength(2)
    expect(otherDelete).toHaveBeenCalledTimes(1)
  })

  it("keeps the keyboard after a click on the grid leaves the focus nowhere", async () => {
    outside.focus()
    root.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }))
    outside.blur()
    press({ key: "Delete", code: "Delete" }, document.body)
    await settle()
    expect(lead()).toEqual([])
    expect(otherDelete).not.toHaveBeenCalled()
  })

  it("shows each action the key it has inside the piano roll", () => {
    expect(shortcutLabel("pianoRoll.delete")).toBe("Del")
    expect(shortcutLabel("pianoRoll.duplicate")).toBe("Ctrl+D")
    expect(shortcutLabel("pianoRoll.toolDraw")).toBe("D")
    expect(shortcutLabel("other.delete")).toBe("Del")
    useUiStore.getState().setKeymap("fl")
    expect(shortcutLabel("pianoRoll.duplicate")).toBe("Ctrl+B")
    expect(shortcutLabel("pianoRoll.toolDraw")).toBe("P")
    expect(shortcutLabel("pianoRoll.deselect")).toBe("Ctrl+D")
  })

  it("moves notes with the arrows and leaves typing alone", async () => {
    root.focus()
    press({ key: "ArrowRight", code: "ArrowRight" })
    press({ key: "ArrowUp", code: "ArrowUp" })
    await settle()
    expect(lead()).toEqual(["1200:61:240", "1440:65:240"])
    const field = document.createElement("input")
    root.append(field)
    field.focus()
    press({ key: "ArrowRight", code: "ArrowRight" }, field)
    press({ key: "Delete", code: "Delete" }, field)
    await settle()
    expect(lead()).toEqual(["1200:61:240", "1440:65:240"])
  })

  it("switches tools with letters, FL's letters in the FL keymap", () => {
    root.focus()
    press({ key: "b", code: "KeyB" })
    expect(usePianoRollStore.getState().tool).toBe("paint")
    press({ key: "e", code: "KeyE" })
    expect(usePianoRollStore.getState().tool).toBe("erase")
    press({ key: "s", code: "KeyS" })
    expect(usePianoRollStore.getState().tool).toBe("select")
    press({ key: "d", code: "KeyD" })
    expect(usePianoRollStore.getState().tool).toBe("draw")

    useUiStore.getState().setKeymap("fl")
    press({ key: "d", code: "KeyD" })
    expect(usePianoRollStore.getState().tool).toBe("erase")
    press({ key: "e", code: "KeyE" })
    expect(usePianoRollStore.getState().tool).toBe("select")
    press({ key: "p", code: "KeyP" })
    expect(usePianoRollStore.getState().tool).toBe("draw")
  })

  it("uses FL's editing keys in the FL keymap", async () => {
    useUiStore.getState().setKeymap("fl")
    root.focus()
    press({ key: "b", code: "KeyB", ctrlKey: true })
    await settle()
    expect(lead()).toHaveLength(4)
    press({ key: "ArrowUp", code: "ArrowUp", ctrlKey: true })
    await settle()
    expect(selectedBrief().map((text) => text.split(":")[1])).toEqual([
      "72",
      "76",
    ])
    press({ key: "d", code: "KeyD", ctrlKey: true })
    expect(roll.editor.selection.size).toBe(0)
  })

  it("does nothing outside the piano roll tab", async () => {
    useUiStore.getState().showCenterTab("channelRack")
    root.focus()
    press({ key: "ArrowRight", code: "ArrowRight" })
    press({ key: "b", code: "KeyB" })
    await settle()
    expect(lead()).toEqual(["960:60:240", "1200:64:240"])
    expect(usePianoRollStore.getState().tool).toBe("draw")
  })
})

describe("the step sequencer edits the same notes", () => {
  it("shows steps as notes on C5", async () => {
    const kick = channel("Kick")
    const steps = litSteps(notesOf("Kick"), 16)
    expect(brief(roll.editor.notes)).toEqual(
      steps.flatMap((lit, step) => (lit ? [`${step * 240}:60:240`] : []))
    )
    await dispatch({
      type: "toggleStep",
      pattern: currentPattern().id,
      channel: kick.id,
      step: 5,
    })
    expect(brief(roll.editor.notes)).toContain("1200:60:240")
  })

  it("lights a step for a note drawn on C5 at a step", async () => {
    expect(litSteps(notesOf("Kick"), 16)[6]).toBe(false)
    await roll.click(at(6 * 240 + 30, 60))
    const drawn = notesOf("Kick").find((note) => note.start === 6 * 240)
    expect(drawn && isStepNote(drawn)).toBe(true)
    expect(litSteps(notesOf("Kick"), 16)[6]).toBe(true)
    // Turning the step off in the rack removes the note here.
    await dispatch({
      type: "toggleStep",
      pattern: currentPattern().id,
      channel: channel("Kick").id,
      step: 6,
    })
    expect(brief(roll.editor.notes)).not.toContain("1440:60:240")
  })
})

describe("the panel", () => {
  beforeEach(() => {
    // jsdom has no canvas; the grid reports that instead of drawing.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  })
  afterEach(() => vi.restoreAllMocks())

  function renderPanel() {
    return render(
      <TooltipProvider>
        <PianoRollPanel />
      </TooltipProvider>
    )
  }

  it("asks for a channel when the project has none", async () => {
    roll.stop()
    roll = await startRoll({ project: emptyProject("Empty"), channel: null })
    renderPanel()
    expect(
      screen.getByText("There is no channel to write notes for")
    ).toBeInTheDocument()
    expect(screen.queryByRole("toolbar", { name: "Piano roll" })).toBeNull()
    expect(screen.getByRole("button", { name: "Add channel" })).toBeEnabled()
  })

  it("edits the selected channel, or the first one when none is selected", async () => {
    useUiStore.getState().selectChannel(null)
    renderPanel()
    await settle()
    expect(screen.getByRole("toolbar", { name: "Piano roll" })).toBeVisible()
    expect(useUiStore.getState().selectedChannel).toBe(channel("Kick").id)
    expect(screen.getByRole("combobox", { name: "Channel" })).toHaveTextContent(
      "Kick"
    )
    for (const name of [
      "Draw tool",
      "Paint tool",
      "Select tool",
      "Erase tool",
    ]) {
      expect(screen.getByRole("button", { name })).toBeEnabled()
    }
    expect(screen.getByRole("button", { name: "Draw tool" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )
    expect(
      screen.getByRole("scrollbar", { name: "Scroll through time" })
    ).toBeInTheDocument()
    expect(
      screen.getByRole("group", { name: "Piano keyboard" })
    ).toBeInTheDocument()
  })

  it("says so when the grid cannot start", async () => {
    renderPanel()
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The note grid could not start"
    )
  })
})
