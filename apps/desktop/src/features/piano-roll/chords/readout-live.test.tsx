import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { dispatch } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import PianoRollPanel from "../index"
import { currentSession } from "../session"
import { useStepEntryStore } from "../step-entry"
import { channel, currentPattern, notesOf, startRoll, undoSteps } from "../test-utils"
import { useTypingKeyboardStore } from "../typing-keyboard"
import { heldKeys, holdKey, releaseHeldKeys } from "./held-keys"
import { useChordDisplayStore } from "./readout-panel"

/*
 * The whole piano roll over the WASM-backed test app, with the real audition
 * path: what the typing keyboard, step entry and the key gutter hold is what
 * the chord readout and its display name.
 */
let roll: Awaited<ReturnType<typeof startRoll>>
let grid: HTMLElement

beforeEach(async () => {
  useTypingKeyboardStore.setState({ enabled: false, baseKey: 60 })
  useStepEntryStore.setState({ enabled: false, cursor: null })
  roll = await startRoll()
  render(
    <TooltipProvider>
      <PianoRollPanel />
    </TooltipProvider>
  )
  grid = screen.getByRole("application", { name: "Note grid" })
  act(() => grid.focus())
})

afterEach(() => {
  cleanup()
  roll.stop()
  releaseHeldKeys()
  useChordDisplayStore.setState({ open: false })
  useTypingKeyboardStore.setState({ enabled: false, baseKey: 60 })
  useStepEntryStore.setState({ enabled: false, cursor: null })
  vi.restoreAllMocks()
})

const press = (key: string) =>
  fireEvent.keyDown(grid, { key, code: `Key${key.toUpperCase()}` })
const lift = (key: string) =>
  fireEvent.keyUp(grid, { key, code: `Key${key.toUpperCase()}` })
const typing = () =>
  fireEvent.click(screen.getByRole("button", { name: "Typing" }))
const readout = () => screen.getByLabelText("Chord")
const kick = () => channel("Kick").id

function display() {
  const dialog = screen.getByRole("dialog", { name: "Chord display" })
  const fact = (term: string) =>
    within(dialog).queryByText(term, { selector: "dt" })?.nextElementSibling
      ?.textContent ?? null
  return {
    symbol: dialog.querySelector('[data-slot="chord-symbol"]')?.textContent,
    name: dialog.querySelector('[data-slot="chord-name"]')?.textContent,
    source: fact("Source"),
    bass: fact("Bass"),
    pitchClasses: fact("Pitch classes"),
  }
}

describe("chord readout over the mounted piano roll", () => {
  it("clears native hardware notes on Stop while retaining a separate audition owner", async () => {
    act(() => {
      roll.backend.playMidiHardwareNote({ channel: kick(), key: 60, velocity: 100 })
      roll.backend.playMidiHardwareNote({ channel: kick(), key: 64, velocity: 100 })
      holdKey(kick(), 72)
    })
    await act(async () => { await roll.backend.transportStop(); await settle() })
    expect(heldKeys(kick())).toEqual([72])
  })
  it("names accepted native MIDI notes, filters destinations and clears on project replacement", async () => {
    const before = undoSteps()
    const send = (key: number, velocity: number, channelId = kick()) => {
      act(() => roll.backend.playMidiHardwareNote({ channel: channelId, key, velocity }))
    }
    send(60, 100)
    send(64, 100)
    send(67, 100)
    send(60, 110)
    expect(readout()).toHaveTextContent("C major")
    send(72, 100, channel("Clap").id)
    expect(readout()).toHaveTextContent("C major")
    // Accepted ups arrive after the native sustain latch releases them.
    send(60, 0)
    send(64, 0)
    expect(heldKeys(kick())).toEqual([67])
    act(() => announceProjectReplaced())
    expect(heldKeys(kick())).toEqual([])
    expect(heldKeys(channel("Clap").id)).toEqual([])
    expect(undoSteps()).toBe(before)
  })
  it("names typing-keyboard keys as they go down and up, in the toolbar and the display", async () => {
    await act(async () => {
      await dispatch({
        type: "removeNotes",
        pattern: currentPattern().id,
        channel: kick(),
        notes: notesOf("Kick").map((note) => note.id),
      })
      await settle()
    })
    const steps = undoSteps()
    const send = vi.spyOn(roll.backend, "dispatch")
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    expect(readout()).toHaveTextContent("—")
    fireEvent.click(readout())
    expect(display()).toMatchObject({ symbol: "—", name: "No chord" })
    // Opening the display leaves the keyboard focus for the notes.
    act(() => grid.focus())
    typing()
    act(() => grid.focus())

    press("a")
    expect(readout()).toHaveTextContent(/^C$/)
    expect(display()).toMatchObject({ symbol: "C", source: "Played", bass: "C" })
    press("d")
    expect(readout()).toHaveTextContent("C–E major third")
    press("g")
    expect(readout()).toHaveTextContent("C major")
    expect(display()).toEqual({
      symbol: "C",
      name: "C major",
      source: "Played",
      bass: "C",
      pitchClasses: "C · E · G",
    })
    press("j")
    expect(readout()).toHaveTextContent("C major seventh")
    expect(display()).toMatchObject({ symbol: "Cmaj7", pitchClasses: "C · E · G · B" })
    lift("a")
    expect(readout()).toHaveTextContent("E minor")
    expect(display()).toMatchObject({ symbol: "Em", source: "Played", bass: "E" })
    lift("d")
    lift("g")
    expect(readout()).toHaveTextContent(/^B$/)
    lift("j")
    expect(readout()).toHaveTextContent("—")
    expect(display()).toMatchObject({ symbol: "—", name: "No chord", source: null })

    expect(on).toHaveBeenCalledTimes(4)
    expect(off).toHaveBeenCalledTimes(4)
    expect(heldKeys(kick())).toEqual([])
    expect(send).not.toHaveBeenCalled()
    expect(undoSteps()).toBe(steps)
    expect(notesOf("Kick")).toEqual([])
  })

  it("shows the selection again once the played keys are released", async () => {
    let created: number[] = []
    await act(async () => {
      created = (await dispatch({
        type: "addNotes",
        pattern: currentPattern().id,
        channel: kick(),
        notes: [65, 69, 72].map((key) => ({ key, start: 0, length: 120 })),
      }))!.created
      await settle()
    })
    const steps = undoSteps()
    fireEvent.click(readout())
    act(() => currentSession()!.editor.setSelection(created))
    expect(readout()).toHaveTextContent("F major")
    expect(display()).toMatchObject({ symbol: "F", source: "Selected" })
    act(() => grid.focus())
    typing()
    act(() => grid.focus())
    press("s")
    expect(readout()).toHaveTextContent(/^D$/)
    expect(display()).toMatchObject({ symbol: "D", source: "Played" })
    lift("s")
    expect(readout()).toHaveTextContent("F major")
    expect(display()).toMatchObject({ symbol: "F", source: "Selected" })
    act(() => currentSession()!.editor.setSelection([]))
    expect(display().source).not.toBe("Selected")
    expect(undoSteps()).toBe(steps)
  })

  it.each(["keyup", "toggle", "focus", "window", "project", "channel", "unmount"])(
    "lets go of typed keys on %s",
    async (reason) => {
      typing()
      act(() => grid.focus())
      press("a")
      press("d")
      expect(heldKeys(kick())).toEqual([60, 64])
      expect(readout()).toHaveTextContent("C–E major third")
      const id = kick()
      if (reason === "keyup") {
        lift("a")
        lift("d")
      }
      if (reason === "toggle") typing()
      if (reason === "focus")
        act(() => screen.getByRole("button", { name: "Typing" }).focus())
      if (reason === "window") fireEvent.blur(window)
      if (reason === "project") act(() => announceProjectReplaced())
      if (reason === "channel") {
        act(() => useUiStore.getState().selectChannel(channel("Clap").id))
        await act(settle)
      }
      if (reason === "unmount") cleanup()
      expect(heldKeys(id)).toEqual([])
      if (reason !== "unmount" && reason !== "project") {
        expect(readout()).not.toHaveTextContent("C–E major third")
      }
      // A late key-up for a key already let go changes nothing.
      lift("a")
      lift("d")
      expect(heldKeys(id)).toEqual([])
    }
  )

  it("names the key held in step entry while it is down", async () => {
    fireEvent.click(screen.getByRole("button", { name: "Step entry" }))
    act(() => grid.focus())
    press("a")
    expect(heldKeys(kick())).toEqual([60])
    expect(readout()).toHaveTextContent(/^C$/)
    lift("a")
    await act(settle)
    expect(heldKeys(kick())).toEqual([])
  })

  it("names a key played on the gutter keyboard while it is down", () => {
    const key = document.querySelector<HTMLElement>('[data-key="67"]')!
    expect(key).not.toBeNull()
    act(() => key.focus())
    fireEvent.keyDown(key, { key: "Enter" })
    expect(heldKeys(kick())).toEqual([67])
    expect(readout()).toHaveTextContent(/^G$/)
    fireEvent.keyUp(key, { key: "Enter" })
    expect(heldKeys(kick())).toEqual([])
    // The keyboard also lets go when the window loses focus mid-press.
    fireEvent.keyDown(key, { key: "Enter" })
    expect(heldKeys(kick())).toEqual([67])
    fireEvent.blur(window)
    expect(heldKeys(kick())).toEqual([])
  })
})
