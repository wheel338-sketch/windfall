import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { ValueContextMenus } from "@/components/value-context-menu"
import { dispatch, undo, useProjectStore } from "@/lib/store"
import { startTestApp, settle } from "@/test/harness"
import { EffectEditor } from "./effect-editor"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async()=>{app=await startTestApp()})
afterEach(()=>{app.stop();vi.restoreAllMocks()})
function Panel({track,effect}:{track:number;effect:number}){
  const slot=useProjectStore(state=>state.project.mixer.tracks.find(item=>item.id===track)?.effects.find(slot=>slot.id===effect))
  return slot ? <EffectEditor trackId={track} slot={slot} /> : null
}
it("opens the registered lighting editor, reports a missing service and preserves history",async()=>{
  app.backend.musicLightingStatus=vi.fn().mockResolvedValue({connected:false,message:"Chroma REST service unavailable: enable local service"})
  const track=useProjectStore.getState().project.mixer.tracks[0].id
  const result=await dispatch({type:"addEffect",track,kind:"musicLighting"})
  if(!result)throw new Error("effect creation failed")
  const effect=result.created[0]
  render(<ValueContextMenus><Panel track={track} effect={effect} /></ValueContextMenus>);await act(()=>settle())
  expect(screen.getByRole("alert")).toHaveTextContent("Chroma REST service unavailable")
  await dispatch({type:"setEffectParam",track,effect,param:1,value:2});await act(()=>settle())
  expect(useProjectStore.getState().project.mixer.tracks[0].effects.find(slot=>slot.id===effect)?.params).toMatchObject({type:"musicLighting",device:2})
  await undo();await act(()=>settle())
  expect(useProjectStore.getState().project.mixer.tracks[0].effects.find(slot=>slot.id===effect)?.params).toMatchObject({type:"musicLighting",device:0})
  app.backend.musicLightingStatus=vi.fn().mockResolvedValue({connected:true,message:"Connected to local Chroma REST service"})
  fireEvent.click(screen.getByRole("button",{name:"Refresh lighting service"}));await act(()=>settle())
  expect(screen.getByRole("alert")).toHaveTextContent("Connected to local Chroma REST service")
})
