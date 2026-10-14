import type { MeterChange, TimeSignature } from "@/bindings"
import type { TimeGridSpec } from "@/lib/canvas"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"
import { meterSegments } from "@/lib/timeline"
import { TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export type SnapMode = "none" | "step" | "beat" | "bar"

export const SNAP_MODES: { mode: SnapMode; label: string; about: string }[] = [
  { mode: "bar", get label() { return t("Bar") }, get about() { return t("Clips land on bar lines") } },
  { mode: "beat", get label() { return t("Beat") }, get about() { return t("Clips land on beats") } },
  { mode: "step", get label() { return t("Step") }, get about() { return t("Clips land on sixteenth notes") } },
  { mode: "none", get label() { return t("None") }, get about() { return t("Clips land exactly where dropped") } },
]

/** Grid size in ticks, or 0 when snapping is off. */
export function snapTicks(mode: SnapMode, signature: TimeSignature): number {
  switch (mode) {
    case "none":
      return 0
    case "step":
      return TICKS_PER_STEP
    case "beat":
      return ticksPerBeat(signature)
    case "bar":
      return ticksPerBar(signature)
    default: {
      const _exhaustive: never = mode
      return _exhaustive
    }
  }
}

/** What an arrow key moves a clip by: the snap, or a step when snap is off. */
export function nudgeTicks(mode: SnapMode, signature: TimeSignature): number {
  return snapTicks(mode, signature) || TICKS_PER_STEP
}

/**
 * The lines the grid draws for a snap setting. The finest line is the snap
 * itself, so what is drawn is what clips land on. Zooming out drops the
 * finest lines by itself.
 */
export function gridSpecFor(
  mode: SnapMode,
  signature: TimeSignature,
  meters: readonly MeterChange[] = []
): TimeGridSpec {
  if (meters.length > 0) {
    return {
      ...gridSpecFor(mode, signature),
      segments: meterSegments(signature, meters).map((segment) => ({
        ...gridSpecFor(mode, segment.signature),
        start: segment.start,
        end: segment.end,
      })),
    }
  }
  const beat = ticksPerBeat(signature)
  switch (mode) {
    case "bar":
      // Bars, with every fourth one stronger.
      return {
        ticksPerStep: ticksPerBar(signature),
        stepsPerBeat: 1,
        beatsPerBar: 4,
      }
    case "beat":
      return {
        ticksPerStep: beat,
        stepsPerBeat: signature.numerator,
        beatsPerBar: 4,
      }
    case "step":
    case "none":
      return {
        ticksPerStep: TICKS_PER_STEP,
        stepsPerBeat: Math.max(1, beat / TICKS_PER_STEP),
        beatsPerBar: signature.numerator,
      }
    default: {
      const _exhaustive: never = mode
      return _exhaustive
    }
  }
}
