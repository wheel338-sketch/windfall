import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from "react"

import type { ChannelId, Note, NoteId, NoteUpdate, PatternId } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { useChannel, useLane } from "@/lib/store/selectors"
import { useUiStore } from "@/lib/store/ui"
import { clamp, colorToCss, PPQ, TICKS_PER_STEP } from "@/lib/units"

import { GRAPH_LABELS, GRAPH_PROPERTIES, graphFraction, graphPatch, graphRange, graphReadout, graphSteps, graphValue, type GraphProperty } from "./graph-values"
import { LEFT_WIDTH, pitches, STEPS_INSET, STEPS_TRAIL } from "./layout"
import { useRackStore } from "./rack-store"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const HEIGHT = 112
const NO_NOTES: Note[] = []
type Stroke = {
  pointer: number
  generation: number
  notes: Note[]
  property: GraphProperty
  range: number
  last: { step: number; fraction: number }
  reset: boolean
  updates: Map<NoteId, NoteUpdate>
}

function LaneGraph({ pattern, channel, lengthSteps, groupSize }: { pattern: PatternId; channel: ChannelId; lengthSteps: number; groupSize: number }) {
  const lane = useLane(pattern, channel)
  const owner = useChannel(channel)
  const property = useRackStore((state) => state.graphProperty)
  const setProperty = useRackStore((state) => state.setGraphProperty)
  const notes = lane?.notes ?? NO_NOTES
  const steps = useMemo(() => graphSteps(notes, lengthSteps), [notes, lengthSteps])
  const [lengthRange, setLengthRange] = useState(PPQ)
  const [preview, setPreview] = useState(new Map<NoteId, NoteUpdate>())
  const [readout, setReadout] = useState("")
  const [previousInputs, setPreviousInputs] = useState({ notes, property, lengthRange, lengthSteps })
  if (previousInputs.notes !== notes || previousInputs.property !== property || previousInputs.lengthRange !== lengthRange || previousInputs.lengthSteps !== lengthSteps) {
    setPreviousInputs({ notes, property, lengthRange, lengthSteps })
    setPreview(new Map())
    setReadout("")
  }
  const [pending, setPending] = useState(false)
  const submitting = useRef(false)
  const stroke = useRef<Stroke | null>(null)
  const surface = useRef<HTMLDivElement>(null)
  const range = graphRange(property, lengthRange)

  function cancel() {
    const current = stroke.current
    stroke.current = null
    if (current && surface.current?.hasPointerCapture(current.pointer)) surface.current.releasePointerCapture(current.pointer)
    setPreview(new Map())
    setReadout("")
  }

  useEffect(() => {
    const stopReplace = onProjectReplaced(cancel)
    const stopHistory = onHistoryNavigation(cancel)
    return () => { stopReplace(); stopHistory(); stroke.current = null }
  }, [])
  useEffect(() => {
    const current = stroke.current
    stroke.current = null
    if (current && surface.current?.hasPointerCapture(current.pointer)) surface.current.releasePointerCapture(current.pointer)
  }, [notes, property, lengthRange, lengthSteps])

  function position(event: PointerEvent<HTMLDivElement>) {
    const rect = event.currentTarget.getBoundingClientRect()
    return {
      step: clamp(Math.floor((event.clientX - rect.left) / rect.width * lengthSteps), 0, lengthSteps - 1),
      fraction: clamp(1 - (event.clientY - rect.top) / rect.height, 0, 1),
    }
  }

  function paint(point: Stroke["last"]) {
    const current = stroke.current
    if (!current) return
    const { min, max, reset } = graphRange(current.property, current.range)
    const from = current.last
    const distance = Math.abs(point.step - from.step)
    for (let index = 0; index <= distance; index++) {
      const step = from.step + Math.sign(point.step - from.step) * index
      const fraction = distance === 0 ? point.fraction : from.fraction + (point.fraction - from.fraction) * index / distance
      const value = current.reset ? reset : min + fraction * (max - min)
      for (const note of steps[step] ?? []) current.updates.set(note.id, { id: note.id, patch: graphPatch(note, current.property, value, current.range) })
    }
    current.last = point
    setPreview(new Map(current.updates))
    setReadout(`Step ${point.step + 1}: ${graphReadout(current.property, current.reset ? reset : min + point.fraction * (max - min))}`)
  }

  async function commit(updates: NoteUpdate[], capturedNotes: Note[], generation: number) {
    if (submitting.current || generation !== getProjectGeneration()) return
    const current = useProjectStore.getState().project.patterns.find((item) => item.id === pattern)
    if (current?.lengthSteps !== lengthSteps || (current.lanes.find((item) => item.channel === channel)?.notes ?? NO_NOTES) !== capturedNotes) return
    const changed = updates.filter((update) => {
      const note = capturedNotes.find((item) => item.id === update.id)
      return note && Object.entries(update.patch).some(([key, value]) => note[key as keyof Note] !== value)
    })
    if (!changed.length) return
    submitting.current = true
    setPending(true)
    try { await dispatch({ type: "updateCapturedNotes", pattern, channel, expected: capturedNotes, updates: changed }) }
    finally { submitting.current = false; setPending(false) }
  }

  function begin(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 && event.button !== 2 || submitting.current) return
    event.preventDefault()
    const point = position(event)
    stroke.current = { pointer: event.pointerId, generation: getProjectGeneration(), notes, property, range: lengthRange, last: point, reset: event.button === 2 || event.altKey, updates: new Map() }
    event.currentTarget.setPointerCapture(event.pointerId)
    paint(point)
  }

  function finish(event: PointerEvent<HTMLDivElement>) {
    const current = stroke.current
    if (!current || current.pointer !== event.pointerId) return
    paint(position(event))
    const updates = [...current.updates.values()]
    cancel()
    void commit(updates, current.notes, current.generation)
  }

  function keyEdit(event: KeyboardEvent<HTMLButtonElement>, note: Note) {
    if (submitting.current || event.ctrlKey || event.metaKey || event.altKey) return
    let value = graphValue(note, property)
    const increment = range.step * (event.shiftKey ? 10 : 1)
    if (event.key === "ArrowUp") value += increment
    else if (event.key === "ArrowDown") value -= increment
    else if (event.key === "Home") value = range.min
    else if (event.key === "End") value = range.max
    else if (event.key === "Delete" || event.key === "Backspace") value = range.reset
    else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault()
      event.stopPropagation()
      const buttons = [...(surface.current?.querySelectorAll<HTMLButtonElement>("[data-graph-note]") ?? [])]
      const next = buttons[buttons.indexOf(event.currentTarget) + (event.key === "ArrowLeft" ? -1 : 1)]
      next?.focus()
      return
    } else return
    event.preventDefault()
    event.stopPropagation()
    void commit([{ id: note.id, patch: graphPatch(note, property, value, lengthRange) }], notes, getProjectGeneration())
  }

  return (
    <div className="flex shrink-0 border-y bg-chassis/30" data-slot="rack-graph">
      <div className="sticky left-0 z-10 flex shrink-0 flex-col gap-1.5 bg-background px-3 py-2" style={{ width: LEFT_WIDTH, height: HEIGHT + 16 }}>
        <div className="flex items-center justify-between gap-2">
          <span className="truncate font-medium">{owner?.name ?? t("Channel")}{" "}{t("graph")}</span>
          <Button variant="ghost" size="xs" aria-label={t("Close step graph")} onClick={() => useRackStore.getState().setGraphOpen(false)}>{t("Close")}</Button>
        </div>
        <Select value={property} onValueChange={(value) => { if (GRAPH_PROPERTIES.includes(value as GraphProperty)) setProperty(value as GraphProperty) }}>
          <SelectTrigger size="sm" aria-label={t("Step graph property")}><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup>{GRAPH_PROPERTIES.map((item) => <SelectItem key={item} value={item}>{GRAPH_LABELS[item]}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>
        {property === "length" && <Select value={String(lengthRange)} onValueChange={(value) => { if (value) setLengthRange(Number(value)) }}>
          <SelectTrigger size="sm" aria-label={t("Graph length range")}><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup>{[...new Set([TICKS_PER_STEP, PPQ, 4 * PPQ, lengthSteps * TICKS_PER_STEP])].sort((a, b) => a - b).map((ticks) => <SelectItem key={ticks} value={String(ticks)}>{ticks}{" "}{t("ticks")}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>}
        <span className="text-[0.625rem] text-muted-foreground" aria-live="polite">{readout || (pending ? t("Applying…") : t("Paint values · right-drag resets"))}</span>
      </div>
      <div style={{ paddingLeft: STEPS_INSET, paddingRight: STEPS_TRAIL }} className="flex items-center">
        <div ref={surface} role="group" aria-label={message("{ownerName} {graphLabels} step graph", { ownerName: owner?.name ?? t("Channel"), graphLabels: GRAPH_LABELS[property] })} aria-busy={pending} className="relative flex touch-none select-none" style={{ height: HEIGHT, width: pitches(lengthSteps) }} onPointerDown={begin} onPointerMove={(event) => { if (stroke.current?.pointer === event.pointerId) paint(position(event)) }} onPointerUp={finish} onPointerCancel={cancel} onLostPointerCapture={cancel} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation() }}>
          {steps.map((items, step) => <div key={step} className={`relative flex h-full shrink-0 items-end gap-px border-r border-(--wf-grid-line) px-px ${Math.floor(step / groupSize) % 2 ? "bg-accent/30" : "bg-background/30"}`} style={{ width: pitches(1) }}>
            {items.map((original) => {
              const note = { ...original, ...preview.get(original.id)?.patch }
              const fraction = graphFraction(note, property, lengthRange)
              return <button key={note.id} type="button" role="slider" data-graph-note={note.id} aria-label={message("Step {step}, MIDI {key}, {graphLabels}", { step: plainNumber(step + 1), key: plainNumber(note.key), graphLabels: GRAPH_LABELS[property] })} aria-valuemin={range.min} aria-valuemax={range.max} aria-valuenow={graphValue(note, property)} aria-valuetext={graphReadout(property, graphValue(note, property))} disabled={pending} className="relative h-full min-w-0 flex-1 outline-none focus-visible:ring-2 focus-visible:ring-ring" title={message("{name}: {value}", { name: GRAPH_LABELS[property], value: graphReadout(property, graphValue(note, property)) })} onKeyDown={(event) => keyEdit(event, original)} onDoubleClick={(event) => { event.stopPropagation(); cancel(); void commit([{ id: original.id, patch: graphPatch(original, property, range.reset, lengthRange) }], notes, getProjectGeneration()) }}>
                <span aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 rounded-t-sm opacity-70" style={{ height: `${Math.max(0.02, fraction) * 100}%`, backgroundColor: owner ? colorToCss(owner.color) : "var(--wf-brand)" }} />
                <span aria-hidden className="pointer-events-none absolute inset-x-0 h-0.5 bg-foreground" style={{ bottom: `calc(${fraction * 100}% - 1px)` }} />
              </button>
            })}
          </div>)}
          {["pan", "release", "finePitchCents", "modulationX", "modulationY"].includes(property) && <div aria-hidden className="pointer-events-none absolute inset-x-0 top-1/2 h-px bg-foreground/25" />}
          {notes.every((note) => note.start >= lengthSteps * TICKS_PER_STEP) && <p className="pointer-events-none absolute inset-0 flex items-center justify-start px-3 text-muted-foreground">{t("Turn on steps or add notes to edit their values.")}</p>}
        </div>
      </div>
    </div>
  )
}

/** The graph follows the selected channel and shares the rack's step pitch. */
export function StepGraph({ pattern, lengthSteps, groupSize }: { pattern: PatternId; lengthSteps: number; groupSize: number }) {
  const channel = useUiStore((state) => state.selectedChannel)
  const open = useRackStore((state) => state.graphOpen)
  if (!open || channel === null) return null
  return <LaneGraph key={`${pattern}:${channel}`} pattern={pattern} channel={channel} lengthSteps={lengthSteps} groupSize={groupSize} />
}
