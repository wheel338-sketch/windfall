import { useEffect, useRef, useState } from "react"
import type { CommandProposal, ControlMap, ControlSource, LinkTarget, PianoScriptPreview, ScriptKind, ScriptLibrary, ScriptParameter } from "@/bindings"
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
import { ControllerBindingFields } from "./controller-binding"
import { InstalledScriptParameters, ScriptNoteRefusals } from "./script-parameters"
import { useControllerStatus } from "@/features/controllers/status"
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

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
  const [controllerSource, setControllerSource] = useState<ControlSource>({ type: "midi", channel: 1, control: { type: "cc", number: 7 } })
  const [controllerTarget, setControllerTarget] = useState<LinkTarget | null>({ type: "slot", slot: { type: "masterVolume" } })
  const [controllerMap, setControllerMap] = useState<ControlMap>({ min: 0, max: 1, smoothingMs: 0, pickup: "off", mode: "absolute" })
  const [feedbackChannel, setFeedbackChannel] = useState(1)
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
      if (!Number.isFinite(value) || value < p.min || value > p.max) throw new Error(message("{pName} must be {min}..{max}.", { pName: p.name, min: plainNumber(p.min), max: plainNumber(p.max) }))
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
    if (!context || !editor || editor.busy) throw new Error(t("Open a piano-roll lane before running a piano script."))
    const selected = editor.selectedNotes().map((note) => note.id)
    const stamp = await backend.proposalStamp()
    if (!current(token) || stamp.revision !== useProjectStore.getState().revision) throw new Error(t("The project changed; preview again."))
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
    if (!current(token) || stamp.revision !== state.revision) throw new Error(t("The project changed; review again."))
    const saved = state.project.settings.scripts
    if (!saved) throw new Error(t("The script assignment no longer exists."))
    const library: ScriptLibrary = { ...saved, channels: saved.channels.filter((binding) => binding.channel !== channel), controllers: saved.controllers.filter((_binding, index) => index !== controllerIndex) }
    setProposal({ stamp, label: t("Remove script assignment"), explanation: "Remove this installed assignment. Saved script revisions remain available; Undo restores the assignment.", commands: [{ type: "setScriptLibrary", library }] })
  }) }

  function loadController(index: number) { return work(async token => {
    const library = useProjectStore.getState().project.settings.scripts
    const binding = library?.controllers[index]
    const asset = binding && library?.assets.find(asset => asset.reference.id === binding.script.id && asset.reference.revision === binding.script.revision)
    if (!asset || !binding) throw new Error(t("This controller assignment no longer exists."))
    const parameters = await backend.scriptParameters(asset.source)
    if (!current(token)) return
    setId(asset.reference.id); setKind("controller"); setSource(asset.source)
    setParameters(parameters); setValues(parameters.map((parameter, i) => String(binding.parameters[i] ?? parameter.default)))
    setControllerSource(binding.source); setControllerTarget(binding.target)
    setControllerMap(binding.map); setFeedback(binding.feedbackChannel !== null)
    setFeedbackChannel(binding.feedbackChannel ?? 1)
  }) }

  function saveAndAssign(assign: boolean) { return work(async (token) => {
    const stamp = await backend.proposalStamp()
    const state = useProjectStore.getState()
    if (!current(token) || stamp.revision !== state.revision) throw new Error(t("The project changed; review again."))
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
      if (channel === null) throw new Error(t("Select a channel before assigning the note transform."))
      library.channels = [...library.channels.filter((binding) => binding.channel !== channel), { channel, script: reference, parameters: parameterValues() }]
    }
    if (assign && kind === "controller") {
      library.controllers = [...library.controllers.filter(binding => JSON.stringify(binding.source) !== JSON.stringify(controllerSource)), {
        source: controllerSource, script: reference, parameters: parameterValues(), target: controllerTarget,
        feedbackChannel: feedback ? feedbackChannel : null, map: controllerMap,
      }]
    }
    setProposal({ stamp, label: assign ? t("Install script revision") : t("Save script revision"), explanation: `Save ${id} revision ${revision}${assign ? ` and assign its ${kind} module` : ""}. Existing revisions remain unchanged.`, commands: [{ type: "setScriptLibrary", library }] })
  }) }

  return <div className="flex flex-col gap-4">
    <ToggleGroup value={[kind]} onValueChange={(next) => { const value = next[0]; if (value === "piano" || value === "controller" || value === "noteTransform") changeKind(value) }} aria-label={t("Script purpose")} disabled={busy}>
      <ToggleGroupItem value="piano">{t("Piano notes")}</ToggleGroupItem><ToggleGroupItem value="controller">{t("Controller")}</ToggleGroupItem><ToggleGroupItem value="noteTransform">{t("Note transform")}</ToggleGroupItem>
    </ToggleGroup>
    <FieldGroup>
      <Field><FieldLabel htmlFor="script-id">{t("Script ID")}</FieldLabel><Input id="script-id" value={id} maxLength={64} disabled={busy} onChange={(event) => { setId(event.target.value); setProposal(null) }} /></Field>
      <Field><FieldLabel htmlFor="script-source">{t("Windfall Script v1 source")}</FieldLabel><Textarea id="script-source" className="min-h-48 font-mono" value={source} maxLength={16384} disabled={busy} onChange={(event) => editSource(event.target.value)} /><FieldDescription>{t("Postfix arithmetic: set key key transpose + 0 127 clamp. Use selected/end or repeat COUNT/end, then emit. No Python, IO or runtime installation.")}</FieldDescription></Field>
      {parameters.map((parameter, index) => <Field key={parameter.name}><FieldLabel htmlFor={`script-param-${index}`}>{parameter.name} ({parameter.min}..{parameter.max})</FieldLabel><Input id={`script-param-${index}`} type="number" min={parameter.min} max={parameter.max} value={values[index] ?? ""} disabled={busy} onChange={(event) => { setValues(values.map((value, i) => i === index ? event.target.value : value)); setProposal(null); setPreview(null) }} /></Field>)}
      {kind === "piano" && <Field><FieldLabel htmlFor="script-replace">{t("Replace selected notes")}</FieldLabel><Checkbox id="script-replace" checked={replace} disabled={busy} onCheckedChange={(checked) => { setReplace(checked); setProposal(null) }} /><FieldDescription>{t("Unselected notes stay unchanged. Emitted notes use new IDs and default expression. Empty selection is valid for generation.")}</FieldDescription></Field>}
      {kind === "controller" && <>
        <ControllerBindingFields project={project} source={controllerSource} target={controllerTarget} map={controllerMap}
          feedbackChannel={feedbackChannel} disabled={busy} change={patch => {
            if (patch.source) setControllerSource(patch.source)
            if ("target" in patch) setControllerTarget(patch.target ?? null)
            if (patch.map) setControllerMap(patch.map)
            if (patch.feedbackChannel !== undefined) setFeedbackChannel(patch.feedbackChannel)
            setProposal(null)
          }} />
        <Field><FieldLabel htmlFor="script-feedback">{t("Enable MIDI feedback")}</FieldLabel><Checkbox id="script-feedback" checked={feedback} disabled={busy} onCheckedChange={(checked) => { setFeedback(checked); setProposal(null) }} /><FieldDescription>{t("Explicitly send emitted CC feedback through the configured MIDI output on the chosen channel.")}</FieldDescription></Field>
      </>}
    </FieldGroup>
    {backend.kind !== "tauri" && <Alert><AlertDescription>{t("Piano execution and hardware controller scripts require the native bounded worker. You can edit and save revisions here.")}</AlertDescription></Alert>}
    {kind === "noteTransform" && <Alert><AlertDescription>{t("Assigned transforms process channel notes from patterns, the keyboard and MIDI input. Installed parameter controls support automation and controller links. Key release cancels pending copies and releases the voices from that press.")}</AlertDescription></Alert>}
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" disabled={busy} onClick={() => void readParameters()}>{t("Read parameters")}</Button>
      {kind === "piano" && <Button disabled={busy || backend.kind !== "tauri"} onClick={() => void previewNotes()}>{t("Preview notes")}</Button>}
      <Button variant="outline" disabled={busy} onClick={() => void saveAndAssign(false)}>{t("Review save revision")}</Button>
      {kind !== "piano" && <Button disabled={busy || (kind === "controller" && backend.kind !== "tauri")} onClick={() => void saveAndAssign(true)}>{t("Review save and assign")}</Button>}
      <Button variant="ghost" disabled={!busy} onClick={() => { operation.current += 1; cancelRun(run.current); setBusy(false) }}>{t("Cancel run")}</Button>
    </div>
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    <InstalledScriptParameters />
    <ScriptNoteRefusals />
    {kind === "controller" && <ControllerScriptStatus />}
    {preview && <details open><summary>{preview.notes.length}{" "}{t("emitted notes")}</summary><pre className="max-h-48 overflow-auto">{preview.notes.slice(0, 24).map((n) => message("tick {start} · length {count} · key {key} · velocity {velocity}", { start: plainNumber(n.start), count: plainNumber(n.length), key: plainNumber(n.key), velocity: plainNumber(n.velocity) })).join("\n")}</pre></details>}
    {proposal && <ProposalReview key={`${proposal.stamp.revision}:${proposal.label}:${JSON.stringify(proposal.commands)}`} proposal={proposal} generation={generation} cancel={() => { setProposal(null); setPreview(null) }} />}
    <details><summary>{t("Saved revisions")}</summary>{project.settings.scripts?.assets.map((asset) => <Button key={`${asset.reference.id}:${asset.reference.revision}`} variant="ghost" disabled={busy} onClick={() => { setId(asset.reference.id); setKind(asset.kind); editSource(asset.source) }}>{asset.name}{" "}{message("r{revision}", { revision: plainNumber(asset.reference.revision) })}</Button>)}</details>
    <details><summary>{t("Installed assignments")}</summary>
      {project.settings.scripts?.channels.map((binding) => <div key={binding.channel} className="flex items-center gap-2"><span>{t("Channel")}{" "}{binding.channel}: {binding.script.id} {message("r{revision}", { revision: plainNumber(binding.script.revision) })}</span><Button variant="outline" disabled={busy} onClick={() => void unassign(binding.channel, null)}>{t("Review remove channel")}{" "}{binding.channel}</Button></div>)}
      {project.settings.scripts?.controllers.map((binding, index) => <div key={index} className="flex items-center gap-2"><span>{t("Controller")}{" "}{index + 1}: {binding.script.id} {message("r{revision}", { revision: plainNumber(binding.script.revision) })}{binding.feedbackChannel ? t(" · feedback enabled") : ""}</span><Button variant="outline" disabled={busy} onClick={() => void loadController(index)}>{t("Edit controller assignment")}{" "}{index + 1}</Button><Button variant="outline" disabled={busy} onClick={() => void unassign(null, index)}>{t("Review remove controller")}{" "}{index + 1}</Button></div>)}
    </details>
  </div>
}

function ControllerScriptStatus() {
  const { state } = useControllerStatus()
  return <>
    <p role="status">{state?.waiting.length ? message("{count} control targets held for pickup.", { count: plainNumber(state.waiting.length) }) : t("No controls held for pickup.")}</p>
    {state?.error && <Alert variant="destructive"><AlertDescription>{state.error}</AlertDescription></Alert>}
    {!!state?.dropped && <p role="status">{state.dropped}{" "}{t("controller messages dropped.")}</p>}
  </>
}

export function ScriptEditor() {
  const open = useAssistantWindows((state) => state.scripting)
  const epoch = useAssistantWindows((state) => state.epoch)
  return <Dialog open={open} onOpenChange={(next) => { if (!next) closeScripts() }}><DialogContent className="max-h-[85vh] overflow-y-auto"><DialogHeader><DialogTitle>{t("Script editor")}</DialogTitle><DialogDescription>{t("Bounded offline scripts. Preview and review all project changes.")}</DialogDescription></DialogHeader>{open && <ScriptForm key={epoch} />}</DialogContent></Dialog>
}
