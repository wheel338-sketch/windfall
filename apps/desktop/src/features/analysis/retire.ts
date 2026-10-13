import { errorMessage } from "@/lib/ipc"
import {
  isTerminalStatus,
  type AnalysisBackend,
  type AnalysisJob,
} from "./types"
import { translatedText as t, message } from "@/lib/i18n"
/** Bounded control retirement; refusal retains native quota/evidence and finals.
 * There is no static job map or speculative filesystem deletion in the UI.
 */
export async function retireJob(
  api: AnalysisBackend,
  job: string,
  observe?: (job: AnalysisJob) => void
): Promise<void> {
  try {
    for (let attempt = 0; attempt < 600; attempt += 1) {
      const current = await api.analysisStatus(job)
      observe?.(current)
      if (isTerminalStatus(current.status)) {
        await api.analysisForget(job)
        return
      }
      await api.analysisCancel(job).catch(() => {}) // an apply claim can still own it
      const cancelled = await api.analysisStatus(job)
      observe?.(cancelled)
      if (isTerminalStatus(cancelled.status)) {
        await api.analysisForget(job)
        return
      }
      await new Promise<void>((resolve) => setTimeout(resolve, 200))
    }
    throw new Error(
      t("Retirement deadline reached; use native status and cleanup retry. No files were deleted by this panel.")
    )
  } catch (error) {
    throw new Error(
      message("Analysis job {job} remains retained: {error}", { job, error: errorMessage(error) }),
      { cause: error }
    )
  }
}
