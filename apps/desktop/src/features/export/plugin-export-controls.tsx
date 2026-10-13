import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"
import { translatedText as t } from "@/lib/i18n"

/** Native bundle export has its own file lifecycle and reports real capability errors. */
export function PluginExportControls() {
  const [destination, setDestination] = useState("")
  const [runtime, setRuntime] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [complete, setComplete] = useState(false)
  async function exportPlugin() {
    setError(null)
    setComplete(false)
    if (!("__TAURI_INTERNALS__" in window)) {
      setError(t("Plugin bundle export requires the desktop app."))
      return
    }
    if (!destination.trim()) {
      setError(t("Enter a new destination folder."))
      return
    }
    setBusy(true)
    try {
      await backend.exportPluginBundle(destination.trim(), runtime.trim())
      setComplete(true)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setBusy(false)
    }
  }
  return (
    <section aria-label={t("Export as a plugin")} className="flex flex-col gap-3 rounded-md border p-3">
      <p className="text-sm font-medium">{t("Export as a plugin")}</p>
      <p className="text-xs text-muted-foreground">{t("Create VST3 and CLAP bundles with this project and its samples. The receiving host controls transport, tempo and meter. Nested CLAP/VST3 use the bundled audio helper; their plugin files must be installed on the receiving machine. Hardware recording routes cannot be exported.")}</p>
      <FieldGroup>
      <Field>
        <FieldLabel htmlFor="plugin-export-destination">{t("New destination folder")}</FieldLabel>
        <Input id="plugin-export-destination" value={destination} disabled={busy} onChange={(event) => { setDestination(event.target.value); setComplete(false) }} />
      </Field>
      <Field>
        <FieldLabel htmlFor="plugin-export-runtime">{t("Windfall plugin runtime file")}</FieldLabel>
        <Input id="plugin-export-runtime" value={runtime} disabled={busy} onChange={(event) => { setRuntime(event.target.value); setComplete(false) }} />
        <p className="text-xs text-muted-foreground">{t("Optional override. Leave blank to use the runtime included with Windfall. Its ABI, version and architecture are checked before export.")}</p>
      </Field>
      </FieldGroup>
      <Button type="button" variant="outline" disabled={busy} onClick={() => void exportPlugin()}>{busy ? t("Packaging…") : t("Export plugin bundles")}</Button>
      {error && <FieldError>{error}</FieldError>}
      {complete && <p role="status" className="text-sm">{t("VST3 and CLAP bundles saved to")}{" "}{destination}.</p>}
    </section>
  )
}
