import type { AutomationTarget, Project, TimeSignature } from "@/bindings"
import {
  formatGain,
  formatPan,
  formatPercent,
  parseGain,
  parsePan,
  parsePercent,
} from "@/components/audio"
import { formatParamDisplay, parseParamDisplay } from "@/features/params"
import { rangeNormalized, rangeValue } from "@/lib/automation/curve"
import { targetState } from "@/lib/automation/targets"
import { formatTempo, parseTempo, tickToPosition } from "@/lib/time"

type Source = Pick<Project, "channels" | "mixer" | "settings">

/**
 * An automation value in the unit of what it moves: "−6.0 dB" for a
 * volume, "L30" for a pan, "1.20 kHz" or "250 ms" or "40%" for a setting,
 * "128.00 bpm" for the tempo. Empty when the target is gone.
 */
export function formatAutomationValue(
  project: Source,
  target: AutomationTarget,
  normalized: number
): string {
  const state = targetState(project, target)
  if (!state) return ""
  const real = rangeValue(state.range, normalized)
  if (state.info) return formatParamDisplay(state.info, real)
  switch (target.type) {
    case "channelVolume":
    case "trackVolume":
    case "sendGain":
    case "sidechainGain":
      return formatGain(real)
    case "channelPan":
    case "trackPan":
      return formatPan(real)
    case "effectMix":
      return formatPercent(real)
    case "tempo":
      return `${formatTempo(real)} bpm`
    default:
      return String(real)
  }
}

/**
 * Reads text typed in the target's own unit as an automation value, 0 to
 * 1: "-6" for a volume is −6 dB, "120" for the tempo is 120 bpm. Null when
 * the text cannot be read or the target is gone.
 */
export function parseAutomationValue(
  project: Source,
  target: AutomationTarget,
  text: string
): number | null {
  const state = targetState(project, target)
  if (!state) return null
  let real: number | null
  if (state.info) {
    real = parseParamDisplay(state.info, text)
  } else {
    switch (target.type) {
      case "channelVolume":
      case "trackVolume":
      case "sendGain":
    case "sidechainGain":
        real = parseGain(text)
        break
      case "channelPan":
      case "trackPan":
        real = parsePan(text)
        break
      case "effectMix":
        real = parsePercent(text)
        break
      case "tempo":
        real = parseTempo(text.replace(/\s*bpm$/i, ""))
        break
      default:
        real = null
    }
  }
  if (real === null || Number.isNaN(real)) return null
  return rangeNormalized(state.range, real)
}

/** A tick as bar.beat.step, counted from 1: "3.2.1". */
export function formatBarBeat(tick: number, signature: TimeSignature): string {
  const { bar, beat, step } = tickToPosition(tick, signature)
  return `${bar}.${beat}.${step}`
}
