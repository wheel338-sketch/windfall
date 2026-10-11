import {
  act,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EffectId, EffectKind, EffectSlot, TrackId } from "@/bindings"
import { formatDb, formatHz } from "@/components/audio"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor, paramIndex, readParam } from "@/features/params"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import { customEditor, EffectEditor } from "./effect-editor"
import { curveGainId } from "./graphic/morph-eq"
import { SEVEN_BAND_HZ, SEVEN_BAND_IDS } from "./graphic/seven-band"
import { describeOffset } from "./stereo/stereo"

/*
 * The purpose-built editors of the equalizer, filter and stereo effects,
 * mounted over the real project document (the Rust core compiled to WASM).
 * Only the canvas is missing, as it is everywhere in jsdom, so what a
 * display shows is read from its accessible description.
 */

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void
/** The track the effect under test is on: the first insert. */
let track: TrackId

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  ;({ stop } = await startTestApp())
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

/** The stored value of one setting, as `setEffectParam` numbers it. */
function stored(effect: EffectId, kind: EffectKind, id: string): number {
  const descriptor = effectDescriptor(kind)
  return readParam(
    slotOf(effect).params,
    descriptor.params[paramIndex(descriptor, id)]
  )
}

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

/** Takes a slider to the end of its range with the keyboard: one gesture. */
async function toEnd(slider: HTMLElement, key: "Home" | "End" = "End") {
  act(() => slider.focus())
  fireEvent.keyDown(slider, { key })
  fireEvent.keyUp(slider, { key })
  await flush()
}

/**
 * Every setting in `ids` has a slider in the editor that covers its range,
 * and moving it is one undo step on the setting the descriptor numbers.
 */
async function expectSliders(
  kind: EffectKind,
  effect: EffectId,
  ids: readonly string[]
) {
  const descriptor = effectDescriptor(kind)
  for (const id of ids) {
    const info = descriptor.params[paramIndex(descriptor, id)]
    const slider = screen.getByRole("slider", { name: info.name })
    expect(slider, id).toHaveAttribute("aria-valuemin", String(info.min))
    expect(slider, id).toHaveAttribute("aria-valuemax", String(info.max))
    const before = structuredClone(slotOf(effect).params)
    const cursor = history().cursor
    const up = info.default !== info.max
    await toEnd(slider, up ? "End" : "Home")
    expect(stored(effect, kind, id), id).toBe(up ? info.max : info.min)
    expect(history().cursor, id).toBe(cursor + 1)
    await undo()
    await flush()
    expect(slotOf(effect).params, id).toEqual(before)
    await redo()
    await flush()
    expect(stored(effect, kind, id), id).toBe(up ? info.max : info.min)
    await undo()
    await flush()
  }
}

describe("Seven Band", () => {
  it("has its own editor: seven fixed-frequency faders and nothing else to set", async () => {
    expect(customEditor("sevenBand")).toBeDefined()
    const effect = await open("sevenBand")
    expect(
      screen.getByRole("group", { name: "Seven Band settings" })
    ).toBeVisible()
    expect(screen.getAllByRole("slider")).toHaveLength(7)
    for (const slider of screen.getAllByRole("slider")) {
      expect(slider).toHaveAttribute("aria-orientation", "vertical")
    }
    await expectSliders("sevenBand", effect, SEVEN_BAND_IDS)
  })

  it("draws the curve the faders make and says what it is", async () => {
    const effect = await open("sevenBand")
    expect(
      screen.getByRole("img", { name: "Seven Band response: flat" })
    ).toBeVisible()
    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: 3,
      value: 6,
    })
    await flush()
    expect(
      screen.getByRole("img", {
        name: `Seven Band response: ${formatHz(SEVEN_BAND_HZ[3])} ${formatDb(6, 1)}`,
      })
    ).toBeVisible()
  })

  it("puts every band back to 0 dB as one undo step", async () => {
    const user = userEvent.setup()
    const effect = await open("sevenBand")
    const flat = screen.getByRole("button", { name: "Flat" })
    expect(flat).toBeDisabled()
    for (const [param, value] of [
      [0, 9],
      [4, -12],
      [6, 3],
    ]) {
      await dispatch({ type: "setEffectParam", track, effect, param, value })
    }
    await flush()
    const shaped = structuredClone(slotOf(effect).params)
    const cursor = history().cursor
    await user.click(flat)
    await flush()
    expect(slotOf(effect).params).toEqual(
      effectDescriptor("sevenBand").defaults
    )
    expect(history().cursor).toBe(cursor + 1)
    await undo()
    await flush()
    expect(slotOf(effect).params).toEqual(shaped)
  })
})

/** Sets one setting by id, as its own undo step. */
async function setParam(
  kind: EffectKind,
  effect: EffectId,
  id: string,
  value: number
) {
  await dispatch({
    type: "setEffectParam",
    track,
    effect,
    param: paramIndex(effectDescriptor(kind), id),
    value,
  })
  await flush()
}

/** Picks an option of a setting shown as a list or a strip of buttons. */
async function choose(kind: EffectKind, id: string, label: string) {
  const user = userEvent.setup()
  const root = document.querySelector<HTMLElement>(`[data-param="${id}"]`)
  if (!root) throw new Error(`No control is showing for "${id}" of ${kind}`)
  const radio = within(root).queryByRole("radio", { name: label })
  if (radio) {
    await user.click(radio)
  } else {

    const combobox = root.querySelector<HTMLElement>("[role=combobox]")
    if (!combobox) throw new Error(`"${id}" has no list of options`)
    await user.click(combobox)
    await user.click(await screen.findByRole("option", { name: label }))
  }
  await flush()
}

describe("Spreader", () => {
  it("has its own editor with every control and the mono promise", async () => {
    expect(customEditor("spreader")).toBeDefined()
    const effect = await open("spreader")
    expect(
      screen.getByRole("group", { name: "Spreader settings" })
    ).toBeVisible()
    expect(screen.getByText(/Mono playback hears the original/)).toBeVisible()
    await expectSliders("spreader", effect, [
      "width",
      "mix",
      "haasMs",
      "amount",
    ])
  })

  it("draws the width it is set to", async () => {
    const effect = await open("spreader")
    expect(
      screen.getByRole("img", {
        name: "Stereo image at 120% of its width. The mono sum is unchanged",
      })
    ).toBeVisible()
    await setParam("spreader", effect, "width", 0)
    const field = screen.getByRole("img", {
      name: "Stereo image at 0% of its width. The mono sum is unchanged",
    })
    // Mono is a line in the middle.
    expect(
      field.querySelector<HTMLElement>("[data-slot=stereo-field-image]")?.style
        .left
    ).toBe("50%")
  })
})

describe("Stereo Enhancer", () => {
  it("has its own editor with width, balance, phase and bass controls", async () => {
    expect(customEditor("stereoEnhancer")).toBeDefined()
    const effect = await open("stereoEnhancer")
    expect(
      screen.getByRole("group", { name: "Stereo Enhancer settings" })
    ).toBeVisible()
    await expectSliders("stereoEnhancer", effect, [
      "width",
      "balance",
      "phaseOffsetMs",
      "bassHz",
      "bassLock",
      "mix",
    ])
  })

  it("says which side the phase offset holds back", async () => {
    const effect = await open("stereoEnhancer")
    const readout = () =>
      document.querySelector("[data-slot=phase-readout]")?.textContent
    expect(readout()).toBe("Both sides in time")
    await setParam("stereoEnhancer", effect, "phaseOffsetMs", 5)
    expect(readout()).toBe(describeOffset(5))
    expect(readout()).toMatch(/^Right side .* late$/)
    await setParam("stereoEnhancer", effect, "phaseOffsetMs", -12)
    expect(readout()).toMatch(/^Left side .* late$/)
  })

  it("inverts one side from its own control, as one undo step", async () => {
    const effect = await open("stereoEnhancer")
    const cursor = history().cursor
    await choose("stereoEnhancer", "invert", "Left")
    expect(slotOf(effect).params).toMatchObject({ invert: "left" })
    expect(history().cursor).toBe(cursor + 1)
    await choose("stereoEnhancer", "invert", "Right")
    expect(slotOf(effect).params).toMatchObject({ invert: "right" })
    await undo()
    await undo()
    await flush()
    expect(slotOf(effect).params).toMatchObject({ invert: "off" })
  })
})

describe("Pan Motion", () => {
  it("has its own editor with pan, volume and motion controls", async () => {
    expect(customEditor("panLfo")).toBeDefined()
    const effect = await open("panLfo")
    expect(
      screen.getByRole("group", { name: "Pan Motion settings" })
    ).toBeVisible()
    await expectSliders("panLfo", effect, [
      "rateHz",
      "pan",
      "depth",
      "gain",
      "volumeDepth",
    ])
  })

  it("swaps the rate for beats when it follows the tempo", async () => {
    const user = userEvent.setup()
    const effect = await open("panLfo")
    expect(screen.queryByRole("slider", { name: "Cycle" })).toBeNull()
    await user.click(screen.getByRole("switch", { name: "Sync to tempo" }))
    await flush()
    expect(slotOf(effect).params).toMatchObject({ sync: true })
    expect(screen.queryByRole("slider", { name: "Rate" })).toBeNull()
    await expectSliders("panLfo", effect, ["beats"])
  })

  it("describes the motion it will make, shape and reach", async () => {
    const effect = await open("panLfo")
    const label = () =>
      document
        .querySelector("[data-slot=motion-display]")
        ?.getAttribute("aria-label")
    expect(label()).toMatch(/^Pan moves between L100 and R100 as a sine/)
    await choose("panLfo", "shape", "Square")
    expect(slotOf(effect).params).toMatchObject({ shape: "square" })
    await setParam("panLfo", effect, "depth", 0.5)
    await setParam("panLfo", effect, "pan", 0.5)
    expect(label()).toMatch(/^Pan moves between C and R100 as a square/)
  })
})

describe("Morph EQ", () => {
  const label = () =>
    document
      .querySelector("[data-slot=response-display]")
      ?.getAttribute("aria-label")

  it("has its own editor: a graphic curve, the morph and the curve count", async () => {
    expect(customEditor("morphEq")).toBeDefined()
    const effect = await open("morphEq")
    expect(
      screen.getByRole("group", { name: "Morph EQ settings" })
    ).toBeVisible()
    const faders = within(
      screen.getByRole("group", { name: "Curve A" })
    ).getAllByRole("slider")
    expect(faders).toHaveLength(15)
    await expectSliders("morphEq", effect, [
      "morph",
      "curveCount",
      curveGainId(0, 0),
      curveGainId(0, 14),
    ])
  })

  it("edits the stored curve that is selected, and only offers the curves in use", async () => {
    const user = userEvent.setup()
    const effect = await open("morphEq")
    expect(screen.getByRole("button", { name: "Curve A" })).toHaveAttribute(
      "aria-pressed",
      "true"
    )
    expect(screen.getByRole("button", { name: "Curve C" })).toBeDisabled()
    await user.click(screen.getByRole("button", { name: "Curve B" }))
    expect(
      within(screen.getByRole("group", { name: "Curve B" })).getAllByRole(
        "slider"
      )
    ).toHaveLength(15)
    await expectSliders("morphEq", effect, [curveGainId(1, 8)])
    await setParam("morphEq", effect, "curveCount", 4)
    expect(screen.getByRole("button", { name: "Curve C" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Curve D" }))
    await expectSliders("morphEq", effect, [curveGainId(3, 2)])
  })

  it("shows where the morph is and what is heard there", async () => {
    const effect = await open("morphEq")
    expect(label()).toBe("Morph EQ response at Curve A: flat")
    await setParam("morphEq", effect, curveGainId(0, 8), 12)
    await setParam("morphEq", effect, "morph", 0.5)
    expect(label()).toBe(
      `Morph EQ response at 50% from A to B: ${formatHz(1_000)} ${formatDb(6, 1)}`
    )
    await setParam("morphEq", effect, "morph", 1)
    expect(label()).toBe("Morph EQ response at Curve B: flat")
  })

  it("stores what is heard into the selected curve, and flattens it, each as one undo step", async () => {
    const user = userEvent.setup()
    const effect = await open("morphEq")
    await setParam("morphEq", effect, curveGainId(0, 8), 12)
    await setParam("morphEq", effect, curveGainId(0, 3), -8)
    await setParam("morphEq", effect, "morph", 0.5)
    await user.click(screen.getByRole("button", { name: "Curve B" }))
    const before = structuredClone(slotOf(effect).params)
    const cursor = history().cursor
    await user.click(
      screen.getByRole("button", { name: "Store what is heard in B" })
    )
    await flush()
    expect(stored(effect, "morphEq", curveGainId(1, 8))).toBe(6)
    expect(stored(effect, "morphEq", curveGainId(1, 3))).toBe(-4)
    // Curve A is as it was.
    expect(stored(effect, "morphEq", curveGainId(0, 8))).toBe(12)
    expect(history().cursor).toBe(cursor + 1)
    await user.click(screen.getByRole("button", { name: "Flat" }))
    await flush()
    expect(stored(effect, "morphEq", curveGainId(1, 8))).toBe(0)
    expect(history().cursor).toBe(cursor + 2)
    await undo()
    await undo()
    await flush()
    expect(slotOf(effect).params).toEqual(before)
  })

  it("keeps the movable bells behind a switch, each on its own setting", async () => {
    const user = userEvent.setup()
    const effect = await open("morphEq")
    expect(
      screen.queryByRole("slider", { name: "A Band 1 Frequency" })
    ).toBeNull()
    await user.click(screen.getByRole("button", { name: "Movable bells" }))
    await expectSliders("morphEq", effect, [
      "snapshotA.bands.0.frequencyHz",
      "snapshotA.bands.3.gainDb",
      "snapshotB.bands.2.q",
    ])
  })
})
describe("Band Split", () => {
  it("shows the crossovers, band gains and the actual two/three-band layout", async () => {
    const user = userEvent.setup()
    const effect = await open("bandSplit")
    expect(customEditor("bandSplit")).toBeDefined()
    expect(screen.getByRole("group", { name: "Band Split settings" })).toBeVisible()
    await expectSliders("bandSplit", effect, ["lowCrossoverHz", "highCrossoverHz", "lowGainDb", "midGainDb", "highGainDb"])
    const choices = document.querySelector('[data-param="bands"]')!
    await user.click(within(choices as HTMLElement).getByRole("radio", { name: "Two" }))
    await flush()
    expect(stored(effect, "bandSplit", "bands")).toBe(0)
    expect(screen.queryByRole("slider", { name: "Mid gain" })).toBeNull()
    expect(screen.queryByRole("slider", { name: "High crossover" })).toBeNull()
    expect(screen.getByRole("img", { name: /Band Split response: Low below/ })).toBeVisible()
    await undo()
    await flush()
    expect(screen.getByRole("slider", { name: "Mid gain" })).toBeVisible()
  })
})

describe("Resonant filter", () => {
  it("has a sweep editor with automatable frequency and resonance and a response", async () => {
    const effect = await open("selectableFilter")
    expect(customEditor("selectableFilter")).toBeDefined()
    expect(screen.getByRole("img", { name: /Resonant filter: Lowpass/ })).toBeVisible()
    await expectSliders("selectableFilter", effect, ["frequencyHz", "q", "gainDb"])
  })
})

describe("Seven movable EQ bands", () => {
  it("exposes seven independently positioned nodes with a distinct parametric editor", async () => {
    const effect = await open("eq")
    const curve = screen.getByRole("group", { name: "Equaliser curve" })
    expect(within(curve).getAllByRole("slider")).toHaveLength(7)
    const before = structuredClone(slotOf(effect).params)
    const cursor = history().cursor
    const node = within(curve).getByRole("slider", { name: "Peak 2" })
    fireEvent.keyDown(node, { key: "ArrowRight" })
    fireEvent.keyUp(node, { key: "ArrowRight" })
    await flush()
    expect(slotOf(effect).params).not.toEqual(before)
    expect(history().cursor).toBe(cursor + 1)
    await undo()
    await flush()
    expect(slotOf(effect).params).toEqual(before)
    await redo()
    await flush()
    expect(slotOf(effect).params).not.toEqual(before)
  })
})

describe("Filter Bank", () => {
  it("exposes eight real units with independently stored drawn gate/cutoff points", async () => {
    const user = userEvent.setup()
    const effect = await open("filterBank")
    expect(customEditor("filterBank")).toBeDefined()
    const tabs = screen.getByRole("tablist", { name: "Filter units" })
    expect(within(tabs).getAllByRole("tab")).toHaveLength(8)
    for (let unit = 0; unit < 8; unit += 1) {
      await user.click(within(tabs).getByRole("tab", { name: `Unit ${unit + 1}` }))
      await flush()
      expect(screen.getByRole("img", { name: new RegExp(`Unit ${unit + 1} gate envelope:`) })).toBeVisible()
      await expectSliders("filterBank", effect, [`patterns.${unit}.gate.0`, `patterns.${unit}.cutoff.7`])
    }
  })

  it("draws a whole stroke as one undo step and restores it through redo", async () => {
    const effect = await open("filterBank")
    const graph = screen.getByRole("img", { name: /Unit 1 gate envelope:/ })
    vi.spyOn(graph, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 320, bottom: 100, width: 320, height: 100, toJSON: () => ({}),
    })
    const before = structuredClone(slotOf(effect).params)
    const cursor = history().cursor
    fireEvent.pointerDown(graph, { pointerId: 1, button: 0, clientX: 10, clientY: 90 })
    fireEvent.pointerMove(graph, { pointerId: 1, clientX: 290, clientY: 50 })
    fireEvent.pointerUp(graph, { pointerId: 1 })
    await flush()
    expect(stored(effect, "filterBank", "patterns.0.gate.0")).toBe(0)
    expect(stored(effect, "filterBank", "patterns.0.gate.7")).toBeCloseTo(0.5)
    expect(history().cursor).toBe(cursor + 1)
    await undo()
    await flush()
    expect(slotOf(effect).params).toEqual(before)
    await redo()
    await flush()
    expect(stored(effect, "filterBank", "patterns.0.gate.0")).toBe(0)
  })
})
