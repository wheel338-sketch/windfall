import { useState } from "react"
import { Button } from "@/components/ui/button"
import { describeTarget } from "@/lib/automation/targets"
import { useProjectStore } from "@/lib/store/project"
import { backend, errorMessage } from "@/lib/ipc"
import { useTransportStore } from "@/lib/store/transport"
import { useControllerStatus } from "./status"
import { dumpNoteLog } from "./actions"

/** Real session arms, shared by transport and controller status polling. */
export function MidiRecordingControls() {
  const { state, setState } = useControllerStatus()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function arm() {
    if (!state || busy) return
    setBusy(true)
    try { setState(await backend.noteRecordArm(!state.noteRecord.armed)); setError(null) }
    catch (failure) { setError(errorMessage(failure)) } finally { setBusy(false) }
  }
  const project = useProjectStore((state) => state.project)
  const bars = useTransportStore((state) => state.countInBars ?? 0)
  return <div className="flex items-center gap-1">
    <Button title={`Record MIDI notes; ${bars} count-in bars`} aria-label="Record MIDI notes" aria-pressed={state?.noteRecord.armed ?? false} disabled={!state || busy || backend.kind !== "tauri"} variant={state?.noteRecord.armed ? "destructive" : "secondary"} onClick={() => void arm()}>MIDI record</Button>
    <Button size="sm" variant="ghost" onClick={() => void dumpNoteLog(null)}>Dump note log</Button>
    <span role="status" aria-label="Controller pickup indicator">{state?.waiting.length ? `Pickup held: ${state.waiting.map((target) => describeTarget(project, target)).join(", ")}` : ""}</span>
    <span role="status" aria-label="MIDI recording status">{error ?? (state?.noteRecord.armed ? `${state.noteRecord.pending} notes captured` : "")}</span>
  </div>
}
