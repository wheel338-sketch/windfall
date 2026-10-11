import { useEffect, useState } from "react"

import type { MidiHardwareState } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { FieldDescription, FieldGroup } from "@/components/ui/field"
import { backend, errorMessage } from "@/lib/ipc"
import { getProjectGeneration } from "@/lib/store/replaced"

const SYSTEM_GM = "windfall:system-general-midi"
const FEEDBACK = "windfall:xinput-feedback"

/** Own special-output workflows on the registered MidiOut instrument. */
export function SpecialOutputEditor() {
  const [state,setState] = useState<MidiHardwareState|null>(null)
  const [busy,setBusy] = useState(false)
  const [error,setError] = useState<string|null>(null)
  useEffect(()=>{
    let current=true
    void backend.midiHardwareState().then(state=>{if(current)setState(state)})
      .catch(error=>{if(current)setError(errorMessage(error))})
    return ()=>{current=false}
  },[])
  async function select(output: string) {
    const generation=getProjectGeneration()
    setBusy(true)
    try {
      const fresh=await backend.midiHardwareState()
      if(generation!==getProjectGeneration())return
      if(!fresh.outputs.some(port=>port.id===output))throw new Error("Requested system output is unavailable")
      const next=await backend.midiHardwareConfigure({...fresh.settings,output})
      if(generation===getProjectGeneration()){setState(next);setError(next.error)}
    } catch(error){if(generation===getProjectGeneration())setError(errorMessage(error))}
    finally{setBusy(false)}
  }
  return <FieldGroup>
    <Button variant="outline" disabled={busy || !state?.outputs.some(port=>port.id===SYSTEM_GM)}
      onClick={()=>void select(SYSTEM_GM)}>Use system General MIDI synth</Button>
    <FieldDescription>Uses the Windows software synth on the output worker. Program and bank controls below select its sounds. The OS plays this audio outside Windfall’s mixer and exports.</FieldDescription>
    <Button variant="outline" disabled={busy || !state?.outputs.some(port=>port.id===FEEDBACK)}
      onClick={()=>void select(FEEDBACK)}>Use game controller feedback</Button>
    <FieldDescription>Requires a Windows build with XInput enabled. MIDI channels 1–4 select controllers 0–3; notes below 64 drive the low motor, notes 64–127 the high motor, and velocity sets strength.</FieldDescription>
    <FieldDescription>The selected output is an app MIDI setting shared by MIDI output channels.</FieldDescription>
    {state && <FieldDescription>Selected output: {state.outputs.find(port=>port.id===state.settings.output)?.name ?? "None"}</FieldDescription>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}
