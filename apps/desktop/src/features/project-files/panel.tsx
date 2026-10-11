import { ScoreFileControls } from "./score-controls"
import { useEffect, useRef, useState } from "react"
import type { PresetEntry, TemplateInfo } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { backend, errorMessage } from "@/lib/ipc"
import { applyPresetFile, newFromTemplate, saveSelectedPreset, saveTemplate } from "./actions"

/** A disk preset browser and template library, with clear native capability errors. */
export function ProjectFilesPanel() {
  const [presets, setPresets] = useState<PresetEntry[]>([])
  const [templates, setTemplates] = useState<TemplateInfo[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  useEffect(() => {
    let cancelled = false
    void Promise.allSettled([backend.presetLibrary(), backend.templates()]).then(([presets, templates]) => {
      if (cancelled) return
      if (presets.status === "fulfilled") setPresets(presets.value)
      if (templates.status === "fulfilled") setTemplates(templates.value)
      const errors = [presets, templates].filter((result) => result.status === "rejected").map((result) => errorMessage(result.reason))
      if (errors.length) setError(errors.join(" "))
    })
    return () => { cancelled = true }
  }, [])
  async function work(task: () => Promise<unknown>) {
    if (pending.current) return
    pending.current = true; setBusy(true); setError(null)
    try {
      await task()
      const [presets, templates] = await Promise.allSettled([backend.presetLibrary(), backend.templates()])
      if (presets.status === "fulfilled") setPresets(presets.value)
      if (templates.status === "fulfilled") setTemplates(templates.value)
    } catch (error) { setError(errorMessage(error)) } finally { pending.current = false; setBusy(false) }
  }
  return <div className="flex flex-col gap-3 p-2">
    {error && <Alert><AlertTitle>Project files</AlertTitle><AlertDescription>{error}</AlertDescription></Alert>}
    <ScoreFileControls />
    <section aria-label="Preset files" className="flex flex-col gap-2"><h2>Presets</h2>
      <Button disabled={busy} variant="outline" onClick={() => void work(() => applyPresetFile())}>Browse preset files on disk…</Button>
      <Button disabled={busy} variant="outline" onClick={() => void work(() => saveSelectedPreset(false, true))}>Save channel to preset library</Button>
      <Button disabled={busy} variant="outline" onClick={() => void work(() => saveSelectedPreset(true, true))}>Save plugin to preset library</Button>
      {presets.map((preset) => <div key={preset.path} className="flex flex-wrap gap-2"><Button disabled={busy} variant="ghost" onClick={() => void work(() => applyPresetFile(preset.path))}>{preset.name} · {preset.kind}</Button><Button disabled={busy} size="sm" variant="outline" onClick={() => void work(() => backend.presetLibraryDelete(preset.path))}>Delete {preset.name}</Button></div>)}
    </section>
    <section aria-label="Project templates" className="flex flex-col gap-2"><h2>Templates</h2>
      <Button disabled={busy} variant="outline" onClick={() => void work(saveTemplate)}>Save current project as template</Button>
      {templates.map((template) => <div key={template.id} className="flex flex-col gap-1"><Button disabled={busy} variant="ghost" onClick={() => void work(() => newFromTemplate(template.id))}>{template.name}{template.factory ? " · Factory" : ""}</Button><p className="text-muted-foreground">{template.description}</p>{!template.factory && <Button disabled={busy} variant="outline" size="sm" onClick={() => void work(() => backend.templateDelete(template.id))}>Delete {template.name}</Button>}</div>)}
    </section>
  </div>
}
