// Written, not executed. Uses the real document WASM via the existing harness.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { RackGraph, RackNoteModule } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { targetState } from "@/lib/automation/targets"
import type { MockBackend } from "@/lib/ipc/mock"
import { settle } from "@/test/harness"
import { instrumentChannel, project, startRack } from "../channel-rack/test-utils"
import { ModularRackSection } from "./editor"

let stop: () => void
let files: MockBackend
const NAME = "Module channel"
const library = () => within(screen.getByRole("region", { name: "Rack preset library" }))
const stored = (): RackGraph => {
  const graph = project().settings.modularRacks?.find((rack) => rack.channel === instrumentChannel(NAME).id)?.graph
  if (!graph) throw new Error("Missing saved rack")
  return graph
}
function Mounted() {
  const channel = useProjectStore((state) => state.project.channels.find((channel) => channel.name === NAME))
  return channel ? <ModularRackSection channel={channel} /> : null
}
beforeEach(async () => {
  const app = await startRack(); stop = app.stop; files = app.backend
  await dispatch({ type: "addChannel", name: NAME, instrument: "subtractiveSynth" })
  await act(settle); render(<Mounted />)
  fireEvent.click(screen.getByRole("button", { name: "Create modular rack" })); await act(settle)
  fireEvent.click(screen.getByRole("button", { name: "Edit modular rack" })); await act(settle)
})
afterEach(() => { vi.restoreAllMocks(); stop() })
async function choose(name: string, value: string) {
  const user = userEvent.setup(); await user.click(screen.getByRole("combobox", { name })); await user.click(await screen.findByRole("option", { name: value })); await act(settle)
}
async function add(type: string) {
  await choose("Module type", type); fireEvent.click(screen.getByRole("button", { name: "Add node" })); await act(settle)
}
function editNumber(name: string, value: number) {
  const field = screen.getByRole("spinbutton", { name }); fireEvent.change(field, { target: { value: String(value) } }); fireEvent.blur(field)
}
describe("mounted modular rack", () => {
  it.each([
    ["colorRouter", "Color 0", 0], ["envelope", "Attack", 1],
    ["propertyScaler", "Curve", 1], ["keySplitter", "Boundary key", 1],
    ["keyMapper", "Key 60", 60], ["sequencer", "Gate", 2],
  ] as const)("creates a real saved automation clip from the %s parameter menu", async (type, label, parameter) => {
    await add(type)
    fireEvent.click(screen.getByText("Automation and controller links"))
    const id = stored().nodes.at(-1)!.id
    const address = stored().parameters?.findIndex((address) => address.path.length === 1 && address.path[0] === id && address.parameter === parameter)
    expect(address).toBeGreaterThanOrEqual(0)
    const target = { type: "instrumentParam", channel: instrumentChannel(NAME).id, param: 32 + address! } as const
    expect(targetState(project(), target)?.info?.name).toBe(label)
    const slider = screen.getByRole("slider", { name: label })
    fireEvent.contextMenu(slider, { clientX: 30, clientY: 30 })
    fireEvent.click(await screen.findByRole("menuitem", { name: "Create automation clip" }))
    await waitFor(() => expect(project().automations.at(-1)?.target).toEqual(target))
    expect(project().playlist.clips.some((clip) => clip.content.type === "automation")).toBe(true)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle)
    expect(project().automations).toHaveLength(0)
  })
  it("saves a native-container rack library preset and loads with fresh identities in one undo step", async () => {
    await add("envelope")
    const saved = stored()
    fireEvent.change(screen.getByRole("textbox", { name: "Rack preset name" }), { target: { value: "Soft rack" } })
    fireEvent.click(screen.getByRole("button", { name: "Save rack to library" }))
    await waitFor(() => expect(library().getByRole("status")).toHaveTextContent("Saved Soft rack"))
    fireEvent.click(screen.getByRole("button", { name: "Remove node" })); await act(settle)
    const before = stored()
    fireEvent.click(screen.getByRole("button", { name: "Soft rack" }))
    await waitFor(() => expect(library().getByRole("status")).toHaveTextContent("Rack preset loaded"))
    expect(stored().nodes.map((node) => node.processor)).toEqual(saved.nodes.map((node) => node.processor))
    expect(stored().nodes.every((node) => node.id >= before.nextId)).toBe(true)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle)
    expect(stored()).toEqual(before)
  })
  it("saves and loads a rack file through Backend while preserving channel ownership", async () => {
    await add("keyMapper")
    const source = instrumentChannel(NAME).source
    const saved = stored()
    fireEvent.click(screen.getByRole("button", { name: "Save rack file" }))
    await waitFor(() => expect(library().getByRole("status")).toHaveTextContent("Rack preset file saved"))
    fireEvent.click(screen.getByRole("button", { name: "Remove node" })); await act(settle)
    fireEvent.click(screen.getByRole("button", { name: "Load rack file" }))
    await waitFor(() => expect(library().getByRole("status")).toHaveTextContent("Rack preset loaded"))
    expect(stored().nodes.map((node) => node.processor)).toEqual(saved.nodes.map((node) => node.processor))
    expect(instrumentChannel(NAME).source).toEqual(source)
  })
  it("refuses a rack library result after a document edit and keeps the current graph", async () => {
    await add("envelope")
    fireEvent.click(screen.getByRole("button", { name: "Save rack to library" }))
    await waitFor(() => expect(library().getByRole("status")).toHaveTextContent("Saved"))
    const entry = (await files.presetLibrary())[0]
    const review = await files.presetRead(entry.path)
    let finish: (value: typeof review) => void = () => { throw new Error("Read was not started") }
    vi.spyOn(files, "presetRead").mockImplementationOnce(() => new Promise((resolve) => { finish = resolve }))
    fireEvent.click(screen.getByRole("button", { name: entry.name })); await act(settle)
    await act(async () => { await dispatch({ type: "updateSettings", patch: { tempoBpm: 99 } }); await settle() })
    const before = stored()
    await act(async () => { finish(review); await settle() })
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("project changed"))
    expect(stored()).toEqual(before)
  })
  it.each(["Soft double envelope", "Two octave chord", "Quarter beat pulse"] as const)("loads the original CC0 %s example through the checked document", async (name) => {
    const before = stored()
    fireEvent.click(screen.getByRole("button", { name })); await act(settle)
    expect(stored().nodes.some((node) => node.processor.type === "note")).toBe(true)
    expect(stored().parameters?.length).toBeGreaterThan(0)
    expect(library().getByRole("status")).toHaveTextContent("Rack preset loaded")
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle)
    expect(stored()).toEqual(before)
  })
  it("adds a node, connects a signal wire, disconnects and undoes through the document", async () => {
    const before = stored(); await add("effect"); expect(stored().nodes).toHaveLength(before.nodes.length + 1)
    fireEvent.click(screen.getByRole("button", { name: "Instrument 2 output Audio" }))
    fireEvent.click(screen.getByRole("button", { name: "effect 4 input Audio" })); await act(settle)
    expect(stored().edges.at(-1)).toMatchObject({ from: { node: 2, port: 1 }, to: { node: 4, port: 0 } })
    const connected = stored(); fireEvent.click(screen.getByRole("button", { name: "Disconnect wire 3" })); await act(settle)
    expect(stored().edges).toHaveLength(2)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle); expect(stored()).toEqual(connected)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle); expect(stored()).toEqual(before)
  })
  it("refuses a cycle atomically and reports it in the mounted editor", async () => {
    await add("effect"); const before = stored(); const history = project()
    fireEvent.click(screen.getByRole("button", { name: "effect 4 output Audio" }))
    fireEvent.click(screen.getByRole("button", { name: "effect 4 input Audio" })); await act(settle)
    expect(stored()).toEqual(before); expect(project()).toBe(history)
    expect(screen.getByRole("alert")).toHaveTextContent("Rack edit was refused")
  })
  it("round-trips a nested rack preset with stable identities and wires", async () => {
    await add("rack"); const root = stored()
    fireEvent.click(screen.getByRole("button", { name: "Open nested rack" })); await act(settle)
    fireEvent.click(screen.getByRole("button", { name: "Write preset text" })); await act(settle)
    const preset = screen.getByRole("textbox", { name: "Rack preset" }); if (!(preset instanceof HTMLTextAreaElement)) throw new Error("Expected preset textarea"); const text = preset.value
    expect(JSON.parse(text)).toMatchObject({ schema: "windfall.rack", version: 1, graph: { nextId: 5 } })
    fireEvent.click(screen.getByRole("button", { name: "Load rack preset" })); await act(settle); expect(stored()).toEqual(root)
    fireEvent.click(screen.getByRole("button", { name: "Parent rack" })); await act(settle)
    expect(screen.getByRole("button", { name: "Edit rack 4" })).toBeVisible()
  })
  it("saves mute and solo and makes removal undoable", async () => {
    fireEvent.click(screen.getByRole("button", { name: "Edit Instrument 2" })); await act(settle)
    fireEvent.click(screen.getByRole("checkbox", { name: "Mute node" })); await act(settle)
    fireEvent.click(screen.getByRole("checkbox", { name: "Solo node" })); await act(settle)
    expect(stored().nodes.find((node) => node.id === 2)).toMatchObject({ muted: true, solo: true })
    const before = stored(); fireEvent.click(screen.getByRole("button", { name: "Remove node" })); await act(settle)
    expect(stored().edges).toHaveLength(0)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle); expect(stored()).toEqual(before)
  })
  it.each([
    ["colorRouter", "Color 1 route", 4], ["envelope", "attackMs", 200],
    ["propertyScaler", "Curve", 2], ["keySplitter", "boundary", 64],
    ["keyMapper", "Chord voices", 3], ["sequencer", "Sequence length", 8],
  ] as const)("mounts and saves the distinct %s controls", async (type, label, value) => {
    await add(type); const before = stored(); editNumber(label, value); await act(settle)
    const processor = stored().nodes.at(-1)?.processor
    expect(processor?.type).toBe("note")
    expect(stored()).not.toEqual(before)
    if (processor?.type !== "note") throw new Error("Expected note module")
    expect(processor.module.type).toBe(type satisfies RackNoteModule["type"])
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle); expect(stored()).toEqual(before)
  })
  it("retains the final parameter drag value as one undoable gesture", async () => {
    await add("effect"); const before = stored()
    await choose("Effect module", "balance")
    await add("effect")
    const initial = stored(); const cutoff = screen.getByRole("slider", { name: "Gain" })
    fireEvent.pointerDown(cutoff, { pointerId: 1, button: 0, clientY: 300 })
    for (const clientY of [310, 340, 380, 420]) fireEvent.pointerMove(cutoff, { pointerId: 1, clientY })
    const finalValue = Number(cutoff.getAttribute("aria-valuenow"))
    fireEvent.pointerUp(cutoff, { pointerId: 1 }); await act(settle)
    expect(stored()).not.toEqual(initial)
    expect(stored().nodes.at(-1)?.processor).toMatchObject({ type: "effect", params: { type: "balance", gain: finalValue } })
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await act(settle); expect(stored()).toEqual(initial)
    expect(before.nodes).toHaveLength(4)
  })
  it("exposes held-chord sequence modes and persists the selection", async () => {
    await add("sequencer"); await choose("Sequence mode", "down")
    expect(stored().nodes.at(-1)?.processor).toMatchObject({ type: "note", module: { type: "sequencer", params: { mode: "down" } } })
  })
})
