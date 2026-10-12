import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { errorMessage } from "@/lib/ipc"

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
      const { invoke } = await import("@tauri-apps/api/core")
      await invoke("export_plugin_bundle", { destination: destination.trim(), runtime: runtime.trim() })
      setComplete(true)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setBusy(false)
    }
  }
  return (
    <section aria-label="Export as a plugin" className="space-y-3 rounded-md border p-3">
      <p className="text-sm font-medium">Export as a plugin</p>
      <p className="text-xs text-muted-foreground">Create VST3 and CLAP bundles with this project and its samples. The receiving host controls transport, tempo and meter. Native plugin bindings and hardware recording routes must be removed before export.</p>
      <Field>
        <FieldLabel htmlFor="plugin-export-destination">New destination folder</FieldLabel>
        <Input id="plugin-export-destination" value={destination} disabled={busy} onChange={(event) => { setDestination(event.target.value); setComplete(false) }} />
      </Field>
      <Field>
        <FieldLabel htmlFor="plugin-export-runtime">Windfall plugin runtime file</FieldLabel>
        <Input id="plugin-export-runtime" value={runtime} disabled={busy} onChange={(event) => { setRuntime(event.target.value); setComplete(false) }} />
        <p className="text-xs text-muted-foreground">Choose windfall_plugin.dll, libwindfall_plugin.so or libwindfall_plugin.dylib from the Windfall distribution.</p>
      </Field>
      <Button type="button" variant="outline" disabled={busy} onClick={() => void exportPlugin()}>{busy ? "Packaging…" : "Export plugin bundles"}</Button>
      {error && <FieldError>{error}</FieldError>}
      {complete && <p role="status" className="text-sm">VST3 and CLAP bundles saved to {destination}.</p>}
    </section>
  )
}
