import {
  act,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { runAction } from "@/lib/actions"
import { sourceSample } from "@/lib/channel-source"
import { SAMPLE_DRAG_TYPE } from "@/lib/dnd"
import type { Backend } from "@/lib/ipc"
import { dispatch, redo, undo } from "@/lib/store/project"
import { setTransportPattern, useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { settle } from "@/test/harness"

import ChannelRackPanel from "./index"
import { useRackStore } from "./rack-store"
import { CHANNEL_DRAG_TYPE } from "./rack-grid"
import {
  answerConfirm,
  answerText,
  channel,
  channelNames,
  dragData,
  history,
  labels,
  layoutGrid,
  notesOf,
  project,
  shownRow,
  startRack,
  stepButtons,
  stepGrid,
  storedRow,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let backend: Backend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startRack())
  vi.mocked(toast.error).mockClear()
})
afterEach(() => stop())

const press = (target: HTMLElement, init: object = {}) =>
  fireEvent.pointerDown(target, { pointerId: 1, button: 0, ...init })
const lift = (target: HTMLElement) =>
  fireEvent.pointerUp(target, { pointerId: 1 })

const nameButton = (name: string) => screen.getByRole("button", { name })
const lamp = (name: string) =>
  screen.getByRole("button", { name: `${name} on` })
const scroller = () =>
  document.querySelector<HTMLElement>('[data-slot="rack-scroll"]')!

/** jsdom has no DragEvent, so the pointer position is added by hand. */
function fireDrag(
  type: "dragStart" | "dragOver" | "drop" | "dragLeave" | "dragEnd",
  target: Element,
  dataTransfer: ReturnType<typeof dragData>,
  clientY = 0
) {
  const event = createEvent[type](target, { dataTransfer })
  Object.defineProperty(event, "clientY", { value: clientY })
  fireEvent(target, event)
  return event
}

describe("rows", () => {
  it("shows one row per channel, in rack order, lit from the store", () => {
    render(<ChannelRackPanel />)
    const rows = within(
      screen.getByRole("group", { name: "Channels" })
    ).getAllByRole("group", { name: /steps$/ })
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
      "Kick steps",
      "Clap steps",
      "Hat steps",
      "Snare steps",
    ])
    expect(shownRow("Kick")).toBe("x...x...x...x...")
    expect(shownRow("Clap")).toBe("....x.......x...")
    expect(shownRow("Hat")).toBe("x.x.x.x.x.x.x.x.")
    expect(shownRow("Snare")).toBe(".......x.......x")
  })

  it("shows which mixer track each channel plays into", () => {
    render(<ChannelRackPanel />)
    const badge = screen.getByRole("button", {
      name: "Clap plays into Clap. Change routing",
    })
    expect(badge).toHaveTextContent("2")
  })

  it("marks a channel that has no sample", async () => {
    await dispatch({ type: "addChannel", name: "Empty" })
    render(<ChannelRackPanel />)
    expect(
      screen.getByRole("button", { name: "Empty, no sample" })
    ).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: "Load a sound into Empty" })
    ).toHaveTextContent("no sample")
  })

  it("marks instruments that need a source and have none, and only those", async () => {
    await dispatch({ type: "addChannel", name: "Choir", instrument: "soundFont" })
    await dispatch({ type: "addChannel", name: "Keys", instrument: "multiSampler" })
    await dispatch({ type: "addChannel", name: "Bells", instrument: "fourOp" })
    await dispatch({ type: "addChannel", name: "Piano", instrument: "pianoKeys" })
    render(<ChannelRackPanel />)
    expect(screen.getByRole("button", { name: "Load a sound into Choir" })).toHaveTextContent("no SoundFont")
    expect(screen.getByRole("button", { name: "Load a sound into Keys" })).toHaveTextContent("no sample")
    // A synth and the built-in piano make their own sound.
    expect(screen.queryByRole("button", { name: "Load a sound into Bells" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Load a sound into Piano" })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Bells" })).not.toHaveTextContent("no sample")
  })

  it("opens the loader from the badge: settings for a SoundFont, the file picker for a sampler", async () => {
    const choir = await dispatch({ type: "addChannel", name: "Choir", instrument: "soundFont" })
    const empty = await dispatch({ type: "addChannel", name: "Empty" })
    const picked = vi.spyOn(backend, "pickAudioFile").mockResolvedValue(null)
    render(<ChannelRackPanel />)
    fireEvent.click(screen.getByRole("button", { name: "Load a sound into Choir" }))
    expect(useUiStore.getState().selectedChannel).toBe(choir!.created[0])
    expect(useRackStore.getState().inspectorOpen).toBe(true)
    expect(picked).not.toHaveBeenCalled()
    useRackStore.getState().setInspectorOpen(false)
    fireEvent.click(screen.getByRole("button", { name: "Load a sound into Empty" }))
    await act(settle)
    expect(picked).toHaveBeenCalledTimes(1)
    expect(useUiStore.getState().selectedChannel).toBe(empty!.created[0])
    expect(useRackStore.getState().inspectorOpen).toBe(false)
  })

  it("marks steps holding notes the grid cannot show", async () => {
    const kick = channel("Kick")
    await dispatch({
      type: "addNotes",
      pattern: project().patterns[0].id,
      channel: kick.id,
      notes: [
        { start: 500, length: 240, key: 60 },
        { start: 960, length: 240, key: 72 },
      ],
    })
    render(<ChannelRackPanel />)
    expect(
      screen.getByRole("button", {
        name: "Kick note preview. Open in piano roll",
      })
    ).toBeInTheDocument()
    expect(
      screen.queryByRole("group", { name: "Kick steps" })
    ).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole("button", { name: "Kick row view" }))
    await userEvent.click(
      await screen.findByRole("menuitemcheckbox", { name: /^Show steps/ })
    )
    const row = stepGrid("Kick").parentElement!
    const marked = [...row.querySelectorAll("[data-detail-step]")].map((mark) =>
      mark.getAttribute("data-detail-step")
    )
    // Step 2 holds the note between steps, step 4 the extra key.
    expect(marked).toEqual(["2", "4"])
    expect(stepButtons("Kick")[4]).toHaveAccessibleName(
      "Step 5, with notes the step grid cannot show"
    )
    // The off-grid note does not light a step.
    expect(shownRow("Kick")).toBe("x...x...x...x...")
  })
})

describe("steps", () => {
  it("toggles a step with one toggleStep command and one undo step", async () => {
    render(<ChannelRackPanel />)
    const sent = vi.spyOn(backend, "dispatch")
    const step = stepButtons("Kick")[1]
    press(step)
    lift(step)
    await settle()

    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent).toHaveBeenCalledWith(
      {
        type: "toggleStep",
        pattern: project().patterns[0].id,
        channel: channel("Kick").id,
        step: 1,
      },
      expect.any(Number)
    )
    expect(storedRow("Kick")).toBe("xx..x...x...x...")
    expect(shownRow("Kick")).toBe("xx..x...x...x...")
    expect(history().cursor).toBe(1)

    press(stepButtons("Kick")[0])
    lift(stepButtons("Kick")[0])
    await settle()
    expect(shownRow("Kick")).toBe(".x..x...x...x...")
    expect(labels()).toEqual(["Toggle step", "Toggle step"])
  })

  it("paints a drag as one undo step", async () => {
    render(<ChannelRackPanel />)
    const sent = vi.spyOn(backend, "dispatch")
    const grid = stepGrid("Clap")
    layoutGrid(grid, 320)
    press(grid, { clientX: 5 })
    fireEvent.pointerMove(grid, { pointerId: 1, clientX: 70 })
    fireEvent.pointerMove(grid, { pointerId: 1, clientX: 150 })
    lift(grid)
    await settle()

    // Step 4 was lit already, so seven steps changed.
    expect(sent).toHaveBeenCalledTimes(7)
    const gestures = new Set(sent.mock.calls.map((call) => call[1]))
    expect(gestures.size).toBe(1)
    expect(storedRow("Clap")).toBe("xxxxxxxx....x...")
    expect(shownRow("Clap")).toBe("xxxxxxxx....x...")
    expect(history().cursor).toBe(1)

    await undo()
    expect(storedRow("Clap")).toBe("....x.......x...")
    expect(shownRow("Clap")).toBe("....x.......x...")
    await redo()
    expect(shownRow("Clap")).toBe("xxxxxxxx....x...")
  })

  it("starts a new undo step with each stroke", async () => {
    render(<ChannelRackPanel />)
    const grid = stepGrid("Snare")
    layoutGrid(grid, 320)
    press(grid, { clientX: 5 })
    fireEvent.pointerMove(grid, { pointerId: 1, clientX: 45 })
    lift(grid)
    await settle()
    press(grid, { clientX: 205 })
    fireEvent.pointerMove(grid, { pointerId: 1, clientX: 245 })
    lift(grid)
    await settle()
    expect(storedRow("Snare")).toBe("xxx....x..xxx..x")
    expect(history().cursor).toBe(2)
  })

  it("erases along a right-drag as one undo step", async () => {
    render(<ChannelRackPanel />)
    const grid = stepGrid("Hat")
    layoutGrid(grid, 320)
    fireEvent.pointerDown(grid, { pointerId: 1, button: 2, clientX: 5 })
    fireEvent.pointerMove(grid, { pointerId: 1, clientX: 315 })
    lift(grid)
    await settle()
    expect(storedRow("Hat")).toBe("................")
    expect(shownRow("Hat")).toBe("................")
    expect(history().cursor).toBe(1)
  })

  it("follows the store through undo, redo and a pattern switch", async () => {
    render(<ChannelRackPanel />)
    const first = project().patterns[0].id
    const step = stepButtons("Snare")[0]
    press(step)
    lift(step)
    await settle()
    expect(shownRow("Snare")).toBe("x......x.......x")

    await undo()
    expect(shownRow("Snare")).toBe(".......x.......x")
    await redo()
    expect(shownRow("Snare")).toBe("x......x.......x")

    const added = await dispatch({ type: "addPattern" })
    await setTransportPattern(added!.created[0])
    await settle()
    expect(shownRow("Snare")).toBe("................")
    expect(shownRow("Kick")).toBe("................")

    // A step set here lands in the new pattern only.
    press(stepButtons("Kick")[3])
    lift(stepButtons("Kick")[3])
    await settle()
    expect(shownRow("Kick")).toBe("...x............")

    await setTransportPattern(first)
    await settle()
    expect(shownRow("Kick")).toBe("x...x...x...x...")
    expect(shownRow("Snare")).toBe("x......x.......x")
  })

  it("shows as many steps as the pattern is long", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    expect(stepButtons("Kick")).toHaveLength(16)

    const length = screen.getByRole("group", { name: "Pattern length" })
    await user.click(within(length).getByRole("button", { name: "32" }))
    await settle()
    expect(project().patterns[0].lengthSteps).toBe(32)
    expect(stepButtons("Kick")).toHaveLength(32)
    expect(shownRow("Kick")).toBe("x...x...x...x...................")
    expect(labels()).toEqual(["Change pattern length"])
    expect(within(length).getByRole("button", { name: "32" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )

    // The length field steps one at a time.
    const field = screen.getByRole("slider", {
      name: "Pattern length in steps",
    })
    fireEvent.keyDown(field, { key: "ArrowUp" })
    fireEvent.keyUp(field, { key: "ArrowUp" })
    await settle()
    expect(project().patterns[0].lengthSteps).toBe(33)
    expect(stepButtons("Kick")).toHaveLength(33)
  })

  it("moves the playhead across the rows while the pattern plays", async () => {
    render(<ChannelRackPanel />)
    expect(document.querySelector("[data-playing]")).toBeNull()

    await backend.transportPlay()
    await waitFor(() =>
      expect(stepGrid("Kick").querySelector("[data-playing]")).not.toBeNull()
    )
    // Every row shows the same step.
    const playing = ["Kick", "Clap", "Hat", "Snare"].map((name) =>
      stepButtons(name).findIndex((step) => step.hasAttribute("data-playing"))
    )
    expect(new Set(playing).size).toBe(1)

    await backend.transportStop()
    await waitFor(() =>
      expect(document.querySelector("[data-playing]")).toBeNull()
    )
  })

  it("shows no playhead while the song plays instead of the pattern", async () => {
    render(<ChannelRackPanel />)
    // The song needs a clip, or there is nothing to play.
    const track = await backend.dispatch({ type: "addPlaylistTrack" })
    await backend.dispatch({
      type: "addClips",
      clips: [
        {
          track: track.created[0],
          start: 0,
          content: { type: "pattern", pattern: project().patterns[0].id },
        },
      ],
    })
    await backend.transportSet({ mode: "song" })
    await backend.transportPlay()
    await new Promise((resolve) => setTimeout(resolve, 80))
    expect(useTransportStore.getState().playing).toBe(true)
    expect(document.querySelector("[data-playing]")).toBeNull()
    await backend.transportStop()
  })
})

describe("keyboard", () => {
  it("keeps Space for play and stop when a step has the focus", async () => {
    render(<ChannelRackPanel />)
    const step = stepButtons("Kick")[2]
    step.focus()
    const down = fireEvent.keyDown(step, { key: " ", code: "Space" })
    const up = fireEvent.keyUp(step, { key: " ", code: "Space" })
    await settle()

    // Both halves are cancelled, so the button does not click itself.
    expect(down).toBe(false)
    expect(up).toBe(false)
    expect(useTransportStore.getState().playing).toBe(true)
    expect(storedRow("Kick")).toBe("x...x...x...x...")

    fireEvent.keyDown(step, { key: " ", code: "Space" })
    await settle()
    expect(useTransportStore.getState().playing).toBe(false)
  })

  it("toggles the focused step with Enter", async () => {
    render(<ChannelRackPanel />)
    const step = stepButtons("Kick")[2]
    step.focus()
    // Enter on a button arrives as a click with no pointer detail.
    fireEvent.click(step, { detail: 0 })
    await settle()
    expect(storedRow("Kick")).toBe("x.x.x...x...x...")
    expect(history().cursor).toBe(1)
  })

  it("leaves Space alone while typing a value", async () => {
    render(<ChannelRackPanel />)
    const volume = screen.getByRole("slider", { name: "Kick channel volume" })
    fireEvent.keyDown(volume, { key: "Enter" })
    const input = screen.getByRole("textbox", { name: "Kick channel volume" })
    expect(fireEvent.keyDown(input, { key: " ", code: "Space" })).toBe(true)
    await settle()
    expect(useTransportStore.getState().playing).toBe(false)
  })

  it("moves between rows and from the name into the steps with the arrows", () => {
    render(<ChannelRackPanel />)
    nameButton("Kick").focus()
    fireEvent.keyDown(nameButton("Kick"), { key: "ArrowDown" })
    expect(nameButton("Clap")).toHaveFocus()
    expect(useUiStore.getState().selectedChannel).toBe(channel("Clap").id)

    fireEvent.keyDown(nameButton("Clap"), { key: "ArrowRight" })
    expect(stepButtons("Clap")[0]).toHaveFocus()
    fireEvent.keyDown(stepButtons("Clap")[0], { key: "ArrowRight" })
    expect(stepButtons("Clap")[1]).toHaveFocus()
    fireEvent.keyDown(stepButtons("Clap")[1], { key: "ArrowDown" })
    expect(stepButtons("Hat")[1]).toHaveFocus()
    fireEvent.keyDown(stepButtons("Hat")[1], { key: "ArrowLeft" })
    fireEvent.keyDown(stepButtons("Hat")[0], { key: "ArrowLeft" })
    expect(nameButton("Hat")).toHaveFocus()
    fireEvent.keyDown(nameButton("Hat"), { key: "ArrowUp" })
    expect(nameButton("Clap")).toHaveFocus()
  })
})

describe("mixing a channel", () => {
  it("mutes with a click on the lamp", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    expect(lamp("Kick")).toHaveAttribute("aria-pressed", "true")
    await user.click(lamp("Kick"))
    await settle()
    expect(channel("Kick").muted).toBe(true)
    expect(lamp("Kick")).toHaveAttribute("aria-pressed", "false")
    expect(labels()).toEqual(["Mute channel"])

    await user.click(lamp("Kick"))
    await settle()
    expect(channel("Kick").muted).toBe(false)
  })

  it("solos with Ctrl-click or a right-click, one channel at a time", async () => {
    render(<ChannelRackPanel />)
    fireEvent.click(lamp("Clap"), { ctrlKey: true })
    await settle()
    expect(channel("Clap").solo).toBe(true)
    expect(channel("Clap").muted).toBe(false)
    expect(lamp("Clap")).toHaveAttribute("data-solo")
    expect(lamp("Clap")).toHaveTextContent("S")

    const menu = fireEvent.contextMenu(lamp("Hat"))
    await settle()
    expect(menu).toBe(false)
    expect(channel("Hat").solo).toBe(true)
    expect(channel("Clap").solo).toBe(false)
    expect(lamp("Clap")).not.toHaveAttribute("data-solo")
    // Moving the solo is one step, not two.
    expect(labels()).toEqual(["Solo channel", "Solo channel"])

    fireEvent.contextMenu(lamp("Hat"))
    await settle()
    expect(project().channels.some((item) => item.solo)).toBe(false)
  })

  it("changes the volume with a drag that is one undo step", async () => {
    render(<ChannelRackPanel />)
    const sent = vi.spyOn(backend, "dispatch")
    const knob = screen.getByRole("slider", { name: "Kick channel volume" })
    const before = channel("Kick").volume
    press(knob, { clientY: 100 })
    fireEvent.pointerMove(knob, { pointerId: 1, clientY: 80 })
    fireEvent.pointerMove(knob, { pointerId: 1, clientY: 60 })
    lift(knob)
    await settle()

    expect(sent.mock.calls.length).toBeGreaterThanOrEqual(2)
    for (const [command] of sent.mock.calls) {
      expect(command).toMatchObject({
        type: "updateChannel",
        id: channel("Kick").id,
      })
    }
    const volume = channel("Kick").volume
    expect(volume).toBeGreaterThan(before)
    expect(volume).toBeLessThanOrEqual(2)
    expect(labels()).toEqual(["Change channel volume"])
    // The knob shows what was dragged until the project has answered, and
    // the project stores a volume in fewer digits than the drag works in.
    await waitFor(() =>
      expect(knob).toHaveAttribute("aria-valuenow", String(volume))
    )

    await undo()
    expect(channel("Kick").volume).toBe(before)
    expect(knob).toHaveAttribute("aria-valuenow", String(before))
  })

  it("pans with a drag and never leaves the range", async () => {
    render(<ChannelRackPanel />)
    const knob = screen.getByRole("slider", { name: "Hat channel pan" })
    press(knob, { clientY: 100 })
    fireEvent.pointerMove(knob, { pointerId: 1, clientY: 140 })
    fireEvent.pointerMove(knob, { pointerId: 1, clientY: 900 })
    lift(knob)
    await settle()
    expect(channel("Hat").pan).toBe(-1)
    expect(labels()).toEqual(["Change channel pan"])
    expect(toast.error).not.toHaveBeenCalled()
  })
})

describe("the channel button", () => {
  it("selects the channel and opens its settings on a click", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    expect(screen.queryByRole("complementary")).toBeNull()
    await user.click(nameButton("Hat"))
    expect(useUiStore.getState().selectedChannel).toBe(channel("Hat").id)
    expect(nameButton("Hat")).toHaveAttribute("aria-pressed", "true")
    expect(
      screen.getByRole("complementary", { name: "Channel settings" })
    ).toBeVisible()
  })

  it("plays the channel while pressed and always lets go", () => {
    render(<ChannelRackPanel />)
    const on = vi.spyOn(backend, "auditionNoteOn")
    const off = vi.spyOn(backend, "auditionNoteOff")
    const kick = channel("Kick").id
    const button = nameButton("Kick")

    press(button)
    expect(on).toHaveBeenCalledWith(kick, 60, 0.8)
    expect(off).not.toHaveBeenCalled()
    lift(button)
    expect(off).toHaveBeenCalledWith(kick, 60)
    expect(off).toHaveBeenCalledTimes(1)

    // Sliding off the button, a cancelled touch and losing focus all stop it.
    press(button)
    fireEvent.pointerLeave(button)
    expect(off).toHaveBeenCalledTimes(2)
    press(button)
    fireEvent.pointerCancel(button)
    expect(off).toHaveBeenCalledTimes(3)
    press(button)
    fireEvent.blur(button)
    expect(off).toHaveBeenCalledTimes(4)
    press(button)
    fireEvent.blur(window)
    expect(off).toHaveBeenCalledTimes(5)

    // Nothing is held now, so nothing more is sent.
    lift(button)
    fireEvent.pointerLeave(button)
    expect(off).toHaveBeenCalledTimes(5)
    expect(on).toHaveBeenCalledTimes(5)
  })

  it("stops the note when the row goes away mid-press", async () => {
    render(<ChannelRackPanel />)
    const off = vi.spyOn(backend, "auditionNoteOff")
    const kick = channel("Kick").id
    press(nameButton("Kick"))
    await dispatch({ type: "removeChannel", id: kick })
    await settle()
    expect(off).toHaveBeenCalledWith(kick, 60)
  })

  it("does not play on a right-click, and selects the row for its menu", () => {
    render(<ChannelRackPanel />)
    const on = vi.spyOn(backend, "auditionNoteOn")
    fireEvent.pointerDown(nameButton("Snare"), { pointerId: 1, button: 2 })
    fireEvent.contextMenu(nameButton("Snare"))
    expect(on).not.toHaveBeenCalled()
    expect(useUiStore.getState().selectedChannel).toBe(channel("Snare").id)
  })

  it("lists the row's actions on a right-click", async () => {
    render(<ChannelRackPanel />)
    fireEvent.contextMenu(nameButton("Kick"))
    const menu = await screen.findByRole("menu")
    const items = within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent)
    expect(items).toEqual([
      "Rename channel…F2",
      "Change channel color…",
      "Duplicate channelCtrl+D",
      "Channel group",
      "Replace sample from an audio file…",
      "Mute",
      "Solo",
      "Unmute all",
      "Unsolo all",
      "Reset levels",
      "Select next muted channel",
      "Select next solo channel",
      "Clear steps",
      "Send steps to piano rollAlt+3",
      "Fill every 2 steps",
      "Fill every 4 steps",
      "Fill every 8 steps",
      "Advanced step fill…",
      "Shift steps leftCtrl+Shift+←",
      "Shift steps rightCtrl+Shift+→",
      "Move channel upAlt+↑",
      "Move channel downAlt+↓",
      "Route to a new mixer track",
      "Show the channel's mixer track",
      "Delete channel…Del",
    ])
    // The first row cannot move up.
    expect(
      within(menu).getByRole("menuitem", { name: /Move channel up/ })
    ).toHaveAttribute("aria-disabled", "true")
  })
})

describe("row actions", () => {
  async function select(name: string) {
    useUiStore.getState().selectChannel(channel(name).id)
    await settle()
  }

  it("fills, shifts and clears, each as one undo step", async () => {
    render(<ChannelRackPanel />)
    await select("Hat")

    await runAction("channel.fill4")
    expect(storedRow("Hat")).toBe("x...x...x...x...")
    expect(shownRow("Hat")).toBe("x...x...x...x...")
    expect(notesOf("Hat")).toHaveLength(4)
    for (const note of notesOf("Hat")) {
      expect(note).toMatchObject({ key: 60, length: 240 })
    }
    expect(history().cursor).toBe(1)

    await runAction("channel.shiftRight")
    expect(shownRow("Hat")).toBe(".x...x...x...x..")
    expect(history().cursor).toBe(2)

    await runAction("channel.shiftLeft")
    await runAction("channel.shiftLeft")
    expect(shownRow("Hat")).toBe("...x...x...x...x")
    expect(history().cursor).toBe(4)

    await runAction("channel.fill2")
    expect(shownRow("Hat")).toBe("x.x.x.x.x.x.x.x.")
    await runAction("channel.fill8")
    expect(shownRow("Hat")).toBe("x.......x.......")

    await runAction("channel.clearSteps")
    expect(shownRow("Hat")).toBe("................")
    expect(notesOf("Hat")).toHaveLength(0)

    expect(labels()).toEqual([
      "Fill every 4 steps",
      "Shift steps right",
      "Shift steps left",
      "Shift steps left",
      "Fill every 2 steps",
      "Fill every 8 steps",
      "Clear notes",
    ])

    // Other rows were never touched.
    expect(shownRow("Kick")).toBe("x...x...x...x...")

    await undo()
    await undo()
    expect(shownRow("Hat")).toBe("x.x.x.x.x.x.x.x.")
  })

  it("shifts within the pattern length", async () => {
    render(<ChannelRackPanel />)
    await dispatch({
      type: "updatePattern",
      id: project().patterns[0].id,
      patch: { lengthSteps: 8 },
    })
    await select("Snare")
    // Step 7 wraps to the start; the note on step 15 is outside and stays.
    await runAction("channel.shiftRight")
    expect(shownRow("Snare")).toBe("x.......")
    expect(notesOf("Snare").map((note) => note.start / 240)).toEqual([0, 15])
  })

  it("does nothing without a selected channel", async () => {
    render(<ChannelRackPanel />)
    await runAction("channel.fill4")
    await runAction("channel.delete")
    await settle()
    expect(history().cursor).toBe(0)
  })

  it("renames after asking for a name", async () => {
    render(<ChannelRackPanel />)
    await select("Kick")
    void runAction("channel.rename")
    await answerText("Boom")
    expect(channelNames()).toEqual(["Boom", "Clap", "Hat", "Snare"])
    expect(nameButton("Boom")).toBeVisible()
    expect(labels()).toEqual(["Rename channel"])

    void runAction("channel.rename")
    await answerText(null)
    expect(history().cursor).toBe(1)
  })

  it("duplicates right below and selects the copy", async () => {
    render(<ChannelRackPanel />)
    await select("Clap")
    await runAction("channel.duplicate")
    const names = channelNames()
    expect(names).toHaveLength(5)
    expect(names[1]).toBe("Clap")
    expect(names[2]).toMatch(/^Clap/)
    const copy = project().channels[2]
    expect(useUiStore.getState().selectedChannel).toBe(copy.id)
    expect(shownRow(copy.name)).toBe("....x.......x...")
    expect(history().cursor).toBe(1)
  })

  it("asks before deleting, then selects the next row", async () => {
    render(<ChannelRackPanel />)
    await select("Clap")
    void runAction("channel.delete")
    await answerConfirm(null)
    expect(channelNames()).toHaveLength(4)

    void runAction("channel.delete")
    await answerConfirm("delete")
    expect(channelNames()).toEqual(["Kick", "Hat", "Snare"])
    expect(screen.queryByRole("group", { name: "Clap steps" })).toBeNull()
    expect(useUiStore.getState().selectedChannel).toBe(channel("Hat").id)
    expect(labels()).toEqual(["Delete channel"])

    await undo()
    expect(channelNames()).toEqual(["Kick", "Clap", "Hat", "Snare"])
    expect(shownRow("Clap")).toBe("....x.......x...")
  })

  it("moves the selected channel up and down, stopping at the ends", async () => {
    render(<ChannelRackPanel />)
    await select("Kick")
    await runAction("channel.moveUp")
    expect(history().cursor).toBe(0)
    await runAction("channel.moveDown")
    expect(channelNames()).toEqual(["Clap", "Kick", "Hat", "Snare"])
    await runAction("channel.moveDown")
    await runAction("channel.moveDown")
    await runAction("channel.moveDown")
    expect(channelNames()).toEqual(["Clap", "Hat", "Snare", "Kick"])
    expect(history().cursor).toBe(3)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("changes the color from the swatches", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    await select("Kick")
    await runAction("channel.color")
    await user.click(await screen.findByRole("button", { name: "Teal" }))
    await settle()
    expect(channel("Kick").color).toBe(0x12a594)
    expect(labels()).toEqual(["Change channel color"])
    expect(screen.queryByRole("button", { name: "Teal" })).toBeNull()
  })

  it("routes to a new mixer track as one undo step", async () => {
    render(<ChannelRackPanel />)
    await select("Kick")
    const tracks = project().mixer.tracks.length
    await runAction("channel.routeToNewTrack")
    await settle()
    expect(project().mixer.tracks).toHaveLength(tracks + 1)
    const added = project().mixer.tracks[tracks]
    expect(channel("Kick").mixerTrack).toBe(added.id)
    expect(useUiStore.getState().selectedTrack).toBe(added.id)
    expect(history().cursor).toBe(1)

    await undo()
    expect(project().mixer.tracks).toHaveLength(tracks)
    expect(channel("Kick").mixerTrack).not.toBe(added.id)
  })

  it("routes to another track from the badge's menu", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    await user.click(
      screen.getByRole("button", { name: "Hat plays into Hat. Change routing" })
    )
    // Opening the menu shows the track in the mixer.
    await waitFor(() => expect(useUiStore.getState().selectedTrack).toBe(channel("Hat").mixerTrack))
    await user.click(
      await screen.findByRole("menuitemradio", { name: /Master/ })
    )
    await settle()
    expect(channel("Hat").mixerTrack).toBe(0)
    expect(labels()).toEqual(["Route channel"])
    expect(
      screen.getByRole("button", {
        name: "Hat plays into Master. Change routing",
      })
    ).toHaveTextContent("M")
  })
})

describe("dragging", () => {
  const rim = "/factory/Drums/Percussion/Rim 01.wav"
  const sample = (path: string) =>
    dragData(SAMPLE_DRAG_TYPE, JSON.stringify({ path, name: "Rim 01" }))
  const dropLine = () => document.querySelector('[data-slot="rack-drop-line"]')

  it("adds a channel where a sample is dropped between rows", async () => {
    render(<ChannelRackPanel />)
    const over = fireDrag("dragOver", scroller(), sample(rim), 28 + 3)
    expect(over.defaultPrevented).toBe(true)
    expect(dropLine()).toHaveAttribute("data-index", "1")

    fireDrag("drop", scroller(), sample(rim), 28 + 3)
    await settle()
    expect(dropLine()).toBeNull()
    expect(channelNames()).toEqual(["Kick", "Rim 01", "Clap", "Hat", "Snare"])
    expect(sourceSample(channel("Rim 01").source)).not.toBeNull()
    expect(useUiStore.getState().selectedChannel).toBe(channel("Rim 01").id)
    expect(labels()).toEqual(["Add channel"])
  })

  it("adds at the end when dropped on the empty space below", async () => {
    render(<ChannelRackPanel />)
    fireDrag("dragOver", scroller(), sample(rim), 600)
    expect(dropLine()).toHaveAttribute("data-index", "4")
    fireDrag("drop", scroller(), sample(rim), 600)
    await settle()
    expect(channelNames()[4]).toBe("Rim 01")
  })

  it("replaces the sample when dropped on a channel's button", async () => {
    render(<ChannelRackPanel />)
    const before = sourceSample(channel("Clap").source)
    fireDrag("dragOver", nameButton("Clap"), sample(rim), 40)
    expect(dropLine()).toBeNull()
    expect(nameButton("Clap")).toHaveTextContent("replace")

    fireDrag("drop", nameButton("Clap"), sample(rim), 40)
    await settle()
    expect(channelNames()).toEqual(["Kick", "Clap", "Hat", "Snare"])
    const after = sourceSample(channel("Clap").source)
    expect(after).not.toBe(before)
    expect(project().samples.find((item) => item.id === after)?.name).toBe(
      "Rim 01"
    )
    expect(labels()).toEqual(["Change channel sample"])
    expect(nameButton("Clap")).not.toHaveTextContent("replace")
  })

  it("shows a backend error and changes nothing", async () => {
    render(<ChannelRackPanel />)
    fireDrag("drop", scroller(), sample("/factory/Notes.txt"), 0)
    await settle()
    expect(toast.error).toHaveBeenCalledWith(
      "Could not add the sample",
      expect.objectContaining({ description: expect.stringContaining("Notes") })
    )
    expect(channelNames()).toHaveLength(4)
    expect(history().cursor).toBe(0)
  })

  it("clears the drop mark when the drag leaves", () => {
    render(<ChannelRackPanel />)
    fireDrag("dragOver", scroller(), sample(rim), 60)
    expect(dropLine()).not.toBeNull()
    fireDrag("dragLeave", scroller(), sample(rim))
    expect(dropLine()).toBeNull()
  })

  it("ignores drags that are neither a sample nor a row", () => {
    render(<ChannelRackPanel />)
    const other = dragData("text/plain", "hello")
    const over = fireDrag("dragOver", scroller(), other, 30)
    expect(over.defaultPrevented).toBe(false)
    expect(dropLine()).toBeNull()
  })

  it("reorders rows by dragging the grip", async () => {
    render(<ChannelRackPanel />)
    const kick = channel("Kick").id
    const grip = screen.getByRole("img", { name: "Move Kick" })
    const row = dragData(CHANNEL_DRAG_TYPE, String(kick))
    fireDrag("dragStart", grip, row)

    // The gaps around the row itself are not a move, so they show no mark.
    fireDrag("dragOver", scroller(), row, 28)
    expect(dropLine()).toBeNull()

    fireDrag("dragOver", scroller(), row, 3 * 28)
    expect(dropLine()).toHaveAttribute("data-index", "3")
    fireDrag("drop", scroller(), row, 3 * 28)
    await settle()
    expect(channelNames()).toEqual(["Clap", "Hat", "Kick", "Snare"])
    expect(labels()).toEqual(["Move channel"])

    const snare = dragData(CHANNEL_DRAG_TYPE, String(channel("Snare").id))
    fireDrag(
      "dragStart",
      screen.getByRole("img", { name: "Move Snare" }),
      snare
    )
    fireDrag("drop", scroller(), snare, 0)
    await settle()
    expect(channelNames()).toEqual(["Snare", "Clap", "Hat", "Kick"])
  })
})
