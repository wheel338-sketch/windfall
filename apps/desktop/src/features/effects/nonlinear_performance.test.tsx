import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { ReactNode } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { EffectKind } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import MixerPanel from "@/features/mixer"
import { chain, kinds } from "@/features/mixer/effect-test-utils"
import { useEffectsUi } from "@/features/mixer/effects-ui"
import {
  flush,
  history,
  project,
  strip,
  stubCanvas,
  trackNamed,
} from "@/features/mixer/test-utils"
import { effectDescriptor, readParam } from "@/features/params"
import { registry, runAction } from "@/lib/actions"
import { SimDocument } from "@/lib/ipc/sim/document"
import { emptyProject } from "@/lib/ipc/sim/project"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"

// jsdom has no layout. Only the resize surface is substituted: descriptors,
// controls, actions, stores and the Rust WASM document are the real code.
vi.mock("@/components/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizablePanel: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
  ResizableHandle: () => <div role="separator" />,
}))
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}))

const KINDS = ["overdrive", "waveshaper", "guitarRack", "driveChain", "volumeGate", "timeTransport", "performanceRack", "scratch"] as const satisfies readonly EffectKind[]
let app: Awaited<ReturnType<typeof startTestApp>>

beforeEach(async () => {
  stubCanvas()
  useEffectsUi.setState(useEffectsUi.getInitialState(), true)
  app = await startTestApp()
})
afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

async function open(kind: (typeof KINDS)[number]) {
  const user = userEvent.setup()
  render(
    <ValueContextMenus>
      <MixerPanel />
    </ValueContextMenus>
  )
  const descriptor = effectDescriptor(kind)
  expect(registry.get(`mixer.addEffect.${kind}`)).toBeDefined()
  expect(registry.get(`mixer.replaceEffect.${kind}`)).toBeDefined()
  await user.click(
    within(strip("Clap")).getByRole("button", { name: "Add effect" })
  )
  await user.click(
    await screen.findByRole("menuitem", { name: descriptor.name })
  )
  await flush()
  expect(kinds("Clap")).toEqual([kind])
  expect(chain("Clap")[0]).toMatchObject({
    enabled: true,
    mix: 1,
    params: descriptor.defaults,
  })
  expect(history().cursor).toBe(1)
  return {
    user,
    descriptor,
    editor: within(
      screen.getByRole("group", { name: `${descriptor.name} settings` })
    ),
  }
}

describe("nonlinear and buffered effects with the shared Rust WASM document", () => {
  it.each(KINDS)(
    "%s retains actual mixer mix/bypass/copy settings and replaces with a fresh processor",
    async (kind) => {
      const { descriptor, editor } = await open(kind)
      const original = chain("Clap")[0].id
      const info = descriptor.params.find((info) => info.kind === "float" && editor.queryByRole("slider", { name: info.name }))!
      const control = editor.getByRole("slider", { name: info.name })
      const key = info.default === info.max ? "Home" : "End"
      fireEvent.keyDown(control, { key })
      fireEvent.keyUp(control, { key })
      const mix = screen.getByRole("slider", {
        name: `${descriptor.name} dry/wet mix`,
      })
      fireEvent.keyDown(mix, { key: "Home" })
      fireEvent.keyUp(mix, { key: "Home" })
      await flush()
      expect(chain("Clap")[0].mix).toBe(0)
      const before = structuredClone(chain("Clap")[0])
      await runAction("mixer.bypassEffect")
      await flush()
      expect(chain("Clap")[0].enabled).toBe(false)
      await undo()
      await flush()
      expect(chain("Clap")[0]).toEqual(before)
      await runAction("mixer.duplicateEffect")
      await flush()
      expect(chain("Clap")).toHaveLength(2)
      expect(chain("Clap")[1].id).not.toBe(original)
      expect(chain("Clap")[1]).toMatchObject({
        params: before.params,
        mix: 0,
        enabled: true,
      })
      await undo()
      await flush()
      expect(chain("Clap")).toEqual([before])
      act(() => useEffectsUi.getState().selectEffect(original))
      // The action deliberately disables replacement with the current kind.
      const sameCursor = history().cursor
      await runAction(`mixer.replaceEffect.${kind}`)
      await flush()
      expect(chain("Clap")).toEqual([before])
      expect(history().cursor).toBe(sameCursor)
      const replacement = "balance"
      await runAction(`mixer.replaceEffect.${replacement}`)
      await flush()
      expect(kinds("Clap")).toEqual([replacement])
      expect(chain("Clap")[0].id).not.toBe(original)
      expect(chain("Clap")[0].params).toEqual(
        effectDescriptor(replacement).defaults
      )
      expect(
        screen.getByRole("group", {
          name: `${effectDescriptor(replacement).name} settings`,
        })
      ).toBeVisible()
      await undo()
      await flush()
      expect(chain("Clap")).toEqual([before])
      await redo()
      await flush()
      act(() => useEffectsUi.getState().selectEffect(chain("Clap")[0].id))
      await runAction(`mixer.replaceEffect.${kind}`)
      await flush()
      expect(kinds("Clap")).toEqual([kind])
      expect(chain("Clap")[0].id).not.toBe(original)
      expect(chain("Clap")[0].params).toEqual(descriptor.defaults)
    }
  )

  it("draws a saved waveshaper curve through actual commands and exact undo", async () => {
    const { editor } = await open("waveshaper")
    const before = structuredClone(chain("Clap")[0].params)
    const cursor = history().cursor
    fireEvent.keyDown(editor.getByRole("application", { name: "Transfer curve drawing" }), { key: "ArrowUp" })
    await flush()
    const changed = structuredClone(chain("Clap")[0].params)
    if (changed.type !== "waveshaper") throw new Error("expected waveshaper")
    expect(changed.shape).toHaveLength(33)
    expect(changed.shape[0].y).toBe(0.01)
    expect(history().cursor).toBe(cursor+1)
    await undo(); await flush(); expect(chain("Clap")[0].params).toEqual(before)
    await redo(); await flush(); expect(chain("Clap")[0].params).toEqual(changed)
    const saved = (await app.backend.documentSnapshot()).project
    const path = await app.backend.projectSave("/projects/drawn-transfer.windfall")
    await act(async () => { await app.backend.projectOpen(path) }); await flush()
    expect(project()).toEqual(saved)
  })

  it("steers both performance axes as one saved undo gesture", async () => {
    const { editor } = await open("performanceRack")
    const before = structuredClone(chain("Clap")[0].params)
    const cursor = history().cursor
    fireEvent.keyDown(editor.getByRole("application", { name: "Performance XY pad" }), { key: "ArrowLeft" })
    await flush()
    expect(chain("Clap")[0].params).toMatchObject({ xyEnabled: true, x: 0.99, y: 0.5 })
    expect(history().cursor).toBe(cursor+1)
    await undo(); await flush(); expect(chain("Clap")[0].params).toEqual(before)
    await redo(); await flush()
    expect(chain("Clap")[0].params).toMatchObject({ xyEnabled: true, x: 0.99 })
  })

  it("chooses a loaded project scratch source and retains it through history", async () => {
    const { editor, user } = await open("scratch")
    const sample = project().samples[0]
    const before = structuredClone(chain("Clap")[0].params)
    await user.click(editor.getByRole("combobox", { name: "Scratch source" }))
    await user.click(await screen.findByRole("option", { name: sample.name }))
    await flush()
    expect(chain("Clap")[0].params).toMatchObject({ sourceId: sample.id })
    await undo(); await flush(); expect(chain("Clap")[0].params).toEqual(before)
    await redo(); await flush(); expect(chain("Clap")[0].params).toMatchObject({ sourceId: sample.id })
  })

  it.each(["volumeGate", "timeTransport"] as const)("%s exposes an undoable persisted MIDI-linkable trigger", async (kind) => {
    const { editor, user } = await open(kind)
    const before = structuredClone(chain("Clap")[0].params)
    await user.click(editor.getByRole("button", { name: "Trigger", exact: true }))
    await flush()
    expect(chain("Clap")[0].params).toMatchObject({ trigger: true })
    await undo(); await flush(); expect(chain("Clap")[0].params).toEqual(before)
    await redo(); await flush()
    const saved = (await app.backend.documentSnapshot()).project
    const path = await app.backend.projectSave(`/projects/${kind}-trigger.windfall`)
    await act(async () => { await app.backend.projectOpen(path) }); await flush()
    expect(project()).toEqual(saved)
    expect(chain("Clap")[0].params).toMatchObject({ trigger: true })
  })

  it.each(KINDS)(
    "%s resolves every automation index and defaults through actual WASM",
    (kind) => {
      const doc = SimDocument.create(emptyProject())
      let reopened: SimDocument | undefined
      try {
        const descriptor = effectDescriptor(kind)
        const effect = doc.dispatch({ type: "addEffect", track: 0, kind })
          .created[0]
        const settings = () => doc.project().mixer.tracks[0].effects[0].params
        expect(settings()).toEqual(descriptor.defaults)
        for (const [param, info] of descriptor.params.entries()) {
          const value = info.default === info.max ? info.min : info.max
          const before = structuredClone(settings())
          doc.dispatch({
            type: "setEffectParam",
            track: 0,
            effect,
            param,
            value,
          })
          expect(readParam(settings(), info)).toBe(value)
          doc.undo()
          expect(settings()).toEqual(before)
          doc.redo()
          expect(readParam(settings(), info)).toBe(value)
          doc.dispatch({
            type: "addAutomation",
            target: { type: "effectParam", track: 0, effect, param },
          })
          expect(doc.project().automations.at(-1)?.target).toEqual({
            type: "effectParam",
            track: 0,
            effect,
            param,
          })
        }
        const before = structuredClone(doc.project())
        const cursor = doc.snapshot(null).history.cursor
        expect(() =>
          doc.dispatch({
            type: "setEffectParam",
            track: 0,
            effect,
            param: descriptor.params.length,
            value: 0,
          })
        ).toThrow()
        expect(doc.project()).toEqual(before)
        expect(doc.snapshot(null).history.cursor).toBe(cursor)
        reopened = SimDocument.open(doc.fileText())
        expect(reopened.project()).toEqual(before)
      } finally {
        reopened?.dispose()
        doc.dispose()
      }
    }
  )
})

