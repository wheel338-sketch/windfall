import { useId, useRef, useState, useSyncExternalStore } from "react"
import { create } from "zustand"
import type { Automation, AutomationId, AutomationPoint, AutomationTarget, ChannelId, CurveLfo, CurveLfoWave, Note, NoteExpressionCurve, NoteLfoProperty, PatternId } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { currentSession } from "@/features/piano-roll/session"
import { LANE_KINDS } from "@/features/piano-roll/lane-math"
import { usePianoRollStore } from "@/features/piano-roll/store"
import { describeTarget } from "@/lib/automation/targets"
import { targetKey } from "@/lib/automation/lanes"
import { LFO_WAVES, lfoPreviewValue } from "@/lib/curve-lfo"
import { refuse } from "@/lib/errors"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { ticksPerBar } from "@/lib/time"
import { MAX_PATTERN_STEPS, MAX_SONG_TICKS, PPQ, TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

type Capture = { generation: number; revision: number; serial: number }
type Request = Capture & (
  { kind: "piano"; pattern: PatternId; channel: ChannelId; notes: Note[]; curves: NoteExpressionCurve[]; selection: number[]; property: NoteLfoProperty } |
  { kind: "automation"; expected: Automation; start: number; end: number } |
  { kind: "events"; pattern: PatternId; target: AutomationTarget; expected: AutomationPoint[]; end: number; label: string }
)
/** Where a note LFO goes: one value per note at its start, or a curve along each note. */
type NoteLfoMode = "onset" | "curve"
const NOTE_LFO_MODES: { value: NoteLfoMode; label: string }[] = [
  { value: "onset", get label() { return t("One value per note") } },
  { value: "curve", get label() { return t("Curve along each note") } },
]
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
  if (notes.length > 16384) { refuse(t("LFO selection is too large"), t("Choose at most 16,384 notes.")); return }
  const ids = new Set(notes.map((note) => note.id))
  const stored = useProjectStore.getState().project.patterns.find((item) => item.id === context.pattern.id)
  useLfoDialog.setState({ request: {
    ...capture(), kind: "piano", pattern: context.pattern.id, channel: context.channel,
    notes: notes.map((note) => ({ ...note, expression: note.expression ? { ...note.expression } : undefined })),
    // The curves these notes have now, in the pattern's order, for the document to compare.
    curves: (stored?.noteCurves ?? []).filter((curve) => ids.has(curve.note)).map((curve) => ({ ...curve, points: curve.points.map((point) => ({ ...point })) })),
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
  const shown = range && range.end > range.start && range.start >= 0 ? range : null
  useLfoDialog.setState({ request: {
    ...capture(), kind: "automation", expected: { ...automation, target: { ...automation.target }, points: automation.points.map((point) => ({ ...point })) },
    start: shown ? Math.min(MAX_SONG_TICKS - 1, shown.start) : 0,
    end: shown ? Math.min(MAX_SONG_TICKS, shown.end) : Math.min(MAX_SONG_TICKS, Math.max(PPQ * 4, clipEnd, automation.points.at(-1)?.tick ?? 0)),
  } })
}

/** Opens the LFO tool for the event lane a pattern has, or is to get, for a control. */
export function openEventLfo(pattern: PatternId, target: AutomationTarget) {
  const project = useProjectStore.getState().project
  const source = project.patterns.find((item) => item.id === pattern)
  if (!source || target.type === "tempo") return
  const key = targetKey(target)
  const lane = (source.events ?? []).find((item) => targetKey(item.target) === key)
  useLfoDialog.setState({ request: {
    ...capture(), kind: "events", pattern, target: { ...target },
    expected: (lane?.points ?? []).map((point) => ({ ...point })),
    end: Math.max(1, source.lengthSteps) * TICKS_PER_STEP,
    label: `${source.name} · ${describeTarget(project, target)}`,
  } })
}

function requestCurrent(request: Request): boolean {
  if (useLfoDialog.getState().request !== request || request.generation !== getProjectGeneration() || request.revision !== useProjectStore.getState().revision) return false
  if (request.kind !== "piano") return true
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
  const first = request.kind === "piano" ? request.notes.reduce((first, note) => Math.min(first, note.start), MAX_PATTERN_STEPS * TICKS_PER_STEP) : request.kind === "automation" ? request.start : 0
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
  const [mode, setMode] = useState<NoteLfoMode>("onset")
  // Velocity is struck once: only the other properties can move along a note.
  const curved = request.kind === "piano" && mode === "curve" && property !== "velocity"
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const submitting = useRef(false)
  const current = revision === request.revision && requestCurrent(request)
  const maximum = request.kind === "automation" ? MAX_SONG_TICKS : MAX_PATTERN_STEPS * TICKS_PER_STEP
  const controlsValid = valid(period, 1, MAX_SONG_TICKS, true) && [phase, center, depth, width].every((value) => valid(value, 0, 100)) &&
    valid(seed, 0, 4294967295, true) && valid(start, 0, maximum, true) && (request.kind === "piano"
      ? valid(strength, 0, 100) && (!curved || valid(resolution, 1, maximum, true))
      : valid(end, numeric(start) + 1, maximum, true) && valid(resolution, 1, MAX_SONG_TICKS, true))
  const lfo: CurveLfo = { wave, period: numeric(period), phase: numeric(phase) / 100, center: numeric(center) / 100, depth: numeric(depth) / 100, width: numeric(width) / 100, seed: numeric(seed) }
  const previewSpan = Math.max(1, request.kind === "piano" ? last - first : numeric(end) - numeric(start))
  const displaySpan = Math.min(previewSpan, Math.max(1, lfo.period) * 8)
  const preview = controlsValid ? Array.from({ length: 241 }, (_, index) => `${index * 320 / 240},${64 - lfoPreviewValue(lfo, (request.kind === "piano" ? first - numeric(start) : 0) + displaySpan * index / 240) * 60}`).join(" ") : ""

  return <Dialog open onOpenChange={(open) => { if (!open && !submitting.current) closeLfoDialog() }}>
    <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg" showCloseButton={!pending}>
      <DialogHeader>
        <DialogTitle>{request.kind === "piano" ? t("Note-event LFO") : request.kind === "events" ? message("Event LFO · {requestLabel}", { requestLabel: request.label }) : `LFO · ${request.expected.name}`}</DialogTitle>
        <DialogDescription>{request.kind === "piano" ? (curved ? message("Write a curve along each of {count} captured notes in one undo step.", { count: plainNumber(request.notes.length) }) : message("Write values at the onsets of {count} captured notes in one undo step.", { count: plainNumber(request.notes.length) })) : request.kind === "events" ? t("Write an editable LFO into this pattern's event lane, in pattern ticks. The lane is created at the control's current value when the pattern has none.") : t("Write an editable LFO into this curve's source ticks. Every clip sharing this curve follows the edit.")}</DialogDescription>
      </DialogHeader>
      <form className="flex flex-col gap-4" onSubmit={async (event) => {
        event.preventDefault()
        if (!controlsValid || !requestCurrent(request) || submitting.current) return
        submitting.current = true; setPending(true); setFailed(false)
        try {
          const ok = request.kind === "piano"
            ? curved
              ? !!await dispatch({ type: "generateNoteCurveLfo", pattern: request.pattern, channel: request.channel, expected: request.notes, expectedCurves: request.curves, parameter: property, origin: numeric(start), resolution: numeric(resolution), strength: numeric(strength) / 100, lfo })
              : await currentSession()!.editor.transformNotes(request.notes, { type: "lfo", property, origin: numeric(start), strength: numeric(strength) / 100, lfo })
            : request.kind === "events"
              ? !!await dispatch({ type: "generatePatternEventLfo", pattern: request.pattern, target: request.target, expected: request.expected, start: numeric(start), end: numeric(end), resolution: numeric(resolution), lfo })
              : !!await dispatch({ type: "generateAutomationLfo", expected: request.expected, start: numeric(start), end: numeric(end), resolution: numeric(resolution), lfo })
          if (useLfoDialog.getState().request !== request) return
          if (ok) { if (request.kind === "piano") usePianoRollStore.getState().setLaneKind(property); closeLfoDialog() }
          else setFailed(true)
        } finally { submitting.current = false; setPending(false) }
      }}>
        <Field><FieldLabel>{t("Shape")}</FieldLabel><Select items={LFO_WAVES} value={wave} disabled={pending} onValueChange={(value) => { const option = LFO_WAVES.find((item) => item.value === value); if (option) setWave(option.value) }}>
          <SelectTrigger aria-label={t("LFO shape")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{LFO_WAVES.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
        </Select></Field>
        {preview && <svg role="img" aria-label={t("LFO shape preview")} viewBox="0 0 320 70" className="h-20 w-full rounded border bg-display text-brand">
          <path d="M0 34 H320" className="stroke-border" fill="none" /><polyline points={preview} fill="none" stroke="currentColor" strokeWidth={1.5} />
        </svg>}
        <FieldGroup className="grid grid-cols-2 gap-3">
          <Numeric label={t("Period (ticks)")} value={period} set={setPeriod} min={1} max={MAX_SONG_TICKS} />
          <Numeric label={t("Phase (%)")} value={phase} set={setPhase} min={0} max={100} step={0.1} />
          <Numeric label={t("Center (%)")} value={center} set={setCenter} min={0} max={100} step={0.1} />
          <Numeric label={t("Depth (± %)")} value={depth} set={setDepth} min={0} max={100} step={0.1} />
          {wave === "square" && <Numeric label={t("Pulse width (%)")} value={width} set={setWidth} min={0} max={100} step={0.1} />}
          {wave === "sampleHold" && <Numeric label={t("Seed")} value={seed} set={setSeed} min={0} max={4294967295} />}
          <Numeric label={request.kind === "piano" ? t("Origin (ticks)") : request.kind === "events" ? t("Start (pattern ticks)") : t("Start (source ticks)")} value={start} set={setStart} min={0} max={maximum} />
          {request.kind !== "piano" && <>
            <Numeric label={request.kind === "events" ? t("End (pattern ticks)") : t("End (source ticks)")} value={end} set={setEnd} min={numeric(start) + 1} max={maximum} />
            <Numeric label={t("Point spacing (ticks)")} value={resolution} set={setResolution} min={1} max={MAX_SONG_TICKS} />
          </>}
          {request.kind === "piano" && <>
            <Field><FieldLabel>{t("Event property")}</FieldLabel><Select items={LANE_KINDS.map(({ id, label }) => ({ value: id, label }))} value={property} onValueChange={(value) => { const option = LANE_KINDS.find((item) => item.id === value); if (option) setProperty(option.id) }}>
              <SelectTrigger aria-label={t("LFO event property")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{LANE_KINDS.map((item) => <SelectItem key={item.id} value={item.id}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
            </Select></Field>
            <Numeric label={t("Apply strength (%)")} value={strength} set={setStrength} min={0} max={100} step={0.1} />
            <Field><FieldLabel>{t("Write as")}</FieldLabel><Select items={NOTE_LFO_MODES} value={mode} disabled={pending} onValueChange={(value) => { const option = NOTE_LFO_MODES.find((item) => item.value === value); if (option) setMode(option.value) }}>
              <SelectTrigger aria-label={t("LFO note mode")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{NOTE_LFO_MODES.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
            </Select></Field>
            {curved && <Numeric label={t("Point spacing (ticks)")} value={resolution} set={setResolution} min={1} max={maximum} />}
          </>}
        </FieldGroup>
        <div className="flex gap-1"><Button type="button" size="sm" variant="outline" onClick={() => setPeriod(String(PPQ))}>{t("1 beat")}</Button><Button type="button" size="sm" variant="outline" onClick={() => setPeriod(String(ticksPerBar(useProjectStore.getState().project.settings.timeSignature)))}>{t("1 bar")}</Button></div>
        {request.kind === "piano" && mode === "curve" && property === "velocity" && <Alert><AlertDescription>{t("Velocity is struck once at each note's start, so it is written as one value per note.")}</AlertDescription></Alert>}
        <FieldDescription>{request.kind === "piano" ? t("Center and depth span the selected property's full range; 50% is centered pan/fine pitch and neutral release/modulation. Apply strength blends with current note values. Notes retain their lengths, MIDI keys, articulation and color routing.") : t("Point spacing samples continuous shapes. Pulse and sample-and-hold use held values; saw and pulse jumps receive explicit boundaries. The curve outside Start–End is retained. Combined curves are limited to 4,096 points.")}{" "}{t("The preview shows up to eight cycles.")}</FieldDescription>
        {!current && <Alert><AlertDescription>{t("The project or selection changed. Close this tool and reopen the current source.")}</AlertDescription></Alert>}
        {!controlsValid && <Alert><AlertDescription>{t("Use finite values within the displayed ranges, with whole ticks and seeds.")}</AlertDescription></Alert>}
        {failed && <Alert><AlertDescription>{t("The LFO could not be applied. Reopen the current source or increase its period/point spacing if the curve reached its point limit.")}</AlertDescription></Alert>}
        <DialogFooter><Button type="button" variant="ghost" disabled={pending} onClick={closeLfoDialog}>{t("Cancel")}</Button><Button type="submit" disabled={pending || !current || !controlsValid}>{pending ? t("Applying…") : t("Write LFO")}</Button></DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

export function LfoDialog() {
  const request = useLfoDialog((state) => state.request)
  return request ? <LfoForm key={request.serial} request={request} /> : null
}
