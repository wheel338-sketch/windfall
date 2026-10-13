import { useRef, useState, type PointerEvent } from "react"
import { create } from "zustand"
import type { ChannelId, Note, NoteCurveParameter, NoteCurvePoint, NoteExpressionCurve, PatternId } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { curveShape } from "@/lib/automation/curve"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { currentSession } from "./session"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const PARAMETERS: { value: NoteCurveParameter; label: string; min: number; max: number }[] = [
  { value: "pan", get label() { return t("Pan") }, min: -1, max: 1 },
  { value: "release", get label() { return t("Release") }, min: 0, max: 1 },
  { value: "finePitchCents", get label() { return t("Fine pitch (cents)") }, min: -1200, max: 1200 },
  { value: "modulationX", get label() { return t("Modulation X") }, min: 0, max: 1 },
  { value: "modulationY", get label() { return t("Modulation Y") }, min: 0, max: 1 },
]
type Request = { pattern: PatternId; channel: ChannelId; notes: Note[]; curves: NoteExpressionCurve[]; generation: number; revision: number }
const useCurves = create<{ request: Request | null }>(() => ({ request: null }))
const copy = (curves: NoteExpressionCurve[]) => curves.map((curve) => ({ ...curve, points: curve.points.map((point) => ({ ...point })) }))
export function closeNoteCurves() { useCurves.setState({ request: null }) }
export function openNoteCurves() {
  const session = currentSession(), context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return
  const ids = new Set(notes.map((note) => note.id))
  useCurves.setState({ request: { pattern: context.pattern.id, channel: context.channel,
    notes: notes.map((note) => ({ ...note, expression: note.expression ? { ...note.expression } : undefined })),
    curves: copy((useProjectStore.getState().project.patterns.find((pattern) => pattern.id === context.pattern.id)?.noteCurves ?? []).filter((curve) => ids.has(curve.note))),
    generation: getProjectGeneration(), revision: useProjectStore.getState().revision } })
}
onProjectReplaced(closeNoteCurves)
onHistoryNavigation(closeNoteCurves)

function valueAt(points: NoteCurvePoint[], position: number) {
  if (position <= points[0].position) return points[0].value
  const index = points.findIndex((point) => point.position > position)
  if (index < 0) return points.at(-1)!.value
  const from = points[index - 1], to = points[index]
  return from.hold ? from.value : from.value + (to.value - from.value) * curveShape((position - from.position) / (to.position - from.position), from.curve)
}

function CurveForm({ request }: { request: Request }) {
  const [draft, setDraft] = useState(() => copy(request.curves))
  const [noteId, setNoteId] = useState(request.notes[0].id)
  const [parameter, setParameter] = useState<NoteCurveParameter>("pan")
  const [selected, setSelected] = useState(0)
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const submitting = useRef(false), dragging = useRef<number | null>(null), graph = useRef<SVGSVGElement>(null)
  const revision = useProjectStore((state) => state.revision)
  const current = revision === request.revision && request.generation === getProjectGeneration()
  const spec = PARAMETERS.find((item) => item.value === parameter)!
  const note = request.notes.find((note) => note.id === noteId)!
  const base = parameter === "pan" ? note.pan : ({ ...DEFAULT_NOTE_EXPRESSION, ...note.expression })[parameter]
  const stored = draft.find((curve) => curve.note === noteId && curve.parameter === parameter)
  const points = stored?.points ?? [{ position: 0, value: base, curve: 0, hold: false }, { position: 1, value: base, curve: 0, hold: false }]
  const point = points[Math.min(selected, points.length - 1)]
  const disabled = pending || !current
  const dirty = JSON.stringify(draft) !== JSON.stringify(request.curves)
  function replace(next: NoteCurvePoint[]) {
    setDraft((draft) => [...draft.filter((curve) => curve.note !== noteId || curve.parameter !== parameter), { note: noteId, parameter, points: next }])
  }
  function update(index: number, patch: Partial<NoteCurvePoint>) {
    const next = points.map((point) => ({ ...point }))
    next[index] = { ...next[index], ...patch }
    next[index].position = Math.max(index ? next[index - 1].position + 1e-6 : 0, Math.min(index + 1 < next.length ? next[index + 1].position - 1e-6 : 1, next[index].position))
    next[index].value = Math.max(spec.min, Math.min(spec.max, next[index].value))
    next[index].curve = Math.max(-1, Math.min(1, next[index].curve))
    replace(next)
  }
  function location(event: PointerEvent<SVGSVGElement>) {
    const rect = graph.current!.getBoundingClientRect()
    return { position: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)), value: spec.max - Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)) * (spec.max - spec.min) }
  }
  const path = Array.from({ length: 129 }, (_, index) => {
    const x = index / 128, y = (spec.max - valueAt(points, x)) / (spec.max - spec.min)
    return `${index ? "L" : "M"}${x * 600},${y * 180}`
  }).join(" ")
  return <Dialog open onOpenChange={(open) => { if (!open && !submitting.current) closeNoteCurves() }}>
    <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
      <DialogHeader><DialogTitle>{t("Note expression curves")}</DialogTitle><DialogDescription>{t("Positions follow each note’s musical duration. Double-click the graph to add a point; drag points or edit their values. Curves hold their first and last values outside their points.")}</DialogDescription></DialogHeader>
      {!current && <Alert><AlertDescription>{t("The source changed. Close and reopen the curve editor.")}</AlertDescription></Alert>}
      {failed && <Alert><AlertDescription>{t("The curves could not be applied. Your draft is retained.")}</AlertDescription></Alert>}
      <div className="grid gap-3 sm:grid-cols-2">
        <Field><FieldLabel>{t("Source note")}</FieldLabel><Select items={request.notes.map((note) => ({ value: note.id, label: message("Key {key} · tick {start} · {count} ticks", { key: plainNumber(note.key), start: plainNumber(note.start), count: plainNumber(note.length) }) }))} value={noteId} disabled={disabled} onValueChange={(id: number | null) => { if (id !== null) { setNoteId(id); setSelected(0) } }}><SelectTrigger aria-label={t("Expression curve source note")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{request.notes.map((note) => <SelectItem key={note.id} value={note.id}>{t("Key")}{" "}{note.key}{" "}{t("· tick")}{" "}{note.start} · {note.length}{" "}{t("ticks")}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
        <Field><FieldLabel>{t("Property")}</FieldLabel><Select items={PARAMETERS} value={parameter} disabled={disabled} onValueChange={(value: NoteCurveParameter | null) => { if (value) { setParameter(value); setSelected(0) } }}><SelectTrigger aria-label={t("Note curve property")}><SelectValue /></SelectTrigger><SelectContent><SelectGroup>{PARAMETERS.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
      </div>
      <svg ref={graph} viewBox="0 0 600 180" role="group" aria-label={message("{specLabel} note curve", { specLabel: spec.label })} className="h-48 w-full touch-none rounded border bg-muted/30" preserveAspectRatio="none"
        onDoubleClick={(event) => { if (disabled || points.length >= 256) return; const rect = graph.current!.getBoundingClientRect(); const position = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)); if (points.some((point) => Math.abs(point.position - position) < 1e-6)) return; const value = spec.max - Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)) * (spec.max - spec.min); const next = [...points, { position, value, curve: 0, hold: false }].sort((a, b) => a.position - b.position); replace(next); setSelected(next.findIndex((point) => point.position === position)) }}
        onPointerMove={(event) => { if (dragging.current !== null && !disabled) update(dragging.current, location(event)) }}
        onPointerUp={() => { dragging.current = null }} onPointerCancel={() => { dragging.current = null }} onLostPointerCapture={() => { dragging.current = null }}>
        {[0, .25, .5, .75, 1].map((position) => <line key={position} x1={position * 600} y1={0} x2={position * 600} y2={180} className="stroke-border" />)}
        <path d={path} fill="none" className="stroke-primary" strokeWidth={2} />
        {points.map((point, index) => <circle key={index} cx={point.position * 600} cy={(spec.max - point.value) / (spec.max - spec.min) * 180} r={selected === index ? 6 : 4} tabIndex={disabled ? -1 : 0} role="button" aria-label={message("Curve point {index}: {position}%, {pointValue}", { index: plainNumber(index + 1), position: (point.position * 100).toFixed(1), pointValue: point.value.toFixed(3) })} className="fill-primary stroke-background outline-none focus:stroke-foreground" strokeWidth={2}
          onPointerDown={(event) => { if (disabled || event.button !== 0) return; event.preventDefault(); event.stopPropagation(); setSelected(index); dragging.current = index; graph.current?.setPointerCapture(event.pointerId) }}
          onFocus={() => setSelected(index)} onKeyDown={(event) => {
            if (disabled) return
            if (event.key === "Delete" && points.length > 1) { event.preventDefault(); replace(points.filter((_, at) => at !== index)); setSelected(Math.max(0, index - 1)) }
            const direction = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0
            const vertical = event.key === "ArrowDown" ? -1 : event.key === "ArrowUp" ? 1 : 0
            if (direction || vertical) { event.preventDefault(); update(index, { position: point.position + direction * (event.shiftKey ? .1 : .01), value: point.value + vertical * (spec.max - spec.min) * (event.shiftKey ? .1 : .01) }) }
          }} />)}
      </svg>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field><FieldLabel htmlFor="note-curve-position">{t("Position (%)")}</FieldLabel><Input id="note-curve-position" type="number" min={0} max={100} step="any" disabled={disabled} value={point.position * 100} onChange={(event) => { const next = event.target.valueAsNumber; if (Number.isFinite(next)) update(Math.min(selected, points.length - 1), { position: next / 100 }) }} /></Field>
        <Field><FieldLabel htmlFor="note-curve-value">{t("Value")}</FieldLabel><Input id="note-curve-value" type="number" min={spec.min} max={spec.max} step="any" disabled={disabled} value={point.value} onChange={(event) => { const next = event.target.valueAsNumber; if (Number.isFinite(next)) update(Math.min(selected, points.length - 1), { value: next }) }} /></Field>
        <Field><FieldLabel htmlFor="note-curve-shape">{t("Segment bend")}</FieldLabel><Input id="note-curve-shape" type="number" min={-1} max={1} step={.05} disabled={disabled} value={point.curve} onChange={(event) => { const next = event.target.valueAsNumber; if (Number.isFinite(next)) update(Math.min(selected, points.length - 1), { curve: next }) }} /></Field>
        <Field orientation="horizontal"><Checkbox id="note-curve-hold" disabled={disabled} checked={point.hold} onCheckedChange={(hold) => update(Math.min(selected, points.length - 1), { hold: !!hold })} /><FieldLabel htmlFor="note-curve-hold">{t("Hold segment")}</FieldLabel></Field>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={disabled} onClick={() => replace(points.map((point) => ({ ...point })))}>{t("Enable curve")}</Button>
        <Button size="sm" variant="outline" disabled={disabled || points.length < 2} onClick={() => { replace(points.filter((_, index) => index !== Math.min(selected, points.length - 1))); setSelected(0) }}>{t("Remove point")}</Button>
        <Button size="sm" variant="outline" disabled={disabled || !stored} onClick={() => setDraft((draft) => draft.filter((curve) => curve.note !== noteId || curve.parameter !== parameter))}>{t("Remove this curve")}</Button>
        <Button size="sm" variant="outline" disabled={disabled} onClick={() => { const ids = new Set(request.notes.map((note) => note.id)); setDraft((draft) => [...draft.filter((curve) => curve.parameter !== parameter || !ids.has(curve.note)), ...request.notes.map((note) => ({ note: note.id, parameter, points: points.map((point) => ({ ...point })) }))]) }}>{t("Copy curve to selected notes")}</Button>
        <Button size="sm" variant="ghost" disabled={disabled || !draft.length} onClick={() => setDraft([])}>{t("Clear all selected curves")}</Button>
      </div>
      <DialogFooter><Button variant="ghost" disabled={pending} onClick={closeNoteCurves}>{t("Cancel")}</Button><Button disabled={disabled || !dirty} onClick={async () => {
        if (submitting.current || !current) return
        submitting.current = true; setPending(true); setFailed(false)
        try { const result = await dispatch({ type: "setNoteExpressionCurves", pattern: request.pattern, channel: request.channel, expected: request.notes, expectedCurves: request.curves, curves: draft }); if (result) closeNoteCurves(); else setFailed(true) }
        catch { setFailed(true) } finally { submitting.current = false; setPending(false) }
      }}>{t("Apply curves")}</Button></DialogFooter>
    </DialogContent>
  </Dialog>
}

export function NoteCurvesDialog() { const request = useCurves((state) => state.request); return request ? <CurveForm key={`${request.generation}:${request.pattern}:${request.channel}`} request={request} /> : null }
