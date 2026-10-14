import { useEffect, useState } from "react"
import { backend, errorMessage } from "@/lib/ipc"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Field, FieldLabel, FieldDescription } from "@/components/ui/field"
import { parseManifest } from "@/features/analysis/manifest"
import type { AnalysisModel, AnalysisRuntimeSettings } from "@/features/analysis/types"
import { localizeBackendText, translatedText as t } from "@/lib/i18n"

export function StemSeparationSettings() {
  const [settings, setSettings] = useState<AnalysisRuntimeSettings>({ libraryPath: null, modelId: null, modelVersion: null })
  const [models, setModels] = useState<AnalysisModel[]>([])
  const [native, setNative] = useState(false)
  const [path, setPath] = useState("")
  const [manifest, setManifest] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [status, setStatus] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void Promise.all([backend.analysisRuntimeSettings(), backend.analysisCapability()]).then(([settings, capability]) => {
      if (!live) return
      setSettings(settings); setNative(capability.native); setModels(capability.models); setStatus(capability.reason && localizeBackendText(capability.reason))
    }).catch((e: unknown) => { if (live) setError(errorMessage(e)) })
    return () => { live = false }
  }, [])
  async function run(action: () => Promise<void>) {
    setBusy(true); setError(null)
    try { await action() } catch (e) { setError(errorMessage(e)) } finally { setBusy(false) }
  }
  return <section aria-label={t("Stem separation settings")} className="flex flex-col gap-2">
    <h3 className="text-xs font-medium">{t("Stem separation")}</h3>
    {status && <p role="status" className="text-xs text-muted-foreground">{status}</p>}
    {error && <p role="alert">{error}</p>}
    <fieldset disabled={!native || busy} className="flex flex-col gap-2">
      <Field><FieldLabel htmlFor="stem-runtime">{t("ONNX Runtime library")}</FieldLabel>
        <Input id="stem-runtime" value={settings.libraryPath ?? ""} maxLength={2048}
          onChange={(e) => setSettings({ ...settings, libraryPath: e.target.value || null })} />
        <Button variant="outline" onClick={() => void run(async () => {
          const libraryPath = await backend.pickAnalysisRuntime()
          if (libraryPath) setSettings((s) => ({ ...s, libraryPath }))
        })}>{t("Choose runtime library")}</Button>
        <FieldDescription>{t("Install the CPU runtime yourself. Nothing is downloaded. Restart Windfall after changing a library that has already been loaded.")}</FieldDescription>
      </Field>
      <Field><FieldLabel htmlFor="stem-pack">{t("Default model pack")}</FieldLabel>
        <select id="stem-pack" value={JSON.stringify([settings.modelId, settings.modelVersion])}
          onChange={(e) => {
            const [modelId, modelVersion] = JSON.parse(e.target.value) as [string | null, string | null]
            setSettings({ ...settings, modelId, modelVersion })
          }}>
          <option value="[null,null]">{t("Choose a separation pack")}</option>
          {models.filter((m) => m.provenance.adapterId === "windfall-onnx-separation").map((m) =>
            <option key={`${m.id}/${m.version}`} value={JSON.stringify([m.id, m.version])}>{m.id} {m.version}</option>)}
        </select>
      </Field>
      <Button onClick={() => void run(async () => {
        setSettings(await backend.analysisRuntimeConfigure(settings))
        const { reason } = await backend.analysisCapability()
        setStatus(reason ? localizeBackendText(reason) : t("Saved. Separate audio from the playlist or browser."))
      })}>{t("Save separation settings")}</Button>
      <Field><FieldLabel htmlFor="stem-weights">{t("Import local ONNX model")}</FieldLabel>
        <Input id="stem-weights" value={path} maxLength={2048} onChange={(e) => setPath(e.target.value)} />
        <Button variant="outline" onClick={() => void run(async () => {
          const chosen = await backend.pickAnalysisModel(); if (chosen) setPath(chosen)
        })}>{t("Choose model weights")}</Button>
        <FieldDescription>{t("Paste the pack's manifest JSON below. The checksum, tensor layout and stem order are verified. Review the declared weight license before importing.")}</FieldDescription>
      </Field>
      <textarea aria-label={t("Separation model manifest JSON")} value={manifest} maxLength={16384}
        className="min-h-24 rounded border border-input bg-background p-2 font-mono text-xs"
        onChange={(e) => setManifest(e.target.value)} />
      <Button disabled={!path || !manifest} onClick={() => void run(async () => {
        const model = parseManifest(manifest)
        if (model.provenance.adapterId !== "windfall-onnx-separation") throw new Error(t("Choose a waveform ONNX separation pack."))
        const imported = await backend.analysisModelImport(path, model)
        setModels((await backend.analysisCapability()).models)
        setSettings((s) => ({ ...s, modelId: imported.id, modelVersion: imported.version }))
        setStatus(t("Model imported. Save to use it as the default separation pack."))
      })}>{t("Verify and import separation pack")}</Button>
    </fieldset>
  </section>
}
