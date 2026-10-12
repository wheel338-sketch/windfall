import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"

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
      setError("Plugin bundle export requires the desktop app.")
      return
    }
    if (!destination.trim() || !runtime.trim()) {
      setError("Enter a new destination folder and the Windfall plugin runtime file.")
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
    <section aria-label="Export as a plugin" className="flex flex-col gap-3 rounded-md border p-3">
      <p className="text-sm font-medium">Export as a plugin</p>
      <p className="text-xs text-muted-foreground">Create VST3 and CLAP bundles with this project and its samples. The receiving host controls transport, tempo and meter. Native plugin bindings and hardware recording routes must be removed before export.</p>
      <FieldGroup>
      <Field>
        <FieldLabel htmlFor="plugin-export-destination">New destination folder</FieldLabel>
        <Input id="plugin-export-destination" value={destination} disabled={busy} onChange={(event) => { setDestination(event.target.value); setComplete(false) }} />
      </Field>
      <Field>
        <FieldLabel htmlFor="plugin-export-runtime">Windfall plugin runtime file</FieldLabel>
        <Input id="plugin-export-runtime" value={runtime} disabled={busy} onChange={(event) => { setRuntime(event.target.value); setComplete(false) }} />
        <p className="text-xs text-muted-foreground">Choose windfall_plugin.dll, libwindfall_plugin.so or libwindfall_plugin.dylib from the Windfall distribution.</p>
      </Field>
      </FieldGroup>
      <Button type="button" variant="outline" disabled={busy} onClick={() => void exportPlugin()}>{busy ? "Packaging…" : "Export plugin bundles"}</Button>
      {error && <FieldError>{error}</FieldError>}
      {complete && <p role="status" className="text-sm">VST3 and CLAP bundles saved to {destination}.</p>}
    </section>
  )
}
