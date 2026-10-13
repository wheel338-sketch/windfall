import type { NoteExpression, NoteId } from "@/bindings"
import { DEFAULT_NOTE_EXPRESSION } from "@/lib/note-expression"
import { dispatch } from "@/lib/store/project"
import { PPQ, TICKS_PER_STEP } from "@/lib/units"

import { currentSession } from "./session"
import { translatedText as t } from "@/lib/i18n"

export const GLIDE_PRESETS = [
  { get label() { return t("16th") }, glideTicks: TICKS_PER_STEP },
  { get label() { return t("8th") }, glideTicks: TICKS_PER_STEP * 2 },
  { get label() { return t("Quarter") }, glideTicks: PPQ },
  { get label() { return t("Half") }, glideTicks: PPQ * 2 },
] as const

type GlideNote = {
  id: NoteId
  expression?: NoteExpression
}

export function glidePresetUpdates(
  notes: readonly GlideNote[],
  preset: number
): { id: NoteId; expression: NoteExpression }[] {
  return notes.flatMap(({ id, expression }) => {
    const current = expression?.glideTicks ?? DEFAULT_NOTE_EXPRESSION.glideTicks
    return current === preset
      ? []
      : [
          {
            id,
            expression: {
              ...DEFAULT_NOTE_EXPRESSION,
              ...expression,
              glideTicks: preset,
            },
          },
        ]
  })
}

export async function setSelectedGlide(preset: number): Promise<void> {
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return

  const updates = glidePresetUpdates(notes, preset).map(
    ({ id, expression }) => ({
      id,
      patch: { expression },
    })
  )
  if (updates.length) {
    await dispatch({
      type: "updateCapturedNotes",
      pattern: context.pattern.id,
      channel: context.channel,
      expected: notes,
      updates,
    })
  }
}
