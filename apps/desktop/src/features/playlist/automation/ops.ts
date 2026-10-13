import type { Automation, AutomationId, AutomationPoint } from "@/bindings"
import { refuse } from "@/lib/errors"
import { newGestureId } from "@/lib/store/gesture"
import { dispatch } from "@/lib/store/project"
import { askConfirm, askText } from "@/lib/store/prompts"

import { playlist, project } from "../selectors"
import { usePlaylistStore, type PointRef } from "../store"
import { formatAutomationValue, parseAutomationValue } from "./format"
import { showInView } from "./view-store"
import {
  deletePoint,
  samePoints,
  setPointValue,
  straighten,
  toggleHold,
} from "./points"
import { translatedText as t, message, plainNumber } from "@/lib/i18n"

/*
 * What the playlist does to automations and their curves. Each function is
 * one undo step.
 */

export function automationById(id: AutomationId): Automation | undefined {
  return project().automations.find((item) => item.id === id)
}

/**
 * Replaces the curve of an automation. Every clip that shows the
 * automation changes with it. A gesture id makes the edits of one drag one
 * undo step; without one, this call is a step of its own.
 */
export async function setCurve(
  id: AutomationId,
  points: readonly AutomationPoint[],
  gesture: number = newGestureId()
): Promise<boolean> {
  const automation = automationById(id)
  if (!automation || samePoints(automation.points, points)) return true
  const done = await dispatch(
    { type: "setAutomationPoints", id, points: [...points] },
    gesture
  )
  return done !== null
}

function pointOf(ref: PointRef) {
  const automation = automationById(ref.automation)
  const point = automation?.points[ref.index]
  return automation && point ? { automation, point } : null
}

/** Makes a point a step, or a slope again. */
export async function togglePointHold(ref: PointRef): Promise<void> {
  const found = pointOf(ref)
  if (!found) return
  await setCurve(ref.automation, toggleHold(found.automation.points, ref.index))
}

/** Takes the bend and the step out of the stretch that leaves a point. */
export async function straightenPoint(ref: PointRef): Promise<void> {
  const found = pointOf(ref)
  if (!found) return
  await setCurve(ref.automation, straighten(found.automation.points, ref.index))
}

/** Deletes a point. A curve keeps at least one, and says so. */
export async function deletePointAt(ref: PointRef): Promise<void> {
  const found = pointOf(ref)
  if (!found) return
  const points = deletePoint(found.automation.points, ref.index)
  if (!points) {
    refuse(
      t("A curve keeps at least one point"),
      t("Delete the clip, or the automation in the list on the left, instead.")
    )
    return
  }
  await setCurve(ref.automation, points)
}

/** Asks for a point's value in the unit of what the curve moves. */
export async function typePointValue(ref: PointRef): Promise<void> {
  const found = pointOf(ref)
  if (!found) return
  const { automation, point } = found
  const text = await askText({
    title: message("Value of the point in {automationName}", { automationName: automation.name }),
    label: t("Value"),
    initial: formatAutomationValue(project(), automation.target, point.value),
    submitLabel: t("Set"),
  })
  if (text === null) return
  const value = parseAutomationValue(project(), automation.target, text)
  if (value === null) {
    refuse(
      message("\"{text}\" is not a value for {automationName}", { text, automationName: automation.name }),
      t("Type it the way it is shown, such as −6 dB, 40% or 1.2 kHz.")
    )
    return
  }
  const current = pointOf(ref)
  if (!current) return
  // A value typed outside the chosen view takes the view's edge with it.
  showInView(ref.automation, [value])
  await setCurve(
    ref.automation,
    setPointValue(current.automation.points, ref.index, value)
  )
}

export async function renameAutomation(id: AutomationId): Promise<void> {
  const automation = automationById(id)
  if (!automation) return
  const name = await askText({
    title: t("Rename automation"),
    label: t("Name"),
    initial: automation.name,
    submitLabel: t("Rename"),
  })
  const trimmed = name?.trim()
  if (!trimmed || trimmed === automation.name) return
  await dispatch({ type: "updateAutomation", id, patch: { name: trimmed } })
}

export async function setAutomationColor(
  id: AutomationId,
  color: number
): Promise<void> {
  if (automationById(id)?.color === color) return
  await dispatch({ type: "updateAutomation", id, patch: { color } })
}

/** Copies an automation's curve, without its clips, and makes it the brush. */
export async function duplicateAutomation(id: AutomationId): Promise<void> {
  const result = await dispatch({ type: "duplicateAutomation", id })
  if (!result) return
  usePlaylistStore
    .getState()
    .setBrush({ type: "automation", automation: result.created[0] })
}

/** Deletes an automation and its clips, asking first when it has any. */
export async function deleteAutomation(id: AutomationId): Promise<void> {
  const automation = automationById(id)
  if (!automation) return
  const count = playlist().clips.filter(
    (clip) =>
      clip.content.type === "automation" && clip.content.automation === id
  ).length
  if (count > 0) {
    const choice = await askConfirm({
      title: message("Delete {automationName}?", { automationName: automation.name }),
      description: message("Its {shown} clips on the playlist will be deleted with it. Undo brings them back.", { count, shown: plainNumber(count) }),
      choices: [
        { id: "delete", label: t("Delete automation"), variant: "destructive" },
      ],
    })
    if (choice !== "delete") return
  }
  await dispatch({ type: "removeAutomation", id })
}
