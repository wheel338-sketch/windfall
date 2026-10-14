import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { setLanguage } from "@/lib/i18n"
import { useProjectStore } from "@/lib/store/project"
import { MENUS, menuActionIds } from "@/lib/actions/menus"
import { UpdateDecision, UpdateNotice } from "./notice"
import { UpdatesSection } from "./section"
import { AUTO_CHECK_KEY, CHECK_INTERVAL_MS, LAST_CHECK_KEY, checkDue, notesSummary } from "./schedule"
import { checkFromMenu, checkForUpdates, resetUpdates, setUpdateApi, startAutomaticChecks, useUpdateStore } from "./store"
import type { UpdateApi, UpdateProgress } from "./native"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }) }))
const coded = (code: string, error: string) => `windfall-message:${JSON.stringify({ code, params: { error } })}`

const original = useProjectStore.getState()
afterEach(() => {
  vi.useRealTimers(); vi.restoreAllMocks(); vi.mocked(toast).mockClear(); vi.mocked(toast.error).mockClear()
  useProjectStore.setState(original); resetUpdates(); localStorage.clear(); setLanguage("en")
})
const NOTES = "## Windfall 1.0.2\n\n- Fixes the **mixer** meter.\n- Faster start-up.\n\nFull notes on the release page."
function fixture(): UpdateApi {
  return {
    check: vi.fn(async () => ({ configured: true, reason: null, update: { token: 7, version: "1.0.2", notes: NOTES, channel: "stable" as const, projectFormat: 1 } })),
    download: vi.fn(async (_token: number, progress: (next: UpdateProgress) => void) => { progress({ downloaded: 5, total: 10, verified: false }); progress({ downloaded: 10, total: 10, verified: true }) }),
    begin: vi.fn(async () => ({ dirty: useProjectStore.getState().dirty, path: "/song.windfall" })),
    cancel: vi.fn(async () => {}),
    install: vi.fn(async () => {}),
  }
}
function mount(api: UpdateApi) {
  setUpdateApi(api)
  return render(<><UpdateNotice /><UpdateDecision /></>)
}
const notice = () => screen.queryByRole("region", { name: "Update available" })

describe("when Windfall checks by itself", () => {
  it("is due a day after the last check, and when a clock went back", () => {
    const now = Date.UTC(2026, 9, 14, 12)
    expect(checkDue(now, null)).toBe(true)
    expect(checkDue(now, now - CHECK_INTERVAL_MS + 1)).toBe(false)
    expect(checkDue(now, now - 60_000)).toBe(false)
    expect(checkDue(now, now - CHECK_INTERVAL_MS)).toBe(true)
    expect(checkDue(now, now + 60_000)).toBe(true)
  })
  it("checks once, in the background, a while after start-up and then not again that day", async () => {
    vi.useFakeTimers({ now: Date.UTC(2026, 9, 14, 12) })
    const api = fixture(); setUpdateApi(api)
    startAutomaticChecks({ native: true, delay: 5_000 })
    await vi.advanceTimersByTimeAsync(4_999)
    expect(api.check).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    expect(api.check).toHaveBeenCalledExactlyOnceWith("stable", true)
    expect(localStorage.getItem(LAST_CHECK_KEY)).toBe(String(Date.UTC(2026, 9, 14, 12, 0, 5)))
    resetUpdates(); setUpdateApi(api)
    startAutomaticChecks({ native: true, delay: 5_000 })
    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS / 2)
    expect(api.check).toHaveBeenCalledTimes(1)
    vi.setSystemTime(Date.UTC(2026, 9, 15, 12, 0, 6))
    startAutomaticChecks({ native: true, delay: 5_000 })
    await vi.advanceTimersByTimeAsync(5_000)
    expect(api.check).toHaveBeenCalledTimes(2)
  })
  it("does not check when the setting is off, outside the desktop app, or when stopped first", async () => {
    vi.useFakeTimers()
    const api = fixture(); setUpdateApi(api)
    startAutomaticChecks({ native: false, delay: 10 })
    await vi.advanceTimersByTimeAsync(20)
    localStorage.setItem(AUTO_CHECK_KEY, "off")
    startAutomaticChecks({ native: true, delay: 10 })
    await vi.advanceTimersByTimeAsync(20)
    localStorage.removeItem(AUTO_CHECK_KEY)
    const stop = startAutomaticChecks({ native: true, delay: 10 })
    stop()
    await vi.advanceTimersByTimeAsync(20)
    expect(api.check).not.toHaveBeenCalled()
  })
  it("turns automatic checks off from Settings, kept for next time", async () => {
    vi.useFakeTimers()
    const api = fixture(); setUpdateApi(api)
    render(<UpdatesSection />)
    const automatic = screen.getByRole("switch", { name: "Check for updates automatically" })
    expect(automatic).toBeChecked()
    act(() => { fireEvent.click(automatic) })
    expect(automatic).not.toBeChecked()
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("off")
    startAutomaticChecks({ native: true, delay: 10 })
    await vi.advanceTimersByTimeAsync(20)
    expect(api.check).not.toHaveBeenCalled()
    expect(screen.queryByRole("combobox", { name: "Update channel" })).toBeNull()
  })
  it("says nothing when the background check fails or finds nothing", async () => {
    const plain = vi.spyOn(console, "info").mockImplementation(() => {})
    const api = fixture(); vi.mocked(api.check).mockRejectedValueOnce(new Error("error sending request: dns error"))
    mount(api)
    await act(async () => { await checkForUpdates("background") })
    expect(notice()).toBeNull()
    expect(screen.queryByRole("alert")).toBeNull()
    expect(useUpdateStore.getState()).toMatchObject({ phase: "idle", error: "", status: null })
    expect(localStorage.getItem(LAST_CHECK_KEY)).toBeNull()
    vi.mocked(api.check).mockResolvedValueOnce({ configured: false, reason: "Development builds do not check for updates by themselves.", update: null })
    await act(async () => { await checkForUpdates("background") })
    vi.mocked(api.check).mockResolvedValueOnce({ configured: true, reason: null, update: null })
    await act(async () => { await checkForUpdates("background") })
    expect(notice()).toBeNull()
    expect(useUpdateStore.getState().status).toBeNull()
    expect(toast).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
    expect(plain).toHaveBeenCalledTimes(1)
  })
})

describe("the update notice", () => {
  it("shows the version and the start of the notes, and Later puts it away", async () => {
    const api = fixture(); mount(api)
    await act(async () => { await checkForUpdates("background") })
    expect(notice()).toBeVisible()
    expect(screen.getByRole("heading", { name: "Windfall 1.0.2 is available" })).toBeVisible()
    expect(screen.getByText(/Fixes the mixer meter\.\s+Faster start-up\./)).toBeVisible()
    act(() => setLanguage("es"))
    expect(screen.getByRole("heading", { name: "Windfall 1.0.2 está disponible" })).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Más tarde" }))
    expect(notice()).toBeNull()
    expect(api.download).not.toHaveBeenCalled()
    // The update is still there for Settings and the Help menu.
    expect(useUpdateStore.getState().candidate?.version).toBe("1.0.2")
  })
  it("installs now: downloads, verifies, then installs a clean project through the shutdown", async () => {
    const api = fixture(); useProjectStore.setState({ dirty: false }); mount(api)
    await act(async () => { await checkForUpdates("background") })
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(api.download).toHaveBeenCalledWith(7, expect.any(Function))
    expect(api.begin).toHaveBeenCalledWith(7)
    expect(api.install).toHaveBeenCalledWith(7, "discard", undefined)
    expect(vi.mocked(api.download).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(api.begin).mock.invocationCallOrder[0])
  })
  it("asks about unsaved work first, and Cancel keeps the verified update and the project", async () => {
    const api = fixture(); useProjectStore.setState({ dirty: true }); mount(api)
    await act(async () => { await checkForUpdates("background") })
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(screen.getByRole("dialog", { name: "Unsaved project" })).toBeVisible()
    expect(api.install).not.toHaveBeenCalled()
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Cancel" })))
    expect(api.cancel).toHaveBeenCalledWith(7)
    expect(useUpdateStore.getState()).toMatchObject({ phase: "verified", unsaved: null })
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(api.download).toHaveBeenCalledTimes(1)
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save and install" })))
    expect(api.install).toHaveBeenCalledWith(7, "save", "/song.windfall")
  })
  it("refuses a package that fails verification, installs nothing and lets the user try again", async () => {
    const api = fixture(); mount(api)
    vi.mocked(api.download).mockRejectedValueOnce(new Error(coded("updates.downloadFailed", "signature verification failed")))
    await act(async () => { await checkForUpdates("background") })
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(screen.getByRole("alert")).toHaveTextContent("Download or signature verification failed. Nothing was installed.")
    expect(api.begin).not.toHaveBeenCalled()
    expect(api.install).not.toHaveBeenCalled()
    expect(useUpdateStore.getState().phase).toBe("available")
    expect(screen.getByRole("button", { name: "Install now" })).toBeEnabled()
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(api.install).toHaveBeenCalledTimes(1)
  })
  it("rolls the shutdown back when the installer is refused", async () => {
    const api = fixture(); mount(api)
    vi.mocked(api.install).mockRejectedValueOnce(new Error("Installer refused"))
    await act(async () => { await checkForUpdates("background") })
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Install now" })))
    expect(api.cancel).toHaveBeenCalledWith(7)
    expect(screen.getByRole("alert")).toHaveTextContent("Installer refused")
    expect(useUpdateStore.getState().phase).toBe("verified")
  })
})

describe("Help → Check for updates", () => {
  it("is in the Help menu", () => {
    const help = MENUS.find((menu) => menu.title === "Help")
    expect(help && menuActionIds(help.entries)).toContain("help.checkUpdates")
  })
  it("raises the notice for an update, and says when there is none", async () => {
    const api = fixture(); mount(api)
    await act(async () => { await checkFromMenu() })
    expect(api.check).toHaveBeenCalledWith("stable", false)
    expect(notice()).toBeVisible()
    expect(toast).not.toHaveBeenCalled()
    vi.mocked(api.check).mockResolvedValueOnce({ configured: true, reason: null, update: null })
    await act(async () => { await checkFromMenu() })
    expect(toast).toHaveBeenCalledExactlyOnceWith("You are up to date.")
    expect(notice()).toBeNull()
  })
  it("reports a failed check clearly", async () => {
    const error = vi.mocked(toast.error)
    const api = fixture(); vi.mocked(api.check).mockRejectedValueOnce(new Error(coded("updates.checkFailed", "error sending request")))
    mount(api)
    await act(async () => { await checkFromMenu() })
    expect(error).toHaveBeenCalledExactlyOnceWith("Could not check for updates", { description: "Update check failed; retry when online. error sending request" })
    expect(notice()).toBeNull()
  })
})

describe("the notes summary", () => {
  it("keeps the first lines without Markdown and stops at its length", () => {
    expect(notesSummary(NOTES)).toBe("Windfall 1.0.2\nFixes the mixer meter.\nFaster start-up.\nFull notes on the release page.")
    expect(notesSummary(NOTES, 40)).toBe("Windfall 1.0.2\nFixes the mixer meter.\n…")
    expect(notesSummary("x".repeat(400), 10)).toBe("xxxxxxxxx…")
    expect(notesSummary("")).toBe("")
  })
})
