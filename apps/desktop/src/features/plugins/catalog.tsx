import { localizeBackendText } from "@/lib/i18n"
import { message as i18nMessage } from "@/lib/i18n"
import { translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useEffect, useId, useRef, useState } from "react"
import type { PluginDatabaseView, PluginRecord, PresetEntry } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Checkbox } from "@/components/ui/checkbox"
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { backend, errorMessage } from "@/lib/ipc"
import { insertPlugin, startPluginDrag } from "./insert"
import { applyPresetFile } from "@/features/project-files/actions"

const imageKey = "windfall.plugin-tile-images.v1"
function readImages(): Record<string, string> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(imageKey) ?? "{}")
    if (!value || typeof value !== "object" || Array.isArray(value)) return {}
    return Object.fromEntries(Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string" && /^data:image\/(png|jpeg|webp);base64,/.test(entry[1])))
  } catch { return {} }
}
function Artwork({ record, image }: { record: PluginRecord; image?: string }) {
  useLanguageStore((state) => state.language)
  const seed = [...`${record.name}:${record.vendor}:${record.instrument}`].reduce((hash, char) => ((hash * 31 + char.charCodeAt(0)) >>> 0), 7)
  if (image) return <img alt={i18nMessage("{name} artwork", { name: record.name })} src={image} className="h-24 w-full object-cover" />
  return <svg aria-label={i18nMessage("{value0} generated artwork", { value0: record.name })} role="img" viewBox="0 0 160 90" className="h-24 w-full bg-muted text-primary">
    <path fill="none" stroke="currentColor" strokeWidth="2" d={Array.from({ length: 32 }, (_, i) => `${i === 0 ? "M" : "L"}${i * 5},${45 + Math.sin(i * (1 + (seed % 9) / 10)) * (12 + seed % 24)}`).join(" ")} />
    <text x="10" y="80" fill="currentColor" fontSize="12">{record.instrument ? t("INSTRUMENT") : t("EFFECT")}</text>
  </svg>
}

/** The database browser and full-screen tile picker share the same persisted catalog. */
export function PluginCatalog({ tiles = false, track, searchLabel = t("Search plugins") }: { tiles?: boolean; track?: number; searchLabel?: string }) {
  useLanguageStore((state) => state.language)
  const id = useId()
  const [view, setView] = useState<PluginDatabaseView | null>(null)
  const [presets, setPresets] = useState<PresetEntry[]>([])
  const [search, setSearch] = useState("")
  const [favorites, setFavorites] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  const [images, setImages] = useState(readImages)
  useEffect(() => {
    let cancelled = false
    void Promise.all([backend.pluginDatabase(), backend.presetLibrary()]).then(([catalog, presets]) => { if (!cancelled) { setView(catalog); setPresets(presets) } }).catch((error: unknown) => { if (!cancelled) setError(errorMessage(error)) })
    return () => { cancelled = true }
  }, [])
  async function work(task: () => Promise<unknown>) {
    if (pending.current) return
    pending.current = true; setBusy(true); setError(null)
    try { await task() } catch (error) { setError(errorMessage(error)) } finally { pending.current = false; setBusy(false) }
  }
  const records = view?.records.filter((record) => (!favorites || record.favorite) && `${record.name} ${record.vendor} ${record.category} ${record.format} ${t(record.status)}`.toLowerCase().includes(search.toLowerCase())) ?? []
  return <div className="flex min-h-0 flex-col gap-3 p-2">
    <FieldGroup><Field><FieldLabel htmlFor={`${id}-search`}>{searchLabel}</FieldLabel><Input id={`${id}-search`} value={search} onChange={(event) => setSearch(event.target.value)} /></Field>
      <Field orientation="horizontal"><Checkbox id={`${id}-favorites`} checked={favorites} onCheckedChange={(checked) => setFavorites(checked === true)} /><FieldLabel htmlFor={`${id}-favorites`}>{t("Favorites only")}</FieldLabel></Field>
    </FieldGroup>
    {(error || view?.error) && <Alert><AlertTitle>{t("Plugin catalog")}</AlertTitle><AlertDescription>{localizeBackendText(error ?? view?.error ?? "")}</AlertDescription></Alert>}
    {!view && !error && <p role="status">{t("Loading plugins…")}</p>}
    {view && records.length === 0 && <Empty><EmptyHeader><EmptyTitle>{t("No matching plugins")}</EmptyTitle></EmptyHeader></Empty>}
    <div aria-label={tiles ? t("Plugin tiles") : t("Plugin database")} className={tiles ? "grid min-h-0 grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3 overflow-auto" : "flex flex-col gap-3"}>
      {records.map((record) => {
        const key = `${record.format}:${record.id}`
        const usable = record.status === "builtin" || record.status === "verified"
        return <section key={key} aria-label={record.name} className="flex flex-col gap-2 rounded border p-2" draggable={usable} onDragStart={(event) => startPluginDrag(event, record)} onDoubleClick={(event) => { if (usable && !(event.target instanceof Element && event.target.closest("button,input,label"))) void work(() => insertPlugin(record, { track: record.instrument ? undefined : track })) }}>
          {tiles && <Artwork record={record} image={images[key]} />}
          <div className="flex flex-wrap items-center gap-2"><h3>{record.name}</h3><Badge variant="outline">{t(record.status)}</Badge></div>
          <p className="text-muted-foreground">{record.vendor} · {record.instrument ? t("Instrument") : t("Effect")} · {record.category}</p>
          {record.detail && <p>{localizeBackendText(record.detail)}</p>}
          <div className="flex flex-wrap gap-2"><Button size="sm" variant="outline" disabled={busy || !usable} onClick={() => void work(() => insertPlugin(record, { track: record.instrument ? undefined : track }))}>{i18nMessage("Add {name}", { name: record.name })}</Button>
            <Button size="sm" variant="ghost" disabled={busy} aria-label={i18nMessage(record.favorite ? "Unfavorite {name}" : "Favorite {name}", { name: record.name })} onClick={() => void work(async () => setView(await backend.pluginDatabaseAnnotate(record.format, record.id, { favorite: !record.favorite, category: record.categorized ? record.category : null })))}>{record.favorite ? "★" : "☆"}</Button>
          </div>
          {!tiles && <Field><FieldLabel htmlFor={`${id}-category-${key}`}>{i18nMessage("Category for {name}", { name: record.name })}</FieldLabel><Input id={`${id}-category-${key}`} defaultValue={record.categorized ? record.category : ""} placeholder={record.category} maxLength={48} disabled={busy} onBlur={(event) => { const category = event.target.value.trim() || null; if (category !== (record.categorized ? record.category : null)) void work(async () => setView(await backend.pluginDatabaseAnnotate(record.format, record.id, { favorite: record.favorite, category }))) }} /></Field>}
          {tiles && <Field><FieldLabel htmlFor={`${id}-image-${key}`}>{i18nMessage("Tile image for {name}", { name: record.name })}</FieldLabel><Input id={`${id}-image-${key}`} type="file" accept="image/png,image/jpeg,image/webp" onChange={(event) => void work(async () => {
            const file = event.target.files?.[0]; if (!file) return
            if (file.size > 2 * 1024 * 1024 || !["image/png", "image/jpeg", "image/webp"].includes(file.type)) throw new Error(t("Choose a PNG, JPEG or WebP image smaller than 2 MiB"))
            const image = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(new Error(t("Could not read tile image"))); reader.readAsDataURL(file) })
            const next = { ...images, [key]: image }; localStorage.setItem(imageKey, JSON.stringify(next)); setImages(next)
          })} />{images[key] && <Button size="sm" variant="ghost" onClick={() => void work(async () => { const next = { ...images }; delete next[key]; localStorage.setItem(imageKey, JSON.stringify(next)); setImages(next) })}>{t("Use generated artwork")}</Button>}</Field>}
          {presets.filter((preset) => preset.plugin?.format === record.format && preset.plugin.id === record.id).map((preset) => <Button key={preset.path} variant="ghost" size="sm" onClick={() => void work(() => applyPresetFile(preset.path))}>{preset.name}</Button>)}
        </section>
      })}
    </div>
  </div>
}
