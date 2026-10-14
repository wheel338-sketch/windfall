import { useState } from "react"
import type { Channel, PresetEntry, RackGraph } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { exampleRack, importRackGraph, RACK_EXAMPLES } from "./examples"
import { localizeBackendText, message, translatedText as t } from "@/lib/i18n"

/** Rack files use the existing native .wfpreset container. Capture includes
 * the whole graph; loading here applies only its rack in one checked edit. */
export function RackLibrary({ channel, root }: { channel: Channel; root: RackGraph }) {
  const [name, setName] = useState(`${channel.name} rack`)
  const [entries, setEntries] = useState<PresetEntry[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [status, setStatus] = useState("")
  async function action(run: (guard: () => void, generation: number, revision: number) => Promise<void>) {
    if (busy) return
    const generation = getProjectGeneration()
    const revision = useProjectStore.getState().revision
    const guard = () => {
      if (generation !== getProjectGeneration() || revision !== useProjectStore.getState().revision)
        throw new Error(t("The project changed while the rack preset was open; retry from its current graph"))
    }
    setBusy(true); setError(null); setStatus("")
    try {
      const identity = await backend.timelineState()
      guard()
      if (identity.revision !== revision) throw new Error(t("The project changed while the rack preset was open; retry from its current graph"))
      await run(guard, identity.generation, identity.revision)
    }
    catch (cause) { if (generation === getProjectGeneration()) setError(cause instanceof Error ? cause.message : t("Rack preset operation failed")) }
    finally { if (generation === getProjectGeneration()) setBusy(false) }
  }
  async function apply(graph: RackGraph, guard: () => void) {
    guard()
    const result = await dispatch({ type: "setChannelRack", id: channel.id, expected: root, graph: importRackGraph(graph, root.nextId) })
    if (!result) throw new Error(t("Rack preset was refused; the saved graph was kept"))
    setStatus("Rack preset loaded")
  }
  const load = (path?: string) => action(async (guard) => {
    const picked = path ?? await backend.pickPresetPath()
    if (!picked) return
    const review = await backend.presetRead(picked)
    const graph = review.preset.channel?.rack
    if (!graph) throw new Error(t("This preset does not contain a modular rack"))
    await apply(graph, guard)
    const warnings = [...review.notes]
    if (review.missingAudio.length > 0) warnings.push(message("Missing rack assets: {names}. Open the owning module and choose a replacement source to relink.", { names: review.missingAudio.join(", ") }))
    if (warnings.length > 0) setError(warnings.join(" "))
  })
  const save = (library: boolean) => action(async (guard, generation, revision) => {
    if (!name.trim()) throw new Error(t("Give the rack preset a name"))
    const capture = await backend.presetCaptureChannel(channel.id, generation, revision)
    guard()
    capture.preset.name = name.trim()
    if (library) {
      const entry = await backend.presetLibrarySave(name.trim(), capture)
      setEntries(await backend.presetLibrary()); setStatus(`Saved ${entry.name}`)
    } else {
      const path = await backend.pickPresetPath(name.trim())
      if (!path) return
      guard(); await backend.presetWrite(path, capture); setStatus("Rack preset file saved")
    }
  })
  return <section aria-label={t("Rack preset library")} className="flex flex-col gap-2 rounded-md border p-3">
    <Field><FieldLabel htmlFor="rack-library-name">{t("Rack preset name")}</FieldLabel><Input id="rack-library-name" value={name} onChange={(event) => setName(event.target.value)} /></Field>
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" disabled={busy} onClick={() => void save(false)}>{t("Save rack file")}</Button>
      <Button variant="outline" disabled={busy} onClick={() => void load()}>{t("Load rack file")}</Button>
      <Button variant="outline" disabled={busy} onClick={() => void save(true)}>{t("Save rack to library")}</Button>
      <Button variant="outline" disabled={busy} onClick={() => void action(async (guard) => { const list = await backend.presetLibrary(); guard(); setEntries(list) })}>{t("Browse rack library")}</Button>
    </div>
    {error && <Alert variant="destructive"><AlertDescription>{localizeBackendText(error)}</AlertDescription></Alert>}
    {status && <p role="status">{status}</p>}
    {entries && <ul aria-label={t("User rack presets")}>{entries.filter((entry) => entry.kind === "channel").map((entry) => <li key={entry.path}>
      <Button variant="ghost" disabled={busy} onClick={() => void load(entry.path)}>{entry.name}</Button>
    </li>)}</ul>}
    {entries?.length === 0 && <p className="text-muted-foreground">{t("The user preset library is empty.")}</p>}
    <p className="text-muted-foreground">{t("Original CC0 example racks")}</p>
    <div className="flex flex-wrap gap-2">{RACK_EXAMPLES.map((name) => <Button key={name} variant="outline" disabled={busy} onClick={() => void action(async (guard) => {
      if (channel.source.type !== "instrument") throw new Error(t("An example rack requires an instrument channel"))
      await apply(exampleRack(name, channel.source.params), guard)
    })}>{t(name)}</Button>)}</div>
  </section>
}
