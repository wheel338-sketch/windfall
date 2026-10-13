import type { AudioClipUpdate, ClipId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const PAN_PRESETS = [
  { get label() { return t("Hard left") }, pan: -1 },
  { get label() { return t("Left") }, pan: -0.5 },
  { get label() { return t("Center") }, pan: 0 },
  { get label() { return t("Right") }, pan: 0.5 },
  { get label() { return t("Hard right") }, pan: 1 },
] as const

type PanClip = {
  id: ClipId
  pan: number
}

export function panPresetUpdates(
  clips: readonly PanClip[],
  preset: number
): AudioClipUpdate[] {
  return clips.flatMap(({ id, pan }) =>
    Math.abs(pan - preset) < 0.001 ? [] : [{ id, patch: { pan: preset } }]
  )
}
