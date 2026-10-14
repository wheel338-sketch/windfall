import { useId, useRef, useState } from "react"
import { create } from "zustand"

import type { ChannelId, Note, NoteArticulation, NotePatch, NoteUpdate, PatternId } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { DEFAULT_NOTE_EXPRESSION, isNoteArticulation, NOTE_ARTICULATIONS } from "@/lib/note-expression"
import { isNoteColorGroup } from "@/lib/note-colors"
import { dispatch, onHistoryNavigation, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { MAX_PATTERN_STEPS, TICKS_PER_STEP } from "@/lib/units"

import { currentSession } from "./session"
import { NoteColorPicker } from "./note-color-picker"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

const MAX_TICKS = MAX_PATTERN_STEPS * TICKS_PER_STEP
const FIELDS = [
  { id: "start", get label() { return t("Start (ticks)") }, min: 0, max: MAX_TICKS - 1, step: 1, factor: 1 },
  { id: "length", get label() { return t("Length (ticks)") }, min: 1, max: MAX_TICKS, step: 1, factor: 1 },
  { id: "key", get label() { return t("MIDI key") }, min: 0, max: 127, step: 1, factor: 1 },
  { id: "velocity", get label() { return t("Velocity (%)") }, min: 0, max: 100, step: 0.1, factor: 100 },
  { id: "pan", get label() { return t("Pan (%)") }, min: -100, max: 100, step: 0.1, factor: 100 },
  { id: "release", get label() { return t("Release (%)") }, min: 0, max: 100, step: 0.1, factor: 100 },
  { id: "finePitchCents", get label() { return t("Fine pitch (cents)") }, min: -1200, max: 1200, step: 1, factor: 1 },
  { id: "modulationX", get label() { return t("Modulation X (%)") }, min: 0, max: 100, step: 0.1, factor: 100 },
  { id: "modulationY", get label() { return t("Modulation Y (%)") }, min: 0, max: 100, step: 0.1, factor: 100 },
  { id: "glideTicks", get label() { return t("Portamento duration (ticks)") }, min: 1, max: MAX_TICKS, step: 1, factor: 1 },
] as const
type Property = (typeof FIELDS)[number]["id"]
type Values = Record<Property, string>
type Request = { pattern: PatternId; channel: ChannelId; notes: Note[]; generation: number; revision: number }
const useProperties = create<{ request: Request | null }>(() => ({ request: null }))

export function closeNoteProperties() { useProperties.setState({ request: null }) }
export function openNoteProperties() {
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return
  useProperties.setState({ request: {
    pattern: context.pattern.id,
    channel: context.channel,
    notes: notes.map((note) => ({ ...note, expression: note.expression ? { ...note.expression } : undefined })),
    generation: getProjectGeneration(),
    revision: useProjectStore.getState().revision,
  } })
}
onProjectReplaced(closeNoteProperties)
onHistoryNavigation(closeNoteProperties)

export async function setSelectedArticulation(articulation: NoteArticulation): Promise<void> {
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return
  const updates = notes.filter((note) => (note.expression?.articulation ?? "normal") !== articulation).map((note) => ({
    id: note.id,
    patch: { expression: { ...DEFAULT_NOTE_EXPRESSION, ...note.expression, articulation } },
  }))
  if (updates.length) await dispatch({ type: "updateCapturedNotes", pattern: context.pattern.id, channel: context.channel, expected: notes, updates })
}

function propertyValue(note: Note, property: Property): number {
  if (property === "glideTicks") return note.expression?.glideTicks ?? 240
  if (property === "release" || property === "finePitchCents" || property === "modulationX" || property === "modulationY") return (note.expression ?? DEFAULT_NOTE_EXPRESSION)[property]
  return note[property]
}

export async function setSelectedColorGroup(colorGroup: number | null): Promise<void> {
  if (colorGroup !== null && !isNoteColorGroup(colorGroup)) return
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return
  const updates = notes.filter((note) => (note.expression?.colorGroup ?? null) !== colorGroup).map((note) => ({
    id: note.id,
    patch: { expression: { ...DEFAULT_NOTE_EXPRESSION, ...note.expression, colorGroup: colorGroup ?? undefined } },
  }))
  if (updates.length) await dispatch({ type: "updateCapturedNotes", pattern: context.pattern.id, channel: context.channel, expected: notes, updates })
}

function startingValues(notes: Note[]): Values {
  const entries = FIELDS.map((field) => {
    const first = propertyValue(notes[0], field.id)
    return [field.id, notes.every((note) => propertyValue(note, field.id) === first) ? String(Math.round(first * field.factor * 1000) / 1000) : ""]
  })
  return Object.fromEntries(entries) as Values
}

function PropertyForm({ request }: { request: Request }) {
  const [values, setValues] = useState(() => startingValues(request.notes))
  const [touched, setTouched] = useState(new Set<Property>())
  const [articulation, setArticulation] = useState<NoteArticulation | "keep">(() => {
    const first = request.notes[0].expression?.articulation ?? "normal"
    return request.notes.every((note) => (note.expression?.articulation ?? "normal") === first) ? first : "keep"
  })
  const [articulationTouched, setArticulationTouched] = useState(false)
  const [colorGroup, setColorGroup] = useState<number | null | "keep">(() => {
    const first = request.notes[0].expression?.colorGroup ?? null
    return request.notes.every((note) => (note.expression?.colorGroup ?? null) === first) ? first : "keep"
  })
  const [colorTouched, setColorTouched] = useState(false)
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const submitting = useRef(false)
  const formId = useId()
  const revision = useProjectStore((state) => state.revision)
  const current = revision === request.revision && getProjectGeneration() === request.generation
  const active = FIELDS.filter((field) => touched.has(field.id) && values[field.id].trim() !== "")
  const hasEdits = active.length > 0 || articulationTouched && articulation !== "keep" || colorTouched && colorGroup !== "keep"
  const invalid = active.some((field) => {
    const value = Number(values[field.id])
    return !Number.isFinite(value) || value < field.min || value > field.max || field.step === 1 && !Number.isInteger(value)
  })

  function change(property: Property, value: string) {
    setValues((previous) => ({ ...previous, [property]: value }))
    setTouched((previous) => new Set([...previous, property]))
  }

  function updates(): NoteUpdate[] {
    return request.notes.map((note) => {
      const patch: NotePatch = {}
      const expression = { ...(note.expression ?? DEFAULT_NOTE_EXPRESSION) }
      let expressionChanged = false
      for (const field of active) {
        const value = Number(values[field.id]) / field.factor
        if (value === propertyValue(note, field.id)) continue
        if (field.id === "release" || field.id === "finePitchCents" || field.id === "modulationX" || field.id === "modulationY" || field.id === "glideTicks") {
          expression[field.id] = value
          expressionChanged = true
        } else patch[field.id] = value
      }
      if (articulationTouched && articulation !== "keep" && (note.expression?.articulation ?? "normal") !== articulation) {
        expression.articulation = articulation
        expressionChanged = true
      }
      if (colorTouched && colorGroup !== "keep" && (note.expression?.colorGroup ?? null) !== colorGroup) {
        expression.colorGroup = colorGroup ?? undefined
        expressionChanged = true
      }
      if (expressionChanged) patch.expression = expression
      return { id: note.id, patch }
    }).filter((update) => Object.keys(update.patch).length > 0)
  }

  return <Dialog open onOpenChange={(open) => { if (!open && !submitting.current) closeNoteProperties() }}>
    <DialogContent className="max-h-[85dvh] overflow-y-auto sm:max-w-lg" showCloseButton={!pending}>
      <DialogHeader>
        <DialogTitle>{t("Note properties")}</DialogTitle>
        <DialogDescription>{t("Edit")}{" "}{request.notes.length === 1 ? t("this note") : message("{count} selected notes", { count: plainNumber(request.notes.length) })}{" "}{t("in one undo step. Blank values keep each note's current value; changed values apply to every captured note.")}</DialogDescription>
      </DialogHeader>
      <form className="flex flex-col gap-4" onSubmit={async (event) => {
        event.preventDefault()
        if (!current || invalid || submitting.current || !hasEdits) return
        const changed = updates()
        if (!changed.length) { closeNoteProperties(); return }
        submitting.current = true
        setPending(true)
        setFailed(false)
        try {
          const result = await dispatch({ type: "updateCapturedNotes", pattern: request.pattern, channel: request.channel, expected: request.notes, updates: changed })
          if (useProperties.getState().request !== request) return
          if (result) closeNoteProperties()
          else setFailed(true)
        } finally { submitting.current = false; setPending(false) }
      }}>
        <Field>
          <FieldLabel>{t("Articulation")}</FieldLabel>
          <Select value={articulation} disabled={pending} onValueChange={(value) => {
            if (value === "keep" || isNoteArticulation(value)) { setArticulation(value); setArticulationTouched(value !== "keep") }
          }}>
            <SelectTrigger aria-label={t("Note articulation")}><SelectValue /></SelectTrigger>
            <SelectContent><SelectGroup>
              <SelectItem value="keep">{t("Keep each note's articulation")}</SelectItem>
              {NOTE_ARTICULATIONS.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}
            </SelectGroup></SelectContent>
          </Select>
          <FieldDescription>{t("A slide bends the held chord over this note's length and starts no voice. Portamento starts a voice gliding from the previous held pitch over its duration below.")}</FieldDescription>
        </Field>
        <FieldGroup className="grid grid-cols-2 gap-3">
          <Field className="col-span-2">
            <FieldLabel>{t("Color group / MIDI channel")}</FieldLabel>
            <NoteColorPicker mixed value={colorGroup} disabled={pending} onChange={(value) => { setColorGroup(value); setColorTouched(value !== "keep") }} />
            <FieldDescription>{t("Explicit groups route MIDI notes to channels 1–16. Channel color keeps automatic routing.")}</FieldDescription>
          </Field>
          {FIELDS.map((field) => <Field key={field.id}>
            <FieldLabel htmlFor={`${formId}-${field.id}`}>{field.label}</FieldLabel>
            <Input id={`${formId}-${field.id}`} type="number" min={field.min} max={field.max} step={field.step} value={values[field.id]} placeholder={t("Mixed · keep values")} disabled={pending} onChange={(event) => change(field.id, event.target.value)} />
          </Field>)}
        </FieldGroup>
        <FieldDescription>{t("Pan: negative is left, positive is right. Release and modulation use 50% as neutral. Modulation X offsets filter cutoff; Y offsets resonance. Start and length are stored note positions before channel swing, gate and shift.")}</FieldDescription>
        <Button type="button" variant="outline" size="sm" disabled={pending} onClick={() => {
          change("release", "50"); change("finePitchCents", "0"); change("modulationX", "50"); change("modulationY", "50")
          change("glideTicks", "240"); setArticulation("normal"); setArticulationTouched(true)
        }}>{t("Reset expression")}</Button>
        {!current && <Alert><AlertDescription>{t("The project changed while this dialog was open. Close it and reopen the current notes.")}</AlertDescription></Alert>}
        {invalid && <Alert><AlertDescription>{t("Use values within the displayed ranges and whole ticks, MIDI keys and cents.")}</AlertDescription></Alert>}
        {failed && <Alert><AlertDescription>{t("The captured notes could not be updated. Reopen the dialog to capture their current values.")}</AlertDescription></Alert>}
        <DialogFooter>
          <Button type="button" variant="ghost" disabled={pending} onClick={closeNoteProperties}>{t("Cancel")}</Button>
          <Button type="submit" disabled={pending || !current || invalid || !hasEdits}>{pending ? t("Applying…") : t("Apply properties")}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

export function NotePropertiesDialog() {
  const request = useProperties((state) => state.request)
  return request ? <PropertyForm key={`${request.generation}:${request.revision}:${request.pattern}:${request.channel}`} request={request} /> : null
}
