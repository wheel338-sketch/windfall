import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { PluginBinding } from "@/bindings"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store"
import { startTestApp } from "@/test/harness"
import { PluginWrapperControls } from "./wrapper-controls"

const fixture: PluginBinding = {
  target: { type: "instrument", channel: 0 },
  format: "clap",
  path: "/plugins/fixture.clap",
  id: "fixture.synth",
  name: "Fixture synth",
  state: [1, 2],
  parameters: [],
}
let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
  await act(async () => {
    await dispatch({ type: "addPluginInstrument", plugin: fixture })
  })
})
afterEach(() => {
  app.stop()
  vi.restoreAllMocks()
})

function Controls() {
  const binding = useProjectStore((state) => state.project.plugins?.[0])
  return binding ? <PluginWrapperControls binding={binding} /> : null
}
const saved = () => useProjectStore.getState().project.plugins![0]
async function choose(label: string, option: string) {
  await userEvent.click(screen.getByRole("combobox", { name: label }))
  await userEvent.click(await screen.findByRole("option", { name: option }))
  await waitFor(() =>
    expect(screen.getByRole("combobox", { name: label })).toBeEnabled()
  )
}

it("saves wrapper edits through document history and restores defaults on undo", async () => {
  render(<Controls />)
  const smart = screen.getByRole("switch", { name: "Smart disable" })
  expect(smart).not.toBeChecked()
  await userEvent.click(smart)
  await waitFor(() => expect(saved().wrapper?.smartDisable).toBe(true))
  await choose("Native buffer", "128 frames")
  expect(saved().wrapper?.fixedBuffer).toBe(128)
  await act(async () => { await undo() })
  expect(saved().wrapper?.fixedBuffer).toBeUndefined()
  expect(saved().wrapper?.smartDisable).toBe(true)
  await act(async () => { await undo() })
  expect(saved().wrapper).toBeUndefined()
  expect(smart).not.toBeChecked()
  await act(async () => { await redo(); await redo() })
  expect(saved().wrapper?.fixedBuffer).toBe(128)
  expect(smart).toBeChecked()
  await choose("Native buffer", "Variable")
  expect(saved().wrapper?.fixedBuffer ?? null).toBeNull()
})

it("retains owner and editor choices in an actual saved and reopened document", async () => {
  render(<Controls />)
  await userEvent.click(screen.getByRole("button", { name: "In process" }))
  await waitFor(() => expect(saved().wrapper?.processing).toBe("inProcess"))
  await choose("Native editor scale", "150%")
  await userEvent.click(screen.getByRole("switch", { name: "Detached editor" }))
  await waitFor(() => expect(saved().wrapper?.detached).toBe(false))
  const document = SimDocument.create((await app.backend.documentSnapshot()).project)
  const reopened = SimDocument.open(document.fileText())
  try {
    expect(reopened.project().plugins![0]).toEqual(saved())
    expect(saved().wrapper).toMatchObject({
      processing: "inProcess", editorScale: 1.5, detached: false,
    })
  } finally {
    reopened.dispose()
    document.dispose()
  }
  const threaded = screen.getByRole("button", { name: "Threaded" })
  expect(threaded).toBeEnabled()
  await userEvent.click(threaded)
  await waitFor(() => expect(saved().wrapper?.processing).toBe("threaded"))
  await act(async () => { await undo() })
  expect(saved().wrapper?.processing).toBe("inProcess")
  await act(async () => { await redo() })
  expect(saved().wrapper?.processing).toBe("threaded")
  const threadedDocument = SimDocument.create((await app.backend.documentSnapshot()).project)
  const threadedReopened = SimDocument.open(threadedDocument.fileText())
  try {
    expect(threadedReopened.project().plugins![0].wrapper?.processing).toBe("threaded")
  } finally {
    threadedReopened.dispose()
    threadedDocument.dispose()
  }
})

it("preserves saved uncommon values while changing a threaded owner policy", async () => {
  await act(async () => {
    await dispatch({
      type: "setPluginWrapper",
      target: saved().target,
      options: {
        smartDisable: true, fixedBuffer: 96, processing: "threaded",
        editorScale: 1.1, detached: true,
      },
    })
  })
  render(<Controls />)
  expect(screen.getByRole("combobox", { name: "Native buffer" })).toHaveTextContent("96 frames (saved)")
  expect(screen.getByRole("combobox", { name: "Native editor scale" })).toHaveTextContent("110")
  expect(screen.getByRole("button", { name: "Threaded" })).toHaveAttribute("aria-pressed", "true")
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Automatic" }))
  await waitFor(() => expect(saved().wrapper?.processing).toBe("automatic"))
  expect(saved().wrapper?.fixedBuffer).toBe(96)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("keeps saved controls unchanged on a failed command and permits retry", async () => {
  render(<Controls />)
  const native = vi.spyOn(app.backend, "dispatch")
  native.mockRejectedValueOnce(new Error("Fixture refused the update"))
  await userEvent.click(screen.getByRole("switch", { name: "Smart disable" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("not changed")
  expect(saved().wrapper).toBeUndefined()
  const smart = screen.getByRole("switch", { name: "Smart disable" })
  expect(smart).not.toBeChecked()
  expect(smart).toBeEnabled()
  await userEvent.click(smart)
  await waitFor(() => expect(saved().wrapper?.smartDisable).toBe(true))
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})
