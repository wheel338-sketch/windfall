import { prepareInstrumentAsset, type AssetRequest } from "./asset-preparation"
self.onmessage = (event: MessageEvent<AssetRequest>) => {
  try { self.postMessage({ asset: prepareInstrumentAsset(event.data) }) }
  catch (error) { self.postMessage({ error: error instanceof Error ? error.message : String(error) }) }
}
