import { backend } from "@/lib/ipc"
import { isPanelId, useUiStore, type PanelId } from "@/lib/store/ui"
import { message } from "@/lib/i18n"

const pending = new Map<PanelId, Promise<void>>()

/** Keep the docked panel until its window opens; repeated clicks share the request. */
export function detachPanel(id: string): Promise<void> {
  if (!isPanelId(id)) return Promise.reject(new Error(message("Unknown panel: {id}", { id })))
  if (useUiStore.getState().detachedPanels.includes(id))
    return Promise.resolve()
  const existing = pending.get(id)
  if (existing) return existing
  const request = backend
    .detachPanel(id)
    .then(() => useUiStore.getState().addDetachedPanel(id))
    .finally(() => pending.delete(id))
  pending.set(id, request)
  return request
}
