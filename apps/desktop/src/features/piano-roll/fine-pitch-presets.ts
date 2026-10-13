import type { NoteExpression, NoteId } from "@/bindings"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { translatedText as t } from "@/lib/i18n"

export const FINE_PITCH_PRESETS = [
  { get label() { return t("Octave down") }, finePitchCents: -1200 },
  { get label() { return t("Semitone down") }, finePitchCents: -100 },
  { get label() { return t("In tune") }, finePitchCents: 0 },
  { get label() { return t("Semitone up") }, finePitchCents: 100 },
  { get label() { return t("Octave up") }, finePitchCents: 1200 },
] as const

type FinePitchNote = {
  id: NoteId
  expression?: NoteExpression
}

export function finePitchPresetUpdates(
  notes: readonly FinePitchNote[],
  preset: number
): { id: NoteId; expression: NoteExpression }[] {
  return notes.flatMap(({ id, expression }) => {
    const current = expression ? expression.finePitchCents : 0
    return Math.abs(current - preset) < 0.001
      ? []
      : [
          {
            id,
            expression: {
              ...(expression ?? DEFAULT_NOTE_EXPRESSION),
              finePitchCents: preset,
            },
          },
        ]
  })
}
