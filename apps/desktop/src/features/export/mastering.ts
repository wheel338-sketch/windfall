import type {
  LoudnessReading,
  MasteringOptions,
  MasteringReview,
} from "@/bindings"

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
  { id: "streaming", label: "Streaming, −14 LUFS", target: "-14", ceiling: "-1" },
  { id: "dynamic", label: "Dynamic, −16 LUFS", target: "-16", ceiling: "-1" },
  { id: "broadcast", label: "Broadcast, −23 LUFS", target: "-23", ceiling: "-1" },
  { id: "loud", label: "Loud, −9 LUFS", target: "-9", ceiling: "-0.3" },
]

export const CUSTOM_PRESET = "custom"

export const DYNAMICS_CHOICES: readonly { value: number; label: string }[] = [
  { value: 0, label: "Off: gain and limiting only" },
  { value: 0.25, label: "Gentle" },
  { value: 0.5, label: "Balanced" },
  { value: 0.75, label: "Firm" },
  { value: 1, label: "Tight" },
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

export const BAD_TARGET = `Enter a loudness from ${MASTERING_TARGET.min} to ${MASTERING_TARGET.max} LUFS.`
export const BAD_CEILING = `Enter a ceiling from ${MASTERING_CEILING.min} to ${MASTERING_CEILING.max} dBTP.`
export const NOT_WITH_STEMS =
  "Mastering works on the whole mix. Turn off stems to master the export."

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
  const stemsError = stems ? NOT_WITH_STEMS : null
  const ready = targetLufs !== null && ceilingDbtp !== null && !stemsError
  return {
    options: ready
      ? { targetLufs, ceilingDbtp, dynamics: draft.dynamics }
      : undefined,
    targetError: targetLufs === null ? BAD_TARGET : null,
    ceilingError: ceilingDbtp === null ? BAD_CEILING : null,
    stemsError,
  }
}

function signed(value: number, digits = 1): string {
  const rounded = Number(value.toFixed(digits))
  if (rounded === 0) return (0).toFixed(digits)
  return `${rounded < 0 ? "−" : "+"}${Math.abs(rounded).toFixed(digits)}`
}

/** "−14.0 LUFS", and "silent" for a reading at the floor. */
export function formatLevel(value: number, unit: string): string {
  if (!Number.isFinite(value) || value <= LOUDNESS_FLOOR) return "silent"
  const rounded = Number(value.toFixed(1))
  const text =
    rounded < 0 ? `−${Math.abs(rounded).toFixed(1)}` : rounded.toFixed(1)
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
  const range = (reading: LoudnessReading) => `${reading.loudnessRangeLu.toFixed(1)} LU`
  const crest = (reading: LoudnessReading) =>
    reading.truePeakDbtp - reading.integratedLufs
  return [
    row("integratedLufs", "Loudness", before.integratedLufs, after.integratedLufs, "LUFS", "LU"),
    row("truePeakDbtp", "True peak", before.truePeakDbtp, after.truePeakDbtp, "dBTP", "dB"),
    row("samplePeakDb", "Sample peak", before.samplePeakDb, after.samplePeakDb, "dBFS", "dB"),
    row(
      "shortTermMaxLufs",
      "Loudest 3 seconds",
      before.shortTermMaxLufs,
      after.shortTermMaxLufs,
      "LUFS",
      "LU"
    ),
    {
      id: "loudnessRangeLu",
      label: "Loudness range",
      before: range(before),
      after: range(after),
      change: `${signed(after.loudnessRangeLu - before.loudnessRangeLu)} LU`,
    },
    {
      id: "peakToLoudness",
      label: "Peak to loudness",
      before: audible ? `${crest(before).toFixed(1)} LU` : "silent",
      after: audible ? `${crest(after).toFixed(1)} LU` : "silent",
      change: audible ? `${signed(crest(after) - crest(before))} LU` : "",
    },
  ]
}

/** One sentence on whether the master is what was asked for. */
export function reviewVerdict(review: MasteringReview): string {
  const target = formatLevel(review.targetLufs, "LUFS")
  const ceiling = formatLevel(review.ceilingDbtp, "dBTP")
  if (review.targetReached) {
    return `On target: ${target} under a ceiling of ${ceiling}.`
  }
  const short = review.targetLufs - review.after.integratedLufs
  const over = review.after.truePeakDbtp - review.ceilingDbtp
  if (over > 0.05) {
    return `The true peak is ${over.toFixed(1)} dB over the ceiling of ${ceiling}. Lower the target or the ceiling and export again.`
  }
  if (short > MASTERING_TOLERANCE_LU) {
    return `${short.toFixed(1)} LU under the target of ${target}: this mix cannot be that loud under a ceiling of ${ceiling} without being crushed. Lower the target or raise the ceiling.`
  }
  return `${Math.abs(short).toFixed(1)} LU over the target of ${target}.`
}

/** What the chain did, for the line under the table. */
export function reviewSummary(review: MasteringReview): string {
  const parts = [`${signed(review.gainDb)} dB of gain`]
  if (review.compressionDb > 0.05) {
    parts.push(`up to ${review.compressionDb.toFixed(1)} dB of band compression`)
  }
  parts.push(
    review.limitingDb > 0.05
      ? `up to ${review.limitingDb.toFixed(1)} dB of limiting`
      : "no limiting"
  )
  return `${parts.join(", ")}, in ${review.passes} passes.`
}
