import { useEffect, useRef, useState } from "react"
import type { DispatchResult, ScriptParameter } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { backend, errorMessage } from "@/lib/ipc"
import { getProjectGeneration, onProjectReplaced } from "@/lib/store/replaced"
import { useProjectStore } from "@/lib/store/project"
import { usePlaylistStore } from "@/features/playlist/store"
import type { AudioEditPreview, AudioScriptPreview } from "./types"
import type { Selection } from "./selection"
import { Waveform } from "./waveform"

// Original examples, using only Windfall's bounded language.
export const AUDIO_SCRIPT_EXAMPLES = {
  "Gain ramp": "param first 0 1 0\nparam last 0 1 1\ngain first last first - value * +",
  Reverse: "reverse",
  Normalize: "param peak 0.01 1 0.8\nnormalize peak",
  "Linear fade in": "gain value",
  "Squared fade out": "gain 1 value - 1 value - *",
}

export function ScriptTools({ preview, selection, disabled, onBusy, onCommit }: {
  preview: AudioEditPreview; selection: Selection; disabled: boolean
  onBusy(busy: boolean): void; onCommit(result: DispatchResult): void
}) {
  const [source, setSource] = useState(AUDIO_SCRIPT_EXAMPLES["Gain ramp"])
  const [parameters, setParameters] = useState<ScriptParameter[]>([])
  const [values, setValues] = useState<number[]>([])
  const [result, setResult] = useState<AudioScriptPreview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const request = useRef<number | null>(null)
  const operation = useRef(0)
  const working = useRef(false)
  const alive = useRef(false)
  const current = useRef<() => boolean>(() => false)
  useEffect(() => {
    alive.current = true
    const generation = getProjectGeneration()
    const revision = useProjectStore.getState().revision
    const selected = usePlaylistStore.getState().selection
    current.current = () => alive.current && generation === getProjectGeneration() &&
      revision === useProjectStore.getState().revision && selected === usePlaylistStore.getState().selection
    const invalidate = () => { if (!current.current()) {
      operation.current++
      if (request.current !== null) void backend.scriptCancel(request.current).catch(() => {})
      setResult(null)
    } }
    const offProject = onProjectReplaced(invalidate)
    const offEdit = useProjectStore.subscribe(invalidate)
    const offSelection = usePlaylistStore.subscribe(invalidate)
    return () => {
      alive.current = false; operation.current++
      offProject(); offEdit(); offSelection()
      if (request.current !== null) void backend.scriptCancel(request.current).catch(() => {})
      void backend.previewStop().catch(() => {})
    }
  }, [preview.token])
  function cancel() {
    operation.current++
    if (request.current !== null) void backend.scriptCancel(request.current).catch(() => {})
    request.current = null
    setResult(null)
    void backend.previewStop().catch(() => {})
  }
  function edit(text: string) { cancel(); setSource(text); setParameters([]); setValues([]); setError(null) }
  async function work(task: (op: number) => Promise<void>) {
    if (working.current || !current.current()) return
    working.current = true; setBusy(true); onBusy(true); setError(null)
    const op = ++operation.current
    try { await task(op) } catch (failure) {
      if (current.current() && op === operation.current) setError(errorMessage(failure))
    } finally { working.current = false; if (alive.current) { setBusy(false); onBusy(false) } }
  }
  const matches = result !== null && result.range.startFrame === selection.start && result.range.endFrame === selection.end
  return <section aria-label="Audio selection scripts" className="flex flex-col gap-2">
    <FieldGroup>
      <Field><FieldLabel htmlFor="audio-script-source">Audio Windfall Script source</FieldLabel>
        <Textarea id="audio-script-source" maxLength={16384} value={source} disabled={disabled || busy}
          onChange={event => edit(event.target.value)} /></Field>
      {parameters.map((parameter, i) => <Field key={parameter.name}>
        <FieldLabel htmlFor={`audio-script-param-${i}`}>{parameter.name} ({parameter.min}..{parameter.max})</FieldLabel>
        <Input id={`audio-script-param-${i}`} type="number" min={parameter.min} max={parameter.max}
          value={Number.isNaN(values[i]) ? "" : values[i] ?? ""} disabled={disabled || busy}
          onChange={event => { cancel(); setValues(values.map((v, index) => index === i ? event.target.valueAsNumber : v)) }} />
      </Field>)}
    </FieldGroup>
    <div className="flex flex-wrap gap-2">
      {Object.entries(AUDIO_SCRIPT_EXAMPLES).map(([name, text]) => <Button key={name} size="sm" variant="outline"
        disabled={disabled || busy} onClick={() => edit(text)}>Load {name}</Button>)}
      <Button variant="outline" disabled={disabled || busy} onClick={() => void work(async op => {
        const params = await backend.scriptParameters(source)
        if (current.current() && op === operation.current) { setParameters(params); setValues(params.map(p => p.default)) }
      })}>Read audio script parameters</Button>
      <Button disabled={disabled || busy || backend.kind !== "tauri"} onClick={() => void work(async op => {
        setResult(null)
        const ticket = await backend.scriptBegin()
        if (!current.current() || op !== operation.current) { await backend.scriptCancel(ticket); return }
        request.current = ticket
        const value = await backend.audioScriptPreview({ request: ticket, token: preview.token,
          range: { startFrame: selection.start, endFrame: selection.end }, source, parameters: values })
        if (current.current() && op === operation.current) setResult(value)
      })}>Run script</Button>
      <Button variant="outline" disabled={!result || !matches || disabled || busy} onClick={() => void work(async () => {
        if (result) await backend.audioScriptAudition(preview.token, result.request)
      })}>Audition script result</Button>
      <Button disabled={!result || !matches || disabled || busy} onClick={() => void work(async op => {
        if (!result) return
        const applied = await backend.audioScriptApply(preview.token, result.request)
        if (current.current() && op === operation.current) onCommit(applied)
      })}>Apply script result</Button>
      <Button variant="outline" disabled={!busy && !result} onClick={cancel}>Cancel audio script</Button>
    </div>
    {backend.kind !== "tauri" && <p role="status">Audio script processing requires the desktop app.</p>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {result && matches && <><p role="status">Reviewed result: {result.frames} frames. Apply creates one undoable selection edit.</p>
      <Waveform preview={{ ...preview, frames: result.frames, sampleRate: result.sampleRate, peaks: result.peaks }}
        selection={{ start: 0, end: result.frames }} disabled onSelection={() => {}} /></>}
    <p className="text-xs text-muted-foreground">gain EXPRESSION, reverse, normalize AMPLITUDE. value is frame position 0–1;
      index is the frame number. Five-second deadline, 100 million instructions, one output buffer up to 64 MiB.
      Preview has no document edits. Cancel invalidates the reviewed result.</p>
  </section>
}
