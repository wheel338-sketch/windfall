import { useEffect, useRef, useState } from "react"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import type { InstrumentChannel } from "@/lib/channel-source"
import { dispatch } from "@/lib/store"
import { onHistoryNavigation } from "@/lib/store/project"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { useInstrumentBinding } from "./use-instrument-binding"
import { prepareAsset } from "./asset-job"

export function NumberEditor({ channel }: { channel: InstrumentChannel }) {
  const [busy,setBusy] = useState(false), [error,setError] = useState<string | null>(null)
  const request = useRef<AbortController | null>(null)
  useEffect(() => { const cancel = () => { request.current?.abort(); setBusy(false) }; const off = onProjectReplaced(cancel), historyOff = onHistoryNavigation(cancel); return () => { off(); historyOff(); cancel() } }, [])
  const bind = useInstrumentBinding(channel.id,channel.source.params)
  const asset = channel.source.asset
  const input = useRef<HTMLInputElement>(null)
  const number = asset?.type === "spectra" ? asset.presetNumber ?? 12345 : 12345
  async function load() {
    request.current?.abort()
    const job = new AbortController(); request.current = job
    const generation = getProjectGeneration(), expected = channel.source
    setBusy(true); setError(null)
    try {
      const asset = await prepareAsset({ type: "number",number: Number(input.current?.value) },job.signal)
      if (generation !== getProjectGeneration() || job.signal.aborted) return
      if (!await dispatch({ type: "setInstrumentAsset",channel: channel.id,expected,asset })) throw new Error("The instrument changed; prepare the source again")
    } catch (error) { if (!job.signal.aborted) setError(error instanceof Error ? error.message : String(error)) }
    finally { if (request.current === job && !job.signal.aborted) setBusy(false) }
  }
  return <div role="group" aria-label="Number Voice settings" className="flex flex-col gap-3">
    <FieldGroup><Field><FieldLabel htmlFor={`patch-number-${channel.id}`}>Patch number</FieldLabel><Input key={number} ref={input} id={`patch-number-${channel.id}`} type="number" min={0} max={4294967295} step={1} defaultValue={number} disabled={busy} /><FieldDescription>Loaded patch {number}. Each whole number selects a reproducible sound.</FieldDescription></Field></FieldGroup>
    <Button disabled={busy} onClick={()=>void load()}>Load numbered patch</Button>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <ParamGroup title="Output"><ParamRow><ParamControl {...bind("level")} /></ParamRow></ParamGroup>
  </div>
}
