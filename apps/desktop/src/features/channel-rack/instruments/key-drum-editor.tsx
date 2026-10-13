import { useContext, useRef, useState } from "react"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { GenericInstrumentEditor } from "../inspector/generic-instrument-editor"
import { ParamControl, ParamGroup, ParamRow, instrumentDescriptor, readParam, writeParam } from "@/features/params"
import type { DrumVoiceParams } from "@/bindings"
import type { InstrumentChannel } from "@/lib/channel-source"
import { reportError } from "@/lib/errors"
import { newGestureId, useProjectStore } from "@/lib/store"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useAudition } from "../audition"
import { InstrumentEditorOwner, useInstrumentAssetDispatch } from "./editor-owner"

export function KeyDrumEditor({ channel }: { channel: InstrumentChannel }) {
  const owner = useContext(InstrumentEditorOwner)
  const dispatch = useInstrumentAssetDispatch()
  const [key, setKey] = useState(36)
  const gesture = useRef<number | undefined>(undefined)
  const pending = useRef(Promise.resolve())
  const audition = useAudition(channel.id, key)
  const { params, asset } = channel.source
  if (params.type !== "drumVoice") return null
  const bank = asset?.type === "drumKeys" ? asset.patches : []
  const patch = bank.find(patch => patch.key === key)?.params ?? params
  const descriptor = instrumentDescriptor("drumVoice")
  function edit(index: number, value: number) {
    const generation = getProjectGeneration()
    const capturedKey = key
    const capturedGesture = gesture.current
    pending.current = pending.current.then(async () => {
      if (generation !== getProjectGeneration()) return
      const current = owner ? owner.currentChannel() : useProjectStore.getState().project.channels.find(item => item.id === channel.id)
      if (current?.source.type !== "instrument" || current.source.params.type !== "drumVoice") return
      const bank = current.source.asset?.type === "drumKeys" ? current.source.asset.patches : []
      const before = bank.find(patch => patch.key === capturedKey)?.params ?? current.source.params
      const edited = writeParam({ ...before, type: "drumVoice" }, descriptor.params[index], value)
      const params: DrumVoiceParams = { mode: edited.mode, pitchHz: edited.pitchHz, decayMs: edited.decayMs, tone: edited.tone, snap: edited.snap, level: edited.level }
      const patches = [...bank.filter(patch => patch.key !== capturedKey), { key: capturedKey, params }].sort((a,b) => a.key - b.key)
      await dispatch({ type: "setInstrumentAsset", channel: channel.id, expected: current.source, asset: { type: "drumKeys", patches } }, capturedGesture)
    }).catch((error: unknown) => reportError(error,"Could not edit the drum key"))
  }
  return <div role="group" aria-label="Per-key drum programs" className="flex flex-col gap-3">
    <FieldGroup><Field><FieldLabel htmlFor={`drum-key-${channel.id}`}>Program MIDI key</FieldLabel><Input id={`drum-key-${channel.id}`} type="number" min={0} max={127} value={key} onChange={event => { const value = Number(event.target.value); if (Number.isInteger(value) && value >= 0 && value <= 127) { audition.stop(); setKey(value) } }} /></Field></FieldGroup>
    <ParamGroup title={`MIDI ${key} patch`}>
      <Button onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); audition.start() }} onPointerUp={audition.stop} onPointerCancel={audition.stop} onLostPointerCapture={audition.stop} onBlur={audition.stop} onKeyDown={event => { if (event.key === " " || event.key === "Enter") audition.start() }} onKeyUp={audition.stop}>Hold to audition key {key}</Button>
      <ParamRow>{descriptor.params.map((info,index) => <ParamControl key={info.id} info={info} value={readParam(patch,info)} onGestureStart={() => { gesture.current = newGestureId() }} onGestureEnd={() => { gesture.current = undefined }} onValueChange={value => edit(index,value)} />)}</ParamRow>
    </ParamGroup>
    <ParamGroup title="Unprogrammed-key defaults and master"><GenericInstrumentEditor channel={channel.id} params={params} /></ParamGroup>
  </div>
}
