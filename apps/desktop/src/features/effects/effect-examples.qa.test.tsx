import { act, fireEvent, render } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import { ValueContextMenus } from "@/components/value-context-menu"
import { effectDescriptor } from "@/features/params"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"

import { EffectEditor } from "./effect-editor"

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

// Import the actual shipped project files. Rust applies legacy defaults and
// checks each file before any editor is mounted; no mock parameter fixtures.
const examples = import.meta.glob<string>(
  "../../../../../docs/examples/dynamics-eq/*.windfall",
  { query: "?raw", import: "default", eager: true }
)

let stop: (() => void) | undefined
afterEach(() => {
  stop?.()
  stop = undefined
  vi.restoreAllMocks()
})

function Editor() {
  const slot = useProjectStore((state) => state.project.mixer.tracks[0].effects[0])
  return slot ? <EffectEditor trackId={0} slot={slot} /> : null
}

const flush = () => act(() => settle())
const slotOf = () => useProjectStore.getState().project.mixer.tracks[0].effects[0]

it("includes all seventeen original self-contained effect projects", () => {
  expect(Object.keys(examples)).toHaveLength(17)
})

it.each(Object.entries(examples))(
  "%s opens with its original patch, mounts editable controls, and preserves edits through history and reopen",
  async (file, text) => {
    const document = SimDocument.open(text)
    const project = structuredClone(document.project())
    document.dispose()
    const app = await startTestApp({ project })
    stop = app.stop
    const original = structuredClone(slotOf().params)
    const descriptor = effectDescriptor(original.type)
    const preferred = original.type === "morphEq" ? "morph"
      : original.type === "panLfo" ? "depth"
      : original.type === "filterBank" ? "slots.0.frequencyHz"
      : null
    const info = preferred
      ? descriptor.params.find((item) => item.id === preferred)
      : descriptor.params.find((item) => item.kind === "float")
    if (!info) throw new Error("The example has no editable continuous control")
    render(<ValueContextMenus><Editor /></ValueContextMenus>)
    await flush()
    const root = window.document.querySelector<HTMLElement>(`[data-param="${info.id}"]`)
    expect(root).toBeVisible()
    const slider = root?.matches("[role=slider]") ? root
      : root?.querySelector<HTMLElement>("[role=slider]")
    if (!slider) throw new Error(`No visible control for ${info.id} in ${file}`)
    const cursor = useProjectStore.getState().history.cursor
    act(() => slider.focus())
    fireEvent.keyDown(slider, { key: "End" })
    fireEvent.keyUp(slider, { key: "End" })
    await flush()
    const edited = structuredClone(slotOf().params)
    expect(edited).not.toEqual(original)
    expect(useProjectStore.getState().history.cursor).toBe(cursor + 1)
    await undo()
    await flush()
    expect(slotOf().params).toEqual(original)
    await redo()
    await flush()
    expect(slotOf().params).toEqual(edited)
    const saved = structuredClone((await app.backend.documentSnapshot()).project)
    const path = await app.backend.projectSave("/projects/original-effect-example.windfall")
    await dispatch({ type: "removeEffect", track: 0, effect: slotOf().id })
    await flush()
    await act(async () => { await app.backend.projectOpen(path) })
    await flush()
    expect(useProjectStore.getState().project).toEqual(saved)
    expect(slotOf().params).toEqual(edited)
    expect(useProjectStore.getState().project.settings.name).toBe(project.settings.name)
  }
)
