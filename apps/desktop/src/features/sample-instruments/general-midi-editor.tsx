import { useRef, useState } from "react"
import type { SoundFontPreset } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import type { InstrumentChannel } from "@/lib/channel-source"
import { backend, errorMessage } from "@/lib/ipc"
import { receivePatch } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"

export function GeneralMidiEditor({ channel }: { channel: InstrumentChannel }) {
  const [path,setPath]=useState("")
  const [presets,setPresets]=useState<SoundFontPreset[]>([])
  const [busy,setBusy]=useState(false)
  const [error,setError]=useState<string|null>(null)
  const request=useRef(0)
  return <FieldGroup>
    <Field><FieldLabel>General MIDI SoundFont bank</FieldLabel><Input aria-label="General MIDI SoundFont bank" value={path} onChange={event=>{setPath(event.target.value);setPresets([]);request.current++}} />
      <FieldDescription>Windfall's General MIDI equivalent uses your own GM SoundFont. Choosing a preset saves this destination on the rack channel; its notes and mixer assignment stay in place, and live playback and export use the same audio engine.</FieldDescription></Field>
    <Button variant="outline" disabled={busy || !path || !backend.soundfontPresets || !backend.sampleInstrumentGeneralMidi} onClick={async()=>{
      const token=++request.current;setBusy(true)
      try {const presets=await backend.soundfontPresets!(path);if(token===request.current){setPresets(presets);setError(null)}}
      catch(error){if(token===request.current)setError(errorMessage(error))}finally{setBusy(false)}
    }}>Browse GM bank presets</Button>
    {presets.map(preset=><Button key={`${preset.bank}:${preset.program}`} variant="outline" disabled={busy} onClick={async()=>{
      const generation=getProjectGeneration();setBusy(true)
      try {const result=await backend.sampleInstrumentGeneralMidi!(channel.id,channel.source,path,preset.bank,preset.program)
        if(generation===getProjectGeneration()){receivePatch(result.patch);setError(null)}}
      catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}finally{setBusy(false)}
    }}>Use GM preset {preset.bank}:{preset.program} · {preset.name}</Button>)}
    {!backend.sampleInstrumentGeneralMidi && <FieldDescription>General MIDI bank loading requires the desktop app.</FieldDescription>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}
