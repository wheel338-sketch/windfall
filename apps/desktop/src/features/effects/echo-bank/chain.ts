/** The echo bank always has this many units. */
import { translatedText as t } from "@/lib/i18n"
export const UNIT_COUNT = 8

/** The id of one unit's setting, as the descriptor spells it. */
export function unitParam(unit: number, field: string): string {
  return `units.${unit}.${field}`
}

/** What the routing of the chain depends on, for one unit. */
export type UnitRouting = {
  enabled: boolean
  /** How much of the bank's input the unit takes in. */
  inputGain: number
  /** How much of the unit goes to the main output. Negative inverts it. */
  outputGain: number
  /** How much of the unit goes on into the next one. Negative inverts it. */
  nextSend: number
}

/**
 * Whether unit `from` really feeds the unit after it: both are switched on
 * and the send between them is not zero. A unit that is off hands nothing
 * on, so it breaks a chain. The last unit has nothing after it.
 */
export function linked(units: readonly UnitRouting[], from: number): boolean {
  const next = units[from + 1]
  return (
    next !== undefined &&
    units[from].enabled &&
    next.enabled &&
    units[from].nextSend !== 0
  )
}

/**
 * The runs of units that feed one another, in order, each as the numbers
 * the user sees (1 to 8). A unit that is off is in no run. Units that only
 * take the bank's input are runs of one.
 */
export function chainRuns(units: readonly UnitRouting[]): number[][] {
  const runs: number[][] = []
  let run: number[] = []
  units.forEach((unit, index) => {
    if (!unit.enabled) return
    run.push(index + 1)
    if (!linked(units, index)) {
      runs.push(run)
      run = []
    }
  })
  return runs
}

/**
 * The routing in words: "1 → 2 → 3, 4" is three units in series beside one
 * on its own. With no unit on there is nothing to hear.
 */
export function chainSummary(units: readonly UnitRouting[]): string {
  const runs = chainRuns(units)
  if (runs.length === 0) return t("No unit is on")
  return runs.map((run) => run.join(" → ")).join(", ")
}

/** One setting to change, by id. */
export type RoutingEdit = { id: string; value: number }

/**
 * The edits that put the units that are on in series: within each run of
 * neighbors that are on, only the first takes the bank's input and each
 * sends all of itself to the next. A unit that is off separates two runs,
 * because it hands nothing on. Only settings that differ are returned.
 */
export function serialEdits(units: readonly UnitRouting[]): RoutingEdit[] {
  const edits: RoutingEdit[] = []
  units.forEach((unit, index) => {
    const after = units[index + 1]
    const before = units[index - 1]
    if (after !== undefined) {
      const send = unit.enabled && after.enabled ? 1 : 0
      if (unit.nextSend !== send) {
        edits.push({ id: unitParam(index, "nextSend"), value: send })
      }
    }
    const input = before?.enabled && unit.enabled ? 0 : 1
    if (unit.inputGain !== input) {
      edits.push({ id: unitParam(index, "inputGain"), value: input })
    }
  })
  return edits
}

/**
 * The edits that put every unit side by side: each takes the bank's input
 * and none sends to the next.
 */
export function parallelEdits(units: readonly UnitRouting[]): RoutingEdit[] {
  const edits: RoutingEdit[] = []
  units.forEach((unit, index) => {
    if (index + 1 < units.length && unit.nextSend !== 0) {
      edits.push({ id: unitParam(index, "nextSend"), value: 0 })
    }
    if (unit.inputGain !== 1) {
      edits.push({ id: unitParam(index, "inputGain"), value: 1 })
    }
  })
  return edits
}
