import { useState } from "react"
import type { ControllerLink, LinkTarget, SourceMode, PickupMode } from "@/bindings"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { describeTarget } from "@/lib/automation/targets"
import { backend, errorMessage } from "@/lib/ipc"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { sameShape } from "./menu"
import { useControllerStatus } from "./status"

export function ControllerChoice<T extends string>({ label, value, choices, change, disabled = false }: {
  label: string; value: T; choices: { value: T; label: string }[]; change(value: T): void; disabled?: boolean
}) {
  return <Field><FieldLabel>{label}</FieldLabel><Select items={choices} value={value} disabled={disabled} onValueChange={(value) => { if (value !== null) change(value) }}>
    <SelectTrigger aria-label={label}><SelectValue /></SelectTrigger>
    <SelectContent alignItemWithTrigger={false}><SelectGroup>{choices.map((choice) => <SelectItem key={choice.value} value={choice.value}>{choice.label}</SelectItem>)}</SelectGroup></SelectContent>
  </Select></Field>
}

type Draft = { link: ControllerLink; links: ControllerLink[]; index: number; global: boolean; revision: number; generation: number }
export function ControllerLinksPanel() {
  const { state, setState } = useControllerStatus()
  const project = useProjectStore((state) => state.project)
  const revision = useProjectStore((state) => state.revision)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [globalTarget, setGlobalTarget] = useState("master")
  const links = project.settings.controllerLinks ?? []
  const globals = state?.settings.links ?? []
  const stale = draft !== null && (draft.generation !== getProjectGeneration() || draft.revision !== revision)
  const targetName = (target: LinkTarget) => target.type === "param" ? describeTarget(project, target.target) :
    target.type === "action" ? target.action : JSON.stringify(target.slot)

  function capture(index: number, global: boolean) {
    const list = global ? globals : links
    setDraft({ link: structuredClone(list[index]!), links: structuredClone(list), index, global, revision, generation: getProjectGeneration() })
    setError(null)
  }
  function map(change: Partial<ControllerLink["map"]>) {
    setDraft((draft) => draft && ({ ...draft, link: { ...draft.link, map: { ...draft.link.map, ...change } } }))
  }
  async function apply(remove = false) {
    if (!draft || stale || busy) return
    setBusy(true)
    setError(null)
    try {
      const current = await backend.controllerState()
      if (draft.generation !== getProjectGeneration() || draft.revision !== useProjectStore.getState().revision ||
        !sameShape(draft.links, draft.global ? current.settings.links : useProjectStore.getState().project.settings.controllerLinks ?? []))
        throw new Error("The links changed. Reopen the link before applying.")
      const links = draft.links.flatMap((link, index) => index === draft.index ? remove ? [] : [draft.link] : [link])
      if (draft.global) setState(await backend.controllerSettingsSet({ ...current.settings, links }))
      else await dispatch({ type: "setControllerLinks", links })
      setDraft(null)
    } catch (failure) { setError(errorMessage(failure)) } finally { setBusy(false) }
  }
  async function learnGlobal() {
    if (!state || busy) return
    setBusy(true)
    setError(null)
    try {
      const target: LinkTarget = globalTarget === "master" ? { type: "slot", slot: { type: "masterVolume" } } :
        globalTarget === "tempo" ? { type: "slot", slot: { type: "tempo" } } :
        globalTarget === "play" ? { type: "action", action: "togglePlay" } :
        { type: "slot", slot: { type: "mixerVolume", index: Number(globalTarget.slice(6)) } }
      setState(await backend.controllerLearn([target], true, false, state.generation))
    } catch (failure) { setError(errorMessage(failure)) } finally { setBusy(false) }
  }
  return <FieldSet><FieldLegend>Controller links</FieldLegend>
    <div role="list" aria-label="Project controller links">{links.map((link, index) => <div role="listitem" key={index}>
      <span>{targetName(link.target)}</span>{" "}
      <Button size="sm" variant="outline" onClick={() => capture(index, false)} aria-label={`Edit project link ${index + 1}`}>Edit link</Button>
    </div>)}{links.length === 0 && <p>No project links.</p>}</div>
    <div role="list" aria-label="Global controller links">{globals.map((link, index) => <div role="listitem" key={index}>
      <span>{targetName(link.target)}</span>{" "}
      <Button size="sm" variant="outline" onClick={() => capture(index, true)} aria-label={`Edit global link ${index + 1}`}>Edit global link</Button>
    </div>)}{globals.length === 0 && <p>No global links.</p>}</div>
    <ControllerChoice label="Global link target" value={globalTarget} change={setGlobalTarget} choices={[
      { value: "master", label: "Master volume" }, { value: "tempo", label: "Tempo" }, { value: "play", label: "Play / stop" },
      ...Array.from({ length: 8 }, (_, index) => ({ value: `mixer:${index}`, label: `Mixer strip ${index + 1} volume` })),
    ]} />
    <Button variant="outline" disabled={busy || backend.kind !== "tauri"} onClick={() => void learnGlobal()}>Learn global link</Button>
    <Button variant="outline" disabled={busy || backend.kind !== "tauri"} onClick={() => void backend.controllerLearn([], false, true, state?.generation ?? 0).then(setState).catch((failure: unknown) => setError(errorMessage(failure)))}>Multi-link controllers</Button>
    {state?.learn && <p role="status">Move hardware controls in order: {state.learn.linked} of {state.learn.targets.length} linked.
      <Button size="sm" variant="outline" onClick={() => void backend.controllerLearnCancel().then(setState)}>Stop linking</Button></p>}
    {draft && <FieldGroup>
      <p>{targetName(draft.link.target)} · {JSON.stringify(draft.link.source)}</p>
      {(["min", "max", "smoothingMs"] as const).map((key) => <Field key={key}><FieldLabel htmlFor={`controller-${key}`}>{key === "min" ? "Range start" : key === "max" ? "Range end" : "Smoothing (ms)"}</FieldLabel>
        <Input id={`controller-${key}`} type="number" min={0} max={key === "smoothingMs" ? 5000 : 1} step={key === "smoothingMs" ? 1 : 0.01} value={draft.link.map[key]} onChange={(event) => map({ [key]: Number(event.target.value) })} /></Field>)}
      <Field><FieldLabel htmlFor="controller-formula">Mapping formula</FieldLabel><Input id="controller-formula" value={draft.link.map.formula ?? ""} placeholder="x" onChange={(event) => map({ formula: event.target.value || undefined })} /></Field>
      <ControllerChoice<SourceMode> label="Controller mode" value={draft.link.map.mode} change={(mode) => map({ mode })} choices={[
        { value: "absolute", label: "Absolute" }, { value: "relativeTwosComplement", label: "Relative: two's complement" },
        { value: "relativeSignedBit", label: "Relative: sign bit" }, { value: "relativeBinaryOffset", label: "Relative: offset" }, { value: "toggle", label: "Toggle" },
      ]} />
      <ControllerChoice<PickupMode> label="Link pickup" value={draft.link.map.pickup} change={(pickup) => map({ pickup })} choices={[
        { value: "inherit", label: "Use global pickup" }, { value: "on", label: "On" }, { value: "off", label: "Off" },
      ]} />
      {stale && <p role="status">The project changed. Reopen this link to edit it.</p>}
      <div className="flex gap-2"><Button disabled={stale || busy} onClick={() => void apply()}>Apply controller link</Button>
        <Button variant="destructive" disabled={stale || busy} onClick={() => void apply(true)}>Remove link</Button>
        <Button variant="outline" onClick={() => setDraft(null)}>Cancel</Button></div>
    </FieldGroup>}
    <div role="status" aria-label="Controller pickup state">{state?.waiting.length ?
      `Pickup held: ${state.waiting.map((target) => describeTarget(project, target)).join(", ")}` : "No controls held for pickup."}</div>
    {(error || (backend.kind === "tauri" && state?.error)) && <Alert variant="destructive"><AlertDescription>{error || state?.error}</AlertDescription></Alert>}
  </FieldSet>
}
