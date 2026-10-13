import { create } from "zustand"
import { toast } from "sonner"
import { backend, errorMessage } from "@/lib/ipc"
import { retireJob } from "./retire"
import { isTerminalStatus, type AnalysisJob } from "./types"
import { message as i18nMessage, translatedText as t } from "@/lib/i18n"

const LIMIT = 8 // matches the native retained-record cap; IDs never persist
type Retained = {
  job: AnalysisJob
  dismissed: boolean
  busy: boolean
  error: string | null
}
export const useAnalysisRecovery = create(() => ({
  jobs: [] as Retained[],
  pending: 0,
  selected: null as string | null,
}))
export function hasRecoveryCapacity(): boolean {
  const state = useAnalysisRecovery.getState()
  return state.pending + state.jobs.length < LIMIT
}
/** Reserve before IPC, including submits that finish after the panel unmounts. */
export function reserveRecovery(): (job: AnalysisJob | null) => void {
  if (!hasRecoveryCapacity())
    throw new Error(
      t("Retire retained analysis jobs before submitting another (limit 8).")
    )
  useAnalysisRecovery.setState((state) => ({ pending: state.pending + 1 }))
  let completed = false
  return (job) => {
    if (completed) return
    completed = true
    useAnalysisRecovery.setState((state) => ({
      pending: state.pending - 1,
      jobs: job
        ? [...state.jobs, { job, dismissed: false, busy: false, error: null }]
        : state.jobs,
    }))
  }
}
export function updateRecoveryJob(job: AnalysisJob) {
  useAnalysisRecovery.setState((state) => ({
    jobs: state.jobs.map((item) =>
      item.job.job === job.job ? { ...item, job } : item
    ),
  }))
}
export function removeRecoveryJob(id: string) {
  useAnalysisRecovery.setState((state) => {
    const jobs = state.jobs.filter((item) => item.job.job !== id)
    return {
      jobs,
      selected:
        state.selected === id
          ? (jobs.find((item) => item.dismissed)?.job.job ?? null)
          : state.selected,
    }
  })
}
function change(id: string, patch: Partial<Omit<Retained, "job">>) {
  useAnalysisRecovery.setState((state) => ({
    jobs: state.jobs.map((item) =>
      item.job.job === id ? { ...item, ...patch } : item
    ),
  }))
}
export function recoveryFailure(id: string, error: unknown) {
  const message = errorMessage(error)
  const identity = i18nMessage("Analysis job {id} remains retained:", { id })
  // Bound retained UI evidence; the native record retains full cleanup evidence.
  const report = (
    message.startsWith(identity) ? message : i18nMessage("Analysis job {id} remains retained: {detail}", { id, detail: message })
  ).slice(0, 2048)
  change(id, { busy: false, error: report })
  return report
}
export async function retireTrackedJob(id: string): Promise<void> {
  const item = useAnalysisRecovery
    .getState()
    .jobs.find((item) => item.job.job === id)
  if (!item || item.busy) return
  change(id, { dismissed: true, busy: true })
  useAnalysisRecovery.setState({ selected: id })
  try {
    await retireJob(backend, id, updateRecoveryJob)
    removeRecoveryJob(id) // only after actual native forget
  } catch (error) {
    toast.error(recoveryFailure(id, error))
  }
}
export function selectedRecoveryJob() {
  const state = useAnalysisRecovery.getState()
  return state.jobs.find(
    (item) => item.dismissed && item.job.job === state.selected
  )
}
export function selectNextRecoveryJob() {
  const state = useAnalysisRecovery.getState()
  const jobs = state.jobs.filter((item) => item.dismissed)
  if (jobs.length === 0) return
  const next =
    jobs[
      (jobs.findIndex((item) => item.job.job === state.selected) + 1) %
        jobs.length
    ]
  useAnalysisRecovery.setState({ selected: next.job.job })
  toast(
    i18nMessage("Retained analysis job {job}: {error}", { job: next.job.job, error: next.error ?? next.job.status })
  )
}
/** Cleanup commands have no source/review authority and never apply a job. */
export async function recoverJob(operation: "cancel" | "retry" | "forget") {
  const item = selectedRecoveryJob()
  if (!item || item.busy) return
  const id = item.job.job
  change(id, { busy: true, error: null })
  try {
    if (operation === "cancel")
      updateRecoveryJob(await backend.analysisCancel(id))
    if (operation === "retry") {
      await backend.analysisRetryCleanup(id)
      updateRecoveryJob(await backend.analysisStatus(id))
    }
    if (operation === "forget") {
      const job = await backend.analysisStatus(id)
      updateRecoveryJob(job)
      if (!isTerminalStatus(job.status))
        throw new Error(t("Cancel the job before forgetting it"))
      await backend.analysisForget(id)
      removeRecoveryJob(id)
    }
    change(id, { busy: false })
  } catch (error) {
    throw new Error(recoveryFailure(id, error), { cause: error })
  }
}
