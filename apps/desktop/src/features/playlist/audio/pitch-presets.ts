import type { AudioClipUpdate, ClipId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const PITCH_PRESETS = [
  { get label() { return t("Octave down") }, pitch: -12 },
  { get label() { return t("Fifth down") }, pitch: -7 },
  { get label() { return t("Unison") }, pitch: 0 },
  { get label() { return t("Fifth up") }, pitch: 7 },
  { get label() { return t("Octave up") }, pitch: 12 },
] as const

type PitchClip = {
  id: ClipId
  pitch: number
}

export function pitchPresetUpdates(
  clips: readonly PitchClip[],
  preset: number
): AudioClipUpdate[] {
  return clips.flatMap(({ id, pitch }) =>
    Math.abs(pitch - preset) < 0.001 ? [] : [{ id, patch: { pitch: preset } }]
  )
}
