import { describe, expect, it, vi } from "vitest"
import { invoke } from "@tauri-apps/api/core"
import { createTauriBackend } from "@/lib/ipc/tauri"
import type { AudioLoggerDumpRequest, AudioLoggerSettings } from "./types"
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))
describe("native audio logger IPC", () => {
  it("passes settings and dump requests under the names the commands take", async () => {
    const backend = createTauriBackend()
    const caller = { generation: 1, token: "logger-fixture-window" }
    vi.mocked(invoke).mockResolvedValue(null).mockResolvedValueOnce({ generation: 1, caller })
    await backend.audioLoggerState()
    expect(invoke).toHaveBeenNthCalledWith(1, "window_document_snapshot", undefined)
    expect(invoke).toHaveBeenLastCalledWith("audio_logger_state", { caller })
    const settings: AudioLoggerSettings = {
      enabled: true,
      seconds: 300,
      master: true,
      tracks: [3],
      input: { host: "WASAPI", device: "Line in", left: 0, right: 1 },
    }
    await backend.audioLoggerConfigure(settings)
    expect(invoke).toHaveBeenLastCalledWith("audio_logger_configure", {
      settings,
      caller,
    })
    const request: AudioLoggerDumpRequest = {
      source: { type: "track", track: 3 },
      seconds: 30,
      start: null,
      track: null,
    }
    await backend.audioLoggerDump(request)
    expect(invoke).toHaveBeenLastCalledWith("audio_logger_dump", { request, caller })
  })
})
