import type { InstrumentParams, RackEndpoint, RackGraph, RackNode, RackNoteModule, RackProcessor } from "@/bindings"
import descriptors from "@/bindings/descriptors.json"

export const AUDIO_IN = 0
export const AUDIO_OUT = 1
export const NOTE_IN = 2
export const NOTE_OUT = 3
export const CONTROL_IN = 4
export const CONTROL_OUT = 5
export const NOTE_CHANNEL_BASE = 16
export const AUX_AUDIO_BASE = 32

export function node(id: number, name: string, processor: RackProcessor, position: [number, number]): RackNode {
  return { id, name, processor, position, muted: false, solo: false }
}

export function initialGraph(params: InstrumentParams): RackGraph {
  return { nextId: 4, nodes: [
    node(1, "Notes", { type: "noteInput" }, [20, 40]),
    node(2, "Instrument", { type: "instrument", params }, [280, 40]),
    node(3, "Output", { type: "audioOutput" }, [540, 40]),
  ], edges: [
    { from: { node: 1, port: NOTE_OUT }, to: { node: 2, port: NOTE_IN }, gain: 1 },
    { from: { node: 2, port: AUDIO_OUT }, to: { node: 3, port: AUDIO_IN }, gain: 1 },
  ] }
}

export function nestedGraph(): RackGraph {
  return { nextId: 5, nodes: [
    node(1, "Audio in", { type: "audioInput" }, [20, 40]),
    node(2, "Audio out", { type: "audioOutput" }, [400, 40]),
    node(3, "Notes in", { type: "noteInput" }, [20, 200]),
    node(4, "Notes out", { type: "noteOutput" }, [400, 200]),
  ], edges: [
    { from: { node: 1, port: AUDIO_OUT }, to: { node: 2, port: AUDIO_IN }, gain: 1 },
    { from: { node: 3, port: NOTE_OUT }, to: { node: 4, port: NOTE_IN }, gain: 1 },
  ] }
}

export function defaultModule(type: RackNoteModule["type"]): RackNoteModule {
  switch (type) {
    case "colorRouter": return { type, params: { table: Array.from({ length: 16 }, (_, index) => index) } }
    case "envelope": return { type, params: { destination: "gain", attackMs: 10, decayMs: 100, sustain: 0.7, releaseMs: 100, amount: 1 } }
    case "propertyScaler": return { type, params: { property: "velocity", levels: { percent: 100, floor: 0, ceiling: 1 }, curve: 1 } }
    case "keySplitter": return { type, preserveKey: true, params: { lowMin: 0, boundary: 60, highMax: 127, lowKey: 48, highKey: 72, lowChannel: 0, highChannel: 1 } }
    case "keyMapper": return { type, params: { table: Array.from({ length: 128 }, (_, index) => index) }, chord: [0, 4, 7, 12], voices: 1 }
    case "sequencer": return { type, params: { mode: "sequence", length: 16, gate: 0.5, octaves: 1, grid: { stepsPerBeat: 4, steps: Array.from({ length: 16 }, () => ({ gate: true, pitchOffset: 0, velocityScale: 1 })) } } }
  }
}

export type Port = { id: number; name: string; signal: "audio" | "note" | "control" }
export function ports(processor: RackProcessor, output: boolean): Port[] {
  const result: Port[] = []
  const type = processor.type
  if (output) {
    if (["audioInput", "instrument", "effect", "rack"].includes(type)) result.push({ id: AUDIO_OUT, name: "Audio", signal: "audio" })
    if (processor.type === "effect") {
      const outputs = (descriptors.effectOutputs as Record<string, { label: string }[]>)[processor.params.type] ?? []
      outputs.forEach((port, index) => result.push({ id: AUX_AUDIO_BASE + index, name: port.label, signal: "audio" }))
    }
    if (["noteInput", "note", "rack"].includes(type)) result.push({ id: NOTE_OUT, name: "All notes", signal: "note" })
    if (type === "note" && ["colorRouter", "keySplitter"].includes(processor.module.type)) {
      for (let channel = 0; channel < 16; channel++) result.push({ id: NOTE_CHANNEL_BASE + channel, name: `Route ${channel + 1}`, signal: "note" })
    }
    if (type === "controlInput") result.push({ id: CONTROL_OUT, name: "Control", signal: "control" })
  } else {
    if (["audioOutput", "effect", "rack"].includes(type)) result.push({ id: AUDIO_IN, name: "Audio", signal: "audio" })
    if (["noteOutput", "instrument", "note", "rack"].includes(type)) result.push({ id: NOTE_IN, name: "Notes", signal: "note" })
    if (["instrument", "effect"].includes(type)) result.push({ id: CONTROL_IN, name: "Control", signal: "control" })
  }
  return result
}

export function graphAt(root: RackGraph, path: readonly number[]): RackGraph {
  let graph = root
  for (const id of path) {
    const found = graph.nodes.find((node) => node.id === id)
    if (found?.processor.type !== "rack") return root
    graph = found.processor.graph
  }
  return graph
}
export function replaceGraph(root: RackGraph, path: readonly number[], graph: RackGraph): RackGraph {
  if (path.length === 0) return graph
  const [id, ...rest] = path
  return { ...root, nodes: root.nodes.map((node) => node.id === id && node.processor.type === "rack"
    ? { ...node, processor: { ...node.processor, graph: replaceGraph(node.processor.graph, rest, graph) } } : node) }
}
export function sameEndpoint(a: RackEndpoint, b: RackEndpoint) { return a.node === b.node && a.port === b.port }

export function rackPreset(name: string, graph: RackGraph): string {
  return JSON.stringify({ schema: "windfall.rack", version: 1, name, graph }, null, 2)
}
