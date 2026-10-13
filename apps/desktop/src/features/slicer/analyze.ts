import type { SliceAnalysis, SliceAnalysisRequest } from "./types"
import { translatedText as t } from "@/lib/i18n"

/** A separate WASM instance keeps heavy analysis away from the UI/document. */
export function analyzeInWorker(
  request: SliceAnalysisRequest
): Promise<SliceAnalysis> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./worker.ts", import.meta.url), {
      type: "module",
    })
    const finish = () => {
      clearTimeout(timeout)
      worker.terminate()
    }
    const timeout = setTimeout(() => {
      finish()
      reject(
        new Error(t("Slice analysis timed out. Trim the clip and try again."))
      )
    }, 60_000)
    worker.onmessage = (
      event: MessageEvent<{
        ready?: boolean
        analysis?: SliceAnalysis
        error?: string
      }>
    ) => {
      if (event.data.ready) {
        worker.postMessage(request)
        return
      }
      finish()
      if (event.data.analysis) resolve(event.data.analysis)
      else reject(new Error(event.data.error ?? t("Slice analysis failed.")))
    }
    worker.onerror = (event) => {
      finish()
      reject(new Error(event.message || t("Slice analysis worker failed.")))
    }
  })
}
