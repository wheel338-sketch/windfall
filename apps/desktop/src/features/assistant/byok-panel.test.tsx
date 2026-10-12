import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { AssistantPanel } from "./panel"
import { closeAssistant, openAssistant } from "./store"

let app: Awaited<ReturnType<typeof startTestApp>>
beforeEach(async () => { app = await startTestApp(); Object.defineProperty(app.backend, "kind", { value: "tauri", configurable: true }) })
afterEach(() => { cleanup(); closeAssistant(); app.stop(); vi.restoreAllMocks() })
function open() { render(<TooltipProvider><AssistantPanel /></TooltipProvider>); act(() => openAssistant()) }
const connected = { baseUrl: "https://example.invalid/v1", model: "chosen", enabled: true, hasKey: true }

describe("BYOK panel explicit-action cases (HTTP responses are fixtures)", () => {
  it("stays OFF without a key, saves credentials outside the document and only sends on Ask", async () => {
    const remote = vi.spyOn(app.backend, "assistantRemote")
    vi.spyOn(app.backend, "assistantRequestBegin").mockResolvedValue(2)
    const configure = vi.spyOn(app.backend, "assistantConfigure").mockResolvedValue(connected)
    open()
    fireEvent.change(screen.getByLabelText("Ask about Windfall or request an edit"), { target: { value: "set tempo 127" } })
    expect(screen.getByRole("button", { name: "Ask BYOK assistant" })).toBeDisabled()
    fireEvent.click(screen.getByText("Bring your own key connection"))
    await screen.findByText("OFF until you enter a key and enable this connection.")
    fireEvent.change(screen.getByLabelText("Chat completions base URL"), { target: { value: connected.baseUrl } })
    fireEvent.change(screen.getByLabelText("Model name"), { target: { value: connected.model } })
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "fixture-private-key" } })
    fireEvent.click(screen.getByRole("checkbox", { name: "Enable BYOK requests" }))
    const before = await app.backend.documentSnapshot()
    fireEvent.click(screen.getByRole("button", { name: "Save BYOK connection" }))
    await waitFor(() => expect(screen.getByRole("button", { name: "Ask BYOK assistant" })).toBeEnabled())
    expect(configure).toHaveBeenCalledWith({ baseUrl: connected.baseUrl, model: connected.model, enabled: true, apiKey: "fixture-private-key" })
    expect(screen.getByLabelText("API key")).toHaveValue("")
    expect(remote).not.toHaveBeenCalled()
    expect(await app.backend.documentSnapshot()).toEqual(before)
    const reply = await app.backend.assistantRespond("set tempo 127")
    remote.mockResolvedValueOnce(reply)
    fireEvent.click(screen.getByRole("button", { name: "Ask BYOK assistant" }))
    await screen.findByText("updateSettings")
    expect(remote).toHaveBeenCalledWith(2, "set tempo 127")
    expect(await app.backend.documentSnapshot()).toEqual(before)
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.tempoBpm).toBe(127))
    expect(JSON.stringify(useProjectStore.getState().project)).not.toContain("fixture-private-key")
  })
  it("cancels native requests, ignores late replies, surfaces malformed/timeout failures and removes a key", async () => {
    vi.spyOn(app.backend, "assistantConnection").mockResolvedValue(connected)
    vi.spyOn(app.backend, "assistantRequestBegin").mockResolvedValue(8)
    const cancel = vi.spyOn(app.backend, "assistantRequestCancel").mockResolvedValue()
    const remote = vi.spyOn(app.backend, "assistantRemote")
    let finish!: (reply: Awaited<ReturnType<typeof app.backend.assistantRemote>>) => void
    remote.mockReturnValueOnce(new Promise((resolve) => { finish = resolve }))
    open()
    fireEvent.change(screen.getByLabelText("Ask about Windfall or request an edit"), { target: { value: "help" } })
    await waitFor(() => expect(screen.getByRole("button", { name: "Ask BYOK assistant" })).toBeEnabled())
    fireEvent.click(screen.getByRole("button", { name: "Ask BYOK assistant" }))
    await waitFor(() => expect(remote).toHaveBeenCalled())
    fireEvent.click(screen.getByRole("button", { name: "Cancel response" }))
    expect(cancel).toHaveBeenCalledWith(8)
    await act(async () => finish({ answer: "Late remote answer", proposal: null }))
    expect(screen.queryByText("Late remote answer")).toBeNull()
    for (const message of ["assistant returned malformed reply JSON", "assistant endpoint timed out"]) {
      remote.mockRejectedValueOnce(new Error(message))
      fireEvent.click(screen.getByRole("button", { name: "Ask BYOK assistant" }))
      expect(await screen.findByText(message.charAt(0).toUpperCase() + message.slice(1))).toBeVisible()
    }
    fireEvent.click(screen.getByText("Bring your own key connection"))
    const configure = vi.spyOn(app.backend, "assistantConfigure").mockResolvedValue({ ...connected, enabled: false, hasKey: false })
    fireEvent.click(screen.getByRole("button", { name: "Remove key and turn OFF" }))
    await waitFor(() => expect(screen.getByRole("button", { name: "Ask BYOK assistant" })).toBeDisabled())
    expect(configure.mock.calls[0][0].apiKey).toBe("")
    expect(configure.mock.calls[0][0].enabled).toBe(false)
  })
})
