import { useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { FieldDescription, FieldError } from "@/components/ui/field"
import { errorMessage, backend } from "@/lib/ipc"
import { useTranslation } from "@/lib/i18n"
import { useProjectStore } from "@/lib/store/project"
import { useTransportStore } from "@/lib/store/transport"
import { useUiStore } from "@/lib/store/ui"
import { useRecordingStore } from "@/features/transport/recording-store"
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
  const [exported, setExported] = useState(false)
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  useEffect(() => backend.onExportProgress((progress) => {
    if (progress.done && !progress.cancelled && progress.error === null) setExported(true)
  }), [])
  const tutorial = TUTORIALS[kind]
  const current = tutorial.steps[step]
  if (!current) return <p role="status">{t("Completed")}: {t(tutorial.title)}</p>
  const completed = current.complete({ project, before, dirty, playing: transport.playing, song: transport.mode === "song", recording: recording.state.active, recordingOpen: recording.open, inputs: recording.inputs.length, exportOpen: dialog === "export", exported })
  return <section aria-label={t(tutorial.title)} className="flex flex-col gap-3">
    <Badge variant="secondary">{step + 1} / {tutorial.steps.length}</Badge>
    <h3 className="font-medium">{t(current.title)}</h3>
    <p>{t(current.instruction)}</p>
    <FieldDescription>{completed ? t("Completed") : t("Waiting for the real app state.")}</FieldDescription>
    <div className="flex gap-2">
      <Button variant="outline" size="sm" disabled={busy} onClick={() => {
        if (pending.current) return
        pending.current = true; setBusy(true); setError("")
        void runTutorialAction(current.action).catch((failure: unknown) => setError(errorMessage(failure)))
          .finally(() => { pending.current = false; setBusy(false) })
      }}>{t(current.title)}</Button>
      <Button size="sm" disabled={!completed || busy} onClick={() => setStep(step + 1)}>{t("Next")}</Button>
    </div>
    {error && <FieldError role="alert">{error}</FieldError>}
  </section>
}
