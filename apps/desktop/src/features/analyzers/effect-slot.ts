import { useMemo } from "react"

import type { EffectId, TrackId } from "@/bindings"

import { useAnalyzerSelection, type AnalyzerSelection } from "./feed"
import type { AnalyzerWant } from "./types"

const NOTHING: AnalyzerWant = {
  scope: false,
  spectrum: false,
  vector: false,
  envelope: false,
}

/** The slot of one effect's editor. Editors open side by side each have one. */
export function effectSlot(effect: EffectId): string {
  return `fx-${effect}`
}

/**
 * Analyzes the track an effect is on, after its fader, for as long as the
 * effect's editor is mounted, and returns the slot its views read.
 *
 * With `bound` the effect's own measurements come with the audio: a
 * limiter's levels and reduction, a tuner's reading. Without a track there
 * is nothing to select and the views stay empty.
 *
 * The analyzer has two sources and four slots for the whole app. An editor
 * that finds them taken says so in its status line and draws nothing.
 */
export function useEffectAnalyzer(
  track: TrackId | undefined,
  effect: EffectId,
  options: { bound: boolean; want?: Partial<AnalyzerWant> }
): string {
  const slot = effectSlot(effect)
  const { bound } = options
  const spectrum = options.want?.spectrum ?? false
  const selection = useMemo<AnalyzerSelection | null>(
    () =>
      track === undefined
        ? null
        : {
            source: { type: "trackPostFader", track },
            effect: bound ? effect : undefined,
            want: { ...NOTHING, spectrum },
          },
    [track, effect, bound, spectrum]
  )
  useAnalyzerSelection(slot, selection)
  return slot
}
