import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type {
  Command,
  EffectId,
  EffectKind,
  EffectSlot,
  ParamInfo,
  TrackId,
} from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor, paramIndex, readParam } from "@/features/params"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import {
  bandReductionDb,
  effectiveCrossovers,
  frequencyPosition,
  levelPosition,
  levelStageReductionDb,
  masterLimiterLatencyFrames,
  maximizerLatencyFrames,
  oneKnobRecipe,
  transientShape,
} from "./dynamics/curves"
import { customEditor, EffectEditor } from "./effect-editor"
import { historyLabelText } from "@/lib/i18n"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

const KINDS = [
  "multibandCompressor",
  "multibandMaximizer",
  "oneKnob",
  "transientShaper",
  "bassHarmonics",
  "stageStack",
  "masterLimiter",
] as const satisfies readonly EffectKind[]
const METERED: readonly EffectKind[] = [
  "multibandCompressor",
  "multibandMaximizer",
  "oneKnob",
  "stageStack",
  "masterLimiter",
]

let stop: () => void
let backend: MockBackend
let track: TrackId

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  ;({ stop, backend } = await startTestApp())
  track = useProjectStore.getState().project.mixer.tracks[1].id
  vi.spyOn(backend, "dispatch")
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

function Editor({ effect }: { effect: EffectId }) {
  const slot = useProjectStore((state) =>
    state.project.mixer.tracks
      .find((item) => item.id === track)
      ?.effects.find((item) => item.id === effect)
  )
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}

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

function sliderOf(id: string): HTMLElement {
  const root = control(id)
  const slider = root.matches("[role=slider]")
    ? root
    : root.querySelector<HTMLElement>("[role=slider]")
  if (!slider) throw new Error(`"${id}" has no slider`)
  return slider
}

/** Changes a setting through its control and returns the commands sent. */
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
    const button = root.matches("[role=switch], button[aria-pressed]")
      ? root
      : root.querySelector<HTMLElement>("[role=switch], button[aria-pressed]")
    if (!button) throw new Error(`"${info.id}" has no switch`)
    await user.click(button)
  } else {
    const other = info.choices[value === 0 ? 1 : 0].label
    await user.click(within(root).getByRole("radio", { name: other }))
  }
  await flush()
  return sent(before)
}

describe("what the dynamics editors work out for themselves", () => {
  it("constrains the two splits the way the crossover does", () => {
    expect(effectiveCrossovers(200, 2_500, 48_000)).toEqual([200, 2_500])
    // The upper edge stays a quarter above the lower.
    expect(effectiveCrossovers(2_000, 200, 48_000)).toEqual([2_000, 2_500])
    // And both stay under their share of the sample rate.
    const [low, high] = effectiveCrossovers(4_000, 18_000, 8_000)
    expect(low).toBeCloseTo(2_800, 6)
    expect(high).toBeCloseTo(3_600, 6)
    expect(effectiveCrossovers(2_000, 18_000, 8_000)[0]).toBe(2_000)
    expect(frequencyPosition(20)).toBe(0)
    expect(frequencyPosition(20_000)).toBe(1)
    expect(frequencyPosition(632.456)).toBeCloseTo(0.5, 3)
    expect(levelPosition(0)).toBe(1)
    expect(levelPosition(-60)).toBe(0)
    expect(levelPosition(-18)).toBeCloseTo(0.7, 6)
  })

  it("follows the band compressor's hard knee and limiter ratio", () => {
    expect(bandReductionDb(-30, -18, 4)).toBe(0)
    expect(bandReductionDb(-6, -18, 4)).toBeCloseTo(9, 6)
    expect(bandReductionDb(-6, -18, 1)).toBe(0)
    expect(bandReductionDb(0, -24, 100)).toBe(24)
  })

  it("moves every maximizer setting with the one knob, as the engine does", () => {
    expect(oneKnobRecipe(0)).toEqual({
      inputGainDb: 0,
      ceilingDb: -0,
      low: { thresholdDb: -0, ratio: 1, makeupDb: 0 },
      mid: { thresholdDb: -0, ratio: 1, makeupDb: 0 },
      high: { thresholdDb: -0, ratio: 1, makeupDb: 0 },
    })
    expect(oneKnobRecipe(1)).toEqual({
      inputGainDb: 8,
      ceilingDb: -0.3,
      low: { thresholdDb: -24, ratio: 3, makeupDb: 3 },
      mid: { thresholdDb: -18, ratio: 2.5, makeupDb: 2 },
      high: { thresholdDb: -22, ratio: 2, makeupDb: 4 },
    })
    expect(oneKnobRecipe(0.5).mid).toEqual({
      thresholdDb: -9,
      ratio: 1.75,
      makeupDb: 1,
    })
    expect(oneKnobRecipe(7)).toEqual(oneKnobRecipe(1))
  })

  it("draws a hit taller at the front for attack and fuller behind it for sustain", () => {
    const flat = transientShape(0, 0, 21)
    expect(flat.shaped).toEqual(flat.dry)
    expect(flat.dry[0]).toBe(1)
    const punchy = transientShape(1, 0, 21)
    // 18 dB at the very front, nothing once the attack share has run out.
    expect(punchy.shaped[0] / punchy.dry[0]).toBeCloseTo(10 ** (18 / 20), 6)
    expect(punchy.shaped[4]).toBeCloseTo(punchy.dry[4], 9)
    expect(punchy.shaped[20]).toBeCloseTo(punchy.dry[20], 9)
    const long = transientShape(0, -1, 21)
    expect(long.shaped[0]).toBeCloseTo(long.dry[0], 9)
    expect(long.shaped[20] / long.dry[20]).toBeCloseTo(10 ** (-18 / 20), 6)
  })

  it("knows the fixed delays and the level stage's curve", () => {
    expect(masterLimiterLatencyFrames(48_000)).toBe(78)
    expect(masterLimiterLatencyFrames(44_100)).toBe(72)
    expect(masterLimiterLatencyFrames(1)).toBe(7)
    expect(maximizerLatencyFrames(48_000)).toBe(96)
    // 0.6 of every dB over -16 dB RMS at full leveling, past the knee.
    expect(levelStageReductionDb(9, 1)).toBeCloseTo(15, 6)
    expect(levelStageReductionDb(9, 0.5)).toBeCloseTo(7.5, 6)
    expect(levelStageReductionDb(-19, 1)).toBe(0)
    expect(levelStageReductionDb(-16, 1)).toBeCloseTo(0.45, 6)
    expect(levelStageReductionDb(9, 0)).toBe(0)
  })
})

describe("the purpose-built dynamics editors on the real document", () => {
  it.each(KINDS)(
    "%s has its own editor with a control for every setting",
    async (kind) => {
      const descriptor = effectDescriptor(kind)
      const effect = await open(kind)
      expect(customEditor(kind)).toBeDefined()
      const editor = screen.getByRole("group", {
        name: `${descriptor.name} settings`,
      })
      expect(editor).toBeVisible()
      expect(
        editor.querySelector("[data-slot=gain-reduction-meter]") !== null
      ).toBe(METERED.includes(kind))
      // Each setting exactly once: an editor that showed one twice would
      // have two controls fighting over a drag.
      for (const info of descriptor.params) {
        expect(
          editor.querySelectorAll(`[data-param="${info.id}"]`),
          info.id
        ).toHaveLength(1)
      }
      for (const [index, info] of descriptor.params.entries()) {
        const stored = readParam(slotOf(effect).params, info)
        const commands = await operate(info, stored)
        expect(commands.length, info.id).toBeGreaterThan(0)
        for (const command of commands) {
          expect(command, info.id).toMatchObject({
            type: "setEffectParam",
            track,
            effect,
            param: index,
          })
        }
        expect(readParam(slotOf(effect).params, info), info.id).not.toBe(stored)
      }
    }
  )

  it.each(KINDS)(
    "%s edits are one undo step each and survive save and reopen",
    async (kind) => {
      const descriptor = effectDescriptor(kind)
      const effect = await open(kind)
      const info = descriptor.params.find((item) => item.kind === "float")!
      const slider = sliderOf(info.id)
      const original = slotOf(effect).params
      const before = history().cursor
      act(() => slider.focus())
      const key = info.default === info.max ? "ArrowDown" : "ArrowUp"
      for (let step = 0; step < 3; step += 1) {
        fireEvent.keyDown(slider, { key })
        await flush()
      }
      fireEvent.keyUp(slider, { key })
      await flush()
      const edited = slotOf(effect).params
      expect(edited).not.toEqual(original)
      expect(history().cursor).toBe(before + 1)
      expect(historyLabelText(history().entries.at(-1)!.label, "en")).toBe(`Change ${info.name}`)
      await undo()
      await flush()
      expect(slotOf(effect).params).toEqual(original)
      await redo()
      await flush()
      expect(slotOf(effect).params).toEqual(edited)

      const saved = structuredClone(
        (await backend.documentSnapshot()).project
      )
      const path = await backend.projectSave(`/projects/${kind}.windfall`)
      await dispatch({ type: "removeEffect", track, effect })
      await flush()
      await act(async () => {
        await backend.projectOpen(path)
      })
      await flush()
      expect(useProjectStore.getState().project).toEqual(saved)
      expect(slotOf(effect).params).toEqual(edited)
    }
  )

  it.each(KINDS)(
    "%s makes an automation clip from its own control",
    async (kind) => {
      const descriptor = effectDescriptor(kind)
      const effect = await open(kind)
      const info = descriptor.params.find((item) => item.kind === "float")!
      const param = paramIndex(descriptor, info.id)
      const before = history().cursor
      fireEvent.contextMenu(sliderOf(info.id), { clientX: 20, clientY: 20 })
      await userEvent
        .setup()
        .click(
          await screen.findByRole("menuitem", { name: "Create automation clip" })
        )
      await flush()
      expect(
        useProjectStore.getState().project.automations.at(-1)?.target
      ).toEqual({ type: "effectParam", track, effect, param })
      expect(history().cursor).toBe(before + 1)
      await undo()
      await flush()
      expect(useProjectStore.getState().project.automations).toEqual([])
    }
  )

  it("lays the multiband compressor out as three bands that the display describes", async () => {
    const effect = await open("multibandCompressor")
    const editor = within(
      screen.getByRole("group", { name: "Multiband Compressor settings" })
    )
    expect(
      editor.getByRole("img", {
        name: "Three bands split at 200 Hz and 2.50 kHz",
      })
    ).toBeVisible()
    for (const [band, title] of [
      ["low", "Low"],
      ["mid", "Mid"],
      ["high", "High"],
    ]) {
      const group = within(editor.getByRole("region", { name: title }))
      // Five controls, all of this band and of no other.
      const ids = [
        ...group
          .getAllByRole("slider")
          .map((slider) => slider.closest("[data-param]")),
      ].map((element) => element?.getAttribute("data-param"))
      expect(ids).toEqual(
        ["thresholdDb", "ratio", "attackMs", "releaseMs", "makeupDb"].map(
          (id) => `${band}.${id}`
        )
      )
    }
    // Moving a split is shown before the project answers, and stored.
    const split = sliderOf("lowCrossoverHz")
    act(() => split.focus())
    fireEvent.keyDown(split, { key: "End" })
    fireEvent.keyUp(split, { key: "End" })
    await flush()
    expect(slotOf(effect).params).toMatchObject({ lowCrossoverHz: 2_000 })
    expect(
      editor.getByRole("img", {
        name: "Three bands split at 2.00 kHz and 2.50 kHz",
      })
    ).toBeVisible()
  })

  it("gives the maximizer a ceiling for every band and a limiter on the sum", async () => {
    await open("multibandMaximizer")
    const editor = within(
      screen.getByRole("group", { name: "Multiband Maximizer settings" })
    )
    for (const title of ["Low", "Mid", "High"]) {
      expect(
        within(editor.getByRole("region", { name: title })).getByRole(
          "slider",
          { name: `${title} ceiling` }
        )
      ).toBeVisible()
    }
    const output = within(editor.getByRole("region", { name: "Output" }))
    expect(output.getByRole("slider", { name: "Input gain" })).toBeVisible()
    expect(output.getByRole("slider", { name: "Ceiling" })).toBeVisible()
    expect(output.getByRole("slider", { name: "Limiter release" })).toBeVisible()
    expect(
      document.querySelector("[data-slot=maximizer-latency]")
    ).toHaveTextContent("2.0 ms (96 samples)")
  })

  it("keeps the one-knob enhancer to its one knob and says what the knob has set", async () => {
    const effect = await open("oneKnob")
    const editor = within(
      screen.getByRole("group", { name: "One Knob settings" })
    )
    expect(editor.getAllByRole("slider")).toHaveLength(1)
    expect(editor.queryAllByRole("radio")).toHaveLength(0)
    expect(editor.queryAllByRole("switch")).toHaveLength(0)
    const recipe = document.querySelector("[data-slot=one-knob-recipe]")
    expect(recipe).toHaveTextContent(
      "Drive +0.0 dB into three bands at 1.0:1, 1.0:1 and 1.0:1, with a ceiling of 0.0 dB."
    )
    const knob = editor.getByRole("slider", { name: "Amount" })
    act(() => knob.focus())
    fireEvent.keyDown(knob, { key: "End" })
    fireEvent.keyUp(knob, { key: "End" })
    await flush()
    expect(slotOf(effect).params).toMatchObject({ amount: 1 })
    expect(recipe).toHaveTextContent(
      "Drive +8.0 dB into three bands at 3.0:1, 2.5:1 and 2.0:1, with a ceiling of -0.3 dB."
    )
    expect(history().cursor).toBe(2)
  })

  it("lets either part of the transient shaper's output be heard alone", async () => {
    const effect = await open("transientShaper")
    const editor = within(
      screen.getByRole("group", { name: "Transient Shaper settings" })
    )
    expect(slotOf(effect).params).toMatchObject({ listen: "both" })
    expect(
      editor.getByRole("img", { name: "A hit before and after shaping, whole hit" })
    ).toBeVisible()
    const before = history().cursor
    await userEvent
      .setup()
      .click(editor.getByRole("radio", { name: "Attack only" }))
    await flush()
    expect(slotOf(effect).params).toMatchObject({ listen: "attack" })
    expect(history().cursor).toBe(before + 1)
    expect(
      editor.getByRole("img", {
        name: "A hit before and after shaping, attack only",
      })
    ).toBeVisible()
    await userEvent
      .setup()
      .click(editor.getByRole("radio", { name: "Sustain only" }))
    await flush()
    expect(slotOf(effect).params).toMatchObject({ listen: "sustain" })
    await undo()
    await undo()
    await flush()
    expect(slotOf(effect).params).toMatchObject({ listen: "both" })

    const attack = editor.getByRole("slider", { name: "Attack" })
    act(() => attack.focus())
    fireEvent.keyDown(attack, { key: "End" })
    fireEvent.keyUp(attack, { key: "End" })
    await flush()
    expect(
      document.querySelector("[data-slot=transient-amounts]")
    ).toHaveTextContent("Attack +18.0 dB, sustain +0.0 dB.")
  })

  it("names the Mastering Limiter's three stages in signal order and its delay", async () => {
    await open("masterLimiter")
    const editor = within(
      screen.getByRole("group", { name: "Mastering Limiter settings" })
    )
    expect(
      editor.getAllByRole("region").map((region) => region.getAttribute("aria-label"))
    ).toEqual(["1 Level", "2 Color", "3 Peak"])
    expect(
      within(editor.getByRole("region", { name: "3 Peak" })).getByRole(
        "slider",
        { name: "Ceiling" }
      )
    ).toHaveAttribute("aria-valuenow", "-1")
    expect(
      document.querySelector("[data-slot=master-limiter-latency]")
    ).toHaveTextContent("1.6 ms (78 samples)")
    // The short form shows its two stages the same way.
    await open("stageStack")
    const short = within(
      screen.getByRole("group", { name: "Stage Stack settings" })
    )
    expect(
      short.getAllByRole("region").map((region) => region.getAttribute("aria-label"))
    ).toEqual(["1 Saturate", "2 Limit", "Blend"])
  })
})
