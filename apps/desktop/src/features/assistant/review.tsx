import { useRef, useState } from "react"
import type { CommandProposal } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { backend } from "@/lib/ipc"
import { errorMessage } from "@/lib/ipc/backend"
import { receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { localizeBackendText, translatedText as t } from "@/lib/i18n"

/** Review shows the exact checked command payload, including plugin targets and
 * values. Apply is one Rust batch. Undo is offered only while this is the latest
 * applied revision, so it cannot accidentally undo a later unrelated edit. */
export function ProposalReview({ proposal, generation, cancel }: {
  proposal: CommandProposal
  generation: number
  cancel(): void
}) {
  const revision = useProjectStore((state) => state.revision)
  const [applied, setApplied] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submitting = useRef(false)
  const current = generation === getProjectGeneration()
  const stale = !current || (applied === null && revision !== proposal.stamp.revision)

  async function apply() {
    if (stale || submitting.current) return
    submitting.current = true
    setBusy(true)
    setError(null)
    try {
      const result = await backend.proposalApply(proposal)
      if (generation !== getProjectGeneration()) return
      receivePatch(result.patch)
      setApplied(result.patch.revision)
    } catch (failure) { setError(errorMessage(failure)) }
    finally { submitting.current = false; setBusy(false) }
  }

  async function undo() {
    if (!current || revision !== applied || submitting.current) return
    submitting.current = true
    setBusy(true)
    try {
      const patch = await backend.proposalUndo({ ...proposal.stamp, revision: applied! })
      if (generation !== getProjectGeneration()) return
      if (patch) receivePatch(patch)
      cancel()
    } catch (failure) { setError(errorMessage(failure)) }
    finally { submitting.current = false; setBusy(false) }
  }

  return <section aria-label={t("Command proposal")} className="flex flex-col gap-3">
    <p>{localizeBackendText(proposal.explanation)}</p>
    <ol className="flex list-decimal flex-col gap-2 pl-5">
      {proposal.commands.map((command, index) => <li key={index}>
        <details><summary>{command.type}</summary><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(command, null, 2)}</pre></details>
      </li>)}
    </ol>
    {stale && <Alert><AlertDescription>{t("The project changed. Generate a new preview.")}</AlertDescription></Alert>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {applied !== null && <p role="status">{t("Applied as one undo step.")}</p>}
    <div className="flex gap-2">
      <Button disabled={busy || stale || applied !== null} onClick={() => void apply()}>{t("Apply batch")}</Button>
      <Button variant="outline" disabled={busy || !current || applied === null || revision !== applied} onClick={() => void undo()}>{t("Undo batch")}</Button>
      <Button variant="ghost" disabled={busy} onClick={cancel}>{t("Dismiss proposal")}</Button>
    </div>
  </section>
}
