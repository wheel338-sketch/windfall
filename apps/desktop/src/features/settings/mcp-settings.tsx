import { useEffect, useState } from "react"
import type { McpSettings } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"
import { localizeBackendText, translatedText as t } from "@/lib/i18n"

/** What Claude Desktop's claude_desktop_config.json needs to start the stdio bridge. */
function desktopConfig(executable: string) {
  return { mcpServers: { windfall: { command: executable, args: ["--mcp-stdio"] } } }
}

function quoted(path: string) {
  return `"${path}"`
}

/**
 * Settings > AI agents (MCP). The server is off until the user turns it on,
 * listens on the loopback address only, and lets an agent change the project
 * only through proposals the user approves in the panel.
 */
export function McpSettingsSection() {
  const [state, setState] = useState<McpSettings | null>(null)
  const [port, setPort] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [shown, setShown] = useState(false)
  const [copied, setCopied] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void backend.mcpState().then(
      (loaded) => { if (live) { setState(loaded); setPort(String(loaded.port)) } },
      (failure: unknown) => { if (live) setError(errorMessage(failure)) },
    )
    return () => { live = false }
  }, [])

  async function toggle() {
    if (!state) return
    const number = Number(port)
    if (!Number.isInteger(number) || number < 1024 || number > 65535) {
      setError(t("The port must be a whole number from 1024 to 65535."))
      return
    }
    setBusy(true); setError(null)
    try { setState(await backend.mcpConfigure({ enabled: !state.enabled, port: number })) }
    catch (failure) { setError(errorMessage(failure)) }
    finally { setBusy(false) }
  }

  async function regenerate() {
    setBusy(true); setError(null)
    try { setState(await backend.mcpRegenerateToken()) }
    catch (failure) { setError(errorMessage(failure)) }
    finally { setBusy(false) }
  }

  async function copy(what: string, text: string) {
    try { await navigator.clipboard.writeText(text); setCopied(what) }
    catch (failure) { setError(errorMessage(failure)) }
  }

  const disabled = busy || state === null
  return <FieldSet>
    <FieldLegend>{t("AI agents (MCP)")}</FieldLegend>
    <FieldDescription>{t("Lets an AI agent on this computer, such as Claude Desktop or Claude Code, read the open project, render and measure it, and propose edits that you approve. It is off until you turn it on, and listens on 127.0.0.1 only.")}</FieldDescription>
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor="mcp-port">{t("Port")}</FieldLabel>
        <Input id="mcp-port" inputMode="numeric" value={port} disabled={disabled || state?.enabled === true} onChange={(event) => setPort(event.target.value)} />
      </Field>
      <Button variant="outline" disabled={disabled} onClick={() => void toggle()}>{state?.enabled ? t("Turn off the MCP server") : t("Turn on the MCP server")}</Button>
      {state?.enabled && state.running && <p role="status">{t("Listening at")}{" "}<code>{state.url}</code></p>}
      {state?.enabled && !state.running && <p role="status">{t("Not listening.")}</p>}
      {state?.error && <Alert variant="destructive"><AlertDescription>{localizeBackendText(state.error)}</AlertDescription></Alert>}
      {state && <Field>
        <FieldLabel>{t("Bearer token")}</FieldLabel>
        <code className="break-all" aria-label={t("Bearer token")}>{shown ? state.token : "●".repeat(16)}</code>
        <div className="flex flex-wrap gap-2">
          <Button variant="ghost" size="sm" onClick={() => setShown(!shown)}>{shown ? t("Hide token") : t("Show token")}</Button>
          <Button variant="ghost" size="sm" onClick={() => void copy("token", state.token)}>{t("Copy token")}</Button>
          <Button variant="ghost" size="sm" disabled={disabled} onClick={() => void regenerate()}>{t("New token")}</Button>
        </div>
        <FieldDescription>{t("A new token disconnects every agent until it is given the new one. The stdio bridge reads the token by itself.")}</FieldDescription>
      </Field>}
      {state && <Field>
        <FieldLabel>{t("Connect a client")}</FieldLabel>
        <FieldDescription>{t("Claude Desktop: add this to claude_desktop_config.json, then restart Claude Desktop.")}</FieldDescription>
        <pre className="overflow-x-auto rounded-md bg-muted p-2"><code>{JSON.stringify(desktopConfig(state.executable), null, 2)}</code></pre>
        <Button variant="ghost" size="sm" onClick={() => void copy("desktop", JSON.stringify(desktopConfig(state.executable), null, 2))}>{t("Copy")}</Button>
        <FieldDescription>{t("Claude Code, through the stdio bridge:")}</FieldDescription>
        <pre className="overflow-x-auto rounded-md bg-muted p-2"><code>claude mcp add --transport stdio windfall -- {quoted(state.executable)} --mcp-stdio</code></pre>
        <Button variant="ghost" size="sm" onClick={() => void copy("stdio", `claude mcp add --transport stdio windfall -- ${quoted(state.executable)} --mcp-stdio`)}>{t("Copy")}</Button>
        <FieldDescription>{t("Claude Code, over HTTP. Copy puts the token in; the text shown names it WINDFALL_MCP_TOKEN.")}</FieldDescription>
        <pre className="overflow-x-auto rounded-md bg-muted p-2"><code>claude mcp add --transport http windfall {state.url} --header "Authorization: Bearer $WINDFALL_MCP_TOKEN"</code></pre>
        <Button variant="ghost" size="sm" onClick={() => void copy("http", `claude mcp add --transport http windfall ${state.url} --header "Authorization: Bearer ${state.token}"`)}>{t("Copy")}</Button>
        {copied && <p role="status">{t("Copied")}</p>}
      </Field>}
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    </FieldGroup>
  </FieldSet>
}
