import { useEffect, useState } from "react"
import type { LoopCatalog, LoopEntry } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Empty, EmptyHeader, EmptyTitle, EmptyDescription } from "@/components/ui/empty"
import { songTick } from "@/features/playlist/ops"
import { backend, errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { useProjectGeneration } from "@/lib/store/replaced"
import { setLoopDrag, type LoopDrag } from "./drag"
import { insertFactoryLoop } from "./insert"
import { translatedText as t, message } from "@/lib/i18n"

const KEYS = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"]
type Option = { value: string; label: string }
function Choice({ label, value, options, change }: { label: string; value: string; options: Option[]; change(value: string): void }) {
  return <Field><FieldLabel>{label}</FieldLabel><Select items={options} value={value} onValueChange={(value) => { if (value !== null) change(value) }}><SelectTrigger aria-label={label}><SelectValue /></SelectTrigger><SelectContent alignItemWithTrigger={false}><SelectGroup>{options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectGroup></SelectContent></Select></Field>
}
export function LoopBrowser() {
  const generation = useProjectGeneration()
  const revision = useProjectStore((state) => state.revision)
  const tempo = useProjectStore((state) => state.project.settings.tempoBpm)
  const [catalog, setCatalog] = useState<LoopCatalog | null>(null)
  const [genre, setGenre] = useState("all")
  const [keyFilter, setKeyFilter] = useState("all")
  const [fitKey, setFitKey] = useState("source")
  const [minTempo, setMinTempo] = useState(40)
  const [maxTempo, setMaxTempo] = useState(300)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void backend.loopCatalog().then((catalog) => { if (live) { setCatalog(catalog); setError(null) } }, (error: unknown) => { if (live) setError(errorMessage(error)) })
    return () => { live = false }
  }, [generation, revision])
  const entries = catalog?.entries ?? []
  const genres = [...new Set(entries.map((entry) => entry.genre))].sort()
  const visible = entries.filter((entry) => (genre === "all" || entry.genre === genre) && (keyFilter === "all" || entry.key === Number(keyFilter)) && entry.tempo >= minTempo && entry.tempo <= maxTempo)
  function payload(entry: LoopEntry): LoopDrag | null {
    if (!catalog || catalog.revision !== revision) return null
    return { id: entry.id, name: entry.name, bars: entry.bars, key: fitKey === "source" ? null : Number(fitKey), generation: catalog.generation, revision: catalog.revision }
  }
  async function preview(entry: LoopEntry) {
    setBusy(true); setError(null)
    try { await backend.loopPreview(entry.id) } catch (error) { setError(errorMessage(error)) } finally { setBusy(false) }
  }
  async function insert(entry: LoopEntry) {
    const loop = payload(entry); if (!loop) return
    setBusy(true); try { await insertFactoryLoop(loop, songTick()) } finally { setBusy(false) }
  }
  return <div className="flex flex-col gap-3 p-3">
    <FieldGroup>
      <Field><FieldLabel>{t("Windfall starter loops")}</FieldLabel><FieldDescription>{t("Original synthesized CC0 content. Audition at the source tempo; Insert or drag to the playlist resynthesizes at")}{" "}{tempo}{" "}{t("BPM with independent key fit.")}</FieldDescription></Field>
      <Choice label={t("Genre")} value={genre} options={[{ value: "all", label: t("All genres") }, ...genres.map((genre) => ({ value: genre, label: genre }))]} change={setGenre} />
      <Choice label={t("Source key")} value={keyFilter} options={[{ value: "all", label: t("All keys") }, ...KEYS.map((key, index) => ({ value: String(index), label: message("{key} minor", { key }) }))]} change={setKeyFilter} />
      <div className="flex gap-2"><Field><FieldLabel htmlFor="loop-min-tempo">{t("Minimum BPM")}</FieldLabel><Input id="loop-min-tempo" type="number" min={40} max={300} value={minTempo} onChange={(event) => setMinTempo(Number(event.target.value))} /></Field><Field><FieldLabel htmlFor="loop-max-tempo">{t("Maximum BPM")}</FieldLabel><Input id="loop-max-tempo" type="number" min={40} max={300} value={maxTempo} onChange={(event) => setMaxTempo(Number(event.target.value))} /></Field></div>
      <Choice label={t("Insert key")} value={fitKey} options={[{ value: "source", label: t("Keep source key") }, ...KEYS.map((key, index) => ({ value: String(index), label: message("{key} minor", { key }) }))]} change={setFitKey} />
      <Button variant="outline" onClick={() => void backend.previewStop().catch((error: unknown) => setError(errorMessage(error)))}>{t("Stop preview")}</Button>
    </FieldGroup>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {!visible.length ? <Empty><EmptyHeader><EmptyTitle>{t("No matching loops")}</EmptyTitle><EmptyDescription>{t("Change genre, key or tempo filters. The desktop app generates the factory library.")}</EmptyDescription></EmptyHeader></Empty> : <ul className="flex flex-col gap-4">{visible.map((entry) => <li key={entry.id} draggable={!busy && catalog?.revision === revision} onDragStart={(event) => { const loop = payload(entry); if (loop) setLoopDrag(event, loop); else event.preventDefault() }} className="flex flex-col gap-2"><span>{entry.name} <Badge variant="secondary">{entry.genre}</Badge></span><span className="text-muted-foreground">{entry.tempo} BPM · {KEYS[entry.key]} {entry.mode} · {entry.bars}{" "}{t("bars")}</span><div className="flex gap-2"><Button variant="outline" disabled={busy} onClick={() => void preview(entry)}>{t("Audition")}{" "}{entry.name}</Button><Button disabled={busy || catalog?.revision !== revision} onClick={() => void insert(entry)}>{t("Insert")}{" "}{entry.name}</Button></div><details><summary>{t("Provenance")}</summary><p>{entry.provenance}</p></details></li>)}</ul>}
  </div>
}
