import type {
  LoudnessReading,
  MasteringOptions,
  MasteringReview,
} from "@/bindings"
import { formatNumber, translatedText as t, message, plainNumber } from "@/lib/i18n"

/*
 * Mastering on export: the form's side of `windfall_dsp::mastering`. The
 * ranges are the ones `MasteringSettings::sanitized` holds the values to.
 */

export const MASTERING_TARGET = { min: -30, max: -6 } as const
export const MASTERING_CEILING = { min: -9, max: 0 } as const
/** Within this many LU of the target a master counts as on target. */
export const MASTERING_TOLERANCE_LU = 0.3
/** The reading that stands for silence. */
export const LOUDNESS_FLOOR = -120

/** What the form holds. The two levels stay text until they are sent. */
export type MasteringDraft = {
  enabled: boolean
  target: string
  ceiling: string
  dynamics: number
}

export const DEFAULT_MASTERING_DRAFT: MasteringDraft = {
  enabled: false,
  target: "-14",
  ceiling: "-1",
  dynamics: 0.5,
}

export type MasteringPreset = {
  id: string
  label: string
  target: string
  ceiling: string
}

/** Common destinations. Any other pair of values is "Custom". */
export const MASTERING_PRESETS: readonly MasteringPreset[] = [
  { id: "streaming", get label() { return t("Streaming, −14 LUFS") }, target: "-14", ceiling: "-1" },
  { id: "dynamic", get label() { return t("Dynamic, −16 LUFS") }, target: "-16", ceiling: "-1" },
  { id: "broadcast", get label() { return t("Broadcast, −23 LUFS") }, target: "-23", ceiling: "-1" },
  { id: "loud", get label() { return t("Loud, −9 LUFS") }, target: "-9", ceiling: "-0.3" },
]

export const CUSTOM_PRESET = "custom"

export const DYNAMICS_CHOICES: readonly { value: number; label: string }[] = [
  { value: 0, get label() { return t("Off: gain and limiting only") } },
  { value: 0.25, get label() { return t("Gentle") } },
  { value: 0.5, get label() { return t("Balanced") } },
  { value: 0.75, get label() { return t("Firm") } },
  { value: 1, get label() { return t("Tight") } },
]

/** A number typed into the form, if it is one and lies in the range. */
export function parseLevel(
  text: string,
  range: { min: number; max: number }
): number | null {
  const cleaned = text.trim().replace("−", "-").replace(",", ".")
  if (cleaned === "" || !/^-?\d+(\.\d+)?$/.test(cleaned)) return null
  const value = Number(cleaned)
  return value >= range.min && value <= range.max ? value : null
}

/** The preset a draft's two levels are, or "custom". */
export function presetOf(draft: Pick<MasteringDraft, "target" | "ceiling">) {
  const target = parseLevel(draft.target, MASTERING_TARGET)
  const ceiling = parseLevel(draft.ceiling, MASTERING_CEILING)
  const preset = MASTERING_PRESETS.find(
    (item) => Number(item.target) === target && Number(item.ceiling) === ceiling
  )
  return preset?.id ?? CUSTOM_PRESET
}

/** Said when the target is not a loudness mastering can aim for. */
export function badTarget(): string {
  return message("Enter a loudness from {min} to {max} LUFS.", { min: plainNumber(MASTERING_TARGET.min), max: plainNumber(MASTERING_TARGET.max) })
}
/** Said when the ceiling is not one mastering can hold. */
export function badCeiling(): string {
  return message("Enter a ceiling from {min} to {max} dBTP.", { min: plainNumber(MASTERING_CEILING.min), max: plainNumber(MASTERING_CEILING.max) })
}
/** Said when stems are on: mastering needs the whole mix. */
export function notWithStems(): string {
  return t("Mastering works on the whole mix. Turn off stems to master the export.")
}

export type MasteringRequest = {
  /** What to send, when mastering is on and everything is in order. */
  options: MasteringOptions | undefined
  targetError: string | null
  ceilingError: string | null
  stemsError: string | null
}

/** What a draft asks the export for, or what is wrong with it. */
export function masteringRequest(
  draft: MasteringDraft,
  stems: boolean
): MasteringRequest {
  if (!draft.enabled) {
    return {
      options: undefined,
      targetError: null,
      ceilingError: null,
      stemsError: null,
    }
  }
  const targetLufs = parseLevel(draft.target, MASTERING_TARGET)
  const ceilingDbtp = parseLevel(draft.ceiling, MASTERING_CEILING)
  const stemsError = stems ? notWithStems() : null
  const ready = targetLufs !== null && ceilingDbtp !== null && !stemsError
  return {
    options: ready
      ? { targetLufs, ceilingDbtp, dynamics: draft.dynamics }
      : undefined,
    targetError: targetLufs === null ? badTarget() : null,
    ceilingError: ceilingDbtp === null ? badCeiling() : null,
    stemsError,
  }
}

/** A reading with a fixed number of decimals, in the decimal mark of the language shown. */
function fixed(value: number, digits = 1): string {
  return formatNumber(value, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false })
}

function signed(value: number, digits = 1): string {
  const rounded = Number(value.toFixed(digits))
  if (rounded === 0) return fixed(0, digits)
  return `${rounded < 0 ? "−" : "+"}${fixed(Math.abs(rounded), digits)}`
}

/** "−14.0 LUFS", and "silent" for a reading at the floor. */
export function formatLevel(value: number, unit: string): string {
  if (!Number.isFinite(value) || value <= LOUDNESS_FLOOR) return t("silent")
  const rounded = Number(value.toFixed(1))
  const text =
    rounded < 0 ? `−${fixed(Math.abs(rounded))}` : fixed(rounded)
  return `${text} ${unit}`
}

export type ReviewRow = {
  id: keyof LoudnessReading | "peakToLoudness"
  label: string
  before: string
  after: string
  /** How the figure moved, such as "+9.2 LU". Empty when it cannot say. */
  change: string
}

function row(
  id: ReviewRow["id"],
  label: string,
  before: number,
  after: number,
  unit: string,
  changeUnit: string
): ReviewRow {
  const measurable =
    before > LOUDNESS_FLOOR &&
    after > LOUDNESS_FLOOR &&
    Number.isFinite(before) &&
    Number.isFinite(after)
  return {
    id,
    label,
    before: formatLevel(before, unit),
    after: formatLevel(after, unit),
    change: measurable ? `${signed(after - before)} ${changeUnit}` : "",
  }
}

/** The figures of a review, before and after, in the order they are read. */
export function reviewRows(review: MasteringReview): ReviewRow[] {
  const { before, after } = review
  const audible =
    before.integratedLufs > LOUDNESS_FLOOR &&
    after.integratedLufs > LOUDNESS_FLOOR
  const range = (reading: LoudnessReading) => `${fixed(reading.loudnessRangeLu)} LU`
  const crest = (reading: LoudnessReading) =>
    reading.truePeakDbtp - reading.integratedLufs
  return [
    row("integratedLufs", t("Loudness"), before.integratedLufs, after.integratedLufs, "LUFS", "LU"),
    row("truePeakDbtp", t("True peak"), before.truePeakDbtp, after.truePeakDbtp, "dBTP", "dB"),
    row("samplePeakDb", t("Sample peak"), before.samplePeakDb, after.samplePeakDb, "dBFS", "dB"),
    row(
      "shortTermMaxLufs",
      t("Loudest 3 seconds"),
      before.shortTermMaxLufs,
      after.shortTermMaxLufs,
      "LUFS",
      "LU"
    ),
    {
      id: "loudnessRangeLu",
      label: t("Loudness range"),
      before: range(before),
      after: range(after),
      change: `${signed(after.loudnessRangeLu - before.loudnessRangeLu)} LU`,
    },
    {
      id: "peakToLoudness",
      label: t("Peak to loudness"),
      before: audible ? `${fixed(crest(before))} LU` : t("silent"),
      after: audible ? `${fixed(crest(after))} LU` : t("silent"),
      change: audible ? `${signed(crest(after) - crest(before))} LU` : "",
    },
  ]
}

/** One sentence on whether the master is what was asked for. */
export function reviewVerdict(review: MasteringReview): string {
  const target = formatLevel(review.targetLufs, "LUFS")
  const ceiling = formatLevel(review.ceilingDbtp, "dBTP")
  if (review.targetReached) {
    return message("On target: {target} under a ceiling of {ceiling}.", { target, ceiling })
  }
  const short = review.targetLufs - review.after.integratedLufs
  const over = review.after.truePeakDbtp - review.ceilingDbtp
  if (over > 0.05) {
    return message("The true peak is {over} dB over the ceiling of {ceiling}. Lower the target or the ceiling and export again.", { over: fixed(over), ceiling })
  }
  if (short > MASTERING_TOLERANCE_LU) {
    return message("{short} LU under the target of {target}: this mix cannot be that loud under a ceiling of {ceiling} without being crushed. Lower the target or raise the ceiling.", { short: fixed(short), target, ceiling })
  }
  return message("{short} LU over the target of {target}.", { short: fixed(Math.abs(short)), target })
}

/** What the chain did, for the line under the table. */
export function reviewSummary(review: MasteringReview): string {
  const parts = [message("{signed} dB of gain", { signed: signed(review.gainDb) })]
  if (review.compressionDb > 0.05) {
    parts.push(message("up to {amount} dB of band compression", { amount: fixed(review.compressionDb) }))
  }
  parts.push(
    review.limitingDb > 0.05
      ? message("up to {amount} dB of limiting", { amount: fixed(review.limitingDb) })
      : t("no limiting")
  )
  return message("{parts}, in {passes} passes.", { parts: parts.join(", "), passes: plainNumber(review.passes) })
}
