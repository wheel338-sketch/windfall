import { create } from "zustand"
import type { ExportRequestProgress } from "@/lib/ipc/backend"
import { getProjectGeneration } from "@/lib/store/replaced"

type Request = {
  id: string
  owner: string | null
  generation: number
  path: string
  status: "pending" | "succeeded" | "cancelled" | "failed"
  error: string | null
}
// Window-local intent plus the exact native request. Repeated exports to the
// same path and broadcasts from another window can never complete this one.
export const useExportRequestStore = create(() => ({
  intent: null as string | null,
  request: null as Request | null,
}))
export function setExportIntent(owner: string | null) {
  useExportRequestStore.setState({ intent: owner })
}
export function releaseExportIntent(owner: string) {
  if (useExportRequestStore.getState().intent === owner) setExportIntent(null)
}
export function beginExportRequest(path: string, owner = useExportRequestStore.getState().intent): string {
  const id = crypto.randomUUID()
  useExportRequestStore.setState({ intent: null, request: {
    id, path, owner, generation: getProjectGeneration(),
    status: "pending", error: null,
  } })
  return id
}
export function receiveExportRequest(progress: ExportRequestProgress) {
  const request = useExportRequestStore.getState().request
  if (!request || request.status !== "pending" || request.id !== progress.request ||
      request.path !== progress.path || request.generation !== getProjectGeneration()) return
  if (progress.error !== null) failExportRequest(request.id, progress.error)
  else if (progress.done) useExportRequestStore.setState({ request: {
    ...request, status: progress.cancelled ? "cancelled" : "succeeded",
  } })
}
export function failExportRequest(id: string, error: string) {
  const request = useExportRequestStore.getState().request
  if (request?.id === id && request.status === "pending")
    useExportRequestStore.setState({ request: { ...request, status: "failed", error } })
}
