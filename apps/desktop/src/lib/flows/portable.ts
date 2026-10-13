import { create } from "zustand"
import { toast } from "sonner"
import { useRecordingStore } from "@/features/transport/recording-store"
import { backupAfterSave, prepareBackupAfterSave } from "@/features/backup/store"
import { backend, errorMessage } from "@/lib/ipc"
import { registry } from "@/lib/actions/registry"
import { reportError } from "@/lib/errors"
import { useProjectStore, refetchSnapshot } from "@/lib/store/project"
import { fileName } from "@/lib/time"
import { projectName, refreshRecentProjects } from "./project"
import { translatedText as t } from "@/lib/i18n"

export const useArchiveStore = create<{
  busy: boolean
  cancellable: boolean
  report: string | null
}>(() => ({
  busy: false,
  cancellable: false,
  report: null,
}))
export function archiveReport(message: string | null) {
  if (message === "Project archive cancelled.") return
  useArchiveStore.setState({ report: message })
}
export function archiveBusy(value: boolean, cancellable = value) {
  useArchiveStore.setState({ busy: value, cancellable })
  registry.invalidate()
}
export function canArchive(): boolean {
  return (
    backend.kind === "tauri" &&
    !useArchiveStore.getState().busy &&
    !useRecordingStore.getState().state.active
  )
}
export function portableUnavailable(): string {
  if (backend.kind !== "tauri") return t("Desktop app required")
  if (useRecordingStore.getState().state.active)
    return t("Stop or cancel recording first")
  return t("Project archive operation in progress")
}
export async function saveNewVersion(): Promise<boolean> {
  if (!canArchive()) {
    toast.error(portableUnavailable())
    return false
  }
  try {
    const base =
      useProjectStore.getState().path === null
        ? await backend.pickProjectSavePath(projectName())
        : undefined
    if (base === null) return false
    const backup = await prepareBackupAfterSave()
    const saved = await backend.projectSaveNewVersion(base)
    // Rust publishes the dirty flag. A concurrent edit remains dirty.
    await refetchSnapshot()
    toast.success(t("Saved new version"), { description: fileName(saved) })
    void refreshRecentProjects()
    void backupAfterSave(backup, saved, true)
    return true
  } catch (error) {
    reportError(error, t("Could not save a new version"))
    return false
  }
}
export async function saveArchive(): Promise<void> {
  if (!canArchive()) {
    toast.error(portableUnavailable())
    return
  }
  archiveBusy(true, false)
  archiveReport(null)
  try {
    const path = await backend.pickProjectArchivePath(projectName())
    if (path === null) return
    archiveBusy(true)
    const saved = await backend.projectArchiveSave(path)
    toast.success(t("Portable project archive created"), {
      description: fileName(saved),
    })
  } catch (error) {
    reportError(error, t("Could not create a portable project archive"))
    archiveReport(errorMessage(error))
  } finally {
    archiveBusy(false)
  }
}
export async function cancelArchive(): Promise<void> {
  try {
    await backend.projectArchiveCancel()
  } catch (error) {
    reportError(error, t("Could not cancel the project archive"))
  }
}
