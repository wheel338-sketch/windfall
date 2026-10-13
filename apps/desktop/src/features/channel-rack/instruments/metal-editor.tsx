import type {ChannelId,InstrumentParams} from "@/bindings"
import {ParamControl,ParamGroup,ParamRow} from "@/features/params"
import {useInstrumentBinding} from "./use-instrument-binding"
import { translatedText as t } from "@/lib/i18n"
type Params=Extract<InstrumentParams,{type:"metalVoice"}>
export function MetalEditor({channel,params}:{channel:ChannelId;params:Params}) {
 const bind=useInstrumentBinding(channel,params)
 return <div role="group" aria-label={t("Metal Voice settings")} className="flex flex-col gap-3">
  <ParamGroup title="Metallic partials"><svg viewBox="0 0 160 80" role="img" aria-label={t("Inharmonic metallic partial spacing")} className="h-24 text-foreground">{Array.from({length:8},(_,i)=>{const n=i+1,ratio=(i===0?1:n*n*0.69)*(1+params.stretch*n*n*0.001);return <line key={i} x1={ratio*3} x2={ratio*3} y1="80" y2={80-70/Math.pow(n,1.4)} stroke="currentColor"/>})}</svg><ParamRow><ParamControl {...bind("stretch")}/><ParamControl {...bind("decaySeconds")}/></ParamRow></ParamGroup>
  <ParamGroup title="Shimmer"><ParamRow><ParamControl {...bind("shimmerHz")}/><ParamControl {...bind("shimmerDepth")}/></ParamRow></ParamGroup>
  <ParamRow>{["releaseMs","level","polyphony"].map(id=><ParamControl key={id} {...bind(id)}/>)}</ParamRow>
 </div>
}
