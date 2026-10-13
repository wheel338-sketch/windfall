import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import { NativeNoteCapabilities } from "./note-capabilities-view"
import { panLawSupported, expressionSupported } from "./note-capabilities"
import { setLanguage } from "@/lib/i18n"
import { dispatch, useProjectStore } from "@/lib/store/project"

const binding={target:{type:"instrument" as const,channel:900},format:"clap",path:"fixture.clap",id:"fixture",name:"Authored instrument",state:[],parameters:[]}
afterEach(()=>{cleanup();setLanguage("en");vi.restoreAllMocks()})
it("shows all native capabilities and visible refusal, then updates the real settings surface",async()=>{
  const app=await startTestApp()
  try {
    const query=vi.spyOn(app.backend,"pluginNoteCapabilities").mockResolvedValue({projectPanLaw:true,instances:true,nativePan:Array(16).fill(true),nativeVolume:Array(16).fill(false)})
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
    expect(expressionSupported(caps, 0, true, false)).toBe(instances && pan)
    expect(expressionSupported(caps, 0, false, true)).toBe(instances && volume)
    expect(expressionSupported(caps, 16, false, false)).toBe(false)
    expect(expressionSupported(caps, -1, true, true)).toBe(false)
    expect(expressionSupported(caps, 0.5, false, true)).toBe(false)
  }
})

it("follows the actual live MIDI channel and announces capability loss and return", async () => {
  const app = await startTestApp()
  try {
    const pan = Array(16).fill(false), volume = Array(16).fill(false)
    pan[3] = true
    volume[3] = true
    const query = vi.spyOn(app.backend, "pluginNoteCapabilities").mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: volume, observedChannels: [3] })
    render(<NativeNoteCapabilities binding={binding} />)
    await waitFor(() => expect(screen.getByText("Note MIDI channel 4")).toBeVisible())
    expect(screen.getByText("Pan route: Native; Volume route: Native")).toBeVisible()
    expect(screen.queryByText("Note MIDI channel 1")).toBeNull()
    query.mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: Array(16).fill(false), observedChannels: [3] })
    await waitFor(() => expect(screen.getByText("Pan route: Native; Volume route: Unavailable")).toBeVisible(), { timeout: 2000 })
    expect(screen.getByRole("status")).toHaveTextContent("Plugin capabilities changed")
    query.mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: volume, observedChannels: [3], refusal: "Invalid note MIDI channel: expected a zero-based channel from 0 through 15; the note was refused." })
    await waitFor(() => expect(screen.getByText("Pan route: Native; Volume route: Native")).toBeVisible(), { timeout: 2000 })
    expect(screen.getByText(/Invalid note MIDI channel/)).toBeVisible()
  } finally { app.stop() }
})

it("uses the saved note colour channel when the plugin supports a different channel from zero", async () => {
  const app = await startTestApp()
  try {
    await act(async () => { await dispatch({ type: "addChannel", name: "Colour probe" }) })
    const project = useProjectStore.getState().project
    const channel = project.channels.find((channel) => channel.name === "Colour probe")!
    const pattern = project.patterns[0].id
    await act(async () => {
      await dispatch({ type: "addNotes", pattern, channel: channel.id, notes: [{ start: 0, length: 240, key: 69, expression: { release: 0.5, finePitchCents: 0, modulationX: 0.5, modulationY: 0.5, colorGroup: 5 } }] })
    })
    const pan = Array(16).fill(false), volume = Array(16).fill(false)
    pan[5] = true
    volume[5] = true
    vi.spyOn(app.backend, "pluginNoteCapabilities").mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: volume })
    render(<NativeNoteCapabilities binding={{ ...binding, target: { type: "instrument", channel: channel.id } }} />)
    await waitFor(() => expect(screen.getByText("Pan route: Native; Volume route: Native")).toBeVisible())
    expect(screen.getByText("Note MIDI channel 6")).toBeVisible()
    expect(screen.queryByText("Note MIDI channel 1")).toBeNull()
    expect(screen.queryByText(/cannot apply the project law/)).toBeNull()
  } finally { app.stop() }
})
