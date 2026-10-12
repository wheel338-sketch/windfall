import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { ReactNode } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EffectKind } from "@/bindings"
import { effectDescriptor, EFFECT_KINDS } from "@/features/params"
import {
  disabledReason,
  getAppState,
  isEnabled,
  registry,
  runAction,
} from "@/lib/actions"
import { dispatch, redo, undo } from "@/lib/store/project"
import { useHintStore } from "@/lib/store/hint"
import { usePromptStore } from "@/lib/store/prompts"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"

import MixerPanel from "."
import { MAX_EFFECT_SLOTS } from "./effect-ops"
import {
  addEffects,
  chain,
  dragData,
  dragEffect,
  fireDrag,
  ids,
  kinds,
  layOutRows,
  nameOfKind,
  rackNames,
  ROW,
  slotButton,
  slotLamp,
  slotRow,
} from "./effect-test-utils"
import { useEffectsUi } from "./effects-ui"
import {
  drag,
  flush,
  history,
  sizeMixer,
  strip,
  stubCanvas,
  trackNamed,
  tracks,
} from "./test-utils"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

const { toast } = await import("sonner")

// jsdom lays nothing out, so every element sits at 0,0 with no size, and
// the real resize handle takes every click for a press on itself.
vi.mock("@/components/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: ReactNode }) => (
    <div className="flex">{children}</div>
  ),
  ResizablePanel: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizableHandle: (props: { "aria-label"?: string }) => (
    <div role="separator" aria-label={props["aria-label"]} />
  ),
}))

let stop: () => void

beforeEach(async () => {
  stubCanvas()
  vi.mocked(toast.error).mockClear()
  useEffectsUi.setState(useEffectsUi.getInitialState(), true)
  ;({ stop } = await startTestApp())
})
afterEach(() => {
  stop()
  vi.restoreAllMocks()
})

const effectsUi = () => useEffectsUi.getState()
const labels = () => history().entries.map((entry) => entry.label)
const steps = () => history().cursor
const inspector = () => screen.getByRole("complementary", { name: "Effects" })
const panels = () =>
  [...inspector().querySelectorAll("[data-slot=effect-panel]")].map(
    (panel) => panel.querySelector("[data-slot=effect-title]")?.textContent
  )
const action = (id: string) => {
  const found = registry.get(id)
  if (!found) throw new Error(`"${id}" is not registered`)
  return found
}
const enabled = (id: string) => isEnabled(action(id), getAppState())
const select = (effect: number) => act(() => effectsUi().selectEffect(effect))

async function undoAll(count: number) {
  for (let step = 0; step < count; step += 1) await undo()
  await flush()
}

describe("the effect rack on a strip", () => {
  it.each([
    "balance",
    "dcBlock",
    "channelMute",
    "polarity",
    "stereoMatrix",
    "softClipper",
    "distortion",
  ] as const)("discovers and opens %s from the add menu", async (kind) => {
    render(<MixerPanel />)
    const user = userEvent.setup()
    const name = effectDescriptor(kind).name
    expect(registry.get(`mixer.addEffect.${kind}`)).toBeDefined()
    expect(registry.get(`mixer.replaceEffect.${kind}`)).toBeDefined()
    await user.click(
      within(strip("Clap")).getByRole("button", { name: "Add effect" })
    )
    await user.click(await screen.findByRole("menuitem", { name }))
    await flush()
    expect(kinds("Clap")).toEqual([kind])
    expect(panels()).toEqual([name])
    expect(history().cursor).toBe(1)
    await undoAll(1)
    expect(kinds("Clap")).toEqual([])
  })
  it("lists a track's effects in chain order, with a row to add one", async () => {
    render(<MixerPanel />)
    expect(rackNames("Kick")).toEqual([])
    expect(
      within(strip("Kick")).getByRole("button", { name: "Add effect" })
    ).toBeVisible()

    await addEffects("Kick", "eq", "compressor")
    expect(rackNames("Kick")).toEqual(["Parametric EQ", "Compressor"])
    expect(rackNames("Clap")).toEqual([])
    // The master has a rack like any other track.
    await addEffects("Master", "limiter")
    expect(rackNames("Master")).toEqual(["Limiter"])
  })

  it("adds an effect from the menu as one undo step and opens it", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    await user.click(
      within(strip("Clap")).getByRole("button", { name: "Add effect" })
    )
    expect(
      (await screen.findAllByRole("menuitem")).map((item) => item.textContent)
    ).toEqual(["Browse plugins…", "Audio editor recorder", ...EFFECT_KINDS.map(nameOfKind)])
    await user.click(screen.getByRole("menuitem", { name: "Reverb" }))
    await flush()

    expect(kinds("Clap")).toEqual(["reverb"])
    expect(chain("Clap")[0]).toMatchObject({ enabled: true, mix: 1 })
    expect(labels()).toEqual(["Add effect"])
    expect(useUiStore.getState().selectedTrack).toBe(trackNamed("Clap").id)
    expect(effectsUi().selectedEffect).toBe(ids("Clap")[0])
    expect(panels()).toEqual(["Reverb"])

    await undoAll(1)
    expect(kinds("Clap")).toEqual([])
    await redo()
    await flush()
    expect(kinds("Clap")).toEqual(["reverb"])
  })

  it("says a full chain is full where the add row was", async () => {
    render(<MixerPanel />)
    const kick = trackNamed("Kick").id
    await addEffects("Kick", ...Array<EffectKind>(MAX_EFFECT_SLOTS).fill("eq"))
    expect(chain("Kick")).toHaveLength(MAX_EFFECT_SLOTS)

    const rack = within(strip("Kick"))
    expect(rack.queryByRole("button", { name: "Add effect" })).toBeNull()
    expect(rack.getByText("10/10")).toBeVisible()

    act(() => useUiStore.getState().selectTrack(kick))
    expect(enabled("mixer.addEffect.reverb")).toBe(false)
    expect(
      disabledReason(action("mixer.addEffect.reverb"), getAppState())
    ).toBe("Holds 10 at most")
    await select(ids("Kick")[0])
    expect(enabled("mixer.duplicateEffect")).toBe(false)

    // The core refuses too, in its own words.
    await dispatch({ type: "addEffect", track: kick, kind: "delay" })
    expect(toast.error).toHaveBeenCalledWith(
      expect.stringContaining(
        'The mixer track "Kick" is full: it holds 10 effects at most'
      )
    )
    expect(chain("Kick")).toHaveLength(MAX_EFFECT_SLOTS)
  })

  it("switches an effect off and on with its lamp", async () => {
    render(<MixerPanel />)
    const [reverb] = await addEffects("Hat", "reverb")
    expect(slotLamp("Hat", reverb)).toHaveAttribute("aria-pressed", "true")

    fireEvent.click(slotLamp("Hat", reverb))
    await flush()
    expect(chain("Hat")[0].enabled).toBe(false)
    expect(labels().at(-1)).toBe("Switch effect off")
    expect(slotLamp("Hat", reverb)).toHaveAttribute("aria-pressed", "false")
    // A bypassed effect looks bypassed on the strip.
    expect(slotRow("Hat", reverb)).toHaveAttribute("data-bypassed")

    await undoAll(1)
    expect(chain("Hat")[0].enabled).toBe(true)
    expect(slotRow("Hat", reverb)).not.toHaveAttribute("data-bypassed")
  })

  it("opens the inspector on the effect that is clicked", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Snare", "eq", "compressor")
    act(() => effectsUi().setCollapsed(compressor, true))
    expect(screen.queryByRole("complementary", { name: "Effects" })).toBeNull()

    fireEvent.click(slotButton("Snare", compressor))
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(useUiStore.getState().selectedTrack).toBe(trackNamed("Snare").id)
    expect(effectsUi().selectedEffect).toBe(compressor)
    expect(effectsUi().collapsed).not.toContain(compressor)
    expect(panels()).toEqual(["Parametric EQ", "Compressor"])
    expect(slotRow("Snare", compressor)).toHaveAttribute("data-selected")
    expect(slotRow("Snare", eq)).not.toHaveAttribute("data-selected")
    // Opening an editor is not an edit.
    expect(steps()).toBe(2)
  })

  it("shows a gain reduction bar only on the effects that turn their signal down", async () => {
    render(<MixerPanel />)
    const withBar: EffectKind[] = []
    for (const kind of EFFECT_KINDS) {
      const [id] = await addEffects("Kick", kind)
      if (slotRow("Kick", id).querySelector("[data-slot=gain-reduction-bar]"))
        withBar.push(kind)
      await undoAll(1)
    }
    // Generated JSON keys are alphabetical; native kind indices stay append-only.
    expect(withBar).toEqual([
      "compressor",
      "limiter",
      "masterLimiter",
      "multibandCompressor",
      "multibandMaximizer",
      "oneKnob",
      "stageStack",
    ])
  })
})

describe("keys on a slot", () => {
  it("removes the focused effect with Delete, and not its track", async () => {
    render(<MixerPanel />)
    const [eq, compressor, reverb] = await addEffects(
      "Kick",
      "eq",
      "compressor",
      "reverb"
    )
    act(() => slotButton("Kick", compressor).focus())
    expect(effectsUi().selectedEffect).toBe(compressor)

    fireEvent.keyDown(slotButton("Kick", compressor), {
      key: "Delete",
      code: "Delete",
    })
    await flush()
    expect(ids("Kick")).toEqual([eq, reverb])
    expect(labels().at(-1)).toBe("Delete effect")
    expect(steps()).toBe(4)
    expect(tracks()).toHaveLength(5)
    expect(usePromptStore.getState().confirm).toBeNull()
    // The effect below takes the selection and the focus, for the next key.
    expect(effectsUi().selectedEffect).toBe(reverb)
    expect(slotButton("Kick", reverb)).toHaveFocus()

    await undoAll(1)
    expect(ids("Kick")).toEqual([eq, compressor, reverb])
  })

  it("removes and duplicates an effect from its header in the inspector", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Kick", "eq", "compressor")
    fireEvent.click(slotButton("Kick", eq))
    const title = within(inspector()).getByRole("button", {
      name: "Compressor",
    })
    act(() => title.focus())
    expect(effectsUi().selectedEffect).toBe(compressor)

    fireEvent.keyDown(title, { key: "d", code: "KeyD", ctrlKey: true })
    await flush()
    expect(kinds("Kick")).toEqual(["eq", "compressor", "compressor"])
    expect(tracks()).toHaveLength(5)

    const copy = ids("Kick")[2]
    expect(effectsUi().selectedEffect).toBe(copy)
    const header = inspector().querySelector<HTMLElement>(
      `[data-effect-row="${copy}"] [data-slot=effect-title]`
    )
    if (!header) throw new Error("the copy has no panel")
    act(() => header.focus())
    fireEvent.keyDown(header, { key: "Delete", code: "Delete" })
    await flush()
    expect(ids("Kick")).toEqual([eq, compressor])
    expect(usePromptStore.getState().confirm).toBeNull()
    expect(tracks()).toHaveLength(5)
  })

  it("keeps Delete and Ctrl+D inside an effect's editor from the track", async () => {
    render(<MixerPanel />)
    const [compressor] = await addEffects("Kick", "compressor")
    fireEvent.click(slotButton("Kick", compressor))
    const body = inspector().querySelector<HTMLElement>(
      "[data-slot=effect-body]"
    )
    if (!body) throw new Error("the compressor has no editor")

    // On a knob Delete is used up like anywhere else in the editor: it
    // neither removes the effect nor puts the setting back.
    const [knob] = within(body).getAllByRole("slider")
    act(() => knob.focus())
    fireEvent.keyDown(knob, { key: "ArrowUp" })
    fireEvent.keyUp(knob, { key: "ArrowUp" })
    await flush()
    const moved = knob.getAttribute("aria-valuenow")
    expect(fireEvent.keyDown(knob, { key: "Delete", code: "Delete" })).toBe(
      false
    )
    fireEvent.keyUp(knob, { key: "Delete", code: "Delete" })
    await flush()
    expect(knob.getAttribute("aria-valuenow")).toBe(moved)
    expect(ids("Kick")).toEqual([compressor])

    // Anywhere else in the editor Delete is used up and does nothing, and
    // Ctrl+D copies neither the effect nor anything else.
    const before = steps()
    expect(fireEvent.keyDown(body, { key: "Delete", code: "Delete" })).toBe(
      false
    )
    fireEvent.keyDown(knob, { key: "d", code: "KeyD", ctrlKey: true })
    await flush()
    expect(usePromptStore.getState().confirm).toBeNull()
    expect(ids("Kick")).toEqual([compressor])
    expect(tracks()).toHaveLength(5)
    expect(steps()).toBe(before)
  })

  it("still deletes the track with Delete anywhere else on the strip", async () => {
    render(<MixerPanel />)
    await dispatch({ type: "addMixerTrack" })
    await flush()
    const bus = tracks()[5]
    await dispatch({ type: "addEffect", track: bus.id, kind: "delay" })
    await flush()
    act(() => strip(bus.name).focus())

    fireEvent.keyDown(strip(bus.name), { key: "Delete", code: "Delete" })
    await flush()
    // A track with effects is not deleted without a word about them.
    const question = usePromptStore.getState().confirm
    expect(question?.description).toBe("Its Delay goes with it.")
    question?.resolve("delete")
    await flush()
    expect(tracks()).toHaveLength(5)
  })

  it("moves the selected effect with Alt and the arrows, and keeps the focus", async () => {
    render(<MixerPanel />)
    const [eq, compressor, reverb] = await addEffects(
      "Kick",
      "eq",
      "compressor",
      "reverb"
    )
    act(() => slotButton("Kick", eq).focus())
    expect(enabled("mixer.moveEffectUp")).toBe(false)

    fireEvent.keyDown(slotButton("Kick", eq), {
      key: "ArrowDown",
      code: "ArrowDown",
      altKey: true,
    })
    await flush()
    expect(ids("Kick")).toEqual([compressor, eq, reverb])
    expect(rackNames("Kick")).toEqual(["Compressor", "Parametric EQ", "Reverb"])
    expect(labels().at(-1)).toBe("Move effect")
    expect(steps()).toBe(4)
    expect(slotButton("Kick", eq)).toHaveFocus()

    fireEvent.keyDown(slotButton("Kick", eq), {
      key: "ArrowDown",
      code: "ArrowDown",
      altKey: true,
    })
    await flush()
    expect(ids("Kick")).toEqual([compressor, reverb, eq])
    expect(enabled("mixer.moveEffectDown")).toBe(false)

    fireEvent.keyDown(slotButton("Kick", eq), {
      key: "ArrowUp",
      code: "ArrowUp",
      altKey: true,
    })
    await flush()
    expect(ids("Kick")).toEqual([compressor, eq, reverb])

    await undoAll(3)
    expect(ids("Kick")).toEqual([eq, compressor, reverb])
  })

  it("opens the editor with Enter, which presses the slot", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    const [delay] = await addEffects("Clap", "delay")
    act(() => slotButton("Clap", delay).focus())
    await user.keyboard("{Enter}")
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(panels()).toEqual(["Delay"])
  })
})

describe("dragging effects", () => {
  it("reorders within a strip as one undo step", async () => {
    render(<MixerPanel />)
    const [eq, compressor, reverb] = await addEffects(
      "Kick",
      "eq",
      "compressor",
      "reverb"
    )
    const result = await dragEffect(slotRow("Kick", eq), strip("Kick"), 3)
    expect(result).toMatchObject({ accepted: true, dropEffect: "move" })
    expect(result.carried).toBe(String(eq))
    expect(ids("Kick")).toEqual([compressor, reverb, eq])
    expect(labels().at(-1)).toBe("Move effect")
    expect(steps()).toBe(4)

    await dragEffect(slotRow("Kick", reverb), strip("Kick"), 0)
    expect(ids("Kick")).toEqual([reverb, compressor, eq])
    expect(steps()).toBe(5)

    await undoAll(2)
    expect(ids("Kick")).toEqual([eq, compressor, reverb])
  })

  it("takes no drop that would leave the effect where it is", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq", "compressor")
    for (const gap of [0, 1]) {
      const result = await dragEffect(slotRow("Kick", eq), strip("Kick"), gap)
      expect(result.accepted).toBe(false)
    }
    expect(steps()).toBe(2)
  })

  it("moves an effect to another strip, where it keeps its id and settings", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Kick", "eq", "compressor")
    const [reverb] = await addEffects("Clap", "reverb")
    await dispatch({
      type: "setEffectParam",
      track: trackNamed("Kick").id,
      effect: compressor,
      param: 0,
      value: -30,
    })
    await flush()
    const before = chain("Kick")[1]

    const result = await dragEffect(
      slotRow("Kick", compressor),
      strip("Clap"),
      0
    )
    expect(result).toMatchObject({ accepted: true, dropEffect: "move" })
    expect(ids("Kick")).toEqual([eq])
    expect(ids("Clap")).toEqual([compressor, reverb])
    expect(chain("Clap")[0]).toEqual(before)
    expect(labels().at(-1)).toBe("Move effect")
    expect(steps()).toBe(5)
    // The selection follows the effect to its new track.
    expect(useUiStore.getState().selectedTrack).toBe(trackNamed("Clap").id)
    expect(effectsUi().selectedEffect).toBe(compressor)

    await undoAll(1)
    expect(ids("Kick")).toEqual([eq, compressor])
    expect(ids("Clap")).toEqual([reverb])
  })

  it("copies with Ctrl or Alt held, as one undo step", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Kick", "eq", "compressor")
    const [reverb] = await addEffects("Clap", "reverb")

    // Onto another strip.
    const result = await dragEffect(slotRow("Kick", eq), strip("Clap"), 1, {
      ctrlKey: true,
    })
    expect(result).toMatchObject({ accepted: true, dropEffect: "copy" })
    expect(ids("Kick")).toEqual([eq, compressor])
    expect(kinds("Clap")).toEqual(["reverb", "eq"])
    const copy = ids("Clap")[1]
    expect(copy).not.toBe(eq)
    expect(chain("Clap")[1].params).toEqual(chain("Kick")[0].params)
    expect(labels().at(-1)).toBe("Duplicate effect")
    expect(steps()).toBe(4)

    // Within the strip, to the top.
    await dragEffect(slotRow("Kick", compressor), strip("Kick"), 0, {
      altKey: true,
    })
    expect(kinds("Kick")).toEqual(["compressor", "eq", "compressor"])
    expect(ids("Kick").slice(1)).toEqual([eq, compressor])
    expect(steps()).toBe(5)

    await undoAll(2)
    expect(ids("Kick")).toEqual([eq, compressor])
    expect(ids("Clap")).toEqual([reverb])
  })

  it("copies out of a full chain too, still as one step", async () => {
    render(<MixerPanel />)
    const full = await addEffects(
      "Kick",
      ...Array<EffectKind>(MAX_EFFECT_SLOTS).fill("delay")
    )
    await dispatch({
      type: "updateEffect",
      track: trackNamed("Kick").id,
      effect: full[2],
      patch: { enabled: false, mix: 0.25 },
    })
    await flush()
    const before = steps()

    await dragEffect(slotRow("Kick", full[2]), strip("Hat"), 0, {
      ctrlKey: true,
    })
    expect(chain("Kick")).toHaveLength(MAX_EFFECT_SLOTS)
    expect(chain("Hat")).toHaveLength(1)
    expect(chain("Hat")[0]).toMatchObject({
      enabled: false,
      mix: 0.25,
      params: chain("Kick")[2].params,
    })
    expect(steps()).toBe(before + 1)
  })

  it("does not take a move onto a strip that is full", async () => {
    render(<MixerPanel />)
    await addEffects(
      "Kick",
      ...Array<EffectKind>(MAX_EFFECT_SLOTS).fill("delay")
    )
    const [reverb] = await addEffects("Clap", "reverb")
    const before = steps()
    const result = await dragEffect(slotRow("Clap", reverb), strip("Kick"), 0)
    expect(result.accepted).toBe(false)
    expect(steps()).toBe(before)
    expect(ids("Clap")).toEqual([reverb])
  })

  it("marks the gap a dragged effect would land in", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq", "compressor", "reverb")
    const dataTransfer = dragData()
    layOutRows(strip("Kick"))
    fireEvent.dragStart(slotRow("Kick", eq), { dataTransfer })
    fireDrag("dragOver", strip("Kick"), dataTransfer, 2 * ROW + 1)

    const rack = strip("Kick").querySelector("[data-slot=effect-rack]")
    const order = [...(rack?.children ?? [])].map(
      (child) => (child as HTMLElement).dataset.slot ?? "add"
    )
    expect(order).toEqual([
      "effect-slot",
      "effect-slot",
      "effect-drop",
      "effect-slot",
      "add",
    ])
    expect(strip("Kick")).toHaveAttribute("data-drop")

    fireDrag("dragLeave", strip("Kick"), dataTransfer, 0)
    expect(strip("Kick").querySelector("[data-slot=effect-drop]")).toBeNull()
    fireEvent.dragEnd(slotRow("Kick", eq), { dataTransfer })
  })
})

describe("effect actions", () => {
  it("offers what can be done to an effect on right-click", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq", "compressor")
    fireEvent.contextMenu(slotRow("Kick", eq))
    const menu = await screen.findByRole("menu")
    expect(effectsUi().selectedEffect).toBe(eq)
    expect(
      [...menu.querySelectorAll("[role^=menuitem]")].map(
        (item) => item.textContent
      )
    ).toEqual([
      "Open effect",
      "Bypass effect",
      "Duplicate effectCtrl+D",
      "Move effect upAlt+↑",
      "Move effect downAlt+↓",
      "Replace with",
      "Reset effect to defaults",
      "Remove effectDel",
    ])
    expect(
      within(menu).getByRole("menuitemcheckbox", { name: "Bypass effect" })
    ).toHaveAttribute("aria-checked", "false")
    expect(
      within(menu).getByRole("menuitem", { name: /^Move effect up/ })
    ).toHaveAttribute("aria-disabled", "true")
  })

  it("registers every effect action for the palette and the menus", () => {
    render(<MixerPanel />)
    for (const id of [
      "mixer.effects",
      "mixer.openEffect",
      "mixer.bypassEffect",
      "mixer.duplicateEffect",
      "mixer.moveEffectUp",
      "mixer.moveEffectDown",
      "mixer.resetEffect",
      "mixer.removeEffect",
      ...EFFECT_KINDS.flatMap((kind) => [
        `mixer.addEffect.${kind}`,
        `mixer.replaceEffect.${kind}`,
      ]),
    ]) {
      expect(registry.get(id), id).toBeDefined()
    }
    expect(action("mixer.addEffect.eq").title).toBe("Add Parametric EQ")
    // With nothing selected there is nothing for them to act on.
    expect(enabled("mixer.removeEffect")).toBe(false)
    expect(enabled("mixer.bypassEffect")).toBe(false)
    expect(enabled("mixer.addEffect.eq")).toBe(false)
  })

  it("adds to the selected track from the palette", async () => {
    render(<MixerPanel />)
    act(() => useUiStore.getState().selectTrack(trackNamed("Hat").id))
    await runAction("mixer.addEffect.limiter")
    await flush()
    expect(kinds("Hat")).toEqual(["limiter"])
    expect(labels()).toEqual(["Add effect"])
  })

  it("duplicates the selected effect right after itself", async () => {
    render(<MixerPanel />)
    const [eq, reverb] = await addEffects("Kick", "eq", "reverb")
    await select(eq)
    await runAction("mixer.duplicateEffect")
    await flush()
    expect(kinds("Kick")).toEqual(["eq", "eq", "reverb"])
    expect(ids("Kick")[2]).toBe(reverb)
    expect(labels().at(-1)).toBe("Duplicate effect")
    expect(steps()).toBe(3)
    // The copy is the one selected now.
    expect(effectsUi().selectedEffect).toBe(ids("Kick")[1])
    await undoAll(1)
    expect(ids("Kick")).toEqual([eq, reverb])
  })

  it("replaces an effect in place as one undo step", async () => {
    render(<MixerPanel />)
    const [eq, compressor, reverb] = await addEffects(
      "Kick",
      "eq",
      "compressor",
      "reverb"
    )
    await select(compressor)
    expect(enabled("mixer.replaceEffect.compressor")).toBe(false)
    await runAction("mixer.replaceEffect.limiter")
    await flush()

    expect(kinds("Kick")).toEqual(["eq", "limiter", "reverb"])
    expect(chain("Kick")[1].params).toEqual(
      effectDescriptor("limiter").defaults
    )
    expect(ids("Kick")[0]).toBe(eq)
    expect(ids("Kick")[2]).toBe(reverb)
    expect(steps()).toBe(4)
    // One history entry, named for what was done and not for its parts.
    expect(labels().at(-1)).toBe("Replace effect")
    expect(effectsUi().selectedEffect).toBe(ids("Kick")[1])

    await undoAll(1)
    expect(ids("Kick")).toEqual([eq, compressor, reverb])
  })

  it("replaces an effect on a full track", async () => {
    render(<MixerPanel />)
    const full = await addEffects(
      "Kick",
      ...Array<EffectKind>(MAX_EFFECT_SLOTS).fill("delay")
    )
    await select(full[9])
    await runAction("mixer.replaceEffect.reverb")
    await flush()
    expect(kinds("Kick")[9]).toBe("reverb")
    expect(chain("Kick")).toHaveLength(MAX_EFFECT_SLOTS)
    expect(steps()).toBe(MAX_EFFECT_SLOTS + 1)
    expect(labels().at(-1)).toBe("Replace effect")
  })

  it("resets an effect's settings and leaves its mix and lamp alone", async () => {
    render(<MixerPanel />)
    const [compressor] = await addEffects("Snare", "compressor")
    const snare = trackNamed("Snare").id
    await dispatch({
      type: "setEffectParam",
      track: snare,
      effect: compressor,
      param: 1,
      value: 12,
    })
    await dispatch({
      type: "updateEffect",
      track: snare,
      effect: compressor,
      patch: { mix: 0.5, enabled: false },
    })
    await flush()
    await select(compressor)
    await runAction("mixer.resetEffect")
    await flush()

    expect(chain("Snare")[0]).toEqual({
      id: compressor,
      enabled: false,
      mix: 0.5,
      params: effectDescriptor("compressor").defaults,
    })
    expect(labels().at(-1)).toBe("Change effect settings")
    expect(steps()).toBe(4)
    await undoAll(1)
    expect(chain("Snare")[0].params).toMatchObject({ ratio: 12 })
  })

  it("bypasses and removes through the registry", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Clap", "eq")
    await select(eq)
    await runAction("mixer.bypassEffect")
    await flush()
    expect(chain("Clap")[0].enabled).toBe(false)
    expect(action("mixer.bypassEffect").checked?.(getAppState())).toBe(true)
    await runAction("mixer.bypassEffect")
    await flush()
    expect(chain("Clap")[0].enabled).toBe(true)

    await runAction("mixer.removeEffect")
    await flush()
    expect(chain("Clap")).toEqual([])
    expect(effectsUi().selectedEffect).toBeNull()
    expect(steps()).toBe(4)
  })
})

describe("the selected effect", () => {
  it("is let go when another track is selected", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq")
    fireEvent.click(slotButton("Kick", eq))
    expect(effectsUi().selectedEffect).toBe(eq)
    expect(enabled("mixer.removeEffect")).toBe(true)

    act(() => useUiStore.getState().selectTrack(trackNamed("Clap").id))
    expect(effectsUi().selectedEffect).toBeNull()
    expect(enabled("mixer.removeEffect")).toBe(false)
    expect(slotRow("Kick", eq)).not.toHaveAttribute("data-selected")
  })

  it("follows an undo that takes the effect away", async () => {
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq")
    fireEvent.click(slotButton("Kick", eq))
    await undoAll(1)
    expect(chain("Kick")).toEqual([])
    expect(enabled("mixer.removeEffect")).toBe(false)
    expect(enabled("mixer.bypassEffect")).toBe(false)
  })
})

describe("the effect inspector", () => {
  it("is folded to a tab until it is asked for", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    expect(screen.queryByRole("complementary", { name: "Effects" })).toBeNull()
    await user.click(screen.getByRole("button", { name: "Show effects" }))
    expect(inspector()).toBeVisible()
    expect(within(inspector()).getByText("No track selected")).toBeVisible()

    await runAction("mixer.effects")
    await flush()
    expect(screen.queryByRole("complementary", { name: "Effects" })).toBeNull()
  })

  it("shows the track's name and its number of effects, the count never cut short", async () => {
    render(<MixerPanel />)
    await addEffects("Kick", "eq", "compressor")
    fireEvent.click(slotButton("Kick", ids("Kick")[0]))
    const heading = within(inspector()).getByRole("heading", {
      name: "Kick effects",
    })
    const count = within(inspector()).getByLabelText("2 of 10 effects")
    expect(count).toHaveTextContent("2/10")
    // The name gives way in a narrow panel. The count is beside it, not
    // in it, so it is not what gets cut off.
    expect(heading).toHaveClass("truncate")
    expect(heading).not.toContainElement(count)
    expect(count).toHaveClass("shrink-0")
  })

  it("is given the editor area by its Enlarge button, and comes back", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq")
    fireEvent.click(slotButton("Kick", eq))
    expect(useUiStore.getState().centerOverlay).toBeNull()

    await user.click(
      within(inspector()).getByRole("button", { name: "Enlarge effects" })
    )
    expect(useUiStore.getState().centerOverlay).toBe("effects")
    // Beside the strips there is only a tab now, which brings them back.
    expect(screen.queryByRole("complementary", { name: "Effects" })).toBeNull()
    const back = screen.getByRole("button", { name: "Bring the effects back" })
    await user.click(back)
    expect(useUiStore.getState().centerOverlay).toBeNull()
    expect(inspector()).toBeVisible()
  })

  it("goes back beside the strips when an editor tab is chosen or it is closed", async () => {
    render(<MixerPanel />)
    await runAction("mixer.enlargeEffects")
    expect(useUiStore.getState().centerOverlay).toBe("effects")
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(enabled("mixer.effectsBack")).toBe(true)

    await runAction("view.playlist")
    expect(useUiStore.getState().centerOverlay).toBeNull()
    expect(enabled("mixer.effectsBack")).toBe(false)

    // "Show effects" closes them wherever they are.
    await runAction("mixer.enlargeEffects")
    await runAction("mixer.effects")
    expect(useUiStore.getState().centerOverlay).toBeNull()
    expect(effectsUi().inspectorOpen).toBe(false)
  })

  it("invites the first effect on an empty chain", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    act(() => {
      useUiStore.getState().selectTrack(trackNamed("Hat").id)
      effectsUi().setInspectorOpen(true)
    })
    expect(
      within(inspector()).getByRole("heading", { name: "Hat effects" })
    ).toBeVisible()
    expect(
      within(inspector()).getByText("No effects on this track")
    ).toBeVisible()
    await user.click(
      within(inspector()).getByRole("button", { name: "Add effect" })
    )
    await user.click(await screen.findByRole("menuitem", { name: "Delay" }))
    await flush()
    expect(kinds("Hat")).toEqual(["delay"])
    expect(panels()).toEqual(["Delay"])
  })

  it("shows the selected track's chain and follows the selection", async () => {
    render(<MixerPanel />)
    await addEffects("Kick", "eq", "compressor")
    await addEffects("Clap", "reverb")
    act(() => {
      useUiStore.getState().selectTrack(trackNamed("Kick").id)
      effectsUi().setInspectorOpen(true)
    })
    expect(panels()).toEqual(["Parametric EQ", "Compressor"])
    expect(within(inspector()).getByText("2/10")).toBeVisible()
    act(() => useUiStore.getState().selectTrack(trackNamed("Clap").id))
    expect(panels()).toEqual(["Reverb"])
  })

  it("sets the dry/wet mix with the header knob as one undo step", async () => {
    render(<MixerPanel />)
    const [reverb] = await addEffects("Clap", "reverb")
    fireEvent.click(slotButton("Clap", reverb))
    const knob = within(inspector()).getByRole("slider", {
      name: "Reverb dry/wet mix",
    })
    expect(knob).toHaveAttribute("aria-valuetext", "100%")

    drag(knob, [-10, -10, -10, -10, -10])
    await flush()
    const mix = chain("Clap")[0].mix
    expect(mix).toBeGreaterThan(0.5)
    expect(mix).toBeLessThan(1)
    expect(labels().at(-1)).toBe("Change effect mix")
    expect(steps()).toBe(2)

    await undoAll(1)
    expect(chain("Clap")[0].mix).toBe(1)
  })

  it("folds an effect away and shows a bypassed one as bypassed", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    const [limiter] = await addEffects("Master", "limiter")
    fireEvent.click(slotButton("Master", limiter))
    const panel = inspector().querySelector("[data-slot=effect-panel]")
    expect(panel?.querySelector("[data-slot=effect-body]")).not.toBeNull()

    await user.click(
      within(inspector()).getByRole("button", { name: "Limiter" })
    )
    expect(effectsUi().collapsed).toEqual([limiter])
    expect(panel?.querySelector("[data-slot=effect-body]")).toBeNull()

    await user.click(
      within(inspector()).getByRole("button", { name: "Limiter on" })
    )
    await flush()
    expect(chain("Master")[0].enabled).toBe(false)
    expect(panel).toHaveAttribute("data-bypassed")
    expect(within(inspector()).getByText("Bypassed")).toBeVisible()
  })

  it("reorders by dragging a panel's grip", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Kick", "eq", "compressor")
    fireEvent.click(slotButton("Kick", eq))
    const list = within(inspector()).getByRole("list", {
      name: "Effect chain",
    })
    const grip = list.querySelector<HTMLElement>("[data-slot=effect-grip]")
    if (!grip) throw new Error("The panel has no grip")
    await dragEffect(grip, list, 2)
    expect(ids("Kick")).toEqual([compressor, eq])
    expect(panels()).toEqual(["Compressor", "Parametric EQ"])
    expect(steps()).toBe(3)
  })

  it("runs the slot's actions from the panel's menu", async () => {
    const user = userEvent.setup()
    render(<MixerPanel />)
    const [eq, reverb] = await addEffects("Kick", "eq", "reverb")
    fireEvent.click(slotButton("Kick", eq))
    await user.click(
      within(inspector()).getByRole("button", { name: "Reverb actions" })
    )
    expect(effectsUi().selectedEffect).toBe(reverb)
    await user.click(
      await screen.findByRole("menuitem", { name: /^Move effect up/ })
    )
    await flush()
    expect(ids("Kick")).toEqual([reverb, eq])
  })

  it("says how much its limiters delay the track", async () => {
    render(<MixerPanel />)
    const [limiter] = await addEffects("Master", "limiter")
    fireEvent.click(slotButton("Master", limiter))
    const latency = () =>
      inspector().querySelector("[data-slot=chain-latency]")?.textContent
    // 5 ms at the mock engine's 48 kHz.
    expect(latency()).toBe("5.0 ms (240 samples) compensated")

    await addEffects("Master", "limiter")
    expect(latency()).toBe("10 ms (480 samples) compensated")

    act(() => useUiStore.getState().selectTrack(trackNamed("Kick").id))
    expect(latency()).toBeUndefined()
  })

  it("shows compensated shared matrix and distortion latency while bypassed", async () => {
    render(<MixerPanel />)
    const [matrix, distortion] = await addEffects(
      "Kick",
      "stereoMatrix",
      "distortion"
    )
    const track = trackNamed("Kick").id
    await dispatch({
      type: "setEffectParams",
      track,
      effect: matrix,
      params: {
        ...effectDescriptor("stereoMatrix").defaults,
        leftDelayMs: 2,
        rightDelayMs: 5,
      },
    })
    await flush()
    fireEvent.click(slotButton("Kick", matrix))
    const badge = () => inspector().querySelector("[data-slot=chain-latency]")
    expect(badge()?.textContent).toBe("2.7 ms (128 samples) compensated")
    for (const effect of [matrix, distortion]) {
      await dispatch({
        type: "updateEffect",
        track,
        effect,
        patch: { enabled: false, mix: 0 },
      })
    }
    await flush()
    expect(badge()?.textContent).toBe("2.7 ms (128 samples) compensated")
    fireEvent.pointerEnter(badge()!)
    expect(useHintStore.getState().text).toContain("shared delay")
    expect(useHintStore.getState().text).not.toContain("limiters on this track")
    await undo()
    await flush()
    expect(badge()?.textContent).toBe("2.7 ms (128 samples) compensated")
    await dispatch({
      type: "setEffectParam",
      track,
      effect: matrix,
      param: 5,
      value: 0,
    })
    await flush()
    expect(badge()?.textContent).toBe("0.7 ms (32 samples) compensated")
    await undo()
    await flush()
    expect(badge()?.textContent).toBe("2.7 ms (128 samples) compensated")
    // Kick keeps its sampler channel, and a hosted effect shares this chain.
    await dispatch({
      type: "addPluginEffect",
      track,
      plugin: {
        target: { type: "effect", effect: 0 },
        format: "clap",
        path: "/plugins/gain.clap",
        id: "native.gain",
        name: "Native Gain",
        state: [1, 2],
        parameters: [],
      },
    })
    await flush()
    expect(badge()?.textContent).toBe("2.7 ms (128 samples) compensated")
    expect(inspector().textContent).toContain("Native Gain")
  })

  it("remembers that it is open, and not what is folded", async () => {
    render(<MixerPanel />)
    const [eq, compressor] = await addEffects("Kick", "eq", "compressor")
    fireEvent.click(slotButton("Kick", eq))
    act(() => effectsUi().setCollapsed(compressor, true))

    // Folds are kept by effect id, and ids start over in every project, so
    // only whether the inspector is open is saved.
    const saved = JSON.parse(localStorage.getItem("windfall.mixer") ?? "{}")
    expect(saved.state).toEqual({ inspectorOpen: true })

    // A new session starts from what was saved.
    useEffectsUi.setState(useEffectsUi.getInitialState(), true)
    localStorage.setItem("windfall.mixer", JSON.stringify(saved))
    await act(() => useEffectsUi.persist.rehydrate())
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(effectsUi().collapsed).toEqual([])
    // The selection is not kept: it belongs to the session.
    expect(effectsUi().selectedEffect).toBeNull()

    // What an earlier version saved loads without its folds.
    localStorage.setItem(
      "windfall.mixer",
      JSON.stringify({
        state: { inspectorOpen: true, collapsed: [compressor] },
        version: 1,
      })
    )
    await act(() => useEffectsUi.persist.rehydrate())
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(effectsUi().collapsed).toEqual([])
  })

  it("saves its width with the other panel sizes", () => {
    render(<MixerPanel />)
    act(() => effectsUi().setInspectorOpen(true))
    expect(
      screen.getByRole("separator", { name: "Resize the effects" })
    ).toBeVisible()
    act(() =>
      useUiStore.getState().saveLayout("mixer:strips+effects", {
        strips: 60,
        effects: 40,
      })
    )
    const saved = JSON.parse(localStorage.getItem("windfall.ui") ?? "{}")
    expect(saved.state.layouts["mixer:strips+effects"]).toEqual({
      strips: 60,
      effects: 40,
    })
  })
})

describe("low panels", () => {
  it("shrinks the rack to a badge that opens the chain", async () => {
    sizeMixer(800, 150)
    render(<MixerPanel />)
    await addEffects("Kick", "eq", "compressor")
    const kick = within(strip("Kick"))
    expect(strip("Kick").querySelector("[data-slot=effect-rack]")).toBeNull()
    const badge = kick.getByRole("button", { name: "FX: 2 effects" })
    expect(badge).toHaveTextContent("FX2")
    // A track without effects offers to add one.
    expect(
      within(strip("Clap")).getByRole("button", { name: "FX: 0 effects" })
    ).toHaveTextContent("+ FX")

    fireEvent.click(badge)
    expect(effectsUi().inspectorOpen).toBe(true)
    expect(panels()).toEqual(["Parametric EQ", "Compressor"])
  })

  it("keeps the badge in the name row at the lowest height, for tracks that have effects", async () => {
    sizeMixer(800, 84)
    render(<MixerPanel />)
    await addEffects("Hat", "delay")
    expect(
      within(strip("Hat")).getByRole("button", { name: "FX: 1 effect" })
    ).toHaveTextContent("FX1")
    expect(strip("Kick").querySelector("[data-slot=effect-badge]")).toBeNull()
  })

  it("takes a dropped effect on a strip that shows only a badge", async () => {
    sizeMixer(800, 150)
    render(<MixerPanel />)
    const [eq] = await addEffects("Kick", "eq")
    await addEffects("Clap", "reverb")
    act(() => {
      useUiStore.getState().selectTrack(trackNamed("Kick").id)
      effectsUi().setInspectorOpen(true)
    })
    const grip = inspector().querySelector<HTMLElement>(
      "[data-slot=effect-grip]"
    )
    if (!grip) throw new Error("The panel has no grip")
    await dragEffect(grip, strip("Clap"), 0)
    // With no rows to aim at, it goes to the end of the chain.
    expect(kinds("Clap")).toEqual(["reverb", "eq"])
    expect(ids("Clap")[1]).toBe(eq)
  })
})

describe("deleting a track with effects", () => {
  it("says the effects go with it", async () => {
    render(<MixerPanel />)
    await addEffects("Kick", "eq", "compressor")
    act(() => useUiStore.getState().selectTrack(trackNamed("Kick").id))
    const deleting = runAction("mixer.deleteTrack")
    await flush()
    const question = usePromptStore.getState().confirm
    expect(question?.description).toContain('The channel "Kick"')
    expect(question?.description).toContain("Its 2 effects go with it.")
    question?.resolve(null)
    await deleting
    expect(tracks()).toHaveLength(5)
  })
})
