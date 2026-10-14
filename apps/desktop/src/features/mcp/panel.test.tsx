import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { McpPanel, McpProposal } from "@/bindings"
import { setBackend, type Backend } from "@/lib/ipc"
import { setLanguage } from "@/lib/i18n"
import { useUiStore } from "@/lib/store/ui"
import { startTestApp } from "@/test/harness"
import { McpPanelHost } from "./panel"

const PROPOSAL: McpProposal = {
  id: "p1",
  agent: "Claude Code",
  label: "Claude Code: Add notes",
  explanation: "A bass line for the verse.",
  status: "pending",
  error: null,
  stale: false,
  automatic: false,
  counts: [{ kind: "notes", before: "8", after: "12" }, { kind: "tempoBpm", before: "120", after: "96" }, { kind: "somethingNew", before: "1", after: "2" }],
  commands: [{ type: "addPattern" }],
}
const PANEL: McpPanel = { proposals: [PROPOSAL], agents: [{ name: "Claude Code", trusted: false }], rendering: false }

describe("AI agent proposals panel", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  let listeners: ((panel: McpPanel) => void)[]
  beforeEach(async () => { app = await startTestApp(); listeners = [] })
  afterEach(() => { cleanup(); app.stop(); setLanguage("en"); vi.restoreAllMocks() })

  function mount(overrides: Partial<Backend> = {}) {
    const calls = {
      mcpApprove: vi.fn(async () => ({ ...PANEL, proposals: [{ ...PROPOSAL, status: "applied" }] })),
      mcpReject: vi.fn(async () => ({ ...PANEL, proposals: [{ ...PROPOSAL, status: "rejected" }] })),
      mcpDismiss: vi.fn(async () => ({ ...PANEL, proposals: [] })),
      mcpTrust: vi.fn(async (_agent: string, trusted: boolean) => ({ ...PANEL, agents: [{ name: "Claude Code", trusted }] })),
      mcpFocus: vi.fn(async () => {}),
      mcpCancelRender: vi.fn(async () => {}),
    }
    setBackend({
      ...app.backend,
      kind: "tauri",
      mcpPanel: async () => PANEL,
      onMcpPanel: (handler: (panel: McpPanel) => void) => { listeners.push(handler); return () => {} },
      ...calls,
      ...overrides,
    })
    render(<McpPanelHost />)
    return calls
  }

  it("is not there while nothing waits", async () => {
    setBackend({ ...app.backend, kind: "tauri", mcpPanel: async () => ({ proposals: [], agents: [], rendering: false }), onMcpPanel: () => () => {} })
    render(<McpPanelHost />)
    await act(async () => {})
    expect(screen.queryByRole("complementary", { name: "AI agent proposals" })).toBeNull()
  })

  it("shows a pending proposal without blocking the window, and applies it on request", async () => {
    const calls = mount()
    const panel = await screen.findByRole("complementary", { name: "AI agent proposals" })
    expect(within(panel).getByText("Claude Code: Add notes")).toBeVisible()
    expect(within(panel).getByText("A bass line for the verse.")).toBeVisible()
    expect(within(panel).getByText("Notes: 8 → 12")).toBeVisible()
    expect(within(panel).getByText("Tempo: 120 → 96 BPM")).toBeVisible()
    expect(within(panel).queryByText(/somethingNew/)).toBeNull()
    expect(within(panel).getByText("Waiting for your decision.")).toBeVisible()
    // Non-modal: no dialog takes the focus.
    expect(screen.queryByRole("dialog")).toBeNull()
    const user = userEvent.setup()
    await user.click(within(panel).getByRole("button", { name: "Apply" }))
    expect(calls.mcpApprove).toHaveBeenCalledWith("p1")
    expect(await within(panel).findByText("Applied as one undo step.")).toBeVisible()
    await user.click(within(panel).getByRole("button", { name: "Dismiss" }))
    expect(calls.mcpDismiss).toHaveBeenCalledWith("p1")
    await waitFor(() => expect(screen.queryByRole("complementary", { name: "AI agent proposals" })).toBeNull())
  })

  it("rejects, follows pushed updates, and disables Apply on a stale proposal", async () => {
    const calls = mount()
    const panel = await screen.findByRole("complementary", { name: "AI agent proposals" })
    const user = userEvent.setup()
    await user.click(within(panel).getByRole("button", { name: "Reject" }))
    expect(calls.mcpReject).toHaveBeenCalledWith("p1")
    expect(await within(panel).findByText("Rejected.")).toBeVisible()
    act(() => listeners.forEach((listen) => listen({ ...PANEL, proposals: [{ ...PROPOSAL, id: "p2", stale: true }] })))
    expect(await within(panel).findByText("The project changed since this was proposed. Ask the agent for a new proposal.")).toBeVisible()
    expect(within(panel).getByRole("button", { name: "Apply" })).toBeDisabled()
  })

  it("lets the user allow an agent to apply without asking", async () => {
    const calls = mount()
    const panel = await screen.findByRole("complementary", { name: "AI agent proposals" })
    fireEvent.click(within(panel).getByRole("checkbox"))
    await waitFor(() => expect(calls.mcpTrust).toHaveBeenCalledWith("Claude Code", true))
    expect(within(panel).getByText("Let Claude Code apply its proposals without asking")).toBeVisible()
    expect(within(panel).getByText("Until Windfall quits or the MCP server is turned off.")).toBeVisible()
  })

  it("shows a failed apply and lets the user stop a render", async () => {
    const calls = mount({
      mcpPanel: async () => ({ proposals: [{ ...PROPOSAL, status: "failed", error: "Stop or cancel recording first." }], agents: [], rendering: true }),
    })
    const panel = await screen.findByRole("complementary", { name: "AI agent proposals" })
    expect(within(panel).getByText("Could not be applied.")).toBeVisible()
    expect(within(panel).getByText("Stop or cancel recording first.")).toBeVisible()
    expect(within(panel).getByText("An agent is rendering the project.")).toBeVisible()
    await userEvent.setup().click(within(panel).getByRole("button", { name: "Stop the render" }))
    expect(calls.mcpCancelRender).toHaveBeenCalled()
  })

  it("speaks Spanish and reports what the user selects", async () => {
    setLanguage("es")
    const calls = mount()
    const panel = await screen.findByRole("complementary", { name: "Propuestas de agentes de IA" })
    expect(within(panel).getByText("Notas: 8 → 12")).toBeVisible()
    expect(within(panel).getByRole("button", { name: "Rechazar" })).toBeVisible()
    act(() => useUiStore.setState({ selectedChannel: 3, centerTab: "pianoRoll" }))
    await waitFor(() => expect(calls.mcpFocus).toHaveBeenLastCalledWith({ channel: 3, track: null, view: "pianoRoll" }), { timeout: 2000 })
  })
})
