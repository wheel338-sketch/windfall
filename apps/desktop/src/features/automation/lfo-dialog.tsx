import { useId, useRef, useState, useSyncExternalStore } from "react"
import { create } from "zustand"
import type { Automation, AutomationId, ChannelId, CurveLfo, CurveLfoWave, Note, NoteLfoProperty, PatternId } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { currentSession } from "@/features/piano-roll/session"
import { LANE_KINDS } from "@/features/piano-roll/lane-math"
import { usePianoRollStore } from "@/features/piano-roll/store"
import { LFO_WAVES, lfoPreviewValue } from "@/lib/curve-lfo"
import { refuse } from "@/lib/errors"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { ticksPerBar } from "@/lib/time"
import { MAX_PATTERN_STEPS, MAX_SONG_TICKS, PPQ, TICKS_PER_STEP } from "@/lib/units"

type Capture = { generation: number; revision: number; serial: number }
type Request = Capture & (
  { kind: "piano"; pattern: PatternId; channel: ChannelId; notes: Note[]; selection: number[]; property: NoteLfoProperty } |
  { kind: "automation"; expected: Automation; start: number; end: number }
)
const useLfoDialog = create<{ request: Request | null }>(() => ({ request: null }))
let serial = 0
const capture = (): Capture => ({ generation: getProjectGeneration(), revision: useProjectStore.getState().revision, serial: ++serial })
export function closeLfoDialog() { useLfoDialog.setState({ request: null }) }
export function closeNoteLfo() { if (useLfoDialog.getState().request?.kind === "piano") closeLfoDialog() }
onProjectReplaced(closeLfoDialog)
onHistoryNavigation(closeLfoDialog)

export function openNoteLfo() {
  const session = currentSession()
  const context = session?.editor.context
  if (!session || !context || session.editor.busy || session.editor.hasStampChoice) return
  const selected = session.editor.selectedNotes()
  const notes = selected.length ? selected : context.notes
  if (!notes.length) return
  if (notes.length > 16384) { refuse("LFO selection is too large", "Choose at most 16,384 notes."); return }
  useLfoDialog.setState({ request: {
    ...capture(), kind: "piano", pattern: context.pattern.id, channel: context.channel,
    notes: notes.map((note) => ({ ...note, expression: note.expression ? { ...note.expression } : undefined })),
    selection: [...session.editor.selection], property: usePianoRollStore.getState().laneKind,
  } })
}

/**
 * Opens the LFO writer on an automation. `range` is the stretch of the
 * curve, in its own ticks, that the dialog offers to write: the window one
 * clip shows of it, when the tool is opened from that clip.
 */
export function openAutomationLfo(id: AutomationId, range?: { start: number; end: number }) {
  const project = useProjectStore.getState().project
  const automation = project.automations.find((item) => item.id === id)
  if (!automation) return
  const clipEnd = project.playlist.clips.reduce((end, clip) => clip.content.type === "automation" && clip.content.automation === id
    ? Math.max(end, Math.min(MAX_SONG_TICKS, clip.offset + clip.length)) : end, 0)
  const ranged = range && range.end > range.start && range.start >= 0
  useLfoDialog.setState({ request: {
    ...capture(), kind: "automation", expected: { ...automation, target: { ...automation.target }, points: automation.points.map((point) => ({ ...point })) },
    start: ranged ? Math.min(MAX_SONG_TICKS - 1, range.start) : 0,
    end: ranged ? Math.min(MAX_SONG_TICKS, range.end) : Math.min(MAX_SONG_TICKS, Math.max(PPQ * 4, clipEnd, automation.points.at(-1)?.tick ?? 0)),
  } })
}

function requestCurrent(request: Request): boolean {
  if (useLfoDialog.getState().request !== request || request.generation !== getProjectGeneration() || request.revision !== useProjectStore.getState().revision) return false
  if (request.kind === "automation") return true
  const editor = currentSession()?.editor
  return !!editor && !editor.busy && !editor.hasStampChoice && editor.context?.pattern.id === request.pattern && editor.context.channel === request.channel &&
    editor.selection.size === request.selection.length && request.selection.every((id) => editor.selection.has(id))
}

function Numeric({ label, value, set, min, max, step = 1 }: { label: string; value: string; set(value: string): void; min: number; max: number; step?: number }) {
  const id = useId()
  return <Field><FieldLabel htmlFor={id}>{label}</FieldLabel><Input id={id} type="number" value={value} min={min} max={max} step={step} onChange={(event) => set(event.target.value)} /></Field>
}
const numeric = (text: string) => text.trim() === "" ? NaN : Number(text)
const valid = (text: string, min: number, max: number, integer = false) => Number.isFinite(numeric(text)) && numeric(text) >= min && numeric(text) <= max && (!integer || Number.isInteger(numeric(text)))

function LfoForm({ request }: { request: Request }) {
  const session = request.kind === "piano" ? currentSession() : null
  const revision = useProjectStore((state) => state.revision)
  useSyncExternalStore((notify) => session?.editor.subscribe(notify) ?? (() => {}), () => session?.editor.selection)
  const first = request.kind === "piano" ? request.notes.reduce((first, note) => Math.min(first, note.start), MAX_PATTERN_STEPS * TICKS_PER_STEP) : request.start
  const last = request.kind === "piano" ? request.notes.reduce((last, note) => Math.max(last, note.start + note.length), 0) : request.end
  const [wave, setWave] = useState<CurveLfoWave>("sine")
  const [period, setPeriod] = useState(String(PPQ))
  const [phase, setPhase] = useState("0")
  const [center, setCenter] = useState("50")
  const [depth, setDepth] = useState("50")
  const [width, setWidth] = useState("50")
  const [seed, setSeed] = useState("1")
  const [start, setStart] = useState(String(first))
  const [end, setEnd] = useState(String(last))
  const [resolution, setResolution] = useState("60")
  const [property, setProperty] = useState<NoteLfoProperty>(request.kind === "piano" ? request.property : "velocity")
  const [strength, setStrength] = useState("100")
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const submitting = useRef(false)
  const current = revision === request.revision && requestCurrent(request)
  const maximum = request.kind === "piano" ? MAX_PATTERN_STEPS * TICKS_PER_STEP : MAX_SONG_TICKS
  const controlsValid = valid(period, 1, MAX_SONG_TICKS, true) && [phase, center, depth, width].every((value) => valid(value, 0, 100)) &&
    valid(seed, 0, 4294967295, true) && valid(start, 0, maximum, true) && (request.kind === "piano"
      ? valid(strength, 0, 100) : valid(end, numeric(start) + 1, maximum, true) && valid(resolution, 1, MAX_SONG_TICKS, true))
  const lfo: CurveLfo = { wave, period: numeric(period), phase: numeric(phase) / 100, center: numeric(center) / 100, depth: numeric(depth) / 100, width: numeric(width) / 100, seed: numeric(seed) }
  const previewSpan = Math.max(1, request.kind === "piano" ? last - first : numeric(end) - numeric(start))
  const displaySpan = Math.min(previewSpan, Math.max(1, lfo.period) * 8)
  const preview = controlsValid ? Array.from({ length: 241 }, (_, index) => `${index * 320 / 240},${64 - lfoPreviewValue(lfo, (request.kind === "piano" ? first - numeric(start) : 0) + displaySpan * index / 240) * 60}`).join(" ") : ""

  return <Dialog open onOpenChange={(open) => { if (!open && !submitting.current) closeLfoDialog() }}>
    <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg" showCloseButton={!pending}>
      <DialogHeader>
        <DialogTitle>{request.kind === "piano" ? "Note-event LFO" : `LFO · ${request.expected.name}`}</DialogTitle>
        <DialogDescription>{request.kind === "piano" ? `Write values at the onsets of ${request.notes.length} captured notes in one undo step.` : "Write an editable LFO into this curve's source ticks. Every clip sharing this curve follows the edit."}</DialogDescription>
      </DialogHeader>
      <form className="flex flex-col gap-4" onSubmit={async (event) => {
        event.preventDefault()
        if (!controlsValid || !requestCurrent(request) || submitting.current) return
        submitting.current = true; setPending(true); setFailed(false)
        try {
          const ok = request.kind === "piano"
            ? await currentSession()!.editor.transformNotes(request.notes, { type: "lfo", property, origin: numeric(start), strength: numeric(strength) / 100, lfo })
            : !!await dispatch({ type: "generateAutomationLfo", expected: request.expected, start: numeric(start), end: numeric(end), resolution: numeric(resolution), lfo })
          if (useLfoDialog.getState().request !== request) return
          if (ok) { if (request.kind === "piano") usePianoRollStore.getState().setLaneKind(property); closeLfoDialog() }
          else setFailed(true)
        } finally { submitting.current = false; setPending(false) }
      }}>
        <Field><FieldLabel>Shape</FieldLabel><Select items={LFO_WAVES} value={wave} disabled={pending} onValueChange={(value) => { const option = LFO_WAVES.find((item) => item.value === value); if (option) setWave(option.value) }}>
          <SelectTrigger aria-label="LFO shape"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{LFO_WAVES.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
        </Select></Field>
        {preview && <svg role="img" aria-label="LFO shape preview" viewBox="0 0 320 70" className="h-20 w-full rounded border bg-display text-brand">
          <path d="M0 34 H320" className="stroke-border" fill="none" /><polyline points={preview} fill="none" stroke="currentColor" strokeWidth={1.5} />
        </svg>}
        <FieldGroup className="grid grid-cols-2 gap-3">
          <Numeric label="Period (ticks)" value={period} set={setPeriod} min={1} max={MAX_SONG_TICKS} />
          <Numeric label="Phase (%)" value={phase} set={setPhase} min={0} max={100} step={0.1} />
          <Numeric label="Center (%)" value={center} set={setCenter} min={0} max={100} step={0.1} />
          <Numeric label="Depth (± %)" value={depth} set={setDepth} min={0} max={100} step={0.1} />
          {wave === "square" && <Numeric label="Pulse width (%)" value={width} set={setWidth} min={0} max={100} step={0.1} />}
          {wave === "sampleHold" && <Numeric label="Seed" value={seed} set={setSeed} min={0} max={4294967295} />}
          <Numeric label={request.kind === "piano" ? "Origin (ticks)" : "Start (source ticks)"} value={start} set={setStart} min={0} max={maximum} />
          {request.kind === "automation" && <>
            <Numeric label="End (source ticks)" value={end} set={setEnd} min={numeric(start) + 1} max={maximum} />
            <Numeric label="Point spacing (ticks)" value={resolution} set={setResolution} min={1} max={MAX_SONG_TICKS} />
          </>}
          {request.kind === "piano" && <>
            <Field><FieldLabel>Event property</FieldLabel><Select value={property} onValueChange={(value) => { const option = LANE_KINDS.find((item) => item.id === value); if (option) setProperty(option.id) }}>
              <SelectTrigger aria-label="LFO event property"><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{LANE_KINDS.map((item) => <SelectItem key={item.id} value={item.id}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
            </Select></Field>
            <Numeric label="Apply strength (%)" value={strength} set={setStrength} min={0} max={100} step={0.1} />
          </>}
        </FieldGroup>
        <div className="flex gap-1"><Button type="button" size="sm" variant="outline" onClick={() => setPeriod(String(PPQ))}>1 beat</Button><Button type="button" size="sm" variant="outline" onClick={() => setPeriod(String(ticksPerBar(useProjectStore.getState().project.settings.timeSignature)))}>1 bar</Button></div>
        <FieldDescription>{request.kind === "piano" ? "Center and depth span the selected property's full range; 50% is centered pan/fine pitch and neutral release/modulation. Apply strength blends with current note values. Notes retain their lengths, MIDI keys, articulation and color routing." : "Point spacing samples continuous shapes. Pulse and sample-and-hold use held values; saw and pulse jumps receive explicit boundaries. The curve outside Start–End is retained. Combined curves are limited to 4,096 points."} The preview shows up to eight cycles.</FieldDescription>
        {!current && <Alert><AlertDescription>The project or selection changed. Close this tool and reopen the current source.</AlertDescription></Alert>}
        {!controlsValid && <Alert><AlertDescription>Use finite values within the displayed ranges, with whole ticks and seeds.</AlertDescription></Alert>}
        {failed && <Alert><AlertDescription>The LFO could not be applied. Reopen the current source or increase its period/point spacing if the curve reached its point limit.</AlertDescription></Alert>}
        <DialogFooter><Button type="button" variant="ghost" disabled={pending} onClick={closeLfoDialog}>Cancel</Button><Button type="submit" disabled={pending || !current || !controlsValid}>{pending ? "Applying…" : "Write LFO"}</Button></DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

export function LfoDialog() {
  const request = useLfoDialog((state) => state.request)
  return request ? <LfoForm key={request.serial} request={request} /> : null
}
