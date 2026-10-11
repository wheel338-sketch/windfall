import { Profiler } from "react"
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { runAction } from "@/lib/actions"
import { dispatch } from "@/lib/store/project"
import { announceProjectReplaced } from "@/lib/store/replaced"
import { settle } from "@/test/harness"

import { auditionOff, auditionOn } from "../audition"
import { SessionContext } from "../context"
import {
  at,
  channel,
  currentPattern,
  notesOf,
  startRoll,
  undoSteps,
} from "../test-utils"
import { PianoRollToolbar } from "../toolbar"
import { heldKeys, releaseHeldKeys } from "./held-keys"
import { ChordReadout } from "./readout-control"
import { useChordDisplayStore } from "./readout-panel"

let roll: Awaited<ReturnType<typeof startRoll>>
beforeEach(async () => {
  roll = await startRoll()
  await dispatch({ type: "addChannel", name: "Lead" })
  roll.show("Lead")
})
afterEach(() => {
  cleanup()
  roll.stop()
  releaseHeldKeys()
  useChordDisplayStore.setState({ open: false })
  vi.restoreAllMocks()
})

function mount() {
  return render(
    <SessionContext.Provider value={roll.session}>
      <PianoRollToolbar channelId={channel("Lead").id} readoutRef={null} />
    </SessionContext.Provider>
  )
}

async function add(keys: number[], start = 0, length = 240, name = "Lead") {
  return (await dispatch({
    type: "addNotes",
    pattern: currentPattern().id,
    channel: channel(name).id,
    notes: keys.map((key) => ({ key, start, length })),
  }))!.created
}

const readout = () => screen.getByLabelText("Chord")
const display = () => screen.getByRole("dialog", { name: "Chord display" })
const slot = (name: string) =>
  display().querySelector(`[data-slot="${name}"]`)
/** The value beside a term of the display, or null when the term is absent. */
function fact(term: string): string | null {
  const found = within(display()).queryByText(term, { selector: "dt" })
  return found?.nextElementSibling?.textContent ?? null
}
function facts() {
  return {
    symbol: slot("chord-symbol")?.textContent,
    name: slot("chord-name")?.textContent,
    source: fact("Source"),
    root: fact("Root"),
    bass: fact("Bass"),
    pitchClasses: fact("Pitch classes"),
    intervals: fact("Intervals") ?? fact("Intervals above bass"),
    leftOut: fact("Left out"),
  }
}
const lead = () => channel("Lead").id
const hold = (...keys: number[]) =>
  act(() => keys.forEach((key) => auditionOn(lead(), key, 0.8)))
const release = (...keys: number[]) =>
  act(() => keys.forEach((key) => auditionOff(lead(), key)))

describe("persistent toolbar chord readout", () => {
  it("updates with selection while the chord tools dialog is closed", async () => {
    const selected = await add([60, 64, 67], 240)
    await add([60, 63, 67])
    roll.session.setPlayhead(10)
    mount()
    expect(readout()).toHaveTextContent("C minor")
    expect(
      screen.getByRole("button", { name: "Chords" }).nextElementSibling
    ).toBe(readout())
    act(() => roll.editor.setSelection(selected))
    expect(readout()).toHaveTextContent("C major")
    act(() => roll.session.setPlayhead(300))
    expect(readout()).toHaveTextContent("C major")
    act(() => {
      roll.session.setPlayhead(10)
      roll.editor.setSelection([])
    })
    expect(readout()).toHaveTextContent("C minor")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("updates at the floored playhead and excludes a note at its end", async () => {
    await add([60, 63, 67], 10, 10)
    await add([64], 0, 10)
    await add([71], 11, 10)
    mount()
    act(() => roll.session.setPlayhead(10.9))
    expect(readout()).toHaveTextContent("C minor")
    act(() => roll.session.setPlayhead(21))
    expect(readout()).toHaveTextContent("—")
  })

  it("excludes other channels and follows changes of the open lane", async () => {
    await dispatch({ type: "addChannel", name: "Other" })
    await add([60, 63, 67])
    await add([64], 0, 240, "Other")
    roll.session.setPlayhead(10)
    mount()
    expect(readout()).toHaveTextContent("C minor")
    act(() => roll.show("Other"))
    // One pitch class is named as a note.
    expect(readout()).toHaveTextContent(/^E$/)
  })

  it("shows an em dash with a null playhead without dispatching", async () => {
    await add([60, 64, 67])
    const dispatchSpy = vi.spyOn(roll.backend, "dispatch")
    mount()
    expect(readout()).toHaveTextContent("—")
    act(() => roll.session.setPlayhead(10))
    expect(readout()).toHaveTextContent("C major")
    act(() => roll.session.setPlayhead(null))
    expect(readout()).toHaveTextContent("—")
    expect(dispatchSpy).not.toHaveBeenCalled()
  })

  it("names inversions, sevenths and larger chords, not only triads", async () => {
    const seventh = await add([64, 67, 71, 72])
    const thirteenth = await add([48, 52, 58, 62, 69], 240)
    mount()
    act(() => roll.editor.setSelection(seventh))
    expect(readout()).toHaveTextContent("C major seventh / E")
    act(() => roll.editor.setSelection(thirteenth))
    expect(readout()).toHaveTextContent("C dominant thirteenth")
    act(() => roll.editor.setSelection([seventh[0]]))
    expect(readout()).toHaveTextContent(/^E$/)
    // The button is named for what it is and described by the chord it shows.
    expect(readout()).toHaveAccessibleDescription("E")
  })
})

describe("live held keys in the toolbar readout", () => {
  it("puts held keys before the selection and the playhead, and gives way on release", async () => {
    const selected = await add([60, 64, 67], 240)
    await add([60, 63, 67])
    const before = { steps: undoSteps(), notes: structuredClone(notesOf("Lead")) }
    const send = vi.spyOn(roll.backend, "dispatch")
    const on = vi.spyOn(roll.backend, "auditionNoteOn")
    const off = vi.spyOn(roll.backend, "auditionNoteOff")
    roll.session.setPlayhead(10)
    mount()
    expect(readout()).toHaveTextContent("C minor")

    hold(65)
    expect(readout()).toHaveTextContent(/^F$/)
    hold(69)
    expect(readout()).toHaveTextContent("F–A major third")
    hold(72)
    expect(readout()).toHaveTextContent("F major")
    // The selection and the playhead wait behind the keys that are down.
    act(() => roll.editor.setSelection(selected))
    act(() => roll.session.setPlayhead(300))
    expect(readout()).toHaveTextContent("F major")
    release(65)
    expect(readout()).toHaveTextContent("A–C minor third")
    release(69, 72)
    expect(readout()).toHaveTextContent("C major")
    // The selection still wins over the playhead; without it the notes
    // under the playhead are named.
    act(() => roll.session.setPlayhead(10))
    expect(readout()).toHaveTextContent("C major")
    act(() => roll.editor.setSelection([]))
    expect(readout()).toHaveTextContent("C minor")
    act(() => roll.session.setPlayhead(600))
    expect(readout()).toHaveTextContent("—")

    // Audition itself is unchanged: one backend call per key, each way.
    expect(on.mock.calls).toEqual([
      [lead(), 65, 0.8],
      [lead(), 69, 0.8],
      [lead(), 72, 0.8],
    ])
    expect(off.mock.calls).toEqual([
      [lead(), 65],
      [lead(), 69],
      [lead(), 72],
    ])
    expect(send).not.toHaveBeenCalled()
    expect(undoSteps()).toBe(before.steps)
    expect(notesOf("Lead")).toEqual(before.notes)
  })

  it("follows the note auditioned while it is drawn in the grid", async () => {
    mount()
    expect(readout()).toHaveTextContent("—")
    act(() => roll.editor.pointerDown(at(0, 64), "left"))
    expect(heldKeys(lead())).toEqual([64])
    expect(readout()).toHaveTextContent(/^E$/)
    await act(async () => {
      roll.editor.pointerUp(at(0, 64))
      await settle()
    })
    expect(heldKeys(lead())).toEqual([])
  })

  it("shows only keys held on the open channel", async () => {
    await dispatch({ type: "addChannel", name: "Other" })
    mount()
    act(() => auditionOn(channel("Other").id, 60, 0.8))
    expect(readout()).toHaveTextContent("—")
    act(() => roll.show("Other"))
    expect(readout()).toHaveTextContent(/^C$/)
    act(() => auditionOff(channel("Other").id, 60))
    expect(readout()).toHaveTextContent("—")
  })

  it.each(["channel", "project", "unmount"])(
    "lets go of held keys whose release never arrives on %s",
    async (reason) => {
      await dispatch({ type: "addChannel", name: "Other" })
      const mounted = mount()
      hold(60, 64, 67)
      expect(readout()).toHaveTextContent("C major")
      if (reason === "channel") act(() => roll.show("Other"))
      if (reason === "project") act(() => announceProjectReplaced())
      if (reason === "unmount") mounted.unmount()
      expect(heldKeys(lead())).toEqual([])
      if (reason === "channel") {
        expect(readout()).toHaveTextContent("—")
        act(() => roll.show("Lead"))
        expect(readout()).toHaveTextContent("—")
      }
    }
  )
})

describe("chord display", () => {
  it("opens from the readout with the chord's symbol, name, bass, pitch classes, intervals and source", async () => {
    const selected = await add([64, 67, 71, 72])
    mount()
    act(() => roll.editor.setSelection(selected))
    expect(readout()).toHaveAttribute("aria-expanded", "false")
    fireEvent.click(readout())
    expect(readout()).toHaveAttribute("aria-expanded", "true")
    expect(facts()).toEqual({
      symbol: "Cmaj7/E",
      name: "C major seventh / E",
      source: "Selected",
      root: "C",
      bass: "E",
      pitchClasses: "C · E · G · B",
      intervals: "1 · 3 · 5 · 7",
      leftOut: null,
    })
    // The readout itself keeps the long name.
    expect(readout()).toHaveTextContent("C major seventh / E")
    fireEvent.click(readout())
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("follows selection, playhead and held keys while open, without a command or history", async () => {
    const major = await add([60, 64, 67], 240)
    await add([60, 63, 67, 70])
    const before = { steps: undoSteps(), notes: structuredClone(notesOf("Lead")) }
    const send = vi.spyOn(roll.backend, "dispatch")
    mount()
    fireEvent.click(readout())
    expect(facts()).toMatchObject({ symbol: "—", name: "No chord" })
    expect(display()).toHaveTextContent(/Hold keys, select notes or move the playhead/)

    act(() => roll.session.setPlayhead(10))
    expect(facts()).toMatchObject({
      symbol: "Cm7",
      name: "C minor seventh",
      source: "Playhead",
      bass: "C",
      pitchClasses: "C · D♯ · G · A♯",
      intervals: "1 · ♭3 · 5 · ♭7",
    })
    act(() => roll.session.setPlayhead(250))
    expect(facts()).toMatchObject({ symbol: "C", source: "Playhead" })

    act(() => roll.editor.setSelection([major[1], major[2]]))
    expect(facts()).toMatchObject({
      symbol: "E–G",
      name: "E–G minor third",
      source: "Selected",
      root: "E",
      bass: "E",
      intervals: "1 · ♭3",
    })

    hold(60, 64, 70)
    expect(facts()).toEqual({
      symbol: "C7",
      name: "C dominant seventh",
      source: "Played",
      root: "C",
      bass: "C",
      pitchClasses: "C · E · A♯",
      intervals: "1 · 3 · ♭7",
      leftOut: "G",
    })
    release(60)
    hold(61, 62)
    expect(facts()).toEqual({
      symbol: "?",
      name: "Unknown chord (C♯, D, E, A♯)",
      source: "Played",
      root: "—",
      bass: "C♯",
      pitchClasses: "C♯ · D · E · A♯",
      intervals: "1 · ♭2 · ♭3 · 6",
      leftOut: null,
    })
    release(61, 62, 64, 70)
    expect(facts()).toMatchObject({ symbol: "E–G", source: "Selected" })
    act(() => roll.editor.setSelection([]))
    expect(facts()).toMatchObject({ symbol: "C", source: "Playhead" })
    act(() => roll.session.setPlayhead(null))
    expect(facts()).toMatchObject({ symbol: "—", name: "No chord", source: null })

    expect(send).not.toHaveBeenCalled()
    expect(undoSteps()).toBe(before.steps)
    expect(notesOf("Lead")).toEqual(before.notes)
  })

  it("stays up while the user works around it, and closes from the readout or Escape on it", async () => {
    await add([60, 64, 67])
    mount()
    fireEvent.click(readout())
    expect(display()).toBeInTheDocument()
    // Presses and focus elsewhere are the editor's: the display is pinned.
    const elsewhere = screen.getByRole("button", { name: "Chords" })
    fireEvent.pointerDown(document.body)
    fireEvent.mouseDown(document.body)
    fireEvent.click(document.body)
    act(() => elsewhere.focus())
    fireEvent.keyDown(elsewhere, { key: "Escape" })
    expect(display()).toBeInTheDocument()
    act(() => readout().focus())
    fireEvent.keyDown(readout(), { key: "Escape" })
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("opens from the palette action, which the View menu lists, and closes with the project", async () => {
    mount()
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    await act(async () => {
      await runAction("pianoRoll.chordDisplay")
    })
    expect(display()).toBeInTheDocument()
    expect(readout()).toHaveAttribute("aria-expanded", "true")
    const { VIEW_MENU, PANEL_MENU } = await import("../menu")
    expect(VIEW_MENU).toContain("pianoRoll.chordDisplay")
    expect(PANEL_MENU).toContain("pianoRoll.chordDisplay")
    act(() => announceProjectReplaced())
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })
})

describe("rendering and subscriptions", () => {
  function mountAlone() {
    const commits = vi.fn()
    const mounted = render(
      <SessionContext.Provider value={roll.session}>
        <Profiler id="chord" onRender={commits}>
          <ChordReadout />
        </Profiler>
      </SessionContext.Provider>
    )
    return { commits, mounted }
  }

  it.each([false, true])(
    "does not render again while the playhead moves inside the same chord (display open: %s)",
    async (open) => {
      await add([60, 64, 67])
      // The same chord again, an octave up, then another.
      await add([79, 72, 76], 240)
      await add([60, 63, 67], 480)
      const { commits } = mountAlone()
      if (open) fireEvent.click(readout())
      act(() => roll.session.setPlayhead(0))
      expect(readout()).toHaveTextContent("C major")
      commits.mockClear()
      for (const tick of [0.5, 1, 17.25, 120, 239.9, 240, 300, 479]) {
        act(() => roll.session.setPlayhead(tick))
      }
      // Pointer movement over the grid is not a change of chord either.
      act(() => roll.editor.pointerMove(at(100, 62)))
      act(() => roll.editor.pointerMove(at(130, 70)))
      expect(commits).not.toHaveBeenCalled()
      expect(readout()).toHaveTextContent("C major")
      act(() => roll.session.setPlayhead(480))
      expect(commits).toHaveBeenCalledTimes(1)
      expect(readout()).toHaveTextContent("C minor")
      if (open) expect(facts()).toMatchObject({ symbol: "Cm", source: "Playhead" })
    }
  )

  it("does not render again for a held key that doubles one already down", async () => {
    const { commits } = mountAlone()
    hold(48, 64, 67)
    expect(readout()).toHaveTextContent("C major")
    commits.mockClear()
    hold(72, 76)
    release(64)
    expect(commits).not.toHaveBeenCalled()
    release(48)
    expect(readout()).toHaveTextContent("C major / G")
    expect(commits).toHaveBeenCalledTimes(1)
  })

  it("removes every subscription on unmount", () => {
    const active = { editor: 0, playhead: 0 }
    const editorSubscribe = roll.editor.subscribe.bind(roll.editor)
    vi.spyOn(roll.editor, "subscribe").mockImplementation((listener) => {
      active.editor += 1
      const stop = editorSubscribe(listener)
      return () => {
        active.editor -= 1
        stop()
      }
    })
    const onPlayhead = roll.session.onPlayhead.bind(roll.session)
    vi.spyOn(roll.session, "onPlayhead").mockImplementation((listener) => {
      active.playhead += 1
      const stop = onPlayhead(listener)
      return () => {
        active.playhead -= 1
        stop()
      }
    })
    const { commits, mounted } = mountAlone()
    expect(active.editor).toBeGreaterThan(0)
    expect(active.playhead).toBe(1)
    mounted.unmount()
    expect(active).toEqual({ editor: 0, playhead: 0 })
    commits.mockClear()
    act(() => {
      roll.session.setPlayhead(5)
      auditionOn(lead(), 60, 0.8)
      auditionOff(lead(), 60)
    })
    expect(commits).not.toHaveBeenCalled()
  })
})
