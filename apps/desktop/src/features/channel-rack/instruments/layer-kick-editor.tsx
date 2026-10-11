import { useEffect, useRef, useState } from "react"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import type { InstrumentChannel } from "@/lib/channel-source"
import { dispatch } from "@/lib/store"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { useInstrumentBinding } from "./use-instrument-binding"
import { prepareAsset } from "./asset-job"

export function LayerKickEditor({ channel }: { channel: InstrumentChannel }) {
  const [busy,setBusy] = useState(false), [error,setError] = useState<string | null>(null)
  const request = useRef<AbortController | null>(null)
  useEffect(() => { const cancel = () => request.current?.abort(); const off = onProjectReplaced(cancel); return () => { off(); cancel() } }, [])
  const bind = useInstrumentBinding(channel.id,channel.source.params)
  const asset = channel.source.asset
  async function load(file: File) {
    request.current?.abort()
    const job = new AbortController(); request.current = job
    const generation = getProjectGeneration(), expected = channel.source
    setBusy(true); setError(null)
    try {
      if (file.size > 16*1024*1024) throw new Error("Choose a WAV of 16 MiB or less")
      const bytes = await file.arrayBuffer()
      const asset = await prepareAsset({ type: "layer",bytes },job.signal)
      if (generation !== getProjectGeneration() || job.signal.aborted) return
      await dispatch({ type: "setInstrumentAsset",channel: channel.id,expected,asset })
    } catch (error) { if (!job.signal.aborted) setError(error instanceof Error ? error.message : String(error)) }
    finally { if (request.current === job && !job.signal.aborted) setBusy(false) }
  }
  return <div role="group" aria-label="Layer Kick settings" className="flex flex-col gap-3">
    <ParamGroup title="Swept body"><ParamRow>{["pitchHz","decayMs","pitchDropSemitones","pitchDecayMs","click","level"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    <ParamGroup title="Optional sample layer">
      <FieldGroup><Field><FieldLabel htmlFor={`kick-layer-${channel.id}`}>Load kick layer WAV</FieldLabel><Input id={`kick-layer-${channel.id}`} type="file" accept=".wav" disabled={busy} onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void load(file) }} /><FieldDescription>{asset?.type === "pcm" ? `${asset.samples.length} mono frames at ${asset.sampleRate} Hz` : "Synthesized kick only. WAV layers keep up to 32768 mono frames; longer files fade at the limit."}</FieldDescription></Field></FieldGroup>
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      <Button variant="outline" disabled={!asset || busy} onClick={() => void dispatch({ type: "setInstrumentAsset",channel: channel.id,expected: channel.source,asset: null })}>Remove sample layer</Button>
      <ParamRow>{["sampleMix","sampleTune","sampleStart"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow>
    </ParamGroup>
  </div>
}
