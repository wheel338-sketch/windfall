import type { NoteArticulation, NoteExpression } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const DEFAULT_NOTE_EXPRESSION: Readonly<NoteExpression> = {
  release: 0.5,
  finePitchCents: 0,
  modulationX: 0.5,
  modulationY: 0.5,
  articulation: "normal",
  glideTicks: 240,
}

export const NOTE_ARTICULATIONS: { value: NoteArticulation; label: string }[] = [
  { value: "normal", get label() { return t("Normal note") } },
  { value: "slide", get label() { return t("Slide held notes") } },
  { value: "portamento", get label() { return t("Portamento note") } },
]

export function isNoteArticulation(value: unknown): value is NoteArticulation {
  return NOTE_ARTICULATIONS.some((item) => item.value === value)
}

/** Copy expression at clipboard/preview boundaries; never share mutable state. */
export function copyNoteExpression(expression: NoteExpression | undefined): NoteExpression | undefined {
  return expression ? { ...expression } : undefined
}
