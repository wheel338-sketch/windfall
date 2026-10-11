import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { settle } from "@/test/harness"
import { LfoDialog, openNoteLfo, openEventLfo, closeLfoDialog } from "@/features/automation/lfo-dialog"
import { NoteToolsDialog } from "./note-tools-dialog"
import { closeNoteTools, openNoteTools } from "./note-tools"
import { channel, currentPattern, notesOf, startRoll, undoSteps } from "./test-utils"
import { RiffGeneratorDialog } from "./riff-generator-dialog"
import { closeRiffGenerator, openRiffGenerator } from "./riff-generator"
import { SheetExportDialog } from "./score/export-dialog"
import { closeSheetExport, exportSheetMusic } from "./sheet-export"
import { heldKeys } from "./chords/held-keys"
import { NoteCurvesDialog, openNoteCurves, closeNoteCurves } from "./note-curves"

let roll: Awaited<ReturnType<typeof startRoll>>
beforeEach(async () => {
  roll = await startRoll()
  await dispatch({ type: "addChannel", name: "Lead" })
  await dispatch({ type: "addNotes", pattern: currentPattern().id, channel: channel("Lead").id,
    notes: [{ start: 0, length: 480, key: 60 }, { start: 960, length: 480, key: 72 }] })
  roll.show("Lead")
  roll.editor.setSelection(notesOf("Lead").map((note) => note.id))
  render(<><NoteToolsDialog /><LfoDialog /><RiffGeneratorDialog /><SheetExportDialog /><NoteCurvesDialog /></>)
})
afterEach(() => { cleanup(); closeNoteTools(); closeLfoDialog(); closeRiffGenerator(); closeSheetExport(); closeNoteCurves(); roll.stop(); vi.useRealTimers(); vi.restoreAllMocks() })
const field = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } })
async function click(label: string) { await act(async () => { fireEvent.click(screen.getByRole("button", { name: label, exact: true })); await settle() }) }
async function choose(label: string, value: string) {
  const user = userEvent.setup()
  await user.click(screen.getByRole("combobox", { name: label }))
  await user.click(screen.getByRole("option", { name: value, exact: true }))
}

it.each([
  { source: "wheel", parameter: "finePitchCents" as const, label: "Fine pitch (cents)", values: [0, 100, 200], changed: 125 },
  { source: "pressure", parameter: "modulationY" as const, label: "Modulation Y", values: [.2, .5, .8], changed: .7 },
  { source: "CC1", parameter: "modulationY" as const, label: "Modulation Y", values: [.1, .4, .6], changed: .7 },
  { source: "CC74", parameter: "modulationX" as const, label: "Modulation X", values: [.1, .5, .9], changed: .7 },
])("displays and edits canonical recorded $source expression with one undo and save/reopen", async ({ parameter, label, values, changed }) => {
  // The capture worker supplies the same canonical data/command as MIDI import.
  // No separate piano-roll hardware recorder or private expression model.
  const expected = structuredClone(notesOf("Lead"))
  await act(async () => {
    await dispatch({ type: "setNoteExpressionCurves", pattern: currentPattern().id, channel: channel("Lead").id,
      expected, expectedCurves: [], curves: [{ note: expected[0].id, parameter,
        points: values.map((value, index) => ({ position: index / 2, value, curve: 0, hold: false })) }] })
    await settle()
  })
  const captured = structuredClone(currentPattern().noteCurves)
  const before = undoSteps()
  act(openNoteCurves)
  await choose("Note curve property", label)
  const graph = screen.getByRole("group", { name: `${label} note curve` })
  expect(graph.querySelectorAll("circle")).toHaveLength(3)
  expect(graph.querySelectorAll("circle")[1].getAttribute("cx")).toBe("300")
  fireEvent.focus(screen.getByRole("button", { name: /Curve point 2:/ }))
  expect(screen.getByLabelText("Value")).toHaveValue(values[1])
  field("Value", String(changed))
  await click("Apply curves")
  expect(currentPattern().noteCurves![0].points[1].value).toBeCloseTo(changed)
  expect(undoSteps()).toBe(before + 1)
  const edited = structuredClone(currentPattern())
  await roll.backend.projectSave("/projects/recorded-expression.windfall")
  await act(async () => { await undo(); await settle() })
  expect(currentPattern().noteCurves).toEqual(captured)
  await act(async () => { await redo(); await settle() })
  expect(currentPattern()).toEqual(edited)
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/recorded-expression.windfall"); await settle() })
  expect(currentPattern()).toEqual(edited)
})

it("mounted Portamento captures phrasing, applies once and survives undo/redo/reopen", async () => {
  const original = structuredClone(notesOf("Lead"))
  const before = undoSteps()
  act(() => openNoteTools("portamento"))
  field("Glide (ticks)", "240")
  field("Overlap (ticks)", "60")
  await click("Apply")
  const changed = structuredClone(notesOf("Lead"))
  expect(changed[0].length).toBe(1020)
  expect(changed[1].expression).toMatchObject({ articulation: "portamento", glideTicks: 240 })
  expect(undoSteps()).toBe(before + 1)
  await act(async () => { await undo(); await settle() })
  expect(notesOf("Lead")).toEqual(original)
  await act(async () => { await redo(); await settle() })
  expect(notesOf("Lead")).toEqual(changed)
  await roll.backend.projectSave("/projects/portamento.windfall")
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/portamento.windfall"); await settle() })
  expect(notesOf("Lead")).toEqual(changed)
})

it.each(["Sine", "Triangle", "Rising saw", "Falling saw", "Pulse", "Sample and hold"])("mounted %s LFO writes onset, note curves and pattern events as native single edits", async (shape) => {
  const before = undoSteps()
  act(openNoteLfo)
  await choose("LFO shape", shape)
  field("Period (ticks)", "720")
  await click("Write LFO")
  expect(undoSteps()).toBe(before + 1)
  await act(async () => { await undo(); await settle() })
  act(openNoteLfo)
  await choose("LFO shape", shape)
  await choose("LFO event property", "Modulation X")
  await choose("LFO note mode", "Curve along each note")
  await click("Write LFO")
  expect(currentPattern().noteCurves).toHaveLength(2)
  expect(currentPattern().noteCurves!.every((curve) => curve.parameter === "modulationX")).toBe(true)
  const curved = structuredClone(currentPattern())
  await roll.backend.projectSave("/projects/lfo-curves.windfall")
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/lfo-curves.windfall"); await settle() })
  expect(currentPattern()).toEqual(curved)
  const eventBefore = undoSteps()
  act(() => openEventLfo(currentPattern().id, { type: "channelPan", channel: channel("Lead").id }))
  await choose("LFO shape", shape)
  await click("Write LFO")
  expect(currentPattern().events).toHaveLength(1)
  expect(undoSteps()).toBe(eventBefore + 1)
  await act(async () => { await undo(); await settle() })
  expect(currentPattern().events ?? []).toEqual([])
})

it("mounts all eight riff stages, auditions/releases and atomically applies with history/file checks", async () => {
  const original = structuredClone(currentPattern())
  const before = undoSteps()
  act(openRiffGenerator)
  for (const stage of ["Progression", "Chords", "Arpeggio", "Mirror", "Levels", "Length", "Groove", "Fit"]) {
    await userEvent.setup().click(screen.getByRole("tab", { name: new RegExp(stage) }))
    if (["Mirror", "Length", "Groove"].includes(stage)) {
      fireEvent.click(screen.getByRole("switch", { name: `${stage} stage` }))
      if (stage === "Mirror") fireEvent.click(screen.getByRole("switch", { name: "Reverse each chord in time" }))
      if (stage === "Length") field("Multiplier (%)", "75")
      if (stage === "Groove") field("Swing (%)", "30")
    }
  }
  field("Seed", "17")
  await userEvent.setup().click(screen.getByRole("button", { name: "Replace the selection", exact: true }))
  vi.useFakeTimers()
  const on = vi.spyOn(roll.backend, "auditionNoteOn")
  const off = vi.spyOn(roll.backend, "auditionNoteOff")
  await click("Audition")
  await act(async () => { vi.advanceTimersByTime(1); await settle() })
  expect(on).toHaveBeenCalled()
  expect(heldKeys(channel("Lead").id).length).toBeGreaterThan(0)
  await click("Stop")
  expect(heldKeys(channel("Lead").id)).toEqual([])
  expect(off.mock.calls.length).toBe(on.mock.calls.length)
  vi.useRealTimers()
  await click("Apply")
  expect(undoSteps()).toBe(before + 1)
  expect(notesOf("Lead").length).toBeGreaterThan(2)
  const generated = structuredClone(currentPattern())
  await roll.backend.projectSave("/projects/riff.windfall")
  await act(async () => { await undo(); await settle() })
  expect(currentPattern()).toEqual(original)
  await act(async () => { await redo(); await settle() })
  expect(currentPattern()).toEqual(generated)
  await act(async () => { await undo(); await roll.backend.projectOpen("/projects/riff.windfall"); await settle() })
  expect(currentPattern()).toEqual(generated)
})

it.each(["cancel", "unmount", "replacement", "selection", "settings"])("releases every sounding riff audition note on %s", async (reason) => {
  act(openRiffGenerator)
  vi.useFakeTimers()
  const on = vi.spyOn(roll.backend, "auditionNoteOn")
  const off = vi.spyOn(roll.backend, "auditionNoteOff")
  await click("Audition")
  await act(async () => { vi.advanceTimersByTime(1); await settle() })
  expect(on).toHaveBeenCalled()
  await act(async () => {
    if (reason === "cancel") closeRiffGenerator()
    if (reason === "unmount") cleanup()
    if (reason === "replacement") announceProjectReplaced()
    if (reason === "selection") roll.editor.setSelection([])
    if (reason === "settings") field("Seed", "99")
    await settle()
  })
  expect(heldKeys(channel("Lead").id)).toEqual([])
  expect(off.mock.calls.length).toBe(on.mock.calls.length)
  await act(async () => { vi.advanceTimersByTime(100000); await settle() })
  expect(off.mock.calls.length).toBe(on.mock.calls.length)
})

it("refuses a raced riff source in Rust and releases on changed selection/replacement", async () => {
  act(openRiffGenerator)
  const real = roll.backend.dispatch.bind(roll.backend)
  vi.spyOn(roll.backend, "dispatch").mockImplementationOnce(async (command, gesture) => {
    await real({ type: "updateNotes", pattern: currentPattern().id, channel: channel("Lead").id,
      updates: [{ id: notesOf("Lead")[0].id, patch: { length: 481 } }] })
    return real(command, gesture)
  })
  const before = undoSteps()
  await click("Apply")
  expect(undoSteps()).toBe(before + 1)
  expect(notesOf("Lead")).toHaveLength(2)
  expect(notesOf("Lead")[0].length).toBe(481)
  act(closeRiffGenerator)
  act(openRiffGenerator)
  act(() => roll.editor.setSelection([notesOf("Lead")[0].id]))
  expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled()
  act(() => announceProjectReplaced())
  expect(screen.queryByRole("dialog")).toBeNull()
})

it("mounted score export saves the preview's real PDF, prints SVG and leaves the document unchanged", async () => {
  const before = useProjectStore.getState()
  const saved = vi.spyOn(roll.backend, "sheetMusicSave").mockResolvedValue("/exports/phrase.pdf")
  const print = vi.spyOn(window, "print").mockImplementation(() => {})
  act(exportSheetMusic)
  const page = screen.getByRole("img", { name: "Page 1 of 1" })
  expect(page.querySelectorAll("svg ellipse")).toHaveLength(3)
  await choose("Staves", "Treble staff")
  await click("Save PDF…")
  expect(saved).toHaveBeenCalledWith(expect.any(String), expect.stringContaining("%PDF-1.4"), "pdf")
  expect(saved.mock.calls[0][1]).toContain("/Count 1")
  await click("Print…")
  expect(print).toHaveBeenCalledOnce()
  expect(document.querySelector('[data-score-print="pages"] svg')).not.toBeNull()
  fireEvent(window, new Event("afterprint"))
  expect(document.querySelector('[data-score-print="pages"]')).toBeNull()
  expect(useProjectStore.getState()).toBe(before)
  act(() => announceProjectReplaced())
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
})
