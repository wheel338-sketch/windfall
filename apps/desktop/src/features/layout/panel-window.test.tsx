import { act, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { runAction } from "@/lib/actions"
import { useUiStore, activeScopeOf } from "@/lib/store/ui"
import { useRecordingStore, openRecording } from "@/features/transport/recording-store"
import { startTestApp } from "@/test/harness"
import { closeGettingStarted } from "@/features/help/getting-started-store"
import { PanelWindow } from "./app-shell"

// The harness already installs the real document/actions/keymap. This case
// exercises composition rather than double-installing shell boot listeners.
vi.mock("./use-app-boot", () => ({ useAppBoot: vi.fn(), useWindowTitle: vi.fn() }))
vi.mock("@/features/mixer", () => ({ default: () => <p>Window mixer</p> }))
let stop: () => void
beforeEach(async () => {
  const app = await startTestApp()
  stop = app.stop
  useRecordingStore.setState(useRecordingStore.getInitialState(), true)
})
afterEach(() => { closeGettingStarted(); useRecordingStore.setState({ open: false }); stop() })

describe("detached shell composition", () => {
  it("starts in its own transient keyboard scope without changing the saved center tab", () => {
    const center = useUiStore.getState().centerTab
    const mounted = render(<PanelWindow panel="mixer" />)
    expect(activeScopeOf(useUiStore.getState())).toBe("mixer")
    expect(useUiStore.getState().centerTab).toBe(center)
    mounted.unmount()
    expect(useUiStore.getState().activeScope).toBeNull()
  })
  it("renders real settings, export and recording surfaces opened from detached actions", async () => {
    render(<PanelWindow panel="mixer" />)
    await act(async () => { await runAction("options.settings") })
    expect(screen.getByRole("dialog", { name: "Settings" })).toBeVisible()
    act(() => useUiStore.getState().closeDialog())
    await act(async () => { await runAction("file.export") })
    expect(screen.getByRole("dialog", { name: "Export audio" })).toBeVisible()
    act(() => useUiStore.getState().closeDialog())
    await act(async () => { await openRecording() })
    expect(screen.getByRole("dialog", { name: "Record audio input" })).toBeVisible()
  })
})
