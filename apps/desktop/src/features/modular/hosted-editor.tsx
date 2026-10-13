import { useState } from "react"
import type { ChannelId, PluginEntry, PluginTarget, RackGraph, RackNode } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { PluginControls } from "@/features/plugins/controls"
import { backend } from "@/lib/ipc"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { ChoiceField } from "./note-editor"
import { translatedText as t } from "@/lib/i18n"

export function RackHostedEditor({ channel, path, node, detach }: { channel: ChannelId; path: number[]; node: RackNode; root: RackGraph; detach(): Promise<unknown> }) {
  const [entries, setEntries] = useState<PluginEntry[]>([])
  const [choice, setChoice] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const instrument = node.processor.type === "instrument"
  async function browse() {
    setError(null)
    try { const state = await backend.pluginsState(); setEntries(state.entries.filter((entry) => entry.usable && entry.instrument === instrument && ["clap", "vst3", "au"].includes(entry.format))); if (backend.kind === "mock") throw new Error(t("Native plugin preparation is unavailable in browser mode")) }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
  }
  async function load() {
    if (busy) return
    const entry = entries.find((entry, index) => `${entry.name} (${entry.format}, ${index + 1})` === choice)
    if (!entry) return
    const generation = getProjectGeneration(), revision = useProjectStore.getState().revision
    setBusy(true); setError(null)
    try {
      const state = await backend.timelineState()
      if (generation !== getProjectGeneration() || revision !== useProjectStore.getState().revision) throw new Error(t("Project changed during provider selection; retry"))
      const ids = [...path, node.id]
      if (ids.length > 4) throw new Error(t("Rack path exceeds the native owner depth"))
      const target: PluginTarget = { type: "rack", channel, path: [ids[0] ?? 0, ids[1] ?? 0, ids[2] ?? 0, ids[3] ?? 0], instrument }
      const result = await backend.pluginsInsert(entry.path, entry.id, target, null, state.generation, state.revision)
      if (generation !== getProjectGeneration()) return
      receivePatch(result.patch)
    } catch (cause) { if (generation === getProjectGeneration()) setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { if (generation === getProjectGeneration()) setBusy(false) }
  }
  return <>
    {node.hosted && <><PluginControls binding={node.hosted} /><Button variant="outline" disabled={busy} onClick={() => void detach()}>{t("Use built-in provider")}</Button></>}
    <Button variant="outline" disabled={busy} onClick={() => void browse()}>{t("Choose native provider")}</Button>
    {entries.length > 0 && <><ChoiceField label={t("Rack native provider")} value={choice} choices={entries.map((entry, index) => `${entry.name} (${entry.format}, ${index + 1})`)} onChange={setChoice} /><Button disabled={busy || !choice} onClick={() => void load()}>{t("Load native provider")}</Button></>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </>
}
