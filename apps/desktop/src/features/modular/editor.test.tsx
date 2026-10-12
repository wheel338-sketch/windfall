// Written, not executed. Uses the real document WASM via the existing harness.
import { fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import type { RackGraph, RackNoteModule } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { settle } from "@/test/harness"
import { instrumentChannel, project, startRack } from "../channel-rack/test-utils"
import { ModularRackSection } from "./editor"

let stop: () => void
const NAME = "Module channel"
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
  const app = await startRack(); stop = app.stop
  await dispatch({ type: "addChannel", name: NAME, instrument: "subtractiveSynth" })
  await settle(); render(<Mounted />)
  fireEvent.click(screen.getByRole("button", { name: "Create modular rack" })); await settle()
  fireEvent.click(screen.getByRole("button", { name: "Edit modular rack" })); await settle()
})
afterEach(() => stop())
async function choose(name: string, value: string) {
  const user = userEvent.setup(); await user.click(screen.getByRole("combobox", { name })); await user.click(await screen.findByRole("option", { name: value, exact: true })); await settle()
}
async function add(type: string) {
  await choose("Module type", type); fireEvent.click(screen.getByRole("button", { name: "Add node" })); await settle()
}
function editNumber(name: string, value: number) {
  const field = screen.getByRole("spinbutton", { name }); fireEvent.change(field, { target: { value: String(value) } }); fireEvent.blur(field)
}
describe("mounted modular rack", () => {
  it("adds a node, connects a signal wire, disconnects and undoes through the document", async () => {
    const before = stored(); await add("effect"); expect(stored().nodes).toHaveLength(before.nodes.length + 1)
    fireEvent.click(screen.getByRole("button", { name: "Instrument 2 output Audio" }))
    fireEvent.click(screen.getByRole("button", { name: "effect 4 input Audio" })); await settle()
    expect(stored().edges.at(-1)).toMatchObject({ from: { node: 2, port: 1 }, to: { node: 4, port: 0 } })
    const connected = stored(); fireEvent.click(screen.getByRole("button", { name: "Disconnect wire 3" })); await settle()
    expect(stored().edges).toHaveLength(2)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle(); expect(stored()).toEqual(connected)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle()
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle(); expect(stored()).toEqual(before)
  })
  it("refuses a cycle atomically and reports it in the mounted editor", async () => {
    await add("effect"); const before = stored(); const history = project()
    fireEvent.click(screen.getByRole("button", { name: "effect 4 output Audio" }))
    fireEvent.click(screen.getByRole("button", { name: "effect 4 input Audio" })); await settle()
    expect(stored()).toEqual(before); expect(project()).toBe(history)
    expect(screen.getByRole("alert")).toHaveTextContent("Rack edit was refused")
  })
  it("round-trips a nested rack preset with stable identities and wires", async () => {
    await add("rack"); const root = stored()
    fireEvent.click(screen.getByRole("button", { name: "Open nested rack" })); await settle()
    fireEvent.click(screen.getByRole("button", { name: "Write preset text" })); await settle()
    const preset = screen.getByRole("textbox", { name: "Rack preset" }); if (!(preset instanceof HTMLTextAreaElement)) throw new Error("Expected preset textarea"); const text = preset.value
    expect(JSON.parse(text)).toMatchObject({ schema: "windfall.rack", version: 1, graph: { nextId: 5 } })
    fireEvent.click(screen.getByRole("button", { name: "Load rack preset" })); await settle(); expect(stored()).toEqual(root)
    fireEvent.click(screen.getByRole("button", { name: "Parent rack" })); await settle()
    expect(screen.getByRole("button", { name: "Edit rack 4" })).toBeVisible()
  })
  it("saves mute and solo and makes removal undoable", async () => {
    fireEvent.click(screen.getByRole("button", { name: "Edit Instrument 2" })); await settle()
    fireEvent.click(screen.getByRole("checkbox", { name: "Mute node" })); await settle()
    fireEvent.click(screen.getByRole("checkbox", { name: "Solo node" })); await settle()
    expect(stored().nodes.find((node) => node.id === 2)).toMatchObject({ muted: true, solo: true })
    const before = stored(); fireEvent.click(screen.getByRole("button", { name: "Remove node" })); await settle()
    expect(stored().edges).toHaveLength(0)
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle(); expect(stored()).toEqual(before)
  })
  it.each([
    ["colorRouter", "Color 1 route", 4], ["envelope", "attackMs", 200],
    ["propertyScaler", "Curve", 2], ["keySplitter", "boundary", 64],
    ["keyMapper", "Chord voices", 3], ["sequencer", "Sequence length", 8],
  ] as const)("mounts and saves the distinct %s controls", async (type, label, value) => {
    await add(type); const before = stored(); editNumber(label, value); await settle()
    const processor = stored().nodes.at(-1)?.processor
    expect(processor?.type).toBe("note")
    expect(stored()).not.toEqual(before)
    if (processor?.type !== "note") throw new Error("Expected note module")
    expect(processor.module.type).toBe(type satisfies RackNoteModule["type"])
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle(); expect(stored()).toEqual(before)
  })
  it("retains the final parameter drag value as one undoable gesture", async () => {
    await add("effect"); const before = stored()
    await choose("Effect module", "balance")
    await add("effect")
    const initial = stored(); const cutoff = screen.getByRole("slider", { name: "Gain" })
    fireEvent.pointerDown(cutoff, { pointerId: 1, button: 0, clientY: 300 })
    for (const clientY of [310, 340, 380, 420]) fireEvent.pointerMove(cutoff, { pointerId: 1, clientY })
    const finalValue = Number(cutoff.getAttribute("aria-valuenow"))
    fireEvent.pointerUp(cutoff, { pointerId: 1 }); await settle()
    expect(stored()).not.toEqual(initial)
    expect(stored().nodes.at(-1)?.processor).toMatchObject({ type: "effect", params: { type: "balance", gain: finalValue } })
    fireEvent.click(screen.getByRole("button", { name: "Undo rack edit" })); await settle(); expect(stored()).toEqual(initial)
    expect(before.nodes).toHaveLength(4)
  })
  it("exposes held-chord sequence modes and persists the selection", async () => {
    await add("sequencer"); await choose("Sequence mode", "down")
    expect(stored().nodes.at(-1)?.processor).toMatchObject({ type: "note", module: { type: "sequencer", params: { mode: "down" } } })
  })
})
