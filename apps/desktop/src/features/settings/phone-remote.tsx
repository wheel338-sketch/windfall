import { useEffect, useState } from "react"
import type { RemoteSettingsState } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { backend, errorMessage } from "@/lib/ipc"

export function PhoneRemoteSettings() {
  const [state, setState] = useState<RemoteSettingsState | null>(null)
  const [address, setAddress] = useState("127.0.0.1:8787")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void backend.phoneRemoteState().then((state) => { if (live) setState(state) }, (error: unknown) => { if (live) setError(errorMessage(error)) })
    return () => { live = false }
  }, [])
  async function change() {
    setBusy(true); setError(null)
    try { setState(await backend.phoneRemoteSet(state?.enabled ? null : address)) }
    catch (error) { setError(errorMessage(error)) }
    finally { setBusy(false) }
  }
  return <FieldSet>
    <FieldLegend>Phone remote</FieldLegend>
    <FieldGroup>
      <Field><FieldLabel htmlFor="remote-address">Local IPv4 address and port</FieldLabel>
        <Input id="remote-address" value={address} disabled={busy || state?.enabled === true} onChange={(event) => setAddress(event.target.value)} />
        <FieldDescription>Enter this computer’s LAN address for a phone, for example 192.168.1.4:8787. Loopback works only on this computer. The listener starts only when enabled.</FieldDescription>
      </Field>
      <Button variant="outline" disabled={busy || state === null} onClick={() => void change()}>{state?.enabled ? "Disable phone remote" : "Enable phone remote"}</Button>
      {state?.enabled && <Field><FieldLabel>Pair your phone</FieldLabel><FieldDescription>Open {state.address} on the same local network. Pairing code: <strong>{state.pairingCode}</strong>. The code expires after five minutes and pairs one device. Disable and enable for a fresh session.</FieldDescription></Field>}
      {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    </FieldGroup>
  </FieldSet>
}
