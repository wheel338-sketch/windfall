import { describe, expect, it, vi } from "vitest"
const fixture = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listeners: new Map<string, (message: { payload: unknown }) => void>(),
}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: fixture.invoke, Channel: class {} }))
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: (message: { payload: unknown }) => void) => {
  fixture.listeners.set(name, handler)
  return () => fixture.listeners.delete(name)
}) }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))
import { createTauriBackend } from "./tauri"

describe("native window document identity", () => {
  it("sends the server generation with edits and history, then uses the replacement envelope", async () => {
    fixture.invoke.mockReset()
    const backend = createTauriBackend()
    await expect(backend.dispatch({ type: "addPattern" })).rejects.toThrow("still loading")
    const snapshot = { revision: 0, project: {}, history: { entries: [], cursor: 0 }, dirty: false, path: null }
    fixture.invoke.mockResolvedValueOnce({ generation: 19, caller: { generation: 19, token: "first-window" }, snapshot }).mockResolvedValue(undefined)
    expect(await backend.documentSnapshot()).toBe(snapshot)
    await backend.dispatch({ type: "addPattern" })
    expect(fixture.invoke).toHaveBeenLastCalledWith("dispatch", { command: { type: "addPattern" }, gesture: undefined, generation: 19, caller: { generation: 19, token: "first-window" } })
    const loaded = vi.fn()
    const off = backend.onProjectLoaded(loaded)
    await Promise.resolve()
    fixture.listeners.get("project:loaded")?.({ payload: { generation: 20, snapshot } })
    expect(loaded).toHaveBeenCalledWith(snapshot)
    await backend.undo()
    expect(fixture.invoke).toHaveBeenLastCalledWith("undo", { generation: 20, caller: { generation: 20, token: "first-window" } })
    await backend.redo()
    expect(fixture.invoke).toHaveBeenLastCalledWith("redo", { generation: 20, caller: { generation: 20, token: "first-window" } })
    await backend.historyJump(1)
    expect(fixture.invoke).toHaveBeenLastCalledWith("history_jump", { cursor: 1, generation: 20, caller: { generation: 20, token: "first-window" } })
    off()
    await Promise.resolve()
    expect(fixture.listeners.has("project:loaded")).toBe(false)
  })
  it("an old startup snapshot cannot overwrite the generation of a loaded replacement", async () => {
    fixture.invoke.mockReset()
    const backend = createTauriBackend()
    const off = backend.onProjectLoaded(vi.fn())
    await Promise.resolve()
    let finish: (value: unknown) => void = () => {}
    fixture.invoke.mockReturnValueOnce(new Promise((resolve) => { finish = resolve })).mockResolvedValue(undefined)
    const pending = backend.documentSnapshot()
    fixture.listeners.get("project:loaded")?.({ payload: { generation: 22, snapshot: {} } })
    finish({ generation: 21, caller: { generation: 21, token: "same-window" }, snapshot: {} })
    await expect(pending).rejects.toThrow("replaced")
    await backend.dispatch({ type: "addPattern" })
    expect(fixture.invoke.mock.calls.at(-1)?.[1]?.generation).toBe(22)
    off()
  })
  it("includes immutable caller identity on custom mutations and never retries stale refusals", async () => {
    fixture.invoke.mockReset()
    fixture.invoke.mockResolvedValueOnce({ generation: 4, caller: { generation: 4, token: "detached-open-1" }, snapshot: {} }).mockResolvedValue(undefined)
    const backend = createTauriBackend()
    await backend.documentSnapshot()
    const mutations = [
      () => backend.pluginsAdd("fixture.clap", "gain"),
      () => backend.addChannelFromFile("take.wav"),
      () => backend.automate({ type: "tempo" }),
      () => backend.prepareClipCommand({ type: "addPattern" }),
      () => backend.dockPanel("mixer"),
    ]
    for (const mutate of mutations) {
      await mutate()
      expect(fixture.invoke.mock.calls.at(-1)?.[1]?.caller).toEqual({ generation: 4, token: "detached-open-1" })
    }
    fixture.invoke.mockRejectedValueOnce("This window was closed or reopened")
    const before = fixture.invoke.mock.calls.length
    await expect(backend.pluginsAdd("fixture.clap", "gain")).rejects.toThrow("closed or reopened")
    expect(fixture.invoke.mock.calls.length).toBe(before + 1)
  })
})
