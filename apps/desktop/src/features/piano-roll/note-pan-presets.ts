import type { NoteId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const NOTE_PAN_PRESETS = [
  { get label() { return t("Hard left") }, pan: -1 },
  { get label() { return t("Left") }, pan: -0.5 },
  { get label() { return t("Center") }, pan: 0 },
  { get label() { return t("Right") }, pan: 0.5 },
  { get label() { return t("Hard right") }, pan: 1 },
] as const

type PanNote = {
  id: NoteId
  pan: number
}

export function notePanPresetUpdates(
  notes: readonly PanNote[],
  preset: number
): PanNote[] {
  return notes.flatMap(({ id, pan }) =>
    Math.abs(pan - preset) < 0.001 ? [] : [{ id, pan: preset }]
  )
}
