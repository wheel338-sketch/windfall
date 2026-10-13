import type { AnalysisBackend } from "./types"
import { translatedText as t } from "@/lib/i18n"

export const BROWSER_REASON =
  "Native analysis requires the desktop app. No inference runs in the browser."
const unavailable = () => Promise.reject(new Error(t(BROWSER_REASON)))
/** Honest mock: no weights, workers, fabricated review or successful apply. */
export function unavailableAnalysis(): AnalysisBackend {
  return {
    analysisRuntimeSettings: () => Promise.resolve({ libraryPath: null, modelId: null, modelVersion: null }),
    analysisRuntimeConfigure: unavailable,
    pickAnalysisRuntime: () => Promise.resolve(null),
    analysisCapability: () =>
      Promise.resolve({
        native: false,
        available: false,
        reason: t(BROWSER_REASON),
        models: [],
      }),
    analysisModelImport: unavailable,
    pickAnalysisModel: () => Promise.resolve(null),
    analysisSubmit: unavailable,
    analysisStatus: unavailable,
    analysisCancel: unavailable,
    analysisCancelPreparation: () => Promise.resolve(),
    analysisForget: unavailable,
    analysisRetryCleanup: unavailable,
    analysisReview: unavailable,
    analysisPreview: unavailable,
    analysisApply: unavailable,
    analysisShutdown: () => Promise.resolve(),
  }
}
