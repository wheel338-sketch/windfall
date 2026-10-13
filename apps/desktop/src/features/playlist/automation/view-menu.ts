import type { Automation, AutomationId } from "@/bindings"
import type { ContextItem } from "@/components/context-actions"
import {
  FULL_VIEW,
  fittedView,
  isFullView,
  makeView,
  viewOfAutomation,
  type ViewRange,
} from "@/lib/automation/view-range"
import { refuse } from "@/lib/errors"
import { askText } from "@/lib/store/prompts"

import { project, selectedClips } from "../selectors"
import { formatAutomationValue, parseAutomationValue } from "./format"
import { chosenView, useCurveViews } from "./view-store"
import { message, translatedText as t, dynamicTitle } from "@/lib/i18n"

/*
 * Choosing how much of a curve's range its clips show: the entries of the
 * clip's and the automation's menus, and what they do. None of it touches
 * the project. A view is a way of looking at a curve.
 */

function automationById(id: AutomationId): Automation | undefined {
  return project().automations.find((item) => item.id === id)
}

/** The view an automation's clips are drawn in now. */
export function currentView(automation: Automation): ViewRange {
  return viewOfAutomation(
    automation,
    project().settings.tempoBpm,
    chosenView(automation.id)
  )
}

const setView = (id: AutomationId, view: ViewRange | null) =>
  useCurveViews.getState().setView(id, view)

/** Shows exactly the curve, with a little air above and below it. */
export function fitViewToCurve(id: AutomationId): void {
  const automation = automationById(id)
  if (automation) setView(id, fittedView(automation))
}

/** Shows the whole range of what the curve moves. */
export function showFullRange(id: AutomationId): void {
  const automation = automationById(id)
  if (!automation) return
  // The whole range is the default of everything but the tempo.
  setView(id, automation.target.type === "tempo" ? FULL_VIEW : null)
}

/** Back to the view that follows the tempo and the curve's points. */
export function followTempo(id: AutomationId): void {
  setView(id, null)
}

/**
 * Reads the two ends of a range typed in the unit of what the curve
 * moves: "100 to 140", "-12 dB to 0 dB", "L50 .. R50", "20%; 80%".
 */
export function parseViewRange(
  automation: Automation,
  text: string
): ViewRange | null {
  let parts = text
    .split(/\s+to\s+|\s*\.{2,}\s*|\s*…\s*|\s*;\s*/i)
    .map((part) => part.trim())
    .filter((part) => part !== "")
  if (parts.length === 1) {
    // "100-140" and "100 – 140": a dash between two things, not a sign.
    const dashed = /^(.+?\S)\s*[-–—]\s*(\S.*)$/.exec(parts[0])
    if (dashed) parts = [dashed[1], dashed[2]]
  }
  if (parts.length !== 2) return null
  const current = project()
  const ends = parts.map((part) =>
    parseAutomationValue(current, automation.target, part)
  )
  const [a, b] = ends
  if (a === null || b === null || a === undefined || b === undefined) {
    return null
  }
  return a === b ? null : makeView(a, b)
}

/** Asks for the lowest and the highest value an automation's clips show. */
export async function typeViewRange(id: AutomationId): Promise<void> {
  const automation = automationById(id)
  if (!automation) return
  const view = currentView(automation)
  const current = project()
  const format = (value: number) =>
    formatAutomationValue(current, automation.target, value)
  const text = await askText({
    title: message("Range shown for {automationName}", { automationName: automation.name }),
    description:
      t("The lowest and the highest value its clips show. This changes how the curve is drawn, not what it does."),
    label: t("From, to"),
    initial: `${format(view.lo)} to ${format(view.hi)}`,
    submitLabel: t("Set range"),
  })
  if (text === null) return
  const found = automationById(id)
  if (!found) return
  const range = parseViewRange(found, text)
  if (!range) {
    refuse(
      message("\"{text}\" is not a range for {automationName}", { text, automationName: automation.name }),
      message("Type the lowest and the highest value to show, such as {format} to {format2}.", { format: format(view.lo), format2: format(view.hi) })
    )
    return
  }
  setView(id, range)
}

/**
 * The entries about the view of these automations, for a menu: fit, the
 * whole range, a typed range and, for the tempo, the view that follows it.
 * Empty when there is no automation to show.
 */
export function viewRangeItems(ids: readonly AutomationId[]): ContextItem[] {
  const automations = ids
    .map(automationById)
    .filter((item): item is Automation => item !== undefined)
  if (automations.length === 0) return []
  const tempo = automations.filter((item) => item.target.type === "tempo")
  const all = (run: (id: AutomationId) => void) => () => {
    for (const automation of automations) run(automation.id)
  }
  const items: ContextItem[] = [
    {
      // "Fit" for the tempo, whose default is a fit of a kind already.
      ...dynamicTitle(tempo.length === automations.length ? "Fit to curve" : "Zoom to curve", {}),
      run: all(fitViewToCurve),
    },
    {
      ...dynamicTitle("Full range", {}),
      checked: automations.every((item) => isFullView(currentView(item))),
      run: all(showFullRange),
    },
    {
      ...dynamicTitle("Set range…", {}),
      disabled: automations.length !== 1,
      reason: t("One automation at a time"),
      run: () => typeViewRange(automations[0].id),
    },
  ]
  if (tempo.length > 0) {
    items.push({
      ...dynamicTitle("Around the tempo", {}),
      checked: tempo.every((item) => chosenView(item.id) === undefined),
      run: () => {
        for (const automation of tempo) followTempo(automation.id)
      },
    })
  }
  return [{ submenu: t("Range shown"), items }]
}

/** The same for the automation clips that are selected on the playlist. */
export function selectedViewRangeItems(): ContextItem[] {
  const ids = new Set<AutomationId>()
  for (const clip of selectedClips()) {
    if (clip.content.type === "automation") ids.add(clip.content.automation)
  }
  return viewRangeItems([...ids])
}
