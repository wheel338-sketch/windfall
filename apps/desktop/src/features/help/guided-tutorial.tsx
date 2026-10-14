import { useEffect, useId, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { FieldDescription, FieldError } from "@/components/ui/field"
import { errorMessage, backend } from "@/lib/ipc"
import { localizeBackendText, useTranslation } from "@/lib/i18n"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { cancelRecording, openRecording, refreshRecording, useRecordingStore } from "@/features/transport/recording-store"
import { receiveExportRequest, releaseExportIntent, setExportIntent, useExportRequestStore } from "@/features/export/requests"
import { getProjectGeneration } from "@/lib/store/replaced"
import { baseline, runTutorialAction, TUTORIALS, type TutorialKind } from "./tutorials"

export function GuidedTutorial({ kind }: { kind: TutorialKind }) {
  const t = useTranslation()
  const project = useProjectStore((state) => state.project)
  const dirty = useProjectStore((state) => state.dirty)
  const transport = useTransportStore()
  const recording = useRecordingStore()
  const dialog = useUiStore((state) => state.dialog)
  const [before] = useState(() => baseline(project))
  const [step, setStep] = useState(0)
  const owner = useId()
  const request = useExportRequestStore((state) => state.request)
  const [exportedGeneration, setExportedGeneration] = useState<number | null>(() => request?.owner === owner && request.status === "succeeded" ? request.generation : null)
  const [openedGeneration, setOpenedGeneration] = useState<number | null>(() => dialog === "export" ? getProjectGeneration() : null)
  useEffect(() => {
    return useExportRequestStore.subscribe(({ request }) => {
      if (request?.owner === owner && request.generation === getProjectGeneration() && request.status === "succeeded") setExportedGeneration(request.generation)
    })
  }, [owner])
  useEffect(() => useUiStore.subscribe(({ dialog }) => {
    if (dialog === "export") setOpenedGeneration(getProjectGeneration())
  }), [])
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  useEffect(() => {
    const stop = backend.onExportProgress(receiveExportRequest)
    const refresh = () => { if (kind === "record" && document.visibilityState !== "hidden") void refreshRecording() }
    window.addEventListener("focus", refresh)
    document.addEventListener("visibilitychange", refresh)
    return () => {
      stop(); releaseExportIntent(owner)
      window.removeEventListener("focus", refresh)
      document.removeEventListener("visibilitychange", refresh)
    }
  }, [kind, owner])
  const tutorial = TUTORIALS[kind]
  const current = tutorial.steps[step]
  const recordingError = kind === "record" ? recording.error || recording.state.error : null
  const exportError = request?.owner === owner ? request.error : null
  const recovery = kind === "record" && (recordingError || (step > 0 && !recording.state.active))
  if (!current) return <p role="status">{t("Completed")}: {t(tutorial.title)}</p>
  const completed = current.complete({ project, before, dirty, playing: transport.playing, song: transport.mode === "song", recording: recording.state.active, recordingOpen: recording.open, inputs: recording.inputs.length, exportOpen: dialog === "export" || openedGeneration === getProjectGeneration(), exported: exportedGeneration === getProjectGeneration() })
  return <section aria-label={t(tutorial.title)} className="flex flex-col gap-3">
    <Badge variant="secondary">{step + 1} / {tutorial.steps.length}</Badge>
    <h3 className="font-medium">{t(current.title)}</h3>
    <p>{t(current.instruction)}</p>
    <FieldDescription>{completed ? t("Completed") : t("Waiting for the real app state.")}</FieldDescription>
    <div className="flex gap-2">
      <Button variant="outline" size="sm" disabled={busy} onClick={() => {
        if (pending.current) return
        pending.current = true; setBusy(true); setError("")
        if (kind === "export") setExportIntent(owner)
        void runTutorialAction(current.action).catch((failure: unknown) => setError(errorMessage(failure)))
          .finally(() => { pending.current = false; setBusy(false) })
      }}>{t(current.title)}</Button>
      <Button size="sm" disabled={!completed || busy} onClick={() => setStep(step + 1)}>{t("Next")}</Button>
    </div>
    {recovery && !completed && <>
      <FieldDescription>{t("Recording stopped or its device became unavailable. Reopen setup, select an input and retry; an existing kept take still counts.")}</FieldDescription>
      <Button variant="outline" size="sm" disabled={busy || recording.busy} onClick={() => {
        setError("")
        void (async () => {
          const current = useRecordingStore.getState().state
          if (current.active && current.error) await cancelRecording()
          await openRecording()
          setStep(useRecordingStore.getState().state.active ? 2 : 0)
        })()
      }}>{t(recording.state.active && recording.state.error ? "Discard failed take and retry setup" : "Retry recording setup")}</Button>
    </>}
    {kind === "export" && request?.owner === owner && (request.status === "cancelled" || request.status === "failed") &&
      <FieldDescription>{t("The export did not finish. Reopen Export, choose a destination and retry. Only this guide's successful request counts.")}</FieldDescription>}
    {(recordingError || exportError) && <FieldError role="alert">{localizeBackendText(recordingError || exportError || "")}</FieldError>}
    {error && <FieldError role="alert">{localizeBackendText(error)}</FieldError>}
  </section>
}
