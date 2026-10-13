import { useRef, useState } from "react"
import type { AutomationTarget } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { ParamControl } from "@/features/params/param-control"
import { automationItems } from "@/features/automation/menu"
import { useProjectStore, dispatch } from "@/lib/store/project"
import { newGestureId } from "@/lib/store/gesture"
import { scriptParameter, scriptParameterTargets } from "@/lib/script-parameters"
import { useRealtime } from "@/lib/store"
import { errorMessage } from "@/lib/ipc/backend"

function ScriptParameterControl({ target }: { target: Extract<AutomationTarget, { type: "instrumentParam" }> }) {
  const project = useProjectStore(state => state.project)
  const gesture = useRef<number | undefined>(undefined)
  const [error, setError] = useState<string | null>(null)
  const found = scriptParameter(project, target.channel, target.param)
  if (!found) return null
  return <div>
    <ParamControl info={found.info} value={found.stored} contextItems={automationItems(target)}
      onGestureStart={() => { gesture.current = newGestureId() }}
      onGestureEnd={() => { gesture.current = undefined }}
      onValueChange={value => {
        setError(null)
        void dispatch({ type: "setInstrumentParam", channel: target.channel, param: target.param, value }, gesture.current)
          .then(result => { if (result === null) setError("The script parameter edit was refused. See the reported error.") })
          .catch(failure => setError(errorMessage(failure)))
      }} />
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </div>
}

export function InstalledScriptParameters() {
  const project = useProjectStore(state => state.project)
  return <div className="flex flex-col gap-2">{scriptParameterTargets(project).map(target =>
    <div key={target.param}><span className="text-sm text-muted-foreground">{project.channels.find(c => c.id === target.channel)?.name}</span>
      <ScriptParameterControl target={target} />
    </div>)}</div>
}

export function ScriptNoteRefusals() {
  const output = useRef<HTMLOutputElement>(null)
  useRealtime(frame => {
    const text = (frame.rejectedRackEvents ?? 0) > 0
      ? `Note-path events refused: ${frame.rejectedRackEvents}. Check script bounds and simultaneous notes.` : ""
    if (output.current && output.current.textContent !== text) output.current.textContent = text
  })
  return <output ref={output} aria-label="Script note refusals" aria-live="polite" className="text-sm text-destructive" />
}
