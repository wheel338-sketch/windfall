import type { Automation } from "@/bindings"
import { openAutomationLfo } from "@/features/automation/lfo-dialog"
import {
  contextSeparator,
  type ContextItem,
} from "@/components/context-actions"
import { CHANNEL_COLORS } from "@/features/channel-rack/color-swatches"

import { selectedClips } from "../selectors"
import type { PointRef } from "../store"
import { viewRangeItems } from "./view-menu"
import {
  automationById,
  deleteAutomation,
  deletePointAt,
  duplicateAutomation,
  renameAutomation,
  setAutomationColor,
  straightenPoint,
  togglePointHold,
  typePointValue,
} from "./ops"
import { message, dynamicTitle, translatedText as t } from "@/lib/i18n"

/** What a right-click on a point of a curve offers. */
export function pointMenu(ref: PointRef): ContextItem[] {
  const automation = automationById(ref.automation)
  const point = automation?.points[ref.index]
  if (!automation || !point) return []
  const last = automation.points.length <= 1
  const leaves = ref.index < automation.points.length - 1
  return [
    { label: message("Point of {automationName}", { automationName: automation.name }) },
    { ...dynamicTitle("Type value…", {}), run: () => typePointValue(ref) },
    {
      ...dynamicTitle("Hold", {}),
      checked: point.hold,
      // The last point has no stretch after it to make a step of.
      disabled: !leaves,
      run: () => togglePointHold(ref),
    },
    {
      ...dynamicTitle("Straighten", {}),
      disabled: !leaves || (point.curve === 0 && !point.hold),
      run: () => straightenPoint(ref),
    },
    contextSeparator,
    {
      ...dynamicTitle("Delete", {}),
      destructive: true,
      disabled: last,
      reason: t("The only point"),
      run: () => deletePointAt(ref),
    },
  ]
}

/** What a right-click on an automation in the list offers. */
export function automationMenu(automation: Automation): ContextItem[] {
  const id = automation.id
  return [
    { ...dynamicTitle("Rename…", {}), run: () => renameAutomation(id) },
    {
      submenu: t("Color"),
      items: CHANNEL_COLORS.map(({ color, name }) => ({
        title: name,
        checked: automation.color === color,
        run: () => setAutomationColor(id, color),
      })),
    },
    { ...dynamicTitle("Duplicate", {}), run: () => duplicateAutomation(id) },
    { ...dynamicTitle("Write LFO…", {}), run: () => openAutomationLfo(id) },
    ...viewRangeItems([id]),
    contextSeparator,
    {
      ...dynamicTitle("Delete…", {}),
      destructive: true,
      run: () => deleteAutomation(id),
    },
  ]
}

/**
 * The LFO writer for the one automation clip that is selected, opened on
 * the part of the curve that clip shows. Empty for any other selection.
 */
export function selectedClipLfoItems(): ContextItem[] {
  const clips = selectedClips()
  if (clips.length !== 1) return []
  const [clip] = clips
  if (clip.content.type !== "automation") return []
  const automation = clip.content.automation
  return [
    {
      ...dynamicTitle("Write LFO…", {}),
      run: () =>
        openAutomationLfo(automation, {
          start: clip.offset,
          end: clip.offset + clip.length,
        }),
    },
  ]
}
