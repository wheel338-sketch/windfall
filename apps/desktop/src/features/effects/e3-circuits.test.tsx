// Written, not executed. Requires regenerated descriptors and Rust simulator.
import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { EffectId, EffectKind, EffectSlot, TrackId } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor, paramIndex, readParam } from "@/features/params"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"
import { customEditor, EffectEditor } from "./effect-editor"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }) }))

const CASES = [
  { kind: "room", id: "size", value: 1, slider: "Size" },
  { kind: "lushSpace", id: "pitchAmount", value: 1, slider: "Pitch amount" },
  { kind: "vintageChorus", id: "mode", value: 1, group: "Mode", choice: "I" },
  { kind: "vintagePhaser", id: "stages", value: 1, group: "Stages", choice: "4" },
  { kind: "hyperChorus", id: "voices", value: 16, slider: "Voices" },
  { kind: "stackedFlanger", id: "order", value: 4, slider: "Order" },
] as const satisfies readonly { kind: EffectKind, id: string, value: number, slider?: string, group?: string, choice?: string }[]

let app: Awaited<ReturnType<typeof startTestApp>>
let track: TrackId
const flush = () => act(() => settle())

beforeEach(async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
  app = await startTestApp()
  track = useProjectStore.getState().project.mixer.tracks[1].id
})
afterEach(() => { app.stop(); vi.restoreAllMocks() })

function slotOf(effect: EffectId): EffectSlot {
  const slot = useProjectStore.getState().project.mixer.tracks.find(t => t.id === track)?.effects.find(s => s.id === effect)
  if (!slot) throw new Error(`Missing effect ${effect}`)
  return slot
}
function Editor({ effect }: { effect: EffectId }) {
  const slot = useProjectStore(s => s.project.mixer.tracks.find(t => t.id === track)?.effects.find(e => e.id === effect))
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}
async function open(kind: EffectKind) {
  const added = await dispatch({ type: "addEffect", track, kind })
  if (!added) throw new Error(`Could not add ${kind}`)
  const effect = added.created[0]
  render(<ValueContextMenus><Editor effect={effect} /></ValueContextMenus>)
  await flush()
  return effect
}

describe("E3 circuit editors mounted with the real Rust document", () => {
  it.each(CASES)("$kind exposes every control and persists its distinct circuit with history", async (item) => {
    expect(customEditor(item.kind)).toBeDefined()
    const effect = await open(item.kind)
    const descriptor = effectDescriptor(item.kind)
    for (const info of descriptor.params) {
      expect(document.querySelector(`[data-param="${info.id}"]`), info.id).not.toBeNull()
    }
    const before = structuredClone(slotOf(effect).params)
    const cursor = useProjectStore.getState().history.cursor
    const spy = vi.spyOn(app.backend, "dispatch")
    if ("slider" in item) {
      const control = screen.getByRole("slider", { name: item.slider })
      fireEvent.keyDown(control, { key: "End" })
      fireEvent.keyUp(control, { key: "End" })
    } else {
      await userEvent.setup().click(within(screen.getByRole("radiogroup", { name: item.group })).getByRole("radio", { name: item.choice }))
    }
    await flush()
    const info = descriptor.params[paramIndex(descriptor, item.id)]
    expect(readParam(slotOf(effect).params, info)).toBe(item.value)
    expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
    expect(spy.mock.calls.at(-1)?.[0]).toMatchObject({ type: "setEffectParam", track, effect, param: paramIndex(descriptor, item.id), value: item.value })
    const changed = structuredClone(slotOf(effect).params)
    await undo(); await flush()
    expect(slotOf(effect).params).toEqual(before)
    await redo(); await flush()
    expect(slotOf(effect).params).toEqual(changed)
    const path = await app.backend.projectSave(`/projects/${item.kind}-circuit.windfall`)
    await act(async () => { await app.backend.projectOpen(path) })
    await flush()
    expect(slotOf(effect).params).toEqual(changed)
  })

  it.each(CASES)("$kind creates automation from an actual mounted control", async ({ kind }) => {
    const effect = await open(kind)
    const descriptor = effectDescriptor(kind)
    const info = descriptor.params.find(p => p.id === "mix")
    if (!info) throw new Error("Missing mix descriptor")
    fireEvent.contextMenu(screen.getByRole("slider", { name: info.name }))
    await userEvent.setup().click(await screen.findByRole("menuitem", { name: "Create automation clip" }))
    await flush()
    expect(useProjectStore.getState().project.automations.at(-1)?.target).toEqual({ type: "effectParam", track, effect, param: paramIndex(descriptor, "mix") })
  })

  it("bucket models disable the free controls and restore them on returning to Free", async () => {
    await open("vintageChorus")
    const user = userEvent.setup()
    const modes = within(screen.getByRole("radiogroup", { name: "Mode" }))
    await user.click(modes.getByRole("radio", { name: "I" })); await flush()
    expect(screen.getByRole("slider", { name: "Rate" })).toHaveAttribute("aria-disabled", "true")
    expect(document.querySelector('[data-slot="bucket-mode"]')).toHaveTextContent("clocked bucket")
    await user.click(modes.getByRole("radio", { name: "Free" })); await flush()
    expect(screen.getByRole("slider", { name: "Rate" })).not.toHaveAttribute("aria-disabled", "true")
  })

  it.each(["vintageChorus", "vintagePhaser", "hyperChorus", "stackedFlanger"] as const)("%s switches free rate to a beat cycle", async (kind) => {
    await open(kind)
    await userEvent.setup().click(screen.getByRole("button", { name: "Tempo sync" }))
    await flush()
    expect(screen.getByRole("slider", { name: "Cycle (beats)" })).not.toHaveAttribute("aria-disabled", "true")
    expect(screen.getByRole("slider", { name: kind === "stackedFlanger" ? "First rate" : "Rate" })).toHaveAttribute("aria-disabled", "true")
  })
})
