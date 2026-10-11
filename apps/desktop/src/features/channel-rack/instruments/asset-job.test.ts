import {afterEach,beforeEach,expect,it,vi} from "vitest"
import {prepareAsset} from "./asset-job"
import type {InstrumentAsset} from "@/bindings"
const asset:InstrumentAsset={type:"pcm",sampleRate:48000,samples:[0,8192,0]}
class WorkerStub {
  static instances:WorkerStub[]=[]
  onmessage:((event:MessageEvent<{asset?:InstrumentAsset;error?:string}>)=>void)|null=null
  onerror:(()=>void)|null=null
  terminate=vi.fn()
  postMessage=vi.fn()
  constructor(){WorkerStub.instances.push(this)}
}
beforeEach(()=>{WorkerStub.instances=[];vi.stubGlobal("Worker",WorkerStub);vi.useFakeTimers()})
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals()})
it("transfers source buffers and retires a successful worker",async()=>{
  const bytes=new ArrayBuffer(44),job=new AbortController(),pending=prepareAsset({type:"layer",bytes},job.signal),worker=WorkerStub.instances[0]!
  expect(worker.postMessage).toHaveBeenCalledWith({type:"layer",bytes},[bytes])
  worker.onmessage?.(new MessageEvent("message",{data:{asset}}));expect(await pending).toEqual(asset);expect(worker.terminate).toHaveBeenCalledOnce();job.abort();expect(worker.terminate).toHaveBeenCalledOnce()
})
it("aborts an in-flight preparation and ignores a late result",async()=>{
  const job=new AbortController(),pending=prepareAsset({type:"number",number:0},job.signal),worker=WorkerStub.instances[0]!,rejected=expect(pending).rejects.toThrow("cancelled")
  job.abort();worker.onmessage?.(new MessageEvent("message",{data:{asset}}));await rejected;expect(worker.terminate).toHaveBeenCalledOnce()
})
it("times out and reports worker errors with no retained worker",async()=>{
  const pending=prepareAsset({type:"number",number:0},new AbortController().signal),rejected=expect(pending).rejects.toThrow("timed out")
  await vi.advanceTimersByTimeAsync(30000);await rejected;expect(WorkerStub.instances[0]!.terminate).toHaveBeenCalledOnce()
  const failed=prepareAsset({type:"number",number:0},new AbortController().signal),error=expect(failed).rejects.toThrow("bad image")
  WorkerStub.instances[1]!.onmessage?.(new MessageEvent("message",{data:{error:"bad image"}}));await error;expect(WorkerStub.instances[1]!.terminate).toHaveBeenCalledOnce()
})
it("refuses an already cancelled request without starting a worker",async()=>{
 const job=new AbortController();job.abort();await expect(prepareAsset({type:"number",number:0},job.signal)).rejects.toThrow("cancelled");expect(WorkerStub.instances).toHaveLength(0)
})
