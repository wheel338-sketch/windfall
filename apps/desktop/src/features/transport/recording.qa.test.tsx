import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { SimDocument } from "@/lib/ipc/sim/document"
import { redo, undo, useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { TransportBar } from "./transport-bar"
import { useRecordingStore } from "./recording-store"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => {
  app = await startTestApp()
  useRecordingStore.setState(useRecordingStore.getInitialState(), true)
})
afterEach(() => { app.stop(); vi.restoreAllMocks() })

it("mounts transport Record/start/keep through take attachment with one WASM undo and reopen", async () => {
  // Only the device is simulated; attachment is a real Rust document edit.
  let active = false
  vi.spyOn(app.backend, "recordingInputs").mockResolvedValue([{ host: "Synthetic", device: "Input", channels: 2, sampleRates: [48000] }])
  const state = () => ({ active, frames: active ? 24000 : 0, sampleRate: 48000, startTick: 960, error: null })
  vi.spyOn(app.backend, "recordingState").mockImplementation(async () => state())
  const start = vi.spyOn(app.backend, "recordingStart").mockImplementation(async () => { active = true; return state() })
  const stop = vi.spyOn(app.backend, "recordingStop").mockImplementation(async () => {
    active = false
    return app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav", { start: 960 })
  })
  const before = structuredClone(useProjectStore.getState().project)
  const cursor = useProjectStore.getState().history.cursor
  render(<TransportBar />)
  await userEvent.click(screen.getByRole("button", { name: "Record" }))
  useRecordingStore.setState({ start: 960 })
  await userEvent.click(await screen.findByRole("button", { name: "Start recording" }))
  expect(start).toHaveBeenCalledOnce()
  expect(screen.getByText(/Recording 0.5 seconds/)).toBeVisible()
  await userEvent.click(screen.getByRole("button", { name: "Stop and keep" }))
  await waitFor(() => expect(stop).toHaveBeenCalledOnce())
  await waitFor(() => expect(useProjectStore.getState().history.cursor).toBe(cursor + 1))
  const after = structuredClone(useProjectStore.getState().project)
  expect(after.playlist.clips).toHaveLength(before.playlist.clips.length + 1)
  expect(after.playlist.clips.find((clip) => !before.playlist.clips.some((old) => old.id === clip.id))?.start).toBe(960)
  const snapshot = (await app.backend.documentSnapshot()).project
  expect(snapshot).toEqual({ ...after, nextId: snapshot.nextId })
  const doc = SimDocument.create(snapshot)
  const loaded = SimDocument.open(doc.fileText())
  expect(loaded.project()).toEqual(snapshot)
  loaded.dispose(); doc.dispose()
  await act(async () => { await undo() })
  expect(useProjectStore.getState().project.playlist).toEqual(before.playlist)
  await act(async () => { await redo() })
  expect(useProjectStore.getState().project).toEqual(after)
})

it("refuses a late Stop and keep reply after WASM project replacement",async()=>{
  const result=await app.backend.addAudioClipFromFile("/factory/Loops/Drum loop 128.wav",{start:960})
  await app.backend.projectNew()
  let active=false
  vi.spyOn(app.backend,"recordingInputs").mockResolvedValue([{host:"Synthetic",device:"Input",channels:2,sampleRates:[48000]}])
  const state=()=>({active,frames:active ? 24000 : 0,sampleRate:48000,startTick:960,error:null})
  vi.spyOn(app.backend,"recordingState").mockImplementation(async()=>state())
  vi.spyOn(app.backend,"recordingStart").mockImplementation(async()=>{active=true;return state()})
  let resolve!: (value:typeof result)=>void
  const keep=vi.spyOn(app.backend,"recordingStop").mockImplementation(()=>new Promise(done=>{active=false;resolve=done}))
  render(<TransportBar/>)
  await userEvent.click(screen.getByRole("button",{name:"Record"}))
  await userEvent.click(await screen.findByRole("button",{name:"Start recording"}))
  await userEvent.click(screen.getByRole("button",{name:"Stop and keep"}))
  await waitFor(()=>expect(keep).toHaveBeenCalledOnce())
  await act(async()=>{await app.backend.projectNew()})
  const before=useProjectStore.getState()
  await act(async()=>{resolve(result)})
  await waitFor(()=>expect(useRecordingStore.getState().busy).toBe(false))
  expect(useRecordingStore.getState().error).toMatch(/project changed/)
  expect(useProjectStore.getState()).toBe(before)
  expect(before.project.playlist.clips).toEqual([])
})
