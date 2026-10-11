import { useState } from "react"
import type { ChannelId, InstrumentParams } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { useAudition } from "../audition"
import { useInstrumentBinding } from "./use-instrument-binding"

type Params = Extract<InstrumentParams, { type: "drumRack" }>
export function DrumRackEditor({ channel, params }: { channel: ChannelId; params: Params }) {
  const [pad, setPad] = useState(0)
  const audition = useAudition(channel, 36 + pad)
  const bind = useInstrumentBinding(channel, params)
  return <div role="group" aria-label="Drum Rack settings" className="flex flex-col gap-3">
    <div role="group" aria-label="Sixteen synthesized pads" className="grid grid-cols-4 gap-2">
      {params.pads.map((_, index) => <Button key={index} variant={pad === index ? "secondary" : "outline"} aria-pressed={pad === index} onClick={() => { audition.stop(); setPad(index) }}>Pad {index + 1} · MIDI {36 + index}</Button>)}
    </div>
    <ParamGroup title={`Pad ${pad + 1} membrane`}>
      <Button onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); audition.start() }} onPointerUp={audition.stop} onPointerCancel={audition.stop} onLostPointerCapture={audition.stop} onKeyDown={event => { if (event.key === " " || event.key === "Enter") audition.start() }} onKeyUp={audition.stop} onBlur={audition.stop}>Hold to audition pad {pad + 1}</Button>
      <ParamRow>{["pitchHz", "decayMs", "tone", "noiseMix", "strikePosition", "damping", "body"].map(field => <ParamControl key={field} {...bind(`pads.${pad}.${field}`)} />)}</ParamRow>
    </ParamGroup>
    <ParamRow><ParamControl {...bind("level")} /></ParamRow>
  </div>
}
