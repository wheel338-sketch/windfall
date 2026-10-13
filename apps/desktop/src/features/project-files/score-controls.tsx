import { useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { errorMessage } from "@/lib/ipc"
import { saveScore, loadScore } from "./actions"
import { translatedText as t } from "@/lib/i18n"
export function ScoreFileControls() {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const pending = useRef(false)
  async function work(task: () => Promise<void>) {
    if (pending.current) return
    pending.current = true; setBusy(true); setError(null)
    try { await task() } catch (error) { setError(errorMessage(error)) } finally { pending.current = false; setBusy(false) }
  }
  return <section aria-label={t("Reusable piano-roll scores")} className="flex flex-col gap-2"><h2>{t("Scores")}</h2><div className="flex flex-wrap gap-2"><Button disabled={busy} variant="outline" onClick={() => void work(saveScore)}>{t("Save lane or selection as score")}</Button><Button disabled={busy} variant="outline" onClick={() => void work(() => loadScore())}>{t("Replace lane from score")}</Button><Button disabled={busy} variant="outline" onClick={() => void work(() => loadScore("merge"))}>{t("Merge score into lane")}</Button></div>{error && <Alert><AlertTitle>{t("Score file")}</AlertTitle><AlertDescription>{error}</AlertDescription></Alert>}</section>
}
