import { sim } from "@/lib/ipc/sim/wasm"
import type { SamplePlayback } from "@/bindings"
import type { DetectionRequest } from "./analyze"
self.onmessage = (event: MessageEvent<DetectionRequest>) => {
  try { self.postMessage({ asset: sim.call<SamplePlayback>("sample_playback_detect", 0, event.data) }) }
  catch (error) { self.postMessage({ error: error instanceof Error ? error.message : String(error) }) }
}
self.postMessage({ ready: true })
