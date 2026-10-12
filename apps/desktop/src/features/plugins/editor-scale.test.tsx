import { useEffect } from "react"
import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import { SettingsDialog } from "@/features/settings/settings-dialog"
import { useApplicationScale } from "@/lib/ui-scale-root"
import { applyUiScale } from "@/lib/ui-scale"
import { useUiStore } from "@/lib/store/ui"
import { useProjectStore } from "@/lib/store/project"
import { settle, startTestApp } from "@/test/harness"
import { connectNativeEditorScale } from "./editor-scale"

afterEach(() => { applyUiScale(100); vi.restoreAllMocks() })
function MountedSettings({ sync }: { sync: (application: number) => Promise<void> }) {
  useApplicationScale()
  useEffect(() => connectNativeEditorScale(sync), [sync])
  return <SettingsDialog />
}

describe("native content scaling boundary", () => {
  it("refreshes main or detached webview density on native DPI notification and releases its listener", async () => {
    const app = await startTestApp()
    let notify: (() => void) | undefined
    const released = vi.fn()
    app.backend.onWindowScale = (handler) => { notify = handler; return released }
    const changed = vi.fn()
    window.addEventListener("windfall:ui-scale", changed)
    const before = useProjectStore.getState()
    const sync = vi.fn<(application: number) => Promise<void>>().mockResolvedValue(undefined)
    try {
      const mounted = render(<MountedSettings sync={sync} />)
      expect(notify).toBeTypeOf("function")
      const events = changed.mock.calls.length
      act(() => notify?.())
      expect(changed.mock.calls.length).toBe(events + 1)
      expect(useProjectStore.getState()).toBe(before)
      expect(document.documentElement.style.getPropertyValue("--wf-viewport-width")).toBe(`${window.innerWidth / (useUiStore.getState().uiScale / 100)}px`)
      mounted.unmount()
      expect(released).toHaveBeenCalledOnce()
    } finally { window.removeEventListener("windfall:ui-scale", changed); app.stop() }
  })
  it("the real Settings selection sends application scale without changing project or audio settings", async () => {
    const app = await startTestApp()
    const sync = vi.fn<(application: number) => Promise<void>>().mockResolvedValue(undefined)
    try {
      const before = useProjectStore.getState()
      const dispatch = vi.spyOn(app.backend, "dispatch")
      const configure = vi.spyOn(app.backend, "engineConfigure")
      useUiStore.getState().openDialog("settings")
      const mounted = render(<MountedSettings sync={sync} />)
      await waitFor(() => expect(sync).toHaveBeenCalledWith(1))
      const user = userEvent.setup()
      await user.click(screen.getByRole("combobox", { name: "Interface scale" }))
      await user.click(await screen.findByRole("option", { name: "150%" }))
      await waitFor(() => expect(sync).toHaveBeenLastCalledWith(1.5))
      expect(useProjectStore.getState()).toBe(before)
      expect(dispatch).not.toHaveBeenCalled()
      expect(configure).not.toHaveBeenCalled()
      expect(JSON.parse(localStorage.getItem("windfall.ui")!).state.uiScale).toBe(150)
      mounted.unmount()
      const calls = sync.mock.calls.length
      applyUiScale(125)
      await settle()
      expect(sync).toHaveBeenCalledTimes(calls)
    } finally { app.stop() }
  })
  it("coalesces changes behind a slow owner call and reports refusal without breaking later changes", async () => {
    applyUiScale(100)
    let finish = () => {}
    const slow = new Promise<void>((resolve) => { finish = resolve })
    const sync = vi.fn<(application: number) => Promise<void>>()
      .mockReturnValueOnce(slow)
      .mockRejectedValueOnce(new Error("This plugin refuses scale"))
      .mockResolvedValue(undefined)
    const fail = vi.fn()
    const stop = connectNativeEditorScale(sync, fail)
    try {
      applyUiScale(125); applyUiScale(200)
      expect(sync.mock.calls).toEqual([[1]])
      await act(async () => { finish(); await settle() })
      expect(sync.mock.calls).toEqual([[1], [2]])
      expect(fail).toHaveBeenCalledWith(expect.objectContaining({ message: "This plugin refuses scale" }))
      applyUiScale(150)
      await settle()
      expect(sync).toHaveBeenLastCalledWith(1.5)
      stop()
      applyUiScale(75)
      await settle()
      expect(sync).toHaveBeenCalledTimes(3)
    } finally { stop() }
  })
})
