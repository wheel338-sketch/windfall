import { Channel } from "@tauri-apps/api/core"
import { backend, errorMessage } from "@/lib/ipc"

export type UpdateChannel = "stable" | "preview"
export type UpdateInfo = { token: number; version: string; notes: string; channel: UpdateChannel; projectFormat: number }
export type UpdateProgress = { downloaded: number; total: number | null; verified: boolean }
export type UpdateCheck = { configured: boolean; reason: string | null; update: UpdateInfo | null }
export type UpdateApi = {
  check(channel: UpdateChannel): Promise<UpdateCheck>
  download(token: number, progress: (progress: UpdateProgress) => void): Promise<void>
  begin(token: number): Promise<{ dirty: boolean; path: string | null }>
  cancel(token: number): Promise<void>
  install(token: number, decision?: "save" | "discard", path?: string): Promise<void>
}
async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (backend.kind !== "tauri") throw new Error("Updates require the installed desktop application.")
  try { return await backend.updateCall<T>(command, args) }
  catch (error) { throw new Error(errorMessage(error), { cause: error }) }
}
export const nativeUpdateApi: UpdateApi = {
  check: (channel) => call("update_check", { channel }),
  download: (token, receive) => {
    const progress = new Channel<UpdateProgress>()
    progress.onmessage = receive
    return call("update_download", { token, progress })
  },
  begin: (token) => call("update_install_begin", { token }),
  cancel: (token) => call("update_install_cancel", { token }),
  install: (token, decision, path) => call("update_install", { token, decision, path }),
}
