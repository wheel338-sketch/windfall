import { useEffect, useRef, useState } from "react"
import type { McpCount, McpPanel, McpProposal } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { backend, errorMessage } from "@/lib/ipc"
import { useUiStore } from "@/lib/store/ui"
import { localizeBackendText, message, translatedText as t } from "@/lib/i18n"

const EMPTY: McpPanel = { proposals: [], agents: [], rendering: false }

/** One changed count, in the interface's language. Unknown kinds are left out. */
function countText(count: McpCount): string | null {
  const values = { before: count.before, after: count.after }
  switch (count.kind) {
    case "channels": return message("Channels: {before} → {after}", values)
    case "patterns": return message("Patterns: {before} → {after}", values)
    case "notes": return message("Notes: {before} → {after}", values)
    case "mixerTracks": return message("Mixer tracks: {before} → {after}", values)
    case "effects": return message("Effects: {before} → {after}", values)
    case "playlistTracks": return message("Playlist tracks: {before} → {after}", values)
    case "clips": return message("Clips: {before} → {after}", values)
    case "automations": return message("Automations: {before} → {after}", values)
    case "tempoBpm": return message("Tempo: {before} → {after} BPM", values)
    case "meter": return message("Meter: {before} → {after}", values)
    default: return null
  }
}

function statusText(proposal: McpProposal): string {
  switch (proposal.status) {
    case "pending": return proposal.stale ? t("The project changed since this was proposed. Ask the agent for a new proposal.") : t("Waiting for your decision.")
    case "applying": return t("Applying…")
    case "applied": return proposal.automatic ? t("Applied without asking, as one undo step.") : t("Applied as one undo step.")
    case "rejected": return t("Rejected.")
    case "withdrawn": return t("Withdrawn by the agent, or the MCP server was turned off.")
    default: return t("Could not be applied.")
  }
}

/** Reports what the user is looking at while an agent is connected, so it can ask. */
function useFocusReport() {
  const channel = useUiStore((state) => state.selectedChannel)
  const track = useUiStore((state) => state.selectedTrack)
  const view = useUiStore((state) => state.centerTab)
  useEffect(() => {
    if (backend.kind !== "tauri") return
    const timer = setTimeout(() => { void backend.mcpFocus({ channel, track, view }).catch(() => {}) }, 300)
    return () => clearTimeout(timer)
  }, [channel, track, view])
}

function ProposalCard({ proposal, busy, act }: {
  proposal: McpProposal
  busy: boolean
  act(action: () => Promise<McpPanel>): void
}) {
  const counts = proposal.counts.map(countText).filter((text): text is string => text !== null)
  const pending = proposal.status === "pending"
  return <li className="flex flex-col gap-1 rounded-md border p-2" aria-label={proposal.label}>
    <p className="font-medium">{proposal.label}</p>
    {proposal.explanation && <p className="text-muted-foreground">{proposal.explanation}</p>}
    {counts.length > 0 && <ul className="list-disc pl-5">{counts.map((text) => <li key={text}>{text}</li>)}</ul>}
    <details>
      <summary>{t("Show the commands")}</summary>
      <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(proposal.commands, null, 2)}</pre>
    </details>
    <p role="status">{statusText(proposal)}</p>
    {proposal.error && <Alert variant="destructive"><AlertDescription>{localizeBackendText(proposal.error)}</AlertDescription></Alert>}
    <div className="flex gap-2">
      {pending && <Button size="sm" disabled={busy || proposal.stale} onClick={() => act(() => backend.mcpApprove(proposal.id))}>{t("Apply")}</Button>}
      {pending && <Button size="sm" variant="outline" disabled={busy} onClick={() => act(() => backend.mcpReject(proposal.id))}>{t("Reject")}</Button>}
      {!pending && proposal.status !== "applying" && <Button size="sm" variant="ghost" disabled={busy} onClick={() => act(() => backend.mcpDismiss(proposal.id))}>{t("Dismiss")}</Button>}
    </div>
  </li>
}

/**
 * Proposals from connected AI agents. Non-modal: the user goes on working
 * while one waits. Apply is one checked batch and one undo step; nothing an
 * agent proposes changes the project before that.
 */
export function McpPanelHost() {
  const [panel, setPanel] = useState<McpPanel>(EMPTY)
  const [hidden, setHidden] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const acting = useRef(false)
  useFocusReport()
  useEffect(() => {
    let live = true
    const stop = backend.onMcpPanel((next) => { if (live) setPanel(next) })
    void backend.mcpPanel().then((loaded) => { if (live) setPanel(loaded) }, () => {})
    return () => { live = false; stop() }
  }, [])

  function act(action: () => Promise<McpPanel>) {
    if (acting.current) return
    acting.current = true
    setBusy(true); setError(null)
    action().then(setPanel, (failure: unknown) => setError(errorMessage(failure))).finally(() => { acting.current = false; setBusy(false) })
  }

  const shown = panel.proposals.filter((proposal) => !hidden.includes(proposal.id))
  const pending = shown.some((proposal) => proposal.status === "pending")
  if (shown.length === 0 && !panel.rendering) return null
  return <aside aria-label={t("AI agent proposals")} className="fixed bottom-10 left-3 z-40 flex max-h-[60vh] w-96 flex-col gap-2 overflow-y-auto rounded-md border bg-popover p-3 text-xs shadow-lg">
    <div className="flex items-center justify-between gap-2">
      <h2 className="font-medium">{t("AI agent proposals")}</h2>
      <Button size="sm" variant="ghost" onClick={() => setHidden(panel.proposals.map((proposal) => proposal.id))}>{t("Hide")}</Button>
    </div>
    {panel.rendering && <div className="flex items-center justify-between gap-2" role="status">
      <span>{t("An agent is rendering the project.")}</span>
      <Button size="sm" variant="outline" onClick={() => void backend.mcpCancelRender().catch((failure: unknown) => setError(errorMessage(failure)))}>{t("Stop the render")}</Button>
    </div>}
    <ol className="flex flex-col gap-2">
      {shown.map((proposal) => <ProposalCard key={proposal.id} proposal={proposal} busy={busy} act={act} />)}
    </ol>
    {pending && panel.agents.length > 0 && <div className="flex flex-col gap-1">
      {panel.agents.map((agent) => <label key={agent.name} className="flex items-center gap-2">
        <Checkbox checked={agent.trusted} disabled={busy} onCheckedChange={(checked) => act(() => backend.mcpTrust(agent.name, checked === true))} />
        <span>{message("Let {agent} apply its proposals without asking", { agent: agent.name })}</span>
      </label>)}
      <p className="text-muted-foreground">{t("Until Windfall quits or the MCP server is turned off.")}</p>
    </div>}
    {error && <Alert variant="destructive"><AlertDescription>{localizeBackendText(error)}</AlertDescription></Alert>}
  </aside>
}
