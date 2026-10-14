import { localizeBackendText } from "@/lib/i18n"
import { message as localizedMessage, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Switch } from "@/components/ui/switch"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { automaticChecks, setAutomaticChecks } from "./schedule"
import { checkForUpdates, downloadUpdate, startInstall, updateBusy, useUpdateStore, type UpdateStatus } from "./store"
import type { UpdateProgress } from "./native"

export function statusText(status: UpdateStatus): string {
  return "key" in status ? localizedMessage(status.key, status.values) : localizeBackendText(status.source)
}
export function progressText(progress: UpdateProgress): string {
  if (progress.verified) return t("Signature verified. Ready to install.")
  return progress.total === null
    ? localizedMessage("Downloaded {downloaded} bytes.", { downloaded: progress.downloaded })
    : localizedMessage("Downloaded {downloaded} of {total} bytes.", { downloaded: progress.downloaded, total: progress.total })
}

/** Settings → Updates: automatic checks, a check now, and the found update's notes and steps. */
export function UpdatesSection() {
  useLanguageStore((state) => state.language)
  const { phase, candidate, progress, status, error } = useUpdateStore()
  const [automatic, setAutomatic] = useState(automaticChecks)
  const dirty = useProjectStore((state) => state.dirty)
  const playing = useTransportStore((state) => state.playing)
  const busy = updateBusy(phase)
  return <section aria-label={t("Updates")} className="flex flex-col gap-2">
    <h3 className="text-xs font-medium">{t("Updates")}</h3>
    <FieldGroup>
      <Field orientation="horizontal">
        <FieldLabel htmlFor="updates-automatic">{t("Check for updates automatically")}</FieldLabel>
        <Switch id="updates-automatic" checked={automatic} onCheckedChange={(on) => { setAutomaticChecks(on); setAutomatic(on) }} />
      </Field>
      <FieldDescription>{t("Once a day when Windfall starts, in the background. Nothing is downloaded until you choose to install.")}</FieldDescription>
      <FieldDescription>{t("Updates never rewrite project files. A release declaring a different project format requires a separate manual upgrade. Save and stop before installing.")}</FieldDescription>
      <Button variant="outline" size="sm" disabled={busy} onClick={() => void checkForUpdates("manual")}>{t("Check for updates")}</Button>
      {status && <p role="status">{statusText(status)}</p>}
      {candidate && <>
        <p className="max-h-32 overflow-auto whitespace-pre-wrap text-muted-foreground">{candidate.notes}</p>
        <Button variant="outline" size="sm" disabled={busy || phase === "verified"} onClick={() => void downloadUpdate()}>{t("Download and verify")}</Button>
        {progress && <p role="status">{progressText(progress)}</p>}
        <Button size="sm" disabled={phase !== "verified"} onClick={() => void startInstall()}>{t("Install and restart")}</Button>
        {(dirty || playing) && <FieldDescription>{t("Installation freezes edits, resolves unsaved work and stops audio before launching the installer.")}</FieldDescription>}
      </>}
      {busy && <p role="status">{phase === "checking" ? t("Checking…") : phase === "installing" ? t("Installing…") : t("Downloading…")}</p>}
      {error && <FieldError role="alert">{localizeBackendText(error)}</FieldError>}
    </FieldGroup>
  </section>
}
