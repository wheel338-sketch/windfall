import type { Clip, ClipContent, ClipStretch, Command } from "@/bindings"
import { MAX_SONG_TICKS } from "@/lib/units"
import { playbackSpeed } from "./geometry"
import { translatedText as t, historyLabel } from "@/lib/i18n"
export type AudioClip = Clip & {
  content: Extract<ClipContent, { type: "audio" }>
}
/** Preserve the same source trim when playback duration changes; one reversible edit. */
export function processingCommand(
  clips: readonly AudioClip[],
  stretch: ClipStretch,
  pitch: number
): Command {
  if (
    !Number.isFinite(pitch) ||
    Math.abs(pitch) > (stretch.mode === "spectral" ? 24 : 48)
  )
    throw new Error(t("Pitch is outside the supported range."))
  if (
    stretch.mode === "spectral" &&
    (!Number.isFinite(stretch.ratio) ||
      stretch.ratio < 0.25 ||
      stretch.ratio > 4)
  )
    throw new Error(t("Duration must be between 0.25 and 4 times the source."))
  const factors = clips.map(
    (clip) => playbackSpeed(clip.content) / playbackSpeed({ pitch, stretch })
  )
  const scale = (value: number, i: number) => Math.round(value * factors[i])
  const timing = clips.map((clip, i) => {
    const length = Math.max(1, scale(clip.length, i)),
      offset = scale(clip.offset, i)
    if (clip.start + length > MAX_SONG_TICKS || offset > MAX_SONG_TICKS)
      throw new Error(
        t("The stretched clip would exceed the song's maximum length.")
      )
    return { id: clip.id, patch: { length, offset } }
  })
  return {
    type: "batch",
    label: historyLabel("Process audio clips"),
    commands: [
      { type: "updateClips", updates: timing },
      {
        type: "updateAudioClips",
        updates: clips.map((clip, i) => ({
          id: clip.id,
          patch: {
            stretch,
            pitch,
            fadeIn: scale(clip.content.fadeIn, i),
            fadeOut: scale(clip.content.fadeOut, i),
          },
        })),
      },
    ],
  }
}
export function fitRatio(sourceBpm: number, targetBpm: number): number {
  const ratio = sourceBpm / targetBpm
  if (
    !Number.isFinite(sourceBpm) ||
    sourceBpm <= 0 ||
    !Number.isFinite(targetBpm) ||
    targetBpm <= 0 ||
    ratio < 0.25 ||
    ratio > 4
  )
    throw new Error(
      t("Enter a positive source tempo that fits within the 0.25 to 4 duration range.")
    )
  return ratio
}
export function beatFitRatio(
  beats: number,
  seconds: number,
  targetBpm: number
): number {
  if (
    !Number.isFinite(beats) ||
    beats <= 0 ||
    !Number.isFinite(seconds) ||
    seconds <= 0
  )
    throw new Error(t("Enter a positive beat count for the whole source file."))
  return fitRatio((beats * 60) / seconds, targetBpm)
}
