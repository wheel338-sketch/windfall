import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type {
  EchoBankParams,
  EffectId,
  EffectKind,
  EffectSlot,
  FrequencyDelayParams,
  TrackId,
} from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor, paramIndex } from "@/features/params"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import {
  chainRuns,
  chainSummary,
  parallelEdits,
  serialEdits,
  type UnitRouting,
} from "./echo-bank/chain"
import { customEditor, EffectEditor } from "./effect-editor"
import {
  bandCenter,
  bandEdges,
  effectiveDelayMs,
  plotPoint,
  shortHz,
  strokeBands,
} from "./frequency-delay/bands"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void
let backend: MockBackend
/** The track the effect under test is on: the first insert. */
let track: TrackId

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  ;({ stop, backend } = await startTestApp())
  track = useProjectStore.getState().project.mixer.tracks[1].id
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

const history = () => useProjectStore.getState().history
const flush = () => act(() => settle())

function slotOf(effect: EffectId): EffectSlot {
  const slot = useProjectStore
    .getState()
    .project.mixer.tracks.find((item) => item.id === track)
    ?.effects.find((item) => item.id === effect)
  if (!slot) throw new Error(`There is no effect ${effect}`)
  return slot
}

function paramsOf<Params>(effect: EffectId): Params {
  return slotOf(effect).params as Params
}

/** The editor of an effect of the project, kept up to date with the store. */
function Editor({ effect }: { effect: EffectId }) {
  const slot = useProjectStore((state) =>
    state.project.mixer.tracks
      .find((item) => item.id === track)
      ?.effects.find((item) => item.id === effect)
  )
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}

/** Adds an effect to the track and shows its editor. */
async function open(kind: EffectKind): Promise<EffectId> {
  const result = await dispatch({ type: "addEffect", track, kind })
  if (!result) throw new Error(`Could not add a ${kind}`)
  const effect = result.created[0]
  render(
    <ValueContextMenus>
      <Editor effect={effect} />
    </ValueContextMenus>
  )
  await flush()
  return effect
}

/** Sets one setting the way a control does, outside any gesture. */
async function set(kind: EffectKind, effect: EffectId, id: string, value: number) {
  const param = paramIndex(effectDescriptor(kind), id)
  await dispatch({ type: "setEffectParam", track, effect, param, value })
  await flush()
}

/** Takes a knob or a bar to the top of its range with the keyboard. */
function toTop(slider: HTMLElement) {
  act(() => slider.focus())
  fireEvent.keyDown(slider, { key: "End" })
  fireEvent.keyUp(slider, { key: "End" })
}

describe("the routing of the echo bank, worked out from its settings", () => {
  const unit = (change: Partial<UnitRouting> = {}): UnitRouting => ({
    enabled: true,
    inputGain: 1,
    outputGain: 1,
    nextSend: 0,
    ...change,
  })
  const off = () => unit({ enabled: false })

  it("reads units side by side, in series and mixed", () => {
    expect(chainSummary([unit(), off(), off()])).toBe("1")
    expect(chainSummary([unit(), unit(), unit()])).toBe("1, 2, 3")
    expect(
      chainSummary([unit({ nextSend: 1 }), unit({ nextSend: -0.5 }), unit()])
    ).toBe("1 → 2 → 3")
    expect(chainRuns([unit({ nextSend: 1 }), unit(), unit(), off()])).toEqual([
      [1, 2],
      [3],
    ])
    expect(chainSummary([off(), off()])).toBe("No unit is on")
  })

  it("does not count a send into, or out of, a unit that is off", () => {
    const units = [unit({ nextSend: 1 }), off(), unit()]
    expect(chainSummary(units)).toBe("1, 3")
    expect(chainSummary([off(), unit({ nextSend: 1 }), unit()])).toBe("2 → 3")
    // The last unit has nowhere to send, whatever its setting says.
    expect(chainSummary([unit(), unit({ nextSend: 1 })])).toBe("1, 2")
  })

  it("puts neighbors that are on in series, and everything side by side again", () => {
    const units = [unit(), unit(), unit(), off(), unit()]
    expect(serialEdits(units)).toEqual([
      { id: "units.0.nextSend", value: 1 },
      { id: "units.1.nextSend", value: 1 },
      { id: "units.1.inputGain", value: 0 },
      { id: "units.2.inputGain", value: 0 },
    ])
    const serial = [
      unit({ nextSend: 1 }),
      unit({ nextSend: 1, inputGain: 0 }),
      unit({ inputGain: 0 }),
      off(),
      unit(),
    ]
    expect(serialEdits(serial)).toEqual([])
    expect(chainSummary(serial)).toBe("1 → 2 → 3, 5")
    expect(parallelEdits(serial)).toEqual([
      { id: "units.0.nextSend", value: 0 },
      { id: "units.1.nextSend", value: 0 },
      { id: "units.1.inputGain", value: 1 },
      { id: "units.2.inputGain", value: 1 },
    ])
    expect(parallelEdits(units)).toEqual([])
  })
})

describe("the geometry of the frequency delay's plot", () => {
  const box = { left: 10, top: 20, width: 160, height: 100 }

  it("divides 20 Hz to 20 kHz into sixteen bands of equal ratio", () => {
    expect(bandEdges(0).from).toBeCloseTo(20, 6)
    expect(bandEdges(15).to).toBeCloseTo(20_000, 3)
    for (let band = 0; band < 15; band += 1) {
      expect(bandEdges(band).to).toBeCloseTo(bandEdges(band + 1).from, 6)
      expect(bandEdges(band).to / bandEdges(band).from).toBeCloseTo(
        1000 ** (1 / 16),
        9
      )
    }
    expect(bandCenter(0)).toBeGreaterThan(20)
    expect(bandCenter(0)).toBeLessThan(bandEdges(0).to)
    expect(shortHz(24.8)).toBe("25")
    expect(shortHz(1540)).toBe("1.5k")
    expect(shortHz(2000)).toBe("2k")
    expect(shortHz(16_100)).toBe("16k")
  })

  it("finds the band and the height under a pointer, and holds at the edges", () => {
    expect(plotPoint(15, 95, box)).toEqual({ band: 0, fraction: 0.25 })
    expect(plotPoint(169, 20, box)).toEqual({ band: 15, fraction: 1 })
    expect(plotPoint(-50, 500, box)).toEqual({ band: 0, fraction: 0 })
    expect(plotPoint(900, -3, box)).toEqual({ band: 15, fraction: 1 })
    expect(plotPoint(15, 95, { ...box, width: 0 })).toBeNull()
  })

  it("fills in the bands a fast stroke jumped over, in either direction", () => {
    const from = { band: 2, fraction: 0.2 }
    expect(strokeBands(null, from)).toEqual([from])
    expect(strokeBands(from, { band: 2, fraction: 0.9 })).toEqual([
      { band: 2, fraction: 0.9 },
    ])
    const right = strokeBands(from, { band: 6, fraction: 1 })
    expect(right.map((point) => point.band)).toEqual([3, 4, 5, 6])
    for (const [index, fraction] of [0.4, 0.6, 0.8, 1].entries()) {
      expect(right[index].fraction).toBeCloseTo(fraction, 12)
    }
    const left = strokeBands({ band: 6, fraction: 1 }, { band: 4, fraction: 0 })
    expect(left).toEqual([
      { band: 5, fraction: 0.5 },
      { band: 4, fraction: 0 },
    ])
  })

  it("says how long a band's echo really is under Scale and Short range", () => {
    expect(effectiveDelayMs(400, 1, false)).toBe(400)
    expect(effectiveDelayMs(400, 0.5, false)).toBe(200)
    expect(effectiveDelayMs(400, -1, false)).toBe(600)
    expect(effectiveDelayMs(400, 0, false)).toBe(0)
    expect(effectiveDelayMs(400, 1, true)).toBeCloseTo(40, 9)
  })
})

describe("the echo bank's editor", () => {
  const bank = (effect: EffectId) => paramsOf<EchoBankParams>(effect)
  const summary = () =>
    document.querySelector('[data-slot="echo-chain-summary"]')?.textContent
  const link = (unit: number) =>
    document.querySelector<HTMLElement>(`[data-link="${unit}"]`)

  it("is purpose-built, like the frequency delay's", () => {
    expect(customEditor("echoBank")).toBeDefined()
    expect(customEditor("frequencyDelay")).toBeDefined()
  })

  it("shows all eight units and the seven sends between them at once", async () => {
    await open("echoBank")
    const editor = within(
      screen.getByRole("group", { name: "Echo bank settings" })
    )
    const chain = within(editor.getByRole("radiogroup", { name: "Unit to edit" }))
    expect(chain.getAllByRole("radio")).toHaveLength(8)
    const sends = chain.getAllByRole("slider")
    expect(sends.map((send) => send.getAttribute("aria-label"))).toEqual(
      [1, 2, 3, 4, 5, 6, 7].map((unit) => `Unit ${unit} next send`)
    )
    // The last unit's send has no destination and is not offered.
    expect(
      editor.queryByRole("slider", { name: "Unit 8 next send" })
    ).not.toBeInTheDocument()
    for (let unit = 1; unit <= 8; unit += 1) {
      expect(
        chain.getByRole("button", { name: `Unit ${unit} enabled` })
      ).toHaveAttribute("aria-pressed", unit === 1 ? "true" : "false")
    }
    expect(summary()).toBe("Routing: 1")
  })

  it("edits the unit picked in the chain, and only that one", async () => {
    const user = userEvent.setup()
    const effect = await open("echoBank")
    expect(screen.getByRole("group", { name: "Unit 1" })).toBeVisible()
    expect(screen.getByRole("slider", { name: "Unit 1 feedback" })).toBeVisible()

    await user.click(screen.getByRole("radio", { name: "Edit unit 3" }))
    expect(screen.getByRole("radio", { name: "Edit unit 3" })).toHaveAttribute(
      "aria-checked",
      "true"
    )
    expect(screen.getByRole("group", { name: "Unit 3" })).toBeVisible()
    expect(
      screen.queryByRole("slider", { name: "Unit 1 feedback" })
    ).not.toBeInTheDocument()
    // Picking a unit is a matter of the view: nothing was sent or stored.
    expect(history().cursor).toBe(1)

    const before = bank(effect)
    toTop(screen.getByRole("slider", { name: "Unit 3 feedback" }))
    await flush()
    expect(bank(effect).units[2].feedback).toBe(1)
    expect(bank(effect).units[0]).toEqual(before.units[0])
    expect(bank(effect).units[3]).toEqual(before.units[3])
    expect(history().cursor).toBe(2)
  })

  it("has a control for every setting of the picked unit", async () => {
    const user = userEvent.setup()
    await open("echoBank")
    await user.click(screen.getByRole("radio", { name: "Edit unit 5" }))
    const ids = effectDescriptor("echoBank")
      .params.map((info) => info.id)
      .filter((id) => id.startsWith("units.4."))
    expect(ids).toHaveLength(27)
    for (const id of ids) {
      // A synced unit shows its note length in place of its time.
      if (id === "units.4.timeMs") continue
      expect(
        document.querySelector(`[data-param="${id}"]`),
        id
      ).not.toBeNull()
    }
    await user.click(screen.getByRole("switch", { name: "Unit 5 sync" }))
    await flush()
    expect(
      document.querySelector('[data-param="units.4.timeMs"]')
    ).not.toBeNull()
    expect(document.querySelector('[data-param="units.4.division"]')).toBeNull()
  })

  it("links two units with the send between them, with history", async () => {
    const user = userEvent.setup()
    const effect = await open("echoBank")
    await user.click(screen.getByRole("button", { name: "Unit 2 enabled" }))
    await flush()
    expect(bank(effect).units[1].enabled).toBe(true)
    expect(summary()).toBe("Routing: 1, 2")
    expect(link(0)).not.toHaveAttribute("data-linked")

    const cursor = history().cursor
    toTop(screen.getByRole("slider", { name: "Unit 1 next send" }))
    await flush()
    expect(bank(effect).units[0].nextSend).toBe(1)
    expect(summary()).toBe("Routing: 1 → 2")
    expect(link(0)).toHaveAttribute("data-linked")
    expect(link(1)).not.toHaveAttribute("data-linked")
    expect(history().cursor).toBe(cursor + 1)

    await undo()
    await flush()
    expect(bank(effect).units[0].nextSend).toBe(0)
    expect(summary()).toBe("Routing: 1, 2")
    await redo()
    await flush()
    expect(summary()).toBe("Routing: 1 → 2")

    // Switching the second unit off breaks the chain without losing the send.
    await user.click(screen.getByRole("button", { name: "Unit 2 enabled" }))
    await flush()
    expect(summary()).toBe("Routing: 1")
    expect(bank(effect).units[0].nextSend).toBe(1)
  })

  it("puts the units in series or side by side as one undo step each", async () => {
    const user = userEvent.setup()
    const effect = await open("echoBank")
    await set("echoBank", effect, "units.1.enabled", 1)
    await set("echoBank", effect, "units.2.enabled", 1)
    const parallel = bank(effect)
    const cursor = history().cursor

    await user.click(screen.getByRole("button", { name: "Series" }))
    await flush()
    const units = bank(effect).units
    expect(units.map((unit) => unit.nextSend)).toEqual([1, 1, 0, 0, 0, 0, 0, 0])
    expect(units.map((unit) => unit.inputGain)).toEqual([1, 0, 0, 1, 1, 1, 1, 1])
    expect(summary()).toBe("Routing: 1 → 2 → 3")
    expect(history().cursor).toBe(cursor + 1)

    // Already in series: the button has nothing to change.
    await user.click(screen.getByRole("button", { name: "Series" }))
    await flush()
    expect(history().cursor).toBe(cursor + 1)

    await undo()
    await flush()
    expect(bank(effect)).toEqual(parallel)
    expect(summary()).toBe("Routing: 1, 2, 3")
    await redo()
    await flush()

    await user.click(screen.getByRole("button", { name: "Parallel" }))
    await flush()
    expect(bank(effect)).toEqual(parallel)
    expect(history().cursor).toBe(cursor + 2)
  })

  it("keeps a linked routing through save and reopen", async () => {
    const user = userEvent.setup()
    const effect = await open("echoBank")
    await set("echoBank", effect, "units.1.enabled", 1)
    await user.click(screen.getByRole("button", { name: "Series" }))
    await flush()
    const saved = structuredClone(bank(effect))
    const path = await backend.projectSave("/projects/echo-chain.windfall")
    await act(async () => {
      await backend.projectOpen(path)
    })
    await flush()
    expect(bank(effect)).toEqual(saved)
    expect(bank(effect).units[0].nextSend).toBe(1)
    expect(bank(effect).units[1].inputGain).toBe(0)
  })
})

describe("the frequency delay's editor", () => {
  const delay = (effect: EffectId) => paramsOf<FrequencyDelayParams>(effect)
  const pointer = { pointerId: 1, pointerType: "mouse", button: 0 }
  const bars = () =>
    screen
      .getAllByRole("slider")
      .filter((item) => item.dataset.slot === "band-bar")

  /** The plot, measured as 160 by 100 pixels: ten pixels to a band. */
  function plot(): HTMLElement {
    const element = document.querySelector<HTMLElement>(
      '[data-slot="band-bars"]'
    )
    if (!element) throw new Error("The band plot is not showing")
    vi.spyOn(element, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      right: 160,
      bottom: 100,
      width: 160,
      height: 100,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    })
    return element
  }

  /** The pointer position at the middle of a band's column, `share` up. */
  const at = (band: number, share: number) => ({
    clientX: band * 10 + 5,
    clientY: 100 - share * 100,
  })

  function stroke(points: { clientX: number; clientY: number }[]) {
    const element = plot()
    fireEvent.pointerDown(element, { ...pointer, ...points[0] })
    for (const point of points.slice(1)) {
      fireEvent.pointerMove(element, { ...pointer, ...point })
    }
    fireEvent.pointerUp(element, { ...pointer, ...points[points.length - 1] })
  }

  it("draws a bar for each of the sixteen bands", async () => {
    await open("frequencyDelay")
    expect(
      screen.getByRole("group", { name: "Frequency delay settings" })
    ).toBeVisible()
    expect(
      screen.getByRole("group", { name: "Delay of the sixteen bands" })
    ).toBeVisible()
    expect(bars().map((bar) => bar.getAttribute("aria-label"))).toEqual(
      Array.from({ length: 16 }, (_, band) => `Band ${band + 1} delay`)
    )
    for (const bar of bars()) {
      expect(bar).toHaveAttribute("aria-valuemin", "0")
      expect(bar).toHaveAttribute("aria-valuemax", "1000")
      expect(bar).toHaveAttribute("aria-valuenow", "0")
    }
  })

  it("sets every band a stroke passes, as one undo step", async () => {
    const effect = await open("frequencyDelay")
    const cursor = history().cursor

    // From band 1 a quarter of the way up straight to band 5 half way up,
    // in one move: the three bands between are filled in on the line.
    stroke([at(0, 0.25), at(4, 0.5)])
    await flush()

    expect(delay(effect).bands.map((band) => band.delayMs)).toEqual([
      250, 312.5, 375, 437.5, 500, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ])
    expect(delay(effect).bands.every((band) => band.level === 1)).toBe(true)
    expect(history().cursor).toBe(cursor + 1)
    expect(bars()[2]).toHaveAttribute("aria-valuenow", "375")

    await undo()
    await flush()
    expect(delay(effect).bands.every((band) => band.delayMs === 0)).toBe(true)
    expect(history().cursor).toBe(cursor)
    await redo()
    await flush()
    expect(delay(effect).bands[4].delayMs).toBe(500)
  })

  it("draws back and forth, and along the edge when the pointer leaves", async () => {
    const effect = await open("frequencyDelay")
    stroke([
      at(15, 1),
      at(13, 0.5),
      { clientX: 500, clientY: -40 },
      { clientX: 145, clientY: 400 },
    ])
    await flush()
    const bands = delay(effect).bands.map((band) => band.delayMs)
    // Out past the top right the stroke holds the last band at the top.
    expect(bands[15]).toBe(1000)
    // The last point is below the plot in the band before it.
    expect(bands[14]).toBe(0)
    expect(bands[13]).toBe(500)
    expect(bands.slice(0, 13).every((ms) => ms === 0)).toBe(true)
  })

  it("draws level and pan on the same bars", async () => {
    const user = userEvent.setup()
    const effect = await open("frequencyDelay")
    const lanes = within(
      screen.getByRole("radiogroup", { name: "Band setting to draw" })
    )
    expect(lanes.getByRole("radio", { name: "Delay" })).toHaveAttribute(
      "aria-checked",
      "true"
    )

    await user.click(lanes.getByRole("radio", { name: "Level" }))
    expect(bars()[0]).toHaveAttribute("aria-label", "Band 1 level")
    expect(bars()[0]).toHaveAttribute("aria-valuenow", "1")
    stroke([at(2, 0.5), at(3, 0.25)])
    await flush()
    expect(delay(effect).bands[2].level).toBe(0.5)
    expect(delay(effect).bands[3].level).toBe(0.25)
    expect(delay(effect).bands[2].delayMs).toBe(0)

    await user.click(lanes.getByRole("radio", { name: "Pan" }))
    expect(bars()[7]).toHaveAttribute("aria-label", "Band 8 pan")
    expect(bars()[7]).toHaveAttribute("aria-valuemin", "-1")
    // The middle of the plot is the center; three quarters up is half right.
    stroke([at(7, 0.75)])
    stroke([at(8, 0)])
    await flush()
    expect(delay(effect).bands[7].pan).toBe(0.5)
    expect(delay(effect).bands[8].pan).toBe(-1)
    expect(delay(effect).bands[2].level).toBe(0.5)
  })

  it("moves one band with the keyboard and resets it on a double-click", async () => {
    const effect = await open("frequencyDelay")
    const bar = () => screen.getByRole("slider", { name: "Band 3 delay" })
    const cursor = history().cursor
    act(() => bar().focus())
    fireEvent.keyDown(bar(), { key: "ArrowUp" })
    await flush()
    expect(delay(effect).bands[2].delayMs).toBe(10)
    fireEvent.keyDown(bar(), { key: "PageUp" })
    await flush()
    expect(delay(effect).bands[2].delayMs).toBe(110)
    fireEvent.keyDown(bar(), { key: "End" })
    await flush()
    expect(delay(effect).bands[2].delayMs).toBe(1000)
    // Each key press is a step of its own.
    expect(history().cursor).toBe(cursor + 3)

    fireEvent.keyDown(bar(), { key: "ArrowRight" })
    expect(screen.getByRole("slider", { name: "Band 4 delay" })).toHaveFocus()

    fireEvent.doubleClick(bar())
    await flush()
    expect(delay(effect).bands[2].delayMs).toBe(0)
  })

  it("switches a band off with its light, and shows it on the bar", async () => {
    const user = userEvent.setup()
    const effect = await open("frequencyDelay")
    expect(bars()[4]).not.toHaveAttribute("data-off")
    await user.click(screen.getByRole("button", { name: "Band 5 enabled" }))
    await flush()
    expect(delay(effect).bands[4].enabled).toBe(false)
    expect(bars()[4]).toHaveAttribute("data-off")
    expect(delay(effect).bands[3].enabled).toBe(true)
  })

  it("reads out the longest echo that will be heard", async () => {
    const user = userEvent.setup()
    const effect = await open("frequencyDelay")
    const longest = () =>
      document.querySelector('[data-slot="frequency-delay-longest"]')
        ?.textContent ?? ""
    await set("frequencyDelay", effect, "bands.0.delayMs", 400)
    await set("frequencyDelay", effect, "bands.9.delayMs", 700)
    expect(longest()).toContain("700")
    // A band that is off plays undelayed and does not count.
    await user.click(screen.getByRole("button", { name: "Band 10 enabled" }))
    await flush()
    expect(longest()).toContain("400")
    await user.click(screen.getByRole("button", { name: "Short range" }))
    await flush()
    expect(delay(effect).shortRange).toBe(true)
    expect(longest()).toContain("40")
    expect(longest()).not.toContain("400")
  })

  it("keeps a drawn shape through save and reopen", async () => {
    const effect = await open("frequencyDelay")
    stroke([at(0, 0.1), at(15, 0.9)])
    await flush()
    const saved = structuredClone(delay(effect))
    expect(saved.bands[0].delayMs).toBeCloseTo(100, 3)
    expect(saved.bands[15].delayMs).toBeCloseTo(900, 3)
    expect(saved.bands[5].delayMs).toBeGreaterThan(saved.bands[4].delayMs)
    const path = await backend.projectSave("/projects/drawn-bands.windfall")
    await act(async () => {
      await backend.projectOpen(path)
    })
    await flush()
    expect(delay(effect)).toEqual(saved)
  })
})
