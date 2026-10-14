import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type {
  Command,
  CompressorParams,
  DelayParams,
  EffectId,
  EffectKind,
  EffectParams,
  EffectSlot,
  EqParams,
  LimiterParams,
  ParamInfo,
  TrackId,
} from "@/bindings"
import {
  effectDescriptor,
  paramIndex,
  type ParamDescriptor,
} from "@/features/params"
import type { MockBackend } from "@/lib/ipc/mock"
import { automationBlocked } from "@/lib/automation/targets"
import { dispatch, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"
import { ValueContextMenus } from "@/components/value-context-menu"

import { customEditor, EffectEditor } from "./effect-editor"
import { EQ_BANDS } from "./eq/bands"
import { useEqView } from "./eq/eq-view"
import { frequencyToX, gainToY, plotRect } from "./eq/geometry"
import { historyLabelText } from "@/lib/i18n"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

let stop: () => void
let backend: MockBackend
/** The track the effect under test is on: the first insert. */
let track: TrackId

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  useEqView.setState(useEqView.getInitialState(), true)
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

/** Every command sent since `from`, oldest first. */
function sent(from = 0): Command[] {
  return vi
    .mocked(backend.dispatch)
    .mock.calls.slice(from)
    .map(([command]) => command)
}

function control(id: string): HTMLElement {
  const found = document.querySelector<HTMLElement>(`[data-param="${id}"]`)
  if (!found) throw new Error(`No control is showing for "${id}"`)
  return found
}

/**
 * Changes a setting through its control, whatever kind of control it is,
 * and returns the commands that sent.
 */
async function operate(info: ParamInfo, value: number): Promise<Command[]> {
  const user = userEvent.setup()
  const before = vi.mocked(backend.dispatch).mock.calls.length
  const root = control(info.id)
  const slider = root.matches("[role=slider]")
    ? root
    : root.querySelector<HTMLElement>("[role=slider]")
  if (slider) {
    const key = value >= info.max ? "ArrowDown" : "ArrowUp"
    act(() => slider.focus())
    fireEvent.keyDown(slider, { key })
    fireEvent.keyUp(slider, { key })
  } else if (info.kind === "toggle") {
    const button = root.querySelector<HTMLElement>(
      "[role=switch], button[aria-pressed]"
    )
    if (!button) throw new Error(`"${info.id}" has no switch`)
    await user.click(button)
  } else {
    const other = info.choices[value === 0 ? 1 : 0].label
    const radios = within(root).queryAllByRole("radio")
    if (radios.length > 0) {
      await user.click(within(root).getByRole("radio", { name: other }))
    } else {
      await user.click(within(root).getByRole("combobox"))
      await user.click(await screen.findByRole("option", { name: other }))
    }
  }
  await flush()
  return sent(before)
}

/**
 * Operates the control of every setting in `ids` and checks that each one
 * sends `setEffectParam` with the index its descriptor gives the setting.
 */
async function expectControls(
  kind: EffectKind,
  effect: EffectId,
  ids: string[]
) {
  const descriptor = effectDescriptor(kind)
  for (const id of ids) {
    const index = paramIndex(descriptor, id)
    const info = descriptor.params[index]
    const stored = slotOf(effect).params as unknown as Record<string, unknown>
    const current = id
      .split(".")
      .reduce<unknown>(
        (at, key) => (at as Record<string, unknown>)[key],
        stored
      )
    const value =
      typeof current === "number"
        ? current
        : typeof current === "boolean"
          ? Number(current)
          : info.choices.findIndex((choice) => choice.value === current)
    const commands = await operate(info, value)
    expect(commands.length, id).toBeGreaterThan(0)
    for (const command of commands) {
      expect(command, id).toMatchObject({
        type: "setEffectParam",
        track,
        effect,
        param: index,
      })
    }
  }
}

const allIds = (kind: EffectKind) =>
  effectDescriptor(kind).params.map((info) => info.id)

const utilities = [
  "balance",
  "dcBlock",
  "channelMute",
  "polarity",
  "stereoMatrix",
  "softClipper",
  "distortion",
] as const

describe("measured utility editors", () => {
  beforeEach(() => {
    vi.spyOn(backend, "dispatch")
  })

  it.each(["ll", "lr", "rl", "rr"] as const)(
    "types and displays a negative matrix %s coefficient as one gesture",
    async (id) => {
      const effect = await open("stereoMatrix")
      const info = effectDescriptor("stereoMatrix").params.find(
        (info) => info.id === id
      )!
      const slider = screen.getByRole("slider", { name: info.name })
      const original = slotOf(effect).params
      const before = history().cursor
      fireEvent.keyDown(slider, { key: "Enter" })
      const input = screen.getByRole("textbox", { name: info.name })
      fireEvent.change(input, { target: { value: "-1" } })
      fireEvent.keyDown(input, { key: "Enter" })
      await flush()
      const params = slotOf(effect).params
      if (params.type !== "stereoMatrix") throw new Error("wrong effect")
      expect(params[id]).toBe(-1)
      expect(slider).toHaveAttribute("aria-valuetext", "−1.00")
      expect(history().cursor).toBe(before + 1)
      await undo()
      await flush()
      expect(slotOf(effect).params).toEqual(original)
    }
  )

  it("shows the existing host boundary for matrix delay automation", async () => {
    const effect = await open("stereoMatrix")
    const project = useProjectStore.getState().project
    const descriptor = effectDescriptor("stereoMatrix")
    for (const info of descriptor.params) {
      const target = {
        type: "effectParam",
        track,
        effect,
        param: paramIndex(descriptor, info.id),
      } as const
      expect(automationBlocked(project, target)).toBe(
        info.id.endsWith("DelayMs") ? "Changes the latency" : null
      )
    }
    const slider = screen.getByRole("slider", { name: "Left delay" })
    fireEvent.contextMenu(slider, { clientX: 20, clientY: 20 })
    const item = await screen.findByRole("menuitem", {
      name: /Create automation clip/,
    })
    expect(item).toHaveAttribute("aria-disabled", "true")
    expect(item).toHaveTextContent("Changes the latency")
  })

  it.each(utilities)(
    "maps every %s descriptor to a real document control",
    async (kind) => {
      const effect = await open(kind)
      expect(customEditor(kind)).toBeUndefined()
      expect(
        screen.getByRole("group", {
          name: `${effectDescriptor(kind).name} settings`,
        })
      ).toBeVisible()
      await expectControls(kind, effect, allIds(kind))
    }
  )

  it.each(utilities)(
    "makes a %s control gesture one undo step",
    async (kind) => {
      const effect = await open(kind)
      const descriptor = effectDescriptor(kind)
      const index = descriptor.params.findIndex((info) => info.kind === "float")
      const info = descriptor.params[Math.max(0, index)]
      const original = slotOf(effect).params
      const before = history().cursor
      if (info.kind === "float") {
        const slider =
          within(control(info.id)).queryByRole("slider") ?? control(info.id)
        act(() => slider.focus())
        for (let step = 0; step < 3; step += 1) {
          fireEvent.keyDown(slider, {
            key: info.default === info.max ? "ArrowDown" : "ArrowUp",
          })
          await flush()
        }
        fireEvent.keyUp(slider, {
          key: info.default === info.max ? "ArrowDown" : "ArrowUp",
        })
        await flush()
      } else {
        await operate(info, 0)
      }
      expect(slotOf(effect).params).not.toEqual(original)
      expect(history().cursor).toBe(before + 1)
      await undo()
      await flush()
      expect(slotOf(effect).params).toEqual(original)
    }
  )

  it.each(utilities)(
    "creates %s automation from its generic control menu",
    async (kind) => {
      const effect = await open(kind)
      const info = effectDescriptor(kind).params[0]
      const root = control(info.id)
      const target = root.matches("[role=slider]")
        ? root
        : (root.querySelector<HTMLElement>(
            "[role=slider], [role=switch], button"
          ) ?? root)
      const before = history().cursor
      fireEvent.contextMenu(target, { clientX: 20, clientY: 20 })
      const item = await screen.findByRole("menuitem", {
        name: "Create automation clip",
      })
      await userEvent.setup().click(item)
      await flush()
      expect(
        useProjectStore.getState().project.automations.at(-1)?.target
      ).toEqual({ type: "effectParam", track, effect, param: 0 })
      expect(history().cursor).toBe(before + 1)
      await undo()
      await flush()
      expect(useProjectStore.getState().project.automations).toEqual([])
    }
  )
})

describe("the editor of each kind", () => {
  beforeEach(() => {
    vi.spyOn(backend, "dispatch")
  })

  it("has a purpose-built editor for the original five kinds", () => {
    for (const kind of ["eq", "compressor", "limiter", "reverb", "delay"]) {
      expect(customEditor(kind), kind).toBeDefined()
    }
    expect(customEditor("somethingNew")).toBeUndefined()
  })

  it("maps every compressor control to its setting", async () => {
    const effect = await open("compressor")
    await expectControls("compressor", effect, allIds("compressor"))
  })

  it("maps every limiter control to its setting", async () => {
    const effect = await open("limiter")
    await expectControls("limiter", effect, allIds("limiter"))
  })

  it("maps every reverb control to its setting", async () => {
    const effect = await open("reverb")
    await expectControls("reverb", effect, allIds("reverb"))
  })

  it("maps every delay control to its setting, synced or not", async () => {
    const effect = await open("delay")
    const ids = allIds("delay")
    // A new delay follows the tempo, so it shows the note length.
    expect(paramsOf<DelayParams>(effect).sync).toBe(true)
    expect(document.querySelector('[data-param="timeMs"]')).toBeNull()
    await expectControls(
      "delay",
      effect,
      ids.filter((id) => id !== "timeMs" && id !== "sync")
    )
    await expectControls("delay", effect, ["sync"])
    expect(paramsOf<DelayParams>(effect).sync).toBe(false)
    expect(document.querySelector('[data-param="division"]')).toBeNull()
    await expectControls("delay", effect, ["timeMs"])
  })

  it("maps every equaliser control to its setting, band by band", async () => {
    const effect = await open("eq")
    const covered: string[] = []
    for (const band of EQ_BANDS) {
      fireEvent.click(
        within(screen.getByRole("radiogroup", { name: "Band" })).getByRole(
          "radio",
          { name: band.name }
        )
      )
      expect(screen.getByRole("region", { name: band.name })).toBeVisible()
      const ids = [
        band.enabled,
        band.frequency,
        band.gain,
        band.slope,
        band.q,
      ].filter((id): id is string => id !== null)
      await expectControls("eq", effect, ids)
      covered.push(...ids)
    }
    await expectControls("eq", effect, ["outputGainDb"])
    covered.push("outputGainDb")
    expect(covered.sort()).toEqual(allIds("eq").sort())
  })
})

describe("the editors docked beside the mixer's strips", () => {
  /*
   * jsdom lays nothing out, so these hold the editors to the layout that
   * was measured in a browser: in the 460 by 246 pixel dock of a 1440 by
   * 900 window the reverb is 211 pixels tall, the delay 239 and the
   * equaliser 244, where they were 323, 283 and 252 and had to be scrolled.
   */
  const DOCKED = "@min-[26rem]/editor:"
  // With the docked rule's own container query, so it is the later rule.
  const ROOMY = "in-data-enlarged:@min-[26rem]/editor:"
  const classesOf = (element: Element | null | undefined) =>
    (element?.getAttribute("class") ?? "").split(/\s+/)

  it("stands the reverb's groups side by side, as wide as their knobs", async () => {
    await open("reverb")
    const root = document.querySelector("[data-slot=reverb-editor]")
    expect(classesOf(root)).toContain(`${DOCKED}grid-cols-[3fr_2fr_1.15fr]`)
    // Enlarged, the groups are equally wide again and wrap.
    expect(classesOf(root)).toContain(
      `${ROOMY}grid-cols-[repeat(auto-fit,minmax(11.5rem,1fr))]`
    )
    const rows = [...(root?.querySelectorAll("[data-slot=param-row]") ?? [])]
    // Three, two and one knob across; three across everywhere enlarged.
    expect(
      rows.map((row) =>
        classesOf(row).filter((name) => name.startsWith(DOCKED))
      )
    ).toEqual([[], [`${DOCKED}grid-cols-2!`], [`${DOCKED}grid-cols-1!`]])
    for (const row of rows.slice(1)) {
      expect(classesOf(row)).toContain(`${ROOMY}grid-cols-3!`)
    }
  })

  it("puts the delay's Time and Echoes one above the other, with Stereo beside them", async () => {
    await open("delay")
    const root = document.querySelector("[data-slot=delay-editor]")
    expect(classesOf(root)).toContain(`${DOCKED}grid-cols-2`)
    const stereo = within(root as HTMLElement).getByRole("region", {
      name: "Stereo",
    })
    expect(classesOf(stereo)).toEqual(
      expect.arrayContaining([
        `${DOCKED}col-start-2`,
        `${DOCKED}row-start-1`,
        `${DOCKED}row-span-2`,
        `${ROOMY}col-start-auto`,
        `${ROOMY}row-span-1`,
      ])
    )
  })

  it("draws the equaliser's parts closer together docked than enlarged", async () => {
    await open("eq")
    const root = document.querySelector("[data-slot=eq-editor]")
    expect(classesOf(root)).toEqual(
      expect.arrayContaining(["gap-1.5", "in-data-enlarged:gap-2"])
    )
  })
})

describe("the equaliser display", () => {
  // The size the display has until it is measured, which is never in jsdom.
  const plot = plotRect(320, 160)
  const at = (hz: number, db: number, range = 24) => ({
    clientX: frequencyToX(hz, plot),
    clientY: gainToY(db, plot, range),
  })
  const node = (name: string) => screen.getByRole("slider", { name })
  const pointer = { pointerId: 1, pointerType: "mouse", button: 0 }
  const eq = (effect: EffectId) => paramsOf<EqParams>(effect)

  function dragNode(
    name: string,
    from: { clientX: number; clientY: number },
    to: { clientX: number; clientY: number },
    keys: { altKey?: boolean } = {}
  ) {
    const element = node(name)
    fireEvent.pointerDown(element, { ...pointer, ...from })
    for (let step = 1; step <= 6; step += 1) {
      fireEvent.pointerMove(element, {
        ...pointer,
        ...keys,
        clientX: from.clientX + ((to.clientX - from.clientX) * step) / 6,
        clientY: from.clientY + ((to.clientY - from.clientY) * step) / 6,
      })
    }
    fireEvent.pointerUp(element, { ...pointer, ...to })
  }

  it("has a node for every band, hollow for the ones that are off", async () => {
    await open("eq")
    const nodes = screen
      .getAllByRole("slider")
      .filter((item) => item.dataset.slot === "eq-node")
    expect(nodes.map((item) => item.getAttribute("aria-label"))).toEqual(
      EQ_BANDS.map((band) => band.name)
    )
    expect(node("Low cut")).not.toHaveAttribute("data-enabled")
    expect(node("Low cut")).toHaveAttribute(
      "aria-valuetext",
      "Off, 30.0 Hz, Q 0.71"
    )
    expect(node("Peak 2")).toHaveAttribute("data-enabled")
    expect(node("Peak 2")).toHaveAttribute(
      "aria-valuetext",
      "1.00 kHz, 0.0 dB, Q 1.00"
    )
  })

  it("sets frequency and gain with one drag, as one undo step", async () => {
    const effect = await open("eq")
    const before = history().cursor

    dragNode("Peak 2", at(1000, 0), at(2500, 9))
    await flush()

    const band = eq(effect).peak2
    expect(band.frequencyHz).toBeGreaterThan(2400)
    expect(band.frequencyHz).toBeLessThan(2600)
    expect(band.gainDb).toBeCloseTo(9, 0)
    expect(band.q).toBe(1)
    expect(history().cursor).toBe(before + 1)
    // One step, named after the setting the drag moved first.
    expect(historyLabelText(history().entries.at(-1)!.label, "en")).toBe("Change Peak 2 frequency")
    // The node sits where the band is now.
    expect(node("Peak 2")).toHaveAttribute(
      "aria-valuenow",
      String(band.frequencyHz)
    )

    await undo()
    await flush()
    expect(eq(effect).peak2).toMatchObject({ frequencyHz: 1000, gainDb: 0 })
    expect(history().cursor).toBe(before)
  })

  it("keeps a dragged node inside the range the display shows", async () => {
    const effect = await open("eq")
    fireEvent.click(
      screen.getByRole("radio", { name: "Show 12 dB either side of 0" })
    )
    dragNode("Peak 1", at(400, 0, 12), { clientX: -50, clientY: -200 })
    await flush()
    expect(eq(effect).peak1).toMatchObject({ frequencyHz: 20, gainDb: 12 })
  })

  it("moves a cut along the frequency axis only", async () => {
    const effect = await open("eq")
    const start = at(18_000, 0)
    dragNode("High cut", start, at(6000, 12))
    await flush()
    const band = eq(effect).highCut
    expect(band.frequencyHz).toBeGreaterThan(5800)
    expect(band.frequencyHz).toBeLessThan(6200)
    // Dragging the node of a band that is off switches it on, in one step.
    expect(band.enabled).toBe(true)
    expect(history().cursor).toBe(2)
  })

  it("switches a band on with a click on its hollow node", async () => {
    const effect = await open("eq")
    const place = at(30, 0)
    fireEvent.pointerDown(node("Low cut"), { ...pointer, ...place })
    fireEvent.pointerUp(node("Low cut"), { ...pointer, ...place })
    await flush()
    expect(eq(effect).lowCut.enabled).toBe(true)
    expect(eq(effect).lowCut.frequencyHz).toBe(30)
    expect(history().cursor).toBe(2)
    expect(node("Low cut")).toHaveAttribute("data-enabled")

    // A click on a node that is on changes nothing.
    fireEvent.pointerDown(node("Low cut"), { ...pointer, ...place })
    fireEvent.pointerUp(node("Low cut"), { ...pointer, ...place })
    await flush()
    expect(history().cursor).toBe(2)
  })

  it("selects the band whose node is pressed", async () => {
    await open("eq")
    expect(screen.getByRole("region", { name: "Peak 2" })).toBeVisible()
    const place = at(8000, 0)
    fireEvent.pointerDown(node("High shelf"), { ...pointer, ...place })
    fireEvent.pointerUp(node("High shelf"), { ...pointer, ...place })
    expect(screen.getByRole("region", { name: "High shelf" })).toBeVisible()
    expect(node("High shelf")).toHaveAttribute("data-selected")
    expect(screen.getByRole("radio", { name: "High shelf" })).toHaveAttribute(
      "aria-checked",
      "true"
    )
  })

  it("sets Q with the wheel, one undo step for a burst", async () => {
    const effect = await open("eq")
    fireEvent.wheel(node("Peak 3"), { deltaY: -100 })
    fireEvent.wheel(node("Peak 3"), { deltaY: -100 })
    fireEvent.wheel(node("Peak 3"), { deltaY: -100 })
    await flush()
    const band = eq(effect).peak3
    expect(band.q).toBeGreaterThan(1.5)
    expect(band).toMatchObject({ frequencyHz: 3500, gainDb: 0 })
    expect(history().cursor).toBe(2)
  })

  it("sets Q with a vertical drag while Alt is held", async () => {
    const effect = await open("eq")
    const start = at(100, 0)
    dragNode(
      "Low shelf",
      start,
      { clientX: start.clientX, clientY: start.clientY + 48 },
      { altKey: true }
    )
    await flush()
    const band = eq(effect).lowShelf
    // 48 pixels down halves it.
    expect(band.q).toBeCloseTo(Math.SQRT1_2 / 2, 2)
    expect(band).toMatchObject({ frequencyHz: 100, gainDb: 0 })
    expect(history().cursor).toBe(2)
  })

  it("puts a band back to its defaults on a double-click", async () => {
    const effect = await open("eq")
    dragNode("Peak 2", at(1000, 0), at(300, -12))
    await flush()
    expect(eq(effect).peak2.gainDb).toBeLessThan(-10)

    fireEvent.doubleClick(node("Peak 2"))
    await flush()
    expect(eq(effect).peak2).toEqual(effectDescriptor("eq").defaults.peak2)
    // The drag and the reset: two steps after adding the effect.
    expect(history().cursor).toBe(3)
  })

  it("moves the focused node with the keyboard", async () => {
    const effect = await open("eq")
    const peak = node("Peak 1")
    act(() => peak.focus())
    fireEvent.keyDown(peak, { key: "ArrowUp" })
    fireEvent.keyDown(peak, { key: "ArrowUp" })
    fireEvent.keyDown(peak, { key: "ArrowRight" })
    fireEvent.keyUp(peak, { key: "ArrowRight" })
    await flush()
    expect(eq(effect).peak1.gainDb).toBe(2)
    expect(eq(effect).peak1.frequencyHz).toBeCloseTo(400 * 2 ** (1 / 12), 0)
    // One held run of keys is one step.
    expect(history().cursor).toBe(2)

    fireEvent.keyDown(peak, { key: "Enter" })
    await flush()
    expect(eq(effect).peak1.enabled).toBe(false)
  })

  it("lets the range of the display be chosen", async () => {
    await open("eq")
    const group = screen.getByRole("radiogroup", {
      name: "Gain range of the display",
    })
    expect(
      within(group)
        .getAllByRole("radio")
        .map((item) => item.textContent)
    ).toEqual(["±6", "±12", "±24"])
    expect(
      within(group).getByRole("radio", { checked: true })
    ).toHaveTextContent("±24")
    fireEvent.click(within(group).getByText("±6"))
    expect(
      within(group).getByRole("radio", { checked: true })
    ).toHaveTextContent("±6")
    // Looking closer is not an edit.
    expect(history().cursor).toBe(1)
  })
})

describe("what the editors read out", () => {
  it("reads the compressor's top ratio as infinity to one", async () => {
    const effect = await open("compressor")
    const ratio = paramIndex(effectDescriptor("compressor"), "ratio")
    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: ratio,
      value: 1000,
    })
    await flush()
    // The core holds the value at the top of the range.
    expect(paramsOf<CompressorParams>(effect).ratio).toBe(100)
    expect(screen.getByRole("slider", { name: "Ratio" })).toHaveAttribute(
      "aria-valuetext",
      "∞:1"
    )
  })

  it("shows the makeup in effect, the automatic part included", async () => {
    const effect = await open("compressor")
    const readout = () =>
      document.querySelector("[data-slot=compressor-makeup]")?.textContent
    expect(readout()).toBe("Makeup in effect: 0.0 dB makeup")
    await dispatch({
      type: "setEffectParams",
      track,
      effect,
      params: {
        ...effectDescriptor("compressor").defaults,
        thresholdDb: -12,
        ratio: 100,
        kneeDb: 12,
        autoMakeup: true,
      },
    })
    await flush()
    expect(readout()).toBe("Makeup in effect: +6.0 dB makeup")
    expect(
      screen.getByRole("group", { name: "Compressor gain reduction" })
    ).toBeVisible()
  })

  it("says how much latency the limiter's look-ahead adds", async () => {
    const effect = await open("limiter")
    const note = () =>
      document.querySelector("[data-slot=limiter-latency]")?.textContent
    // The mock engine runs at 48 kHz.
    expect(note()).toContain("5.0 ms (240 samples)")
    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: paramIndex(effectDescriptor("limiter"), "lookaheadMs"),
      value: 1.5,
    })
    await flush()
    expect(paramsOf<LimiterParams>(effect).lookaheadMs).toBe(1.5)
    expect(note()).toContain("1.5 ms (72 samples)")
    expect(
      screen.getByRole("group", { name: "Limiter gain reduction" })
    ).toBeVisible()
  })

  it("shows a synced delay's time in milliseconds at the tempo", async () => {
    await open("delay")
    const readout = () =>
      document.querySelector("[data-slot=delay-equivalent]")?.textContent
    // An eighth note at the demo's 128 BPM.
    expect(readout()).toBe("Delay time: 1/8234 ms at 128 BPM")
    await dispatch({ type: "updateSettings", patch: { tempoBpm: 120 } })
    await flush()
    expect(readout()).toBe("Delay time: 1/8250 ms at 120 BPM")
  })

  it("shows the note length a free delay time is closest to", async () => {
    const effect = await open("delay")
    const descriptor = effectDescriptor("delay")
    const readout = () =>
      document.querySelector("[data-slot=delay-equivalent]")?.textContent
    await dispatch({ type: "updateSettings", patch: { tempoBpm: 120 } })
    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: paramIndex(descriptor, "sync"),
      value: 0,
    })
    await flush()
    expect(readout()).toBe("Closest note length: 250 ms1/8 at 120 BPM")
    await dispatch({
      type: "setEffectParam",
      track,
      effect,
      param: paramIndex(descriptor, "timeMs"),
      value: 400,
    })
    await flush()
    expect(readout()).toBe(
      "Closest note length: 400 msabout 1/8 dotted at 120 BPM"
    )
  })
})

describe("a kind with no editor of its own", () => {
  const descriptor: ParamDescriptor = {
    name: "Wobbler",
    params: [
      {
        id: "depth",
        name: "Depth",
        kind: "float",
        unit: "fraction",
        scale: "linear",
        min: 0,
        max: 1,
        default: 0.5,
        choices: [],
      },
      {
        id: "rateHz",
        name: "Rate",
        kind: "float",
        unit: "hertz",
        scale: "logarithmic",
        min: 0.1,
        max: 20,
        default: 2,
        choices: [],
      },
      {
        id: "tone.bright",
        name: "Tone bright",
        kind: "toggle",
        unit: "none",
        scale: "linear",
        min: 0,
        max: 1,
        default: 0,
        choices: [],
      },
      {
        id: "tone.shape",
        name: "Tone shape",
        kind: "choice",
        unit: "none",
        scale: "linear",
        min: 0,
        max: 1,
        default: 0,
        choices: [
          { value: "sine", label: "Sine" },
          { value: "square", label: "Square" },
        ],
      },
    ],
    defaults: {},
  }
  // What a project would hold for an effect the core gained later.
  const slot = {
    id: 9001,
    enabled: true,
    mix: 1,
    params: {
      type: "wobbler",
      depth: 0.5,
      rateHz: 2,
      tone: { bright: false, shape: "sine" },
    } as unknown as EffectParams,
  } satisfies EffectSlot

  it("gets the generic editor, with a control for every setting", async () => {
    vi.spyOn(backend, "dispatch").mockImplementation(() =>
      Promise.reject(new Error("no such effect"))
    )
    render(
      <EffectEditor trackId={track} slot={slot} describe={() => descriptor} />
    )
    expect(
      screen.getByRole("group", { name: "Wobbler settings" })
    ).toBeVisible()
    expect(screen.getByRole("slider", { name: "Depth" })).toHaveAttribute(
      "aria-valuetext",
      "50%"
    )
    expect(screen.getByRole("slider", { name: "Rate" })).toHaveAttribute(
      "aria-valuetext",
      "2.00 Hz"
    )

    for (const info of descriptor.params) {
      const value = info.kind === "float" ? info.default : 0
      const commands = await operate(info, value)
      expect(commands.length, info.id).toBeGreaterThan(0)
      for (const command of commands) {
        expect(command, info.id).toMatchObject({
          type: "setEffectParam",
          track,
          effect: slot.id,
          param: paramIndex(descriptor, info.id),
        })
      }
    }
  })
})
