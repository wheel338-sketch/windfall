import { describe, expect, it } from "vitest"
import { effectDescriptor, instrumentDescriptor } from "@/features/params"
import { exampleRack, importRackGraph, RACK_EXAMPLES } from "./examples"
import { AUDIO_OUT, AUX_AUDIO_BASE, ports } from "./model"

describe("rack preset identity and ports", () => {
  it("imports root identities and every endpoint without changing processor state", () => {
    const graph = exampleRack(RACK_EXAMPLES[1], instrumentDescriptor("subtractiveSynth").defaults)
    const imported = importRackGraph(graph, 100)
    expect(imported.nodes.map((node) => node.id)).toEqual([100, 101, 102, 103])
    expect(imported.nextId).toBe(104)
    expect(imported.nodes.map((node) => node.processor)).toEqual(graph.nodes.map((node) => node.processor))
    expect(imported.edges.map((edge) => [edge.from.node, edge.to.node])).toEqual([[101, 102], [100, 103], [103, 101]])
    expect(imported.parameters).toEqual([])
  })
  it("refuses exhausted IDs and broken preset endpoints", () => {
    const graph = exampleRack(RACK_EXAMPLES[0], instrumentDescriptor("subtractiveSynth").defaults)
    expect(() => importRackGraph(graph, 0xffffffff)).toThrow("identity space")
    graph.edges[0].from.node = 999
    expect(() => importRackGraph(graph, 10)).toThrow("missing node")
  })
  it("shows all advertised auxiliary audio outputs with stable appended ports", () => {
    const found = ports({ type: "effect", params: effectDescriptor("bandSplit").defaults, mix: 1 }, true)
    expect(found.map((port) => port.id)).toEqual([AUDIO_OUT, AUX_AUDIO_BASE, AUX_AUDIO_BASE + 1, AUX_AUDIO_BASE + 2])
    expect(found.every((port) => port.signal === "audio")).toBe(true)
  })
})
