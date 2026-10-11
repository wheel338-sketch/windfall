import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { InstrumentKind } from "@/bindings"
import { ValueContextMenus } from "@/components/value-context-menu"
import { InstrumentSection } from "@/features/channel-rack/inspector/instrument-section"
import { isInstrumentChannel } from "@/lib/channel-source"
import type { MockBackend } from "@/lib/ipc/mock"
import { dispatch, undo, useProjectStore } from "@/lib/store"
import { settle, startTestApp } from "@/test/harness"

vi.mock("../channel-rack/actions", () => ({ synthSoundActionId: (id: string) => `channel.synthSound.${id}` }))

let backend: MockBackend
let stop: () => void
beforeEach(async()=>{({backend,stop}=await startTestApp())})
afterEach(()=>{stop();vi.restoreAllMocks()})
function Panel({ id }: { id: number }) {
  const channel=useProjectStore(state=>state.project.channels.find(item=>item.id===id))
  return channel && isInstrumentChannel(channel) ? <InstrumentSection channel={channel} /> : null
}
async function open(kind: InstrumentKind) {
  const result=await dispatch({type:"addChannel",instrument:kind})
  if(!result)throw new Error("channel creation failed")
  const id=result.created[0]
  render(<ValueContextMenus><Panel id={id}/></ValueContextMenus>)
  await act(()=>settle())
  return id
}
describe("sample instrument purpose-built editors",()=>{
  it("maps a source into a zone, edits a boundary and restores it through undo",async()=>{
    const id=await open("multiSampler")
    expect(screen.getByRole("img",{name:/Zone map/})).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button",{name:"Add zone"}));await act(()=>settle())
    const low=screen.getByRole("spinbutton",{name:"Zone 1 low key"})
    fireEvent.change(low,{target:{value:"48"}});fireEvent.blur(low);await act(()=>settle())
    const asset=()=>useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument
    expect(asset()).toMatchObject({type:"zones",zones:[{keyLow:48}]})
    await undo();await act(()=>settle())
    expect(asset()).toMatchObject({type:"zones",zones:[{keyLow:0}]})
  })
  it("shows sixteen pads and adds independent layered sources to the selected pad",async()=>{
    const id=await open("padBank")
    const pads=within(screen.getByRole("group",{name:"Sixteen drum pads"}))
    expect(pads.getAllByRole("button")).toHaveLength(16)
    fireEvent.click(pads.getByRole("button",{name:/Pad 2 ·/}))
    fireEvent.click(screen.getByRole("button",{name:"Add pad layer"}));await act(()=>settle())
    fireEvent.click(screen.getByRole("button",{name:"Add pad layer"}));await act(()=>settle())
    const asset=useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument
    expect(asset).toMatchObject({type:"zones",zones:[{keyLow:37,keyHigh:37},{keyLow:37,keyHigh:37}]})
    expect(screen.getByRole("spinbutton",{name:"Zone 1 choke group"})).toBeInTheDocument()
  })
  it("sequences the selected pad without replacing another pad and restores the step with undo",async()=>{
    const id=await open("padBank")
    fireEvent.click(screen.getByRole("button",{name:"Pad 1 step 1"}));await act(()=>settle())
    fireEvent.click(within(screen.getByRole("group",{name:"Sixteen drum pads"})).getByRole("button",{name:/Pad 2 ·/}))
    fireEvent.click(screen.getByRole("button",{name:"Pad 2 step 1"}));await act(()=>settle())
    const notes=()=>useProjectStore.getState().project.patterns.flatMap(pattern=>pattern.lanes.filter(lane=>lane.channel===id).flatMap(lane=>lane.notes))
    expect(notes().map(note=>note.key)).toEqual([36,37])
    fireEvent.click(screen.getByRole("button",{name:"Pad 2 step 1"}));await act(()=>settle())
    expect(notes().map(note=>note.key)).toEqual([36])
    await undo();await act(()=>settle());expect(notes().map(note=>note.key)).toEqual([36,37])
  })
  it("keeps the player mapping read-only and reports malformed banks",async()=>{
    await open("samplePlayer")
    expect(screen.queryByRole("button",{name:"Add zone"})).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole("textbox",{name:"Ready-made bank references"}),{target:{value:"{"}})
    fireEvent.click(screen.getByRole("button",{name:"Load bank"}))
    expect(screen.getByRole("alert")).toBeInTheDocument()
  })
  it("browses bank/program presets and retains their file reference",async()=>{
    backend.soundfontPresets=vi.fn().mockResolvedValue([{name:"Own piano",bank:3,program:7}])
    const id=await open("soundFont")
    fireEvent.change(screen.getByRole("textbox",{name:"SoundFont file"}),{target:{value:"C:/Own/keys.sf2"}})
    fireEvent.click(screen.getByRole("button",{name:"Browse presets"}));await act(()=>settle())
    fireEvent.click(screen.getByRole("button",{name:"3:7 · Own piano"}));await act(()=>settle())
    expect(useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument)
      .toEqual({type:"soundFont",path:{kind:"external",path:"C:/Own/keys.sf2"},bank:3,program:7})
  })
  it("switches generated piano recipes with undo and exposes the active source",async()=>{
    const id=await open("pianoKeys")
    expect(screen.getByText("Current source: Struck-string piano")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button",{name:"Switch piano / electric keyboard"}));await act(()=>settle())
    expect(useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument).toEqual({type:"piano",electric:true,version:1})
    expect(screen.getByText("Current source: Electric keyboard")).toBeInTheDocument()
    await undo();await act(()=>settle())
    expect(screen.getByText("Current source: Struck-string piano")).toBeInTheDocument()
  })
  it("reports a native portable-bank loading error without changing the player",async()=>{
    backend.sampleInstrumentLoadBank=vi.fn().mockRejectedValue(new Error("Missing bank source"))
    const id=await open("samplePlayer")
    fireEvent.change(screen.getByRole("textbox",{name:"Bank file"}),{target:{value:"C:/Own/player.wfbank"}})
    fireEvent.click(screen.getByRole("button",{name:"Load instrument bank"}));await act(()=>settle())
    expect(screen.getByRole("alert")).toHaveTextContent("Missing bank source")
    expect(useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument).toBeUndefined()
  })
  it("shows desktop-only speech capability without fabricating a rendered source",async()=>{
    await open("textVoice")
    fireEvent.change(screen.getByRole("textbox",{name:"Text"}),{target:{value:"Hello"}})
    expect(screen.getByRole("button",{name:"Render and save source"})).toBeDisabled()
    expect(screen.getByRole("alert")).toHaveTextContent("desktop app")
  })
})
