import { useEffect, useState } from "react"
import type { MidiHardwareState, InstrumentParams } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { FieldDescription, FieldGroup } from "@/components/ui/field"
import { backend, errorMessage } from "@/lib/ipc"
import type { InstrumentChannel } from "@/lib/channel-source"
import { dispatch, useProjectStore } from "@/lib/store"
import { ControllerChoice } from "@/features/controllers/links-panel"
import { instrumentDescriptor, ParamControl, ParamGroup, ParamRow } from "@/features/params"
import { useInstrumentBinding } from "@/features/channel-rack/instruments/use-instrument-binding"
import { getProjectGeneration } from "@/lib/store/replaced"
const SYSTEM_GM = "windfall:system-general-midi"
const FEEDBACK = "windfall:xinput-feedback"
const PROJECT_FEEDBACK = "windfall:project-xinput-feedback"
function ProjectFeedbackEditor({ channel, hardware, refresh }: { channel: InstrumentChannel; hardware: MidiHardwareState|null; refresh: () => Promise<void> }) {
  const bind=useInstrumentBinding(channel.id,channel.source.params)
  const params=channel.source.params
  if(params.type!=="midiOut")return null
  const available=hardware?.outputs.some(port=>port.id===PROJECT_FEEDBACK)??false
  function route(change: Partial<Extract<InstrumentParams, {type:"midiOut"}>>)  {
    const current=useProjectStore.getState().project.channels.find(item=>item.id===channel.id)
    if(current?.source.type!=="instrument" || current.source.params.type!=="midiOut")return
    void dispatch({type:"setInstrumentParams",channel:channel.id,params:{...current.source.params,...change}})
  }
  return <FieldGroup>
    <Button variant="outline" disabled={!available} onClick={()=>route({feedbackEnabled:!params.feedbackEnabled,pluginDestination:0})}>
      {params.feedbackEnabled ? "Disconnect project controller feedback" : "Use project controller feedback"}
    </Button>
    <FieldDescription>Saved with this instrument, including undo and reopen. Original notes drive the selected controller and motor. Strength is maximum times velocity raised to the curve exponent.</FieldDescription>
    {!available && <FieldDescription role="status">Project feedback requires a Windows build with the xinput feature. Real controller presence is checked when a note is sent.</FieldDescription>}
    <ControllerChoice<string> label="Project feedback controller" value={String(params.feedbackController??0)} choices={Array.from({length:4},(_,index)=>({value:String(index),label:`Controller ${index+1}`}))} change={value=>route({feedbackController:Number(value)})} />
    <ControllerChoice<string> label="Project feedback motor" value={String(params.feedbackMotor??0)} choices={[{value:"0",label:"Low-frequency motor"},{value:"1",label:"High-frequency motor"},{value:"2",label:"Both motors"}]} change={value=>route({feedbackMotor:Number(value)})} />
    <ParamGroup title="Motor strength mapping"><ParamRow>{instrumentDescriptor("midiOut").params.slice(24,26).map(info=><ParamControl key={info.id} {...bind(info.id)} label={info.name} />)}</ParamRow></ParamGroup>
    <Button variant="outline" onClick={()=>void refresh()}>Refresh controller capability</Button>
  </FieldGroup>
}
/** Own special-output workflows on the registered MidiOut instrument. */
export function SpecialOutputEditor({ channel }: { channel?: InstrumentChannel }) {
  const [state,setState] = useState<MidiHardwareState|null>(null)
  const [busy,setBusy] = useState(false)
  const [error,setError] = useState<string|null>(null)
  useEffect(()=>{
    let current=true
    void backend.midiHardwareState().then(state=>{if(current){setState(state);setError(state.error)}})
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
  async function refresh() {
    try { const fresh=await backend.midiHardwareState();setState(fresh);setError(fresh.error) }
    catch(error){setError(errorMessage(error))}
  }
  return <FieldGroup>
    {channel && <ProjectFeedbackEditor channel={channel} hardware={state} refresh={refresh} />}
    <Button variant="outline" disabled={busy || !state?.outputs.some(port=>port.id===SYSTEM_GM)}
      onClick={()=>void select(SYSTEM_GM)}>Use system General MIDI synth</Button>
    <FieldDescription>Uses the Windows software synth on the output worker. Program and bank controls below select its sounds. The OS plays this audio outside Windfall’s mixer and exports.</FieldDescription>
    {!channel && <><Button variant="outline" disabled={busy || !state?.outputs.some(port=>port.id===FEEDBACK)}
      onClick={()=>void select(FEEDBACK)}>Use game controller feedback</Button>
    <FieldDescription>Requires a Windows build with XInput enabled. MIDI channels 1–4 select controllers 0–3; notes below 64 drive the low motor, notes 64–127 the high motor, and velocity sets strength.</FieldDescription></>}
    <FieldDescription>The selected output is an app MIDI setting shared by MIDI output channels.</FieldDescription>
    {state && <FieldDescription>Selected output: {state.outputs.find(port=>port.id===state.settings.output)?.name ?? "None"}</FieldDescription>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </FieldGroup>
}
