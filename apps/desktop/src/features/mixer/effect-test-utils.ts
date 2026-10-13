import { createEvent, fireEvent, within } from "@testing-library/react"

import type { EffectId, EffectKind, EffectSlot } from "@/bindings"
import { effectDescriptor } from "@/features/params"
import { dispatch } from "@/lib/store/project"

import { EFFECT_DRAG_TYPE } from "./effect-drag"
import { flush, strip, trackNamed } from "./test-utils"
import { message, plainNumber } from "@/lib/i18n"

/** Helpers the tests of the mixer's effects share. Not part of the app. */

export const chain = (track: string): EffectSlot[] => trackNamed(track).effects
export const kinds = (track: string): EffectKind[] =>
  chain(track).map((slot) => slot.params.type)
export const ids = (track: string): EffectId[] =>
  chain(track).map((slot) => slot.id)

/** Puts effects on a track, one undo step each. Returns their ids. */
export async function addEffects(
  track: string,
  ...added: EffectKind[]
): Promise<EffectId[]> {
  const created: EffectId[] = []
  for (const kind of added) {
    const result = await dispatch({
      type: "addEffect",
      track: trackNamed(track).id,
      kind,
    })
    if (!result) throw new Error(message("Could not add a {kind} to \"{track}\"", { kind, track }))
    created.push(result.created[0])
  }
  await flush()
  return created
}

export const nameOfKind = (kind: EffectKind) => effectDescriptor(kind).name

/** The row of an effect on a strip. */
export function slotRow(track: string, effect: EffectId): HTMLElement {
  const row = strip(track).querySelector<HTMLElement>(
    `[data-slot=effect-slot][data-effect-row="${effect}"]`
  )
  if (!row) throw new Error(message("\"{track}\" shows no slot for effect {effect}", { track, effect: plainNumber(effect) }))
  return row
}

/** The button of a slot that opens the effect, which is what has the focus. */
export function slotButton(track: string, effect: EffectId): HTMLElement {
  const button = slotRow(track, effect).querySelector<HTMLElement>(
    "[data-slot=effect-open]"
  )
  if (!button) throw new Error(message("The slot of effect {effect} has no button", { effect: plainNumber(effect) }))
  return button
}

export function slotLamp(track: string, effect: EffectId): HTMLElement {
  return within(slotRow(track, effect)).getByRole("button", { name: / on$/ })
}

/** The names a strip's rack shows, top to bottom. */
export function rackNames(track: string): string[] {
  return [...strip(track).querySelectorAll("[data-slot=effect-open]")].map(
    (item) => item.textContent ?? ""
  )
}

/** What a drag carries, which jsdom does not provide. */
export function dragData() {
  const data = new Map<string, string>()
  return {
    get types() {
      return [...data.keys()]
    },
    setData: (type: string, value: string) => void data.set(type, value),
    getData: (type: string) => data.get(type) ?? "",
    setDragImage() {},
    effectAllowed: "none",
    dropEffect: "none",
  }
}

/** Height the tests give every row, so a pointer can be put in a gap. */
export const ROW = 20

/** Lays the effect rows of a drop zone out one under another from 0. */
export function layOutRows(zone: HTMLElement) {
  zone
    .querySelectorAll<HTMLElement>("[data-effect-row]")
    .forEach((row, index) => {
      row.getBoundingClientRect = () =>
        new DOMRect(0, index * ROW, 80, ROW) as DOMRect
    })
}

type DragKeys = { ctrlKey?: boolean; altKey?: boolean }

/**
 * Fires a drag event with the pointer at `clientY`. jsdom has no
 * `DragEvent`, so the place and the keys are put on a plain event.
 */
export function fireDrag(
  type: "dragOver" | "drop" | "dragLeave",
  zone: HTMLElement,
  dataTransfer: ReturnType<typeof dragData>,
  clientY: number,
  keys: DragKeys = {}
): boolean {
  const event = createEvent[type](zone, { dataTransfer })
  Object.defineProperties(event, {
    clientY: { value: clientY },
    ctrlKey: { value: keys.ctrlKey ?? false },
    altKey: { value: keys.altKey ?? false },
  })
  return fireEvent(zone, event)
}

/**
 * Drags an effect from where it is into gap `gap` of a drop zone: a strip,
 * or the chain in the inspector. Gap 0 is above the first row.
 */
export async function dragEffect(
  from: HTMLElement,
  zone: HTMLElement,
  gap: number,
  keys: DragKeys = {}
) {
  const dataTransfer = dragData()
  layOutRows(zone)
  // Just below the top of the row after the gap, or below the last row.
  const clientY = gap * ROW + 1
  fireEvent.dragStart(from, { dataTransfer })
  const over = fireDrag("dragOver", zone, dataTransfer, clientY, keys)
  fireDrag("drop", zone, dataTransfer, clientY, keys)
  fireEvent.dragEnd(from, { dataTransfer })
  await flush()
  return {
    /** True when the zone took the drag, which is what allows a drop. */
    accepted: !over,
    dropEffect: dataTransfer.dropEffect,
    carried: dataTransfer.getData(EFFECT_DRAG_TYPE),
  }
}
