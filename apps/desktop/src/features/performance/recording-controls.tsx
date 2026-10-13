import { useEffect, useRef, useState } from "react"
import type { PerformanceRecordingState } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { changeStageMedia, DEFAULT_STAGE_MEDIA, mediaDocumentGuard } from "@/features/performance-media/store"
import { backend } from "@/lib/ipc"
import { reportError } from "@/lib/errors"
import { useProjectStore } from "@/lib/store/project"
import { translatedText as t, message } from "@/lib/i18n"

export function PerformanceRecordingControls({ onHeldChange }: { onHeldChange?(held: boolean): void }) {
  const media = useProjectStore((state) => state.project.settings.stageMedia)
  const takes = media?.takes ?? DEFAULT_STAGE_MEDIA.takes
  const [state, setState] = useState<PerformanceRecordingState | null>(null)
  const [busy, setBusy] = useState(false)
  const elapsed = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    let cancelled = false
    let pending = false
    const refresh = () => {
      if (pending) return
      pending = true
      void backend.performanceRecordState().then((next) => {
        if (cancelled) return
        if (elapsed.current) elapsed.current.textContent = message("{sampleRate} / 60 seconds", { sampleRate: next.sampleRate ? (next.frames / next.sampleRate).toFixed(1) : t("0.0") })
        setState((previous) => previous?.active === next.active && previous.finished === next.finished && previous.error === next.error && previous.sampleRate === next.sampleRate ? previous : next)
      }).catch(reportError).finally(() => { pending = false })
    }
    refresh()
    const timer = setInterval(refresh, 200)
    return () => { cancelled = true; clearInterval(timer) }
  }, [])
  const run = async (operation: () => Promise<void>) => {
    setBusy(true)
    try { await operation(); setState(await backend.performanceRecordState()) } catch (error) { reportError(error) } finally { setBusy(false) }
  }
  const start = async () => { const guard = await mediaDocumentGuard(); await backend.performanceRecordStart(guard.generation, guard.revision) }
  const replay = async (index: number | null) => { const guard = await mediaDocumentGuard(); await backend.performanceTakeReplay(index, guard.generation, guard.revision) }
  const exportTake = async (index: number) => {
    const guard = await mediaDocumentGuard()
    const path = await backend.pickExportPath(`Performance ${index + 1}`, "wav")
    if (path) await backend.performanceTakeExport(index, path, guard.generation, guard.revision)
  }
  const held = state?.active || state?.finished || !!state?.error
  useEffect(() => { onHeldChange?.(!!held) }, [held, onHeldChange])
  return <section aria-label={t("Performance recording")} className="flex flex-col gap-2 rounded-md border p-3">
    <div className="flex flex-wrap items-center gap-2">
      <Button disabled={busy || backend.kind !== "tauri" || !!held || takes.length >= 16} onClick={() => { void run(start) }}>{t("Record performance")}</Button>
      <Button variant="outline" disabled={busy || !state?.active} onClick={() => { void run(() => backend.performanceRecordStop()) }}>{t("Stop capture")}</Button>
      <Button variant="outline" disabled={busy || !state?.finished} onClick={() => { void run(() => backend.performanceRecordSave()) }}>{t("Save performance")}</Button>
      <Button variant="ghost" disabled={busy || !held} onClick={() => { void run(() => backend.performanceRecordCancel()) }}>{t("Cancel capture")}</Button>
      <span ref={elapsed} className="text-xs text-muted-foreground">{t("0.0 / 60 seconds")}</span>
    </div>
    <p className="text-xs text-muted-foreground">{t("Records the callback launch log and final stereo mix, including native plugins. Stop the song first. Saved takes replay and export that exact mix; editing pads and project settings is locked during capture.")}</p>
    {state?.error && <Alert variant="destructive"><AlertDescription>{state.error}</AlertDescription></Alert>}
    {takes.map((take, index) => <div key={take.sample} className="flex flex-wrap items-center gap-2 text-sm">
      <span>{t("Performance")}{" "}{index + 1} · {(take.frames / take.sampleRate).toFixed(2)} s · {take.events.length}{" "}{t("launches")}</span>
      <Button variant="outline" size="sm" disabled={busy || !!held || backend.kind !== "tauri"} onClick={() => { void run(() => replay(index)) }}>{t("Replay performance")}{" "}{index + 1}</Button>
      <Button variant="outline" size="sm" disabled={busy || !!held || backend.kind !== "tauri"} onClick={() => { void run(() => exportTake(index)) }}>{t("Export performance")}{" "}{index + 1} WAV</Button>
      <Button variant="ghost" size="sm" disabled={busy || !!held} onClick={() => { void changeStageMedia((media) => ({ ...media, takes: media.takes.filter((_, at) => at !== index) })) }}>{t("Remove performance")}{" "}{index + 1}</Button>
    </div>)}
    {takes.length > 0 && <Button variant="outline" disabled={busy || !!held || backend.kind !== "tauri"} onClick={() => { void run(() => replay(null)) }}>{t("Stop performance replay")}</Button>}
  </section>
}
