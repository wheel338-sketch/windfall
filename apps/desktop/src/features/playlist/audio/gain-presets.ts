import type { AudioClipUpdate, ClipId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const GAIN_PRESETS = [
  { get label() { return t("Quiet") }, gain: 0.5 },
  { get label() { return t("Unity") }, gain: 1 },
  { get label() { return t("Loud") }, gain: 1.5 },
] as const

type GainClip = {
  id: ClipId
  gain: number
}

export function gainPresetUpdates(
  clips: readonly GainClip[],
  preset: number
): AudioClipUpdate[] {
  return clips.flatMap(({ id, gain }) =>
    Math.abs(gain - preset) < 0.001 ? [] : [{ id, patch: { gain: preset } }]
  )
}
