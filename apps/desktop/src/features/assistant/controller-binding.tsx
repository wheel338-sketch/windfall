import type { AutomationTarget, ControlMap, ControlSource, LinkTarget, MidiControl, PickupMode, Project, SourceMode } from "@/bindings"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ControllerChoice, globalChoices } from "@/features/controllers/links-panel"
import { effectDescriptor, instrumentDescriptor } from "@/features/params/descriptors"
import { automationBlocked, describeTarget, targetState } from "@/lib/automation/targets"
import { scriptParameterTargets } from "@/lib/script-parameters"
import { TRACK_PROCESSING_INFO } from "@/lib/track-processing"

export function scriptTargetChoices(project: Project): { value: string; label: string; target: LinkTarget | null }[] {
  const targets: AutomationTarget[] = [...scriptParameterTargets(project)]
  for (const channel of project.channels) {
    targets.push({ type: "channelVolume", channel: channel.id }, { type: "channelPan", channel: channel.id })
    const plugin = project.plugins?.find(p => p.target.type === "instrument" && p.target.channel === channel.id)
    const params = plugin?.parameters ?? (channel.source.type === "instrument" ? instrumentDescriptor(channel.source.params.type).params : [])
    params.forEach((_, param) => targets.push({ type: "instrumentParam", channel: channel.id, param }))
    project.settings.modularRacks?.find(r => r.channel === channel.id)?.graph.nodes.forEach(node => {
      if (node.processor.type === "controlInput") targets.push({ type: "instrumentParam", channel: channel.id, param: node.processor.index })
    })
  }
  for (const track of project.mixer.tracks) {
    targets.push({ type: "trackVolume", track: track.id }, { type: "trackPan", track: track.id })
    TRACK_PROCESSING_INFO.forEach((_, param) => targets.push({ type: "trackParam", track: track.id, param }))
    track.sends.forEach(send => targets.push({ type: "sendGain", track: track.id, target: send.target }))
    track.sidechains?.forEach(send => targets.push({ type: "sidechainGain", track: track.id, target: send.target }))
    for (const effect of track.effects) {
      targets.push({ type: "effectMix", track: track.id, effect: effect.id })
      const plugin = project.plugins?.find(p => p.target.type === "effect" && p.target.effect === effect.id)
      const params = plugin?.parameters ?? effectDescriptor(effect.params.type).params
      params.forEach((_, param) => targets.push({ type: "effectParam", track: track.id, effect: effect.id, param }))
    }
  }
  const choices = targets.filter(target => targetState(project, target) && !automationBlocked(project, target))
    .map(target => ({ value: JSON.stringify({ type: "param", target }), label: describeTarget(project, target), target: { type: "param" as const, target } }))
  const labelCounts = new Map<string, number>()
  for (const choice of choices) labelCounts.set(choice.label, (labelCounts.get(choice.label) ?? 0) + 1)
  for (const choice of choices) {
    if ((labelCounts.get(choice.label) ?? 0) > 1) {
      const target = choice.target.target
      const scope = "channel" in target ? "Channel" : "Mixer"
      choice.label = `${scope} · ${choice.label}`
    }
  }
  return [{ value: "none", label: "No mapped target (tempo / feedback only)", target: null },
    ...globalChoices.map(choice => ({ ...choice, value: JSON.stringify(choice.target) })),
    ...choices.filter((choice, index) => choices.findIndex(other => other.value === choice.value) === index)]
}

export function ControllerBindingFields({ project, source, target, map, feedbackChannel, disabled, change }: {
  project: Project; source: ControlSource; target: LinkTarget | null; map: ControlMap; feedbackChannel: number
  disabled: boolean; change(patch: { source?: ControlSource; target?: LinkTarget | null; map?: ControlMap; feedbackChannel?: number }): void
}) {
  if (source.type !== "midi") return null
  const control = source.control
  const choices = scriptTargetChoices(project)
  return <>
    <Field><FieldLabel htmlFor="script-device">MIDI device name (blank means any enabled input)</FieldLabel>
      <Input id="script-device" maxLength={256} value={source.device ?? ""} disabled={disabled}
        onChange={e => change({ source: { ...source, device: e.target.value || undefined } })} /></Field>
    <Field><FieldLabel htmlFor="script-input-channel">Input MIDI channel (0 means all)</FieldLabel>
      <Input id="script-input-channel" type="number" min={0} max={16} value={source.channel ?? 0} disabled={disabled}
        onChange={e => change({ source: { ...source, channel: e.target.valueAsNumber || undefined } })} /></Field>
    <ControllerChoice<MidiControl["type"]> label="Script input type" value={control.type} disabled={disabled} choices={[
      { value: "cc", label: "Control change" }, { value: "note", label: "Note / pad" },
      { value: "pitchBend", label: "Pitch wheel (14 bit)" }, { value: "channelPressure", label: "Channel pressure" },
    ]} change={type => change({ source: { ...source, control: type === "cc" ? { type, number: 7 } : type === "note" ? { type, key: 60 } : { type } } })} />
    {(control.type === "cc" || control.type === "note") && <Field>
      <FieldLabel htmlFor="script-cc">{control.type === "cc" && source.channel === 1 ? "Input CC on MIDI channel 1" : "Controller number / note key"}</FieldLabel>
      <Input id="script-cc" type="number" min={0} max={127} value={control.type === "cc" ? control.number : control.key} disabled={disabled}
        onChange={e => change({ source: { ...source, control: control.type === "cc" ? { type: "cc", number: e.target.valueAsNumber } : { type: "note", key: e.target.valueAsNumber } } })} /></Field>}
    <ControllerChoice<string> label="Script mapped target" value={target ? JSON.stringify(target) : "none"} choices={choices} disabled={disabled}
      change={value => change({ target: choices.find(choice => choice.value === value)?.target ?? null })} />
    <ControllerChoice<PickupMode> label="Script pickup" value={map.pickup} disabled={disabled} choices={[
      { value: "off", label: "Immediate takeover" }, { value: "on", label: "Soft takeover" }, { value: "inherit", label: "Use global pickup" },
    ]} change={pickup => change({ map: { ...map, pickup } })} />
    <ControllerChoice<SourceMode> label="Script controller mode" value={map.mode} disabled={disabled} choices={[
      { value: "absolute", label: "Absolute" }, { value: "toggle", label: "Toggle" },
      { value: "relativeTwosComplement", label: "Relative two's complement" }, { value: "relativeSignedBit", label: "Relative sign bit" },
      { value: "relativeBinaryOffset", label: "Relative offset" },
    ]} change={mode => change({ map: { ...map, mode } })} />
    <Field><FieldLabel htmlFor="script-smoothing">Script smoothing (ms)</FieldLabel>
      <Input id="script-smoothing" type="number" min={0} max={5000} value={map.smoothingMs} disabled={disabled}
        onChange={e => change({ map: { ...map, smoothingMs: e.target.valueAsNumber } })} /></Field>
    <Field><FieldLabel htmlFor="script-feedback-channel">Feedback MIDI channel</FieldLabel>
      <Input id="script-feedback-channel" type="number" min={1} max={16} value={feedbackChannel} disabled={disabled}
        onChange={e => change({ feedbackChannel: e.target.valueAsNumber })} /></Field>
  </>
}
