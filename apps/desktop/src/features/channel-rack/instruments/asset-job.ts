import type { InstrumentAsset } from "@/bindings"
import type { AssetRequest } from "./asset-preparation"
import { translatedText as t } from "@/lib/i18n"

export function prepareAsset(request: AssetRequest, signal: AbortSignal): Promise<InstrumentAsset> {
  return new Promise((resolve,reject) => {
    if (signal.aborted) { reject(new Error(t("Instrument preparation cancelled"))); return }
    const worker = new Worker(new URL("./asset-worker.ts", import.meta.url), { type: "module" })
    let settled = false
    const timer = setTimeout(() => fail(new Error(t("Instrument preparation timed out"))), 30000)
    function clean() { clearTimeout(timer); signal.removeEventListener("abort",abort); worker.terminate() }
    function fail(error: Error) { if (settled) return; settled = true; clean(); reject(error) }
    function abort() { fail(new Error(t("Instrument preparation cancelled"))) }
    signal.addEventListener("abort",abort,{ once: true })
    worker.onerror = () => fail(new Error(t("Instrument worker failed")))
    worker.onmessage = (event: MessageEvent<{ asset?: InstrumentAsset; error?: string }>) => {
      if (event.data.error) fail(new Error(event.data.error))
      else if (event.data.asset) { if (settled) return; settled = true; clean(); resolve(event.data.asset) }
      else fail(new Error(t("Instrument worker returned no data")))
    }
    try { worker.postMessage(request, "bytes" in request ? [request.bytes] : request.type === "imageSpectrum" ? [request.pixels] : []) }
    catch (error) { fail(new Error(t("Could not send data to the instrument worker"), { cause: error })) }
  })
}
