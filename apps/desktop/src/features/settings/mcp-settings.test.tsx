import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { McpSettings } from "@/bindings"
import { setBackend } from "@/lib/ipc"
import { setLanguage } from "@/lib/i18n"
import { startTestApp } from "@/test/harness"
import { McpSettingsSection } from "./mcp-settings"

const OFF: McpSettings = {
  enabled: false,
  running: false,
  port: 47133,
  token: "a".repeat(64),
  url: "http://127.0.0.1:47133/mcp",
  executable: "C:\\Program Files\\Windfall\\windfall-desktop.exe",
  error: null,
}

describe("Settings > AI agents (MCP)", () => {
  let app: Awaited<ReturnType<typeof startTestApp>>
  beforeEach(async () => { app = await startTestApp() })
  afterEach(() => { app.stop(); setLanguage("en"); vi.restoreAllMocks() })

  it("is off until turned on, then shows where it listens and how to connect", async () => {
    const mcpConfigure = vi.fn(async ({ enabled, port }: { enabled: boolean, port: number }) => ({ ...OFF, enabled, running: enabled, port, url: `http://127.0.0.1:${port}/mcp` }))
    setBackend({ ...app.backend, kind: "tauri", mcpState: async () => OFF, mcpConfigure })
    render(<McpSettingsSection />)
    const button = await screen.findByRole("button", { name: "Turn on the MCP server" })
    await waitFor(() => expect(button).toBeEnabled())
    expect(mcpConfigure).not.toHaveBeenCalled()
    // The token is masked until shown.
    expect(screen.queryByText(OFF.token)).toBeNull()
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Show token" }))
    expect(screen.getByText(OFF.token)).toBeVisible()
    await user.clear(screen.getByLabelText("Port"))
    await user.type(screen.getByLabelText("Port"), "47200")
    await user.click(button)
    expect(mcpConfigure).toHaveBeenCalledWith({ enabled: true, port: 47200 })
    expect(await screen.findByText("http://127.0.0.1:47200/mcp")).toBeInTheDocument()
    expect(screen.getByLabelText("Port")).toBeDisabled()
    // Ready-to-paste snippets for the stdio bridge and for HTTP.
    expect(screen.getByText(/"--mcp-stdio"/)).toBeInTheDocument()
    expect(screen.getByText(/claude mcp add --transport stdio windfall/)).toBeInTheDocument()
    expect(screen.getByText(/claude mcp add --transport http windfall/)).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Turn off the MCP server" }))
    await waitFor(() => expect(mcpConfigure).toHaveBeenLastCalledWith({ enabled: false, port: 47200 }))
  })

  it("copies the HTTP command with the real token and refuses a bad port", async () => {
    const mcpConfigure = vi.fn(async () => OFF)
    setBackend({ ...app.backend, kind: "tauri", mcpState: async () => OFF, mcpConfigure })
    render(<McpSettingsSection />)
    await screen.findByRole("button", { name: "Turn on the MCP server" })
    // user-event installs a clipboard of its own; this one replaces it.
    const user = userEvent.setup()
    const writeText = vi.fn(() => Promise.resolve())
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true })
    const copies = screen.getAllByRole("button", { name: "Copy" })
    fireEvent.click(copies[copies.length - 1])
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`claude mcp add --transport http windfall ${OFF.url} --header "Authorization: Bearer ${OFF.token}"`))
    expect(await screen.findByText("Copied")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Copy token" }))
    await waitFor(() => expect(writeText).toHaveBeenLastCalledWith(OFF.token))
    await user.clear(screen.getByLabelText("Port"))
    await user.type(screen.getByLabelText("Port"), "80")
    await user.click(screen.getByRole("button", { name: "Turn on the MCP server" }))
    expect(await screen.findByText("The port must be a whole number from 1024 to 65535.")).toBeInTheDocument()
    expect(mcpConfigure).not.toHaveBeenCalled()
  })

  it("shows a refused bind and the native capability error, in Spanish too", async () => {
    setBackend({ ...app.backend, kind: "tauri", mcpState: async () => ({ ...OFF, enabled: true, error: "Windfall could not listen on that port. Another program may be using it; choose another port." }) })
    setLanguage("es")
    render(<McpSettingsSection />)
    expect(await screen.findByText("Windfall no pudo escuchar en ese puerto. Puede que otro programa lo esté usando; elige otro puerto.")).toBeInTheDocument()
    expect(screen.getByText("No está escuchando.")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Desactivar el servidor MCP" })).toBeInTheDocument()
  })

  it("says the browser build cannot run the server", async () => {
    render(<McpSettingsSection />)
    expect(await screen.findByText("This feature requires the Windfall desktop app.")).toBeInTheDocument()
  })
})
