import type { SamplePlayback } from "@/bindings"
import type { SliceAudio, SliceOptions } from "@/features/slicer/types"
export type DetectionRequest = SliceAudio & { sample: number; bpm: number; options: SliceOptions }
export function detectInWorker(request: DetectionRequest): Promise<SamplePlayback> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" })
    const finish = () => { clearTimeout(timeout); worker.terminate() }
    const timeout = setTimeout(() => { finish(); reject(new Error("Sample detection timed out. Use a shorter loop.")) }, 60_000)
    worker.onmessage = (event: MessageEvent<{ ready?: boolean; asset?: SamplePlayback; error?: string }>) => {
      if (event.data.ready) { worker.postMessage(request); return }
      finish()
      if (event.data.asset) resolve(event.data.asset)
      else reject(new Error(event.data.error ?? "Sample detection failed."))
    }
    worker.onerror = (event) => { finish(); reject(new Error(event.message || "Sample detection worker failed.")) }
  })
}
