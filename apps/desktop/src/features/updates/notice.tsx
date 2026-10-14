import { useEffect } from "react"
import { localizeBackendText, message as localizedMessage, translatedText as t, useLanguageStore } from "@/lib/i18n"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { notesSummary } from "./schedule"
import { progressText } from "./section"
import { dismissNotice, installNow, releaseUpdate, resolveUnsaved, startAutomaticChecks, updateBusy, useUpdateStore } from "./store"

/**
 * A found update, in a corner of the window: its version, the start of its
 * notes and the choice to install now or later. It does not take the focus
 * or stop the user working.
 */
export function UpdateNotice() {
  useLanguageStore((state) => state.language)
  const { notice, candidate, phase, progress, error, unsaved } = useUpdateStore()
  if (!notice || !candidate) return null
  const busy = updateBusy(phase) || unsaved !== null
  const summary = notesSummary(candidate.notes)
  return <section aria-label={t("Update available")} className="fixed bottom-9 left-3 z-50 flex w-80 flex-col gap-2 rounded-md border bg-popover p-3 text-xs text-popover-foreground shadow-lg">
    <h3 className="font-medium">{localizedMessage("Windfall {version} is available", { version: candidate.version })}</h3>
    {summary && <p className="max-h-24 overflow-auto whitespace-pre-wrap text-muted-foreground">{summary}</p>}
    {progress && <p role="status">{progressText(progress)}</p>}
    {phase === "downloading" && !progress && <p role="status">{t("Downloading…")}</p>}
    {phase === "installing" && <p role="status">{t("Installing…")}</p>}
    {error && <p role="alert" className="text-destructive">{localizeBackendText(error)}</p>}
    <div className="flex justify-end gap-2">
      <Button size="sm" variant="ghost" disabled={busy} onClick={dismissNotice}>{t("Later")}</Button>
      <Button size="sm" disabled={busy} onClick={() => void installNow()}>{t("Install now")}</Button>
    </div>
  </section>
}

/** The unsaved-project prompt before installing. Edits stay frozen until it is answered. */
export function UpdateDecision() {
  useLanguageStore((state) => state.language)
  const unsaved = useUpdateStore((state) => state.unsaved)
  return <Dialog open={unsaved !== null} onOpenChange={(open) => { if (!open) void resolveUnsaved("cancel") }}>
    <DialogContent>
      <DialogTitle>{t("Unsaved project")}</DialogTitle>
      <DialogDescription>{t("Edits are frozen while you decide. Save your work, discard it for installation, or cancel and continue working.")}</DialogDescription>
      <Button onClick={() => void resolveUnsaved("save")}>{t("Save and install")}</Button>
      <Button variant="outline" onClick={() => void resolveUnsaved("discard")}>{t("Discard and install")}</Button>
      <Button variant="outline" onClick={() => void resolveUnsaved("cancel")}>{t("Cancel")}</Button>
    </DialogContent>
  </Dialog>
}

/** The main window's updates: the start-up check, the notice and the prompt. */
export function UpdateHost() {
  useEffect(() => {
    const stop = startAutomaticChecks()
    return () => { stop(); releaseUpdate() }
  }, [])
  return <>
    <UpdateNotice />
    <UpdateDecision />
  </>
}
