import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { backend } from "@/lib/ipc"
import { startTestApp } from "@/test/harness"
import { NativeNoteCapabilities } from "./note-capabilities-view"
import { panLawSupported } from "./note-capabilities"
import { setLanguage } from "@/lib/i18n"

const binding={target:{type:"instrument" as const,channel:900},format:"clap",path:"fixture.clap",id:"fixture",name:"Authored instrument",state:[],parameters:[]}
afterEach(()=>{cleanup();setLanguage("en");vi.restoreAllMocks()})
it("shows all native capabilities and visible refusal, then updates the real settings surface",async()=>{
  const app=await startTestApp()
  try {
    const query=vi.spyOn(backend,"pluginNoteCapabilities").mockResolvedValue({projectPanLaw:true,instances:true,nativePan:Array(16).fill(true),nativeVolume:Array(16).fill(false)})
    render(<NativeNoteCapabilities binding={binding}/>);
    await waitFor(()=>expect(screen.getByText(/Pan: Available; Volume: Unavailable/)).toBeVisible())
    expect(screen.getByText(/cannot apply the project law/)).toBeVisible()
    act(()=>setLanguage("es"))
    expect(screen.getByText(/Panorama: Disponible; volumen: No disponible/)).toBeVisible()
    expect(screen.getByText("Capacidades nativas de nota de Authored instrument")).toBeVisible()
    query.mockResolvedValue({projectPanLaw:true,instances:true,nativePan:Array(16).fill(true),nativeVolume:Array(16).fill(true)})
    await waitFor(()=>expect(screen.queryByText(/no puede aplicar/)).toBeNull(),{timeout:2000})
  } finally { app.stop() }
})
it("requires actual per-channel Pan, Volume, instances and the provider's implementation",()=>{
  for (const instances of [false,true]) for (const pan of [false,true]) for (const volume of [false,true]) {
    const caps={projectPanLaw:true,instances,nativePan:[pan,false],nativeVolume:[volume,false]}
    expect(panLawSupported(caps)).toBe(instances&&pan&&volume)
    expect(panLawSupported(caps,1)).toBe(false)
    expect(panLawSupported({...caps,projectPanLaw:false})).toBe(false)
  }
})
