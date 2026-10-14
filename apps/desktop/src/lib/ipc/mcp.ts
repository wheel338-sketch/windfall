import type { McpFocus, McpPanel, McpSettings, McpSettingsUpdate } from "@/bindings"
import { translatedText as t } from "@/lib/i18n"

/** The MCP server: off until the user turns it on, loopback only. */
export interface McpBackend {
  mcpState(): Promise<McpSettings>
  mcpConfigure(update: McpSettingsUpdate): Promise<McpSettings>
  mcpRegenerateToken(): Promise<McpSettings>
  mcpPanel(): Promise<McpPanel>
  mcpApprove(id: string): Promise<McpPanel>
  mcpReject(id: string): Promise<McpPanel>
  mcpDismiss(id: string): Promise<McpPanel>
  mcpTrust(agent: string, trusted: boolean): Promise<McpPanel>
  mcpFocus(focus: McpFocus): Promise<void>
  mcpCancelRender(): Promise<void>
  /** The proposals, agents and render the panel shows, whenever they change. */
  onMcpPanel(handler: (panel: McpPanel) => void): () => void
}

type NativeCall = <T>(command: string, args?: Record<string, unknown>) => Promise<T>
type NativeListen = <T>(event: string, handler: (payload: T) => void) => () => void

export const MCP_PANEL_EVENT = "mcp:panel"

export function nativeMcp(call: NativeCall, on: NativeListen): McpBackend {
  return {
    mcpState: () => call("mcp_state"),
    mcpConfigure: (update) => call("mcp_configure", { update }),
    mcpRegenerateToken: () => call("mcp_regenerate_token"),
    mcpPanel: () => call("mcp_panel"),
    mcpApprove: (id) => call("mcp_approve", { id }),
    mcpReject: (id) => call("mcp_reject", { id }),
    mcpDismiss: (id) => call("mcp_dismiss", { id }),
    mcpTrust: (agent, trusted) => call("mcp_trust", { agent, trusted }),
    mcpFocus: (focus) => call("mcp_focus", { focus }),
    mcpCancelRender: () => call("mcp_cancel_render"),
    onMcpPanel: (handler) => on<McpPanel>(MCP_PANEL_EVENT, handler),
  }
}

/** Browser mode has no listener to start and no agent to approve. */
export function unavailableMcp(): McpBackend {
  const unavailable = async (): Promise<never> => { throw new Error(t("This feature requires the Windfall desktop app.")) }
  const empty: McpPanel = { proposals: [], agents: [], rendering: false }
  return {
    mcpState: unavailable,
    mcpConfigure: unavailable,
    mcpRegenerateToken: unavailable,
    mcpPanel: async () => empty,
    mcpApprove: unavailable,
    mcpReject: unavailable,
    mcpDismiss: unavailable,
    mcpTrust: unavailable,
    mcpFocus: async () => {},
    mcpCancelRender: async () => {},
    onMcpPanel: () => () => {},
  }
}
