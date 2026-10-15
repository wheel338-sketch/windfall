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
import type { ReactNode } from "react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { runAction } from "@/lib/actions"
import { SAMPLE_DRAG_TYPE } from "@/lib/dnd"
import type { Backend } from "@/lib/ipc"
import { dispatch, undo } from "@/lib/store/project"
import { usePromptStore } from "@/lib/store/prompts"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { samplesReloaded } from "@/lib/store/warnings"
import { settle } from "@/test/harness"

import ChannelRackPanel from "./index"
import { DEFAULT_ENVELOPE } from "./inspector/envelope-section"
import { parseNoteName } from "./inspector/sound-section"
import { useRackStore } from "./rack-store"
import {
  channel,
  dragData,
  history,
  labels,
  project,
  samplerOf,
  startRack,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

// The resize handle finds the pointer by measuring, and jsdom measures
// everything as zero wide at the origin, so it would take every click.
vi.mock("@/components/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizablePanel: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizableHandle: () => null,
}))

let backend: Backend
let stop: () => void

beforeEach(async () => {
  ;({ backend, stop } = await startRack())
  vi.mocked(toast.error).mockClear()
})
afterEach(() => stop())

const sampler = samplerOf
const settings = () =>
  screen.getByRole("complementary", { name: "Channel settings" })
const slider = (name: string) =>
  within(settings()).getByRole("slider", { name })

/** Renders the rack with a channel selected and its settings open. */
async function openSettings(name: string) {
  useUiStore.getState().selectChannel(channel(name).id)
  useRackStore.getState().setInspectorOpen(true)
  const view = render(<ChannelRackPanel />)
  // The waveform arrives from the backend a moment later.
  await settle()
  return view
}

/** One key press: down and up, which is one gesture for a kit control. */
function tap(target: HTMLElement, key: string, times = 1) {
  for (let count = 0; count < times; count += 1) {
    fireEvent.keyDown(target, { key })
  }
  fireEvent.keyUp(target, { key })
}

describe("the settings panel", () => {
  it("shows preset source rendering warnings beside the sampler waveform", async () => {
    const original = backend.sampleInfoById.bind(backend)
    vi.spyOn(backend, "sampleInfoById").mockImplementation(async (sample) => ({
      ...await original(sample),
      sourceWarnings: ["DrumSynth tone/flat-noise rendering is approximate.", "Retained but not interpreted: own extension.text"],
    }))
    await openSettings("Kick")
    expect(await screen.findByText("DrumSynth tone/flat-noise rendering is approximate.")).toBeVisible()
    expect(screen.getByText("Retained but not interpreted: own extension.text")).toBeVisible()
  })
  it("lists the sections in a bar and scrolls to the one picked", async () => {
    await openSettings("Kick")
    const bar = within(settings()).getByRole("navigation", {
      name: "Channel settings sections",
    })
    const loop = within(settings()).getByRole("region", { name: "Loop" })
    const scroll = vi.fn()
    loop.scrollIntoView = scroll
    fireEvent.click(within(bar).getByRole("button", { name: "Go to Loop" }))
    expect(scroll).toHaveBeenCalledTimes(1)
    expect(
      within(bar).getByRole("button", { name: "Go to Note timing" })
    ).toBeVisible()
  })
  it("dispatches one sampler patch when the loop crossfade knob moves", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    const crossfade = slider("Loop crossfade")
    expect(crossfade).toHaveAttribute("aria-valuenow", "0")
    expect(crossfade).toHaveAttribute("aria-disabled", "true")
    await user.click(
      within(settings()).getByRole("button", { name: "Forward" })
    )
    await settle()
    expect(crossfade).not.toHaveAttribute("aria-disabled", "true")
    const send = vi.spyOn(backend, "dispatch")
    tap(crossfade, "ArrowUp")
    await settle()
    expect(send).toHaveBeenCalledTimes(1)
    expect(send).toHaveBeenCalledWith(
      {
        type: "updateSampler",
        id: channel("Kick").id,
        patch: { loopCrossfade: 0.001 },
      },
      expect.any(Number)
    )
    send.mockRestore()
  })

  it("leaves the crossfade knob inert while the loop is off", async () => {
    await openSettings("Kick")
    const send = vi.spyOn(backend, "dispatch")
    tap(slider("Loop crossfade"), "End")
    await settle()
    expect(send).not.toHaveBeenCalled()
    send.mockRestore()
  })

  it("edits sampler loop modes and points through undoable project commands", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    const modes = within(settings()).getByRole("group", {
      name: "Sample loop mode",
    })
    expect(within(modes).getByRole("button", { name: "Off" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )
    await user.click(within(modes).getByRole("button", { name: "Forward" }))
    await settle()
    expect(sampler("Kick").loopMode).toBe("forward")
    expect(labels().at(-1)).toBe("Change sample loop mode")
    tap(slider("Loop start"), "ArrowUp", 20)
    await settle()
    expect(sampler("Kick").loopStart).toBeCloseTo(0.02)
    expect(labels().at(-1)).toBe("Change sample loop range")
    tap(slider("Loop end"), "ArrowDown", 20)
    await settle()
    expect(sampler("Kick").loopEnd).toBeCloseTo(0.98)
    await user.click(within(modes).getByRole("button", { name: "Ping-pong" }))
    await settle()
    expect(sampler("Kick").loopMode).toBe("pingPong")
    // Turning it off preserves the selected points.
    await user.click(within(modes).getByRole("button", { name: "Off" }))
    await settle()
    expect(sampler("Kick").loopMode ?? "off").toBe("off")
    expect(sampler("Kick").loopStart).toBeCloseTo(0.02)
    await act(() => undo())
    expect(sampler("Kick").loopMode).toBe("pingPong")
  })

  it("prevents loop point crossings and follows the selected sampler", async () => {
    await openSettings("Kick")
    await act(() =>
      dispatch({
        type: "updateSampler",
        id: channel("Kick").id,
        patch: { loopStart: 0.4, loopEnd: 0.401 },
      })
    )
    tap(slider("Loop start"), "ArrowUp", 3)
    await settle()
    expect(sampler("Kick").loopStart).toBeLessThan(sampler("Kick").loopEnd ?? 1)
    expect(toast.error).not.toHaveBeenCalled()
    await act(() => useUiStore.getState().selectChannel(channel("Hat").id))
    expect(slider("Loop start")).toHaveAttribute("aria-valuenow", "0")
    expect(slider("Loop end")).toHaveAttribute("aria-valuenow", "1")
  })

  it("is closed until a channel is opened, and follows the selection", async () => {
    const user = userEvent.setup()
    render(<ChannelRackPanel />)
    expect(screen.queryByRole("complementary")).toBeNull()

    await user.click(screen.getByRole("button", { name: "Kick" }))
    await settle()
    expect(
      within(settings()).getByRole("heading", { name: "Kick" })
    ).toBeVisible()

    await user.click(screen.getByRole("button", { name: "Hat" }))
    await settle()
    expect(
      within(settings()).getByRole("heading", { name: "Hat" })
    ).toBeVisible()
  })

  it("closes from its own button and opens from the action", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    const close = within(settings()).getByRole("button", {
      name: "Channel settings",
    })
    await user.click(close)
    expect(screen.queryByRole("complementary")).toBeNull()

    await runAction("channelRack.settings")
    await settle()
    expect(settings()).toBeVisible()
  })

  it("says what to do when no channel is selected", async () => {
    useRackStore.getState().setInspectorOpen(true)
    render(<ChannelRackPanel />)
    expect(settings()).toHaveTextContent("No channel selected")
    expect(settings()).toHaveTextContent("Click a channel's name")
  })
})

describe("keys while a setting has the focus", () => {
  const DELETE = { key: "Delete", code: "Delete" }
  const CTRL_D = { key: "d", code: "KeyD", ctrlKey: true }
  const asked = () => usePromptStore.getState().confirm

  it("does nothing with Delete on a setting: no reset, and no question about the channel", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "ArrowUp", 3)
    await settle()
    expect(sampler("Kick").tune).toBe(3)

    slider("Tune").focus()
    // Used up by the settings, so it goes nowhere else either.
    expect(fireEvent.keyDown(slider("Tune"), DELETE)).toBe(false)
    expect(
      fireEvent.keyDown(slider("Tune"), { key: "Backspace", code: "Backspace" })
    ).toBe(true)
    await settle()
    expect(asked()).toBeNull()
    expect(sampler("Kick").tune).toBe(3)
    expect(project().channels.map((item) => item.name)).toContain("Kick")

    // A double-click is what puts a setting back.
    fireEvent.doubleClick(slider("Tune"))
    await settle()
    expect(sampler("Kick").tune).toBe(0)
  })

  it("does not duplicate the channel on Ctrl+D", async () => {
    await openSettings("Kick")
    const count = project().channels.length
    slider("Tune").focus()
    // The key is used up, so the webview does nothing with it either.
    expect(fireEvent.keyDown(slider("Tune"), CTRL_D)).toBe(false)
    await settle()
    expect(project().channels).toHaveLength(count)
  })

  it("uses the keys up on a setting that has nothing to put back", async () => {
    await openSettings("Kick")
    const count = project().channels.length
    const reverse = within(settings()).getByRole("switch", { name: "Reverse" })
    reverse.focus()
    expect(fireEvent.keyDown(reverse, DELETE)).toBe(false)
    expect(fireEvent.keyDown(reverse, CTRL_D)).toBe(false)
    await settle()
    expect(asked()).toBeNull()
    expect(project().channels).toHaveLength(count)
  })

  it("still plays with Space and undoes with Ctrl+Z", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "ArrowUp", 2)
    await settle()
    slider("Tune").focus()
    fireEvent.keyDown(slider("Tune"), { key: "z", code: "KeyZ", ctrlKey: true })
    await settle()
    expect(sampler("Kick").tune).toBe(0)

    fireEvent.keyDown(slider("Tune"), { key: " ", code: "Space" })
    await settle()
    expect(useTransportStore.getState().playing).toBe(true)
  })

  it("leaves Delete on a channel's own row what it was", async () => {
    await openSettings("Kick")
    const name = screen.getByRole("button", { name: "Kick" })
    name.focus()
    fireEvent.keyDown(name, DELETE)
    await settle()
    expect(asked()?.title).toBe("Delete Kick?")
    asked()?.resolve(null)
  })
})

describe("the sample", () => {
  it("shows the sample's name and facts", async () => {
    await openSettings("Kick")
    expect(settings()).toHaveTextContent("Kick")
    expect(settings()).toHaveTextContent("420 ms")
    expect(settings()).toHaveTextContent("44.1 kHz")
    expect(settings()).toHaveTextContent("mono")
  })

  it("trims the start and the end, each drag one undo step", async () => {
    await openSettings("Kick")
    const sent = vi.spyOn(backend, "dispatch")
    const start = slider("Region start")
    const end = slider("Region end")

    tap(start, "ArrowRight", 3)
    await settle()
    // An arrow key moves a handle by a hundredth of its travel.
    expect(sampler("Kick").start).toBeCloseTo(0.03, 3)
    expect(sampler("Kick").end).toBe(1)
    expect(sent).toHaveBeenCalledTimes(3)
    expect(new Set(sent.mock.calls.map((call) => call[1])).size).toBe(1)
    expect(labels()).toEqual(["Change sample range"])

    tap(end, "ArrowLeft", 2)
    await settle()
    expect(sampler("Kick").end).toBeCloseTo(0.98, 2)
    expect(labels()).toEqual(["Change sample range", "Change sample range"])

    await undo()
    await undo()
    expect(sampler("Kick").start).toBe(0)
    expect(sampler("Kick").end).toBe(1)
    expect(start).toHaveAttribute("aria-valuenow", "0")
  })

  it("never lets the handles cross", async () => {
    await openSettings("Kick")
    tap(slider("Region start"), "End")
    await settle()
    expect(sampler("Kick").start).toBeLessThan(1)
    expect(sampler("Kick").end).toBeGreaterThan(sampler("Kick").start)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("fetches each waveform once", async () => {
    const info = vi.spyOn(backend, "sampleInfoById")
    await openSettings("Kick")
    useUiStore.getState().selectChannel(channel("Hat").id)
    await settle()
    useUiStore.getState().selectChannel(channel("Kick").id)
    await settle()
    expect(slider("Region start")).toBeVisible()
    // The rows ask too, to mark a channel whose file is missing.
    const asked = info.mock.calls.map((call) => call[0])
    expect(asked).toEqual(expect.arrayContaining([sampler("Kick").sample]))
    expect(new Set(asked).size).toBe(asked.length)
  })

  it("says so when the waveform cannot be read, and keeps the controls", async () => {
    vi.spyOn(backend, "sampleInfoById").mockRejectedValue(
      new Error("The file is missing.")
    )
    await openSettings("Kick")
    expect(within(settings()).getByRole("alert")).toHaveTextContent(
      "The sample file is missing or cannot be read. The file is missing."
    )
    expect(slider("Tune")).toBeVisible()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("marks the channel in the rack, and looks again after a reload", async () => {
    const user = userEvent.setup()
    const info = vi.spyOn(backend, "sampleInfoById")
    info.mockRejectedValue(new Error("The file is missing."))
    await openSettings("Kick")
    const kick = screen.getByRole("button", {
      name: "Kick, sample file is missing",
    })
    expect(kick.querySelector("[data-sample-missing]")).toHaveAttribute(
      "title",
      "Sample file is missing"
    )

    // The file is back: reloading reads the samples that failed again.
    info.mockRestore()
    await user.click(
      within(settings()).getByRole("button", {
        name: "Reload missing samples",
      })
    )
    await settle()
    expect(
      screen.queryByRole("button", { name: "Kick, sample file is missing" })
    ).toBeNull()
    expect(slider("Region start")).toBeVisible()
  })

  it("does not call a sample missing when another project overtook the read", async () => {
    const answers: ((error: Error) => void)[] = []
    const info = vi.spyOn(backend, "sampleInfoById").mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          answers.push(reject)
        })
    )
    await openSettings("Kick")
    const asked = answers.length
    expect(asked).toBeGreaterThan(0)

    // Another project loads while the reads are on their way, and the
    // backend refuses them for that reason.
    samplesReloaded()
    info.mockRestore()
    for (const reject of answers) {
      reject(new Error("Another project was opened in the meantime."))
    }
    await settle()

    expect(
      screen.queryByRole("button", { name: /sample file is missing/ })
    ).toBeNull()
    // The samples still on screen were read again, and are there.
    expect(slider("Region start")).toBeVisible()
  })

  it("says how a reload of missing samples went", async () => {
    const reload = vi.spyOn(backend, "samplesReload")
    reload.mockResolvedValueOnce(0)
    await runAction("file.reloadSamples")
    expect(toast.success).toHaveBeenLastCalledWith("All samples loaded")
    reload.mockResolvedValueOnce(1)
    await runAction("file.reloadSamples")
    expect(toast.error).toHaveBeenLastCalledWith("1 sample is still missing")
    reload.mockResolvedValueOnce(3)
    await runAction("file.reloadSamples")
    expect(toast.error).toHaveBeenLastCalledWith("3 samples are still missing")
  })

  it("explains a channel with no sample and lets one be chosen", async () => {
    const user = userEvent.setup()
    await dispatch({ type: "addChannel", name: "Empty" })
    await openSettings("Empty")
    expect(settings()).toHaveTextContent(
      "No sample. Drag one here from the browser."
    )
    expect(
      within(settings()).getByRole("group", { name: "Piano keyboard" })
    ).toHaveAttribute("data-disabled")

    await user.click(
      within(settings()).getByRole("button", { name: /Choose a sample/ })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: "Hat" }))
    await settle()
    expect(sampler("Empty").sample).toBe(sampler("Hat").sample)
    expect(labels().at(-1)).toBe("Change channel sample")
    expect(settings()).not.toHaveTextContent("No sample.")
  })

  it("takes a sample dropped on the waveform", async () => {
    await openSettings("Clap")
    const zone = settings().querySelector('[data-slot="sample-drop"]')!
    const data = dragData(
      SAMPLE_DRAG_TYPE,
      JSON.stringify({
        path: "/factory/Drums/Percussion/Tom 01.wav",
        name: "Tom 01",
      })
    )
    const over = createEvent.dragOver(zone, { dataTransfer: data })
    fireEvent(zone, over)
    expect(over.defaultPrevented).toBe(true)
    expect(zone).toHaveTextContent("Drop to use this sample")

    fireEvent.drop(zone, { dataTransfer: data })
    await settle()
    const used = project().samples.find(
      (item) => item.id === sampler("Clap").sample
    )
    expect(used?.name).toBe("Tom 01")
    expect(labels()).toEqual(["Change channel sample"])
    expect(zone).not.toHaveTextContent("Drop to use this sample")
  })
})

describe("the sound", () => {
  it("tunes in semitones and in cents", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "ArrowUp", 2)
    await settle()
    expect(sampler("Kick").tune).toBe(2)
    expect(slider("Tune")).toHaveAttribute("aria-valuetext", "+2 st")

    tap(slider("Fine"), "ArrowDown")
    await settle()
    expect(sampler("Kick").tune).toBeCloseTo(1.99, 5)
    // 1.99 is shown as two semitones less one cent.
    expect(slider("Tune")).toHaveAttribute("aria-valuenow", "2")
    expect(slider("Fine")).toHaveAttribute("aria-valuetext", "−1 ct")
    expect(labels()).toEqual(["Change tuning", "Change tuning"])
  })

  it("keeps the semitone on show while Fine goes to either end", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "ArrowUp", 2)
    await settle()

    // Fine all the way down: 1.5, read as two semitones less 50 cents.
    tap(slider("Fine"), "Home")
    await settle()
    expect(sampler("Kick").tune).toBe(1.5)
    expect(slider("Tune")).toHaveAttribute("aria-valuenow", "2")
    expect(slider("Fine")).toHaveAttribute("aria-valuetext", "−50 ct")

    // And all the way up: 2.5 is still semitone 2, now with +50 cents.
    tap(slider("Fine"), "End")
    await settle()
    expect(sampler("Kick").tune).toBe(2.5)
    expect(slider("Tune")).toHaveAttribute("aria-valuenow", "2")
    expect(slider("Fine")).toHaveAttribute("aria-valuetext", "+50 ct")

    // Tune moves by whole semitones and takes the cents along.
    tap(slider("Tune"), "ArrowUp")
    await settle()
    expect(sampler("Kick").tune).toBe(3.5)
    expect(slider("Tune")).toHaveAttribute("aria-valuenow", "3")
    expect(slider("Fine")).toHaveAttribute("aria-valuetext", "+50 ct")
    tap(slider("Tune"), "ArrowDown", 2)
    await settle()
    expect(sampler("Kick").tune).toBe(1.5)
    expect(slider("Tune")).toHaveAttribute("aria-valuenow", "1")
    expect(slider("Fine")).toHaveAttribute("aria-valuetext", "+50 ct")
  })

  it("keeps the tuning inside its range", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "End")
    await settle()
    tap(slider("Fine"), "End")
    await settle()
    expect(sampler("Kick").tune).toBe(48)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("sets the root key by note name", async () => {
    await openSettings("Kick")
    expect(slider("Root")).toHaveAttribute("aria-valuetext", "C5")
    tap(slider("Root"), "ArrowDown")
    await settle()
    expect(sampler("Kick").rootKey).toBe(59)
    expect(slider("Root")).toHaveAttribute("aria-valuetext", "B4")
    expect(labels()).toEqual(["Change root key"])
  })

  it("reads note names the way it prints them", () => {
    expect(parseNoteName("C5")).toBe(60)
    expect(parseNoteName("c#5")).toBe(61)
    expect(parseNoteName("Bb4")).toBe(58)
    expect(parseNoteName(" a 4 ")).toBe(57)
    expect(parseNoteName("64")).toBe(64)
    expect(parseNoteName("loud")).toBeNull()
  })

  it("changes gain, reverse, cut itself and the cut group", async () => {
    const user = userEvent.setup()
    await openSettings("Hat")
    tap(slider("Gain"), "End")
    await settle()
    expect(sampler("Hat").gain).toBe(2)

    await user.click(
      within(settings()).getByRole("switch", { name: "Reverse" })
    )
    await user.click(
      within(settings()).getByRole("switch", { name: "Cut itself" })
    )
    await settle()
    expect(sampler("Hat").reverse).toBe(true)
    expect(sampler("Hat").cutSelf).toBe(true)

    expect(settings()).toHaveTextContent("none")
    tap(slider("Cut group"), "ArrowUp", 2)
    await settle()
    expect(sampler("Hat").cutGroup).toBe(2)
    expect(labels()).toEqual([
      "Change sample gain",
      "Reverse sample",
      "Toggle cut itself",
      "Change cut group",
    ])
  })

  it("puts a control back with a double-click", async () => {
    await openSettings("Kick")
    tap(slider("Tune"), "ArrowUp", 5)
    await settle()
    fireEvent.doubleClick(slider("Tune"))
    await settle()
    expect(sampler("Kick").tune).toBe(0)
    expect(history().cursor).toBe(2)
  })
})

describe("the envelope", () => {
  const toggle = () =>
    within(settings()).getByRole("switch", { name: "Volume envelope" })
  const editor = () =>
    within(settings()).getByRole("group", { name: "Envelope shape" })

  it("turns on with sensible values and off again", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    expect(sampler("Kick").envelope).toBeNull()
    expect(settings()).toHaveTextContent(
      "Every hit plays the sample to its end"
    )

    await user.click(toggle())
    await settle()
    expect(sampler("Kick").envelope).toEqual(DEFAULT_ENVELOPE)
    expect(editor()).toBeVisible()

    await user.click(toggle())
    await settle()
    expect(sampler("Kick").envelope).toBeNull()
    expect(labels()).toEqual(["Change envelope", "Change envelope"])
  })

  it("edits one shape from the knobs and from the nodes", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    await user.click(toggle())
    await settle()

    // The knob, not the node of the same name inside the editor.
    const releaseKnob = within(settings())
      .getAllByRole("slider", { name: "Release" })
      .find((element) => !editor().contains(element))!
    tap(releaseKnob, "End")
    await settle()
    expect(sampler("Kick").envelope?.releaseMs).toBe(10_000)

    const decayNode = within(editor()).getByRole("slider", {
      name: "Decay and sustain",
    })
    tap(decayNode, "ArrowDown", 4)
    await settle()
    const envelope = sampler("Kick").envelope
    expect(envelope?.sustain).toBeCloseTo(0.96, 5)
    expect(envelope?.releaseMs).toBe(10_000)
    expect(envelope?.attackMs).toBe(DEFAULT_ENVELOPE.attackMs)
    // Turning it on, the knob and the held key are three steps.
    expect(labels()).toEqual([
      "Change envelope",
      "Change envelope",
      "Change envelope",
    ])
  })

  it("remembers the shape while it is off", async () => {
    const user = userEvent.setup()
    await openSettings("Kick")
    await user.click(toggle())
    await settle()
    const sustain = within(settings())
      .getAllByRole("slider", { name: "Sustain" })
      .find((element) => !editor().contains(element))!
    tap(sustain, "Home")
    await settle()
    expect(sampler("Kick").envelope?.sustain).toBe(0)

    await user.click(toggle())
    await user.click(toggle())
    await settle()
    expect(sampler("Kick").envelope?.sustain).toBe(0)
  })
})

describe("playing and routing", () => {
  it("plays the channel from the keyboard and always lets go", async () => {
    await openSettings("Kick")
    const on = vi.spyOn(backend, "auditionNoteOn")
    const off = vi.spyOn(backend, "auditionNoteOff")
    const kick = channel("Kick").id
    const keys = within(settings()).getByRole("group", {
      name: "Piano keyboard",
    })
    const key = within(keys).getByRole("button", { name: "D5" })
    key.focus()
    fireEvent.keyDown(key, { key: "Enter" })
    expect(on).toHaveBeenCalledWith(kick, 62, 0.8)
    fireEvent.keyUp(key, { key: "Enter" })
    expect(off).toHaveBeenCalledWith(kick, 62)
    expect(off).toHaveBeenCalledTimes(1)
  })

  it("has a menu of its own for the note names and how many keys it shows", async () => {
    await openSettings("Kick")
    const keys = () =>
      within(settings()).getByRole("group", { name: "Piano keyboard" })
    const menuOf = async () => {
      // The app's menu, so the webview's own stays away.
      expect(
        fireEvent.contextMenu(
          within(keys()).getByRole("button", { name: "D5" }),
          { clientX: 20, clientY: 20 }
        )
      ).toBe(false)
      await act(settle)
      return screen.getByRole("menu")
    }
    const pick = async (name: RegExp) => {
      fireEvent.click(
        within(await menuOf()).getByRole("menuitemcheckbox", { name })
      )
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    }

    const menu = await menuOf()
    expect(
      within(menu)
        .getAllByRole("menuitemcheckbox")
        .map(
          (item) => `${item.textContent}:${item.getAttribute("aria-checked")}`
        )
    ).toEqual([
      "Note names:true",
      "Around the root key (C3 to C7):true",
      "Six octaves (C2 to C8):false",
      "Every key (C0 to G10):false",
    ])
    fireEvent.keyDown(menu, { key: "Escape" })
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())

    // A sampler starts with the four octaves around its root key.
    expect(within(keys()).queryByRole("button", { name: "C0" })).toBeNull()
    await pick(/^Every key/)
    expect(within(keys()).getByRole("button", { name: "C0" })).toBeVisible()
    expect(within(keys()).getByRole("button", { name: "G10" })).toBeVisible()
    expect(useRackStore.getState().keyboardRange).toBe("full")

    expect(keys()).toHaveTextContent("C5")
    await pick(/^Note names/)
    expect(keys()).not.toHaveTextContent("C5")
    expect(useRackStore.getState().keyboardLabels).toBe(false)

    await pick(/^Around the root key/)
    expect(within(keys()).queryByRole("button", { name: "C0" })).toBeNull()
  })

  it("lights the root key", async () => {
    await openSettings("Kick")
    const keys = within(settings()).getByRole("group", {
      name: "Piano keyboard",
    })
    expect(within(keys).getByRole("button", { name: "C5" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )
  })

  it("shows the mixer track and jumps to it", async () => {
    const user = userEvent.setup()
    await openSettings("Snare")
    useUiStore.getState().setPanelVisible("mixer", false)
    expect(
      within(settings()).getByRole("button", { name: "Mixer track: Snare" })
    ).toHaveTextContent("4")

    await user.click(
      within(settings()).getByRole("button", { name: "Show in mixer" })
    )
    expect(useUiStore.getState().selectedTrack).toBe(
      channel("Snare").mixerTrack
    )
    expect(useUiStore.getState().panels.mixer).toBe(true)
  })

  it("routes from the settings to another track", async () => {
    const user = userEvent.setup()
    await openSettings("Snare")
    await user.click(
      within(settings()).getByRole("button", { name: "Mixer track: Snare" })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /Kick/ }))
    await settle()
    expect(channel("Snare").mixerTrack).toBe(channel("Kick").mixerTrack)
    expect(
      within(settings()).getByRole("button", { name: "Mixer track: Kick" })
    ).toBeVisible()
  })
})
