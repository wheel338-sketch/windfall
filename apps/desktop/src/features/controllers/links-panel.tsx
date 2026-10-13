import { useRef, useState } from "react"
import type { ControllerLink, LinkTarget, SourceMode, PickupMode, ControlAction, SlotTarget } from "@/bindings"
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
import { message, plainNumber, translatedText as t } from "@/lib/i18n"

export function ControllerChoice<T extends string>({ label, value, choices, change, disabled = false }: {
  label: string; value: T; choices: { value: T; label: string }[]; change(value: T): void; disabled?: boolean
}) {
  const typingIntent = useRef(false)
  // A queued hidden-input change can outlive the item that was removed.
  // Keep explicit item/key navigation; these routing choices do not autofill.
  return <Field onKeyDownCapture={(event) => { if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) typingIntent.current = true }}><FieldLabel>{label}</FieldLabel><Select items={choices} value={value} disabled={disabled} autoComplete="off" onValueChange={(value, details) => {
    if (value === null) return
    if (details.reason === "none" && (details.event.type === "change" || details.event.type === "input" || !typingIntent.current)) return
    typingIntent.current = false
    change(value)
  }}>
    <SelectTrigger aria-label={label}><SelectValue /></SelectTrigger>
    <SelectContent alignItemWithTrigger={false}><SelectGroup>{choices.map((choice) => <SelectItem key={choice.value} value={choice.value}>{choice.label}</SelectItem>)}</SelectGroup></SelectContent>
  </Select></Field>
}

const actionLabels: Record<Extract<ControlAction, string>, string> = {
  get play() { return t("Play") }, get stop() { return t("Stop") }, get togglePlay() { return t("Play / stop") }, get toggleLoop() { return t("Toggle song loop") }, get toggleMetronome() { return t("Toggle metronome") },
  get toggleNoteRecord() { return t("Toggle MIDI recording") }, get toggleAutomationRecord() { return t("Toggle automation recording") }, get undo() { return t("Undo") }, get redo() { return t("Redo") },
  get patternPrevious() { return t("Previous pattern") }, get patternNext() { return t("Next pattern") }, get bankPrevious() { return t("Previous controller bank") }, get bankNext() { return t("Next controller bank") }, get panicMidi() { return t("Release MIDI notes") }, get performanceStopAll() { return t("Stop all performance tracks") },
}
export function controlActionLabel(action: ControlAction): string {
  if (typeof action === "string") return actionLabels[action]
  if ("performancePad" in action) return message("Launch track {track}, scene {scene}", { track: plainNumber(action.performancePad.track), scene: plainNumber(action.performancePad.scene + 1) })
  return message("Stop performance track {track}", { track: plainNumber(action.performanceStop.track) })
}
export const globalChoices: { value: string; label: string; target: LinkTarget }[] = [
  { value: "master", get label() { return t("Master volume") }, target: { type: "slot", slot: { type: "masterVolume" } } },
  { value: "tempo", get label() { return t("Tempo") }, target: { type: "slot", slot: { type: "tempo" } } },
  ...Object.entries(actionLabels).map(([action, label]) => ({ value: `action:${action}`, label, target: { type: "action", action: action as ControlAction } as LinkTarget })),
  ...(["mixerVolume", "mixerPan", "channelVolume", "channelPan"] as const).flatMap((type) => Array.from({ length: 8 }, (_, index) => ({
    value: `${type}:${index}`, get label() { return slotName({ type, index }) }, target: { type: "slot", slot: { type, index } } as LinkTarget,
  }))),
]
function slotName(slot: SlotTarget): string {
  if (slot.type === "masterVolume") return t("Master volume")
  if (slot.type === "tempo") return t("Tempo")
  const index = plainNumber(slot.index + 1)
  return slot.type === "mixerVolume" ? message("Mixer strip {index} volume", { index })
    : slot.type === "mixerPan" ? message("Mixer strip {index} pan", { index })
    : slot.type === "channelVolume" ? message("Channel {index} volume", { index })
    : message("Channel {index} pan", { index })
}
function sourceName(source: ControllerLink["source"]): string {
  if (source.type !== "midi") return t("Internal controller")
  const control = source.control.type === "cc" ? `CC ${source.control.number}` : source.control.type === "note" ? message("Key {key}", { key: plainNumber(source.control.key) }) : source.control.type === "pitchBend" ? t("Pitch wheel") : t("Pressure")
  return message("{device} · {channel} · {control}", { device: source.device ?? t("Any MIDI device"), channel: source.channel ? message("Channel {channel}", { channel: plainNumber(source.channel) }) : t("All channels"), control })
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
    target.type === "action" ? controlActionLabel(target.action) : slotName(target.slot)

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
        throw new Error(t("The links changed. Reopen the link before applying."))
      const links = draft.links.flatMap((link, index) => index === draft.index ? remove ? [] : [draft.link] : [link])
      if (draft.global) setState(await backend.controllerSettingsSet({ ...current.settings, links }))
      else if (!await dispatch({ type: "setControllerLinks", links })) throw new Error(t("Controller link was refused. Check the mapping and try again."))
      setDraft(null)
    } catch (failure) { setError(errorMessage(failure)) } finally { setBusy(false) }
  }
  async function learnGlobal() {
    if (!state || busy) return
    setBusy(true)
    setError(null)
    try {
      const target = globalChoices.find((choice) => choice.value === globalTarget)!.target
      setState(await backend.controllerLearn([target], true, false, state.generation))
    } catch (failure) { setError(errorMessage(failure)) } finally { setBusy(false) }
  }
  return <FieldSet><FieldLegend>{t("Controller links")}</FieldLegend>
    <div role="list" aria-label={t("Project controller links")}>{links.map((link, index) => <div role="listitem" key={index}>
      <span>{targetName(link.target)}</span>{" "}
      <Button size="sm" variant="outline" onClick={() => capture(index, false)} aria-label={message("Edit project link {index}", { index: plainNumber(index + 1) })}>{t("Edit link")}</Button>
    </div>)}{links.length === 0 && <p>{t("No project links.")}</p>}</div>
    <div role="list" aria-label={t("Global controller links")}>{globals.map((link, index) => <div role="listitem" key={index}>
      <span>{targetName(link.target)}</span>{" "}
      <Button size="sm" variant="outline" onClick={() => capture(index, true)} aria-label={message("Edit global link {index}", { index: plainNumber(index + 1) })}>{t("Edit global link")}</Button>
    </div>)}{globals.length === 0 && <p>{t("No global links.")}</p>}</div>
    <ControllerChoice<string> label={t("Global link target")} value={globalTarget} change={setGlobalTarget} choices={globalChoices} />
    <Button variant="outline" disabled={busy || backend.kind !== "tauri"} onClick={() => void learnGlobal()}>{t("Learn global link")}</Button>
    <Button variant="outline" disabled={busy || backend.kind !== "tauri"} onClick={() => void backend.controllerLearn([], false, true, state?.generation ?? 0).then(setState).catch((failure: unknown) => setError(errorMessage(failure)))}>{t("Multi-link controllers")}</Button>
    {state?.learn && <p role="status">{t("Move hardware controls in order:")}{" "}{state.learn.linked}{" "}{t("of")}{" "}{state.learn.targets.length}{" "}{t("linked.")}
      <Button size="sm" variant="outline" onClick={() => void backend.controllerLearnCancel().then(setState).catch((failure: unknown) => setError(errorMessage(failure)))}>{t("Stop linking")}</Button></p>}
    {draft && <FieldGroup>
      <p>{targetName(draft.link.target)} · {sourceName(draft.link.source)}</p>
      {(["min", "max", "smoothingMs"] as const).map((key) => <Field key={key}><FieldLabel htmlFor={`controller-${key}`}>{key === "min" ? t("Range start") : key === "max" ? t("Range end") : t("Smoothing (ms)")}</FieldLabel>
        <Input id={`controller-${key}`} type="number" min={0} max={key === "smoothingMs" ? 5000 : 1} step={key === "smoothingMs" ? 1 : 0.01} value={draft.link.map[key]} onChange={(event) => map({ [key]: Number(event.target.value) })} /></Field>)}
      <Field><FieldLabel htmlFor="controller-formula">{t("Mapping formula")}</FieldLabel><Input id="controller-formula" value={draft.link.map.formula ?? ""} placeholder="x" onChange={(event) => map({ formula: event.target.value || undefined })} /></Field>
      <ControllerChoice<SourceMode> label={t("Controller mode")} value={draft.link.map.mode} change={(mode) => map({ mode })} choices={[
        { value: "absolute", label: t("Absolute") }, { value: "relativeTwosComplement", label: t("Relative: two's complement") },
        { value: "relativeSignedBit", label: t("Relative: sign bit") }, { value: "relativeBinaryOffset", label: t("Relative: offset") }, { value: "toggle", label: t("Toggle") },
      ]} />
      <ControllerChoice<PickupMode> label={t("Link pickup")} value={draft.link.map.pickup} change={(pickup) => map({ pickup })} choices={[
        { value: "inherit", label: t("Use global pickup") }, { value: "on", label: t("On") }, { value: "off", label: t("Off") },
      ]} />
      {stale && <p role="status">{t("The project changed. Reopen this link to edit it.")}</p>}
      <div className="flex gap-2"><Button disabled={stale || busy} onClick={() => void apply()}>{t("Apply controller link")}</Button>
        <Button variant="destructive" disabled={stale || busy} onClick={() => void apply(true)}>{t("Remove link")}</Button>
        <Button variant="outline" onClick={() => setDraft(null)}>{t("Cancel")}</Button></div>
    </FieldGroup>}
    <div role="status" aria-label={t("Controller pickup state")}>{state?.waiting.length ?
      message("Pickup held: {map}", { map: state.waiting.map((target) => describeTarget(project, target)).join(", ") }) : t("No controls held for pickup.")}</div>
    {(error || (backend.kind === "tauri" && state?.error)) && <Alert variant="destructive"><AlertDescription>{error || state?.error}</AlertDescription></Alert>}
  </FieldSet>
}
