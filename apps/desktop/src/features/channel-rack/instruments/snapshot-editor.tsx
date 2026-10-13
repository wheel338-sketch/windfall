import { useEffect, useRef, useState } from "react"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import type { InstrumentChannel } from "@/lib/channel-source"
import { useInstrumentAssetDispatch } from "./editor-owner"
import { onHistoryNavigation } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { prepareAsset } from "./asset-job"
import { useInstrumentBinding } from "./use-instrument-binding"

export function SnapshotEditor({ channel }: { channel: InstrumentChannel }) {
  const dispatch = useInstrumentAssetDispatch()
  const [harmonic,setHarmonic] = useState(1), [busy,setBusy] = useState(false), [error,setError] = useState<string | null>(null)
  const job = useRef<AbortController | null>(null)
  useEffect(() => { const cancel = () => { job.current?.abort(); setBusy(false) }; const off = onProjectReplaced(cancel), historyOff = onHistoryNavigation(cancel); return () => { off();historyOff();cancel() } }, [])
  const bind = useInstrumentBinding(channel.id,channel.source.params)
  const stored = channel.source.asset
  const frames = stored?.type === "spectra" ? stored.frames : undefined
  const levels = (snapshot: number) => Array.from({ length: 32 }, (_,i) => frames?.[snapshot]?.partials[i]?.level ?? (snapshot === 0 ? (i === 0 ? 1 : 0) : (i%2 === 0 ? 1/(i+1) : 0)))
  async function edit(snapshot: number,value: number) {
    if (!Number.isFinite(value) || value<0 || value>1 || busy) return
    job.current?.abort(); const request = new AbortController(); job.current=request
    const expected=channel.source, generation=getProjectGeneration(), a=levels(0), b=levels(1)
    if (snapshot === 0) a[harmonic-1]=value; else b[harmonic-1]=value
    setBusy(true);setError(null)
    try { const asset = await prepareAsset({ type: "snapshots",a,b },request.signal); if (!request.signal.aborted && generation===getProjectGeneration()) if (!await dispatch({ type: "setInstrumentAsset",channel: channel.id,expected,asset })) throw new Error("The instrument changed; prepare the snapshot again") }
    catch (error) { if (!request.signal.aborted) setError(error instanceof Error ? error.message : String(error)) }
    finally { if (!request.signal.aborted) setBusy(false) }
  }
  return <div role="group" aria-label="Snapshot Morph settings" className="flex flex-col gap-3">
    <ParamGroup title="Harmonic snapshots">
      <div className="grid grid-cols-2 gap-2">{[0,1].map(snapshot => <svg key={snapshot} viewBox="0 0 160 80" role="img" aria-label={`Snapshot ${snapshot===0 ? "A" : "B"} spectrum`} className="h-24 text-foreground">{levels(snapshot).map((level,index) => <rect key={index} x={index*5} y={80-level*80} width="4" height={level*80} fill="currentColor" />)}</svg>)}</div>
      <FieldGroup><Field><FieldLabel htmlFor={`harmonic-${channel.id}`}>Harmonic</FieldLabel><Input id={`harmonic-${channel.id}`} type="number" min={1} max={32} value={harmonic} onChange={event => { const n=Number(event.target.value); if (Number.isInteger(n) && n>=1 && n<=32) setHarmonic(n) }} /></Field>
      {[0,1].map(snapshot => <Field key={snapshot}><FieldLabel htmlFor={`snapshot-${snapshot}-${channel.id}`}>Snapshot {snapshot===0 ? "A" : "B"} harmonic level</FieldLabel><Input id={`snapshot-${snapshot}-${channel.id}`} type="number" min={0} max={1} step={0.01} value={levels(snapshot)[harmonic-1]} disabled={busy} onChange={event => void edit(snapshot,Number(event.target.value))} /></Field>)}</FieldGroup>
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    </ParamGroup>
    <ParamRow>{["morph","attackMs","releaseMs","level","polyphony"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
  </div>
}
