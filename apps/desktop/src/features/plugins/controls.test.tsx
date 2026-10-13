import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"
import type { PluginBinding } from "@/bindings"
import { SimDocument } from "@/lib/ipc/sim/document"
import { dispatch, undo, redo, useProjectStore } from "@/lib/store"
import { startTestApp } from "@/test/harness"
import { PluginControls } from "./controls"
let app: Awaited<ReturnType<typeof startTestApp>>
const fixture: PluginBinding = {
  target: { type: "instrument", channel: 0 }, format: "clap", path: "/fixture.clap", id: "fixture.dynamic", name: "Dynamic", state: [],
  parameters: [{ id: 31, name: "Returning control", min: 0, max: 1, value: 0.5, stepped: false, readOnly: false, automatable: true }],
  capabilities: { unavailableParameters: [], outputs: [{ index: 1, name: "Extra mono", channels: 1 }], routes: [] },
}
beforeEach(async () => { app = await startTestApp(); await act(async () => { await dispatch({ type: "addPluginInstrument", plugin: fixture }) }) })
afterEach(() => app.stop())
const saved = () => useProjectStore.getState().project.plugins![0]
function Controls() { const binding = useProjectStore((state) => state.project.plugins?.[0]); return binding ? <PluginControls binding={binding} /> : null }
it("routes a native bus through checked history and actual file reopen", async () => {
  render(<Controls />)
  await userEvent.click(screen.getByRole("combobox", { name: "Route plugin output 2" }))
  await userEvent.click(await screen.findByRole("option", { name: "Master" }))
  await waitFor(() => expect(saved().capabilities?.routes).toEqual([{ output: 1, track: 0 }]))
  await act(async () => { await undo() }); expect(saved().capabilities?.routes ?? []).toEqual([])
  await act(async () => { await redo() });
  const document = SimDocument.create((await app.backend.documentSnapshot()).project)
  const reopened = SimDocument.open(document.fileText())
  try { expect(reopened.project().plugins![0].capabilities).toEqual(saved().capabilities) } finally { reopened.dispose(); document.dispose() }
})
it("shows retained stale automation addresses and lets a missing output be disconnected", async () => {
  render(<Controls />)
  await act(async () => {
    await dispatch({ type: "setPluginOutputRoute", target: saved().target, output: 1, track: 0 })
    await dispatch({ type: "readmitPlugin", target: saved().target, state: [], parameters: [], auxiliaryInputs: [], outputs: [] })
  })
  expect(await screen.findByText(/Parameter 31 is unavailable/)).toHaveTextContent("automation and controller links are retained")
  expect(screen.getByText(/Saved output 2 to mixer track 0 is unavailable/)).toBeVisible()
  expect(screen.getByRole("spinbutton", { name: "Returning control" })).toBeDisabled()
  await userEvent.click(screen.getByRole("combobox", { name: "Route plugin output 2" }))
  await userEvent.click(await screen.findByRole("option", { name: "Disconnected" }))
  await waitFor(() => expect(saved().capabilities?.routes ?? []).toEqual([]))
  await act(async () => { await dispatch({ type: "readmitPlugin", target: saved().target, state: [], parameters: fixture.parameters, auxiliaryInputs: [], outputs: fixture.capabilities!.outputs }) })
  expect(saved().parameters.map((parameter) => parameter.id)).toEqual([31])
  expect(screen.getByRole("spinbutton", { name: "Returning control" })).toBeEnabled()
  expect(screen.queryByText(/Parameter 31 is unavailable/)).not.toBeInTheDocument()
})
