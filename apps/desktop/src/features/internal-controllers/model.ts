import type { AutomationTarget, ControlMap, EffectKind, InternalControllerSettings, Project } from "@/bindings"
import { dispatch, useProjectStore } from "@/lib/store/project"
import { message, plainNumber, translatedText } from "@/lib/i18n"

const PORTS: Partial<Record<EffectKind, readonly string[]>> = {
  noteEnvelope: ["Envelope"], keyboardSource: ["Pitch", "Velocity"],
  envelopeFollower: ["Peak + LFO", "LFO"], formulaSource: ["Result"],
  xyPad: ["X", "Y"], xyzPad: ["X", "Y", "Z"],
  controlSurface: Array.from({ length: 8 }, (_, i) => `Control ${i + 1}`),
  voltageSource: Array.from({ length: 8 }, (_, i) => `Output ${i + 1}`),
}
export function internalSources(project: Project) {
  const configs = project.settings.internalControllers ?? []
  const result: { id: number; name: string; ports: readonly string[] }[] = []
  for (const track of project.mixer.tracks) for (const effect of track.effects) {
    const ports = PORTS[effect.params.type]
    if (!ports) continue
    const config = configs.find((c) => c.controller === effect.id)
    result.push({ id: effect.id, name: `${track.name || "Master"} / ${effect.params.type}`, ports: ports.map((name, output) => config?.controls?.find((c) => c.output === output)?.name ?? name) })
  }
  for (const channel of project.channels) {
    if (channel.source.type === "instrument" && channel.source.params.type === "dashboard") {
      const config = configs.find((c) => c.controller === channel.id)
      result.push({ id: channel.id, name: channel.name || "Dashboard", ports: Array.from({length: 8}, (_, output) => config?.controls?.find((c) => c.output === output)?.name ?? `Control ${output + 1}`) })
    }
  }
  return result
}
export const INTERNAL_MAP: ControlMap = { min: 0, max: 1, smoothingMs: 5, pickup: "off", mode: "absolute" }
export function addInternalLink(controller: number, output: number, target: AutomationTarget) {
  const links = useProjectStore.getState().project.settings.controllerLinks ?? []
  return dispatch({ type: "setControllerLinks", links: [...links, { source: { type: "internal", controller, output }, target: { type: "param", target }, map: { ...INTERNAL_MAP } }] })
}
export function useControllerConfig(controller: number) {
  const settings = useProjectStore((s) => s.project.settings.internalControllers)
  const config: InternalControllerSettings = settings?.find((c) => c.controller === controller) ?? { controller, durationMs: 1000, rms: false }
  function save(patch: Partial<InternalControllerSettings>, gesture?: number) {
    const all = useProjectStore.getState().project.settings.internalControllers ?? []
    const previous = all.find((c) => c.controller === controller) ?? config
    return dispatch({ type: "setInternalControllerSettings", settings: [...all.filter((c) => c.controller !== controller), { ...previous, ...patch, controller }] }, gesture)
  }
  return { config, save }
}
export function targetTitle(project: Project, target: AutomationTarget): string {
  if ("channel" in target) {
    const name = project.channels.find((c) => c.id === target.channel)?.name ?? "Channel"
    return `${name} · ${target.type === "instrumentParam" ? message("parameter {param}", { param: plainNumber(target.param + 1) }) : target.type === "channelVolume" ? "Volume" : "Pan"}`
  }
  if ("track" in target) {
    const name = project.mixer.tracks.find((t) => t.id === target.track)?.name || translatedText("Master")
    return `${name} · ${target.type === "effectParam" ? message("effect parameter {param}", { param: plainNumber(target.param + 1) }) : target.type === "trackVolume" ? "Volume" : target.type === "trackPan" ? "Pan" : target.type}`
  }
  return translatedText("Tempo")
}
