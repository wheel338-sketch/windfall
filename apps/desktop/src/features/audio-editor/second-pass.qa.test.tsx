import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { startTestApp } from "@/test/harness"
import { receivePatch, useProjectStore, undo, redo } from "@/lib/store/project"
import { SimDocument } from "@/lib/ipc/sim/document"
import { usePlaylistStore } from "@/features/playlist/store"
import { EffectEditor } from "@/features/effects/effect-editor"
import { unavailableLogger } from "@/features/audio-logger/types"
import { AudioEditorButton } from "./index"
import { SELECTION_DRAG } from "./transfer"
import { ChannelInspector } from "@/features/channel-rack/inspector"
import { useUiStore } from "@/lib/store/ui"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async()=>{app=await startTestApp()})
afterEach(()=>{app.stop();vi.restoreAllMocks()})
const project=()=>useProjectStore.getState().project
function reopened() {const doc=SimDocument.create(project());const opened=SimDocument.open(doc.fileText());const saved=opened.project();doc.dispose();opened.dispose();return saved}
async function addClip() {
  const result=await app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav",{start:0})
  receivePatch(result.patch)
  const id=result.created.at(-1)!
  usePlaylistStore.getState().select([id])
  return id
}
function previewAdapter() {
  vi.spyOn(app.backend,"audioEditorOpen").mockImplementation(async clip=>{
    const source=project().playlist.clips.find(c=>c.id===clip)
    const sampleId=source?.content.type==="audio" ? source.content.sample : null
    const sample=project().samples.find(s=>s.id===sampleId)
    return {token:project().nextId,clip,name:"Fixture view",frames:1000,sampleRate:1000,peaks:[-.5,.5],spectral:Array.from({length:8192},(_,i)=>i%64/64),regions:sample?.regions ?? []}
  })
}

it("mounts persistent region/marker edits against WASM with one undo, reopen and spectral selection",async()=>{
  const id=await addClip()
  previewAdapter()
  const write=vi.spyOn(app.backend,"audioEditorRegions").mockImplementation(async(_token,regions)=>{
    const clip=project().playlist.clips.find(c=>c.id===id)!
    if(clip.content.type!=="audio")throw new Error("audio")
    const sampleId=clip.content.sample
    const sample=project().samples.find(s=>s.id===sampleId)!
    return app.backend.dispatch({type:"setSampleRegions",sample:sample.id,expected:sample.regions ?? [],regions})
  })
  render(<AudioEditorButton clip={id}/>)
  await userEvent.click(screen.getByRole("button",{name:"Audio editor"}))
  await screen.findByRole("img",{name:/Clip waveform/})
  await userEvent.click(screen.getByRole("button",{name:"Show spectrum"}))
  expect(screen.getByRole("img",{name:/Clip spectrogram/})).toBeVisible()
  fireEvent.change(screen.getByLabelText("Start frame (included)"),{target:{value:"100"}})
  fireEvent.change(screen.getByLabelText("End frame (excluded)"),{target:{value:"500"}})
  const cursor=useProjectStore.getState().history.cursor
  await userEvent.click(screen.getByRole("button",{name:"Add region"}))
  await waitFor(()=>expect(write).toHaveBeenCalledOnce())
  await screen.findByRole("button",{name:"Region · 100–500"})
  expect(useProjectStore.getState().history.cursor).toBe(cursor+1)
  const saved=project().samples.find(s=>(s.regions?.length ?? 0)>0)!
  expect(reopened().samples.find(s=>s.id===saved.id)?.regions).toEqual(saved.regions)
  await act(async()=>{await undo()})
  expect(project().samples.find(s=>s.id===saved.id)?.regions ?? []).toEqual([])
  await act(async()=>{await redo()})
  expect(project().samples.find(s=>s.id===saved.id)?.regions).toEqual(saved.regions)
  await userEvent.click(screen.getByRole("button",{name:"Close editor"}))
  await userEvent.click(screen.getByRole("button",{name:"Audio editor"}))
  await screen.findByRole("img",{name:/Clip waveform/})
  await userEvent.click(screen.getByRole("button",{name:"Add marker at selection start"}))
  await waitFor(()=>expect(project().samples.find(s=>s.id===saved.id)?.regions).toHaveLength(2))
  const marker=project().samples.find(s=>s.id===saved.id)?.regions?.[1]
  expect(marker?.startFrame).toBe(marker?.endFrame)
})

it("mounts advanced controls and carries selected range, quality and formant choices to native processing",async()=>{
  const id=await addClip();previewAdapter()
  const apply=vi.spyOn(app.backend,"audioEditorApply").mockRejectedValue(new Error("Refused by native fixture"))
  render(<AudioEditorButton clip={id} defaultOpen/>)
  await screen.findByRole("img",{name:/Clip waveform/})
  fireEvent.change(screen.getByLabelText("Start frame (included)"),{target:{value:"100"}})
  fireEvent.change(screen.getByLabelText("End frame (excluded)"),{target:{value:"900"}})
  for(const [name,operation] of [
    ["Equalize selection",{equalize:{lowDb:0,midDb:-3,highDb:0}}],
    ["Blur selection",{blur:{amount:.25}}],
    ["Repair clipping",{repairClipping:{threshold:.98}}],
    ["Tune loop seam",{loopTune:{crossfadeFrames:10}}],
    ["Split and strip silence",{stripSilence:{thresholdDb:-60,minSilenceFrames:50}}],
  ] as const){await userEvent.click(screen.getByRole("button",{name}));await waitFor(()=>expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({operation,startFrame:100,endFrame:900})));await waitFor(()=>expect(screen.getByRole("button",{name})).toBeEnabled())}
  await userEvent.click(screen.getByRole("button",{name:"Use selection as impulse"}))
  await userEvent.click(screen.getByRole("button",{name:"Convolve selection"}))
  await waitFor(()=>expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({operation:{convolve:{impulse:{startFrame:100,endFrame:900},wet:.5}}})))
  await waitFor(()=>expect(screen.getByLabelText("Length (%)")).toBeEnabled())
  fireEvent.change(screen.getByLabelText("Length (%)"),{target:{value:"150"}})
  await userEvent.click(screen.getByRole("combobox",{name:"Stretch quality"}))
  await userEvent.click(await screen.findByRole("option",{name:"High"}))
  await userEvent.click(screen.getByRole("checkbox",{name:"Preserve formants"}))
  await userEvent.click(screen.getByRole("button",{name:"Stretch and pitch"}))
  await waitFor(()=>expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({operation:{stretch:{ratio:1.5,semitones:0,quality:"high",formants:true}}})))
})

it("mounts the real slot recorder through capture, take attachment, history and editor open",async()=>{
  const added=await app.backend.dispatch({type:"addEffect",track:0,kind:"sendTap"});receivePatch(added.patch)
  const slot=project().mixer.tracks[0].effects.find(s=>s.id===added.created[0])!
  let state=unavailableLogger();state={...state,error:null,settings:{...state.settings,enabled:true}}
  vi.spyOn(app.backend,"audioLoggerState").mockImplementation(async()=>state)
  const configure=vi.spyOn(app.backend,"audioLoggerConfigure").mockImplementation(async settings=>{
    state={...state,settings,running:true,sources:(settings.effects ?? []).map(effect=>({source:{type:"effect" as const,effect},label:"Recorder",sampleRate:1000,capacitySeconds:30,availableSeconds:1,availableFrames:1000,droppedFrames:0,memoryBytes:256000,active:true,error:null}))};return state
  })
  const dump=vi.spyOn(app.backend,"audioLoggerDump").mockImplementation(()=>app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav",{start:0}))
  previewAdapter()
  render(<EffectEditor trackId={0} slot={slot}/>)
  await waitFor(()=>expect(screen.getByRole("button",{name:"Record this slot"})).toBeEnabled())
  const cursor=useProjectStore.getState().history.cursor
  await userEvent.click(screen.getByRole("button",{name:"Record this slot"}))
  await waitFor(()=>expect(configure).toHaveBeenCalledWith(expect.objectContaining({effects:[slot.id]})))
  expect(useProjectStore.getState().history.cursor).toBe(cursor)
  await userEvent.click(screen.getByRole("button",{name:"Stop and edit slot recording"}))
  await screen.findByRole("img",{name:/Clip waveform/})
  expect(dump).toHaveBeenCalledWith({source:{type:"effect",effect:slot.id},seconds:null,start:0,track:null})
  expect(useProjectStore.getState().history.cursor).toBe(cursor+1)
  const clips=structuredClone(project().playlist.clips)
  expect(reopened().playlist.clips).toEqual(clips)
  await act(async()=>{await undo()});expect(project().playlist.clips).toHaveLength(clips.length-1)
  await act(async()=>{await redo()});expect(project().playlist.clips).toEqual(clips)
})

it("exports the actual drag selection to a sampler in one guarded WASM edit",async()=>{
  const id=await addClip();previewAdapter()
  const channel=project().channels[0]
  const move=vi.spyOn(app.backend,"audioEditorTransfer").mockImplementation(async request=>{
    if(request.destination.type!=="sampler")throw new Error("sampler")
    const sample=project().nextId
    return app.backend.dispatch({type:"batch",label:"Editor selection",commands:[{type:"addSample",name:"Selection",path:{kind:"external",path:"/tmp/editor-selection.wav"}},{type:"setChannelSample",id:request.destination.channel,sample}]})
  })
  useUiStore.getState().selectChannel(channel.id)
  const mounted=render(<><AudioEditorButton clip={id} defaultOpen/><ChannelInspector/></>)
  await screen.findByRole("img",{name:/Clip waveform/})
  const stored=new Map<string,string>()
  const dataTransfer={setData:(type:string,value:string)=>stored.set(type,value),getData:(type:string)=>stored.get(type) ?? "",types:[SELECTION_DRAG],effectAllowed:"copy"}
  fireEvent.dragStart(screen.getByRole("button",{name:"Drag selection to sampler or playlist"}),{dataTransfer})
  expect(stored.has(SELECTION_DRAG)).toBe(true)
  const cursor=useProjectStore.getState().history.cursor
  const target=mounted.container.querySelector('[data-slot="sample-drop"]')!
  fireEvent.drop(target,{dataTransfer})
  await waitFor(()=>expect(useProjectStore.getState().history.cursor).toBe(cursor+1))
  expect(move).toHaveBeenCalledWith(expect.objectContaining({startFrame:0,endFrame:1000,destination:{type:"sampler",channel:channel.id}}))
  expect(useProjectStore.getState().history.cursor).toBe(cursor+1)
  const saved=project().channels[0].source
  expect(reopened().channels[0].source).toEqual(saved)
  await act(async()=>{await undo()});expect(project().channels[0].source).toEqual(channel.source)
  await act(async()=>{await redo()});expect(project().channels[0].source).toEqual(saved)
})

it("refuses a late selection-transfer reply after the real WASM document is replaced",async()=>{
  const id=await addClip();previewAdapter()
  const result=await app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav",{start:1920})
  let resolve!: (value:typeof result)=>void
  const move=vi.spyOn(app.backend,"audioEditorTransfer").mockImplementation(()=>new Promise(done=>{resolve=done}))
  useUiStore.getState().selectChannel(project().channels[0].id)
  const mounted=render(<><AudioEditorButton clip={id} defaultOpen/><ChannelInspector/></>)
  await screen.findByRole("img",{name:/Clip waveform/})
  const stored=new Map<string,string>()
  const dataTransfer={setData:(type:string,value:string)=>stored.set(type,value),getData:(type:string)=>stored.get(type) ?? "",types:[SELECTION_DRAG],effectAllowed:"copy"}
  fireEvent.dragStart(screen.getByRole("button",{name:"Drag selection to sampler or playlist"}),{dataTransfer})
  fireEvent.drop(mounted.container.querySelector('[data-slot="sample-drop"]')!,{dataTransfer})
  await waitFor(()=>expect(move).toHaveBeenCalledOnce())
  await act(async()=>{await app.backend.projectNew()})
  const before=useProjectStore.getState()
  await act(async()=>{resolve(result)})
  expect(useProjectStore.getState()).toBe(before)
  expect(project().playlist.clips).toEqual([])
})
