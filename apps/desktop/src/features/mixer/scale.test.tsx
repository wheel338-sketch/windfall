import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { dispatch } from "@/lib/store/project"
import { useUiStore } from "@/lib/store/ui"
import { MAX_MIXER_TRACKS } from "@/lib/units"
import { startTestApp } from "@/test/harness"

import MixerPanel from "."
import { adaptiveStripWidth, STRIP_WIDTH } from "./layout"
import { useMixerUi } from "./mixer-ui"
import {
  channelNamed,
  flush,
  sizeMixer,
  strip,
  stubCanvas,
  trackNamed,
  tracks,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void

beforeEach(async () => {
  stubCanvas()
  useMixerUi.setState(useMixerUi.getInitialState(), true)
  ;({ stop } = await startTestApp())
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

/** Fills the mixer to the current ordinary-track limit. */
async function fillMixer() {
  await dispatch({ type: "batch", label: "Fill mixer for virtualization QA",
    commands: Array.from({ length: MAX_MIXER_TRACKS - tracks().filter((track) => !track.current).length },
      () => ({ type: "addMixerTrack" as const })) })
}

function scroller(): HTMLElement {
  const element = document.querySelector<HTMLElement>(
    "[data-slot=mixer-dock-middle]"
  )
  if (!element) throw new Error("The mixer is not mounted")
  return element
}

function stripOf(id: number): HTMLElement {
  const element = scroller().querySelector<HTMLElement>(`[data-track="${id}"]`)
  if (!element) throw new Error(`The strip of track ${id} is not mounted`)
  return element
}

const mountedInserts = () =>
  [...scroller().querySelectorAll<HTMLElement>("[data-track]")].map((item) =>
    Number(item.dataset.track)
  )
const insertIds = () =>
  tracks()
    .slice(1)
    .map((track) => track.id)
const selectTrack = (id: number) =>
  act(() => useUiStore.getState().selectTrack(id))

function scrollTo(left: number) {
  act(() => {
    scroller().scrollLeft = left
    fireEvent.scroll(scroller())
  })
}

describe("a full mixer", () => {
  beforeEach(async () => {
    await fillMixer()
    // Ten strips wide.
    sizeMixer(10 * STRIP_WIDTH, 400)
  })

  it("mounts the strips in view instead of every track at the 500-track limit", () => {
    render(<MixerPanel />)
    // The limit includes Master in addition to 500 insert tracks.
    expect(MAX_MIXER_TRACKS).toBe(501)
    expect(tracks()).toHaveLength(MAX_MIXER_TRACKS)
    expect(insertIds()).toHaveLength(500)
    // The ten in view and a few more on the right.
    expect(mountedInserts()).toEqual(insertIds().slice(0, 13))
    expect(strip("Master")).toBeVisible()
  })

  it("mounts other strips as the mixer scrolls", () => {
    render(<MixerPanel />)
    scrollTo(50 * STRIP_WIDTH)
    expect(mountedInserts()).toEqual(insertIds().slice(48, 63))

    const endStart = insertIds().length - 10
    scrollTo(endStart * STRIP_WIDTH)
    expect(mountedInserts()).toEqual(insertIds().slice(endStart - 2))
  })

  it("mounts nothing new for a scroll that reveals no new strip", () => {
    render(<MixerPanel />)
    scrollTo(50 * STRIP_WIDTH)
    const before = mountedInserts()
    scrollTo(50 * STRIP_WIDTH + STRIP_WIDTH / 2)
    expect(mountedInserts()).toEqual(before)
  })

  it("keeps the selected strip mounted when it scrolls out of view", () => {
    render(<MixerPanel />)
    const third = insertIds()[2]
    selectTrack(third)
    scrollTo(60 * STRIP_WIDTH)
    expect(mountedInserts()).toEqual([...insertIds().slice(58, 73), third])
  })

  it("scrolls a selected strip into view", () => {
    render(<MixerPanel />)
    const hundredth = insertIds()[99]
    selectTrack(hundredth)
    // Just far enough for its right edge to be in view.
    expect(scroller().scrollLeft).toBe(90 * STRIP_WIDTH)
    expect(stripOf(hundredth)).toHaveAttribute("data-selected")

    selectTrack(insertIds()[4])
    expect(scroller().scrollLeft).toBe(4 * STRIP_WIDTH)
  })

  it("scrolls to the strip of the channel selected in the rack", async () => {
    render(<MixerPanel />)
    const far = tracks()[80]
    await dispatch({
      type: "updateChannel",
      id: channelNamed("Hat").id,
      patch: { mixerTrack: far.id },
    })
    await flush()
    expect(scroller().scrollLeft).toBe(0)

    act(() => useUiStore.getState().selectChannel(channelNamed("Hat").id))
    expect(scroller().scrollLeft).toBe(70 * STRIP_WIDTH)
    expect(stripOf(far.id)).toHaveAttribute("data-linked")
    expect(
      within(stripOf(far.id)).getByRole("button", { name: "Channel Hat" })
    ).toBeVisible()
  })

  it("focuses the next strip when an arrow key moves past the mounted ones", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    const ids = insertIds()
    selectTrack(ids[11])
    act(() => stripOf(ids[11]).focus())
    for (let step = 0; step < 5; step += 1) {
      await user.keyboard("{ArrowRight}")
    }
    expect(useUiStore.getState().selectedTrack).toBe(ids[16])
    expect(stripOf(ids[16])).toHaveFocus()
  })

  it("turns the add button off when the mixer is full", () => {
    render(<MixerPanel />)
    scrollTo((insertIds().length - 10) * STRIP_WIDTH)
    expect(
      screen.getByRole("button", { name: "Add mixer track" })
    ).toBeDisabled()
  })
})

describe("adaptive widths", () => {
  it("spreads a few strips over the room instead of cutting their names short", () => {
    sizeMixer(800, 420)
    render(<MixerPanel />)
    const inserts = tracks().filter((track) => track.id !== 0 && !track.current)
    const width = adaptiveStripWidth(800, inserts.length)
    expect(width).toBeGreaterThan(STRIP_WIDTH)
    expect(strip("Kick").parentElement).toHaveStyle({ width: `${width}px` })
  })
})

describe("panel heights", () => {
  const mode = () =>
    document.querySelector("[data-slot=mixer]")?.getAttribute("data-mode")

  it("shows routing inline when the panel is tall", () => {
    sizeMixer(800, 420)
    render(<MixerPanel />)
    expect(mode()).toBe("full")
    const kick = within(strip("Kick"))
    expect(kick.getByRole("button", { name: "Output: Master" })).toBeVisible()
    expect(kick.getByRole("button", { name: "Add send" })).toBeVisible()
    expect(kick.queryByRole("button", { name: "Routing" })).toBeNull()
    expect(kick.getByRole("slider", { name: "Kick volume" })).toHaveAttribute(
      "aria-orientation",
      "vertical"
    )
  })

  it("moves routing behind a button at a medium height", async () => {
    const user = userEvent.setup()
    sizeMixer(800, 260)
    render(<MixerPanel />)
    expect(mode()).toBe("compact")
    const kick = within(strip("Kick"))
    expect(kick.queryByRole("button", { name: "Output: Master" })).toBeNull()
    expect(kick.getByRole("button", { name: "Channel Kick" })).toBeVisible()
    expect(kick.getByRole("button", { name: /^Peak/ })).toBeVisible()
    // The master has nothing to route.
    expect(
      within(strip("Master")).queryByRole("button", { name: "Routing" })
    ).toBeNull()

    await user.click(kick.getByRole("button", { name: "Routing" }))
    const popover = await screen.findByRole("dialog")
    await user.click(
      within(popover).getByRole("button", { name: "Output: Master" })
    )
    await user.click(
      await screen.findByRole("menuitemradio", { name: /Snare/ })
    )
    await flush()
    expect(trackNamed("Kick").output).toBe(trackNamed("Snare").id)

    await user.click(within(popover).getByRole("button", { name: "Add send" }))
    await user.click(await screen.findByRole("menuitem", { name: /Hat/ }))
    await flush()
    expect(trackNamed("Kick").sends).toEqual([
      { target: trackNamed("Hat").id, gain: 1 },
    ])
    expect(
      within(popover).getByRole("slider", { name: "Send to Hat" })
    ).toBeVisible()
  })

  it("keeps the fader upright as long as it can", () => {
    sizeMixer(800, 210)
    render(<MixerPanel />)
    expect(mode()).toBe("tight")
    const kick = within(strip("Kick"))
    expect(kick.getByRole("slider", { name: "Kick volume" })).toHaveAttribute(
      "aria-orientation",
      "vertical"
    )
    expect(kick.queryByRole("button", { name: "Channel Kick" })).toBeNull()
    expect(kick.queryByRole("button", { name: /^Peak/ })).toBeNull()
    expect(kick.getByRole("button", { name: "Routing" })).toBeVisible()
    // The clip light stays on the meter.
    expect(
      strip("Kick").querySelector("[data-slot=level-meter-clip]")
    ).toBeInTheDocument()
  })

  it("lays the fader flat with its readouts when it cannot stand", () => {
    sizeMixer(800, 150)
    render(<MixerPanel />)
    expect(mode()).toBe("flat")
    const kick = within(strip("Kick"))
    expect(kick.getByRole("slider", { name: "Kick volume" })).toHaveAttribute(
      "aria-orientation",
      "horizontal"
    )
    expect(kick.getByRole("button", { name: /^Peak/ })).toBeVisible()
    expect(kick.getByText("0.0 dB")).toBeVisible()
  })

  it("keeps every essential control at the smallest height", async () => {
    const user = userEvent.setup()
    sizeMixer(800, 84)
    render(<MixerPanel />)
    expect(mode()).toBe("mini")
    const kick = within(strip("Kick"))
    expect(kick.getByRole("slider", { name: "Kick volume" })).toHaveAttribute(
      "aria-orientation",
      "horizontal"
    )
    expect(kick.getByRole("slider", { name: "Kick pan" })).toBeVisible()
    expect(kick.getByRole("button", { name: "Mute" })).toBeVisible()
    expect(kick.getByRole("button", { name: "Solo" })).toBeVisible()
    expect(kick.getByText("Kick")).toBeVisible()

    // Channels, output and sends are one click away.
    await user.click(kick.getByRole("button", { name: "Routing" }))
    const popover = await screen.findByRole("dialog")
    expect(
      within(popover).getByRole("button", { name: "Channel Kick" })
    ).toBeVisible()
    expect(
      within(popover).getByRole("button", { name: "Output: Master" })
    ).toBeVisible()
    expect(
      within(popover).getByRole("button", { name: "Add send" })
    ).toBeVisible()

    // The master lists the channels that play straight into it.
    expect(
      within(strip("Master")).getByRole("button", { name: "Channels" })
    ).toBeVisible()
  })

  it("goes back to the popover when send rows would squeeze the fader", async () => {
    sizeMixer(800, 340)
    render(<MixerPanel />)
    expect(mode()).toBe("full")
    await dispatch({
      type: "setSend",
      from: trackNamed("Kick").id,
      to: trackNamed("Hat").id,
      gain: 1,
    })
    await flush()
    expect(mode()).toBe("compact")
  })
})
