import type {
  Automation,
  AutomationRange,
  AutomationTarget,
  EffectSlot,
  ParamInfo,
  PluginBinding,
  Project,
} from "@/bindings"
import descriptors from "@/bindings/descriptors.json"
import { DEFAULT_TRACK_PROCESSING, TRACK_PROCESSING_INFO } from "@/lib/track-processing"
import { rackParameter } from "@/features/modular/parameters"

import {
  GAIN_RANGE,
  MIX_RANGE,
  PAN_RANGE,
  paramRange,
  rangeNormalized,
  rangeValue,
  TEMPO_RANGE,
} from "./curve"
import { targetKey } from "./lanes"

/*
 * What an automation moves, looked up in a project: its range, the value
 * it has there now and the words for it. These mirror
 * `Project::automation_range` and `automation_stored_value` in the core.
 */

type Described = {
  readonly name: string
  readonly params: readonly ParamInfo[]
}

// Generated from the Rust tables that also generate the types. JSON has no
// literal types, which is all the casts add.
const EFFECTS = descriptors.effects as unknown as Record<string, Described>
const INSTRUMENTS = descriptors.instruments as unknown as Record<
  string,
  Described
>

type Source = Pick<Project, "channels" | "mixer" | "settings" | "plugins">

function pluginParameter(
  binding: PluginBinding,
  index: number
): TargetState | null {
  const parameter = binding.parameters[index]
  if (!parameter || parameter.readOnly || !parameter.automatable) return null
  return {
    range: {
      min: parameter.min,
      max: parameter.max,
      taper: parameter.stepped ? "stepped" : "linear",
    },
    stored: parameter.value,
    info: {
      id: String(parameter.id),
      name: parameter.name,
      min: parameter.min,
      max: parameter.max,
      default: parameter.value,
      kind: parameter.stepped ? "integer" : "float",
      unit: "none",
      scale: "linear",
      choices: [],
    },
  }
}

function findEffect(
  project: Source,
  effect: number
): { slot: EffectSlot; track: number } | null {
  for (const track of project.mixer.tracks) {
    const slot = track.effects.find((item) => item.id === effect)
    if (slot) return { slot, track: track.id }
  }
  return null
}

/** Reads a setting out of a parameter struct by its dotted id. */
function readSetting(params: unknown, info: ParamInfo): number | null {
  let node: unknown = params
  for (const key of info.id.split(".")) {
    if (typeof node !== "object" || node === null) return null
    node = (node as Record<string, unknown>)[key]
  }
  if (typeof node === "number") return node
  if (typeof node === "boolean") return node ? 1 : 0
  if (typeof node === "string") {
    const index = info.choices.findIndex((choice) => choice.value === node)
    return index >= 0 ? index : null
  }
  return null
}

/** What is known of a target in a project. */
export type TargetState = {
  readonly range: AutomationRange
  /** The value the target has in the project, in its own unit. */
  readonly stored: number
  /** The setting's description, for a setting of an effect or instrument. */
  readonly info: ParamInfo | null
}

/**
 * The range of a target and the value it has, or null when the project has
 * no such thing: the channel, track, send or effect is not there, the
 * channel is not an instrument, or there is no setting with that index.
 */
export function targetState(
  project: Source,
  target: AutomationTarget
): TargetState | null {
  const channel = (id: number) =>
    project.channels.find((item) => item.id === id)
  const track = (id: number) =>
    project.mixer.tracks.find((item) => item.id === id)
  switch (target.type) {
    case "channelVolume": {
      const found = channel(target.channel)
      return found
        ? { range: GAIN_RANGE, stored: found.volume, info: null }
        : null
    }
    case "channelPan": {
      const found = channel(target.channel)
      return found ? { range: PAN_RANGE, stored: found.pan, info: null } : null
    }
    case "trackVolume": {
      const found = track(target.track)
      return found
        ? { range: GAIN_RANGE, stored: found.volume, info: null }
        : null
    }
    case "trackPan": {
      const found = track(target.track)
      return found ? { range: PAN_RANGE, stored: found.pan, info: null } : null
    }
    case "trackParam": {
      const found = track(target.track)
      const info = TRACK_PROCESSING_INFO[target.param]
      const stored = found && info ? readSetting(found.processing ?? DEFAULT_TRACK_PROCESSING, info) : null
      return info && stored !== null ? { range: paramRange(info), stored, info } : null
    }
    case "sidechainGain": {
      const send = track(target.track)?.sidechains?.find(
        (item) => item.target === target.target
      )
      return send ? { range: GAIN_RANGE, stored: send.gain, info: null } : null
    }
    case "sendGain": {
      const send = track(target.track)?.sends.find(
        (item) => item.target === target.target
      )
      return send ? { range: GAIN_RANGE, stored: send.gain, info: null } : null
    }
    case "effectParam": {
      const plugin = project.plugins?.find(
        (plugin) =>
          plugin.target.type === "effect" &&
          plugin.target.effect === target.effect
      )
      if (plugin) return pluginParameter(plugin, target.param)
      const found = findEffect(project, target.effect)
      const info = found
        ? EFFECTS[found.slot.params.type]?.params[target.param]
        : undefined
      const stored = found && info ? readSetting(found.slot.params, info) : null
      return info && stored !== null
        ? { range: paramRange(info), stored, info }
        : null
    }
    case "effectMix": {
      const found = findEffect(project, target.effect)
      return found
        ? { range: MIX_RANGE, stored: found.slot.mix, info: null }
        : null
    }
    case "instrumentParam": {
      const rack = project.settings.modularRacks?.find((rack) => rack.channel === target.channel)
      if (rack) {
        if (target.param >= 32) {
          const found = rackParameter(rack.graph, target.param)
          const stored = found ? readSetting(found.module, found.info) : null
          return found && stored !== null ? { range: paramRange(found.info), stored, info: found.info } : null
        }
        const node = rack.graph.nodes.find((node) => node.processor.type === "controlInput" && node.processor.index === target.param)
        if (!node || node.processor.type !== "controlInput") return null
        return { range: MIX_RANGE, stored: node.processor.value, info: {
          id: String(node.id), name: node.name, min: 0, max: 1, default: 0,
          kind: "float", unit: "fraction", scale: "linear", choices: [],
        } }
      }
      const plugin = project.plugins?.find(
        (plugin) =>
          plugin.target.type === "instrument" &&
          plugin.target.channel === target.channel
      )
      if (plugin) return pluginParameter(plugin, target.param)
      const source = channel(target.channel)?.source
      if (source?.type !== "instrument") return null
      const info = INSTRUMENTS[source.params.type]?.params[target.param]
      const stored = info ? readSetting(source.params, info) : null
      return info && stored !== null
        ? { range: paramRange(info), stored, info }
        : null
    }
    case "tempo":
      return {
        range: TEMPO_RANGE,
        stored: project.settings.tempoBpm,
        info: null,
      }
    default: {
      const _exhaustive: never = target
      return _exhaustive
    }
  }
}

/** The target's own value for an automation value, or null without a target. */
export function targetValue(
  project: Source,
  target: AutomationTarget,
  normalized: number
): number | null {
  const state = targetState(project, target)
  return state ? rangeValue(state.range, normalized) : null
}

/** The automation value that the target's stored value is. */
export function storedNormalized(
  project: Source,
  target: AutomationTarget
): number | null {
  const state = targetState(project, target)
  return state ? rangeNormalized(state.range, state.stored) : null
}

function effectLabel(project: Source, effect: number): string {
  const plugin = project.plugins?.find(
    (plugin) =>
      plugin.target.type === "effect" && plugin.target.effect === effect
  )
  if (plugin) return plugin.name
  const found = findEffect(project, effect)
  return found
    ? (EFFECTS[found.slot.params.type]?.name ?? "Effect")
    : "Removed effect"
}

/**
 * A target in words: "Kick → volume", "Bass → send to Reverb",
 * "Reverb · Decay", "Lead · Cutoff", "Tempo".
 */
export function describeTarget(
  project: Source,
  target: AutomationTarget
): string {
  const channel = (id: number) =>
    project.channels.find((item) => item.id === id)?.name ?? "Removed channel"
  const track = (id: number) =>
    project.mixer.tracks.find((item) => item.id === id)?.name ?? "Removed track"
  switch (target.type) {
    case "channelVolume":
      return `${channel(target.channel)} → volume`
    case "channelPan":
      return `${channel(target.channel)} → pan`
    case "trackVolume":
      return `${track(target.track)} → volume`
    case "trackPan":
      return `${track(target.track)} → pan`
    case "trackParam":
      return `${track(target.track)} · ${TRACK_PROCESSING_INFO[target.param]?.name ?? "Track setting"}`
    case "sendGain":
      return `${track(target.track)} → send to ${track(target.target)}`
    case "sidechainGain":
      return `${track(target.track)} → sidechain to ${track(target.target)}`
    case "effectParam": {
      const info = targetState(project, target)?.info
      return `${effectLabel(project, target.effect)} · ${info?.name ?? "setting"}`
    }
    case "effectMix":
      return `${effectLabel(project, target.effect)} · Mix`
    case "instrumentParam": {
      const info = targetState(project, target)?.info
      return `${channel(target.channel)} · ${info?.name ?? "setting"}`
    }
    case "tempo":
      return "Tempo"
    default: {
      const _exhaustive: never = target
      return _exhaustive
    }
  }
}

/** The automations of a project that move a target, in project order. */
export function automationsOf(
  automations: readonly Automation[],
  target: AutomationTarget
): Automation[] {
  const key = targetKey(target)
  return automations.filter((item) => targetKey(item.target) === key)
}

/**
 * Why a target cannot be automated, or null when it can. The engine leaves
 * latency-changing controls alone: limiter look-ahead and matrix delays. The
 * delays that line the other paths up with it are made for the stored
 * value.
 */
export function automationBlocked(
  project: Source,
  target: AutomationTarget
): string | null {
  if (target.type !== "effectParam") return null
  const found = findEffect(project, target.effect)
  const info = targetState(project, target)?.info
  if (found?.slot.params.type === "limiter" && info?.id === "lookaheadMs") {
    return "Changes the latency"
  }
  if (
    found?.slot.params.type === "stereoMatrix" &&
    (info?.id === "leftDelayMs" || info?.id === "rightDelayMs")
  ) {
    return "Changes the latency"
  }
  return null
}
