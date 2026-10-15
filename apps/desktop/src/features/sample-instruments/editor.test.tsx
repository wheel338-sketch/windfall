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
    backend.soundfontLoad=vi.fn().mockImplementation(async(channel,expected,path,bank,program)=>{
      // Patches do not carry the allocator frontier; query the owning document.
      const source=(await backend.documentSnapshot()).project.nextId
      return backend.dispatch({type:"batch",label:"Load SoundFont",commands:[
        {type:"addSample",name:"Own bank",path:{kind:"external",path}},
        {type:"registerSoundFontSource",sample:source,expected:{kind:"external",path}},
        {type:"setSampleInstrumentAsset",channel,expected,asset:{type:"soundFont",path:{kind:"external",path},bank,program,source}}
      ]})
    })
    const picked=vi.spyOn(backend,"pickAudioFile").mockResolvedValue("C:/Own/keys.sf2")
    const id=await open("soundFont")
    // An empty player says it is silent, and there is no path to type.
    expect(screen.getByText("This SoundFont player is silent until you choose a SoundFont and one of its presets.")).toBeInTheDocument()
    expect(screen.queryByRole("textbox",{name:"SoundFont file"})).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button",{name:"Choose file…"}));await act(()=>settle())
    expect(picked).toHaveBeenCalledWith("soundFont")
    expect(backend.soundfontPresets).toHaveBeenCalledWith("C:/Own/keys.sf2")
    fireEvent.click(screen.getByRole("button",{name:"3:7 · Own piano"}));await act(()=>settle())
    expect(useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument)
      .toMatchObject({type:"soundFont",path:{kind:"external",path:"C:/Own/keys.sf2"},bank:3,program:7,source:expect.any(Number)})
  })
  it("groups presets by General MIDI family and finds them by name or family",async()=>{
    backend.soundfontPresets=vi.fn().mockResolvedValue([
      {name:"Grand",bank:0,program:0},{name:"Horn",bank:0,program:60},{name:"Trumpet",bank:0,program:56},
      {name:"Standard",bank:128,program:0},{name:"Own sweep",bank:5,program:3},
    ])
    vi.spyOn(backend,"pickAudioFile").mockResolvedValue("C:/Banks/gm.sf2")
    await open("soundFont")
    fireEvent.click(screen.getByRole("button",{name:"Choose file…"}));await act(()=>settle())
    const list=within(screen.getByRole("group",{name:"SoundFont presets"}))
    const families=list.getAllByRole("group").map(group=>group.getAttribute("aria-label"))
    expect(families).toEqual(["Piano","Brass","Drum Kits","Other banks"])
    expect(within(list.getByRole("group",{name:"Brass"})).getAllByRole("button").map(button=>button.textContent))
      .toEqual(["0:56 · Trumpet","0:60 · Horn"])
    const search=screen.getByRole("searchbox",{name:"Search presets"})
    fireEvent.change(search,{target:{value:"brass"}})
    expect(screen.getAllByRole("button",{name:/^0:(56|60) ·/})).toHaveLength(2)
    expect(screen.queryByRole("button",{name:"0:0 · Grand"})).not.toBeInTheDocument()
    // The General MIDI name finds a preset its bank names otherwise.
    fireEvent.change(search,{target:{value:"french horn"}})
    expect(screen.getByRole("button",{name:"0:60 · Horn"})).toBeInTheDocument()
    expect(screen.queryByRole("button",{name:"0:56 · Trumpet"})).not.toBeInTheDocument()
    fireEvent.change(search,{target:{value:"zzz"}})
    expect(screen.getByText("No preset matches.")).toBeInTheDocument()
  })
  it("keeps the file it had when the picker is cancelled",async()=>{
    backend.soundfontPresets=vi.fn().mockResolvedValue([{name:"Own piano",bank:3,program:7}])
    vi.spyOn(backend,"pickAudioFile").mockResolvedValue(null)
    await open("soundFont")
    fireEvent.click(screen.getByRole("button",{name:"Choose file…"}));await act(()=>settle())
    expect(backend.soundfontPresets).not.toHaveBeenCalled()
    expect(screen.getByText("No SoundFont chosen yet")).toBeInTheDocument()
  })
  it("reopens the saved SoundFont path and browses that bank",async()=>{
    backend.soundfontPresets=vi.fn().mockResolvedValue([{name:"Own piano",bank:3,program:7}])
    const result=await dispatch({type:"addChannel",instrument:"soundFont"})
    if(!result)throw new Error("channel creation failed")
    const id=result.created[0]
    await dispatch({type:"setSampleInstrumentAsset",channel:id,expected:null,
      asset:{type:"soundFont",path:{kind:"external",path:"C:/Own/keys.sf2"},bank:3,program:7}})
    render(<ValueContextMenus><Panel id={id}/></ValueContextMenus>)
    await act(()=>settle())
    expect(screen.getByText("C:/Own/keys.sf2")).toBeInTheDocument()
    expect(screen.getByText("Selected bank 3, program 7")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button",{name:"Browse presets"}));await act(()=>settle())
    expect(backend.soundfontPresets).toHaveBeenCalledWith("C:/Own/keys.sf2")
    expect(screen.getByRole("button",{name:"3:7 · Own piano"})).toBeInTheDocument()
  })
  it("validates SoundFont relink in its dedicated dialog and preserves source identity with undo",async()=>{
    const result=await dispatch({type:"addChannel",instrument:"soundFont"})
    if(!result)throw new Error("channel creation failed")
    const id=result.created[0]
    const added=await dispatch({type:"addSample",name:"Missing bank",path:{kind:"external",path:"C:/Missing/bank.sf2"}})
    if(!added)throw new Error("source creation failed")
    const source=added.created[0]
    await dispatch({type:"registerSoundFontSource",sample:source,expected:{kind:"external",path:"C:/Missing/bank.sf2"}})
    const asset={type:"soundFont" as const,path:{kind:"external" as const,path:"C:/Missing/bank.sf2"},bank:0,program:0,source}
    await dispatch({type:"setSampleInstrumentAsset",channel:id,expected:null,asset})
    backend.sampleInstrumentMissingSources=vi.fn().mockResolvedValue([source])
    backend.soundfontRelink=vi.fn().mockRejectedValue(new Error("Replacement does not contain selected preset"))
    render(<ValueContextMenus><Panel id={id}/></ValueContextMenus>);await act(()=>settle())
    expect(screen.getByText("Missing SoundFont file. Relink its project source to restore playback.").closest('[role="alert"]')).toHaveTextContent("Missing SoundFont file")
    fireEvent.click(screen.getByRole("button",{name:"Relink SoundFont source"}))
    const dialog=within(screen.getByRole("dialog"))
    fireEvent.change(dialog.getByRole("textbox",{name:"Replacement SoundFont file"}),{target:{value:"C:/Recovered/bank.sf2"}})
    fireEvent.click(dialog.getByRole("button",{name:"Verify and relink SoundFont"}));await act(()=>settle())
    expect(dialog.getByRole("alert")).toHaveTextContent("Replacement does not contain selected preset")
    expect(useProjectStore.getState().project.channels.find(c=>c.id===id)?.sampleInstrument).toEqual(asset)
    backend.soundfontRelink=vi.fn().mockImplementation(async(_channel,expected,expectedPath,path)=>backend.dispatch({
      type:"relinkSamplePath",sample:expected.type==="soundFont" ? expected.source! : 0,expected:expectedPath,path:{kind:"external",path}}))
    backend.sampleInstrumentMissingSources=vi.fn().mockResolvedValue([])
    fireEvent.click(dialog.getByRole("button",{name:"Verify and relink SoundFont"}));await act(()=>settle())
    expect(useProjectStore.getState().project.samples.find(s=>s.id===source)?.path).toEqual({kind:"external",path:"C:/Recovered/bank.sf2"})
    expect(useProjectStore.getState().project.channels.find(c=>c.id===id)?.sampleInstrument).toEqual(asset)
    await undo();await act(()=>settle())
    expect(useProjectStore.getState().project.samples.find(s=>s.id===source)?.path).toEqual(asset.path)
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
  it("cancels a pending native render and leaves the recipe unchanged",async()=>{
    let rejectRender!: (reason: Error) => void
    backend.sampleInstrumentSpeechBegin=vi.fn().mockResolvedValue(17)
    backend.sampleInstrumentSpeech=vi.fn().mockImplementation(()=>new Promise((_,reject)=>{rejectRender=reject}))
    backend.sampleInstrumentSpeechCancel=vi.fn().mockImplementation(async()=>{rejectRender(new Error("Speech rendering cancelled"))})
    const id=await open("textVoice")
    const before=useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument
    fireEvent.change(screen.getByRole("textbox",{name:"Text"}),{target:{value:"Hello"}})
    fireEvent.click(screen.getByRole("button",{name:"Render and save source"}));await act(()=>settle())
    expect(backend.sampleInstrumentSpeech).toHaveBeenCalledWith(id,null,"Hello",48,2,17)
    fireEvent.click(screen.getByRole("button",{name:"Cancel speech rendering"}));await act(()=>settle())
    expect(backend.sampleInstrumentSpeechCancel).toHaveBeenCalledWith(id,17)
    expect(screen.getByRole("alert")).toHaveTextContent("cancelled")
    expect(useProjectStore.getState().project.channels.find(item=>item.id===id)?.sampleInstrument).toEqual(before)
    expect(screen.getByRole("button",{name:"Render and save source"})).toBeEnabled()
  })

  it("relinks a player source without replacing its bank identity",async()=>{
    backend.sampleInstrumentMissingSources=vi.fn().mockResolvedValue([useProjectStore.getState().project.samples[0].id])
    backend.sampleInstrumentRelinkSource=vi.fn().mockImplementation(async(_channel,_bank,sample,expected,path)=>backend.dispatch({type:"relinkSamplePath",sample,expected,path:{kind:"external",path}}))
    const id=await open("samplePlayer")
    const source=useProjectStore.getState().project.samples[0]
    const {newZone}=await import("./editor")
    await dispatch({type:"setSampleInstrumentAsset",channel:id,expected:null,asset:{type:"zones",zones:[newZone(source.id)]}});await act(()=>settle())
    const bank=useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.sampleInstrument
    vi.spyOn(backend,"pickAudioFile").mockResolvedValue("C:/Relink/new.wav")
    await act(()=>settle())
    expect(screen.getByRole("alert")).toHaveTextContent("Missing sample-bank sources")
    fireEvent.click(screen.getByRole("button",{name:`Relink source ${source.name}`}));await act(()=>settle())
    expect(backend.sampleInstrumentRelinkSource).toHaveBeenCalledWith(id,bank,source.id,source.path,"C:/Relink/new.wav")
    expect(useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.sampleInstrument).toEqual(bank)
    expect(useProjectStore.getState().project.samples.find(sample=>sample.id===source.id)?.path).toEqual({kind:"external",path:"C:/Relink/new.wav"})
    await undo();await act(()=>settle())
    expect(useProjectStore.getState().project.samples.find(sample=>sample.id===source.id)?.path).toEqual(source.path)
  })

  it("edits grouped modulation and restores the routes with undo",async()=>{
    const id=await open("multiSampler")
    fireEvent.click(screen.getByRole("button",{name:"Add zone"}));await act(()=>settle())
    const pitch=screen.getByRole("spinbutton",{name:"Zone 1 Envelope to pitch cents"})
    fireEvent.change(pitch,{target:{value:"1200"}});fireEvent.blur(pitch);await act(()=>settle())
    expect(useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.sampleInstrument).toMatchObject({type:"zones",zones:[{modulation:{envelopePitchCents:1200}}]})
    await undo();await act(()=>settle())
    expect(screen.getByRole("spinbutton",{name:"Zone 1 Envelope to pitch cents"})).toHaveValue(0)
    expect(screen.getByRole("spinbutton",{name:"Zone 1 LFO 2 to amplitude"})).toBeInTheDocument()
  })

  it("saves a project feedback route with undo without changing app MIDI preferences",async()=>{
    const hardware=await backend.midiHardwareState()
    vi.spyOn(backend,"midiHardwareState").mockResolvedValue({...hardware,outputs:[{id:"windfall:project-xinput-feedback",name:"Project feedback"}]})
    const configure=vi.spyOn(backend,"midiHardwareConfigure")
    const id=await open("midiOut")
    fireEvent.click(screen.getByRole("button",{name:"Use project controller feedback"}));await act(()=>settle())
    expect(useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.source).toMatchObject({type:"instrument",params:{type:"midiOut",feedbackEnabled:true,feedbackController:0,feedbackMotor:0,feedbackStrength:1,feedbackCurve:1}})
    expect(screen.getByRole("combobox",{name:"Project feedback motor"})).toBeInTheDocument()
    expect(configure).not.toHaveBeenCalled()
    await undo();await act(()=>settle())
    expect(screen.getByRole("button",{name:"Use project controller feedback"})).toBeEnabled()
  })

  it("offers the GM bank equivalent and shows a native refusal without changing its destination",async()=>{
    backend.soundfontPresets=vi.fn().mockResolvedValue([{name:"Own GM",bank:0,program:0}])
    backend.sampleInstrumentGeneralMidi=vi.fn().mockRejectedValue(new Error("Unsupported SoundFont content"))
    const id=await open("midiOut")
    const before=useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.source
    fireEvent.change(screen.getByRole("textbox",{name:"General MIDI SoundFont bank"}),{target:{value:"C:/own-gm.sf2"}})
    fireEvent.click(screen.getByRole("button",{name:"Browse GM bank presets"}));await act(()=>settle())
    fireEvent.click(screen.getByRole("button",{name:"Use GM preset 0:0 · Own GM"}));await act(()=>settle())
    expect(backend.sampleInstrumentGeneralMidi).toHaveBeenCalledWith(id,before,"C:/own-gm.sf2",0,0)
    expect(screen.getByText("Unsupported SoundFont content").closest('[role="alert"]')).toHaveTextContent("Unsupported SoundFont content")
    expect(useProjectStore.getState().project.channels.find(channel=>channel.id===id)?.source).toEqual(before)
  })

})
