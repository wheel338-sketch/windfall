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

export function ResynthesisEditor({ channel, colorScan = false }: { channel: InstrumentChannel; colorScan?: boolean }) {
  const [busy,setBusy] = useState(false), [error,setError] = useState<string | null>(null)
  const request = useRef<AbortController | null>(null)
  useEffect(() => { const cancel = () => request.current?.abort(); const off = onProjectReplaced(cancel); return () => { off(); cancel() } }, [])
  const bind = useInstrumentBinding(channel.id,channel.source.params)
  const asset = channel.source.asset
  const root = useRef<HTMLInputElement>(null)
  async function load(file: File) {
    request.current?.abort()
    const job = new AbortController(); request.current = job
    const generation = getProjectGeneration(), expected = channel.source
    setBusy(true); setError(null)
    try {
      if (file.size > 16*1024*1024) throw new Error("Choose a source of 16 MiB or less")
      let asset
      if (file.name.toLowerCase().endsWith(".wav")) {
        if (colorScan) throw new Error("Color Scan needs a PNG, JPEG or WebP image")
        asset=await prepareAsset({type:"audioSpectrum",bytes:await file.arrayBuffer(),rootHz:Number(root.current?.value??440)},job.signal)
      } else {
        const image=await createImageBitmap(file)
        try {
          const canvas=document.createElement("canvas");canvas.width=Math.min(256,image.width);canvas.height=Math.min(64,image.height)
          const context=canvas.getContext("2d");if(!context) throw new Error("Image decoding is unavailable")
          context.drawImage(image,0,0,canvas.width,canvas.height)
          const pixels=context.getImageData(0,0,canvas.width,canvas.height).data
          asset=await prepareAsset({type:"imageSpectrum",pixels:pixels.buffer as ArrayBuffer,width:canvas.width,height:canvas.height,color:colorScan},job.signal)
        } finally { image.close() }
      }
      if (generation !== getProjectGeneration() || job.signal.aborted) return
      await dispatch({ type: "setInstrumentAsset",channel: channel.id,expected,asset })
    } catch (error) { if (!job.signal.aborted) setError(error instanceof Error ? error.message : String(error)) }
    finally { if (request.current === job && !job.signal.aborted) setBusy(false) }
  }
  return <div role="group" aria-label={colorScan ? "Color Scan settings" : "Spectral Resynth settings"} className="flex flex-col gap-3">
    <ParamGroup title="Prepared source">
      <FieldGroup><Field><FieldLabel htmlFor={`resynth-source-${channel.id}`}>{colorScan ? "Load scan image" : "Load WAV or image"}</FieldLabel><Input id={`resynth-source-${channel.id}`} type="file" accept={colorScan ? ".png,.jpg,.jpeg,.webp" : ".wav,.png,.jpg,.jpeg,.webp"} disabled={busy} onChange={event=>{const file=event.target.files?.[0];event.target.value="";if(file)void load(file)}} /><FieldDescription>{colorScan ? "Image columns scan left to right; bottom rows are low notes, each row adds a semitone. Hue adds up to two semitones; squared luminance and alpha set amplitude, red/blue set stereo position. Up to 256 columns and 64 rows." : "WAV analysis keeps up to 32768 mono frames and 64 harmonics at the source root pitch. Images map bottom-to-top brightness to harmonics and columns to time; up to 256 columns and 64 rows."}</FieldDescription></Field>
      {!colorScan && <Field><FieldLabel htmlFor={`resynth-root-${channel.id}`}>WAV source root Hz</FieldLabel><Input ref={root} id={`resynth-root-${channel.id}`} type="number" min={8} max={20000} step="any" defaultValue={440} disabled={busy} /></Field>}</FieldGroup>
      {error&&<Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      {asset?.type==="spectra"&&<><FieldDescription>{asset.frames.length} prepared frames, {asset.frames[0]?.partials.length} partials, root {asset.rootHz} Hz</FieldDescription><svg role="img" aria-label="Prepared spectral frames" viewBox={`0 0 ${asset.frames.length} ${asset.frames[0]?.partials.length??1}`} className="h-24 w-full" preserveAspectRatio="none">{asset.frames.flatMap((frame,x)=>frame.partials.map((partial,row)=><rect key={`${x}:${row}`} x={x} y={frame.partials.length-1-row} width={1} height={1} fill="currentColor" opacity={partial.level} />))}</svg></>}
      <Button variant="outline" disabled={!asset||busy} onClick={()=>void dispatch({type:"setInstrumentAsset",channel:channel.id,expected:channel.source,asset:null})}>Reset prepared source</Button>
    </ParamGroup>
    <ParamGroup title="Frame playback"><ParamRow>{(colorScan ? ["position","scan","scanRate","stereoWidth"] : ["position","scan","scanRate"]).map(id=><ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    <ParamGroup title="Voice"><ParamRow>{["attackMs","releaseMs","level","polyphony"].map(id=><ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
  </div>
}
