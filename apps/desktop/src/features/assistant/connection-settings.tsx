import { useEffect, useState } from "react"
import type { AssistantConnection } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend } from "@/lib/ipc"
import { errorMessage } from "@/lib/ipc/backend"
import { translatedText as t } from "@/lib/i18n"

export function ConnectionSettings({ requestBusy, onSaved }: { requestBusy: boolean; onSaved(connection: AssistantConnection): void }) {
  const [baseUrl, setBaseUrl] = useState("")
  const [model, setModel] = useState("")
  const [key, setKey] = useState("")
  const [enabled, setEnabled] = useState(false)
  const [hasKey, setHasKey] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let active = true
    void backend.assistantConnection().then((connection) => {
      if (active) { setBaseUrl(connection.baseUrl); setModel(connection.model); setEnabled(connection.enabled); setHasKey(connection.hasKey); onSaved(connection) }
    }).catch((failure: unknown) => { if (active) setError(errorMessage(failure)) })
    return () => { active = false }
  }, [onSaved])
  async function save(clear: boolean) {
    if (busy || requestBusy) return
    setBusy(true); setError(null)
    try {
      const connection = await backend.assistantConfigure({ baseUrl, model, enabled: clear ? false : enabled, apiKey: clear ? "" : key.trim() || null })
      setKey(""); setEnabled(connection.enabled); setHasKey(connection.hasKey); onSaved(connection)
    } catch (failure) { setError(errorMessage(failure)) }
    finally { setBusy(false) }
  }
  const disabled = busy || requestBusy || backend.kind !== "tauri"
  return <details>
    <summary>{t("Bring your own key connection")}</summary>
    <div className="mt-3 flex flex-col gap-3">
      <p>{hasKey ? t("A key is stored; its value is never returned to this panel.") : t("OFF until you enter a key and enable this connection.")}</p>
      <FieldGroup>
        <Field><FieldLabel htmlFor="assistant-base-url">{t("Chat completions base URL")}</FieldLabel><Input id="assistant-base-url" value={baseUrl} maxLength={2048} disabled={disabled} placeholder="https://your-service.example/v1" onChange={(event) => setBaseUrl(event.target.value)} /><FieldDescription>{t("HTTPS, or literal loopback HTTP for a local service. /chat/completions is appended when needed.")}</FieldDescription></Field>
        <Field><FieldLabel htmlFor="assistant-model">{t("Model name")}</FieldLabel><Input id="assistant-model" value={model} maxLength={256} disabled={disabled} onChange={(event) => setModel(event.target.value)} /></Field>
        <Field><FieldLabel htmlFor="assistant-key">{t("API key")}</FieldLabel><Input id="assistant-key" type="password" autoComplete="off" value={key} maxLength={4096} disabled={disabled} onChange={(event) => setKey(event.target.value)} /><FieldDescription>{t("Leave blank to keep an existing key. This version uses a private per-user settings file outside the project, with plaintext credential storage; no OS credential-store adapter is installed.")}</FieldDescription></Field>
        <Field><FieldLabel htmlFor="assistant-enable">{t("Enable BYOK requests")}</FieldLabel><Checkbox id="assistant-enable" checked={enabled} disabled={disabled} onCheckedChange={setEnabled} /><FieldDescription>{t("Saving never sends a request. Only Ask BYOK assistant contacts this endpoint. Offline help remains available.")}</FieldDescription></Field>
      </FieldGroup>
      {backend.kind !== "tauri" && <Alert><AlertDescription>{t("BYOK connection settings and requests require the native app.")}</AlertDescription></Alert>}
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
      <div className="flex gap-2"><Button variant="outline" disabled={disabled} onClick={() => void save(false)}>{t("Save BYOK connection")}</Button><Button variant="ghost" disabled={disabled || !hasKey} onClick={() => void save(true)}>{t("Remove key and turn OFF")}</Button></div>
    </div>
  </details>
}
