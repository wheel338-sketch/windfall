import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import { NativeNoteCapabilities } from "./note-capabilities-view"
import { panLawSupported, expressionSupported, noteRoutes } from "./note-capabilities"
import { setLanguage } from "@/lib/i18n"
import { dispatch, useProjectStore } from "@/lib/store/project"

// A hosted plugin returns one summed output. Each of a note's pan and volume
// therefore has one route: in the plugin, per note, where the plugin offers
// that, and on the channel after the plugin where it does not. The panel
// says which is which; nothing is refused for a valid channel.
const IN_PLUGIN = "In the plugin, per note"
const ON_CHANNEL = "On the channel, after the plugin"
const routes = (pan: string, volume: string) => `Pan route: ${pan}; Volume route: ${volume}`

const binding={target:{type:"instrument" as const,channel:900},format:"clap",path:"fixture.clap",id:"fixture",name:"Authored instrument",state:[],parameters:[]}
afterEach(()=>{cleanup();setLanguage("en");vi.restoreAllMocks()})
it("shows which route each property takes and explains the channel route, then updates the real settings surface",async()=>{
  const app=await startTestApp()
  try {
    const query=vi.spyOn(app.backend,"pluginNoteCapabilities").mockResolvedValue({projectPanLaw:true,instances:true,nativePan:Array(16).fill(true),nativeVolume:Array(16).fill(false)})
    render(<NativeNoteCapabilities binding={binding}/>);
    await waitFor(()=>expect(screen.getByText(/Pan: Available; Volume: Unavailable/)).toBeVisible())
    expect(screen.getByText(routes(IN_PLUGIN, ON_CHANNEL))).toBeVisible()
    expect(screen.getByText(/The project pan law applies to every note/)).toBeVisible()
    expect(screen.getByText(/follows the most recent note that sounds/)).toBeVisible()
    expect(screen.queryByText(/cannot apply the project law/)).toBeNull()
    act(()=>setLanguage("es"))
    expect(screen.getByText(/Panorama: Disponible; volumen: No disponible/)).toBeVisible()
    expect(screen.getByText("Capacidades nativas de nota de Authored instrument")).toBeVisible()
    expect(screen.getByText("Ruta de panorama: En el plugin, por nota; ruta de volumen: En el canal, después del plugin")).toBeVisible()
    expect(screen.getByText(/La ley de panorama del proyecto se aplica a todas las notas/)).toBeVisible()
    query.mockResolvedValue({projectPanLaw:true,instances:true,nativePan:Array(16).fill(true),nativeVolume:Array(16).fill(true)})
    // With both properties in the plugin there is nothing left to explain.
    await waitFor(()=>expect(screen.queryByText(/La ley de panorama del proyecto se aplica/)).toBeNull(),{timeout:2000})
    expect(screen.getByText("Ruta de panorama: En el plugin, por nota; ruta de volumen: En el plugin, por nota")).toBeVisible()
  } finally { app.stop() }
})
it("gives every valid channel one route for pan and one for volume across the four combinations",()=>{
  for (const instances of [false,true]) for (const pan of [false,true]) for (const volume of [false,true]) {
    const caps={projectPanLaw:true,instances,nativePan:[pan,false],nativeVolume:[volume,false]}
    // The route follows what the plugin offers per note on that channel.
    expect(noteRoutes(caps)).toEqual({ pan: instances && pan ? "plugin" : "channel", volume: instances && volume ? "plugin" : "channel" })
    expect(noteRoutes(caps,1)).toEqual({ pan: "channel", volume: "channel" })
    // Whatever the routes, the law applies and both destinations can be mapped.
    expect(panLawSupported(caps)).toBe(true)
    expect(panLawSupported(caps,1)).toBe(true)
    expect(panLawSupported({...caps,projectPanLaw:false})).toBe(false)
    expect(expressionSupported(caps, 0, true, false)).toBe(true)
    expect(expressionSupported(caps, 0, false, true)).toBe(true)
    expect(expressionSupported(caps, 0, true, true)).toBe(true)
    // Only an invalid channel or an unknown plugin is refused.
    expect(panLawSupported(caps, 16)).toBe(false)
    expect(expressionSupported(caps, 16, false, false)).toBe(false)
    expect(expressionSupported(caps, -1, true, true)).toBe(false)
    expect(expressionSupported(caps, 0.5, false, true)).toBe(false)
    expect(expressionSupported(undefined, 0, true, true)).toBe(false)
    expect(noteRoutes(caps, 16)).toEqual({ pan: "channel", volume: "channel" })
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
    expect(screen.getByText(routes(IN_PLUGIN, IN_PLUGIN))).toBeVisible()
    expect(screen.queryByText("Note MIDI channel 1")).toBeNull()
    expect(screen.queryByText(/The project pan law applies to every note/)).toBeNull()
    // The plugin loses per-note volume: that property moves to the channel.
    query.mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: Array(16).fill(false), observedChannels: [3] })
    await waitFor(() => expect(screen.getByText(routes(IN_PLUGIN, ON_CHANNEL))).toBeVisible(), { timeout: 2000 })
    expect(screen.getByRole("status")).toHaveTextContent("Plugin capabilities changed")
    expect(screen.getByText(/The project pan law applies to every note/)).toBeVisible()
    // It loses note instances altogether: both properties are the channel's.
    query.mockResolvedValue({ projectPanLaw: true, instances: false, nativePan: pan, nativeVolume: volume, observedChannels: [3] })
    await waitFor(() => expect(screen.getByText(routes(ON_CHANNEL, ON_CHANNEL))).toBeVisible(), { timeout: 2000 })
    // And everything returns.
    query.mockResolvedValue({ projectPanLaw: true, instances: true, nativePan: pan, nativeVolume: volume, observedChannels: [3], refusal: "Invalid note MIDI channel: expected a zero-based channel from 0 through 15; the note was refused." })
    await waitFor(() => expect(screen.getByText(routes(IN_PLUGIN, IN_PLUGIN))).toBeVisible(), { timeout: 2000 })
    expect(screen.queryByText(/The project pan law applies to every note/)).toBeNull()
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
    await waitFor(() => expect(screen.getByText(routes(IN_PLUGIN, IN_PLUGIN))).toBeVisible())
    expect(screen.getByText("Note MIDI channel 6")).toBeVisible()
    expect(screen.queryByText("Note MIDI channel 1")).toBeNull()
    expect(screen.queryByText(/The project pan law applies to every note/)).toBeNull()
  } finally { app.stop() }
})
