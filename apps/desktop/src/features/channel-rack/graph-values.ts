import type { Note, NotePatch } from "@/bindings"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { clamp, DEFAULT_KEY, DEFAULT_VELOCITY, TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

export const GRAPH_PROPERTIES = ["velocity", "pan", "key", "release", "finePitchCents", "modulationX", "modulationY", "length", "shift"] as const
export type GraphProperty = (typeof GRAPH_PROPERTIES)[number]

export const GRAPH_LABELS: Record<GraphProperty, string> = {
  get velocity() { return t("Velocity") },
  get pan() { return t("Pan") },
  get key() { return t("Note") },
  get length() { return t("Length") },
  get shift() { return t("Shift within step") },
  get release() { return t("Release") },
  get finePitchCents() { return t("Fine pitch") },
  get modulationX() { return t("Modulation X") },
  get modulationY() { return t("Modulation Y") },
}

export function graphRange(property: GraphProperty, lengthRange: number) {
  switch (property) {
    case "velocity": return { min: 0, max: 1, step: 0.01, reset: DEFAULT_VELOCITY }
    case "pan": return { min: -1, max: 1, step: 0.01, reset: 0 }
    case "key": return { min: 0, max: 127, step: 1, reset: DEFAULT_KEY }
    case "length": return { min: 1, max: Math.max(1, lengthRange), step: 1, reset: TICKS_PER_STEP }
    case "shift": return { min: 0, max: TICKS_PER_STEP - 1, step: 1, reset: 0 }
    case "release":
    case "modulationX":
    case "modulationY": return { min: 0, max: 1, step: 0.01, reset: 0.5 }
    case "finePitchCents": return { min: -1200, max: 1200, step: 1, reset: 0 }
  }
}

export function graphValue(note: Note, property: GraphProperty): number {
  if (property === "shift") return note.start % TICKS_PER_STEP
  if (property === "release" || property === "finePitchCents" || property === "modulationX" || property === "modulationY") return (note.expression ?? DEFAULT_NOTE_EXPRESSION)[property]
  return note[property]
}

export function graphPatch(note: Note, property: GraphProperty, value: number, lengthRange: number): NotePatch {
  const range = graphRange(property, lengthRange)
  const next = clamp(range.min + Math.round((value - range.min) / range.step) * range.step, range.min, range.max)
  if (property === "shift") return { start: Math.floor(note.start / TICKS_PER_STEP) * TICKS_PER_STEP + next }
  if (property === "release" || property === "finePitchCents" || property === "modulationX" || property === "modulationY") return { expression: { ...(note.expression ?? DEFAULT_NOTE_EXPRESSION), [property]: next } }
  return { [property]: next }
}

/** Includes off-grid notes and chords, without manufacturing empty-step notes. */
export function graphSteps(notes: readonly Note[], lengthSteps: number): Note[][] {
  const steps = Array.from({ length: lengthSteps }, () => [] as Note[])
  for (const note of notes) {
    const step = Math.floor(note.start / TICKS_PER_STEP)
    if (step >= 0 && step < steps.length) steps[step].push(note)
  }
  for (const step of steps) step.sort((a, b) => a.start - b.start || a.key - b.key || a.id - b.id)
  return steps
}

export function graphFraction(note: Note, property: GraphProperty, lengthRange: number): number {
  const { min, max } = graphRange(property, lengthRange)
  return clamp((graphValue(note, property) - min) / (max - min), 0, 1)
}

export function graphReadout(property: GraphProperty, value: number): string {
  if (property === "velocity" || property === "release" || property === "modulationX" || property === "modulationY") return `${Math.round(value * 100)}%`
  if (property === "pan") return value === 0 ? t("Center") : `${Math.round(Math.abs(value) * 100)}% ${value < 0 ? "left" : "right"}`
  if (property === "key") return `MIDI ${Math.round(value)}`
  if (property === "finePitchCents") return message("{value}{value2} cents", { value: value > 0 ? t("+") : "", value2: plainNumber(Math.round(value)) })
  return message("{value} ticks", { value: plainNumber(Math.round(value)) })
}
