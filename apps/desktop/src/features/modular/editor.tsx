import { useEffect, useState } from "react"
import type { Channel, DispatchResult, EffectKind, EffectParams, InstrumentKind, InstrumentParams, RackEndpoint, RackGraph, RackNode, RackProcessor } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { effectDescriptor, EFFECT_KINDS, GenericParamEditor, instrumentDescriptor, INSTRUMENT_KINDS, writeParam } from "@/features/params"
import { dispatch, redo, undo, useProjectStore } from "@/lib/store"
import { onHistoryNavigation } from "@/lib/store/project"
import { getProjectGeneration, useProjectGeneration } from "@/lib/store/replaced"
import { cn } from "@/lib/utils"
import { Section } from "../channel-rack/inspector/parts"
import { CONTROL_IN, defaultModule, graphAt, initialGraph, nestedGraph, node, ports, rackPreset, replaceGraph, sameEndpoint } from "./model"
import { ChoiceField, NoteModuleEditor, NumberField } from "./note-editor"

const MODULES = ["colorRouter", "envelope", "propertyScaler", "keySplitter", "keyMapper", "sequencer"] as const
const TYPES = ["instrument", "effect", ...MODULES, "rack", "audioInput", "audioOutput", "noteInput", "noteOutput", "controlInput"] as const
type AddType = typeof TYPES[number]

/** A saved graph editor. Port buttons submit real checked Rust commands. */
export function ModularEditor({ channel, root }: { channel: Channel; root: RackGraph }) {
  const [path, setPath] = useState<number[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [from, setFrom] = useState<RackEndpoint | null>(null)
  const [addType, setAddType] = useState<AddType>("effect")
  const [effect, setEffect] = useState<EffectKind>("polarity")
  const [instrument, setInstrument] = useState<InstrumentKind>("subtractiveSynth")
  const [parameter, setParameter] = useState("")
  const [presetText, setPresetText] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const generation = useProjectGeneration()
  const [parameterQueue] = useState(() => ({ tail: Promise.resolve<DispatchResult | null>(null), epoch: 0 }))
  useEffect(() => {
    const stop = onHistoryNavigation(() => { parameterQueue.epoch += 1 })
    return () => { stop(); parameterQueue.epoch += 1 }
  }, [parameterQueue])
  const graph = graphAt(root, path)
  const activePath = graph === root ? [] : path
  const current = graph.nodes.find((node) => node.id === selected)

  async function save(next: RackGraph, gesture?: number) {
    if (busy) return null
    setBusy(true)
    const result = await dispatch({ type: "setChannelRack", id: channel.id, graph: replaceGraph(root, activePath, next), expected: root }, gesture)
    setBusy(false)
    setError(result ? null : "Rack edit was refused. Check the project error message; the saved graph was kept.")
    return result
  }
  function updateNode(next: RackNode, gesture?: number) {
    return save({ ...graph, nodes: graph.nodes.map((node) => node.id === next.id ? next : node) }, gesture)
  }
  async function add() {
    let processor: RackProcessor
    if (addType === "instrument") processor = { type: "instrument", params: { ...instrumentDescriptor(instrument).defaults, type: instrument } as InstrumentParams }
    else if (addType === "effect") processor = { type: "effect", params: { ...effectDescriptor(effect).defaults, type: effect } as EffectParams, mix: 1 }
    else if (addType === "rack") processor = { type: "rack", graph: nestedGraph() }
    else if (addType === "controlInput") {
      const used = graph.nodes.flatMap((node) => node.processor.type === "controlInput" ? [node.processor.index] : [])
      const index = Array.from({ length: 32 }, (_, index) => index).find((index) => !used.includes(index))
      if (index === undefined) { setError("All 32 macro indices are in use."); return }
      processor = { type: "controlInput", index, value: 0.5 }
    } else if (addType === "audioInput" || addType === "audioOutput" || addType === "noteInput" || addType === "noteOutput") processor = { type: addType }
    else processor = { type: "note", module: defaultModule(addType) }
    const id = Math.max(1, graph.nextId)
    const next = node(id, addType, processor, [20 + graph.nodes.length % 3 * 260, 40 + Math.floor(graph.nodes.length / 3) * 150])
    if (await save({ ...graph, nextId: id + 1, nodes: [...graph.nodes, next] })) setSelected(id)
  }
  async function connect(to: RackEndpoint) {
    if (!from) return
    const source = graph.nodes.find((node) => node.id === from.node)
    const target = graph.nodes.find((node) => node.id === to.node)
    const signal = source && ports(source.processor, true).find((port) => port.id === from.port)?.signal
    if (!target || ports(target.processor, false).find((port) => port.id === to.port)?.signal !== signal) {
      setError("These ports carry different signal types."); return
    }
    let id: string | undefined
    if (to.port === CONTROL_IN) {
      const descriptor = target.processor.type === "effect" ? effectDescriptor(target.processor.params.type)
        : target.processor.type === "instrument" ? instrumentDescriptor(target.processor.params.type) : null
      id = descriptor?.params.find((info) => info.id === parameter)?.id ?? descriptor?.params[0]?.id
    }
    if (await save({ ...graph, edges: [...graph.edges, { from, to, gain: 1, ...(id ? { parameter: id } : {}) }] })) setFrom(null)
  }
  function queueParameter(id: number, type: "instrument" | "effect", kind: InstrumentKind | EffectKind, index: number, value: number, gesture?: number) {
    const epoch = parameterQueue.epoch
    const result = parameterQueue.tail.then(async () => {
      if (generation !== getProjectGeneration() || epoch !== parameterQueue.epoch) return null
      const latest = useProjectStore.getState().project.settings.modularRacks?.find((rack) => rack.channel === channel.id)?.graph
      if (!latest) return null
      const nested = graphAt(latest, activePath)
      if (activePath.length > 0 && nested === latest) return null
      const target = nested.nodes.find((node) => node.id === id)
      if (!target || target.processor.type !== type || (target.processor.type !== "instrument" && target.processor.type !== "effect") || target.processor.params.type !== kind) return null
      const processor = target.processor
      const info = (processor.type === "instrument" ? instrumentDescriptor(processor.params.type) : effectDescriptor(processor.params.type)).params[index]
      if (!info) return null
      const updated: RackProcessor = processor.type === "instrument" ? { ...processor, params: writeParam(processor.params, info, value) } : { ...processor, params: writeParam(processor.params, info, value) }
      setBusy(true)
      const applied = await dispatch({ type: "setChannelRack", id: channel.id, expected: latest, graph: replaceGraph(latest, activePath, { ...nested, nodes: nested.nodes.map((node) => node.id === id ? { ...node, processor: updated } : node) }) }, gesture)
      setBusy(false)
      setError(applied ? null : "Rack parameter edit was refused; the saved graph was kept.")
      return applied
    })
    parameterQueue.tail = result
    return result
  }
  async function loadPreset() {
    try {
      const parsed: unknown = JSON.parse(presetText)
      if (typeof parsed !== "object" || parsed === null || !("schema" in parsed) || parsed.schema !== "windfall.rack" || !("version" in parsed) || parsed.version !== 1 || !("graph" in parsed)) throw new Error("Use a Windfall rack preset, version 1")
      // Rust deserializes and checks the full graph; this assertion does not
      // bypass schema, cycle, port, numeric or resource validation.
      await save(parsed.graph as RackGraph)
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Invalid rack preset") }
  }
  const height = Math.max(300, ...graph.nodes.map((node) => node.position[1] + 130))

  return <div className="flex min-h-0 flex-col gap-3" aria-label="Modular rack editor">
    <div className="flex flex-wrap items-end gap-2">
      <ChoiceField<AddType> label="Module type" value={addType} choices={TYPES} onChange={setAddType} />
      {addType === "effect" && <ChoiceField<EffectKind> label="Effect module" value={effect} choices={EFFECT_KINDS} onChange={setEffect} />}
      {addType === "instrument" && <ChoiceField<InstrumentKind> label="Instrument module" value={instrument} choices={INSTRUMENT_KINDS} onChange={setInstrument} />}
      <Button onClick={() => void add()} disabled={busy || graph.nodes.length >= 64}>Add node</Button>
      <Button variant="outline" disabled={busy} onClick={() => void undo()}>Undo rack edit</Button>
      <Button variant="outline" disabled={busy} onClick={() => void redo()}>Redo rack edit</Button>
      {activePath.length > 0 && <Button variant="outline" onClick={() => { setPath(activePath.slice(0, -1)); setSelected(null); setFrom(null) }}>Parent rack</Button>}
    </div>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <div className="max-h-64 overflow-auto rounded-md border">
      <svg viewBox={`0 0 800 ${height}`} className="min-w-[700px] text-primary" style={{ height }} role="img" aria-label="Rack signal wires">
        {graph.edges.map((edge, index) => {
          const start = graph.nodes.find((node) => node.id === edge.from.node)?.position
          const end = graph.nodes.find((node) => node.id === edge.to.node)?.position
          return start && end ? <path key={index} d={`M ${start[0] + 200} ${start[1] + 45} C ${start[0] + 250} ${start[1] + 45}, ${end[0] - 50} ${end[1] + 45}, ${end[0]} ${end[1] + 45}`} stroke="currentColor" fill="none" strokeWidth={2} /> : null
        })}
        {graph.nodes.map((node) => <g key={node.id} transform={`translate(${node.position[0]},${node.position[1]})`}>
          <rect width={200} height={90} rx={6} className="fill-card stroke-border" />
          <text x={10} y={24} className="fill-foreground">{node.name}</text><text x={10} y={50} className="fill-muted-foreground">{node.processor.type}</text>
        </g>)}
      </svg>
    </div>
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
      <div className="flex max-h-96 flex-col gap-2 overflow-auto">
        {graph.nodes.map((node) => <div key={node.id} className={cn("flex flex-col gap-2 rounded-md border p-2", selected === node.id && "ring-1 ring-primary")}>
          <Button variant="ghost" aria-label={`Edit ${node.name} ${node.id}`} onClick={() => { setSelected(node.id); setParameter("") }}>{node.name} ({node.id})</Button>
          <div className="flex flex-wrap gap-1">{ports(node.processor, true).map((port) => <Button key={port.id} size="sm" variant={from && sameEndpoint(from, { node: node.id, port: port.id }) ? "default" : "outline"}
            disabled={busy} aria-label={`${node.name} ${node.id} output ${port.name}`} onClick={() => { setFrom({ node: node.id, port: port.id }); setError(null) }}>{port.name} →</Button>)}</div>
          <div className="flex flex-wrap gap-1">{ports(node.processor, false).map((port) => <Button key={port.id} size="sm" variant="outline" disabled={busy || !from}
            aria-label={`${node.name} ${node.id} input ${port.name}`} onClick={() => void connect({ node: node.id, port: port.id })}>→ {port.name}</Button>)}</div>
        </div>)}
        {from && <Button variant="outline" onClick={() => setFrom(null)}>Cancel connection</Button>}
        <ul aria-label="Connected wires" className="flex flex-col gap-2">{graph.edges.map((edge, index) => <li key={index} className="flex items-end gap-2">
          <span>{edge.from.node}:{edge.from.port} → {edge.to.node}:{edge.to.port}{edge.parameter ? ` ${edge.parameter}` : ""}</span>
          <NumberField label={`Wire ${index + 1} gain`} value={edge.gain} min={-4} max={4} step={0.01} onChange={(gain) => void save({ ...graph, edges: graph.edges.map((wire, at) => at === index ? { ...wire, gain } : wire) })} />
          <Button variant="outline" disabled={busy} aria-label={`Disconnect wire ${index + 1}`} onClick={() => void save({ ...graph, edges: graph.edges.filter((_, at) => at !== index) })}>Disconnect</Button>
        </li>)}</ul>
      </div>
      <div className="max-h-96 overflow-auto">{current && <FieldGroup>
        <Field><FieldLabel htmlFor="rack-node-name">Node name</FieldLabel><Input key={`${current.id}:${current.name}`} id="rack-node-name" defaultValue={current.name} onBlur={(event) => { if (event.target.value !== current.name) void updateNode({ ...current, name: event.target.value }) }} /></Field>
        <NumberField label="Node X" value={current.position[0]} min={0} max={2000} onChange={(value) => void updateNode({ ...current, position: [value, current.position[1]] })} />
        <NumberField label="Node Y" value={current.position[1]} min={0} max={2000} onChange={(value) => void updateNode({ ...current, position: [current.position[0], value] })} />
        {(["muted", "solo"] as const).map((key) => <Field key={key} orientation="horizontal"><Checkbox id={`rack-${key}`} checked={current[key]} onCheckedChange={(value) => void updateNode({ ...current, [key]: value === true })} /><FieldLabel htmlFor={`rack-${key}`}>{key === "muted" ? "Mute node" : "Solo node"}</FieldLabel></Field>)}
        {current.processor.type === "note" && <NoteModuleEditor key={current.id} module={current.processor.module} onChange={(module) => void updateNode({ ...current, processor: { type: "note", module } })} />}
        {current.processor.type === "rack" && <Button onClick={() => { setPath([...activePath, current.id]); setSelected(null); setFrom(null) }}>Open nested rack</Button>}
        {current.processor.type === "controlInput" && <NumberField label={`Macro ${current.processor.index + 1}`} value={current.processor.value} min={0} max={1} step={0.01} onChange={(value) => {
          const processor = current.processor; if (processor.type === "controlInput") void updateNode({ ...current, processor: { ...processor, value } })
        }} />}
        {(current.processor.type === "instrument" || current.processor.type === "effect") && (() => {
          const processor = current.processor
          const descriptor = processor.type === "instrument" ? instrumentDescriptor(processor.params.type) : effectDescriptor(processor.params.type)
          return <>
            <ChoiceField<string> label="Control destination" value={parameter || descriptor.params[0]?.id || ""} choices={descriptor.params.map((info) => info.id)} onChange={setParameter} />
            {processor.type === "effect" && <NumberField label="Effect mix" value={processor.mix} min={0} max={1} step={0.01} onChange={(mix) => void updateNode({ ...current, processor: { ...processor, mix } })} />}
            {processor.type === "instrument" ? <GenericParamEditor descriptor={instrumentDescriptor(processor.params.type)} params={processor.params} setParam={(index, value, gesture) => queueParameter(current.id, "instrument", processor.params.type, index, value, gesture)} />
              : <GenericParamEditor descriptor={effectDescriptor(processor.params.type)} params={processor.params} setParam={(index, value, gesture) => queueParameter(current.id, "effect", processor.params.type, index, value, gesture)} />}

          </>
        })()}
        <Button variant="destructive" disabled={busy} onClick={async () => {
          if (await save({ ...graph, nodes: graph.nodes.filter((node) => node.id !== current.id), edges: graph.edges.filter((edge) => edge.from.node !== current.id && edge.to.node !== current.id) })) { setSelected(null); setFrom(null) }
        }}>Remove node</Button>
      </FieldGroup>}</div>
    </div>
    <FieldGroup><Field><FieldLabel htmlFor="rack-preset">Rack preset</FieldLabel><Textarea id="rack-preset" value={presetText} onChange={(event) => setPresetText(event.target.value)} placeholder="Copy or paste a Windfall rack preset" /></Field>
      <div className="flex gap-2"><Button variant="outline" onClick={() => setPresetText(rackPreset(channel.name, graph))}>Write preset text</Button><Button variant="outline" disabled={busy || !presetText} onClick={() => void loadPreset()}>Load rack preset</Button></div>
    </FieldGroup>
  </div>
}

export function ModularRackSection({ channel }: { channel: Channel }) {
  const graph = useProjectStore((state) => state.project.settings.modularRacks?.find((rack) => rack.channel === channel.id)?.graph)
  const generation = useProjectGeneration()
  const plugin = useProjectStore((state) => state.project.plugins?.some((plugin) => plugin.target.type === "instrument" && plugin.target.channel === channel.id))
  if (channel.source.type !== "instrument" || plugin) return null
  return <Section title="Modular rack">
    {graph ? <Dialog><DialogTrigger render={<Button variant="outline" />}>Edit modular rack</DialogTrigger><DialogContent className="max-h-[90vh] overflow-auto sm:max-w-6xl">
      <DialogHeader><DialogTitle>{channel.name} modular rack</DialogTitle><DialogDescription>Choose an output, then a matching input, to connect modules. Note routes carry independent note owners. Audio paths align before they mix.</DialogDescription></DialogHeader>
      <ModularEditor key={`${generation}:${channel.id}`} channel={channel} root={graph} />
    </DialogContent></Dialog> : <Button variant="outline" onClick={() => { if (channel.source.type === "instrument") void dispatch({ type: "setChannelRack", id: channel.id, graph: initialGraph(channel.source.params) }) }}>Create modular rack</Button>}
    {graph && <Button variant="outline" onClick={() => void dispatch({ type: "setChannelRack", id: channel.id, graph: null })}>Remove modular rack</Button>}
  </Section>
}
