import type { ChannelId, InstrumentParams } from "@/bindings"
import { ParamControl,ParamGroup,ParamRow } from "@/features/params"
import { useInstrumentBinding } from "./use-instrument-binding"
type Params=Extract<InstrumentParams,{type:"harmonicFilter"}>
export function HarmonicFilterEditor({channel,params}:{channel:ChannelId;params:Params}) {
  const bind=useInstrumentBinding(channel,params)
  const curve=Array.from({length:64},(_,i) => { const hz=20*Math.pow(1000,i/63),ratio=hz/params.cutoffHz;const gain=(1+params.resonance*4*Math.max(0,1-Math.abs((ratio-1)*3)))/(1+Math.pow(ratio,4));return `${i*3},${80-Math.min(1,gain/5)*75}` }).join(" ")
  return <div role="group" aria-label="Harmonic Filter settings" className="flex flex-col gap-3">
    <ParamGroup title="Harmonic oscillator"><ParamRow><ParamControl {...bind("shape")} /></ParamRow></ParamGroup>
    <ParamGroup title="Spectral filter"><svg role="img" aria-label="Harmonic low-pass response" viewBox="0 0 192 80" className="h-24 text-foreground"><polyline points={curve} fill="none" stroke="currentColor" /></svg><ParamRow>{["cutoffHz","resonance"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
    <ParamGroup title="Voice envelope"><ParamRow>{["attackMs","releaseMs","level","polyphony"].map(id => <ParamControl key={id} {...bind(id)} />)}</ParamRow></ParamGroup>
  </div>
}
