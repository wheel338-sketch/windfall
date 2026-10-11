import type { DragEvent } from "react"
export const LOOP_DRAG_TYPE = "application/x-windfall-factory-loop"
export type LoopDrag = { id: string; name: string; bars: number; key: number | null; generation: number; revision: number }
let active: LoopDrag | null = null
export function setLoopDrag(event: DragEvent, loop: LoopDrag) {
  event.dataTransfer.setData(LOOP_DRAG_TYPE, JSON.stringify(loop))
  event.dataTransfer.effectAllowed = "copy"
  active = loop
  const clear = () => { active = null; window.removeEventListener("dragend", clear, true); window.removeEventListener("drop", afterDrop, true) }
  const afterDrop = () => queueMicrotask(clear)
  window.addEventListener("dragend", clear, true); window.addEventListener("drop", afterDrop, true)
}
export function activeLoopDrag() { return active }
export function hasLoopDrag(event: DragEvent) { return event.dataTransfer.types.includes(LOOP_DRAG_TYPE) }
export function readLoopDrag(event: DragEvent): LoopDrag | null {
  try {
    const value: unknown = JSON.parse(event.dataTransfer.getData(LOOP_DRAG_TYPE))
    if (!value || typeof value !== "object" || !("id" in value) || typeof value.id !== "string" || value.id.length > 64 || !("name" in value) || typeof value.name !== "string" || !("bars" in value) || typeof value.bars !== "number" || !Number.isInteger(value.bars) || value.bars < 1 || value.bars > 8 || !("generation" in value) || typeof value.generation !== "number" || !Number.isSafeInteger(value.generation) || value.generation < 0 || !("revision" in value) || typeof value.revision !== "number" || !Number.isSafeInteger(value.revision) || value.revision < 0 || !("key" in value) || !(value.key === null || (typeof value.key === "number" && Number.isInteger(value.key) && value.key >= 0 && value.key < 12))) return null
    return { id: value.id, name: value.name, bars: value.bars, generation: value.generation, revision: value.revision, key: value.key }
  } catch { return null }
}
