import type { DragEvent } from "react"
import type { EffectKind, InstrumentKind, PluginRecord } from "@/bindings"
import { EFFECT_KINDS, INSTRUMENT_KINDS, instrumentDescriptor } from "@/features/params/descriptors"
import { backend } from "@/lib/ipc"
import { dispatch, receivePatch, useProjectStore } from "@/lib/store/project"
import { getProjectGeneration } from "@/lib/store/replaced"
import { useUiStore } from "@/lib/store/ui"
import { selectChannel } from "@/features/channel-rack/channel-ops"
import { openEffect } from "@/features/mixer/effect-ops"
import { reportError } from "@/lib/errors"
import { translatedText as t } from "@/lib/i18n"

const MIME = "application/x-windfall-plugin"
type Destination = { channel?: number; track?: number; effect?: number; index?: number }
export function startPluginDrag(event: DragEvent, record: PluginRecord) {
  event.dataTransfer.effectAllowed = "copy"
  event.dataTransfer.setData(MIME, JSON.stringify({ format: record.format, id: record.id, generation: getProjectGeneration(), revision: useProjectStore.getState().revision }))
  event.dataTransfer.setData("text/plain", record.name)
}
export async function insertPlugin(record: PluginRecord, destination: Destination = {}) {
  if (record.status !== "builtin" && record.status !== "verified") throw new Error(record.detail ?? t("This plugin is unavailable"))
  let result
  const project = useProjectStore.getState().project
  if (destination.channel !== undefined && !project.channels.some((channel) => channel.id === destination.channel)) throw new Error(t("The destination channel was removed"))
  if (destination.track !== undefined && !project.mixer.tracks.some((track) => track.id === destination.track)) throw new Error(t("The destination mixer track was removed"))
  if (destination.effect !== undefined && !project.mixer.tracks.some((track) => (destination.track === undefined || track.id === destination.track) && track.effects.some((effect) => effect.id === destination.effect))) throw new Error(t("The destination effect slot was removed"))
  if (record.format === "builtin") {
    if (record.instrument) {
      if (!INSTRUMENT_KINDS.includes(record.id as InstrumentKind)) throw new Error(t("Unknown built-in instrument"))
      const kind = record.id as InstrumentKind
      const channel = project.channels.find((c) => c.id === destination.channel)
      result = channel ? await dispatch({ type: "applyChannelPreset", id: channel.id, expected: channel, preset: { name: record.name, color: channel.color, volume: channel.volume, pan: channel.pan, source: { type: "instrument", params: instrumentDescriptor(kind).defaults } }, nameColor: false }) : await dispatch({ type: "addChannel", instrument: kind })
    } else {
      if (!EFFECT_KINDS.includes(record.id as EffectKind)) throw new Error(t("Unknown built-in effect"))
      const track = destination.track ?? useUiStore.getState().selectedTrack ?? 0
      result = destination.effect === undefined ? await dispatch({ type: "addEffect", track, kind: record.id as EffectKind, index: destination.index }) : await dispatch({ type: "replaceEffect", track, effect: destination.effect, kind: record.id as EffectKind })
    }
  } else {
    const generation = getProjectGeneration()
    const revision = useProjectStore.getState().revision
    const state = await backend.timelineState()
    if (generation !== getProjectGeneration() || revision !== useProjectStore.getState().revision) throw new Error(t("The project changed during plugin insertion"))
    const target = destination.channel !== undefined ? { type: "instrument" as const, channel: destination.channel } : destination.effect !== undefined ? { type: "effect" as const, effect: destination.effect } : null
    result = await backend.pluginsInsert(record.path, record.id, target, record.instrument ? null : (destination.track ?? useUiStore.getState().selectedTrack ?? 0), state.generation, state.revision)
    if (generation !== getProjectGeneration()) return
    receivePatch(result.patch)
  }
  if (!result) return
  if (record.instrument) { selectChannel(destination.channel ?? result.created[0], { openSettings: true }); useUiStore.getState().showCenterTab("channelRack") }
  else openEffect(destination.effect ?? result.created[0])
}
export function pluginDropHandlers(destination: Destination) {
  return {
    onDragOver: (event: DragEvent) => { if (event.dataTransfer.types.includes(MIME)) { event.preventDefault(); event.stopPropagation(); event.dataTransfer.dropEffect = "copy" } },
    onDrop: (event: DragEvent) => {
      const text = event.dataTransfer.getData(MIME); if (!text) return
      event.preventDefault(); event.stopPropagation()
      void (async () => {
        const value = JSON.parse(text) as { format: string; id: string; generation: number; revision: number }
        if (value.generation !== getProjectGeneration() || value.revision !== useProjectStore.getState().revision) throw new Error(t("The project changed during the plugin drag"))
        const catalog = await backend.pluginDatabase()
        if (value.generation !== getProjectGeneration() || value.revision !== useProjectStore.getState().revision) throw new Error(t("The project changed during the plugin drag"))
        const record = catalog.records.find((r) => r.format === value.format && r.id === value.id)
        if (!record) throw new Error(t("This plugin is no longer in the catalog"))
        if ((destination.channel !== undefined) !== record.instrument && destination.channel !== undefined) throw new Error(t("Drop an instrument onto a channel"))
        if (destination.track !== undefined && record.instrument) throw new Error(t("Drop an effect onto a mixer slot"))
        await insertPlugin(record, destination)
      })().catch((error: unknown) => reportError(error, t("Could not add plugin")))
    },
  }
}
