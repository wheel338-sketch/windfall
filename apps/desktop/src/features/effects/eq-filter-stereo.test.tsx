import { act, fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EffectId, EffectKind, EffectSlot, TrackId } from "@/bindings"
import { formatDb, formatHz } from "@/components/audio"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor, paramIndex, readParam } from "@/features/params"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import { customEditor, EffectEditor } from "./effect-editor"
import { SEVEN_BAND_HZ, SEVEN_BAND_IDS } from "./graphic/seven-band"

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
