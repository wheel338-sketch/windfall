import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { runAction } from "@/lib/actions"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { startTestApp } from "@/test/harness"
import { registerAssistantActions } from "./actions"
import { AssistantPanel } from "./panel"
import { closeAssistant } from "./store"

let app: Awaited<ReturnType<typeof startTestApp>>
let unregister: () => void
beforeEach(async () => { app = await startTestApp(); unregister = registerAssistantActions() })
afterEach(() => { cleanup(); closeAssistant(); unregister(); app.stop(); vi.restoreAllMocks() })
async function open() {
  render(<TooltipProvider><AssistantPanel /></TooltipProvider>)
  await act(() => runAction("help.assistant"))
}
async function ask(prompt: string) {
  fireEvent.change(screen.getByLabelText("Ask about Windfall or request an edit"), { target: { value: prompt } })
  fireEvent.click(screen.getByRole("button", { name: "Ask offline assistant" }))
  await waitFor(() => expect(screen.getByRole("button", { name: "Ask offline assistant" })).toBeEnabled())
}

describe("offline assistant mounted against the Rust document", () => {
  it("answers local help without a project mutation", async () => {
    await open()
    const before = await app.backend.documentSnapshot()
    await ask("how do I undo?")
    expect(screen.getByRole("status")).toHaveTextContent("Undo restores")
    expect(screen.queryByRole("button", { name: "Apply batch" })).toBeNull()
    expect(await app.backend.documentSnapshot()).toEqual(before)
  })
  it("reviews exact commands and applies/undoes one batch", async () => {
    await open()
    const before = useProjectStore.getState()
    await ask("set tempo 127")
    expect(screen.getByText("updateSettings")).toBeVisible()
    expect(useProjectStore.getState().project.settings.tempoBpm).toBe(before.project.settings.tempoBpm)
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.tempoBpm).toBe(127))
    expect(useProjectStore.getState().history.cursor).toBe(before.history.cursor + 1)
    fireEvent.click(screen.getByRole("button", { name: "Undo batch" }))
    await waitFor(() => expect(useProjectStore.getState().project.settings.tempoBpm).toBe(before.project.settings.tempoBpm))
  })
  it("refuses stale proposals and dismisses without changing history", async () => {
    await open(); await ask("create pattern Verse")
    await act(() => dispatch({ type: "addPattern", name: "Concurrent" }))
    expect(screen.getByRole("button", { name: "Apply batch" })).toBeDisabled()
    expect(screen.getByText("The project changed. Generate a new preview.")).toBeVisible()
    const before = await app.backend.documentSnapshot()
    fireEvent.click(screen.getByRole("button", { name: "Dismiss proposal" }))
    expect(await app.backend.documentSnapshot()).toEqual(before)
  })
  it("surfaces recording admission errors and ignores a cancelled provider response", async () => {
    await open(); await ask("set tempo 127")
    vi.spyOn(app.backend, "proposalApply").mockRejectedValueOnce(new Error("Stop or cancel recording first."))
    fireEvent.click(screen.getByRole("button", { name: "Apply batch" }))
    expect(await screen.findByText("Stop or cancel recording first.")).toBeVisible()
    let finish!: (reply: Awaited<ReturnType<typeof app.backend.assistantRespond>>) => void
    const pending = new Promise<Awaited<ReturnType<typeof app.backend.assistantRespond>>>((resolve) => { finish = resolve })
    vi.spyOn(app.backend, "assistantRespond").mockReturnValueOnce(pending)
    fireEvent.click(screen.getByRole("button", { name: "Ask offline assistant" }))
    fireEvent.click(screen.getByRole("button", { name: "Cancel response" }))
    await act(async () => finish({ answer: "Late answer", proposal: null }))
    expect(screen.queryByText("Late answer")).toBeNull()
  })
})
