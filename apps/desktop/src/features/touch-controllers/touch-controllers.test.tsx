import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest"

import type { Channel } from "@/bindings"
import { keyAtPoint, layoutKeys } from "@/components/audio"
import { auditionOff, auditionOn } from "@/features/piano-roll/audition"
import { registry } from "@/lib/actions"
import { MENUS, menuActionIds } from "@/lib/actions/menus"
import { useProjectStore } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { settle, startTestApp } from "@/test/harness"

import { registerTouchControllerActions } from "./actions"
import {
  MIN_VELOCITY,
  OCTAVE_RANGES,
  clampLowKey,
  keyboardSpan,
  maxLowKey,
  padAtPoint,
  stepPadBase,
  velocityAt,
} from "./geometry"
import { createHeldNotes, noteId } from "./held-notes"
import { useTouchControllersStore } from "./store"
import { TouchControllersWindow } from "./window"

vi.mock("@/features/piano-roll/audition", () => ({
  auditionOn: vi.fn(),
  auditionOff: vi.fn(),
}))

/** The size the pad grid is given: every pad is 100 px square. */
const PADS = { left: 0, top: 0, width: 400, height: 400 }
/** The size the keyboard is given in the tests that need its geometry. */
const KEYS = { left: 0, top: 0, width: 750, height: 100 }

let app: Awaited<ReturnType<typeof startTestApp>>
let unregister: () => void
let rects: MockInstance<() => DOMRect>
/** jsdom lays nothing out. With this on, the keyboard has a size too. */
let keyboardHasSize = false

function rect(size: { width: number; height: number }): DOMRect {
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: size.width,
    bottom: size.height,
    width: size.width,
    height: size.height,
    toJSON: () => ({}),
  }
}

beforeEach(async () => {
  vi.clearAllMocks()
  keyboardHasSize = false
  app = await startTestApp()
  useTouchControllersStore.setState(
    useTouchControllersStore.getInitialState(),
    true
  )
  unregister = registerTouchControllerActions()
  rects = vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: HTMLElement) {
      if (this.dataset.slot === "touch-pads") return rect(PADS)
      if (this.dataset.slot === "piano-keyboard" && keyboardHasSize)
        return rect(KEYS)
      return rect({ width: 0, height: 0 })
    })
})

afterEach(() => {
  cleanup()
  rects.mockRestore()
  unregister()
  app.stop()
})

const project = () => useProjectStore.getState().project
const channels = () => project().channels
const store = () => useTouchControllersStore.getState()

function show(mode: "keyboard" | "pads") {
  const id = mode === "keyboard" ? "view.touchKeyboard" : "view.touchPads"
  act(() => {
    void registry.get(id)?.run()
  })
}

function openWindow(mode: "keyboard" | "pads") {
  const view = render(<TouchControllersWindow />)
  show(mode)
  return view
}

const dialog = () =>
  screen.getByRole("dialog", { name: "Touch keyboard and pads" })
const keyboard = () => screen.getByRole("group", { name: /^Touch keyboard, / })
const pianoKey = (name: string) =>
  within(keyboard()).getByRole("button", { name })
const PAD_GRID = /^(Note|Channel) pads/
const padGrid = () => screen.getByRole("group", { name: PAD_GRID })
const pad = (number: number) =>
  within(padGrid()).getByRole("button", {
    name: new RegExp(`^Pad ${number}, `),
  })
const button = (name: string) => screen.getByRole("button", { name })
const focused = () =>
  document.activeElement instanceof HTMLElement ? document.activeElement : null

/** A point on a pad (0 to 15). `down` runs 0 at its top to 1 at its bottom. */
function onPad(index: number, down = 0.5) {
  const column = index % 4
  const row = 3 - Math.floor(index / 4)
  return { clientX: column * 100 + 50, clientY: (row + down) * 100 }
}

function padDown(pointerId: number, index: number, down?: number) {
  fireEvent.pointerDown(padGrid(), {
    pointerId,
    button: 0,
    ...onPad(index, down),
  })
}

function padMove(pointerId: number, index: number) {
  fireEvent.pointerMove(padGrid(), { pointerId, buttons: 1, ...onPad(index) })
}

function padUp(pointerId: number) {
  fireEvent.pointerUp(padGrid(), { pointerId })
}

function keyDown(pointerId: number, name: string) {
  fireEvent.pointerDown(pianoKey(name), { pointerId, button: 0 })
}

function keyMove(pointerId: number, name: string) {
  fireEvent.pointerMove(pianoKey(name), { pointerId, buttons: 1 })
}

function keyUp(pointerId: number, name: string) {
  fireEvent.pointerUp(pianoKey(name), { pointerId })
}

function setChannels(update: (channels: Channel[]) => Channel[]) {
  act(() => {
    useProjectStore.setState((state) => ({
      project: { ...state.project, channels: update(state.project.channels) },
    }))
  })
}

/** A rack of `count` channels named "Channel 1" and up, in that order. */
function fillRack(count: number) {
  setChannels(([template]) =>
    Array.from({ length: count }, (_, index) => ({
      ...template,
      id: 1000 + index,
      name: `Channel ${index + 1}`,
      color: 0x102030 + index,
    }))
  )
}

const ons = () => vi.mocked(auditionOn).mock.calls
const offs = () => vi.mocked(auditionOff).mock.calls
/** Halfway down a key or a pad. */
const HALF = expect.closeTo(0.6, 10)

describe("touch controller geometry", () => {
  it("maps a point to a pad counted from the bottom left, and to nothing outside the grid", () => {
    expect(padAtPoint(1, 399, PADS)?.pad).toBe(0)
    expect(padAtPoint(399, 399, PADS)?.pad).toBe(3)
    expect(padAtPoint(150, 250, PADS)?.pad).toBe(5)
    expect(padAtPoint(1, 1, PADS)?.pad).toBe(12)
    expect(padAtPoint(399, 1, PADS)?.pad).toBe(15)
    // The grid may sit anywhere on the screen.
    const moved = { left: 200, top: 50, width: 80, height: 40 }
    expect(padAtPoint(200, 50, moved)?.pad).toBe(12)
    expect(padAtPoint(279, 89, moved)?.pad).toBe(3)
    for (const [x, y] of [
      [-1, 200],
      [400, 200],
      [200, -1],
      [200, 400],
      [Number.NaN, 200],
    ]) {
      expect(padAtPoint(x, y, PADS), `${x}, ${y}`).toBeNull()
    }
    expect(padAtPoint(0, 0, { ...PADS, width: 0 })).toBeNull()
    expect(padAtPoint(0, 0, { ...PADS, height: 0 })).toBeNull()
  })

  it("plays louder lower in a pad, between the minimum velocity and 1", () => {
    expect(MIN_VELOCITY).toBe(0.2)
    expect(padAtPoint(50, 100, PADS)?.velocity).toBeCloseTo(MIN_VELOCITY, 10)
    expect(padAtPoint(50, 150, PADS)?.velocity).toBeCloseTo(0.6, 10)
    const loudest = padAtPoint(50, 199.999, PADS)?.velocity ?? 0
    expect(loudest).toBeGreaterThan(0.99)
    expect(loudest).toBeLessThanOrEqual(1)
    for (let y = 0; y < 400; y += 7) {
      const velocity = padAtPoint(10, y, PADS)?.velocity ?? -1
      expect(velocity).toBeGreaterThanOrEqual(MIN_VELOCITY)
      expect(velocity).toBeLessThanOrEqual(1)
    }
    expect(velocityAt(-3)).toBe(MIN_VELOCITY)
    expect(velocityAt(9)).toBe(1)
    expect(velocityAt(Number.NaN)).toBe(MIN_VELOCITY)
  })

  it("plays louder lower on a key, between the minimum velocity and 1", () => {
    const { low, high } = keyboardSpan(48, 2)
    const shapes = layoutKeys(low, high, "classic")
    expect(keyAtPoint(shapes, 0.01, 0)).toEqual({
      key: 48,
      velocity: MIN_VELOCITY,
    })
    expect(keyAtPoint(shapes, 0.01, 1)).toEqual({ key: 48, velocity: 1 })
    expect(keyAtPoint(shapes, 0.999, 0.5)?.key).toBe(72)
    expect(keyAtPoint(shapes, 0.01, 1.01)).toBeNull()
  })

  it("keeps the keyboard inside MIDI 0 to 127 at every octave and range", () => {
    expect(keyboardSpan(48, 2)).toEqual({ low: 48, high: 72 })
    expect(keyboardSpan(48, 4)).toEqual({ low: 48, high: 96 })
    for (const octaves of OCTAVE_RANGES) {
      expect(clampLowKey(-12, octaves)).toBe(0)
      expect(clampLowKey(500, octaves)).toBe(maxLowKey(octaves))
      for (let low = -24; low <= 180; low += 12) {
        const span = keyboardSpan(low, octaves)
        expect(span.low % 12).toBe(0)
        expect(span.low).toBeGreaterThanOrEqual(0)
        expect(span.high).toBeLessThanOrEqual(127)
        expect(span.high).toBeGreaterThan(span.low)
      }
      // The top position reaches the last key there is.
      expect(keyboardSpan(maxLowKey(octaves), octaves).high).toBe(127)
    }
    expect(clampLowKey(50, 2)).toBe(48)
    expect(clampLowKey(Number.NaN, 2)).toBe(0)
  })

  it("moves the pad bank by 16 and stops at keys 0 and 127", () => {
    expect(stepPadBase(36, 1)).toBe(52)
    expect(stepPadBase(36, -1)).toBe(20)
    const up = [36]
    while (stepPadBase(up[up.length - 1], 1) !== up[up.length - 1])
      up.push(stepPadBase(up[up.length - 1], 1))
    expect(up).toEqual([36, 52, 68, 84, 100, 112])
    const down = [112]
    while (stepPadBase(down[down.length - 1], -1) !== down[down.length - 1])
      down.push(stepPadBase(down[down.length - 1], -1))
    expect(down).toEqual([112, 100, 84, 68, 52, 36, 20, 4, 0])
    // Back from the lowest bank onto the banks around the default.
    expect(stepPadBase(0, 1)).toBe(4)
    expect(stepPadBase(4, 1)).toBe(20)
    for (const base of [...up, ...down]) {
      expect(base).toBeGreaterThanOrEqual(0)
      expect(base + 15).toBeLessThanOrEqual(127)
    }
  })
})

describe("held notes", () => {
  it("sounds a note once for several sources and stops it when the last lets go", () => {
    const notes = createHeldNotes()
    const seen: number[] = []
    const stop = notes.subscribe(() => seen.push(notes.snapshot().size))
    notes.press("a", 7, 60, 0.5)
    notes.press("b", 7, 60, 0.9)
    expect(ons()).toEqual([[7, 60, 0.5]])
    expect(notes.snapshot().has(noteId(7, 60))).toBe(true)
    notes.release("a")
    expect(offs()).toEqual([])
    notes.release("a")
    notes.release("b")
    expect(offs()).toEqual([[7, 60]])
    expect(notes.snapshot().size).toBe(0)
    // The same key of another channel is another note.
    notes.press("a", 7, 60, 0.5)
    notes.press("b", 8, 60, 0.5)
    expect(ons()).toHaveLength(3)
    // A source that moves on lets go of what it held.
    notes.press("a", 7, 62, 0.5)
    expect(offs()).toEqual([
      [7, 60],
      [7, 60],
    ])
    notes.releaseAll()
    notes.releaseAll()
    expect(offs()).toHaveLength(4)
    expect(seen).toEqual([1, 0, 1, 2, 1, 2, 1, 0])
    stop()
  })
})

describe("the touch controllers window", () => {
  it("opens on the keyboard from view.touchKeyboard and on the pads from view.touchPads", () => {
    render(<TouchControllersWindow />)
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(registry.get("view.touchKeyboard")).toMatchObject({
      title: "Touch keyboard",
      section: "View",
    })
    expect(registry.get("view.touchPads")).toMatchObject({
      title: "Touch pads",
      section: "View",
    })

    show("keyboard")
    expect(dialog()).toBeVisible()
    expect(screen.getByRole("tab", { name: "Keyboard" })).toHaveAttribute(
      "aria-selected",
      "true"
    )
    expect(keyboard()).toHaveAccessibleName(
      `Touch keyboard, ${channels()[0].name}`
    )
    expect(screen.queryByRole("group", { name: PAD_GRID })).toBeNull()

    show("pads")
    expect(screen.getByRole("tab", { name: "Pads" })).toHaveAttribute(
      "aria-selected",
      "true"
    )
    expect(within(padGrid()).getAllByRole("button")).toHaveLength(16)
    expect(screen.queryByRole("group", { name: /^Touch keyboard/ })).toBeNull()

    fireEvent.click(screen.getByRole("tab", { name: "Keyboard" }))
    expect(keyboard()).toBeVisible()
    expect(store().mode).toBe("keyboard")

    fireEvent.click(button("Close"))
    expect(store().open).toBe(false)
    expect(screen.queryByRole("group", { name: /^Touch keyboard/ })).toBeNull()

    show("pads")
    act(() => unregister())
    expect(store().open).toBe(false)
    expect(registry.get("view.touchPads")).toBeUndefined()
  })

  it("is in the View menu and floats without a backdrop or a focus trap", () => {
    const view = MENUS.find((menu) => menu.title === "View")
    expect(menuActionIds(view?.entries ?? [])).toEqual(
      expect.arrayContaining(["view.touchKeyboard", "view.touchPads"])
    )
    render(
      <>
        <button type="button">Outside</button>
        <TouchControllersWindow />
      </>
    )
    show("keyboard")
    expect(document.querySelector('[data-slot="dialog-overlay"]')).toBeNull()
    expect(dialog()).not.toHaveAttribute("aria-modal", "true")
    const outside = button("Outside")
    fireEvent.pointerDown(outside, { pointerId: 1, button: 0 })
    fireEvent.mouseDown(outside)
    fireEvent.click(outside)
    act(() => outside.focus())
    expect(outside).toHaveFocus()
    expect(store().open).toBe(true)
    expect(dialog()).toBeVisible()
  })

  it("closes on Escape from inside the window only", async () => {
    const user = userEvent.setup()
    render(
      <>
        <button type="button">Outside</button>
        <TouchControllersWindow />
      </>
    )
    show("keyboard")
    // Opening moves the focus into the window, a moment later.
    await waitFor(() => expect(dialog()).toContainElement(focused()))
    act(() => button("Outside").focus())
    await user.keyboard("{Escape}")
    expect(button("Outside")).toHaveFocus()
    expect(store().open).toBe(true)
    act(() => button("Octave up").focus())
    await user.keyboard("{Escape}")
    expect(store().open).toBe(false)
  })

  it("holds two keys with two pointers and releases each on its own", () => {
    openWindow("keyboard")
    const id = channels()[0].id
    keyDown(1, "C4")
    keyDown(2, "E4")
    expect(ons()).toEqual([
      [id, 48, expect.any(Number)],
      [id, 52, expect.any(Number)],
    ])
    expect(offs()).toEqual([])
    expect(pianoKey("C4")).toHaveAttribute("aria-pressed", "true")
    expect(pianoKey("E4")).toHaveAttribute("aria-pressed", "true")
    expect(pianoKey("D4")).toHaveAttribute("aria-pressed", "false")

    keyUp(1, "C4")
    expect(offs()).toEqual([[id, 48]])
    expect(pianoKey("C4")).toHaveAttribute("aria-pressed", "false")
    expect(pianoKey("E4")).toHaveAttribute("aria-pressed", "true")

    keyUp(2, "E4")
    expect(offs()).toEqual([
      [id, 48],
      [id, 52],
    ])
    expect(ons()).toHaveLength(2)
  })

  it("plays a glissando when a held pointer slides onto another key", () => {
    openWindow("keyboard")
    const id = channels()[0].id
    keyDown(1, "C4")
    keyDown(2, "G4")
    keyMove(1, "C4")
    keyMove(1, "D4")
    keyMove(1, "E4")
    expect(ons().map(([, key]) => key)).toEqual([48, 55, 50, 52])
    expect(offs()).toEqual([
      [id, 48],
      [id, 50],
    ])
    // The other finger never moved and still holds its key.
    expect(pianoKey("G4")).toHaveAttribute("aria-pressed", "true")
    keyUp(1, "E4")
    keyUp(2, "G4")
    expect(offs().map(([, key]) => key)).toEqual([48, 50, 52, 55])
  })

  it("sounds a key held by two pointers once and releases it with the last", () => {
    openWindow("keyboard")
    const id = channels()[0].id
    keyDown(1, "C4")
    keyDown(2, "C4")
    expect(ons()).toEqual([[id, 48, expect.any(Number)]])
    keyUp(1, "C4")
    expect(offs()).toEqual([])
    expect(pianoKey("C4")).toHaveAttribute("aria-pressed", "true")
    keyUp(2, "C4")
    expect(offs()).toEqual([[id, 48]])
    // A second pointer sliding onto a held key does not play it again.
    keyDown(3, "C4")
    keyDown(4, "D4")
    keyMove(4, "C4")
    expect(ons().map(([, key]) => key)).toEqual([48, 48, 50])
    keyUp(3, "C4")
    expect(offs().map(([, key]) => key)).toEqual([48, 50])
    keyUp(4, "C4")
    expect(offs().map(([, key]) => key)).toEqual([48, 50, 48])
  })

  it("takes the velocity of a key from how low it was touched", () => {
    keyboardHasSize = true
    openWindow("keyboard")
    const id = channels()[0].id
    // 15 white keys across 750 px: C4 is the first 50 px.
    const touch = (pointerId: number, clientY: number) => {
      fireEvent.pointerDown(keyboard(), {
        pointerId,
        button: 0,
        clientX: 10,
        clientY,
      })
      fireEvent.pointerUp(keyboard(), { pointerId })
    }
    touch(1, 0)
    touch(2, 50)
    touch(3, 100)
    expect(ons()).toEqual([
      [id, 48, MIN_VELOCITY],
      [id, 48, HALF],
      [id, 48, 1],
    ])
    // Outside the keys nothing plays.
    touch(4, 140)
    expect(ons()).toHaveLength(3)
    expect(offs()).toHaveLength(3)
  })

  it("moves by octaves within the MIDI range and shows 2, 3 or 4 octaves with C labels", () => {
    openWindow("keyboard")
    const range = () => screen.getByRole("status", { name: "Key range" })
    expect(range()).toHaveTextContent("C4 to C6")
    expect(within(keyboard()).getAllByRole("button")).toHaveLength(25)
    expect(pianoKey("C4")).toHaveTextContent("C4")
    expect(pianoKey("C5")).toHaveTextContent("C5")
    expect(pianoKey("D4")).toHaveTextContent("")

    fireEvent.click(button("Octave up"))
    expect(range()).toHaveTextContent("C5 to C7")
    for (let press = 0; press < 20; press += 1)
      fireEvent.click(button("Octave down"))
    expect(range()).toHaveTextContent("C0 to C2")
    expect(button("Octave down")).toBeDisabled()
    expect(store().lowKey).toBe(0)
    keyDown(1, "C0")
    keyUp(1, "C0")
    expect(ons()[0][1]).toBe(0)

    for (let press = 0; press < 20; press += 1)
      fireEvent.click(button("Octave up"))
    expect(button("Octave up")).toBeDisabled()
    expect(range()).toHaveTextContent("C9 to G10")
    keyDown(1, "G10")
    keyUp(1, "G10")
    expect(ons()[1][1]).toBe(127)
    expect(within(keyboard()).queryByRole("button", { name: "G#10" })).toBeNull()

    // A wider range starts lower rather than running past the last key.
    fireEvent.click(button("4 octaves"))
    expect(button("4 octaves")).toHaveAttribute("aria-pressed", "true")
    expect(range()).toHaveTextContent("C7 to G10")
    expect(store()).toMatchObject({ lowKey: 84, octaves: 4 })
    fireEvent.click(button("Octave down"))
    expect(range()).toHaveTextContent("C6 to C10")
    expect(within(keyboard()).getAllByRole("button")).toHaveLength(49)
    fireEvent.click(button("3 octaves"))
    expect(range()).toHaveTextContent("C6 to C9")
    expect(within(keyboard()).getAllByRole("button")).toHaveLength(37)
  })

  it("plays key base + n of the target channel from the note pads, several at once", () => {
    openWindow("pads")
    const id = channels()[0].id
    expect(padGrid()).toHaveAccessibleName(`Note pads, ${channels()[0].name}`)
    expect(pad(1)).toHaveAccessibleName("Pad 1, C3")
    expect(pad(16)).toHaveAccessibleName("Pad 16, D#4")

    padDown(1, 0, 0)
    padDown(2, 15, 0.5)
    expect(ons()).toEqual([
      [id, 36, MIN_VELOCITY],
      [id, 51, HALF],
    ])
    expect(pad(1)).toHaveAttribute("aria-pressed", "true")
    expect(pad(16)).toHaveAttribute("aria-pressed", "true")
    expect(pad(2)).toHaveAttribute("aria-pressed", "false")
    padUp(2)
    expect(offs()).toEqual([[id, 51]])
    expect(pad(1)).toHaveAttribute("aria-pressed", "true")
    padUp(1)
    expect(offs()).toEqual([
      [id, 51],
      [id, 36],
    ])
    for (let index = 0; index < 16; index += 1) {
      padDown(9, index)
      padUp(9)
    }
    expect(ons().slice(2)).toEqual(
      Array.from({ length: 16 }, (_, index) => [id, 36 + index, HALF])
    )
    // The right button and a pointer that never pressed play nothing.
    fireEvent.pointerDown(padGrid(), { pointerId: 5, button: 2, ...onPad(3) })
    padMove(6, 3)
    expect(ons()).toHaveLength(18)
  })

  it("slides between pads, and plays a pad held by two pointers once", () => {
    openWindow("pads")
    const id = channels()[0].id
    padDown(1, 0)
    padMove(1, 0)
    padMove(1, 1)
    expect(ons().map(([, key]) => key)).toEqual([36, 37])
    expect(offs()).toEqual([[id, 36]])
    // Off the grid the pad is let go, and coming back plays again.
    fireEvent.pointerMove(padGrid(), {
      pointerId: 1,
      buttons: 1,
      clientX: 900,
      clientY: 50,
    })
    expect(offs()).toEqual([
      [id, 36],
      [id, 37],
    ])
    padMove(1, 2)
    expect(ons().map(([, key]) => key)).toEqual([36, 37, 38])
    padUp(1)
    expect(offs().map(([, key]) => key)).toEqual([36, 37, 38])

    vi.clearAllMocks()
    padDown(1, 4)
    padDown(2, 4)
    padDown(3, 5)
    padMove(3, 4)
    expect(ons().map(([, key]) => key)).toEqual([40, 41])
    expect(offs().map(([, key]) => key)).toEqual([41])
    padUp(1)
    padUp(2)
    expect(offs()).toHaveLength(1)
    expect(pad(5)).toHaveAttribute("aria-pressed", "true")
    padUp(3)
    expect(offs().map(([, key]) => key)).toEqual([41, 40])
    expect(pad(5)).toHaveAttribute("aria-pressed", "false")
  })

  it("moves the note pad bank by 16 and keeps it within keys 0 to 127", () => {
    openWindow("pads")
    const id = channels()[0].id
    const bank = () => screen.getByRole("status", { name: "Pad bank" })
    expect(bank()).toHaveTextContent("C3 to D#4")
    fireEvent.click(button("Bank up"))
    expect(store().padBase).toBe(52)
    expect(pad(1)).toHaveAccessibleName("Pad 1, E4")
    padDown(1, 2)
    padUp(1)
    expect(ons()).toEqual([[id, 54, HALF]])

    for (let press = 0; press < 20; press += 1)
      fireEvent.click(button("Bank up"))
    expect(button("Bank up")).toBeDisabled()
    expect(store().padBase).toBe(112)
    expect(bank()).toHaveTextContent("E9 to G10")
    padDown(1, 15)
    padUp(1)
    expect(ons()[1]).toEqual([id, 127, HALF])

    for (let press = 0; press < 20; press += 1)
      fireEvent.click(button("Bank down"))
    expect(button("Bank down")).toBeDisabled()
    expect(store().padBase).toBe(0)
    expect(bank()).toHaveTextContent("C0 to D#1")
    padDown(1, 0)
    padUp(1)
    expect(ons()[2]).toEqual([id, 0, HALF])
  })

  it("puts the rack's channels on the pads in rack order and disables the rest", () => {
    openWindow("pads")
    fireEvent.click(button("Channels"))
    expect(store().padLayout).toBe("channels")
    const rack = channels()
    expect(rack.length).toBeGreaterThan(1)
    expect(rack.length).toBeLessThan(16)
    expect(padGrid()).toHaveAccessibleName("Channel pads")
    rack.forEach((channel, index) => {
      expect(pad(index + 1)).toHaveAccessibleName(
        `Pad ${index + 1}, ${channel.name}`
      )
      expect(pad(index + 1)).toHaveTextContent(channel.name)
      expect(pad(index + 1)).toBeEnabled()
      expect(pad(index + 1).style.getPropertyValue("--pad-color")).toBe(
        `#${channel.color.toString(16).padStart(6, "0")}`
      )
    })
    for (let number = rack.length + 1; number <= 16; number += 1) {
      expect(pad(number)).toHaveAccessibleName(`Pad ${number}, empty`)
      expect(pad(number)).toBeDisabled()
    }
    expect(button("Bank down")).toBeDisabled()
    expect(button("Bank up")).toBeDisabled()

    padDown(1, 1, 1 - 1e-9)
    padDown(2, 0, 0)
    expect(ons()).toEqual([
      [rack[1].id, 60, expect.closeTo(1, 5)],
      [rack[0].id, 60, MIN_VELOCITY],
    ])
    expect(pad(2)).toHaveAttribute("aria-pressed", "true")
    // An empty pad plays nothing, pressed or slid onto.
    padDown(3, 15)
    padMove(2, 15)
    expect(ons()).toHaveLength(2)
    expect(offs()).toEqual([[rack[0].id, 60]])
    padUp(1)
    padUp(2)
    padUp(3)
    expect(offs()).toEqual([
      [rack[0].id, 60],
      [rack[1].id, 60],
    ])

    // The rack's order is the pads' order.
    setChannels((list) => [...list].reverse())
    expect(pad(1)).toHaveAccessibleName(`Pad 1, ${rack[rack.length - 1].name}`)
  })

  it("pages the channel pads when the rack has more than 16 channels", () => {
    openWindow("pads")
    fireEvent.click(button("Channels"))
    fillRack(18)
    const bank = () => screen.getByRole("status", { name: "Pad bank" })
    expect(bank()).toHaveTextContent("Channels 1 to 16 of 18")
    expect(pad(16)).toHaveAccessibleName("Pad 16, Channel 16")
    expect(button("Bank down")).toBeDisabled()

    fireEvent.click(button("Bank up"))
    expect(bank()).toHaveTextContent("Channels 17 to 18 of 18")
    expect(button("Bank up")).toBeDisabled()
    expect(pad(1)).toHaveAccessibleName("Pad 1, Channel 17")
    expect(pad(2)).toHaveAccessibleName("Pad 2, Channel 18")
    expect(pad(3)).toBeDisabled()
    padDown(1, 1)
    padUp(1)
    expect(ons()).toEqual([[1017, 60, HALF]])
    expect(offs()).toEqual([[1017, 60]])

    // Fewer channels than the bank shows: back on the last bank there is.
    fillRack(3)
    expect(bank()).toHaveTextContent("Channels 1 to 3 of 3")
    expect(pad(3)).toHaveAccessibleName("Pad 3, Channel 3")
    expect(pad(4)).toBeDisabled()
    expect(button("Bank down")).toBeDisabled()
    expect(button("Bank up")).toBeDisabled()
  })

  it("plays a focused pad with Enter or Space, once per press", () => {
    openWindow("pads")
    const id = channels()[0].id
    act(() => pad(3).focus())
    fireEvent.keyDown(pad(3), { key: "Enter" })
    fireEvent.keyDown(pad(3), { key: "Enter", repeat: true })
    expect(ons()).toEqual([[id, 38, 0.8]])
    fireEvent.keyUp(pad(3), { key: "Enter" })
    expect(offs()).toEqual([[id, 38]])
    fireEvent.keyDown(pad(3), { key: " " })
    fireEvent.blur(pad(3))
    expect(offs()).toEqual([
      [id, 38],
      [id, 38],
    ])
  })

  describe.each(["keyboard", "pads"] as const)("holding notes on the %s", (mode) => {
    /** Two fingers down. Returns the notes they hold. */
    function hold(): [number, number][] {
      const id = channels()[0].id
      if (mode === "keyboard") {
        keyDown(1, "C4")
        keyDown(2, "E4")
        return [
          [id, 48],
          [id, 52],
        ]
      }
      padDown(1, 0)
      padDown(2, 5)
      return [
        [id, 36],
        [id, 41],
      ]
    }
    const surface = () => (mode === "keyboard" ? keyboard() : padGrid())

    function expectReleased(notes: [number, number][]) {
      expect(ons()).toHaveLength(notes.length)
      expect(offs()).toHaveLength(notes.length)
      expect(offs()).toEqual(expect.arrayContaining(notes))
    }

    it.each(["pointerCancel", "lostPointerCapture"] as const)(
      "releases a pointer's note on %s",
      (ending) => {
        openWindow(mode)
        const notes = hold()
        fireEvent[ending](surface(), { pointerId: 1 })
        expect(offs()).toEqual([notes[0]])
        fireEvent[ending](surface(), { pointerId: 2 })
        expectReleased(notes)
        // The pointer-up that may still follow releases nothing twice.
        fireEvent.pointerUp(surface(), { pointerId: 1 })
        expect(offs()).toHaveLength(2)
      }
    )

    it("releases every note when the window loses the focus", () => {
      openWindow(mode)
      const notes = hold()
      fireEvent.blur(window)
      expectReleased(notes)
      fireEvent.pointerUp(surface(), { pointerId: 1 })
      fireEvent.pointerUp(surface(), { pointerId: 2 })
      expect(offs()).toHaveLength(2)
    })

    it("releases every note when the window is closed", () => {
      openWindow(mode)
      const notes = hold()
      fireEvent.click(button("Close"))
      expect(store().open).toBe(false)
      expectReleased(notes)
    })

    it("releases every note when the other controller is shown", () => {
      openWindow(mode)
      const notes = hold()
      fireEvent.click(
        screen.getByRole("tab", {
          name: mode === "keyboard" ? "Pads" : "Keyboard",
        })
      )
      expectReleased(notes)
    })

    it("releases every note when another channel is selected", () => {
      openWindow(mode)
      const notes = hold()
      const next = channels()[1]
      act(() => useUiStore.getState().selectChannel(next.id))
      // On the channel the notes were started on, not the new one.
      expectReleased(notes)
      vi.clearAllMocks()
      if (mode === "keyboard") keyDown(3, "C4")
      else padDown(3, 0)
      expect(ons()).toEqual([[next.id, mode === "keyboard" ? 48 : 36, expect.any(Number)]])
    })

    it("releases every note when the window is given another channel", () => {
      openWindow(mode)
      const notes = hold()
      act(() => store().setChannelOverride(channels()[2].id))
      expectReleased(notes)
    })

    it("releases every note when another project replaces this one", async () => {
      openWindow(mode)
      const notes = hold()
      await act(async () => {
        await app.backend.projectNew()
        await settle()
      })
      expectReleased(notes)
    })

    it("releases every note when it unmounts", () => {
      const view = openWindow(mode)
      const notes = hold()
      view.unmount()
      expectReleased(notes)
    })

    it("plays without dispatching a command or adding history", async () => {
      const dispatched = vi.spyOn(app.backend, "dispatch")
      openWindow(mode)
      const before = useProjectStore.getState()
      hold()
      fireEvent.pointerUp(surface(), { pointerId: 1 })
      fireEvent.pointerUp(surface(), { pointerId: 2 })
      await act(settle)
      const after = useProjectStore.getState()
      expect(ons()).toHaveLength(2)
      expect(dispatched).not.toHaveBeenCalled()
      expect(after.project).toBe(before.project)
      expect(after.history).toBe(before.history)
      expect(after.history.cursor).toBe(before.history.cursor)
      expect(after.revision).toBe(before.revision)
    })
  })

  it("plays the channel picked in the window instead of the selected one", async () => {
    const user = userEvent.setup()
    openWindow("keyboard")
    const [first, second, third] = channels()
    const target = () => screen.getByRole("combobox", { name: "Target channel" })
    expect(target()).toHaveTextContent(`Selected channel: ${first.name}`)
    act(() => useUiStore.getState().selectChannel(second.id))
    expect(target()).toHaveTextContent(`Selected channel: ${second.name}`)
    expect(keyboard()).toHaveAccessibleName(`Touch keyboard, ${second.name}`)

    await user.click(target())
    await user.click(await screen.findByRole("option", { name: third.name }))
    expect(store().channelOverride).toBe(third.id)
    expect(target()).toHaveTextContent(third.name)
    expect(target()).not.toHaveTextContent("Selected channel")
    // The selection moves on; the window keeps its own channel.
    act(() => useUiStore.getState().selectChannel(first.id))
    keyDown(1, "C4")
    keyUp(1, "C4")
    expect(ons()).toEqual([[third.id, 48, expect.any(Number)]])
    expect(offs()).toEqual([[third.id, 48]])

    // A picked channel that is deleted gives way to the selection again.
    setChannels((list) => list.filter((channel) => channel.id !== third.id))
    expect(target()).toHaveTextContent(`Selected channel: ${first.name}`)
    expect(keyboard()).toHaveAccessibleName(`Touch keyboard, ${first.name}`)
  })

  it("forgets the picked channel and the channel bank with the project", async () => {
    openWindow("pads")
    act(() => {
      store().setChannelOverride(channels()[1].id)
      store().setChannelBank(2)
    })
    await act(async () => {
      await app.backend.projectNew()
      await settle()
    })
    expect(store()).toMatchObject({ channelOverride: null, channelBank: 0 })
  })

  it("shows an empty state when the project has no channels", () => {
    openWindow("keyboard")
    setChannels(() => [])
    expect(within(dialog()).getByText("No channel to play")).toBeVisible()
    expect(screen.queryByRole("group", { name: /^Touch keyboard/ })).toBeNull()
    expect(screen.queryByRole("tab")).toBeNull()
    expect(screen.queryByRole("combobox", { name: "Target channel" })).toBeNull()
    show("pads")
    expect(within(dialog()).getByText("No channel to play")).toBeVisible()
    expect(screen.queryByRole("group", { name: PAD_GRID })).toBeNull()
    expect(ons()).toEqual([])
  })

  it("keeps the view preferences across a restart and nothing else", () => {
    openWindow("pads")
    fireEvent.click(button("Channels"))
    fireEvent.click(button("Notes"))
    fireEvent.click(button("Bank up"))
    fireEvent.click(screen.getByRole("tab", { name: "Keyboard" }))
    fireEvent.click(button("3 octaves"))
    fireEvent.click(button("Octave up"))
    act(() => store().setChannelOverride(channels()[1].id))
    keyDown(1, "C5")

    const saved: unknown = JSON.parse(
      localStorage.getItem("windfall.touchControllers") ?? "null"
    )
    expect(saved).toEqual({
      state: {
        mode: "keyboard",
        lowKey: 60,
        octaves: 3,
        padLayout: "notes",
        padBase: 52,
      },
      version: 1,
    })

    // What storage hands back is checked before the controls show it.
    act(() => {
      useTouchControllersStore.setState(
        useTouchControllersStore.getInitialState(),
        true
      )
    })
    localStorage.setItem(
      "windfall.touchControllers",
      JSON.stringify({
        state: {
          open: true,
          mode: "pads",
          lowKey: 119,
          octaves: 4,
          padLayout: "channels",
          padBase: 500,
          channelOverride: 3,
        },
        version: 1,
      })
    )
    act(() => void useTouchControllersStore.persist.rehydrate())
    expect(store()).toMatchObject({
      open: false,
      mode: "pads",
      lowKey: 84,
      octaves: 4,
      padLayout: "channels",
      padBase: 112,
      channelOverride: null,
    })
    localStorage.setItem(
      "windfall.touchControllers",
      JSON.stringify({
        state: { mode: 7, lowKey: "x", octaves: 9, padLayout: null },
        version: 1,
      })
    )
    act(() => void useTouchControllersStore.persist.rehydrate())
    expect(store()).toMatchObject({
      mode: "keyboard",
      lowKey: 84,
      octaves: 4,
      padLayout: "notes",
      padBase: 112,
    })
  })
})
