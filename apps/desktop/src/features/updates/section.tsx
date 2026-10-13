import { localizeBackendText } from "@/lib/i18n"
import { message as localizedMessage, translatedText as t, useLanguageStore } from "@/lib/i18n"
import type { MessageKey } from "@/lib/i18n"
import { useEffect, useRef, useState } from "react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { backend, errorMessage } from "@/lib/ipc"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { nativeUpdateApi, type UpdateApi, type UpdateChannel, type UpdateInfo, type UpdateProgress } from "./native"

const CHANNEL_KEY = "windfall.update-channel"
function savedChannel(): UpdateChannel {
  try { return localStorage.getItem(CHANNEL_KEY) === "preview" ? "preview" : "stable" }
  catch { return "stable" }
}
export function UpdatesSection({ api = nativeUpdateApi }: { api?: UpdateApi }) {
  useLanguageStore((state) => state.language)
  const [channel, setChannel] = useState(savedChannel)
  const [candidate, setCandidate] = useState<UpdateInfo | null>(null)
  const [progress, setProgress] = useState<UpdateProgress | null>(null)
  const [phase, setPhase] = useState<"idle" | "checking" | "available" | "downloading" | "verified" | "installing">("idle")
  const [status, setStatus] = useState<{ key: MessageKey; values?: Record<string, string | number> } | { source: string } | null>(null)
  const [error, setError] = useState("")
  const generation = useRef(0)
  const pending = useRef(false)
  const shutdown = useRef<number | null>(null)
  const [unsaved, setUnsaved] = useState<{ token: number; path: string | null } | null>(null)
  const dirty = useProjectStore((state) => state.dirty)
  const playing = useTransportStore((state) => state.playing)
  const busy = phase === "checking" || phase === "downloading" || phase === "installing"
  useEffect(() => () => { generation.current += 1; if (shutdown.current !== null) void api.cancel(shutdown.current).catch(() => {}) }, [api])
  async function work(action: () => Promise<void>) {
    if (pending.current) return
    pending.current = true
    const request = generation.current
    setError("")
    try { await action() }
    catch (failure) {
      if (request === generation.current) {
        setError(errorMessage(failure))
        if (shutdown.current !== null) {
          try { await api.cancel(shutdown.current) }
          catch (rollback) { setError(errorMessage(rollback)) }
          shutdown.current = null; setUnsaved(null)
        }
        setPhase(candidate ? progress?.verified ? "verified" : "available" : "idle")
        if (!progress?.verified) setProgress(null)
      }
    } finally { pending.current = false }
  }
  async function install(token: number, decision: "save" | "discard", path?: string) {
    if (decision === "save" && !path) {
      const picked = await backend.pickProjectSavePath(useProjectStore.getState().project.settings.name)
      if (picked === null) { await api.cancel(token); shutdown.current = null; setUnsaved(null); setPhase("verified"); return }
      path = picked
    }
    await api.install(token, decision, path)
    shutdown.current = null; setUnsaved(null)
    setStatus({ key: "Installed. Restart Windfall to use the update." }); setPhase("idle"); setCandidate(null)
  }
  async function cancel() {
    if (shutdown.current !== null) await api.cancel(shutdown.current)
    shutdown.current = null; setUnsaved(null); setPhase("verified")
  }
  const channels = [{ value: "stable", label: t("Stable") }, { value: "preview", label: t("Preview") }]
  return <section aria-label={t("Updates")} className="flex flex-col gap-2">
    <h3 className="text-xs font-medium">{t("Updates")}</h3>
    <FieldGroup>
      <Field>
        <FieldLabel>{t("Update channel")}</FieldLabel>
        <Select items={channels} value={channel} disabled={busy} onValueChange={(value: UpdateChannel | null) => {
          if (value === null) return
          generation.current += 1
          setChannel(value); setCandidate(null); setProgress(null); setStatus(null); setError(""); setPhase("idle")
          try { localStorage.setItem(CHANNEL_KEY, value) } catch { /* Session choice still works. */ }
        }}>
          <SelectTrigger aria-label={t("Update channel")}><SelectValue /></SelectTrigger>
          <SelectContent><SelectGroup>{channels.map((item) => <SelectItem key={item.value} value={item.value}>{t(item.label)}</SelectItem>)}</SelectGroup></SelectContent>
        </Select>
        <FieldDescription>{t("Stable releases by default. Preview releases may contain unfinished work.")}</FieldDescription>
      </Field>
      <FieldDescription>{t("Updates never rewrite project files. A release declaring a different project format requires a separate manual upgrade. Save and stop before installing.")}</FieldDescription>
      <Button variant="outline" size="sm" disabled={busy} onClick={() => void work(async () => {
        const request = generation.current
        setPhase("checking"); setCandidate(null); setProgress(null); setStatus(null)
        const result = await api.check(channel)
        if (request !== generation.current) return
        setCandidate(result.update)
        setPhase(result.update ? "available" : "idle")
        setStatus(result.configured ? result.update ? { key: "Version {version} is available.", values: { version: result.update.version } } : { key: "You are up to date." } : result.reason ? { source: result.reason } : { key: "The operator has not configured updates." })
      })}>{t("Check for updates")}</Button>
      {status && <p role="status">{"key" in status ? localizedMessage(status.key, status.values) : localizeBackendText(status.source)}</p>}
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
          if (!verified) throw new Error(localizedMessage("The package was not verified. Nothing was installed.", {}, "en"))
          setPhase("verified")
        })}>{t("Download and verify")}</Button>
        {progress && <p role="status">{progress.verified ? t("Signature verified. Ready to install.") : progress.total === null ? localizedMessage("Downloaded {downloaded} bytes.", { downloaded: progress.downloaded }) : localizedMessage("Downloaded {downloaded} of {total} bytes.", { downloaded: progress.downloaded, total: progress.total })}</p>}
        <Button size="sm" disabled={phase !== "verified"} onClick={() => void work(async () => {
          setPhase("installing")
          const snapshot = await api.begin(candidate.token)
          shutdown.current = candidate.token
          if (snapshot.dirty) setUnsaved({ token: candidate.token, path: snapshot.path })
          else await install(candidate.token, "discard")
        })}>{t("Install and restart")}</Button>
        {(dirty || playing) && <FieldDescription>{t("Installation freezes edits, resolves unsaved work and stops audio before launching the installer.")}</FieldDescription>}
      </>}
      {busy && <p role="status">{phase === "checking" ? t("Checking…") : phase === "installing" ? t("Installing…") : t("Downloading…")}</p>}
      {error && <FieldError role="alert">{localizeBackendText(error)}</FieldError>}
      <Dialog open={unsaved !== null} onOpenChange={(open) => { if (!open) void work(cancel) }}>
        <DialogContent>
          <DialogTitle>{t("Unsaved project")}</DialogTitle>
          <DialogDescription>{t("Edits are frozen while you decide. Save your work, discard it for installation, or cancel and continue working.")}</DialogDescription>
          <Button onClick={() => void work(async () => { if (unsaved) await install(unsaved.token, "save", unsaved.path ?? undefined) })}>{t("Save and install")}</Button>
          <Button variant="outline" onClick={() => void work(async () => { if (unsaved) await install(unsaved.token, "discard") })}>{t("Discard and install")}</Button>
          <Button variant="outline" onClick={() => void work(cancel)}>{t("Cancel")}</Button>
        </DialogContent>
      </Dialog>
    </FieldGroup>
  </section>
}
