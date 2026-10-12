import { useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { nativeUpdateApi, type UpdateApi, type UpdateChannel, type UpdateInfo, type UpdateProgress } from "./native"

const CHANNEL_KEY = "windfall.update-channel"
function savedChannel(): UpdateChannel {
  try { return localStorage.getItem(CHANNEL_KEY) === "preview" ? "preview" : "stable" }
  catch { return "stable" }
}
export function UpdatesSection({ api = nativeUpdateApi }: { api?: UpdateApi }) {
  const [channel, setChannel] = useState(savedChannel)
  const [candidate, setCandidate] = useState<UpdateInfo | null>(null)
  const [progress, setProgress] = useState<UpdateProgress | null>(null)
  const [phase, setPhase] = useState<"idle" | "checking" | "available" | "downloading" | "verified" | "installing">("idle")
  const [message, setMessage] = useState("")
  const [error, setError] = useState("")
  const generation = useRef(0)
  const pending = useRef(false)
  const dirty = useProjectStore((state) => state.dirty)
  const playing = useTransportStore((state) => state.playing)
  const busy = phase === "checking" || phase === "downloading" || phase === "installing"
  useEffect(() => () => { generation.current += 1 }, [])
  async function work(action: () => Promise<void>) {
    if (pending.current) return
    pending.current = true
    const request = generation.current
    setError("")
    try { await action() }
    catch (failure) {
      if (request === generation.current) {
        setError(errorMessage(failure))
        setPhase(candidate ? "available" : "idle")
        setProgress(null)
      }
    } finally { pending.current = false }
  }
  const channels = [{ value: "stable", label: "Stable" }, { value: "preview", label: "Preview" }]
  return <section aria-label="Updates" className="flex flex-col gap-2">
    <h3 className="text-xs font-medium">Updates</h3>
    <FieldGroup>
      <Field>
        <FieldLabel>Update channel</FieldLabel>
        <Select items={channels} value={channel} disabled={busy} onValueChange={(value: UpdateChannel | null) => {
          if (value === null) return
          generation.current += 1
          setChannel(value); setCandidate(null); setProgress(null); setMessage(""); setError(""); setPhase("idle")
          try { localStorage.setItem(CHANNEL_KEY, value) } catch { /* Session choice still works. */ }
        }}>
          <SelectTrigger aria-label="Update channel"><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup>{channels.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>
        <FieldDescription>Stable releases by default. Preview releases may contain unfinished work.</FieldDescription>
      </Field>
      <FieldDescription>Updates never rewrite project files. A release declaring a different project format requires a separate manual upgrade. Save and stop before installing.</FieldDescription>
      <Button variant="outline" size="sm" disabled={busy} onClick={() => void work(async () => {
        const request = generation.current
        setPhase("checking"); setCandidate(null); setProgress(null); setMessage("")
        const result = await api.check(channel)
        if (request !== generation.current) return
        setCandidate(result.update)
        setPhase(result.update ? "available" : "idle")
        setMessage(result.configured ? result.update ? `Version ${result.update.version} is available.` : "You are up to date." : result.reason ?? "The operator has not configured updates.")
      })}>Check for updates</Button>
      {message && <p role="status">{message}</p>}
      {candidate && <>
        <p className="max-h-32 overflow-auto whitespace-pre-wrap text-muted-foreground">{candidate.notes}</p>
        <Button variant="outline" size="sm" disabled={busy || phase === "verified"} onClick={() => void work(async () => {
          const request = generation.current
          setPhase("downloading"); setProgress(null)
          let verified = false
          await api.download(candidate.token, (next) => {
            if (request !== generation.current) return
            verified = next.verified
            setProgress(next)
          })
          if (request !== generation.current) return
          if (!verified) throw new Error("The package was not verified. Nothing was installed.")
          setPhase("verified")
        })}>Download and verify</Button>
        {progress && <p role="status">{progress.verified ? "Signature verified. Ready to install." : `Downloaded ${progress.downloaded.toLocaleString()}${progress.total === null ? "" : ` of ${progress.total.toLocaleString()}`} bytes.`}</p>}
        <Button size="sm" disabled={phase !== "verified" || dirty || playing} onClick={() => void work(async () => {
          if (useProjectStore.getState().dirty || useTransportStore.getState().playing) throw new Error("Save your project and stop playback first.")
          setPhase("installing")
          await api.install(candidate.token)
          setMessage("Installed. Restart Windfall to use the update."); setPhase("idle"); setCandidate(null)
        })}>Install and restart</Button>
        {(dirty || playing) && <FieldDescription>Save your project and stop playback to enable installation.</FieldDescription>}
      </>}
      {busy && <p role="status">{phase === "checking" ? "Checking…" : phase === "installing" ? "Installing…" : "Downloading…"}</p>}
      {error && <FieldError role="alert">{error}</FieldError>}
    </FieldGroup>
  </section>
}
