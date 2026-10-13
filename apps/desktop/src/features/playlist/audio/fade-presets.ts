import type { AudioClipUpdate, ClipId } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

export const FADE_PRESETS = [
  { get label() { return t("None") }, fraction: 0 },
  { get label() { return t("Short") }, fraction: 1 / 16 },
  { get label() { return t("Medium") }, fraction: 1 / 8 },
  { get label() { return t("Long") }, fraction: 1 / 4 },
] as const

type FadeClip = {
  id: ClipId
  length: number
  fadeIn: number
  fadeOut: number
}

export function fadePresetUpdates(
  clips: readonly FadeClip[],
  fraction: number
): AudioClipUpdate[] {
  return clips.flatMap(({ id, length, fadeIn, fadeOut }) => {
    const target = Math.max(0, Math.min(length, Math.round(length * fraction)))
    return fadeIn === target && fadeOut === target
      ? []
      : [{ id, patch: { fadeIn: target, fadeOut: target } }]
  })
}
