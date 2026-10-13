import type { NoteExpression, NoteId } from "@/bindings"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { translatedText as t } from "@/lib/i18n"

export const MODULATION_PRESETS = [
  { get label() { return t("Low") }, modulation: 0 },
  { get label() { return t("Center") }, modulation: 0.5 },
  { get label() { return t("High") }, modulation: 1 },
] as const

type ModulationNote = {
  id: NoteId
  expression?: NoteExpression
}

export function modulationPresetUpdates(
  notes: readonly ModulationNote[],
  axis: "modulationX" | "modulationY",
  preset: number
): { id: NoteId; expression: NoteExpression }[] {
  return notes.flatMap(({ id, expression }) => {
    const current = expression ? expression[axis] : 0.5
    return Math.abs(current - preset) < 0.001
      ? []
      : [
          {
            id,
            expression: {
              ...(expression ?? DEFAULT_NOTE_EXPRESSION),
              [axis]: preset,
            },
          },
        ]
  })
}
