import type { SamplerPreprocessRequest } from "@/bindings"
import type { SamplerChannel } from "@/lib/channel-source"

/** The longest fade the sample pre-processor accepts, in milliseconds. */
export const MAX_PREPROCESS_FADE_MS = 60_000
/** The largest share of the loop that can blend across its seam. */
export const MAX_PREPROCESS_CROSSFADE = 0.5

/** The treatments chosen in the pre-processing form. */
export type PreprocessChoices = {
  trim: boolean
  reverse: boolean
  removeDc: boolean
  invertPolarity: boolean
  swapStereo: boolean
  normalize: boolean
  /** Text of the two fade fields, in milliseconds. Empty means no fade. */
  fadeInMs: string
  fadeOutMs: string
  /** Share of the loop to blend across its seam, 0 to 0.5. */
  loopCrossfade: number
}

export const NO_PREPROCESSING: PreprocessChoices = {
  trim: false,
  reverse: false,
  removeDc: false,
  invertPolarity: false,
  swapStereo: false,
  normalize: false,
  fadeInMs: "",
  fadeOutMs: "",
  loopCrossfade: 0,
}

/** Whether the channel has a forward loop, the only kind with a seam. */
export function hasLoopSeam(channel: SamplerChannel): boolean {
  return channel.source.loopMode === "forward"
}

/** Whether trimming would remove anything. */
export function isTrimmed(channel: SamplerChannel): boolean {
  return channel.source.start > 0 || channel.source.end < 1
}

function fade(text: string): number | null {
  if (text.trim() === "") return 0
  const value = Number(text)
  return Number.isFinite(value) && value >= 0 && value <= MAX_PREPROCESS_FADE_MS
    ? value
    : null
}

/**
 * The request for the chosen treatments, or the reason they cannot be
 * applied. Treatments that are off are left out of the request.
 */
export function preprocessRequest(
  channel: SamplerChannel,
  choices: PreprocessChoices
): { request: SamplerPreprocessRequest } | { error: string } {
  if (channel.source.sample === null)
    return { error: "Load a sample into the channel first." }
  const fadeInMs = fade(choices.fadeInMs)
  const fadeOutMs = fade(choices.fadeOutMs)
  if (fadeInMs === null || fadeOutMs === null)
    return { error: "A fade is a time from 0 to 60000 ms." }
  const loopCrossfade = hasLoopSeam(channel)
    ? Math.min(MAX_PREPROCESS_CROSSFADE, Math.max(0, choices.loopCrossfade))
    : 0
  const request: SamplerPreprocessRequest = {
    channel: channel.id,
    ...(choices.trim && isTrimmed(channel) ? { trim: true } : {}),
    ...(choices.reverse ? { reverse: true } : {}),
    ...(choices.removeDc ? { removeDc: true } : {}),
    ...(choices.invertPolarity ? { invertPolarity: true } : {}),
    ...(choices.swapStereo ? { swapStereo: true } : {}),
    ...(fadeInMs > 0 ? { fadeInMs } : {}),
    ...(fadeOutMs > 0 ? { fadeOutMs } : {}),
    ...(loopCrossfade > 0 ? { loopCrossfade } : {}),
    ...(choices.normalize ? { normalize: true } : {}),
  }
  if (Object.keys(request).length === 1)
    return { error: "Choose at least one treatment to apply." }
  return { request }
}
