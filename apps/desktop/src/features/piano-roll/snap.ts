import type { NoteMusicalGrid, TimeSignature, Timeline } from "@/bindings"
import { MIN_LINE_SPACING_PX, type TimeGridSpec } from "@/lib/canvas"
import { ticksPerBar, ticksPerBeat } from "@/lib/time"
import { TICKS_PER_STEP } from "@/lib/units"
import { translatedText as t } from "@/lib/i18n"

export type SnapId =
  | "none"
  | "step/6"
  | "step/4"
  | "step/3"
  | "step/2"
  | "step"
  | "beat/6"
  | "beat/4"
  | "beat/3"
  | "beat/2"
  | "beat"
  | "bar"

export type SnapOption = {
  id: SnapId
  label: string
  /** Options are listed in groups, finest first. */
  group: "none" | "step" | "beat" | "bar"
}

export const SNAP_OPTIONS: SnapOption[] = [
  { id: "none", get label() { return t("None") }, group: "none" },
  { id: "step/6", get label() { return t("1/6 step") }, group: "step" },
  { id: "step/4", get label() { return t("1/4 step") }, group: "step" },
  { id: "step/3", get label() { return t("1/3 step") }, group: "step" },
  { id: "step/2", get label() { return t("1/2 step") }, group: "step" },
  { id: "step", get label() { return t("Step") }, group: "step" },
  { id: "beat/6", get label() { return t("1/6 beat") }, group: "beat" },
  { id: "beat/4", get label() { return t("1/4 beat") }, group: "beat" },
  { id: "beat/3", get label() { return t("1/3 beat") }, group: "beat" },
  { id: "beat/2", get label() { return t("1/2 beat") }, group: "beat" },
  { id: "beat", get label() { return t("Beat") }, group: "beat" },
  { id: "bar", get label() { return t("Bar") }, group: "bar" },
]

export const DEFAULT_SNAP: SnapId = "step"

export function isSnapId(value: unknown): value is SnapId {
  return SNAP_OPTIONS.some((option) => option.id === value)
}

/** The snap interval in ticks. 0 means snapping is off. */
export function snapTicks(id: SnapId, signature: TimeSignature): number {
  if (id === "none") return 0
  if (id === "bar") return ticksPerBar(signature)
  const [unit, divisor] = id.split("/")
  const whole = unit === "step" ? TICKS_PER_STEP : ticksPerBeat(signature)
  return Math.max(1, Math.round(whole / Number(divisor ?? 1)))
}

export function snapMusicalGrid(id: SnapId, signature: TimeSignature, timeline?: Timeline): NoteMusicalGrid | null {
  if (id === "none") return null
  const [unit, divisor] = id.split("/") as [NoteMusicalGrid["unit"], string?]
  return { signature, meters: timeline?.meters ?? [], unit, divisor: Number(divisor ?? 1) }
}

/**
 * Which time lines the grid draws. The finest lines follow the snap while
 * they are far enough apart to read; zoomed out they fall back to steps and
 * then to beats, and the canvas core thins them further from there.
 */
export function gridSpecFor(
  snap: number,
  pxPerTick: number,
  signature: TimeSignature
): TimeGridSpec {
  const beat = ticksPerBeat(signature)
  const candidates = [snap, TICKS_PER_STEP, beat].filter(
    (ticks) => ticks > 0 && ticks <= beat && beat % ticks === 0
  )
  const minor =
    candidates.find((ticks) => ticks * pxPerTick >= MIN_LINE_SPACING_PX) ?? beat
  return {
    ticksPerStep: minor,
    stepsPerBeat: beat / minor,
    beatsPerBar: signature.numerator,
  }
}

/** Rounds down to the snap, which is the cell a click lands in. */
export function snapFloor(tick: number, snap: number, origin = 0): number {
  return snap > 0 ? origin + Math.floor((tick - origin) / snap) * snap : Math.round(tick)
}

export function snapCeil(tick: number, snap: number, origin = 0): number {
  return snap > 0 ? origin + Math.ceil((tick - origin) / snap) * snap : Math.round(tick)
}

export function snapRound(tick: number, snap: number, origin = 0): number {
  return snap > 0 ? origin + Math.round((tick - origin) / snap) * snap : Math.round(tick)
}
