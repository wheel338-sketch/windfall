import { describe, expect, it, vi } from "vitest"
const fixture = vi.hoisted(() => ({ invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<void>>().mockResolvedValue(undefined), channels: [] as { onmessage?: (frame: unknown) => void }[] }))
vi.mock("@tauri-apps/api/core", () => ({
  invoke: fixture.invoke,
  Channel: class { onmessage?: (frame: unknown) => void; constructor() { fixture.channels.push(this) } },
}))
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }))
import { createTauriBackend } from "./tauri"

describe("native realtime subscription ownership", () => {
  it("shares one feed, releases on last unmount, ignores stale frames and renews the token", () => {
    const backend = createTauriBackend()
    const first = vi.fn(); const second = vi.fn()
    const offFirst = backend.subscribeRealtime(first)
    const offSecond = backend.subscribeRealtime(second)
    expect(fixture.invoke.mock.calls.filter(([command]) => command === "realtime_subscribe")).toHaveLength(1)
    const oldToken = (fixture.invoke.mock.calls[0][1] as { token: string }).token
    offFirst()
    expect(fixture.invoke).not.toHaveBeenCalledWith("realtime_unsubscribe", expect.anything())
    offSecond()
    expect(fixture.invoke).toHaveBeenCalledWith("realtime_unsubscribe", { token: oldToken })
    const next = vi.fn()
    const offNext = backend.subscribeRealtime(next)
    fixture.channels[0].onmessage?.({ tick: 999 })
    expect(next).not.toHaveBeenCalled()
    fixture.channels[1].onmessage?.({ tick: 1 })
    expect(next).toHaveBeenCalledWith({ tick: 1 })
    const subscribe = fixture.invoke.mock.calls.filter(([command]) => command === "realtime_subscribe")
    expect((subscribe[1][1] as { token: string }).token).not.toBe(oldToken)
    offNext()
  })
})
