import type { BackupVersion, DispatchResult, LoopCatalog, RemoteSettingsState } from "@/bindings"

export interface RemoteContentBackend {
  phoneRemoteState(): Promise<RemoteSettingsState>
  phoneRemoteSet(address: string | null): Promise<RemoteSettingsState>
  backupVersions(folder: string): Promise<BackupVersion[]>
  backupSave(folder: string, expected: string | null, keep: number, generation: number): Promise<BackupVersion>
  backupRestore(folder: string, id: string, target: string): Promise<string>
  backupCancel(): Promise<void>
  loopCatalog(): Promise<LoopCatalog>
  loopPreview(id: string): Promise<void>
  loopInsert(id: string, key: number | null, track: number | null, start: number, generation: number, revision: number): Promise<DispatchResult>
}
type NativeCall = <T>(command: string, args?: Record<string, unknown>) => Promise<T>
export function nativeRemoteContent(call: NativeCall): RemoteContentBackend {
  return {
    phoneRemoteState: () => call("phone_remote_state"),
    phoneRemoteSet: (address) => call("phone_remote_set", { address }),
    backupVersions: (folder) => call("backup_versions", { folder }),
    backupSave: (folder, expected, keep, generation) => call("backup_save", { folder, expected, keep, generation }),
    backupRestore: (folder, id, target) => call("backup_restore", { folder, id, target }),
    backupCancel: () => call("backup_cancel"),
    loopCatalog: () => call("loop_catalog"),
    loopPreview: (id) => call("loop_preview", { id }),
    loopInsert: (id, key, track, start, generation, revision) => call("loop_insert", { id, key, track, start, generation, revision }),
  }
}
/** Browser mode must not invent a listener, filesystem or backup success. */
export function unavailableRemoteContent(): RemoteContentBackend {
  const unavailable = async (): Promise<never> => { throw new Error("This feature requires the Windfall desktop app.") }
  return { phoneRemoteState: unavailable, phoneRemoteSet: unavailable, backupVersions: unavailable, backupSave: unavailable, backupRestore: unavailable, backupCancel: unavailable, loopCatalog: unavailable, loopPreview: unavailable, loopInsert: unavailable }
}
