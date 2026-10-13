import type { NoteId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const VELOCITY_PRESETS = [
  { get label() { return t("Soft") }, velocity: 0.25 },
  { get label() { return t("Medium") }, velocity: 0.5 },
  { get label() { return t("Strong") }, velocity: 0.8 },
  { get label() { return t("Full") }, velocity: 1 },
] as const

type VelocityNote = {
  id: NoteId
  velocity: number
}

export function velocityPresetUpdates(
  notes: readonly VelocityNote[],
  preset: number
): VelocityNote[] {
  return notes.flatMap(({ id, velocity }) =>
    Math.abs(velocity - preset) < 0.001 ? [] : [{ id, velocity: preset }]
  )
}
