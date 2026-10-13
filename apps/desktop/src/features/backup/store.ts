import { toast } from "sonner"
import { create } from "zustand"
import { persist } from "zustand/middleware"
import { backend, errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { translatedText as t } from "@/lib/i18n"

export type FolderBackupConfig = { folder: string | null; keep: number; afterSave: boolean; head: string | null }
export const EMPTY_BACKUP: FolderBackupConfig = { folder: null, keep: 20, afterSave: false, head: null }
type ConfigStore = { projects: Record<string, FolderBackupConfig> }
export const useBackupConfig = create<ConfigStore>()(persist(() => ({ projects: {} }), { name: "windfall-folder-backups-v1" }))
export const useBackupJob = create<{ busy: boolean; error: string | null; result: string | null }>(() => ({ busy: false, error: null, result: null }))
export function rememberBackup(path: string | null, config: FolderBackupConfig) {
  if (path === null) return
  useBackupConfig.setState((state) => ({ projects: { ...state.projects, [path]: config } }))
}
type SaveTicket = { path: string; generation: number; localGeneration: number; config: FolderBackupConfig }
/** Capture identity before Save. Optional backup failure never prevents Save. */
export async function prepareBackupAfterSave(): Promise<SaveTicket | null> {
  const localGeneration = getProjectGeneration()
  const path = useProjectStore.getState().path
  const config = path ? useBackupConfig.getState().projects[path] : undefined
  if (backend.kind !== "tauri" || !path || !config?.afterSave || !config.folder) return null
  try {
    const { generation } = await backend.loopCatalog()
    if (localGeneration !== getProjectGeneration() || useProjectStore.getState().path !== path) return null
    return { path, generation, localGeneration, config }
  }
  catch (error) { toast.error(t("Could not prepare optional backup"), { description: errorMessage(error) }); return null }
}
export async function backupAfterSave(ticket: SaveTicket | null, saved: string, numberedVersion = false): Promise<void> {
  if (!ticket || (!numberedVersion && saved !== ticket.path) || !ticket.config.folder) return
  const chosen = useBackupConfig.getState().projects[ticket.path]
  if (ticket.localGeneration !== getProjectGeneration() || !chosen?.afterSave || chosen.folder !== ticket.config.folder) return
  if (useBackupJob.getState().busy) {
    const error = t("Project saved. Optional backup skipped because another backup job is running.")
    useBackupJob.setState({ error }); toast.error(error); return
  }
  useBackupJob.setState({ busy: true, error: null, result: null })
  try {
    const row = await backend.backupSave(ticket.config.folder, ticket.config.head, ticket.config.keep, ticket.generation)
    // Do not overwrite settings changed while the job was running.
    const current = useBackupConfig.getState().projects[ticket.path]
    if (current?.folder === ticket.config.folder) {
      rememberBackup(ticket.path, { ...current, head: row.id })
      if (numberedVersion && saved !== ticket.path && !useBackupConfig.getState().projects[saved]) rememberBackup(saved, { ...current, head: row.id })
    }
    useBackupJob.setState({ result: "Backup created after Save." })
  } catch (error) {
    const message = errorMessage(error)
    useBackupJob.setState({ error: message }); toast.error(t("Project saved; optional backup failed"), { description: message })
  } finally { useBackupJob.setState({ busy: false }) }
}
