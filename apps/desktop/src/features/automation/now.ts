import type { Automation, AutomationTarget, PatternId } from "@/bindings"
import { rangeValue } from "@/lib/automation/curve"
import { targetKey } from "@/lib/automation/lanes"
import { targetState } from "@/lib/automation/targets"
import { useProjectStore } from "@/lib/store/project"
import { automatedValue } from "@/lib/store/realtime"
import { colorToCss } from "@/lib/units"

/*
 * What automation has in hand right now. The engine reports, 60 times a
 * second, the value each automation that is moving its target is giving
 * it. This is the lookup from a target to that.
 */

let indexedFor: readonly Automation[] | null = null
let index = new Map<string, Automation[]>()

/** The automations of each target, kept until the project's list changes. */
function automationsByTarget(): ReadonlyMap<string, Automation[]> {
  const automations = useProjectStore.getState().project.automations
  if (automations !== indexedFor) {
    indexedFor = automations
    index = new Map()
    for (const automation of automations) {
      const key = targetKey(automation.target)
      const list = index.get(key) ?? []
      list.push(automation)
      index.set(key, list)
    }
  }
  return index
}

/**
 * The automation that has a target in hand in the newest frame, with the
 * value from 0 to 1 it is giving it. Null while nothing has: the song is
 * stopped, the transport loops a pattern, or the song is before the
 * target's first clip.
 */
export function drivingAutomation(
  target: AutomationTarget
): { automation: Automation; normalized: number; pattern?: PatternId } | null {
  const entries = automationsByTarget().get(targetKey(target))
  for (const automation of entries ?? []) {
    const normalized = automatedValue(automation.id)
    if (normalized !== undefined) return { automation, normalized }
  }
  for (const pattern of useProjectStore.getState().project.patterns) {
    const lane = (pattern.events ?? []).find((lane) => targetKey(lane.target) === targetKey(target))
    if (!lane) continue
    const normalized = automatedValue(lane.id)
    if (normalized !== undefined) return { pattern: pattern.id, normalized,
      automation: { ...lane, name: pattern.name, color: pattern.color } }
  }
  return null
}

/**
 * The value automation is giving a target in the newest frame, in the
 * target's own unit, with the color of the automation it comes from. Null
 * while nothing has the target in hand.
 */
export function automatedNow(
  target: AutomationTarget
): { value: number; color: string } | null {
  const driving = drivingAutomation(target)
  if (!driving) return null
  const state = targetState(useProjectStore.getState().project, target)
  if (!state) return null
  return {
    value: rangeValue(state.range, driving.normalized),
    color: colorToCss(driving.automation.color),
  }
}
