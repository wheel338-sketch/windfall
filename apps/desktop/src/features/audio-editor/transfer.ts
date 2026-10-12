import type { DragEvent } from "react"
import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { usePlaylistStore } from "@/features/playlist/store"
export const SELECTION_DRAG = "application/x-windfall-editor-selection"
export type AudioEditDestination = {type:"sampler";channel:number} | {type:"playlist";track:number|null;start:number}
export type AudioEditTransferRequest = {token:number;startFrame:number;endFrame:number;destination:AudioEditDestination}
type DragSelection = Omit<AudioEditTransferRequest,"destination"> & {generation:number;durationSecs:number}
type Receiver = {begin():void;done(clip:number|null):void;error(reason:unknown):void}
const receivers = new Map<number,Receiver>()
let active: DragSelection | null = null
export function registerSelectionReceiver(token: number, receiver: Receiver) {
  receivers.set(token,receiver)
  return () => { if (receivers.get(token) === receiver) receivers.delete(token) }
}
export function startSelectionDrag(event: DragEvent, value: Omit<DragSelection,"generation">) {
  active={...value,generation:getProjectGeneration()}
  event.dataTransfer.setData(SELECTION_DRAG,JSON.stringify(active))
  event.dataTransfer.effectAllowed="copy"
}
export function endSelectionDrag() { active=null }
export function activeSelectionDrag() { return active }
export function hasSelectionDrag(event: DragEvent) { return event.dataTransfer.types.includes(SELECTION_DRAG) }
export function readSelectionDrag(event: DragEvent): DragSelection | null {
  try {
    const value: DragSelection=JSON.parse(event.dataTransfer.getData(SELECTION_DRAG))
    if (!value || ![value.token,value.generation,value.startFrame,value.endFrame].every(v=>Number.isSafeInteger(v) && v>=0)
      || value.endFrame<=value.startFrame || value.generation!==getProjectGeneration() || !Number.isFinite(value.durationSecs) || value.durationSecs<=0 || value.durationSecs>120) return null
    return value
  } catch { return null }
}
export async function transferSelection(value: DragSelection, destination: AudioEditDestination) {
  const receiver=receivers.get(value.token)
  receiver?.begin()
  try {
    if (value.generation!==getProjectGeneration()) throw new Error("The selection belongs to an earlier project.")
    const result=await backend.audioEditorTransfer({token:value.token,startFrame:value.startFrame,endFrame:value.endFrame,destination})
    if (value.generation!==getProjectGeneration()) return
    receivePatch(result.patch)
    const clip=destination.type === "playlist" ? result.created.at(-1) ?? null : null
    if (clip!==null) usePlaylistStore.getState().select([clip])
    receiver?.done(clip)
  } catch(reason) { if (value.generation===getProjectGeneration()) { receiver?.error(reason); if (!receiver) reportError(reason) } }
}
