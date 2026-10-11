/*
 * Fitting a sampler's sample to the tempo: how long the played region is,
 * and what makes it last a whole number of beats. Independent stretch does
 * it with a duration ratio and keeps the pitch; tape does it with the tune
 * and changes the pitch with the speed.
 */

/** The duration ratios independent stretch accepts. */
export const MIN_FIT_RATIO = 0.25
export const MAX_FIT_RATIO = 4
/** The tune range of the sampler, in semitones either way. */
export const MAX_FIT_TUNE = 48
/** The beat counts offered as a starting point, shortest first. */
export const FIT_BEATS = [0.25, 0.5, 1, 2, 4, 8, 16, 32, 64] as const

const usable = (value: number) => Number.isFinite(value) && value > 0

/** Seconds the trimmed region lasts at its recorded pitch. */
export function regionSeconds(
  durationSecs: number,
  start: number,
  end: number
): number {
  if (!usable(durationSecs)) return 0
  const from = Number.isFinite(start) ? Math.min(1, Math.max(0, start)) : 0
  const to = Number.isFinite(end) ? Math.min(1, Math.max(0, end)) : 1
  return Math.max(0, to - from) * durationSecs
}

/** Seconds that `beats` beats last at `bpm`. Zero when either is unusable. */
export function beatsSeconds(beats: number, bpm: number): number {
  return usable(beats) && usable(bpm) ? (beats * 60) / bpm : 0
}

/**
 * The offered beat count nearest the region's own length at this tempo, so
 * the first suggestion changes the sample least. Compared as ratios: twice
 * too long is as far off as half too short.
 */
export function suggestedBeats(seconds: number, bpm: number): number {
  if (!usable(seconds) || !usable(bpm)) return 4
  const natural = (seconds * bpm) / 60
  let best: number = FIT_BEATS[0]
  for (const beats of FIT_BEATS)
    if (
      Math.abs(Math.log2(beats / natural)) < Math.abs(Math.log2(best / natural))
    )
      best = beats
  return best
}

/**
 * The duration ratio that makes the region last `beats` beats. Null when
 * the region or the request is unusable, or the ratio is outside what
 * independent stretch accepts.
 */
export function tempoFitRatio(
  seconds: number,
  beats: number,
  bpm: number
): number | null {
  const target = beatsSeconds(beats, bpm)
  if (!usable(seconds) || target === 0) return null
  const ratio = target / seconds
  return ratio >= MIN_FIT_RATIO && ratio <= MAX_FIT_RATIO ? ratio : null
}

/**
 * The tune, in semitones, at which tape playback of the root key lasts
 * `beats` beats: faster and higher for a region that is too long, slower
 * and lower for one that is too short. Null when unusable or out of range.
 */
export function tempoFitTune(
  seconds: number,
  beats: number,
  bpm: number
): number | null {
  const target = beatsSeconds(beats, bpm)
  if (!usable(seconds) || target === 0) return null
  const tune = 12 * Math.log2(seconds / target)
  return Math.abs(tune) <= MAX_FIT_TUNE ? tune : null
}
