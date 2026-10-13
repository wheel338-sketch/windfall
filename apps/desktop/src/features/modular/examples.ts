// SPDX-License-Identifier: CC0-1.0
import type { InstrumentParams, RackGraph, RackNoteModule } from "@/bindings"
import { defaultModule, initialGraph, node, NOTE_IN, NOTE_OUT } from "./model"
import { translatedText as t } from "@/lib/i18n"

/** Original Windfall example racks, dedicated to the public domain under
 * CC0 1.0. No external presets, samples, artwork or processor names. */
export const RACK_EXAMPLES = ["Soft double envelope", "Two octave chord", "Quarter beat pulse"] as const
export function exampleRack(name: typeof RACK_EXAMPLES[number], instrument: InstrumentParams): RackGraph {
  const graph = initialGraph(instrument)
  const modules: RackNoteModule[] = name === "Soft double envelope"
    ? [
      { type: "envelope", params: { destination: "gain", attackMs: 80, decayMs: 150, sustain: 0.8, releaseMs: 200, amount: 1 } },
      { type: "envelope", params: { destination: "gain", attackMs: 160, decayMs: 300, sustain: 0.9, releaseMs: 400, amount: 0.5 } },
    ] : name === "Two octave chord"
      ? [{ type: "keyMapper", params: { table: Array.from({ length: 128 }, (_, key) => key) }, chord: [0, 12, 24, 0], voices: 3 }]
      : [defaultModule("sequencer")]
  graph.edges = graph.edges.filter((edge) => edge.from.node !== 1)
  for (const [index, module] of modules.entries()) {
    const id = 4 + index
    graph.nodes.push(node(id, module.type, { type: "note", module }, [20 + index * 260, 200]))
    graph.edges.push({ from: { node: index === 0 ? 1 : id - 1, port: NOTE_OUT }, to: { node: id, port: NOTE_IN }, gain: 1 })
  }
  graph.edges.push({ from: { node: 3 + modules.length, port: NOTE_OUT }, to: { node: 2, port: NOTE_IN }, gain: 1 })
  graph.nextId = 4 + modules.length
  return graph
}

/** File/library loads allocate a fresh top-level namespace so a destination's
 * existing automation never attaches to coincidentally equal preset node IDs. */
export function importRackGraph(graph: RackGraph, nextId: number): RackGraph {
  const first = Math.max(1, nextId)
  if (!Number.isSafeInteger(first) || first + graph.nodes.length > 0xffffffff) throw new Error(t("Rack node identity space is exhausted"))
  const ids = new Map(graph.nodes.map((node, i) => [node.id, first + i]))
  const endpoint = (node: number): number => {
    const id = ids.get(node)
    if (id === undefined) throw new Error(t("Preset wire refers to a missing node"))
    return id
  }
  return { ...graph, parameters: [], nextId: first + graph.nodes.length,
    nodes: graph.nodes.map((node) => ({ ...node, id: endpoint(node.id) })),
    edges: graph.edges.map((edge) => ({ ...edge, from: { ...edge.from, node: endpoint(edge.from.node) }, to: { ...edge.to, node: endpoint(edge.to.node) } })),
  }
}
