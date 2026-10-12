import { useEffect, useState } from "react"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { FieldDescription } from "@/components/ui/field"
import { effectDescriptor, ParamControl, ParamRow, type EffectParamsOf } from "@/features/params"
import { backend, errorMessage } from "@/lib/ipc"
import type { EditorProps } from "./editor-props"

export function MusicLightingEditor({ bind }: EditorProps<EffectParamsOf<"musicLighting">>) {
  const [message,setMessage]=useState("Checking local lighting service...")
  const [connected,setConnected]=useState(false)
  async function refresh() {
    if(!backend.musicLightingStatus){setMessage("Music lighting requires the desktop app and a local Chroma REST service.");return}
    try {const status=await backend.musicLightingStatus();setConnected(status.connected);setMessage(status.message)}
    catch(error){setConnected(false);setMessage(errorMessage(error))}
  }
  useEffect(()=>{
    let current=true
    const read=async()=>{
      if(!backend.musicLightingStatus){if(current)setMessage("Music lighting requires the desktop app and a local Chroma REST service.");return}
      try {const status=await backend.musicLightingStatus();if(current){setConnected(status.connected);setMessage(status.message)}}
      catch(error){if(current){setConnected(false);setMessage(errorMessage(error))}}
    }
    void read();const timer=setInterval(()=>void read(),1000)
    return ()=>{current=false;clearInterval(timer)}
  },[])
  return <div className="flex flex-col gap-3">
    <FieldDescription>Source: 0 tempo pulse, 1 audio energy, 2 both. Device: 0 all, 1 keyboard, 2 mouse, 3 mousepad, 4 headset, 5 keypad, 6 linked devices. Uses this mixer track's meters and the session's musical clock.</FieldDescription>
    <ParamRow>{effectDescriptor("musicLighting").params.map(info=><ParamControl key={info.id} {...bind(info.id)} label={info.name} />)}</ParamRow>
    <Alert variant={connected ? "default" : "destructive"}><AlertDescription>{message}</AlertDescription></Alert>
    <Button variant="outline" onClick={()=>void refresh()}>Refresh lighting service</Button>
  </div>
}
