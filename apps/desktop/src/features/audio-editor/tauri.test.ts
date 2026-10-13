import { describe, expect, it, vi } from "vitest"
import { invoke } from "@tauri-apps/api/core"
import { createTauriBackend } from "@/lib/ipc/tauri"
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: vi.fn() }))
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))
describe("native audio editor IPC", () => {
  it("passes clip, token and exact selection; exposes native refusal", async () => {
    const backend = createTauriBackend()
    const caller = { generation: 1, token: "editor-fixture-window" }
    vi.mocked(invoke).mockResolvedValue(null).mockResolvedValueOnce({ generation: 1, caller })
    await backend.audioEditorOpen(42)
    expect(invoke).toHaveBeenNthCalledWith(1, "window_document_snapshot", undefined)
    expect(invoke).toHaveBeenLastCalledWith("audio_editor_open", { clip: 42, caller })
    const request = {
      token: 7,
      operation: "fadeOut" as const,
      startFrame: 1,
      endFrame: 3,
    }
    await backend.audioEditorApply(request)
    expect(invoke).toHaveBeenLastCalledWith("audio_editor_apply", { request, caller })
    await backend.audioEditorDiscard(7)
    expect(invoke).toHaveBeenLastCalledWith("audio_editor_discard", {
      token: 7,
      caller,
    })
    vi.mocked(invoke).mockRejectedValue("stop or cancel recording first")
    await expect(backend.audioEditorOpen(42)).rejects.toThrow(
      "Stop or cancel recording first"
    )
  })
})
