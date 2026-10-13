import type { NoteId } from "@/bindings"
import { dispatch } from "@/lib/store/project"
import { PPQ, TICKS_PER_STEP } from "@/lib/units"

import { currentSession } from "./session"
import { translatedText as t } from "@/lib/i18n"

export const LENGTH_PRESETS = [
  { get label() { return t("16th") }, length: TICKS_PER_STEP },
  { get label() { return t("8th") }, length: TICKS_PER_STEP * 2 },
  { get label() { return t("Quarter") }, length: PPQ },
  { get label() { return t("Half") }, length: PPQ * 2 },
  { get label() { return t("Whole") }, length: PPQ * 4 },
] as const

type LengthNote = {
  id: NoteId
  length: number
}

export function lengthPresetUpdates(
  notes: readonly LengthNote[],
  preset: number
): { id: NoteId; length: number }[] {
  return notes.flatMap(({ id, length }) =>
    length === preset ? [] : [{ id, length: preset }]
  )
}

export async function setSelectedLength(preset: number): Promise<void> {
  const session = currentSession()
  const context = session?.editor.context
  const notes = session?.editor.selectedNotes() ?? []
  if (!context || session?.editor.busy || !notes.length) return

  const updates = lengthPresetUpdates(notes, preset).map(({ id, length }) => ({
    id,
    patch: { length },
  }))
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
