import type { AutomationTarget } from "@/bindings"
import type { ContextItem } from "@/components/context-actions"
import { controllerEntries } from "@/features/controllers/menu"
import { deleteAutomation } from "@/features/playlist/automation/ops"
import {
  automationBlocked,
  automationsOf,
  targetState,
} from "@/lib/automation/targets"
import { useProjectStore } from "@/lib/store/project"

import { openEventEditor } from "@/features/event-editor/actions"

import { createAutomationClip, showAutomation } from "./create"
import { drivingAutomation } from "./now"

/**
 * What the menu of a control offers about automating what it is bound to,
 * as things are in the project now: make an automation clip, go to the
 * ones there are, and remove them.
 */
export function automationEntries(target: AutomationTarget): ContextItem[] {
  const { project } = useProjectStore.getState()
  if (!targetState(project, target)) return []
  const blocked = automationBlocked(project, target)
  const existing = automationsOf(project.automations, target)
  // Two automations of one target start out with the same name, so the
  // later ones are told apart by a number.
  const seen = new Map<string, number>()
  const names = existing.map((automation) => {
    const count = (seen.get(automation.name) ?? 0) + 1
    seen.set(automation.name, count)
    return count === 1 ? automation.name : `${automation.name} (${count})`
  })
  // While the song has the control in hand, an edit to it changes what it
  // is outside its clips. The way to the curve that is moving it comes
  // first.
  const driving = drivingAutomation(target)
  const items: ContextItem[] = [
    ...(driving
      ? [
          {
            title: "Edit the automation instead",
            afterClose: true,
            run: () => showAutomation(driving.automation.id),
          },
        ]
      : []),
    {
      title: "Create automation clip",
      disabled: blocked !== null,
      reason: blocked ?? undefined,
      // Both of these hand the keyboard to the timeline, which the closing
      // menu would take back for the control if they ran first.
      afterClose: true,
      run: () => createAutomationClip(target),
    },
    ...existing.map((automation, index): ContextItem => ({
      title: `Show automation: ${names[index]}`,
      afterClose: true,
      run: () => showAutomation(automation.id),
    })),
    // Automation the selected pattern stores for itself, with no clip.
    ...(target.type === "tempo"
      ? []
      : [
          {
            title: "Edit events in pattern",
            afterClose: true,
            run: () => openEventEditor(target),
          },
        ]),
  ]
  if (existing.length === 1) {
    items.push({
      title: "Remove automation",
      destructive: true,
      run: () => deleteAutomation(existing[0].id),
    })
  } else if (existing.length > 1) {
    items.push({
      submenu: "Remove automation",
      items: existing.map((automation, index): ContextItem => ({
        title: names[index],
        destructive: true,
        run: () => deleteAutomation(automation.id),
      })),
    })
  }
  return items
}

/**
 * The entries a control bound to `target` adds to its right-click menu.
 * They are worked out when the menu opens, so one list serves the control
 * for as long as it is bound to the same thing.
 *
 *     const items = useMemo(() => automationItems({ type: "trackPan", track }), [track])
 *     <ValueContextItems items={items}>…</ValueContextItems>
 */
export function automationItems(target: AutomationTarget): ContextItem[] {
  return [
    {
      dynamic: () => [
        ...automationEntries(target),
        ...controllerEntries(target),
      ],
    },
  ]
}
