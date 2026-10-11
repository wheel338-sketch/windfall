import { useEffect, useRef, useState } from "react"
import type { CommandProposal, PianoScriptPreview, ScriptKind, ScriptLibrary, ScriptParameter } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { backend } from "@/lib/ipc"
import { errorMessage } from "@/lib/ipc/backend"
import { useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { currentSession } from "@/features/piano-roll/session"
import { ProposalReview } from "./review"
import { closeScripts, useAssistantWindows } from "./store"

const EXAMPLES: Record<ScriptKind, string> = {
  piano: "param transpose -24 24 12\nselected\nset key key transpose + 0 127 clamp\nemit\nend",
  noteTransform: "param transpose -24 24 12\nset key key transpose + 0 127 clamp\nemit",
  controller: "map 1 value -\nfeedback 7 value 127 *",
}

function cancelRun(request: number | null) {
  if (request !== null) void backend.scriptCancel(request).catch(() => {})
}

function ScriptForm() {
  const project = useProjectStore((state) => state.project)
  const [kind, setKind] = useState<ScriptKind>("piano")
  const [source, setSource] = useState(EXAMPLES.piano)
  const [id, setId] = useState("user-script")
  const [parameters, setParameters] = useState<ScriptParameter[]>([])
  const [values, setValues] = useState<string[]>([])
  const [replace, setReplace] = useState(true)
  const [cc, setCc] = useState("7")
  const [feedback, setFeedback] = useState(false)
  const [preview, setPreview] = useState<PianoScriptPreview | null>(null)
  const [proposal, setProposal] = useState<CommandProposal | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const run = useRef<number | null>(null)
  const operation = useRef(0)
  const submitting = useRef(false)
  const [generation] = useState(getProjectGeneration)
  useEffect(() => () => {
    operation.current += 1
    cancelRun(run.current)
  }, [])

  function editSource(next: string) {
    operation.current += 1
    cancelRun(run.current)
    setSource(next); setParameters([]); setValues([]); setPreview(null); setProposal(null)
  }
  function changeKind(next: ScriptKind) { setKind(next); editSource(EXAMPLES[next]) }

  async function work(task: (token: number) => Promise<void>) {
    if (submitting.current) return
    submitting.current = true
    const token = ++operation.current
    setBusy(true); setError(null); setProposal(null); setPreview(null)
    try { await task(token) }
    catch (failure) { if (token === operation.current) setError(errorMessage(failure)) }
    finally { submitting.current = false; if (token === operation.current) setBusy(false) }
  }
  const current = (token: number) => token === operation.current && generation === getProjectGeneration()
  function parameterValues() {
    return parameters.map((p, i) => {
      const value = values[i]?.trim() ? Number(values[i]) : NaN
      if (!Number.isFinite(value) || value < p.min || value > p.max) throw new Error(`${p.name} must be ${p.min}..${p.max}.`)
      return value
    })
  }
  function readParameters() { return work(async (token) => {
    const result = await backend.scriptParameters(source)
    if (current(token)) { setParameters(result); setValues(result.map((p) => String(p.default))) }
  }) }

  function previewNotes() { return work(async (token) => {
    const editor = currentSession()?.editor
    const context = editor?.context
    if (!context || !editor || editor.busy) throw new Error("Open a piano-roll lane before running a piano script.")
    const selected = editor.selectedNotes().map((note) => note.id)
    const stamp = await backend.proposalStamp()
    if (!current(token) || stamp.revision !== useProjectStore.getState().revision) throw new Error("The project changed; preview again.")
    const request = await backend.scriptBegin()
    run.current = request
    try {
      const result = await backend.scriptPreview(request, { stamp, pattern: context.pattern.id, channel: context.channel, selected, source, parameters: parameterValues(), replace })
      if (current(token)) { setPreview(result); setProposal(result.proposal) }
    } finally { run.current = null }
  }) }

  function unassign(channel: number | null, controllerIndex: number | null) { return work(async (token) => {
    const stamp = await backend.proposalStamp()
    const state = useProjectStore.getState()
    if (!current(token) || stamp.revision !== state.revision) throw new Error("The project changed; review again.")
    const saved = state.project.settings.scripts
    if (!saved) throw new Error("The script assignment no longer exists.")
    const library: ScriptLibrary = { ...saved, channels: saved.channels.filter((binding) => binding.channel !== channel), controllers: saved.controllers.filter((_binding, index) => index !== controllerIndex) }
    setProposal({ stamp, label: "Remove script assignment", explanation: "Remove this installed assignment. Saved script revisions remain available; Undo restores the assignment.", commands: [{ type: "setScriptLibrary", library }] })
  }) }

  function saveAndAssign(assign: boolean) { return work(async (token) => {
    const stamp = await backend.proposalStamp()
    const state = useProjectStore.getState()
    if (!current(token) || stamp.revision !== state.revision) throw new Error("The project changed; review again.")
    const saved = state.project.settings.scripts ?? { assets: [], channels: [], controllers: [] }
    const revision = Math.max(0, ...saved.assets.filter((a) => a.reference.id === id).map((a) => a.reference.revision)) + 1
    const reference = { id, revision }
    const library: ScriptLibrary = {
      ...saved,
      assets: [...saved.assets, { reference, languageVersion: 1, kind, name: id, source }],
      channels: [...saved.channels], controllers: [...saved.controllers],
    }
    if (assign && kind === "noteTransform") {
      const channel = useUiStore.getState().selectedChannel
      if (channel === null) throw new Error("Select a channel before assigning the note transform.")
      library.channels = [...library.channels.filter((binding) => binding.channel !== channel), { channel, script: reference, parameters: parameterValues() }]
    }
    if (assign && kind === "controller") {
      const number = Number(cc)
      if (!Number.isInteger(number) || number < 0 || number > 127) throw new Error("Controller number must be 0..127.")
      const sourceInput = { type: "midi" as const, channel: 1, control: { type: "cc" as const, number } }
      library.controllers = [...library.controllers.filter((binding) => !(binding.source.type === "midi" && binding.source.device == null && binding.source.channel === 1 && binding.source.control.type === "cc" && binding.source.control.number === number)), {
        source: sourceInput, script: reference, parameters: parameterValues(), target: { type: "slot", slot: { type: "masterVolume" } }, feedbackChannel: feedback ? 1 : null,
      }]
    }
    setProposal({ stamp, label: assign ? "Install script revision" : "Save script revision", explanation: `Save ${id} revision ${revision}${assign ? ` and assign its ${kind} module` : ""}. Existing revisions remain unchanged.`, commands: [{ type: "setScriptLibrary", library }] })
  }) }

  return <div className="flex flex-col gap-4">
    <ToggleGroup value={[kind]} onValueChange={(next) => { const value = next[0]; if (value === "piano" || value === "controller" || value === "noteTransform") changeKind(value) }} aria-label="Script purpose" disabled={busy}>
      <ToggleGroupItem value="piano">Piano notes</ToggleGroupItem><ToggleGroupItem value="controller">Controller</ToggleGroupItem><ToggleGroupItem value="noteTransform">Note transform</ToggleGroupItem>
    </ToggleGroup>
    <FieldGroup>
      <Field><FieldLabel htmlFor="script-id">Script ID</FieldLabel><Input id="script-id" value={id} maxLength={64} disabled={busy} onChange={(event) => { setId(event.target.value); setProposal(null) }} /></Field>
      <Field><FieldLabel htmlFor="script-source">Windfall Script v1 source</FieldLabel><Textarea id="script-source" className="min-h-48 font-mono" value={source} maxLength={16384} disabled={busy} onChange={(event) => editSource(event.target.value)} /><FieldDescription>Postfix arithmetic: set key key transpose + 0 127 clamp. Use selected/end or repeat COUNT/end, then emit. No Python, IO or runtime installation.</FieldDescription></Field>
      {parameters.map((parameter, index) => <Field key={parameter.name}><FieldLabel htmlFor={`script-param-${index}`}>{parameter.name} ({parameter.min}..{parameter.max})</FieldLabel><Input id={`script-param-${index}`} type="number" min={parameter.min} max={parameter.max} value={values[index] ?? ""} disabled={busy} onChange={(event) => { setValues(values.map((value, i) => i === index ? event.target.value : value)); setProposal(null); setPreview(null) }} /></Field>)}
      {kind === "piano" && <Field><FieldLabel htmlFor="script-replace">Replace selected notes</FieldLabel><Checkbox id="script-replace" checked={replace} disabled={busy} onCheckedChange={(checked) => { setReplace(checked); setProposal(null) }} /><FieldDescription>Unselected notes stay unchanged. Emitted notes use new IDs and default expression. Empty selection is valid for generation.</FieldDescription></Field>}
      {kind === "controller" && <>
        <Field><FieldLabel htmlFor="script-cc">Input CC on MIDI channel 1</FieldLabel><Input id="script-cc" type="number" min={0} max={127} value={cc} disabled={busy} onChange={(event) => { setCc(event.target.value); setProposal(null) }} /><FieldDescription>Any enabled input device. Script map output drives Master volume; profile links still handle transport buttons.</FieldDescription></Field>
        <Field><FieldLabel htmlFor="script-feedback">Enable MIDI feedback</FieldLabel><Checkbox id="script-feedback" checked={feedback} disabled={busy} onCheckedChange={(checked) => { setFeedback(checked); setProposal(null) }} /><FieldDescription>Explicitly send emitted CC feedback through the configured MIDI output on channel 1.</FieldDescription></Field>
      </>}
    </FieldGroup>
    {backend.kind !== "tauri" && <Alert><AlertDescription>Piano execution and hardware controller scripts require the native bounded worker. You can edit and save revisions here.</AlertDescription></Alert>}
    {kind === "noteTransform" && <Alert><AlertDescription>Assigned transforms compile into channel pattern events for playback and export. Hardware audition and automation transforms are not supported by this version.</AlertDescription></Alert>}
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" disabled={busy} onClick={() => void readParameters()}>Read parameters</Button>
      {kind === "piano" && <Button disabled={busy || backend.kind !== "tauri"} onClick={() => void previewNotes()}>Preview notes</Button>}
      <Button variant="outline" disabled={busy} onClick={() => void saveAndAssign(false)}>Review save revision</Button>
      {kind !== "piano" && <Button disabled={busy || (kind === "controller" && backend.kind !== "tauri")} onClick={() => void saveAndAssign(true)}>Review save and assign</Button>}
      <Button variant="ghost" disabled={!busy} onClick={() => { operation.current += 1; cancelRun(run.current); setBusy(false) }}>Cancel run</Button>
    </div>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {preview && <details open><summary>{preview.notes.length} emitted notes</summary><pre className="max-h-48 overflow-auto">{preview.notes.slice(0, 24).map((n) => `tick ${n.start} · length ${n.length} · key ${n.key} · velocity ${n.velocity}`).join("\n")}</pre></details>}
    {proposal && <ProposalReview key={`${proposal.stamp.revision}:${proposal.label}:${JSON.stringify(proposal.commands)}`} proposal={proposal} generation={generation} cancel={() => { setProposal(null); setPreview(null) }} />}
    <details><summary>Saved revisions</summary>{project.settings.scripts?.assets.map((asset) => <Button key={`${asset.reference.id}:${asset.reference.revision}`} variant="ghost" disabled={busy} onClick={() => { setId(asset.reference.id); setKind(asset.kind); editSource(asset.source) }}>{asset.name} r{asset.reference.revision}</Button>)}</details>
    <details><summary>Installed assignments</summary>
      {project.settings.scripts?.channels.map((binding) => <div key={binding.channel} className="flex items-center gap-2"><span>Channel {binding.channel}: {binding.script.id} r{binding.script.revision}</span><Button variant="outline" disabled={busy} onClick={() => void unassign(binding.channel, null)}>Review remove channel {binding.channel}</Button></div>)}
      {project.settings.scripts?.controllers.map((binding, index) => <div key={index} className="flex items-center gap-2"><span>Controller {index + 1}: {binding.script.id} r{binding.script.revision}{binding.feedbackChannel ? " · feedback enabled" : ""}</span><Button variant="outline" disabled={busy} onClick={() => void unassign(null, index)}>Review remove controller {index + 1}</Button></div>)}
    </details>
  </div>
}

export function ScriptEditor() {
  const open = useAssistantWindows((state) => state.scripting)
  const epoch = useAssistantWindows((state) => state.epoch)
  return <Dialog open={open} onOpenChange={(next) => { if (!next) closeScripts() }}><DialogContent className="max-h-[85vh] overflow-y-auto"><DialogHeader><DialogTitle>Script editor</DialogTitle><DialogDescription>Bounded offline scripts. Preview and review all project changes.</DialogDescription></DialogHeader>{open && <ScriptForm key={epoch} />}</DialogContent></Dialog>
}
